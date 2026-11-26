pub mod gamemode;
pub mod gpu;
pub mod mangohud;
pub mod zink;

use crate::core::tweaks::model::SystemCapabilities;
use crate::error::{LauncherError, Result};
use std::path::PathBuf;
use std::process::Command;

/// Detecta o tipo de sessão gráfica atual (Wayland, X11 ou Unknown).
pub fn detect_display_session() -> String {
    if let Ok(session) = std::env::var("XDG_SESSION_TYPE") {
        let lower = session.trim().to_ascii_lowercase();
        if lower == "wayland" {
            return "Wayland".to_string();
        }
        if lower == "x11" {
            return "X11".to_string();
        }
        if !lower.is_empty() {
            return session;
        }
    }
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        return "Wayland".to_string();
    }
    if std::env::var_os("DISPLAY").is_some() {
        return "X11".to_string();
    }
    "Unknown".to_string()
}

/// Lê nome legível do sistema Linux em `/etc/os-release` (sem dados sensíveis).
pub fn detect_linux_os_name() -> String {
    if let Ok(content) = std::fs::read_to_string("/etc/os-release") {
        for line in content.lines() {
            if let Some(rest) = line.strip_prefix("PRETTY_NAME=") {
                let clean = rest.trim().trim_matches('"');
                if !clean.is_empty() {
                    return clean.to_string();
                }
            }
        }
    }
    format!("Linux ({})", std::env::consts::ARCH)
}

/// Detecta todas as capacidades Linux de desempenho (GameMode, MangoHud, GPU, Vulkan, Zink).
/// Projetado para ser executado sob demanda e armazenado em cache.
pub fn detect_linux_capabilities() -> SystemCapabilities {
    let (gamemode, gamemode_executable) = gamemode::detect_gamemode();
    let (mangohud, mangohud_executable, mangohud_config_path) = mangohud::detect_mangohud();
    let gpu = gpu::detect_gpus();
    let (vulkan_available, vulkan_detail) = zink::detect_vulkan();
    let zink_status = zink::detect_zink(vulkan_available);

    SystemCapabilities {
        os_name: detect_linux_os_name(),
        cpu_arch: std::env::consts::ARCH.to_string(),
        display_session: detect_display_session(),
        gamemode,
        gamemode_executable,
        mangohud,
        mangohud_executable,
        mangohud_config_path,
        vulkan_available,
        vulkan_detail,
        zink: zink_status,
        gpu,
    }
}

/// Abre o arquivo de configuração do MangoHud no editor padrão se ele existir,
/// ou a pasta `~/.config/MangoHud` sem sobrescrever configurações existentes.
pub fn open_mangohud_configuration() -> Result<PathBuf> {
    if let Some(existing) = mangohud::find_mangohud_config() {
        open::that(&existing).map_err(|source| LauncherError::OpenFolder {
            path: existing.clone(),
            source,
        })?;
        return Ok(existing);
    }

    let config_dir = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .ok_or_else(|| {
            LauncherError::Launch("Could not resolve user configuration directory.".to_string())
        })?
        .join("MangoHud");

    if config_dir.is_dir() {
        open::that(&config_dir).map_err(|source| LauncherError::OpenFolder {
            path: config_dir.clone(),
            source,
        })?;
        return Ok(config_dir);
    }

    Err(LauncherError::Launch(
        "No MangoHud configuration file found at ~/.config/MangoHud/MangoHud.conf.".to_string(),
    ))
}

