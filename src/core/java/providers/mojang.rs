use crate::core::java::providers::{AvailableJavaBuild, JavaPackageType, JavaProviderKind};
use crate::core::java::runtime::parse_major_version;
use crate::error::{LauncherError, Result};
use reqwest::Client;
use serde::Deserialize;
use std::collections::HashMap;

pub const MOJANG_JAVA_MANIFEST_URL: &str =
    "https://launchermeta.mojang.com/v1/products/java-runtime/2ec0cc96c44e5a76b9c8b7c39df7210883d12871/all.json";

#[derive(Debug, Deserialize)]
pub struct MojangComponentEntry {
    manifest: MojangComponentManifestRef,
    version: MojangComponentVersion,
}

#[derive(Debug, Deserialize)]
struct MojangComponentManifestRef {
    sha1: String,
    size: u64,
    url: String,
}

#[derive(Debug, Deserialize)]
struct MojangComponentVersion {
    name: String,
    released: String,
}

/// Manifesto de arquivos individuais de um componente Java da Mojang.
#[derive(Debug, Deserialize)]
pub struct MojangRuntimeFileManifest {
    pub files: HashMap<String, MojangRuntimeFileEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MojangRuntimeFileEntry {
    Directory,
    File {
        #[serde(default)]
        executable: bool,
        downloads: MojangRuntimeFileDownloads,
    },
    Link {
        target: String,
    },
}

#[derive(Debug, Deserialize)]
pub struct MojangRuntimeFileDownloads {
    pub raw: MojangRawDownload,
}

#[derive(Debug, Deserialize)]
pub struct MojangRawDownload {
    pub sha1: String,
    pub size: u64,
    pub url: String,
}

pub fn mojang_platform_key() -> Result<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => Ok("linux"),
        ("linux", "x86") => Ok("linux-i386"),
        ("macos", "x86_64") => Ok("mac-os"),
        ("macos", "aarch64") => Ok("mac-os-arm64"),
        ("windows", "x86_64") => Ok("windows-x64"),
        ("windows", "x86") => Ok("windows-x86"),
        ("windows", "aarch64") => Ok("windows-arm64"),
        (os, arch) => Err(LauncherError::JavaRuntime(format!(
            "Mojang Java runtimes are not published for {os}/{arch}"
        ))),
    }
}

/// Consulta o catálogo oficial de runtimes Java da Mojang para a plataforma atual.
pub async fn fetch_mojang_builds(client: &Client) -> Result<Vec<AvailableJavaBuild>> {
    let platform_key = mojang_platform_key()?;
    let resp = client
        .get(MOJANG_JAVA_MANIFEST_URL)
        .send()
        .await?
        .error_for_status()?;
    let raw: HashMap<String, HashMap<String, Vec<MojangComponentEntry>>> = resp.json().await?;
    Ok(normalize_mojang_catalog(&raw, platform_key))
}

/// Normaliza o catálogo JSON da Mojang para o modelo comum (testável com fixtures locais).
pub fn normalize_mojang_catalog(
    raw: &HashMap<String, HashMap<String, Vec<MojangComponentEntry>>>,
    platform_key: &str,
) -> Vec<AvailableJavaBuild> {
    let Some(components) = raw.get(platform_key) else {
        return Vec::new();
    };

    let mut builds = Vec::new();
    for (component_name, entries) in components {
        for entry in entries {
            let Some(major) = parse_major_version(&entry.version.name) else {
                continue;
            };
            let released = entry.version.released.get(0..10).map(ToString::to_string);
            let is_experimental = component_name.contains("snapshot");

            builds.push(AvailableJavaBuild {
                provider: JavaProviderKind::Mojang,
                distribution: "Mojang".to_string(),
                component: Some(component_name.clone()),
                major_version: major,
                full_version: entry.version.name.clone(),
                build_number: Some(component_name.clone()),
                release_date: released,
                arch: std::env::consts::ARCH.to_string(),
                os: std::env::consts::OS.to_string(),
                package_type: JavaPackageType::Jre,
                jvm_impl: Some("hotspot".to_string()),
                download_url: entry.manifest.url.clone(),
                download_size: Some(entry.manifest.size),
                checksum: Some(entry.manifest.sha1.clone()),
                is_recommended: !is_experimental,
                is_experimental,
            });
        }
    }

    builds.sort_by(|a, b| {
        b.major_version
            .cmp(&a.major_version)
            .then_with(|| b.release_date.cmp(&a.release_date))
    });
    builds
}
