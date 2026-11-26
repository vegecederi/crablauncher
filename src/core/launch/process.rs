use crate::core::launch::command_builder::LaunchCommand;
use crate::core::launch::console::{infer_log_level, redact_sensitive_text, LogLevel};
use crate::core::launch::planner::LaunchPlan;
use crate::core::launch::supervisor::ProcessExitKind;
use crate::error::{LauncherError, Result};
use std::collections::HashMap;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tokio::fs::{self, OpenOptions};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::oneshot;
use tracing::info;

/// Evento emitido pelo processo em execução do Minecraft.
#[derive(Debug, Clone)]
pub enum ProcessEvent {
    Log {
        instance_id: String,
        level: LogLevel,
        line: String,
    },
    Exited {
        instance_id: String,
        exit_code: Option<i32>,
        exit_kind: ProcessExitKind,
    },
}

struct RunningEntry {
    kill_tx: oneshot::Sender<()>,
    killed_by_launcher: Arc<AtomicBool>,
}

/// Gerencia processos ativos do Minecraft, impedindo execução duplicada da mesma instância
/// enquanto permite múltiplas instâncias diferentes em paralelo.
#[derive(Clone, Default)]
pub struct ProcessManager {
    running: Arc<Mutex<HashMap<String, RunningEntry>>>,
}

