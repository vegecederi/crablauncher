use crate::core::components::model::{ModloaderKind, ModloaderSpec};
use crate::core::components::profile::LoaderProfile;
use crate::core::minecraft::libraries::{resolve_libraries, TargetPlatform};
use crate::core::modloaders::fabric::fetch_fabric_profile;
use crate::core::modloaders::forge::forge_installer_url;
use crate::core::modloaders::liteloader::fetch_liteloader_profile;
use crate::core::modloaders::neoforge::neoforge_installer_url;
use crate::core::modloaders::processors::{run_install_processors, InstallProfile};
use crate::core::modloaders::quilt::fetch_quilt_profile;
use crate::error::{LauncherError, Result};
use crate::platform::paths::AppPaths;
use crate::services::download::{DownloadManager, DownloadRequest};
use crate::services::instance_manager::InstanceManager;
use crate::services::storage::write_json_atomic;
use futures_util::stream::{self, StreamExt};
use reqwest::Client;
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tracing::info;

#[derive(Debug, Clone)]
pub struct ModloaderInstallProgress {
    pub step: u32,
    pub total_steps: u32,
    pub stage: String,
    pub completed_files: usize,
    pub total_files: usize,
    pub indeterminate: bool,
}

/// Instala um modloader (Fabric, Forge ou NeoForge) em uma instância de forma transacional:
/// se qualquer etapa falhar ou for cancelada, os metadados anteriores da instância permanecem intactos.
#[allow(clippy::too_many_arguments)]
pub async fn install_modloader(
    instance_id: &str,
    kind: ModloaderKind,
    loader_version: &str,
    java_executable: Option<&Path>,
    paths: &AppPaths,
    client: &Client,
    download_manager: &DownloadManager,
    instance_manager: &InstanceManager,
    cancel: &CancellationToken,
) -> Result<ModloaderSpec> {
    install_modloader_with_progress(
        instance_id,
        kind,
        loader_version,
        java_executable,
        paths,
        client,
        download_manager,
        instance_manager,
        cancel,
        |_| {},
    )
    .await
}

/// Instala um modloader reportando etapas reais e progresso de download das bibliotecas.
#[allow(clippy::too_many_arguments)]
pub async fn install_modloader_with_progress<F>(
    instance_id: &str,
    kind: ModloaderKind,
    loader_version: &str,
    java_executable: Option<&Path>,
    paths: &AppPaths,
    client: &Client,
    download_manager: &DownloadManager,
    instance_manager: &InstanceManager,
    cancel: &CancellationToken,
    on_progress: F,
) -> Result<ModloaderSpec>
where
    F: Fn(ModloaderInstallProgress) + Send + Sync + 'static,
{
    let on_progress: Arc<dyn Fn(ModloaderInstallProgress) + Send + Sync> = Arc::new(on_progress);
    let instance = instance_manager.find_by_id(instance_id).await?;
    let mc_version = &instance.minecraft_version;

    let loader_profile = match kind {
        ModloaderKind::Fabric | ModloaderKind::Quilt | ModloaderKind::LiteLoader => {
            on_progress(ModloaderInstallProgress {
                step: 1,
                total_steps: 2,
                stage: format!("Fetching {} {loader_version} profile...", kind.short_name()),
                completed_files: 0,
                total_files: 0,
                indeterminate: true,
            });
            let profile = match kind {
                ModloaderKind::Fabric => {
                    fetch_fabric_profile(client, mc_version, loader_version).await?
                }
                ModloaderKind::Quilt => {
                    fetch_quilt_profile(client, mc_version, loader_version).await?
                }
                ModloaderKind::LiteLoader => {
                    fetch_liteloader_profile(client, mc_version, loader_version).await?
                }
                _ => unreachable!(),
            };
            download_loader_libraries(
                &profile.libraries,
                paths,
                download_manager,
                cancel,
                Arc::clone(&on_progress),
                2,
                2,
                format!("Downloading Fabric {loader_version} libraries..."),
            )
            .await?;
            profile
        }
        ModloaderKind::Forge | ModloaderKind::NeoForge => {
            let java_exec = java_executable.ok_or_else(|| {
                LauncherError::JavaRuntime(
                    "A compatible Java runtime is required to run Forge/NeoForge install processors."
                        .to_string(),
                )
            })?;
            let installer_url = match kind {
                ModloaderKind::Forge => forge_installer_url(mc_version, loader_version),
                ModloaderKind::NeoForge => neoforge_installer_url(loader_version),
                ModloaderKind::Fabric | ModloaderKind::Quilt | ModloaderKind::LiteLoader => {
                    unreachable!()
                }
            };
            install_forge_like_loader(
                &installer_url,
                mc_version,
                loader_version,
                java_exec,
                paths,
                download_manager,
                cancel,
                Arc::clone(&on_progress),
            )
            .await?
        }
    };

    // Somente após tudo baixado e processado com sucesso, grava `loader-profile.json` e atualiza `instance.json`.
    let profile_path = paths
        .instance_dir(&instance.dir_name)
        .join("loader-profile.json");
    write_json_atomic(&profile_path, &loader_profile).await?;

    let spec = ModloaderSpec {
        kind,
        version: loader_version.to_string(),
    };
    instance_manager
        .set_modloader(instance_id, Some(spec.clone()))
        .await?;

    info!(
        "Installed {} {} on instance '{}'",
        kind.label(),
        loader_version,
        instance.name
    );
    Ok(spec)
}

