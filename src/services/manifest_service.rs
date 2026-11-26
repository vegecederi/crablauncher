use crate::core::manifest::{LoadedManifest, ManifestSource, VersionManifest};
use crate::error::{LauncherError, Result};
use crate::platform::paths::AppPaths;
use crate::services::storage::{read_json, write_json_atomic};
use reqwest::Client;
use tracing::{info, warn};

pub const MOJANG_MANIFEST_URL: &str =
    "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";

/// Serviço responsável por consultar e armazenar em cache o manifesto oficial do Minecraft.
#[derive(Clone)]
pub struct ManifestService {
    client: Client,
    paths: AppPaths,
    manifest_url: String,
}

impl ManifestService {
    pub fn new(client: Client, paths: AppPaths) -> Self {
        Self {
            client,
            paths,
            manifest_url: MOJANG_MANIFEST_URL.to_string(),
        }
    }

    /// Permite sobrescrever a URL do manifesto em testes automatizados.
    pub fn with_url(client: Client, paths: AppPaths, manifest_url: impl Into<String>) -> Self {
        Self {
            client,
            paths,
            manifest_url: manifest_url.into(),
        }
    }

    /// Busca o manifesto online e atualiza o cache local.
    /// Em caso de falha de rede ou resposta inválida, faz fallback para o cache local.
    pub async fn fetch_or_cached(&self) -> Result<LoadedManifest> {
        match self.fetch_remote().await {
            Ok(manifest) => {
                if let Err(err) =
                    write_json_atomic(&self.paths.manifest_cache_file(), &manifest).await
                {
                    warn!("Failed to update local manifest cache: {err}");
                }
                info!(
                    "Loaded Minecraft version manifest from network ({} entries)",
                    manifest.versions.len()
                );
                Ok(LoadedManifest {
                    manifest,
                    source: ManifestSource::Network,
                })
            }
            Err(network_err) => {
                warn!("Failed to fetch remote manifest: {network_err}. Trying local cache.");
                match self.load_cached().await {
                    Ok(manifest) => {
                        info!(
                            "Loaded Minecraft version manifest from local cache ({} entries)",
                            manifest.versions.len()
                        );
                        Ok(LoadedManifest {
                            manifest,
                            source: ManifestSource::CacheFallback,
                        })
                    }
                    Err(cache_err) => {
                        warn!("Local manifest cache unavailable or invalid: {cache_err}");
                        Err(LauncherError::ManifestUnavailable {
                            network: network_err.to_string(),
                        })
                    }
                }
            }
        }
    }

    async fn fetch_remote(&self) -> Result<VersionManifest> {
        let response = self
            .client
            .get(&self.manifest_url)
            .send()
            .await?
            .error_for_status()?;

        let manifest = response.json::<VersionManifest>().await?;
        Ok(manifest)
    }

    async fn load_cached(&self) -> Result<VersionManifest> {
        read_json(&self.paths.manifest_cache_file()).await
    }
}
