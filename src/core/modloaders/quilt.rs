use crate::core::components::model::ModloaderKind;
use crate::core::components::profile::LoaderProfile;
use crate::core::modloaders::fabric::is_fabric_compatible_mc_version;
use crate::core::modloaders::provider::AvailableLoaderVersion;
use crate::error::Result;
use reqwest::Client;
use serde::Deserialize;

const QUILT_META_BASE: &str = "https://meta.quiltmc.org/v3";

#[derive(Debug, Deserialize)]
struct QuiltLoaderEntry {
    loader: QuiltLoaderInfo,
}

#[derive(Debug, Deserialize)]
struct QuiltLoaderInfo {
    version: String,
}

/// Consulta as versões do Quilt Loader compatíveis com `mc_version` na API oficial `meta.quiltmc.org`.
pub async fn fetch_quilt_versions(
    client: &Client,
    mc_version: &str,
) -> Result<Vec<AvailableLoaderVersion>> {
    if !is_fabric_compatible_mc_version(mc_version) {
        return Ok(Vec::new());
    }
    let url = format!("{QUILT_META_BASE}/versions/loader/{mc_version}");
    let resp = client.get(&url).send().await?;
    if resp.status().as_u16() == 400 || resp.status().as_u16() == 404 {
        return Ok(Vec::new());
    }
    let entries: Vec<QuiltLoaderEntry> = resp.error_for_status()?.json().await?;

    let mut first_stable_marked = false;
    let mut out = Vec::with_capacity(entries.len());
    for entry in entries {
        let stable =
            !entry.loader.version.contains("beta") && !entry.loader.version.contains("alpha");
        let recommended = if stable && !first_stable_marked {
            first_stable_marked = true;
            true
        } else {
            false
        };
        out.push(AvailableLoaderVersion {
            loader: ModloaderKind::Quilt,
            version: entry.loader.version,
            minecraft_version: mc_version.to_string(),
            stable,
            recommended,
            release_date: None,
        });
    }
    Ok(out)
}

/// Obtém o perfil JSON oficial do Quilt para `mc_version` + `loader_version`.
pub async fn fetch_quilt_profile(
    client: &Client,
    mc_version: &str,
    loader_version: &str,
) -> Result<LoaderProfile> {
    let url =
        format!("{QUILT_META_BASE}/versions/loader/{mc_version}/{loader_version}/profile/json");
    let profile: LoaderProfile = client
        .get(&url)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(profile)
}
