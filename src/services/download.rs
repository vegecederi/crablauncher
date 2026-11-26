use crate::core::tasks::TaskControl;
use crate::error::{LauncherError, Result};
use crate::services::integrity::{verify_file, FileIntegrity};
use futures_util::StreamExt;
use reqwest::{header, Client, StatusCode};
use sha1::{Digest, Sha1};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use tokio::fs::{self, File, OpenOptions};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{Mutex as AsyncMutex, Notify, Semaphore};
use tokio_util::sync::CancellationToken;
use tracing::{debug, warn};

pub const DEFAULT_MAX_CONCURRENT_DOWNLOADS: usize = 6;
const DEFAULT_MAX_RETRIES: u32 = 1;
const DEFAULT_HTTP_TIMEOUT_SECS: u64 = 60;

static GLOBAL_DOWNLOAD_PAUSED: AtomicBool = AtomicBool::new(false);
static GLOBAL_DOWNLOAD_PAUSE_NOTIFY: OnceLock<Notify> = OnceLock::new();

fn pause_notify() -> &'static Notify {
    GLOBAL_DOWNLOAD_PAUSE_NOTIFY.get_or_init(Notify::new)
}

/// Pausa ou retoma cooperativamente todos os downloads gerenciados pelo DownloadManager.
pub fn set_global_download_paused(paused: bool) {
    GLOBAL_DOWNLOAD_PAUSED.store(paused, Ordering::SeqCst);
    if !paused {
        pause_notify().notify_waiters();
    }
}

pub fn is_global_download_paused() -> bool {
    GLOBAL_DOWNLOAD_PAUSED.load(Ordering::SeqCst)
}

/// Aguarda enquanto os downloads estiverem pausados, acordando imediatamente se cancelado.
pub async fn wait_while_download_paused(cancel: &CancellationToken) -> Result<()> {
    while GLOBAL_DOWNLOAD_PAUSED.load(Ordering::SeqCst) {
        let notified = pause_notify().notified();
        if !GLOBAL_DOWNLOAD_PAUSED.load(Ordering::SeqCst) {
            break;
        }
        tokio::select! {
            _ = cancel.cancelled() => return Err(LauncherError::Cancelled),
            _ = notified => {}
        }
    }
    if cancel.is_cancelled() {
        return Err(LauncherError::Cancelled);
    }
    Ok(())
}

/// Especificação de um único arquivo a ser baixado e validado.
#[derive(Debug, Clone)]
pub struct DownloadRequest {
    pub url: String,
    pub destination: PathBuf,
    pub expected_sha1: Option<String>,
    pub expected_size: Option<u64>,
}

/// Gerenciador centralizado de downloads assíncronos com concorrência limitada,
/// streaming para `.part`, verificação SHA-1, retry com backoff e deduplicação por destino.
#[derive(Clone)]
pub struct DownloadManager {
    client: Client,
    semaphore: Arc<Mutex<Arc<Semaphore>>>,
    max_retries: Arc<AtomicU32>,
    http_timeout_secs: Arc<AtomicU64>,
    /// Impede que dois downloads simultâneos escrevam no mesmo arquivo compartilhado.
    path_locks: Arc<Mutex<HashMap<PathBuf, Arc<AsyncMutex<()>>>>>,
}

