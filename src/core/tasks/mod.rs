use crate::error::{LauncherError, Result};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

/// Estado explícito de uma tarefa gerenciada pelo backend do Crab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    Queued,
    Running,
    Pausing,
    Paused,
    Resuming,
    Cancelling,
    Cancelled,
    Completed,
    Failed,
}

impl TaskState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Queued => "Queued",
            Self::Running => "Running",
            Self::Pausing => "Pausing...",
            Self::Paused => "Paused",
            Self::Resuming => "Resuming...",
            Self::Cancelling => "Cancelling...",
            Self::Cancelled => "Cancelled",
            Self::Completed => "Completed",
            Self::Failed => "Failed",
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Cancelled | Self::Completed | Self::Failed)
    }
}

/// Tipo da tarefa em execução para ícone e classificação na janela Task Progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskKind {
    InstallInstance,
    InstallModloader,
    InstallJava,
    RepairInstance,
    DownloadAssets,
}

impl TaskKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InstallInstance => "minecraft",
            Self::InstallModloader => "modloader",
            Self::InstallJava => "java",
            Self::RepairInstance => "repair",
            Self::DownloadAssets => "assets",
        }
    }
}

/// Snapshot de progresso enviado à interface com throttling para evitar renderizações excessivas.
#[derive(Debug, Clone)]
pub struct TaskProgressSnapshot {
    pub task_id: String,
    pub kind: TaskKind,
    pub title: String,
    pub stage: String,
    pub state: TaskState,
    pub completed_bytes: u64,
    pub total_bytes: Option<u64>,
    pub completed_files: usize,
    pub total_files: usize,
    pub current_file: String,
    pub speed_bytes_per_sec: f64,
    pub eta_seconds: Option<u64>,
    pub elapsed_seconds: u64,
    pub details: String,
}

/// Controle cooperativo de uma tarefa: pausa real, retomada, cancelamento e registro
/// de arquivos temporários pertencentes exclusivamente a esta tarefa para rollback seguro.
#[derive(Clone)]
pub struct TaskControl {
    id: String,
    cancel: CancellationToken,
    pause_tx: Arc<watch::Sender<bool>>,
    pause_rx: watch::Receiver<bool>,
    state: Arc<Mutex<TaskState>>,
    temp_paths: Arc<Mutex<Vec<PathBuf>>>,
}

impl TaskControl {
    pub fn new(id: impl Into<String>) -> Self {
        let (pause_tx, pause_rx) = watch::channel(false);
        Self {
            id: id.into(),
            cancel: CancellationToken::new(),
            pause_tx: Arc::new(pause_tx),
            pause_rx,
            state: Arc::new(Mutex::new(TaskState::Queued)),
            temp_paths: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn cancellation_token(&self) -> &CancellationToken {
        &self.cancel
    }

    pub fn state(&self) -> TaskState {
        self.state.lock().map(|g| *g).unwrap_or(TaskState::Failed)
    }

    pub fn set_state(&self, next: TaskState) {
        if let Ok(mut g) = self.state.lock() {
            if !g.is_terminal() {
                *g = next;
            }
        }
    }

    pub fn is_paused(&self) -> bool {
        *self.pause_rx.borrow()
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }

    /// Solicita pausa cooperativa. A tarefa entra em `Pausing` até atingir um ponto seguro.
    pub fn request_pause(&self) {
        if let Ok(mut g) = self.state.lock() {
            if matches!(
                *g,
                TaskState::Running | TaskState::Resuming | TaskState::Queued
            ) {
                *g = TaskState::Pausing;
                let _ = self.pause_tx.send(true);
            }
        }
    }

    /// Retoma uma tarefa pausada ou em processo de pausa.
    pub fn request_resume(&self) {
        if let Ok(mut g) = self.state.lock() {
            if matches!(*g, TaskState::Paused | TaskState::Pausing) {
                *g = TaskState::Resuming;
                let _ = self.pause_tx.send(false);
            }
        }
    }

    /// Cancela a tarefa de forma cooperativa e desbloqueia qualquer ponto de pausa.
    pub fn request_cancel(&self) {
        if let Ok(mut g) = self.state.lock() {
            if !g.is_terminal() {
                *g = TaskState::Cancelling;
            }
        }
        let _ = self.pause_tx.send(false);
        self.cancel.cancel();
    }

    /// Ponto seguro de verificação de pausa/cancelamento para loops de download e instalação.
    pub async fn checkpoint(&self) -> Result<()> {
        if self.cancel.is_cancelled() {
            return Err(LauncherError::Cancelled);
        }
        let mut rx = self.pause_rx.clone();
        while *rx.borrow() {
            self.set_state(TaskState::Paused);
            tokio::select! {
                _ = self.cancel.cancelled() => return Err(LauncherError::Cancelled),
                changed = rx.changed() => {
                    if changed.is_err() {
                        break;
                    }
                }
            }
        }
        if self.cancel.is_cancelled() {
            return Err(LauncherError::Cancelled);
        }
        if matches!(
            self.state(),
            TaskState::Paused | TaskState::Resuming | TaskState::Queued
        ) {
            self.set_state(TaskState::Running);
        }
        Ok(())
    }

    /// Registra um arquivo temporário (ex.: `.part` ou staging) sob responsabilidade exclusiva desta tarefa.
    pub fn register_temp_path(&self, path: PathBuf) {
        if let Ok(mut g) = self.temp_paths.lock() {
            if !g.contains(&path) {
                g.push(path);
            }
        }
    }

    /// Remove apenas os arquivos temporários registrados por esta tarefa.
    /// Nunca toca em bibliotecas compartilhadas, assets validados, runtimes Java ou mundos.
    pub async fn cleanup_temp_paths(&self) -> usize {
        let paths = self
            .temp_paths
            .lock()
            .map(|mut g| std::mem::take(&mut *g))
            .unwrap_or_default();
        let mut removed = 0usize;
        for path in paths {
            if tokio::fs::metadata(&path).await.is_ok() {
                let res = if path.is_dir() {
                    tokio::fs::remove_dir_all(&path).await
                } else {
                    tokio::fs::remove_file(&path).await
                };
                if res.is_ok() {
                    removed += 1;
                }
            }
        }
        removed
    }
}

pub fn format_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let b = bytes as f64;
    if b >= GB {
        format!("{:.2} GB", b / GB)
    } else if b >= MB {
        format!("{:.1} MB", b / MB)
    } else if b >= KB {
        format!("{:.0} KB", b / KB)
    } else {
        format!("{bytes} B")
    }
}

pub fn format_duration(secs: u64) -> String {
    let m = secs / 60;
    let s = secs % 60;
    if m >= 60 {
        format!("{}h {:02}m {:02}s", m / 60, m % 60, s)
    } else if m > 0 {
        format!("{m}m {s:02}s")
    } else {
        format!("{s}s")
    }
}
