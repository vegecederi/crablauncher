use crate::core::version::{MinecraftVersion, VersionType};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Estrutura raiz do manifesto oficial `version_manifest_v2.json` da Mojang.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionManifest {
    pub latest: LatestVersions,
    pub versions: Vec<ManifestVersionEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatestVersions {
    pub release: String,
    pub snapshot: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestVersionEntry {
    pub id: String,
    #[serde(rename = "type")]
    pub version_type: VersionType,
    pub url: String,
    pub time: DateTime<Utc>,
    pub release_time: DateTime<Utc>,
    #[serde(default)]
    pub sha1: Option<String>,
    #[serde(default)]
    pub compliance_level: Option<u32>,
}

impl VersionManifest {
    /// Retorna as versões suportadas nesta fase (release e snapshot), ordenadas da mais recente para a mais antiga.
    pub fn supported_versions(&self) -> Vec<MinecraftVersion> {
        let mut list: Vec<MinecraftVersion> = self
            .versions
            .iter()
            .filter(|entry| entry.version_type.is_supported_phase1())
            .map(|entry| MinecraftVersion {
                id: entry.id.clone(),
                version_type: entry.version_type,
                release_time: entry.release_time,
                url: entry.url.clone(),
            })
            .collect();

        list.sort_by_key(|a| std::cmp::Reverse(a.release_time));
        list
    }

    /// Retorna todas as versões do manifesto (releases, snapshots, old_beta, old_alpha),
    /// ordenadas da mais recente para a mais antiga.
    pub fn all_versions(&self) -> Vec<MinecraftVersion> {
        let mut list: Vec<MinecraftVersion> = self
            .versions
            .iter()
            .filter(|entry| entry.version_type != VersionType::Unknown)
            .map(|entry| MinecraftVersion {
                id: entry.id.clone(),
                version_type: entry.version_type,
                release_time: entry.release_time,
                url: entry.url.clone(),
            })
            .collect();

        list.sort_by_key(|a| std::cmp::Reverse(a.release_time));
        list
    }

    /// Localiza uma versão específica no manifesto pelo seu identificador.
    pub fn find_version(&self, id: &str) -> Option<&ManifestVersionEntry> {
        self.versions.iter().find(|entry| entry.id == id)
    }
}

/// Origem dos dados do manifesto carregado.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManifestSource {
    Network,
    CacheFallback,
}

/// Resultado do carregamento do manifesto de versões.
#[derive(Debug, Clone)]
pub struct LoadedManifest {
    pub manifest: VersionManifest,
    pub source: ManifestSource,
}