impl DownloadManager {
    pub fn new(client: Client, max_concurrent: usize) -> Self {
        let permits = max_concurrent.max(1);
        Self {
            client,
            semaphore: Arc::new(Mutex::new(Arc::new(Semaphore::new(permits)))),
            max_retries: Arc::new(AtomicU32::new(DEFAULT_MAX_RETRIES)),
            http_timeout_secs: Arc::new(AtomicU64::new(DEFAULT_HTTP_TIMEOUT_SECS)),
            path_locks: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Atualiza em tempo real os limites de concorrência, tentativas e timeout HTTP.
    pub fn apply_task_settings(
        &self,
        concurrent_downloads: usize,
        retry_limit: u32,
        timeout_secs: u64,
    ) {
        let permits = concurrent_downloads.clamp(1, 32);
        if let Ok(mut sem) = self.semaphore.lock() {
            *sem = Arc::new(Semaphore::new(permits));
        }
        self.max_retries
            .store(retry_limit.min(10), Ordering::Relaxed);
        self.http_timeout_secs
            .store(timeout_secs.clamp(5, 600), Ordering::Relaxed);
    }

    fn current_semaphore(&self) -> Arc<Semaphore> {
        self.semaphore
            .lock()
            .map(|g| Arc::clone(&g))
            .unwrap_or_else(|_| Arc::new(Semaphore::new(DEFAULT_MAX_CONCURRENT_DOWNLOADS)))
    }

    /// Baixa um arquivo com streaming para `.part`, validação e movimentação atômica.
    /// Se o arquivo já existir e estiver íntegro (inclusive baixado por outra tarefa concorrente),
    /// reutiliza-o sem refazer o download.
    pub async fn download_file<F>(
        &self,
        request: &DownloadRequest,
        cancel: &CancellationToken,
        on_bytes: F,
    ) -> Result<u64>
    where
        F: Fn(u64) + Send + Sync,
    {
        if cancel.is_cancelled() {
            return Err(LauncherError::Cancelled);
        }

        let path_lock = self.lock_for_path(&request.destination);
        let _guard = tokio::select! {
            _ = cancel.cancelled() => return Err(LauncherError::Cancelled),
            g = path_lock.lock() => g,
        };

        // Verifica novamente após adquirir o lock (deduplicação de downloads simultâneos).
        if verify_file(
            &request.destination,
            request.expected_sha1.as_deref(),
            request.expected_size,
        )
        .await?
            == FileIntegrity::Valid
        {
            debug!("Reusing verified file: {}", request.destination.display());
            return Ok(request.expected_size.unwrap_or(0));
        }

        wait_while_download_paused(cancel).await?;

        let sem = self.current_semaphore();
        let _permit = tokio::select! {
            _ = cancel.cancelled() => return Err(LauncherError::Cancelled),
            permit = sem.acquire_owned() => {
                permit.map_err(|_| LauncherError::Cancelled)?
            }
        };

        let mut last_error: Option<LauncherError> = None;
        let max_retries = self.max_retries.load(Ordering::Relaxed);

        for attempt in 0..=max_retries {
            wait_while_download_paused(cancel).await?;

            if attempt > 0 {
                let backoff_ms = 250u64 * (1u64 << (attempt - 1));
                warn!(
                    "Retrying download (attempt {}/{}) for {} after {}ms",
                    attempt, max_retries, request.url, backoff_ms
                );
                tokio::select! {
                    _ = cancel.cancelled() => return Err(LauncherError::Cancelled),
                    _ = tokio::time::sleep(Duration::from_millis(backoff_ms)) => {}
                }
            }

            match self.download_once(request, cancel, &on_bytes).await {
                Ok(bytes) => return Ok(bytes),
                Err(LauncherError::Cancelled) => return Err(LauncherError::Cancelled),
                Err(err) => {
                    last_error = Some(err);
                }
            }
        }

        Err(last_error.unwrap_or_else(|| LauncherError::DownloadFailed {
            url: request.url.clone(),
            reason: "unknown download failure".to_string(),
        }))
    }

    async fn download_once<F>(
        &self,
        request: &DownloadRequest,
        cancel: &CancellationToken,
        on_bytes: &F,
    ) -> Result<u64>
    where
        F: Fn(u64) + Send + Sync,
    {
        if let Some(parent) = request.destination.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|source| LauncherError::Io {
                    path: parent.to_path_buf(),
                    source,
                })?;
        }

        let part_path = part_file_path(&request.destination);

        let timeout = Duration::from_secs(self.http_timeout_secs.load(Ordering::Relaxed));
        let response = tokio::select! {
            _ = cancel.cancelled() => return Err(LauncherError::Cancelled),
            res = self.client.get(&request.url).timeout(timeout).send() => res?,
        };

        let response = response.error_for_status()?;
        let mut stream = response.bytes_stream();

        let mut file = File::create(&part_path)
            .await
            .map_err(|source| LauncherError::Io {
                path: part_path.clone(),
                source,
            })?;

        let mut hasher = Sha1::new();
        let mut total_written: u64 = 0;

        loop {
            if is_global_download_paused() {
                if let Err(err) = wait_while_download_paused(cancel).await {
                    drop(file);
                    let _ = fs::remove_file(&part_path).await;
                    return Err(err);
                }
            }
            let chunk_opt = tokio::select! {
                _ = cancel.cancelled() => {
                    drop(file);
                    let _ = fs::remove_file(&part_path).await;
                    return Err(LauncherError::Cancelled);
                }
                next = stream.next() => next,
            };

            let Some(chunk_res) = chunk_opt else {
                break;
            };

            let chunk = match chunk_res {
                Ok(c) => c,
                Err(err) => {
                    drop(file);
                    let _ = fs::remove_file(&part_path).await;
                    return Err(LauncherError::Http(err));
                }
            };

            if let Err(source) = file.write_all(&chunk).await {
                drop(file);
                let _ = fs::remove_file(&part_path).await;
                return Err(LauncherError::Io {
                    path: part_path,
                    source,
                });
            }

            hasher.update(&chunk);
            let len = chunk.len() as u64;
            total_written += len;
            on_bytes(len);
        }

        file.flush().await.map_err(|source| LauncherError::Io {
            path: part_path.clone(),
            source,
        })?;
        drop(file);

        // Valida tamanho esperado
        if let Some(expected_size) = request.expected_size {
            if total_written != expected_size {
                let _ = fs::remove_file(&part_path).await;
                return Err(LauncherError::IntegrityFailed {
                    path: request.destination.clone(),
                    reason: format!(
                        "size mismatch (expected {expected_size} bytes, got {total_written} bytes)"
                    ),
                });
            }
        }

        // Valida SHA-1 esperado
        if let Some(expected_sha1) = &request.expected_sha1 {
            let actual_sha1 = hex::encode(hasher.finalize());
            if !actual_sha1.eq_ignore_ascii_case(expected_sha1) {
                let _ = fs::remove_file(&part_path).await;
                return Err(LauncherError::IntegrityFailed {
                    path: request.destination.clone(),
                    reason: format!("SHA-1 mismatch (expected {expected_sha1}, got {actual_sha1})"),
                });
            }
        }

        // Move para o destino definitivo somente após validação completa.
        fs::rename(&part_path, &request.destination)
            .await
            .map_err(|source| LauncherError::Io {
                path: request.destination.clone(),
                source,
            })?;

        Ok(total_written)
    }

