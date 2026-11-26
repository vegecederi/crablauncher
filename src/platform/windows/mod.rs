use crate::core::tweaks::model::{
    GpuDetectionReport, GpuOffloadMethod, SystemCapabilities, ToolAvailability,
};

/// No Windows as ferramentas específicas de Linux não estão disponíveis.
pub fn detect_windows_capabilities() -> SystemCapabilities {
    SystemCapabilities {
        os_name: format!("Windows ({})", std::env::consts::ARCH),
        cpu_arch: std::env::consts::ARCH.to_string(),
        display_session: "Win32".to_string(),
        gamemode: ToolAvailability::Unavailable("Linux only".to_string()),
        gamemode_executable: None,
        mangohud: ToolAvailability::Unavailable("Linux only".to_string()),
        mangohud_executable: None,
        mangohud_config_path: None,
        vulkan_available: false,
        vulkan_detail: "Not probed on Windows".to_string(),
        zink: ToolAvailability::Unavailable("Linux only".to_string()),
        gpu: GpuDetectionReport {
            integrated_gpu: None,
            discrete_gpu: None,
            all_gpus: Vec::new(),
            offload_method: GpuOffloadMethod::None,
            switcherooctl_available: false,
            detail: "GPU offload selection is managed by Windows graphics settings.".to_string(),
        },
    }
}
