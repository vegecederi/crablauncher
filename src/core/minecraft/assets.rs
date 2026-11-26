use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

pub const MINECRAFT_RESOURCES_BASE_URL: &str = "https://resources.download.minecraft.net";

/// Índice oficial de assets (`assets/indexes/<id>.json`).
/// Preserva campos de layouts legados (`virtual`, `map_to_resources`) para evolução futura.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetIndex {
    pub objects: HashMap<String, AssetObject>,
    #[serde(default, rename = "virtual")]
    pub is_virtual: bool,
    #[serde(default)]
    pub map_to_resources: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetObject {
    pub hash: String,
    pub size: u64,
}

/// Objeto de asset deduplicado por hash, pronto para planejamento de download.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedAsset {
    pub hash: String,
    pub size: u64,
    pub url: String,
}

impl AssetIndex {
    /// Retorna os objetos de assets deduplicados por hash SHA-1.
    pub fn deduplicated_objects(&self) -> Vec<ResolvedAsset> {
        self.deduplicated_objects_with_base(MINECRAFT_RESOURCES_BASE_URL)
    }

    /// Permite sobrescrever a URL base em testes determinísticos com servidor HTTP local.
    pub fn deduplicated_objects_with_base(&self, base_url: &str) -> Vec<ResolvedAsset> {
        let mut seen = HashSet::with_capacity(self.objects.len());
        let mut result = Vec::with_capacity(self.objects.len());
        let base = base_url.trim_end_matches('/');

        for obj in self.objects.values() {
            if obj.hash.len() < 2 || !seen.insert(obj.hash.clone()) {
                continue;
            }
            let prefix = &obj.hash[0..2];
            result.push(ResolvedAsset {
                url: format!("{base}/{prefix}/{}", obj.hash),
                hash: obj.hash.clone(),
                size: obj.size,
            });
        }

        result
    }
}
