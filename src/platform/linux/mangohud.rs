use crate::core::tweaks::model::ToolAvailability;
use crate::platform::linux::gamemode::find_in_path;
use std::path::PathBuf;

/// Detecta a disponibilidade real do MangoHud e localiza o arquivo de configuração do usuário.
pub fn detect_mangohud() -> (ToolAvailability, Option<PathBuf>, Option<PathBuf>) {
    let config_path = find_mangohud_config();
    match find_in_path("mangohud") {
        Some(exec) => (ToolAvailability::Available, Some(exec), config_path),
        None => (ToolAvailability::NotInstalled, None, config_path),
    }
}

/// Localiza o arquivo de configuração do MangoHud sem sobrescrever preferências existentes.
pub fn find_mangohud_config() -> Option<PathBuf> {
    if let Ok(env_path) = std::env::var("MANGOHUD_CONFIGFILE") {
        let trimmed = env_path.trim();
        if !trimmed.is_empty() {
            let p = PathBuf::from(trimmed);
            if p.is_file() {
                return Some(p);
            }
        }
    }

    if let Some(config_home) = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
    {
        let user_conf = config_home.join("MangoHud").join("MangoHud.conf");
        if user_conf.is_file() {
            return Some(user_conf);
        }
    }

    None
}
