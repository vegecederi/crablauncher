use crate::core::java::providers::{AvailableJavaBuild, JavaPackageType, JavaProviderKind};
use crate::error::{LauncherError, Result};
use reqwest::Client;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct AzulPackage {
    java_version: Vec<u32>,
    #[serde(default)]
    distro_version: Vec<u32>,
    java_package_type: String,
    download_url: String,
}

fn azul_os_and_ext() -> Result<(&'static str, &'static str)> {
    match std::env::consts::OS {
        "linux" => Ok(("linux", "tar.gz")),
        "macos" => Ok(("macos", "tar.gz")),
        "windows" => Ok(("windows", "zip")),
        other => Err(LauncherError::JavaRuntime(format!(
            "Unsupported OS for Azul Zulu: {other}"
        ))),
    }
}

fn azul_arch() -> Result<&'static str> {
    match std::env::consts::ARCH {
        "x86_64" => Ok("x64"),
        "aarch64" => Ok("arm64"),
        "x86" => Ok("x86"),
        other => Err(LauncherError::JavaRuntime(format!(
            "Unsupported arch for Azul Zulu: {other}"
        ))),
    }
}

/// Consulta a API oficial de metadados do Azul Zulu.
pub async fn fetch_azul_builds(client: &Client) -> Result<Vec<AvailableJavaBuild>> {
    let (os, ext) = azul_os_and_ext()?;
    let arch = azul_arch()?;
    let url = format!(
        "https://api.azul.com/metadata/v1/zulu/packages/?os={os}&arch={arch}&archive_type={ext}&javafx_bundled=false&latest=true&release_status=ga&page_size=40"
    );

    let packages: Vec<AzulPackage> = client
        .get(&url)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let mut builds = Vec::new();
    for pkg in packages {
        let Some(&major) = pkg.java_version.first() else {
            continue;
        };
        if major < 8 {
            continue;
        }
        let pkg_type = match pkg.java_package_type.as_str() {
            "jre" => JavaPackageType::Jre,
            "jdk" => JavaPackageType::Jdk,
            _ => continue,
        };
        let full_version = pkg
            .java_version
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(".");
        let build_number = if !pkg.distro_version.is_empty() {
            Some(
                pkg.distro_version
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("."),
            )
        } else {
            None
        };

        builds.push(AvailableJavaBuild {
            provider: JavaProviderKind::Azul,
            distribution: "Azul Zulu".to_string(),
            component: None,
            major_version: major,
            full_version,
            build_number,
            release_date: None,
            arch: arch.to_string(),
            os: os.to_string(),
            package_type: pkg_type,
            jvm_impl: Some("hotspot".to_string()),
            download_url: pkg.download_url,
            download_size: None,
            checksum: None,
            is_recommended: matches!(major, 8 | 17 | 21) && pkg_type == JavaPackageType::Jre,
            is_experimental: false,
        });
    }

    builds.sort_by_key(|a| std::cmp::Reverse(a.major_version));
    Ok(builds)
}
