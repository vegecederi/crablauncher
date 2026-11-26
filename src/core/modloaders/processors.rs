use crate::core::launch::classpath::classpath_separator;
use crate::core::minecraft::libraries::maven_coordinate_to_path;
use crate::core::minecraft::metadata::Library;
use crate::error::{LauncherError, Result};
use crate::platform::paths::AppPaths;
use crate::services::integrity::{verify_file, FileIntegrity};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::process::Command;
use tokio_util::sync::CancellationToken;
use tracing::info;

/// Estrutura do `install_profile.json` presente em instaladores modernos do Forge e NeoForge.
#[derive(Debug, Clone, Deserialize)]
pub struct InstallProfile {
    #[serde(default)]
    pub spec: u32,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub minecraft: String,
    #[serde(default)]
    pub data: HashMap<String, DataEntry>,
    #[serde(default)]
    pub processors: Vec<ProcessorSpec>,
    #[serde(default)]
    pub libraries: Vec<Library>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DataEntry {
    pub client: String,
    #[serde(default)]
    pub server: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProcessorSpec {
    #[serde(default)]
    pub sides: Vec<String>,
    pub jar: String,
    #[serde(default)]
    pub classpath: Vec<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub outputs: HashMap<String, String>,
}

/// Executa os processors client-side de um `install_profile.json` usando o runtime Java fornecido.
pub async fn run_install_processors(
    profile: &InstallProfile,
    java_executable: &Path,
    installer_jar: &Path,
    minecraft_jar: &Path,
    paths: &AppPaths,
    cancel: &CancellationToken,
) -> Result<()> {
    let mut data_vars: HashMap<String, String> = HashMap::new();
    data_vars.insert("SIDE".into(), "client".into());
    data_vars.insert(
        "MINECRAFT_JAR".into(),
        minecraft_jar.to_string_lossy().into_owned(),
    );
    data_vars.insert(
        "INSTALLER".into(),
        installer_jar.to_string_lossy().into_owned(),
    );
    data_vars.insert("ROOT".into(), paths.data_dir.to_string_lossy().into_owned());
    data_vars.insert(
        "LIBRARY_DIR".into(),
        paths.libraries_dir().to_string_lossy().into_owned(),
    );

    let extract_dir = paths.cache_dir.join("modloaders").join("processor-data");
    let _ = std::fs::create_dir_all(&extract_dir);

    for (key, entry) in &profile.data {
        let resolved = resolve_data_value(&entry.client, installer_jar, &extract_dir, paths);
        data_vars.insert(key.clone(), resolved);
    }

    for (idx, proc_spec) in profile.processors.iter().enumerate() {
        if cancel.is_cancelled() {
            return Err(LauncherError::Cancelled);
        }
        // Executa apenas processors aplicáveis ao lado cliente.
        if !proc_spec.sides.is_empty() && !proc_spec.sides.iter().any(|s| s == "client") {
            continue;
        }

        // Pula processor se todos os outputs esperados já existirem e estiverem íntegros.
        if !proc_spec.outputs.is_empty()
            && outputs_already_valid(&proc_spec.outputs, &data_vars, paths).await?
        {
            continue;
        }

        let jar_rel = maven_coordinate_to_path(&proc_spec.jar, None).ok_or_else(|| {
            LauncherError::Launch(format!(
                "Invalid processor jar coordinate: {}",
                proc_spec.jar
            ))
        })?;
        let jar_path = paths.library_file(&jar_rel);
        let main_class = read_jar_main_class(&jar_path)?;

        let mut cp_paths: Vec<String> = vec![jar_path.to_string_lossy().into_owned()];
        for cp_coord in &proc_spec.classpath {
            if let Some(rel) = maven_coordinate_to_path(cp_coord, None) {
                cp_paths.push(paths.library_file(&rel).to_string_lossy().into_owned());
            }
        }
        let cp = cp_paths.join(classpath_separator());

        let mut resolved_args = Vec::with_capacity(proc_spec.args.len());
        for raw_arg in &proc_spec.args {
            resolved_args.push(resolve_processor_arg(raw_arg, &data_vars, paths));
        }

        info!(
            "Running modloader processor {}/{}: {}",
            idx + 1,
            profile.processors.len(),
            proc_spec.jar
        );

        let output = tokio::select! {
            _ = cancel.cancelled() => return Err(LauncherError::Cancelled),
            res = Command::new(java_executable)
                .arg("-cp")
                .arg(&cp)
                .arg(&main_class)
                .args(&resolved_args)
                .output() => res.map_err(|err| {
                    LauncherError::Launch(format!("Failed to start processor '{}': {err}", proc_spec.jar))
                })?,
        };

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(LauncherError::Launch(format!(
                "Modloader processor '{}' failed (exit {:?}): {}",
                proc_spec.jar,
                output.status.code(),
                stderr.trim()
            )));
        }
    }

    Ok(())
}

fn resolve_data_value(
    raw: &str,
    installer_jar: &Path,
    extract_dir: &Path,
    paths: &AppPaths,
) -> String {
    if raw.starts_with('[') && raw.ends_with(']') {
        let coord = &raw[1..raw.len() - 1];
        if let Some(rel) = maven_coordinate_to_path(coord, None) {
            return paths.library_file(&rel).to_string_lossy().into_owned();
        }
    }
    if raw.starts_with('\'') && raw.ends_with('\'') && raw.len() >= 2 {
        return raw[1..raw.len() - 1].to_string();
    }
    // Caminhos iniciados por '/' no install_profile.json referenciam arquivos dentro do installer.jar
    // (ex.: "/data/client.lzma" usado pelo processor BINPATCH do Forge).
    if let Some(inner) = raw.strip_prefix('/') {
        let dest = extract_dir.join(inner);
        if let Some(parent) = dest.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(file) = std::fs::File::open(installer_jar) {
            if let Ok(mut archive) = zip::ZipArchive::new(file) {
                if let Ok(mut entry) = archive.by_name(inner) {
                    if let Ok(mut out) = std::fs::File::create(&dest) {
                        if std::io::copy(&mut entry, &mut out).is_ok() {
                            return dest.to_string_lossy().into_owned();
                        }
                    }
                }
            }
        }
    }
    raw.to_string()
}

pub fn resolve_processor_arg(
    raw: &str,
    data_vars: &HashMap<String, String>,
    paths: &AppPaths,
) -> String {
    if raw.starts_with('[') && raw.ends_with(']') {
        let coord = &raw[1..raw.len() - 1];
        if let Some(rel) = maven_coordinate_to_path(coord, None) {
            return paths.library_file(&rel).to_string_lossy().into_owned();
        }
    }
    let mut out = raw.to_string();
    for (k, v) in data_vars {
        let placeholder = format!("{{{k}}}");
        if out.contains(&placeholder) {
            out = out.replace(&placeholder, v);
        }
    }
    out
}

async fn outputs_already_valid(
    outputs: &HashMap<String, String>,
    data_vars: &HashMap<String, String>,
    paths: &AppPaths,
) -> Result<bool> {
    for (raw_path, raw_sha1) in outputs {
        let resolved_path = PathBuf::from(resolve_processor_arg(raw_path, data_vars, paths));
        let expected_sha1 = resolve_processor_arg(raw_sha1, data_vars, paths);
        let clean_sha1 = expected_sha1.trim_matches('\'');
        if verify_file(&resolved_path, Some(clean_sha1), None).await? != FileIntegrity::Valid {
            return Ok(false);
        }
    }
    Ok(true)
}

fn read_jar_main_class(jar_path: &Path) -> Result<String> {
    let file = std::fs::File::open(jar_path).map_err(|source| LauncherError::Io {
        path: jar_path.to_path_buf(),
        source,
    })?;
    let mut archive = zip::ZipArchive::new(file).map_err(|err| LauncherError::ArchiveExtract {
        path: jar_path.to_path_buf(),
        reason: err.to_string(),
    })?;
    let mut manifest =
        archive
            .by_name("META-INF/MANIFEST.MF")
            .map_err(|err| LauncherError::ArchiveExtract {
                path: jar_path.to_path_buf(),
                reason: format!("Missing MANIFEST.MF: {err}"),
            })?;
    let mut content = String::new();
    std::io::Read::read_to_string(&mut manifest, &mut content).map_err(|source| {
        LauncherError::Io {
            path: jar_path.to_path_buf(),
            source,
        }
    })?;

    for line in content.lines() {
        if let Some(mc) = line.strip_prefix("Main-Class:") {
            return Ok(mc.trim().to_string());
        }
    }

    Err(LauncherError::Launch(format!(
        "Processor JAR {} has no Main-Class in MANIFEST.MF",
        jar_path.display()
    )))
}
