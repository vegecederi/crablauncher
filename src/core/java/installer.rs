use crate::core::java::providers::mojang::{MojangRuntimeFileEntry, MojangRuntimeFileManifest};
use crate::core::java::providers::{AvailableJavaBuild, JavaProviderKind};
use crate::core::java::runtime::{probe_java_executable, JavaRuntime};
use crate::error::{LauncherError, Result};
use crate::platform::paths::AppPaths;
use crate::services::download::{DownloadManager, DownloadRequest};
use flate2::read::GzDecoder;
use futures_util::stream::{self, StreamExt};
use reqwest::Client;
use std::fs::File;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tar::Archive;
use tokio_util::sync::CancellationToken;
use tracing::info;

#[derive(Debug, Clone)]
pub struct JavaInstallProgress {
    pub step: u32,
    pub total_steps: u32,
    pub stage: String,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub completed_files: usize,
    pub total_files: usize,
    pub bytes_per_sec: u64,
    pub indeterminate: bool,
}

/// Instala qualquer build normalizada de um provider oficial em `runtimes/<provider>/<id>/`.
pub async fn install_java_build(
    build: &AvailableJavaBuild,
    paths: &AppPaths,
    client: &Client,
    download_manager: &DownloadManager,
    cancel: &CancellationToken,
) -> Result<JavaRuntime> {
    install_java_build_with_progress(build, paths, client, download_manager, cancel, |_| {}).await
}

/// Instala qualquer build normalizada de Java reportando progresso real de download e extração.
pub async fn install_java_build_with_progress<F>(
    build: &AvailableJavaBuild,
    paths: &AppPaths,
    client: &Client,
    download_manager: &DownloadManager,
    cancel: &CancellationToken,
    on_progress: F,
) -> Result<JavaRuntime>
where
    F: Fn(JavaInstallProgress) + Send + Sync + 'static,
{
    let on_progress: Arc<dyn Fn(JavaInstallProgress) + Send + Sync> = Arc::new(on_progress);
    if build.provider == JavaProviderKind::Mojang {
        return install_mojang_runtime(build, paths, client, download_manager, cancel, on_progress)
            .await;
    }

    let safe_ver = build
        .full_version
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>();
    let dir_slug = format!(
        "{}-{}-{}-{}",
        build.package_type.label().to_lowercase(),
        safe_ver,
        build.os,
        build.arch
    );
    let target_dir = paths
        .runtimes_dir()
        .join(build.provider.dir_name())
        .join(&dir_slug);

    if let Some(existing) = find_java_bin(&target_dir) {
        if let Ok(rt) = probe_java_executable(&existing, true).await {
            info!("Reusing managed runtime at {}", existing.display());
            return Ok(rt);
        }
    }

    let is_zip = build.download_url.ends_with(".zip");
    let ext = if is_zip { "zip" } else { "tar.gz" };
    let archive_path = paths.cache_dir.join(format!(
        "{}-{}.{}",
        build.provider.dir_name(),
        dir_slug,
        ext
    ));

    let req = DownloadRequest {
        url: build.download_url.clone(),
        destination: archive_path.clone(),
        expected_sha1: None,
        expected_size: build.download_size,
    };

    let downloaded = Arc::new(AtomicU64::new(0));
    let total_bytes = build.download_size.unwrap_or(0);
    let started = Instant::now();
    let stage_dl = format!(
        "Downloading {} {}...",
        build.distribution, build.full_version
    );
    on_progress(JavaInstallProgress {
        step: 1,
        total_steps: 2,
        stage: stage_dl.clone(),
        downloaded_bytes: 0,
        total_bytes,
        completed_files: 0,
        total_files: 1,
        bytes_per_sec: 0,
        indeterminate: total_bytes == 0,
    });

    let dl_cb = Arc::clone(&on_progress);
    let dl_counter = Arc::clone(&downloaded);
    let stage_clone = stage_dl.clone();
    download_manager
        .download_file(&req, cancel, move |delta| {
            let cur = dl_counter.fetch_add(delta, Ordering::Relaxed) + delta;
            let elapsed = started.elapsed().as_secs_f64().max(0.1);
            let bps = (cur as f64 / elapsed) as u64;
            dl_cb(JavaInstallProgress {
                step: 1,
                total_steps: 2,
                stage: stage_clone.clone(),
                downloaded_bytes: cur,
                total_bytes,
                completed_files: 0,
                total_files: 1,
                bytes_per_sec: bps,
                indeterminate: total_bytes == 0,
            });
        })
        .await?;

    if target_dir.exists() {
        let _ = tokio::fs::remove_dir_all(&target_dir).await;
    }

    on_progress(JavaInstallProgress {
        step: 2,
        total_steps: 2,
        stage: format!(
            "Extracting {} {}...",
            build.distribution, build.full_version
        ),
        downloaded_bytes: total_bytes,
        total_bytes,
        completed_files: 1,
        total_files: 1,
        bytes_per_sec: 0,
        indeterminate: true,
    });

    let archive_clone = archive_path.clone();
    let target_clone = target_dir.clone();
    tokio::task::spawn_blocking(move || -> Result<()> {
        if is_zip {
            crate::core::minecraft::natives::extract_native_archive(
                &archive_clone,
                &target_clone,
                &[],
            )?;
        } else {
            extract_tar_gz_safely(&archive_clone, &target_clone)?;
        }
        Ok(())
    })
    .await
    .map_err(|err| LauncherError::JavaRuntime(format!("Extraction failed: {err}")))??;

    let _ = tokio::fs::remove_file(&archive_path).await;

    let java_bin = find_java_bin(&target_dir).ok_or_else(|| {
        LauncherError::JavaRuntime(format!(
            "Installed runtime at {} does not contain a java executable",
            target_dir.display()
        ))
    })?;
    ensure_executable_permission(&java_bin)?;
    probe_java_executable(&java_bin, true).await
}