/// Remove o modloader da instância, retornando-a para Vanilla sem excluir mundos, mods ou configs.
pub async fn remove_modloader(
    instance_id: &str,
    paths: &AppPaths,
    instance_manager: &InstanceManager,
) -> Result<()> {
    let instance = instance_manager.find_by_id(instance_id).await?;
    let profile_path = paths
        .instance_dir(&instance.dir_name)
        .join("loader-profile.json");
    if profile_path.exists() {
        let _ = tokio::fs::remove_file(&profile_path).await;
    }
    instance_manager.set_modloader(instance_id, None).await?;
    info!("Removed modloader from instance '{}'", instance.name);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn install_forge_like_loader(
    installer_url: &str,
    mc_version: &str,
    loader_version: &str,
    java_executable: &Path,
    paths: &AppPaths,
    download_manager: &DownloadManager,
    cancel: &CancellationToken,
    on_progress: Arc<dyn Fn(ModloaderInstallProgress) + Send + Sync>,
) -> Result<LoaderProfile> {
    let installer_path = paths.cache_dir.join(format!(
        "loader-{mc_version}-{loader_version}-installer.jar"
    ));

    on_progress(ModloaderInstallProgress {
        step: 1,
        total_steps: 3,
        stage: format!("Downloading loader installer ({loader_version})..."),
        completed_files: 0,
        total_files: 1,
        indeterminate: true,
    });

    let req = DownloadRequest {
        url: installer_url.to_string(),
        destination: installer_path.clone(),
        expected_sha1: None,
        expected_size: None,
    };
    download_manager.download_file(&req, cancel, |_| {}).await?;

    let (install_profile, version_profile) = extract_installer_profiles(&installer_path, paths)?;

    // Extrai quaisquer artefatos embutidos em maven/ dentro do installer.jar para libraries/.
    extract_embedded_maven_libraries(&installer_path, paths)?;

    // Baixa bibliotecas do instalador (necessárias para os processors)
    download_loader_libraries(
        &install_profile.libraries,
        paths,
        download_manager,
        cancel,
        Arc::clone(&on_progress),
        2,
        3,
        "Downloading installer libraries...".to_string(),
    )
    .await?;

    // Executa os processors client-side (quando houver) ANTES de verificar as bibliotecas da versão,
    // pois os processors geram artefatos locais como forge-*-client.jar.
    if !install_profile.processors.is_empty() {
        on_progress(ModloaderInstallProgress {
            step: 3,
            total_steps: 3,
            stage: "Running modloader install processors...".to_string(),
            completed_files: 0,
            total_files: 0,
            indeterminate: true,
        });
        let mc_jar = paths.version_jar_file(mc_version);
        run_install_processors(
            &install_profile,
            java_executable,
            &installer_path,
            &mc_jar,
            paths,
            cancel,
        )
        .await?;
    }

    // Baixa bibliotecas da versão (pulando artefatos locais já gerados pelos processors)
    download_loader_libraries(
        &version_profile.libraries,
        paths,
        download_manager,
        cancel,
        Arc::clone(&on_progress),
        3,
        3,
        "Downloading loader libraries...".to_string(),
    )
    .await?;

    Ok(version_profile)
}

fn extract_embedded_maven_libraries(installer_jar: &Path, paths: &AppPaths) -> Result<()> {
    let file = std::fs::File::open(installer_jar).map_err(|source| LauncherError::Io {
        path: installer_jar.to_path_buf(),
        source,
    })?;
    let mut archive = zip::ZipArchive::new(file).map_err(|err| LauncherError::ArchiveExtract {
        path: installer_jar.to_path_buf(),
        reason: err.to_string(),
    })?;

    for i in 0..archive.len() {
        let Ok(mut entry) = archive.by_index(i) else {
            continue;
        };
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().to_string();
        if let Some(rel) = name.strip_prefix("maven/") {
            if rel.is_empty() || rel.contains("..") {
                continue;
            }
            let dest = paths.library_file(rel);
            if let Some(parent) = dest.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Ok(mut out) = std::fs::File::create(&dest) {
                let _ = std::io::copy(&mut entry, &mut out);
            }
        }
    }
    Ok(())
}

fn extract_installer_profiles(
    installer_jar: &Path,
    paths: &AppPaths,
) -> Result<(InstallProfile, LoaderProfile)> {
    let file = std::fs::File::open(installer_jar).map_err(|source| LauncherError::Io {
        path: installer_jar.to_path_buf(),
        source,
    })?;
    let mut archive = zip::ZipArchive::new(file).map_err(|err| LauncherError::ArchiveExtract {
        path: installer_jar.to_path_buf(),
        reason: err.to_string(),
    })?;

    let install_profile_raw: String = {
        let mut entry = archive.by_name("install_profile.json").map_err(|err| {
            LauncherError::ArchiveExtract {
                path: installer_jar.to_path_buf(),
                reason: format!("Missing install_profile.json: {err}"),
            }
        })?;
        let mut s = String::new();
        entry
            .read_to_string(&mut s)
            .map_err(|source| LauncherError::Io {
                path: installer_jar.to_path_buf(),
                source,
            })?;
        s
    };

    // Formato moderno (Forge 1.13+ / NeoForge): possui version.json separado
    if let Ok(mut entry) = archive.by_name("version.json") {
        let mut s = String::new();
        entry
            .read_to_string(&mut s)
            .map_err(|source| LauncherError::Io {
                path: installer_jar.to_path_buf(),
                source,
            })?;
        let install_profile: InstallProfile = serde_json::from_str(&install_profile_raw)?;
        let version_profile: LoaderProfile = serde_json::from_str(&s)?;
        return Ok((install_profile, version_profile));
    }

    // Formato legado (Forge 1.6..1.12.2): versionInfo + install.filePath embutidos em install_profile.json
    let root_val: serde_json::Value = serde_json::from_str(&install_profile_raw)?;
    if let Some(version_info) = root_val.get("versionInfo") {
        let version_profile: LoaderProfile = serde_json::from_value(version_info.clone())?;
        // Extrai o universal jar embutido no installer para libraries/, se presente.
        if let (Some(file_path), Some(maven_coord)) = (
            root_val
                .pointer("/install/filePath")
                .and_then(|v| v.as_str()),
            root_val.pointer("/install/path").and_then(|v| v.as_str()),
        ) {
            if let Some(rel) =
                crate::core::minecraft::libraries::maven_coordinate_to_path(maven_coord, None)
            {
                if let Ok(mut jar_entry) = archive.by_name(file_path) {
                    let dest = paths.library_file(&rel);
                    if let Some(parent) = dest.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    if let Ok(mut out) = std::fs::File::create(&dest) {
                        let _ = std::io::copy(&mut jar_entry, &mut out);
                    }
                }
            }
        }
        let empty_profile = InstallProfile {
            spec: 0,
            version: version_profile.id.clone(),
            minecraft: version_profile.inherits_from.clone().unwrap_or_default(),
            data: HashMap::new(),
            processors: Vec::new(),
            libraries: Vec::new(),
        };
        return Ok((empty_profile, version_profile));
    }

    Err(LauncherError::Launch(
        "Unsupported Forge installer format: missing both version.json and legacy versionInfo."
            .to_string(),
    ))
}

#[allow(clippy::too_many_arguments)]
async fn download_loader_libraries(
    libraries: &[crate::core::minecraft::metadata::Library],
    paths: &AppPaths,
    download_manager: &DownloadManager,
    cancel: &CancellationToken,
    on_progress: Arc<dyn Fn(ModloaderInstallProgress) + Send + Sync>,
    step: u32,
    total_steps: u32,
    stage: String,
) -> Result<()> {
    let platform = TargetPlatform::current();
    let resolved = resolve_libraries(libraries, &platform, &HashMap::new());
    let total_files = resolved.len();
    let completed = Arc::new(AtomicUsize::new(0));
    on_progress(ModloaderInstallProgress {
        step,
        total_steps,
        stage: stage.clone(),
        completed_files: 0,
        total_files,
        indeterminate: total_files == 0,
    });

    let results: Vec<Result<()>> = stream::iter(resolved)
        .map(|lib| {
            let dm = download_manager.clone();
            let cancel = cancel.clone();
            let dest = paths.library_file(&lib.relative_path);
            let cb = Arc::clone(&on_progress);
            let done = Arc::clone(&completed);
            let stage_c = stage.clone();
            async move {
                // Bibliotecas sem URL são artefatos locais gerados pelos processors ou
                // extraídos diretamente do installer.jar.
                if lib.url.is_empty() {
                    let cur = done.fetch_add(1, Ordering::Relaxed) + 1;
                    cb(ModloaderInstallProgress {
                        step,
                        total_steps,
                        stage: stage_c,
                        completed_files: cur,
                        total_files,
                        indeterminate: false,
                    });
                    return Ok(());
                }
                let req = DownloadRequest {
                    url: lib.url,
                    destination: dest,
                    expected_sha1: lib.sha1,
                    expected_size: lib.size,
                };
                dm.download_file(&req, &cancel, |_| {}).await?;
                let cur = done.fetch_add(1, Ordering::Relaxed) + 1;
                cb(ModloaderInstallProgress {
                    step,
                    total_steps,
                    stage: stage_c,
                    completed_files: cur,
                    total_files,
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
    Ok(())
}
