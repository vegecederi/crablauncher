use crate::core::java::providers::adoptium::{adoptium_arch, adoptium_os};
use crate::core::java::providers::{AvailableJavaBuild, JavaPackageType, JavaProviderKind};
use crate::error::Result;
use futures_util::stream::{self, StreamExt};
use reqwest::Client;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct SemeruRelease {
    #[serde(default)]
    timestamp: Option<String>,
    version_data: SemeruVersionData,
    #[serde(default)]
    binaries: Vec<SemeruBinary>,
}

#[derive(Debug, Deserialize)]
struct SemeruVersionData {
    major: u32,
    semver: String,
    #[serde(default)]
    build: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct SemeruBinary {
    architecture: String,
    os: String,
    image_type: String,
    package: SemeruPackage,
}

#[derive(Debug, Deserialize)]
struct SemeruPackage {
    link: String,
    size: u64,
    #[serde(default)]
    checksum: Option<String>,
}

/// Consulta builds oficiais do IBM Semeru Open Edition (OpenJ9).
pub async fn fetch_semeru_builds(client: &Client) -> Result<Vec<AvailableJavaBuild>> {
    let os = adoptium_os()?;
    let arch = adoptium_arch()?;
    let majors = [21u32, 17, 11, 8];

    let results: Vec<Vec<AvailableJavaBuild>> = stream::iter(majors)
        .map(|major| {
            let client = client.clone();
            async move {
                let url = format!(
                    "https://api.adoptium.net/v3/assets/feature_releases/{major}/ga?architecture={arch}&os={os}&jvm_impl=openj9&vendor=ibm&page_size=2"
                );
                let Ok(resp) = client.get(&url).send().await else {
                    return Vec::new();
                };
                let Ok(resp) = resp.error_for_status() else {
                    return Vec::new();
                };
                let Ok(releases) = resp.json::<Vec<SemeruRelease>>().await else {
                    return Vec::new();
                };

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
                            provider: JavaProviderKind::Semeru,
                            distribution: "IBM Semeru Open".to_string(),
                            component: None,
                            major_version: rel.version_data.major,
                            full_version: rel.version_data.semver.clone(),
                            build_number: rel.version_data.build.map(|b| format!("+{b}")),
                            release_date: released.clone(),
                            arch: bin.architecture,
                            os: bin.os,
                            package_type: pkg_type,
                            jvm_impl: Some("openj9".to_string()),
                            download_url: bin.package.link,
                            download_size: Some(bin.package.size),
                            checksum: bin.package.checksum,
                            is_recommended: matches!(major, 17 | 21)
                                && pkg_type == JavaPackageType::Jre,
                            is_experimental: false,
                        });
                    }
                }
                out
            }
        })
        .buffer_unordered(4)
        .collect()
        .await;

    let mut builds: Vec<AvailableJavaBuild> = results.into_iter().flatten().collect();
    builds.sort_by_key(|a| std::cmp::Reverse(a.major_version));
    Ok(builds)
}