async fn install_mojang_runtime(
    build: &AvailableJavaBuild,
    paths: &AppPaths,
    client: &Client,
    download_manager: &DownloadManager,
    cancel: &CancellationToken,
    on_progress: Arc<dyn Fn(JavaInstallProgress) + Send + Sync>,
) -> Result<JavaRuntime> {
    let component = build.component.as_deref().unwrap_or("java-runtime");
    let target_dir = paths.runtimes_dir().join("mojang").join(component);

    if let Some(existing) = find_java_bin(&target_dir) {
        if let Ok(rt) = probe_java_executable(&existing, true).await {
            return Ok(rt);
        }
    }

    let manifest: MojangRuntimeFileManifest = client
        .get(&build.download_url)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let mut file_tasks: Vec<(PathBuf, DownloadRequest, bool)> = Vec::new();
    let mut link_tasks: Vec<(PathBuf, String)> = Vec::new();

    for (rel_str, entry) in manifest.files {
        let rel = Path::new(&rel_str);
        if rel.is_absolute()
            || rel.components().any(|c| {
                matches!(
                    c,
                    Component::ParentDir | Component::RootDir | Component::Prefix(_)
                )
            })
        {
            return Err(LauncherError::UnsafePath(format!(
                "Unsafe Mojang runtime entry: '{rel_str}'"
            )));
        }
        let dest = target_dir.join(rel);
        match entry {
            MojangRuntimeFileEntry::Directory => {
                let _ = tokio::fs::create_dir_all(&dest).await;
            }
            MojangRuntimeFileEntry::File {
                executable,
                downloads,
            } => {
                file_tasks.push((
                    dest.clone(),
                    DownloadRequest {
                        url: downloads.raw.url,
                        destination: dest,
                        expected_sha1: Some(downloads.raw.sha1),
                        expected_size: Some(downloads.raw.size),
                    },
                    executable,
                ));
            }
            MojangRuntimeFileEntry::Link { target } => {
                link_tasks.push((dest, target));
            }
        }
    }

    let total_files = file_tasks.len();
    let total_bytes: u64 = file_tasks
        .iter()
        .filter_map(|(_, req, _)| req.expected_size)
        .sum();
    let completed_files = Arc::new(AtomicUsize::new(0));
    let downloaded_bytes = Arc::new(AtomicU64::new(0));
    let started = Instant::now();

    on_progress(JavaInstallProgress {
        step: 1,
        total_steps: 2,
        stage: format!("Downloading {} runtime files...", build.distribution),
        downloaded_bytes: 0,
        total_bytes,
        completed_files: 0,
        total_files,
        bytes_per_sec: 0,
        indeterminate: total_files == 0,
    });

    let results: Vec<Result<()>> = stream::iter(file_tasks)
        .map(|(dest, req, executable)| {
            let dm = download_manager.clone();
            let cancel = cancel.clone();
            let cb = Arc::clone(&on_progress);
            let done_files = Arc::clone(&completed_files);
            let done_bytes = Arc::clone(&downloaded_bytes);
            let dist = build.distribution.clone();
            async move {
                let cb_delta = Arc::clone(&cb);
                let done_bytes_delta = Arc::clone(&done_bytes);
                let done_files_delta = Arc::clone(&done_files);
                let dist_delta = dist.clone();
                dm.download_file(&req, &cancel, move |delta| {
                    let cur_b = done_bytes_delta.fetch_add(delta, Ordering::Relaxed) + delta;
                    let cur_f = done_files_delta.load(Ordering::Relaxed);
                    let elapsed = started.elapsed().as_secs_f64().max(0.1);
                    cb_delta(JavaInstallProgress {
                        step: 1,
                        total_steps: 2,
                        stage: format!("Downloading {dist_delta} runtime files..."),
                        downloaded_bytes: cur_b,
                        total_bytes,
                        completed_files: cur_f,
                        total_files,
                        bytes_per_sec: (cur_b as f64 / elapsed) as u64,
                        indeterminate: false,
                    });
                })
                .await?;
                if executable {
                    ensure_executable_permission(&dest)?;
                }
                let cur_f = done_files.fetch_add(1, Ordering::Relaxed) + 1;
                let cur_b = done_bytes.load(Ordering::Relaxed);
                let elapsed = started.elapsed().as_secs_f64().max(0.1);
                cb(JavaInstallProgress {
                    step: 1,
                    total_steps: 2,
                    stage: format!("Downloading {dist} runtime files..."),
                    downloaded_bytes: cur_b,
                    total_bytes,
                    completed_files: cur_f,
                    total_files,
                    bytes_per_sec: (cur_b as f64 / elapsed) as u64,
                    indeterminate: false,
                });
                Ok(())
            }
        })
        .buffer_unordered(12)
        .collect()
        .await;

    for res in results {
        res?;
    }

    #[cfg(unix)]
    for (dest, target) in link_tasks {
        if let Some(parent) = dest.parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }
        let _ = tokio::fs::remove_file(&dest).await;
        let _ = std::os::unix::fs::symlink(&target, &dest);
    }

    #[cfg(not(unix))]
    let _ = link_tasks;

    let java_bin = find_java_bin(&target_dir).ok_or_else(|| {
        LauncherError::JavaRuntime(format!(
            "Mojang runtime '{}' missing java executable",
            component
        ))
    })?;
    ensure_executable_permission(&java_bin)?;
    probe_java_executable(&java_bin, true).await
}

