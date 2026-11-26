use crate::core::launch::command_builder::LaunchCommand;
use crate::core::launch::console::redact_sensitive_text;
use crate::error::{LauncherError, Result};
use chrono::Local;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::fs::{self, OpenOptions};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use uuid::Uuid;

/// Classificação do encerramento do processo do Minecraft.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessExitKind {
    Normal,
    Crash(i32),
    KilledByLauncher,
    KilledExternally(Option<i32>),
}

impl ProcessExitKind {
    /// Classifica um encerramento a partir do código de saída, sinal POSIX e indicador de parada solicitada pelo Crab.
    /// Quando não é possível distinguir com segurança um crash de encerramento externo (sem código nem sinal),
    /// trata como encerramento anormal (`Crash(-1)`).
    pub fn classify(exit_code: Option<i32>, signal: Option<i32>, killed_by_launcher: bool) -> Self {
        if killed_by_launcher {
            return Self::KilledByLauncher;
        }
        if let Some(code) = exit_code {
            return if code == 0 {
                Self::Normal
            } else {
                Self::Crash(code)
            };
        }
        match signal {
            // SIGINT (2), SIGKILL (9), SIGTERM (15) indicam encerramento externo explícito.
            Some(2 | 9 | 15) => Self::KilledExternally(signal),
            Some(sig) => Self::Crash(128 + sig),
            None => Self::Crash(-1),
        }
    }

    pub fn is_crash(&self) -> bool {
        matches!(self, Self::Crash(_))
    }

    pub fn is_killed(&self) -> bool {
        matches!(self, Self::KilledByLauncher | Self::KilledExternally(_))
    }

    pub fn should_reopen(
        &self,
        reopen_on_exit: bool,
        reopen_on_crash: bool,
        reopen_on_kill: bool,
    ) -> bool {
        match self {
            Self::Normal => reopen_on_exit,
            Self::Crash(_) => reopen_on_crash,
            Self::KilledByLauncher | Self::KilledExternally(_) => reopen_on_kill,
        }
    }

    pub fn status_message(&self) -> String {
        match self {
            Self::Normal => "Game exited normally (code 0).".to_string(),
            Self::Crash(code) => format!("Game crashed / exited abnormally (code {code})."),
            Self::KilledByLauncher => "Game stopped by Crab.".to_string(),
            Self::KilledExternally(Some(sig)) => {
                format!("Game was terminated externally (signal {sig}).")
            }
            Self::KilledExternally(None) => "Game was terminated externally.".to_string(),
        }
    }
}

/// Especificação serializada enviada ao processo supervisor headless (`crab-launcher --supervise <path>`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupervisorLaunchSpec {
    pub instance_id: String,
    pub instance_name: String,
    pub command: LaunchCommand,
    pub log_file_path: PathBuf,
    pub sensitive_values: Vec<String>,
    pub reopen_on_exit: bool,
    pub reopen_on_crash: bool,
    pub reopen_on_kill: bool,
    pub launcher_executable: PathBuf,
    pub reopen_lock_dir: PathBuf,
    #[serde(default)]
    pub instance_metadata_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupervisorHandshake {
    pub status: String,
    pub pid: Option<u32>,
    pub error: Option<String>,
}

