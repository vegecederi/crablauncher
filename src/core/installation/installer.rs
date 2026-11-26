use crate::core::installation::planner::{
    InstallationPlan, InstallationPlanner, PlannedDownload, PlannedItemKind,
};
use crate::core::installation::progress::{InstallProgress, InstallStage};
use crate::core::instance::InstallationStatus;
use crate::core::minecraft::assets::{AssetIndex, MINECRAFT_RESOURCES_BASE_URL};
use crate::core::minecraft::libraries::TargetPlatform;
use crate::core::minecraft::metadata::VersionMetadata;
use crate::core::minecraft::natives::extract_native_archive;
use crate::error::{LauncherError, Result};
use crate::platform::paths::AppPaths;
use crate::services::download::{DownloadManager, DownloadRequest};
use crate::services::instance_manager::InstanceManager;
use crate::services::manifest_service::ManifestService;
use crate::services::storage::read_json;
use futures_util::stream::{self, StreamExt};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

/// Orquestra a instalação e reparação de instâncias do Minecraft Vanilla
/// de forma totalmente desacoplada da interface gráfica.
#[derive(Clone)]
pub struct InstanceInstaller {
    paths: AppPaths,
    manifest_service: ManifestService,
    instance_manager: InstanceManager,
    download_manager: DownloadManager,
    platform: TargetPlatform,
    resources_base_url: String,
    max_concurrent: usize,
}

impl InstanceInstaller {
    pub fn new(
        paths: AppPaths,
        manifest_service: ManifestService,
        instance_manager: InstanceManager,
        download_manager: DownloadManager,
        max_concurrent: usize,
    ) -> Self {
        Self {
            paths,
            manifest_service,
            instance_manager,
            download_manager,
            platform: TargetPlatform::current(),
            resources_base_url: MINECRAFT_RESOURCES_BASE_URL.to_string(),
            max_concurrent: max_concurrent.max(1),
        }
    }