/// Instala um JRE Eclipse Temurin gerenciado pelo Crab em `runtimes/temurin-<major>-<arch>/`
/// sem exigir privilégios de administrador.
pub async fn install_temurin_runtime(
    major_version: u32,
    paths: &AppPaths,
    download_manager: &DownloadManager,
    cancel: &CancellationToken,
) -> Result<JavaRuntime> {
    let (os, ext) = adoptium_os_and_ext()?;
    let arch = adoptium_arch()?;
    let target_dir = paths
        .runtimes_dir()
        .join(format!("temurin-{major_version}-{arch}"));

    // Reutiliza runtime já instalado se o executável responder corretamente.
    if let Some(existing_bin) = find_java_bin(&target_dir) {
        if let Ok(rt) = probe_java_executable(&existing_bin, true).await {
            if rt.major_version == major_version {
                info!(
                    "Reusing managed Temurin runtime at {}",
                    existing_bin.display()
                );
                return Ok(rt);
            }
        }
    }

    let url = format!(
        "https://api.adoptium.net/v3/binary/latest/{major_version}/ga/{os}/{arch}/jre/hotspot/normal/eclipse"
    );
    let archive_path = paths
        .cache_dir
        .join(format!("temurin-{major_version}-{os}-{arch}.{ext}"));

    let req = DownloadRequest {
        url,
        destination: archive_path.clone(),
        expected_sha1: None,
        expected_size: None,
    };

    info!("Downloading Eclipse Temurin JRE {major_version} for {os}/{arch}...");
    download_manager.download_file(&req, cancel, |_| {}).await?;

    if cancel.is_cancelled() {
        return Err(LauncherError::Cancelled);
    }

    // Limpa tentativa incompleta anterior antes de extrair.
    if target_dir.exists() {
        let _ = tokio::fs::remove_dir_all(&target_dir).await;
    }

    let archive_clone = archive_path.clone();
    let target_clone = target_dir.clone();
    let is_zip = ext == "zip";

    tokio::task::spawn_blocking(move || -> Result<()> {
        if is_zip {
            crate::core::minecraft::natives::extract_native_archive(
                &archive_clone,
                &target_clone,
                &[],
            )?;
        } else {
            extract_tar_gz_safely(&archive_clone, &target_clone)?;
        }
        Ok(())
    })
    .await
    .map_err(|err| {
        LauncherError::JavaRuntime(format!("Runtime extraction task failed: {err}"))
    })??;

    let _ = tokio::fs::remove_file(&archive_path).await;

    let java_bin = find_java_bin(&target_dir).ok_or_else(|| {
        LauncherError::JavaRuntime(format!(
            "Extracted runtime at {} does not contain a java executable",
            target_dir.display()
        ))
    })?;

    ensure_executable_permission(&java_bin)?;
    let runtime = probe_java_executable(&java_bin, true).await?;
    info!(
        "Installed managed Java runtime: {}",
        runtime.display_label()
    );
    Ok(runtime)
}