/// Inicia o processo supervisor headless em um grupo de processos independente e aguarda
/// a confirmação (handshake) de que o Minecraft realmente iniciou antes de permitir o fechamento da GUI.
pub async fn spawn_supervised_launch(spec: &SupervisorLaunchSpec, cache_dir: &Path) -> Result<u32> {
    let sup_dir = cache_dir.join("supervisor");
    fs::create_dir_all(&sup_dir)
        .await
        .map_err(|source| LauncherError::Io {
            path: sup_dir.clone(),
            source,
        })?;

    let spec_path = sup_dir.join(format!("launch-{}.json", Uuid::new_v4()));
    let bytes = serde_json::to_vec_pretty(spec)?;
    fs::write(&spec_path, bytes)
        .await
        .map_err(|source| LauncherError::Io {
            path: spec_path.clone(),
            source,
        })?;

    let mut sup_cmd = Command::new(&spec.launcher_executable);
    sup_cmd
        .arg("--supervise")
        .arg(&spec_path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    #[cfg(unix)]
    {
        // Coloca o supervisor em um novo grupo de processos para sobreviver ao encerramento da GUI.
        sup_cmd.process_group(0);
    }

    let mut sup_child = sup_cmd.spawn().map_err(|err| {
        let _ = std::fs::remove_file(&spec_path);
        LauncherError::Launch(format!("Failed to spawn process supervisor: {err}"))
    })?;

    let stdout = sup_child.stdout.take().ok_or_else(|| {
        LauncherError::Launch("Supervisor did not provide a handshake channel.".to_string())
    })?;
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();

    let read_res = tokio::time::timeout(Duration::from_secs(15), reader.read_line(&mut line)).await;
    match read_res {
        Ok(Ok(n)) if n > 0 => {
            let handshake: SupervisorHandshake =
                serde_json::from_str(line.trim()).map_err(|e| {
                    LauncherError::Launch(format!("Invalid supervisor handshake response: {e}"))
                })?;
            if handshake.status == "started" {
                Ok(handshake.pid.unwrap_or(0))
            } else {
                let _ = sup_child.wait().await;
                Err(LauncherError::Launch(handshake.error.unwrap_or_else(
                    || "Supervised Minecraft launch failed.".to_string(),
                )))
            }
        }
        Ok(Ok(_)) => {
            let _ = sup_child.wait().await;
            Err(LauncherError::Launch(
                "Process supervisor exited before reporting Minecraft startup.".to_string(),
            ))
        }
        Ok(Err(err)) => {
            let _ = sup_child.wait().await;
            Err(LauncherError::Launch(format!(
                "Failed reading supervisor handshake: {err}"
            )))
        }
        Err(_) => {
            let _ = sup_child.kill().await;
            let _ = std::fs::remove_file(&spec_path);
            Err(LauncherError::Launch(
                "Timed out waiting for process supervisor to start Minecraft.".to_string(),
            ))
        }
    }
}

/// Ponto de entrada headless executado quando `crab-launcher --supervise <spec_path>` é invocado.
/// Monitora o processo do Minecraft, preserva logs completos redatados e reabre o Crab se configurado.
pub async fn run_supervisor_from_file(spec_path: &Path) -> Result<()> {
    let raw = fs::read(spec_path)
        .await
        .map_err(|source| LauncherError::Io {
            path: spec_path.to_path_buf(),
            source,
        })?;
    let spec: SupervisorLaunchSpec =
        serde_json::from_slice(&raw).map_err(|source| LauncherError::JsonParse {
            path: spec_path.to_path_buf(),
            source,
        })?;

    let result = run_supervisor(&spec, true).await;
    let _ = fs::remove_file(spec_path).await;
    result.map(|_| ())
}

/// Executa a supervisão completa de um `SupervisorLaunchSpec` e retorna o `ProcessExitKind` observado.
/// `emit_stdout_handshake`: quando `true`, imprime o handshake JSON em stdout (modo CLI `--supervise`).
pub async fn run_supervisor(
    spec: &SupervisorLaunchSpec,
    emit_stdout_handshake: bool,
) -> Result<ProcessExitKind> {
    if let Err(err) = fs::create_dir_all(&spec.command.working_dir).await {
        if emit_stdout_handshake {
            emit_handshake(&SupervisorHandshake {
                status: "failed".to_string(),
                pid: None,
                error: Some(format!("Failed to create working dir: {err}")),
            });
        }
        return Err(LauncherError::Io {
            path: spec.command.working_dir.clone(),
            source: err,
        });
    }

    if let Some(log_parent) = spec.log_file_path.parent() {
        let _ = fs::create_dir_all(log_parent).await;
    }

    let log_file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&spec.log_file_path)
        .await
        .ok();
    let log_file = Arc::new(tokio::sync::Mutex::new(log_file));

    write_supervisor_header(&log_file, spec).await;

    let mut cmd = spec.command.to_tokio_command();
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(err) => {
            let msg = format!(
                "Failed to start '{}': {err}",
                spec.command.program.display()
            );
            write_log_line(&log_file, &format!("[Crab Supervisor] ERROR: {msg}")).await;
            if emit_stdout_handshake {
                emit_handshake(&SupervisorHandshake {
                    status: "failed".to_string(),
                    pid: None,
                    error: Some(msg.clone()),
                });
            }
            return Err(LauncherError::Launch(msg));
        }
    };

    let pid = child.id();
    let session_started = std::time::Instant::now();
    if emit_stdout_handshake {
        emit_handshake(&SupervisorHandshake {
            status: "started".to_string(),
            pid,
            error: None,
        });
    }

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let secrets = Arc::new(spec.sensitive_values.clone());

    let lf_out = Arc::clone(&log_file);
    let sec_out = Arc::clone(&secrets);
    let stdout_task = tokio::spawn(async move {
        if let Some(out) = stdout {
            let mut lines = BufReader::new(out).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let clean = redact_sensitive_text(&line, &sec_out);
                write_log_line(&lf_out, &clean).await;
            }
        }
    });

    let lf_err = Arc::clone(&log_file);
    let sec_err = Arc::clone(&secrets);
    let stderr_task = tokio::spawn(async move {
        if let Some(err) = stderr {
            let mut lines = BufReader::new(err).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let clean = redact_sensitive_text(&line, &sec_err);
                write_log_line(&lf_err, &clean).await;
            }
        }
    });

    let status = child.wait().await.ok();
    let elapsed_secs = session_started.elapsed().as_secs();
    let _ = stdout_task.await;
    let _ = stderr_task.await;

    // Persiste o tempo real de execução em `instance.json` mesmo com a GUI do Crab fechada.
    if let Some(meta_path) = &spec.instance_metadata_path {
        record_supervised_playtime(meta_path, elapsed_secs);
    }

    let exit_code = status.and_then(|s| s.code());
    #[cfg(unix)]
    let signal = {
        use std::os::unix::process::ExitStatusExt;
        status.and_then(|s| s.signal())
    };
    #[cfg(not(unix))]
    let signal: Option<i32> = None;

    let exit_kind = ProcessExitKind::classify(exit_code, signal, false);
    write_log_line(
        &log_file,
        &format!(
            "[Crab Supervisor] {} (instance='{}')",
            exit_kind.status_message(),
            spec.instance_name
        ),
    )
    .await;

    if exit_kind.should_reopen(
        spec.reopen_on_exit,
        spec.reopen_on_crash,
        spec.reopen_on_kill,
    ) && try_acquire_reopen_debounce(&spec.reopen_lock_dir)
    {
        let mut relaunch = Command::new(&spec.launcher_executable);
        relaunch
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(unix)]
        {
            relaunch.process_group(0);
        }
        let _ = relaunch.spawn();
    }

    Ok(exit_kind)
}