/// Abre um seletor de arquivos nativo via `zenity` ou `kdialog` quando disponível no Linux.
pub fn pick_java_executable_native() -> Result<Option<PathBuf>> {
    if let Some(zenity) = gamemode::find_in_path("zenity") {
        let out = Command::new(zenity)
            .args([
                "--file-selection",
                "--title=Select Java Executable",
                "--filename=/usr/lib/jvm/",
            ])
            .output()
            .map_err(|err| LauncherError::Launch(format!("Failed to launch file picker: {err}")))?;
        if out.status.success() {
            let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
            return Ok(if path.is_empty() {
                None
            } else {
                Some(PathBuf::from(path))
            });
        }
        return Ok(None);
    }

    if let Some(kdialog) = gamemode::find_in_path("kdialog") {
        let out = Command::new(kdialog)
            .args([
                "--getopenfilename",
                "/usr/lib/jvm",
                "*|All Files",
                "--title",
                "Select Java Executable",
            ])
            .output()
            .map_err(|err| LauncherError::Launch(format!("Failed to launch file picker: {err}")))?;
        if out.status.success() {
            let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
            return Ok(if path.is_empty() {
                None
            } else {
                Some(PathBuf::from(path))
            });
        }
        return Ok(None);
    }

    Err(LauncherError::Launch(
        "No native file picker (zenity or kdialog) found. Enter the Java path manually or click Detect.".to_string(),
    ))
}

/// Abre seletor nativo para importar um arquivo de imagem (PNG, JPEG, WebP).
pub fn pick_icon_file_native() -> Result<Option<PathBuf>> {
    pick_file_generic(
        "Import Instance Icon",
        "*.png *.jpg *.jpeg *.webp",
        "Images (PNG, JPEG, WebP)",
    )
}

/// Abre seletor nativo para importar um arquivo TOML de preset JVM.
pub fn pick_toml_file_native() -> Result<Option<PathBuf>> {
    pick_file_generic("Import JVM Preset", "*.toml", "TOML Preset Files")
}

/// Abre seletor nativo para exportar um arquivo TOML de preset JVM.
pub fn save_toml_file_native(default_name: &str) -> Result<Option<PathBuf>> {
    if let Some(zenity) = gamemode::find_in_path("zenity") {
        let out = Command::new(zenity)
            .args([
                "--file-selection",
                "--save",
                "--confirm-overwrite",
                "--title=Export JVM Preset",
                &format!("--filename={default_name}.toml"),
            ])
            .output()
            .map_err(|err| LauncherError::Launch(format!("Failed to launch file picker: {err}")))?;
        if out.status.success() {
            let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
            return Ok(if path.is_empty() {
                None
            } else {
                Some(PathBuf::from(path))
            });
        }
        return Ok(None);
    }
    if let Some(kdialog) = gamemode::find_in_path("kdialog") {
        let out = Command::new(kdialog)
            .args([
                "--getsavefilename",
                &format!("{default_name}.toml"),
                "*.toml|TOML Files",
                "--title",
                "Export JVM Preset",
            ])
            .output()
            .map_err(|err| LauncherError::Launch(format!("Failed to launch file picker: {err}")))?;
        if out.status.success() {
            let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
            return Ok(if path.is_empty() {
                None
            } else {
                Some(PathBuf::from(path))
            });
        }
        return Ok(None);
    }
    Err(LauncherError::Launch(
        "No native file picker (zenity or kdialog) found.".to_string(),
    ))
}

fn pick_file_generic(
    title: &str,
    glob_filter: &str,
    filter_label: &str,
) -> Result<Option<PathBuf>> {
    if let Some(zenity) = gamemode::find_in_path("zenity") {
        let out = Command::new(zenity)
            .args([
                "--file-selection",
                &format!("--title={title}"),
                &format!("--file-filter={filter_label} | {glob_filter}"),
            ])
            .output()
            .map_err(|err| LauncherError::Launch(format!("Failed to launch file picker: {err}")))?;
        if out.status.success() {
            let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
            return Ok(if path.is_empty() {
                None
            } else {
                Some(PathBuf::from(path))
            });
        }
        return Ok(None);
    }
    if let Some(kdialog) = gamemode::find_in_path("kdialog") {
        let out = Command::new(kdialog)
            .args([
                "--getopenfilename",
                ".",
                &format!("{glob_filter}|{filter_label}"),
                "--title",
                title,
            ])
            .output()
            .map_err(|err| LauncherError::Launch(format!("Failed to launch file picker: {err}")))?;
        if out.status.success() {
            let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
            return Ok(if path.is_empty() {
                None
            } else {
                Some(PathBuf::from(path))
            });
        }
        return Ok(None);
    }
    Err(LauncherError::Launch(
        "No native file picker (zenity or kdialog) found.".to_string(),
    ))
}
