use crate::core::components::model::ModloaderKind;
use crate::core::modloaders::provider::AvailableLoaderVersion;
use crate::error::Result;
use reqwest::Client;
use serde::Deserialize;
use std::collections::HashMap;

const FORGE_PROMOTIONS_URL: &str =
    "https://files.minecraftforge.net/net/minecraftforge/forge/promotions_slim.json";
const FORGE_MAVEN_METADATA_URL: &str =
    "https://maven.minecraftforge.net/net/minecraftforge/forge/maven-metadata.xml";
pub const FORGE_MAVEN_BASE: &str = "https://maven.minecraftforge.net";

#[derive(Debug, Deserialize)]
struct ForgePromotions {
    #[serde(default)]
    promos: HashMap<String, String>,
}

/// Consulta builds do Minecraft Forge compatíveis com `mc_version`,
/// destacando a build recomendada oficial de `promotions_slim.json`.
pub async fn fetch_forge_versions(
    client: &Client,
    mc_version: &str,
) -> Result<Vec<AvailableLoaderVersion>> {
    let promos: ForgePromotions = client
        .get(FORGE_PROMOTIONS_URL)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await
        .unwrap_or(ForgePromotions {
            promos: HashMap::new(),
        });

    let recommended_ver = promos
        .promos
        .get(&format!("{mc_version}-recommended"))
        .cloned();

    let xml = client
        .get(FORGE_MAVEN_METADATA_URL)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;

    Ok(parse_forge_maven_versions(
        &xml,
        mc_version,
        recommended_ver.as_deref(),
    ))
}

/// Extrai e normaliza versões do Forge a partir do `maven-metadata.xml` oficial.
pub fn parse_forge_maven_versions(
    xml: &str,
    mc_version: &str,
    recommended_ver: Option<&str>,
) -> Vec<AvailableLoaderVersion> {
    let prefix = format!("{mc_version}-");
    let mut versions = Vec::new();

    for segment in xml.split("<version>").skip(1) {
        if let Some((raw, _)) = segment.split_once("</version>") {
            let trimmed = raw.trim();
            if let Some(forge_ver) = trimmed.strip_prefix(&prefix) {
                let is_rec = recommended_ver == Some(forge_ver);
                versions.push(AvailableLoaderVersion {
                    loader: ModloaderKind::Forge,
                    version: forge_ver.to_string(),
                    minecraft_version: mc_version.to_string(),
                    stable: true,
                    recommended: is_rec,
                    release_date: None,
                });
            }
        }
    }

    versions.reverse();
    versions
}

pub fn forge_installer_url(mc_version: &str, forge_version: &str) -> String {
    format!(
        "{FORGE_MAVEN_BASE}/net/minecraftforge/forge/{mc_version}-{forge_version}/forge-{mc_version}-{forge_version}-installer.jar"
    )
}