    /// Baixa um arquivo com suporte completo a Pause/Resume (HTTP Range quando suportado)
    /// e cancelamento cooperativo via `TaskControl`.
    pub async fn download_file_controlled<F>(
        &self,
        request: &DownloadRequest,
        control: &TaskControl,
        on_bytes: F,
    ) -> Result<u64>
    where
        F: Fn(u64) + Send + Sync,
    {
        let cancel = control.cancellation_token();
        control.checkpoint().await?;

        let path_lock = self.lock_for_path(&request.destination);
        let _guard = tokio::select! {
            _ = cancel.cancelled() => return Err(LauncherError::Cancelled),
            g = path_lock.lock() => g,
        };

        if verify_file(
            &request.destination,
            request.expected_sha1.as_deref(),
            request.expected_size,
        )
        .await?
            == FileIntegrity::Valid
        {
            debug!("Reusing verified file: {}", request.destination.display());
            return Ok(request.expected_size.unwrap_or(0));
        }

        wait_while_download_paused(cancel).await?;

        let sem = self.current_semaphore();
        let _permit = tokio::select! {
            _ = cancel.cancelled() => return Err(LauncherError::Cancelled),
            permit = sem.acquire_owned() => {
                permit.map_err(|_| LauncherError::Cancelled)?
            }
        };

        let mut last_error: Option<LauncherError> = None;
        let max_retries = self.max_retries.load(Ordering::Relaxed);

        for attempt in 0..=max_retries {
            control.checkpoint().await?;
            wait_while_download_paused(cancel).await?;

            if attempt > 0 {
                let backoff_ms = 250u64 * (1u64 << (attempt - 1));
                tokio::select! {
                    _ = cancel.cancelled() => return Err(LauncherError::Cancelled),
                    _ = tokio::time::sleep(Duration::from_millis(backoff_ms)) => {}
                }
            }

            match self
                .download_once_resumable(request, control, &on_bytes)
                .await
            {
                Ok(bytes) => return Ok(bytes),
                Err(LauncherError::Cancelled) => return Err(LauncherError::Cancelled),
                Err(err) => {
                    last_error = Some(err);
                }
            }
        }

        Err(last_error.unwrap_or_else(|| LauncherError::DownloadFailed {
            url: request.url.clone(),
            reason: "unknown download failure".to_string(),
        }))
    }

