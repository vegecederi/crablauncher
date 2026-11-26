use crate::core::components::model::ModloaderKind;
use crate::core::components::profile::LoaderProfile;
use crate::core::modloaders::provider::AvailableLoaderVersion;
use crate::error::Result;
use reqwest::Client;
use serde::Deserialize;

const FABRIC_META_BASE: &str = "https://meta.fabricmc.net/v2";

#[derive(Debug, Deserialize)]
struct FabricLoaderEntry {
    loader: FabricLoaderInfo,
}

#[derive(Debug, Deserialize)]
struct FabricLoaderInfo {
    version: String,
    stable: bool,
}

/// Verifica previamente se a versão do Minecraft é suportada pelo Fabric (exige Minecraft >= 1.14),
/// evitando requisições HTTP 400 desnecessárias à API `meta.fabricmc.net` para versões antigas (ex.: b1.7.3).
pub fn is_fabric_compatible_mc_version(mc_version: &str) -> bool {
    let v = mc_version.trim();
    if v.is_empty()
        || v.starts_with("b1.")
        || v.starts_with("a1.")
        || v.starts_with("c0.")
        || v.starts_with("rd-")
        || v.starts_with("inf-")
    {
        return false;
    }
    if let Some(rest) = v.strip_prefix("1.") {
        if let Some(minor) = rest.split('.').next().and_then(|s| s.parse::<u32>().ok()) {
            return minor >= 14;
        }
    }
    true
}

/// Consulta as versões do Fabric Loader compatíveis com a versão do Minecraft informada.
pub async fn fetch_fabric_versions(
    client: &Client,
    mc_version: &str,
) -> Result<Vec<AvailableLoaderVersion>> {
    if !is_fabric_compatible_mc_version(mc_version) {
        return Ok(Vec::new());
    }
    let url = format!("{FABRIC_META_BASE}/versions/loader/{mc_version}");
    let entries: Vec<FabricLoaderEntry> = client
        .get(&url)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let mut first_stable_marked = false;
    let mut out = Vec::with_capacity(entries.len());
    for entry in entries {
        let recommended = if entry.loader.stable && !first_stable_marked {
            first_stable_marked = true;
            true
        } else {
            false
        };
        out.push(AvailableLoaderVersion {
            loader: ModloaderKind::Fabric,
            version: entry.loader.version,
            minecraft_version: mc_version.to_string(),
            stable: entry.loader.stable,
            recommended,
            release_date: None,
        });
    }
    Ok(out)
}

/// Obtém o perfil JSON oficial do Fabric para `mc_version` + `loader_version`.
pub async fn fetch_fabric_profile(
    client: &Client,
    mc_version: &str,
    loader_version: &str,
) -> Result<LoaderProfile> {
    let url =
        format!("{FABRIC_META_BASE}/versions/loader/{mc_version}/{loader_version}/profile/json");
    let profile: LoaderProfile = client
        .get(&url)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(profile)
}
