use crate::core::tweaks::model::ToolAvailability;
use std::path::PathBuf;
use std::process::Command;

/// Procura um executável no `PATH` atual sem recorrer a shell.
pub fn find_in_path(binary: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(binary);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Detecta disponibilidade do `gamemoderun` e do daemon Feral GameMode no Linux.
/// Não instala pacotes, não usa sudo e não altera configurações globais do sistema.
pub fn detect_gamemode() -> (ToolAvailability, Option<PathBuf>) {
    let Some(exec) = find_in_path("gamemoderun") else {
        return (ToolAvailability::NotInstalled, None);
    };

    // Verifica se o serviço gamemoded responde ao autoteste (`gamemoded -t`) quando presente.
    if let Some(daemon) = find_in_path("gamemoded") {
        if let Ok(out) = Command::new(&daemon).arg("-t").output() {
            if out.status.success() {
                return (ToolAvailability::Available, Some(exec));
            }
            let stdout = String::from_utf8_lossy(&out.stdout);
            let stderr = String::from_utf8_lossy(&out.stderr);
            let combined = format!("{stdout} {stderr}").trim().to_string();
            // Alguns ambientes headless ou sem D-Bus falham o autoteste mesmo com o binário instalado.
            if combined.contains("gamemode is active") || combined.contains("Passed") {
                return (ToolAvailability::Available, Some(exec));
            }
            let reason = if combined.is_empty() {
                "GameMode daemon self-test failed".to_string()
            } else {
                combined
                    .lines()
                    .last()
                    .unwrap_or("GameMode unavailable")
                    .to_string()
            };
            return (ToolAvailability::Unavailable(reason), Some(exec));
        }
    }

    // Caso apenas `gamemoderun` exista (ex.: wrapper de sistema/Flatpak), marca como disponível.
    (ToolAvailability::Available, Some(exec))
}
