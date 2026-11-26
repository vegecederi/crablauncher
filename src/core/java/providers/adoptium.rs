use crate::core::java::providers::{AvailableJavaBuild, JavaPackageType, JavaProviderKind};
use crate::error::{LauncherError, Result};
use futures_util::stream::{self, StreamExt};
use reqwest::Client;
use serde::Deserialize;

const ADOPTIUM_RELEASES_URL: &str = "https://api.adoptium.net/v3/info/available_releases";

#[derive(Debug, Deserialize)]
struct AvailableReleasesResponse {
    available_lts_releases: Vec<u32>,
    available_releases: Vec<u32>,
}

#[derive(Debug, Deserialize)]
struct AdoptiumRelease {
    #[serde(default)]
    timestamp: Option<String>,
    version_data: AdoptiumVersionData,
    #[serde(default)]
    binaries: Vec<AdoptiumBinary>,
}

#[derive(Debug, Deserialize)]
struct AdoptiumVersionData {
    major: u32,
    semver: String,
    #[serde(default)]
    build: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct AdoptiumBinary {
    architecture: String,
    os: String,
    image_type: String,
    #[serde(default)]
    jvm_impl: Option<String>,
    package: AdoptiumPackage,
}

#[derive(Debug, Deserialize)]
struct AdoptiumPackage {
    link: String,
    size: u64,
    #[serde(default)]
    checksum: Option<String>,
}

pub fn adoptium_os() -> Result<&'static str> {
    match std::env::consts::OS {
        "linux" => Ok("linux"),
        "macos" => Ok("mac"),
        "windows" => Ok("windows"),
        other => Err(LauncherError::JavaRuntime(format!(
            "Unsupported OS for Adoptium: {other}"
        ))),
    }
}

pub fn adoptium_arch() -> Result<&'static str> {
    match std::env::consts::ARCH {
        "x86_64" => Ok("x64"),
        "aarch64" => Ok("aarch64"),
        "x86" => Ok("x32"),
        other => Err(LauncherError::JavaRuntime(format!(
            "Unsupported arch for Adoptium: {other}"
        ))),
    }
}

/// Consulta as versões e builds oficiais do Eclipse Adoptium (Temurin).
pub async fn fetch_adoptium_builds(client: &Client) -> Result<Vec<AvailableJavaBuild>> {
    let os = adoptium_os()?;
    let arch = adoptium_arch()?;

    let info: AvailableReleasesResponse = client
        .get(ADOPTIUM_RELEASES_URL)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let mut majors: Vec<u32> = info
        .available_releases
        .into_iter()
        .filter(|&m| m >= 8)
        .collect();
    majors.sort_by(|a, b| b.cmp(a));
    majors.truncate(8);

    let lts_set = info.available_lts_releases;

    let results: Vec<Vec<AvailableJavaBuild>> = stream::iter(majors)
        .map(|major| {
            let client = client.clone();
            let is_lts = lts_set.contains(&major);
            async move {
                let url = format!(
                    "https://api.adoptium.net/v3/assets/feature_releases/{major}/ga?architecture={arch}&os={os}&vendor=eclipse&page_size=4"
                );
                let Ok(resp) = client.get(&url).send().await else {
                    return Vec::new();
                };
                let Ok(resp) = resp.error_for_status() else {
                    return Vec::new();
                };
                let Ok(releases) = resp.json::<Vec<AdoptiumRelease>>().await else {
                    return Vec::new();
                };
                normalize_adoptium_releases(
                    releases,
                    JavaProviderKind::Adoptium,
                    "Eclipse Temurin",
                    is_lts,
                )
            }
        })
        .buffer_unordered(4)
        .collect()
        .await;

    let mut builds: Vec<AvailableJavaBuild> = results.into_iter().flatten().collect();
    builds.sort_by(|a, b| {
        b.major_version
            .cmp(&a.major_version)
            .then_with(|| b.release_date.cmp(&a.release_date))
    });
    Ok(builds)
}

fn normalize_adoptium_releases(
    releases: Vec<AdoptiumRelease>,
    provider: JavaProviderKind,
    distribution: &str,
    is_lts: bool,
) -> Vec<AvailableJavaBuild> {
    let mut out = Vec::new();
    for rel in releases {
        let released = rel
            .timestamp
            .as_deref()
            .and_then(|s| s.get(0..10))
            .map(ToString::to_string);
        for bin in rel.binaries {
            let pkg_type = match bin.image_type.as_str() {
                "jre" => JavaPackageType::Jre,
                "jdk" => JavaPackageType::Jdk,
                _ => continue,
            };
            out.push(AvailableJavaBuild {
                provider,
                distribution: distribution.to_string(),
                component: None,
                major_version: rel.version_data.major,
                full_version: rel.version_data.semver.clone(),
                build_number: rel.version_data.build.map(|b| format!("+{b}")),
                release_date: released.clone(),
                arch: bin.architecture,
                os: bin.os,
                package_type: pkg_type,
                jvm_impl: bin.jvm_impl,
                download_url: bin.package.link,
                download_size: Some(bin.package.size),
                checksum: bin.package.checksum,
                is_recommended: is_lts && pkg_type == JavaPackageType::Jre,
                is_experimental: false,
            });
        }
    }
    out
}