fn emit_handshake(hs: &SupervisorHandshake) {
    if let Ok(line) = serde_json::to_string(hs) {
        use std::io::Write;
        let mut out = std::io::stdout().lock();
        let _ = writeln!(out, "{line}");
        let _ = out.flush();
    }
}

async fn write_supervisor_header(
    log_file: &Arc<tokio::sync::Mutex<Option<tokio::fs::File>>>,
    spec: &SupervisorLaunchSpec,
) {
    let now = Local::now().format("%Y-%m-%d %H:%M:%S");
    write_log_line(
        log_file,
        &format!(
            "[Crab Supervisor {now}] Launching '{}' (headless supervision mode)",
            spec.instance_name
        ),
    )
    .await;
    if !spec.command.applied_tweaks.is_empty() {
        write_log_line(
            log_file,
            &format!(
                "[Crab Supervisor] Applied tweaks: {}",
                spec.command.applied_tweaks.join(", ")
            ),
        )
        .await;
    }
    for warn_msg in &spec.command.warnings {
        write_log_line(log_file, &format!("[Crab Supervisor] WARNING: {warn_msg}")).await;
    }
}

async fn write_log_line(log_file: &Arc<tokio::sync::Mutex<Option<tokio::fs::File>>>, line: &str) {
    if let Some(file) = log_file.lock().await.as_mut() {
        let _ = file.write_all(line.as_bytes()).await;
        let _ = file.write_all(b"\n").await;
        let _ = file.flush().await;
    }
}

/// Evita reaberturas duplicadas caso múltiplas instâncias supervisionadas encerrem ao mesmo tempo.
fn try_acquire_reopen_debounce(lock_dir: &Path) -> bool {
    let _ = std::fs::create_dir_all(lock_dir);
    let stamp_path = lock_dir.join("last-reopen.stamp");
    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    if let Ok(prev) = std::fs::read_to_string(&stamp_path) {
        if let Ok(prev_secs) = prev.trim().parse::<u64>() {
            if now_secs.saturating_sub(prev_secs) < 4 {
                return false;
            }
        }
    }

    let _ = std::fs::write(&stamp_path, now_secs.to_string());
    true
}

fn record_supervised_playtime(meta_path: &Path, elapsed_secs: u64) {
    let Ok(bytes) = std::fs::read(meta_path) else {
        return;
    };
    let Ok(mut meta) = serde_json::from_slice::<crate::core::instance::InstanceMetadata>(&bytes)
    else {
        return;
    };
    let now = chrono::Utc::now();
    meta.last_played = Some(now);
    meta.playtime.total_playtime_secs = meta
        .playtime
        .total_playtime_secs
        .saturating_add(elapsed_secs);
    meta.playtime.last_session_secs = elapsed_secs;
    meta.playtime.launch_count = meta.playtime.launch_count.saturating_add(1);
    meta.updated_at = now;
    if let Ok(out) = serde_json::to_vec_pretty(&meta) {
        let tmp = meta_path.with_extension("json.tmp");
        if std::fs::write(&tmp, out).is_ok() {
            let _ = std::fs::rename(&tmp, meta_path);
        }
    }
}