    async fn download_once_resumable<F>(
        &self,
        request: &DownloadRequest,
        control: &TaskControl,
        on_bytes: &F,
    ) -> Result<u64>
    where
        F: Fn(u64) + Send + Sync,
    {
        let cancel = control.cancellation_token();
        if let Some(parent) = request.destination.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|source| LauncherError::Io {
                    path: parent.to_path_buf(),
                    source,
                })?;
        }

        let part_path = part_file_path(&request.destination);
        control.register_temp_path(part_path.clone());

        // Verifica se existe um `.part` parcial válido para retomar via HTTP Range.
        let existing_len = fs::metadata(&part_path).await.map(|m| m.len()).unwrap_or(0);
        let can_try_range = existing_len > 0
            && request
                .expected_size
                .map(|sz| existing_len < sz)
                .unwrap_or(true);

        let mut req_builder = self.client.get(&request.url);
        if can_try_range {
            req_builder = req_builder.header(header::RANGE, format!("bytes={existing_len}-"));
        }

        let response = tokio::select! {
            _ = cancel.cancelled() => return Err(LauncherError::Cancelled),
            res = req_builder.send() => res?,
        };

        let status = response.status();
        let resumed = can_try_range && status == StatusCode::PARTIAL_CONTENT;
        let response = response.error_for_status()?;
        let mut stream = response.bytes_stream();

        let mut hasher = Sha1::new();
        let mut total_written: u64 = 0;

        let mut file = if resumed {
            // Calcula o hash do prefixo já salvo em disco antes de anexar novos bytes.
            let mut reader = File::open(&part_path)
                .await
                .map_err(|source| LauncherError::Io {
                    path: part_path.clone(),
                    source,
                })?;
            let mut buf = vec![0u8; 64 * 1024];
            loop {
                let n = reader
                    .read(&mut buf)
                    .await
                    .map_err(|source| LauncherError::Io {
                        path: part_path.clone(),
                        source,
                    })?;
                if n == 0 {
                    break;
                }
                hasher.update(&buf[..n]);
                total_written += n as u64;
            }
            drop(reader);

            OpenOptions::new()
                .append(true)
                .open(&part_path)
                .await
                .map_err(|source| LauncherError::Io {
                    path: part_path.clone(),
                    source,
                })?
        } else {
            File::create(&part_path)
                .await
                .map_err(|source| LauncherError::Io {
                    path: part_path.clone(),
                    source,
                })?
        };

        loop {
            // Ponto seguro de pausa: mantém `.part` íntegro em disco enquanto pausado.
            control.checkpoint().await?;

            let chunk_opt = tokio::select! {
                _ = cancel.cancelled() => {
                    drop(file);
                    let _ = fs::remove_file(&part_path).await;
                    return Err(LauncherError::Cancelled);
                }
                next = stream.next() => next,
            };

            let Some(chunk_res) = chunk_opt else {
                break;
            };

            let chunk = match chunk_res {
                Ok(c) => c,
                Err(err) => {
                    drop(file);
                    return Err(LauncherError::Http(err));
                }
            };

            if let Err(source) = file.write_all(&chunk).await {
                drop(file);
                return Err(LauncherError::Io {
                    path: part_path,
                    source,
                });
            }

            hasher.update(&chunk);
            let len = chunk.len() as u64;
            total_written += len;
            on_bytes(len);
        }

        file.flush().await.map_err(|source| LauncherError::Io {
            path: part_path.clone(),
            source,
        })?;
        drop(file);

        if let Some(expected_size) = request.expected_size {
            if total_written != expected_size {
                let _ = fs::remove_file(&part_path).await;
                return Err(LauncherError::IntegrityFailed {
                    path: request.destination.clone(),
                    reason: format!(
                        "size mismatch (expected {expected_size} bytes, got {total_written} bytes)"
                    ),
                });
            }
        }

        if let Some(expected_sha1) = &request.expected_sha1 {
            let actual_sha1 = hex::encode(hasher.finalize());
            if !actual_sha1.eq_ignore_ascii_case(expected_sha1) {
                let _ = fs::remove_file(&part_path).await;
                return Err(LauncherError::IntegrityFailed {
                    path: request.destination.clone(),
                    reason: format!("SHA-1 mismatch (expected {expected_sha1}, got {actual_sha1})"),
                });
            }
        }

        fs::rename(&part_path, &request.destination)
            .await
            .map_err(|source| LauncherError::Io {
                path: request.destination.clone(),
                source,
            })?;

        Ok(total_written)
    }

    fn lock_for_path(&self, path: &Path) -> Arc<AsyncMutex<()>> {
        let mut map = self
            .path_locks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        map.entry(path.to_path_buf())
            .or_insert_with(|| Arc::new(AsyncMutex::new(())))
            .clone()
    }
}

fn part_file_path(dest: &Path) -> PathBuf {
    let mut os_str = dest.as_os_str().to_os_string();
    os_str.push(".part");
    PathBuf::from(os_str)
}
