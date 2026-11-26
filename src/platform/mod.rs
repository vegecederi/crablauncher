pub mod desktop;
#[cfg(target_os = "linux")]
pub mod linux;
pub mod paths;
pub mod system_info;
#[cfg(target_os = "windows")]
pub mod windows;

use crate::core::tweaks::model::SystemCapabilities;
use crate::error::{LauncherError, Result};
use std::path::{Path, PathBuf};

/// Abre um diretório ou arquivo no aplicativo padrão do sistema operacional.
pub fn open_in_file_manager(path: &Path) -> Result<()> {
    open::that(path).map_err(|source| LauncherError::OpenFolder {
        path: path.to_path_buf(),
        source,
    })
}

/// Detecta capacidades da plataforma atual (Linux / Windows / fallback).
pub fn detect_system_capabilities() -> SystemCapabilities {
    #[cfg(target_os = "linux")]
    {
        linux::detect_linux_capabilities()
    }
    #[cfg(target_os = "windows")]
    {
        windows::detect_windows_capabilities()
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        SystemCapabilities {
            os_name: format!("{} ({})", std::env::consts::OS, std::env::consts::ARCH),
            cpu_arch: std::env::consts::ARCH.to_string(),
            display_session: "Native".to_string(),
            gamemode: ToolAvailability::Unavailable("Linux only".to_string()),
            gamemode_executable: None,
            mangohud: ToolAvailability::Unavailable("Linux only".to_string()),
            mangohud_executable: None,
            mangohud_config_path: None,
            vulkan_available: false,
            vulkan_detail: "Unsupported".to_string(),
            zink: ToolAvailability::Unavailable("Linux only".to_string()),
            gpu: GpuDetectionReport {
                integrated_gpu: None,
                discrete_gpu: None,
                all_gpus: Vec::new(),
                offload_method: GpuOffloadMethod::None,
                switcherooctl_available: false,
                detail: "Unsupported on this platform.".to_string(),
            },
        }
    }
}

/// Abre a configuração do MangoHud no Linux, retornando erro informativo em outros SOs.
pub fn open_mangohud_config() -> Result<PathBuf> {
    #[cfg(target_os = "linux")]
    {
        linux::open_mangohud_configuration()
    }
    #[cfg(not(target_os = "linux"))]
    {
        Err(LauncherError::Launch(
            "MangoHud configuration is only available on Linux.".to_string(),
        ))
    }
}

/// Abre seletor nativo de executável Java quando suportado pelo sistema operacional.
pub fn pick_java_executable() -> Result<Option<PathBuf>> {
    #[cfg(target_os = "linux")]
    {
        linux::pick_java_executable_native()
    }
    #[cfg(not(target_os = "linux"))]
    {
        Err(LauncherError::Launch(
            "Native file picker is not configured for this platform; enter the Java path manually or use Detect."
                .to_string(),
        ))
    }
}

pub fn pick_icon_file() -> Result<Option<PathBuf>> {
    #[cfg(target_os = "linux")]
    {
        linux::pick_icon_file_native()
    }
    #[cfg(not(target_os = "linux"))]
    {
        Err(LauncherError::Launch(
            "Native icon file picker is not configured for this platform.".to_string(),
        ))
    }
}

pub fn pick_toml_file() -> Result<Option<PathBuf>> {
    #[cfg(target_os = "linux")]
    {
        linux::pick_toml_file_native()
    }
    #[cfg(not(target_os = "linux"))]
    {
        Err(LauncherError::Launch(
            "Native file picker is not configured for this platform.".to_string(),
        ))
    }
}

pub fn save_toml_file(default_name: &str) -> Result<Option<PathBuf>> {
    #[cfg(target_os = "linux")]
    {
        linux::save_toml_file_native(default_name)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = default_name;
        Err(LauncherError::Launch(
            "Native save dialog is not configured for this platform.".to_string(),
        ))
    }
}
