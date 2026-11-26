use crate::core::components::model::ModloaderKind;
use crate::core::modloaders::provider::AvailableLoaderVersion;
use crate::error::Result;
use reqwest::Client;

const NEOFORGE_MAVEN_METADATA_URL: &str =
    "https://maven.neoforged.net/releases/net/neoforged/neoforge/maven-metadata.xml";
pub const NEOFORGE_MAVEN_BASE: &str = "https://maven.neoforged.net/releases";

/// Consulta builds do NeoForge compatíveis com `mc_version` a partir do repositório Maven oficial.
pub async fn fetch_neoforge_versions(
    client: &Client,
    mc_version: &str,
) -> Result<Vec<AvailableLoaderVersion>> {
    let xml = client
        .get(NEOFORGE_MAVEN_METADATA_URL)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;

    Ok(parse_neoforge_maven_versions(&xml, mc_version))
}

/// Filtra versões do NeoForge compatíveis com `mc_version` (ex.: `1.21.1` -> prefixo `21.1.`).
pub fn parse_neoforge_maven_versions(xml: &str, mc_version: &str) -> Vec<AvailableLoaderVersion> {
    let Some(nf_prefix) = neoforge_prefix_for_mc(mc_version) else {
        return Vec::new();
    };

    let mut versions = Vec::new();
    for segment in xml.split("<version>").skip(1) {
        if let Some((raw, _)) = segment.split_once("</version>") {
            let trimmed = raw.trim();
            if trimmed.starts_with(&nf_prefix) {
                let is_beta = trimmed.contains("-beta") || trimmed.contains("-alpha");
                versions.push(AvailableLoaderVersion {
                    loader: ModloaderKind::NeoForge,
                    version: trimmed.to_string(),
                    minecraft_version: mc_version.to_string(),
                    stable: !is_beta,
                    recommended: false,
                    release_date: None,
                });
            }
        }
    }

    versions.reverse();
    if let Some(first_stable) = versions.iter_mut().find(|v| v.stable) {
        first_stable.recommended = true;
    }
    versions
}

/// Converte `1.<minor>[.<patch>]` no prefixo oficial de versão do NeoForge (`<minor>.<patch>.`).
fn neoforge_prefix_for_mc(mc_version: &str) -> Option<String> {
    let rest = mc_version.strip_prefix("1.")?;
    let mut parts = rest.split('.');
    let minor: u32 = parts.next()?.parse().ok()?;
    if minor < 20 {
        return None;
    }
    let patch: u32 = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
    Some(format!("{minor}.{patch}."))
}

pub fn neoforge_installer_url(neoforge_version: &str) -> String {
    format!(
        "{NEOFORGE_MAVEN_BASE}/net/neoforged/neoforge/{neoforge_version}/neoforge-{neoforge_version}-installer.jar"
    )
}