    pub fn with_resources_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.resources_base_url = base_url.into();
        self
    }

    /// Executa a instalação (ou reparação) completa da instância, atualizando
    /// `instance.json` ao iniciar e ao finalizar (sucesso, cancelamento ou falha).
    pub async fn install_instance<F>(
        &self,
        instance_id: &str,
        cancel: CancellationToken,
        on_progress: F,
    ) -> Result<()>
    where
        F: Fn(InstallProgress) + Send + Sync + 'static,
    {
        let on_progress: Arc<dyn Fn(InstallProgress) + Send + Sync> = Arc::new(on_progress);
        let instance = self.instance_manager.find_by_id(instance_id).await?;

        self.instance_manager
            .update_installation_status(instance_id, InstallationStatus::Installing, None)
            .await?;

        let result = self
            .run_pipeline(
                instance_id,
                &instance.minecraft_version,
                &cancel,
                Arc::clone(&on_progress),
            )
            .await;

        match result {
            Ok(()) => {
                self.instance_manager
                    .update_installation_status(instance_id, InstallationStatus::Installed, None)
                    .await?;
                on_progress(InstallProgress {
                    instance_id: instance_id.to_string(),
                    stage: InstallStage::Complete,
                    message: InstallStage::Complete.message().to_string(),
                    completed_files: 1,
                    total_files: 1,
                    downloaded_bytes: 0,
                    total_bytes: 0,
                    bytes_per_sec: 0,
                    indeterminate: false,
                });
                info!(
                    "Successfully installed Minecraft {} for instance '{}'",
                    instance.minecraft_version, instance.name
                );
                Ok(())
            }
            Err(LauncherError::Cancelled) => {
                let _ = self
                    .instance_manager
                    .update_installation_status(
                        instance_id,
                        InstallationStatus::Incomplete,
                        Some("Installation was cancelled.".to_string()),
                    )
                    .await;
                Err(LauncherError::Cancelled)
            }
            Err(err) => {
                error!(
                    "Installation failed for instance '{}': {err}",
                    instance.name
                );
                let _ = self
                    .instance_manager
                    .update_installation_status(
                        instance_id,
                        InstallationStatus::Failed,
                        Some(err.to_string()),
                    )
                    .await;
                Err(err)
            }
        }
    }

    async fn run_pipeline(
        &self,
        instance_id: &str,
        version_id: &str,
        cancel: &CancellationToken,
        on_progress: Arc<dyn Fn(InstallProgress) + Send + Sync>,
    ) -> Result<()> {
        // 1. Resolver metadados da versão
        on_progress(InstallProgress::indeterminate(
            instance_id,
            InstallStage::ResolvingMetadata,
        ));

        let loaded_manifest = self.manifest_service.fetch_or_cached().await?;
        let manifest_entry = loaded_manifest
            .manifest
            .find_version(version_id)
            .ok_or_else(|| LauncherError::VersionNotFound(version_id.to_string()))?;

        let version_json_path = self.paths.version_metadata_file(version_id);
        let version_json_req = DownloadRequest {
            url: manifest_entry.url.clone(),
            destination: version_json_path.clone(),
            expected_sha1: manifest_entry.sha1.clone(),
            expected_size: None,
        };
        self.download_manager
            .download_file(&version_json_req, cancel, |_| {})
            .await?;

        let version_metadata: VersionMetadata = read_json(&version_json_path).await?;

        // 2. Baixar / reutilizar asset index
        let asset_index_path = self
            .paths
            .asset_index_file(&version_metadata.asset_index.id);
        let asset_index_req = DownloadRequest {
            url: version_metadata.asset_index.url.clone(),
            destination: asset_index_path.clone(),
            expected_sha1: Some(version_metadata.asset_index.sha1.clone()),
            expected_size: Some(version_metadata.asset_index.size),
        };
        self.download_manager
            .download_file(&asset_index_req, cancel, |_| {})
            .await?;

        let asset_index: AssetIndex = read_json(&asset_index_path).await?;

        // 3. Construir plano e verificar arquivos locais
        on_progress(InstallProgress::indeterminate(
            instance_id,
            InstallStage::CheckingLocalFiles,
        ));

        let planner = InstallationPlanner::new(self.paths.clone(), self.platform.clone())
            .with_resources_base_url(self.resources_base_url.clone());
        let plan = planner.build_plan(&version_metadata, &asset_index).await?;

        info!(
            "Installation plan for {}: {} valid, {} missing, {} corrupted",
            version_id,
            plan.valid_count(),
            plan.missing_count(),
            plan.corrupted_count()
        );

        // 4. Executar downloads pendentes
        self.execute_downloads(instance_id, &plan, cancel, Arc::clone(&on_progress))
            .await?;

        // 5. Extrair bibliotecas nativas (se houver)
        if !plan.native_extractions.is_empty() {
            if cancel.is_cancelled() {
                return Err(LauncherError::Cancelled);
            }
            on_progress(InstallProgress::indeterminate(
                instance_id,
                InstallStage::ExtractingNatives,
            ));

            let extractions = plan.native_extractions.clone();
            tokio::task::spawn_blocking(move || -> Result<()> {
                for task in extractions {
                    extract_native_archive(
                        &task.archive_path,
                        &task.dest_dir,
                        &task.exclude_prefixes,
                    )?;
                }
                Ok(())
            })
            .await
            .map_err(|err| LauncherError::ArchiveExtract {
                path: self.paths.version_dir(version_id),
                reason: err.to_string(),
            })??;
        }

        // 6. Verificação final
        on_progress(InstallProgress::indeterminate(
            instance_id,
            InstallStage::Verifying,
        ));

        Ok(())
    }

    async fn execute_downloads(
        &self,
        instance_id: &str,
        plan: &InstallationPlan,
        cancel: &CancellationToken,
        on_progress: Arc<dyn Fn(InstallProgress) + Send + Sync>,
    ) -> Result<()> {
        let pending: Vec<PlannedDownload> = plan.pending_downloads().into_iter().cloned().collect();
        if pending.is_empty() {
            return Ok(());
        }

        let total_files = pending.len();
        let total_bytes = plan.total_pending_bytes();
        let completed_files = Arc::new(AtomicUsize::new(0));
        let downloaded_bytes = Arc::new(AtomicU64::new(0));
        let started_at = Instant::now();
        let last_emit = Arc::new(Mutex::new(Instant::now() - Duration::from_secs(1)));

        let initial_stage = stage_for_kind(pending[0].kind);
        on_progress(InstallProgress {
            instance_id: instance_id.to_string(),
            stage: initial_stage,
            message: initial_stage.message().to_string(),
            completed_files: 0,
            total_files,
            downloaded_bytes: 0,
            total_bytes,
            bytes_per_sec: 0,
            indeterminate: false,
        });

        let results: Vec<Result<()>> = stream::iter(pending)
            .map(|item| {
                let dm = self.download_manager.clone();
                let cancel = cancel.clone();
                let completed_files = Arc::clone(&completed_files);
                let downloaded_bytes = Arc::clone(&downloaded_bytes);
                let last_emit = Arc::clone(&last_emit);
                let on_progress = Arc::clone(&on_progress);
                let instance_id = instance_id.to_string();

                async move {
                    let stage = stage_for_kind(item.kind);
                    let dl_bytes_ref = Arc::clone(&downloaded_bytes);
                    let completed_ref = Arc::clone(&completed_files);
                    let last_emit_ref = Arc::clone(&last_emit);
                    let on_progress_ref = Arc::clone(&on_progress);
                    let id_ref = instance_id.clone();

                    dm.download_file(&item.request, &cancel, move |chunk_len| {
                        let current_bytes =
                            dl_bytes_ref.fetch_add(chunk_len, Ordering::Relaxed) + chunk_len;
                        // Limita atualizações da interface para no máximo ~10x por segundo.
                        if let Ok(mut guard) = last_emit_ref.try_lock() {
                            if guard.elapsed() >= Duration::from_millis(100) {
                                *guard = Instant::now();
                                let elapsed = started_at.elapsed().as_secs_f64().max(0.05);
                                let speed = (current_bytes as f64 / elapsed) as u64;
                                on_progress_ref(InstallProgress {
                                    instance_id: id_ref.clone(),
                                    stage,
                                    message: stage.message().to_string(),
                                    completed_files: completed_ref.load(Ordering::Relaxed),
                                    total_files,
                                    downloaded_bytes: current_bytes,
                                    total_bytes,
                                    bytes_per_sec: speed,
                                    indeterminate: false,
                                });
                            }
                        }
                    })
                    .await?;

                    let done = completed_files.fetch_add(1, Ordering::Relaxed) + 1;
                    let current_bytes = downloaded_bytes.load(Ordering::Relaxed);
                    let elapsed = started_at.elapsed().as_secs_f64().max(0.05);
                    let speed = (current_bytes as f64 / elapsed) as u64;

                    if let Ok(mut guard) = last_emit.try_lock() {
                        if done == total_files || guard.elapsed() >= Duration::from_millis(100) {
                            *guard = Instant::now();
                            on_progress(InstallProgress {
                                instance_id,
                                stage,
                                message: stage.message().to_string(),
                                completed_files: done,
                                total_files,
                                downloaded_bytes: current_bytes,
                                total_bytes,
                                bytes_per_sec: speed,
                                indeterminate: false,
                            });
                        }
                    }

                    Ok(())
                }
            })
            .buffer_unordered(self.max_concurrent)
            .collect()
            .await;

        for res in results {
            res?;
        }

        Ok(())
    }
}

fn stage_for_kind(kind: PlannedItemKind) -> InstallStage {
    match kind {
        PlannedItemKind::ClientJar => InstallStage::DownloadingClient,
        PlannedItemKind::Library | PlannedItemKind::NativeLibrary => {
            InstallStage::DownloadingLibraries
        }
        PlannedItemKind::AssetObject => InstallStage::DownloadingAssets,
    }
}
