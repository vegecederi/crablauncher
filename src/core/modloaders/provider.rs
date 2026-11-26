use crate::core::components::model::ModloaderKind;
use crate::core::modloaders::{fabric, forge, liteloader, neoforge, quilt};
use crate::error::Result;
use reqwest::Client;

/// Versão disponível de um modloader normalizada para exibição e seleção.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AvailableLoaderVersion {
    pub loader: ModloaderKind,
    pub version: String,
    pub minecraft_version: String,
    pub stable: bool,
    pub recommended: bool,
    pub release_date: Option<String>,
}

/// Consulta as versões disponíveis de um modloader compatíveis com `mc_version`.
pub async fn fetch_loader_versions(
    client: &Client,
    kind: ModloaderKind,
    mc_version: &str,
) -> Result<Vec<AvailableLoaderVersion>> {
    match kind {
        ModloaderKind::Fabric => fabric::fetch_fabric_versions(client, mc_version).await,
        ModloaderKind::Forge => forge::fetch_forge_versions(client, mc_version).await,
        ModloaderKind::NeoForge => neoforge::fetch_neoforge_versions(client, mc_version).await,
        ModloaderKind::Quilt => quilt::fetch_quilt_versions(client, mc_version).await,
        ModloaderKind::LiteLoader => {
            liteloader::fetch_liteloader_versions(client, mc_version).await
        }
    }
}