fn adoptium_os_and_ext() -> Result<(&'static str, &'static str)> {
    match std::env::consts::OS {
        "linux" => Ok(("linux", "tar.gz")),
        "macos" => Ok(("mac", "tar.gz")),
        "windows" => Ok(("windows", "zip")),
        other => Err(LauncherError::JavaRuntime(format!(
            "Unsupported OS for automatic Java download: {other}"
        ))),
    }
}

fn adoptium_arch() -> Result<&'static str> {
    match std::env::consts::ARCH {
        "x86_64" => Ok("x64"),
        "aarch64" => Ok("aarch64"),
        "x86" => Ok("x32"),
        other => Err(LauncherError::JavaRuntime(format!(
            "Unsupported architecture for automatic Java download: {other}"
        ))),
    }
}

/// Extrai um arquivo `.tar.gz` validando todos os caminhos contra path traversal.
fn extract_tar_gz_safely(archive_path: &Path, dest_dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dest_dir).map_err(|source| LauncherError::Io {
        path: dest_dir.to_path_buf(),
        source,
    })?;

    let file = File::open(archive_path).map_err(|source| LauncherError::Io {
        path: archive_path.to_path_buf(),
        source,
    })?;
    let decoder = GzDecoder::new(file);
    let mut archive = Archive::new(decoder);

    let entries = archive
        .entries()
        .map_err(|err| LauncherError::ArchiveExtract {
            path: archive_path.to_path_buf(),
            reason: err.to_string(),
        })?;

    for entry_res in entries {
        let mut entry = entry_res.map_err(|err| LauncherError::ArchiveExtract {
            path: archive_path.to_path_buf(),
            reason: err.to_string(),
        })?;

        let rel_path = entry
            .path()
            .map_err(|err| LauncherError::ArchiveExtract {
                path: archive_path.to_path_buf(),
                reason: err.to_string(),
            })?
            .into_owned();

        if rel_path.is_absolute()
            || rel_path.components().any(|c| {
                matches!(
                    c,
                    Component::ParentDir | Component::RootDir | Component::Prefix(_)
                )
            })
        {
            return Err(LauncherError::UnsafePath(format!(
                "Unsafe tar entry '{}' in {}",
                rel_path.display(),
                archive_path.display()
            )));
        }

        let out_path = dest_dir.join(&rel_path);
        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| LauncherError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        entry
            .unpack(&out_path)
            .map_err(|err| LauncherError::ArchiveExtract {
                path: out_path,
                reason: err.to_string(),
            })?;
    }

    Ok(())
}

fn find_java_bin(root: &Path) -> Option<PathBuf> {
    #[cfg(windows)]
    let bin_name = "java.exe";
    #[cfg(not(windows))]
    let bin_name = "java";

    let direct = root.join("bin").join(bin_name);
    if direct.is_file() {
        return Some(direct);
    }
    if let Ok(entries) = std::fs::read_dir(root) {
        for entry in entries.flatten() {
            let sub = entry.path();
            if sub.is_dir() {
                let candidate = sub.join("bin").join(bin_name);
                if candidate.is_file() {
                    return Some(candidate);
                }
                let mac_candidate = sub.join("Contents").join("Home").join("bin").join(bin_name);
                if mac_candidate.is_file() {
                    return Some(mac_candidate);
                }
            }
        }
    }
    None
}

#[cfg(unix)]
fn ensure_executable_permission(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(path)
        .map_err(|source| LauncherError::Io {
            path: path.to_path_buf(),
            source,
        })?
        .permissions();
    perms.set_mode(perms.mode() | 0o111);
    std::fs::set_permissions(path, perms).map_err(|source| LauncherError::Io {
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(not(unix))]
fn ensure_executable_permission(_path: &Path) -> Result<()> {
    Ok(())
}