impl ProcessManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_running(&self, instance_id: &str) -> bool {
        self.running
            .lock()
            .map(|m| m.contains_key(instance_id))
            .unwrap_or(false)
    }

    pub fn any_running(&self) -> bool {
        self.running.lock().map(|m| !m.is_empty()).unwrap_or(false)
    }

    pub fn running_count(&self) -> usize {
        self.running.lock().map(|m| m.len()).unwrap_or(0)
    }

    /// Solicita encerramento controlado do processo de uma instância e marca como encerrado pelo Crab.
    pub fn stop_instance(&self, instance_id: &str) -> bool {
        if let Ok(mut map) = self.running.lock() {
            if let Some(entry) = map.remove(instance_id) {
                entry.killed_by_launcher.store(true, Ordering::SeqCst);
                let _ = entry.kill_tx.send(());
                return true;
            }
        }
        false
    }

    /// Inicia o processo diretamente a partir de um `LaunchCommand` (sem shell)
    /// e monitora stdout/stderr até o encerramento, gravando log completo no disco.
    pub async fn spawn_command<F>(
        &self,
        plan: LaunchPlan,
        command: LaunchCommand,
        preserve_complete_logs: bool,
        on_event: F,
    ) -> Result<()>
    where
        F: Fn(ProcessEvent) + Send + Sync + 'static,
    {
        let on_event = Arc::new(on_event);
        let (kill_tx, mut kill_rx) = oneshot::channel::<()>();
        let killed_by_launcher = Arc::new(AtomicBool::new(false));

        {
            let mut map = self.running.lock().unwrap_or_else(|p| p.into_inner());
            if map.contains_key(&plan.instance_id) {
                return Err(LauncherError::AlreadyRunning(plan.instance_name.clone()));
            }
            map.insert(
                plan.instance_id.clone(),
                RunningEntry {
                    kill_tx,
                    killed_by_launcher: Arc::clone(&killed_by_launcher),
                },
            );
        }

        fs::create_dir_all(&command.working_dir)
            .await
            .map_err(|source| LauncherError::Io {
                path: command.working_dir.clone(),
                source,
            })?;

        let log_dir = plan.game_dir.join("logs");
        let _ = fs::create_dir_all(&log_dir).await;
        let log_file_path = log_dir.join("crab-latest.log");
        let log_file = if preserve_complete_logs {
            OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&log_file_path)
                .await
                .ok()
        } else {
            None
        };
        let log_file = Arc::new(tokio::sync::Mutex::new(log_file));

        // Registra tweaks aplicados e avisos nos logs de inicialização.
        let mut startup_lines = Vec::new();
        if !command.applied_tweaks.is_empty() {
            startup_lines.push((
                LogLevel::Info,
                format!(
                    "[Crab] Performance tweaks applied: {}",
                    command.applied_tweaks.join(", ")
                ),
            ));
        }
        for warn_msg in &command.warnings {
            startup_lines.push((LogLevel::Warn, format!("[Crab] {warn_msg}")));
        }
        for (lvl, msg) in startup_lines {
            on_event(ProcessEvent::Log {
                instance_id: plan.instance_id.clone(),
                level: lvl,
                line: msg.clone(),
            });
            if let Some(file) = log_file.lock().await.as_mut() {
                let _ = file.write_all(msg.as_bytes()).await;
                let _ = file.write_all(b"\n").await;
            }
        }

        let mut cmd = command.to_tokio_command();
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(err) => {
                if let Ok(mut map) = self.running.lock() {
                    map.remove(&plan.instance_id);
                }
                return Err(LauncherError::Launch(format!(
                    "Failed to start '{}': {err}",
                    command.program.display()
                )));
            }
        };

        info!(
            "Started Minecraft process for '{}' (pid={:?}, program={})",
            plan.instance_name,
            child.id(),
            command.program.display()
        );

        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let secrets = Arc::new(plan.sensitive_values.clone());
        let instance_id = plan.instance_id.clone();

        let emit_line = {
            let on_event = Arc::clone(&on_event);
            let secrets = Arc::clone(&secrets);
            let log_file = Arc::clone(&log_file);
            let instance_id = instance_id.clone();
            move |raw: String, is_stderr: bool| {
                let clean = redact_sensitive_text(&raw, &secrets);
                let level = infer_log_level(&clean, is_stderr);
                on_event(ProcessEvent::Log {
                    instance_id: instance_id.clone(),
                    level,
                    line: clean.clone(),
                });
                let log_file = Arc::clone(&log_file);
                tokio::spawn(async move {
                    if let Some(file) = log_file.lock().await.as_mut() {
                        let _ = file.write_all(clean.as_bytes()).await;
                        let _ = file.write_all(b"\n").await;
                    }
                });
            }
        };

        let emit_stdout = emit_line.clone();
        let stdout_task = tokio::spawn(async move {
            if let Some(out) = stdout {
                let mut lines = BufReader::new(out).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    emit_stdout(line, false);
                }
            }
        });

        let emit_stderr = emit_line;
        let stderr_task = tokio::spawn(async move {
            if let Some(err) = stderr {
                let mut lines = BufReader::new(err).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    emit_stderr(line, true);
                }
            }
        });

        let running_map = Arc::clone(&self.running);
        let id_for_wait = plan.instance_id.clone();
        let on_event_wait = Arc::clone(&on_event);

        tokio::spawn(async move {
            let status = tokio::select! {
                _ = &mut kill_rx => {
                    killed_by_launcher.store(true, Ordering::SeqCst);
                    let _ = child.kill().await;
                    child.wait().await.ok()
                }
                st = child.wait() => {
                    st.ok()
                }
            };

            let _ = stdout_task.await;
            let _ = stderr_task.await;

            let exit_code = status.and_then(|s| s.code());
            #[cfg(unix)]
            let signal = {
                use std::os::unix::process::ExitStatusExt;
                status.and_then(|s| s.signal())
            };
            #[cfg(not(unix))]
            let signal: Option<i32> = None;

            let exit_kind = ProcessExitKind::classify(
                exit_code,
                signal,
                killed_by_launcher.load(Ordering::SeqCst),
            );

            if let Ok(mut map) = running_map.lock() {
                map.remove(&id_for_wait);
            }

            on_event_wait(ProcessEvent::Exited {
                instance_id: id_for_wait,
                exit_code,
                exit_kind,
            });
        });

        Ok(())
    }

    /// Compatibilidade direta: constrói `LaunchCommand` sem tweaks e inicia o processo.
    pub async fn spawn_plan<F>(&self, plan: LaunchPlan, on_event: F) -> Result<()>
    where
        F: Fn(ProcessEvent) + Send + Sync + 'static,
    {
        let cmd = crate::core::launch::command_builder::build_launch_command(
            &plan,
            &crate::core::tweaks::model::ResolvedTweaks::default(),
        );
        self.spawn_command(plan, cmd, true, on_event).await
    }
}
