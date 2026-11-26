use crate::core::components::model::ModloaderKind;
use crate::core::components::profile::LoaderProfile;
use crate::core::minecraft::metadata::{ArgumentEntry, Library, VersionArguments};
use crate::core::modloaders::provider::AvailableLoaderVersion;
use crate::error::{LauncherError, Result};
use reqwest::Client;
use serde::Deserialize;
use std::collections::HashMap;

const LITELOADER_VERSIONS_URL: &str = "https://dl.liteloader.com/versions/versions.json";

#[derive(Debug, Deserialize)]
struct LiteLoaderManifest {
    #[serde(default)]
    versions: HashMap<String, LiteLoaderMcVersion>,
}

#[derive(Debug, Deserialize)]
struct LiteLoaderMcVersion {
    #[serde(default)]
    repo: Option<LiteLoaderRepo>,
    #[serde(default)]
    artefacts: HashMap<String, HashMap<String, LiteLoaderArtefact>>,
    #[serde(default)]
    snapshots: Option<LiteLoaderSnapshots>,
}

#[derive(Debug, Deserialize)]
struct LiteLoaderRepo {
    url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LiteLoaderSnapshots {
    #[serde(default)]
    libraries: Vec<LiteLoaderLib>,
    #[serde(default, rename = "com.mumfrey:liteloader")]
    liteloader: HashMap<String, LiteLoaderArtefact>,
}

#[derive(Debug, Deserialize)]
struct LiteLoaderArtefact {
    version: String,
    #[serde(default)]
    file: String,
    #[serde(default, rename = "tweakClass")]
    tweak_class: Option<String>,
    #[serde(default)]
    libraries: Vec<LiteLoaderLib>,
}

#[derive(Debug, Deserialize)]
struct LiteLoaderLib {
    name: String,
    #[serde(default)]
    url: Option<String>,
}

/// Consulta as versões disponíveis do LiteLoader compatíveis com `mc_version`.
pub async fn fetch_liteloader_versions(
    client: &Client,
    mc_version: &str,
) -> Result<Vec<AvailableLoaderVersion>> {
    let manifest: LiteLoaderManifest = client
        .get(LITELOADER_VERSIONS_URL)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let Some(entry) = manifest.versions.get(mc_version) else {
        return Ok(Vec::new());
    };
    let artefacts = entry
        .artefacts
        .get("com.mumfrey:liteloader")
        .or_else(|| entry.snapshots.as_ref().map(|s| &s.liteloader));
    let Some(artefacts) = artefacts else {
        return Ok(Vec::new());
    };

    let mut out = Vec::new();
    if let Some(latest) = artefacts.get("latest") {
        out.push(AvailableLoaderVersion {
            loader: ModloaderKind::LiteLoader,
            version: latest.version.clone(),
            minecraft_version: mc_version.to_string(),
            stable: true,
            recommended: true,
            release_date: None,
        });
    }
    for (key, art) in artefacts {
        if key == "latest" {
            continue;
        }
        if !out.iter().any(|v| v.version == art.version) {
            out.push(AvailableLoaderVersion {
                loader: ModloaderKind::LiteLoader,
                version: art.version.clone(),
                minecraft_version: mc_version.to_string(),
                stable: !art.version.contains("SNAPSHOT"),
                recommended: false,
                release_date: None,
            });
        }
    }
    Ok(out)
}

/// Constrói o `LoaderProfile` oficial do LiteLoader para `mc_version` + `loader_version`,
/// incluindo o artefato `com.mumfrey:liteloader`, `launchwrapper`, bibliotecas auxiliares e `--tweakClass`.
pub async fn fetch_liteloader_profile(
    client: &Client,
    mc_version: &str,
    loader_version: &str,
) -> Result<LoaderProfile> {
    let manifest: LiteLoaderManifest = client
        .get(LITELOADER_VERSIONS_URL)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let entry = manifest.versions.get(mc_version).ok_or_else(|| {
        LauncherError::InvalidMetadata(format!(
            "LiteLoader does not support Minecraft {mc_version}"
        ))
    })?;
    let artefacts = entry
        .artefacts
        .get("com.mumfrey:liteloader")
        .or_else(|| entry.snapshots.as_ref().map(|s| &s.liteloader))
        .ok_or_else(|| {
            LauncherError::InvalidMetadata(format!(
                "No LiteLoader artefacts found for Minecraft {mc_version}"
            ))
        })?;
    let art = artefacts
        .values()
        .find(|a| a.version == loader_version)
        .or_else(|| artefacts.get("latest"))
        .ok_or_else(|| {
            LauncherError::InvalidMetadata(format!(
                "LiteLoader version {loader_version} not found for Minecraft {mc_version}"
            ))
        })?;

    let repo_url = entry
        .repo
        .as_ref()
        .and_then(|r| r.url.clone())
        .unwrap_or_else(|| "https://dl.liteloader.com/versions/".to_string());

    let mut libraries = Vec::new();
    // Artefato principal do LiteLoader
    let main_coord = format!("com.mumfrey:liteloader:{}", art.version);
    let main_url = if !art.file.is_empty() {
        Some(format!(
            "https://dl.liteloader.com/versions/com/mumfrey/liteloader/{mc_version}/{}",
            art.file
        ))
    } else {
        Some(repo_url.clone())
    };
    libraries.push(Library {
        name: main_coord,
        downloads: None,
        rules: Vec::new(),
        natives: None,
        extract: None,
        url: main_url,
    });

    for lib in &art.libraries {
        libraries.push(Library {
            name: lib.name.clone(),
            downloads: None,
            rules: Vec::new(),
            natives: None,
            extract: None,
            url: lib.url.clone(),
        });
    }
    if let Some(snaps) = &entry.snapshots {
        for lib in &snaps.libraries {
            if !libraries.iter().any(|l| l.name == lib.name) {
                libraries.push(Library {
                    name: lib.name.clone(),
                    downloads: None,
                    rules: Vec::new(),
                    natives: None,
                    extract: None,
                    url: lib.url.clone(),
                });
            }
        }
    }

    let tweak_class = art
        .tweak_class
        .clone()
        .unwrap_or_else(|| "com.mumfrey.liteloader.launch.LiteLoaderTweaker".to_string());

    Ok(LoaderProfile {
        id: format!("liteloader-{mc_version}-{loader_version}"),
        inherits_from: Some(mc_version.to_string()),
        main_class: Some("net.minecraft.launchwrapper.Launch".to_string()),
        minecraft_arguments: None,
        arguments: Some(VersionArguments {
            game: vec![
                ArgumentEntry::Simple("--tweakClass".to_string()),
                ArgumentEntry::Simple(tweak_class),
            ],
            jvm: Vec::new(),
        }),
        libraries,
    })
}
