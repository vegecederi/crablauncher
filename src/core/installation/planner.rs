use crate::core::minecraft::assets::{AssetIndex, MINECRAFT_RESOURCES_BASE_URL};
use crate::core::minecraft::libraries::{resolve_libraries, TargetPlatform};
use crate::core::minecraft::metadata::VersionMetadata;
use crate::error::Result;
use crate::platform::paths::AppPaths;
use crate::services::download::DownloadRequest;
use crate::services::integrity::{verify_file, FileIntegrity};
use std::collections::HashMap;
use std::path::PathBuf;

/// Categoria de arquivo dentro do plano de instalação.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlannedItemKind {
    ClientJar,
    Library,
    NativeLibrary,
    AssetObject,
}

/// Item individual planejado para verificação ou download.
#[derive(Debug, Clone)]
pub struct PlannedDownload {
    pub kind: PlannedItemKind,
    pub request: DownloadRequest,
    pub local_state: FileIntegrity,
}

/// Tarefa de extração de biblioteca nativa.
#[derive(Debug, Clone)]
pub struct PlannedNativeExtraction {
    pub archive_path: PathBuf,
    pub dest_dir: PathBuf,
    pub exclude_prefixes: Vec<String>,
}

/// Plano completo de instalação de uma versão do Minecraft Vanilla,
/// separando o planejamento da execução de downloads.
#[derive(Debug, Clone)]
pub struct InstallationPlan {
    pub version_id: String,
    pub platform_tag: String,
    pub items: Vec<PlannedDownload>,
    pub native_extractions: Vec<PlannedNativeExtraction>,
}

impl InstallationPlan {
    /// Itens que estão ausentes ou corrompidos e precisam ser baixados.
    pub fn pending_downloads(&self) -> Vec<&PlannedDownload> {
        self.items
            .iter()
            .filter(|item| item.local_state != FileIntegrity::Valid)
            .collect()
    }

    pub fn valid_count(&self) -> usize {
        self.items
            .iter()
            .filter(|item| item.local_state == FileIntegrity::Valid)
            .count()
    }

    pub fn missing_count(&self) -> usize {
        self.items
            .iter()
            .filter(|item| item.local_state == FileIntegrity::Missing)
            .count()
    }

    pub fn corrupted_count(&self) -> usize {
        self.items
            .iter()
            .filter(|item| item.local_state == FileIntegrity::Corrupted)
            .count()
    }

    pub fn total_pending_bytes(&self) -> u64 {
        self.pending_downloads()
            .iter()
            .filter_map(|item| item.request.expected_size)
            .sum()
    }
}

/// Constrói planos de instalação determinísticos a partir dos metadados oficiais.
pub struct InstallationPlanner {
    paths: AppPaths,
    platform: TargetPlatform,
    resources_base_url: String,
}

impl InstallationPlanner {
    pub fn new(paths: AppPaths, platform: TargetPlatform) -> Self {
        Self {
            paths,
            platform,
            resources_base_url: MINECRAFT_RESOURCES_BASE_URL.to_string(),
        }
    }

    /// Permite sobrescrever a URL base de assets em testes com servidor HTTP local.
    pub fn with_resources_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.resources_base_url = base_url.into();
        self
    }

    /// Monta o plano completo e verifica o estado local (válido, ausente ou corrompido) de cada arquivo.
    pub async fn build_plan(
        &self,
        metadata: &VersionMetadata,
        asset_index: &AssetIndex,
    ) -> Result<InstallationPlan> {
        let mut items = Vec::new();
        let mut native_extractions = Vec::new();
        let platform_tag = self.platform.tag();
        let natives_dir = self.paths.version_natives_dir(&metadata.id, &platform_tag);

        // 1. Cliente do Minecraft
        let client_dest = self.paths.version_jar_file(&metadata.id);
        let client_state = verify_file(
            &client_dest,
            metadata.downloads.client.sha1.as_deref(),
            metadata.downloads.client.size,
        )
        .await?;
        items.push(PlannedDownload {
            kind: PlannedItemKind::ClientJar,
            request: DownloadRequest {
                url: metadata.downloads.client.url.clone(),
                destination: client_dest,
                expected_sha1: metadata.downloads.client.sha1.clone(),
                expected_size: metadata.downloads.client.size,
            },
            local_state: client_state,
        });

        // 2. Bibliotecas Java e nativas
        let resolved_libs = resolve_libraries(&metadata.libraries, &self.platform, &HashMap::new());
        for lib in resolved_libs {
            let dest = self.paths.library_file(&lib.relative_path);
            let local_state = verify_file(&dest, lib.sha1.as_deref(), lib.size).await?;

            let kind = if lib.is_native {
                native_extractions.push(PlannedNativeExtraction {
                    archive_path: dest.clone(),
                    dest_dir: natives_dir.clone(),
                    exclude_prefixes: lib.extract_exclude,
                });
                PlannedItemKind::NativeLibrary
            } else {
                PlannedItemKind::Library
            };

            // Artefatos gerados localmente por processors do Forge/NeoForge possuem URL vazia.
            if lib.url.is_empty() && dest.exists() {
                continue;
            }

            items.push(PlannedDownload {
                kind,
                request: DownloadRequest {
                    url: lib.url,
                    destination: dest,
                    expected_sha1: lib.sha1,
                    expected_size: lib.size,
                },
                local_state,
            });
        }

        // 3. Objetos de assets deduplicados por hash
        let resolved_assets = asset_index.deduplicated_objects_with_base(&self.resources_base_url);
        for asset in resolved_assets {
            let dest = self.paths.asset_object_file(&asset.hash);
            let local_state = verify_file(&dest, Some(&asset.hash), Some(asset.size)).await?;
            items.push(PlannedDownload {
                kind: PlannedItemKind::AssetObject,
                request: DownloadRequest {
                    url: asset.url,
                    destination: dest,
                    expected_sha1: Some(asset.hash),
                    expected_size: Some(asset.size),
                },
                local_state,
            });
        }

        Ok(InstallationPlan {
            version_id: metadata.id.clone(),
            platform_tag,
            items,
            native_extractions,
        })
    }
}
