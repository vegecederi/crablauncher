use crate::error::{LauncherError, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::process::Command;

const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// Representa uma instalação Java detectada ou gerenciada pelo Crab.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JavaRuntime {
    pub path: PathBuf,
    pub version: String,
    pub major_version: u32,
    pub vendor: String,
    pub arch: String,
    pub is_managed: bool,
}

impl JavaRuntime {
    pub fn display_label(&self) -> String {
        let managed_tag = if self.is_managed { " [Crab]" } else { "" };
        format!(
            "Java {} ({}) — {} ({}){}",
            self.major_version, self.version, self.vendor, self.arch, managed_tag
        )
    }
}

/// Executa `<java> -XshowSettings:properties -version` com timeout para extrair metadados reais.
pub async fn probe_java_executable(path: &Path, is_managed: bool) -> Result<JavaRuntime> {
    if !path.is_file() {
        return Err(LauncherError::JavaRuntime(format!(
            "Java executable not found: {}",
            path.display()
        )));
    }

    let output = tokio::time::timeout(
        PROBE_TIMEOUT,
        Command::new(path)
            .arg("-XshowSettings:properties")
            .arg("-version")
            .output(),
    )
    .await
    .map_err(|_| {
        LauncherError::JavaRuntime(format!(
            "Timed out while probing Java at {}",
            path.display()
        ))
    })?
    .map_err(|err| {
        LauncherError::JavaRuntime(format!(
            "Failed to execute Java at {}: {err}",
            path.display()
        ))
    })?;

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined = format!("{stderr}\n{stdout}");

    parse_java_properties_output(path, &combined, is_managed)
}

/// Extrai versão, major version, fornecedor e arquitetura da saída de `-XshowSettings:properties -version`.
pub fn parse_java_properties_output(
    path: &Path,
    output: &str,
    is_managed: bool,
) -> Result<JavaRuntime> {
    let mut version = None;
    let mut vendor = None;
    let mut arch = None;

    for line in output.lines() {
        let trimmed = line.trim();
        if let Some(val) = trimmed.strip_prefix("java.version =") {
            version = Some(val.trim().to_string());
        } else if let Some(val) = trimmed.strip_prefix("java.vendor =") {
            vendor = Some(val.trim().to_string());
        } else if let Some(val) = trimmed.strip_prefix("os.arch =") {
            arch = Some(val.trim().to_string());
        }
    }

    // Fallback para a linha `version "21.0.5"` caso `-XshowSettings` não esteja presente.
    let version = version.or_else(|| {
        output.lines().find_map(|line| {
            let start = line.find('"')? + 1;
            let rest = &line[start..];
            let end = rest.find('"')?;
            Some(rest[..end].to_string())
        })
    });

    let version = version.ok_or_else(|| {
        LauncherError::JavaRuntime(format!(
            "Could not parse Java version from {}",
            path.display()
        ))
    })?;

    let major_version = parse_major_version(&version).ok_or_else(|| {
        LauncherError::JavaRuntime(format!(
            "Could not determine Java major version from '{version}'"
        ))
    })?;

    Ok(JavaRuntime {
        path: path.to_path_buf(),
        version,
        major_version,
        vendor: vendor.unwrap_or_else(|| "Unknown".to_string()),
        arch: arch.unwrap_or_else(|| std::env::consts::ARCH.to_string()),
        is_managed,
    })
}

/// Testa argumentos JVM opcionais diretamente com o executável Java (sem shell) usando `-version`.
pub async fn test_jvm_arguments(
    java_path: &Path,
    min_memory_mb: u32,
    max_memory_mb: u32,
    raw_args: &str,
) -> Result<String> {
    if !java_path.is_file() {
        return Err(LauncherError::JavaRuntime(format!(
            "Java executable not found: {}",
            java_path.display()
        )));
    }

    let (clean_args, stripped) = crate::core::config::strip_conflicting_memory_args(raw_args);
    let mut cmd = Command::new(java_path);
    cmd.arg(format!("-Xms{min_memory_mb}M"))
        .arg(format!("-Xmx{max_memory_mb}M"))
        .args(&clean_args)
        .arg("-version");

    let output = tokio::time::timeout(PROBE_TIMEOUT, cmd.output())
        .await
        .map_err(|_| LauncherError::JavaRuntime("Timed out while testing JVM arguments.".into()))?
        .map_err(|err| {
            LauncherError::JavaRuntime(format!("Failed to invoke Java for argument test: {err}"))
        })?;

    if output.status.success() {
        let mut msg = "JVM arguments accepted by Java.".to_string();
        if !stripped.is_empty() {
            msg.push_str(&format!(
                " (Ignored conflicting memory flags: {})",
                stripped.join(", ")
            ));
        }
        Ok(msg)
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let first_err = stderr
            .lines()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("JVM rejected arguments");
        Err(LauncherError::JavaRuntime(format!(
            "JVM rejected arguments: {first_err}"
        )))
    }
}

/// Converte strings de versão Java (`1.8.0_412`, `17.0.11`, `21.0.5+11`, `25-ea`) em major version.
pub fn parse_major_version(version: &str) -> Option<u32> {
    let clean = version.trim();
    if let Some(rest) = clean.strip_prefix("1.") {
        return rest
            .split(|c: char| !c.is_ascii_digit())
            .next()
            .and_then(|s| s.parse::<u32>().ok());
    }
    clean
        .split(|c: char| !c.is_ascii_digit())
        .next()
        .and_then(|s| s.parse::<u32>().ok())
}
