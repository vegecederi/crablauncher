use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Preferências de integração de desempenho (Linux Performance Tweaks).
/// Todas são opcionais e desabilitadas por padrão.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct PerformanceTweaks {
    #[serde(default)]
    pub enable_gamemode: bool,
    #[serde(default)]
    pub enable_mangohud: bool,
    #[serde(default)]
    pub use_discrete_gpu: bool,
    #[serde(default)]
    pub use_zink: bool,
}

/// Estado real de disponibilidade de uma ferramenta externa no sistema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolAvailability {
    Available,
    NotInstalled,
    Unavailable(String),
}

impl ToolAvailability {
    pub fn is_available(&self) -> bool {
        matches!(self, Self::Available)
    }

    pub fn status_label(&self, enabled: bool) -> String {
        match self {
            Self::Available => {
                if enabled {
                    "Enabled".to_string()
                } else {
                    "Available".to_string()
                }
            }
            Self::NotInstalled => "Not Installed".to_string(),
            Self::Unavailable(reason) => format!("Unavailable ({reason})"),
        }
    }

    pub fn short_label(&self) -> String {
        match self {
            Self::Available => "Available".to_string(),
            Self::NotInstalled => "Not Installed".to_string(),
            Self::Unavailable(reason) => format!("Unavailable: {reason}"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuVendor {
    Nvidia,
    Amd,
    Intel,
    Other,
}

impl GpuVendor {
    pub fn label(self) -> &'static str {
        match self {
            Self::Nvidia => "NVIDIA",
            Self::Amd => "AMD",
            Self::Intel => "Intel",
            Self::Other => "Unknown",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpuDevice {
    pub name: String,
    pub vendor: GpuVendor,
    pub pci_slot: String,
    pub is_boot_vga: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuOffloadMethod {
    Switcherooctl,
    NvidiaPrime,
    MesaDriPrime,
    None,
}

impl GpuOffloadMethod {
    pub fn label(self) -> &'static str {
        match self {
            Self::Switcherooctl => "switcherooctl",
            Self::NvidiaPrime => "NVIDIA PRIME Render Offload",
            Self::MesaDriPrime => "Mesa DRI_PRIME=1",
            Self::None => "Unavailable",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpuDetectionReport {
    pub integrated_gpu: Option<GpuDevice>,
    pub discrete_gpu: Option<GpuDevice>,
    pub all_gpus: Vec<GpuDevice>,
    pub offload_method: GpuOffloadMethod,
    pub switcherooctl_available: bool,
    pub detail: String,
}

impl GpuDetectionReport {
    pub fn has_discrete_gpu(&self) -> bool {
        self.discrete_gpu.is_some() && self.offload_method != GpuOffloadMethod::None
    }

    pub fn primary_adapter_label(&self) -> String {
        if self.all_gpus.is_empty() {
            "Unknown".to_string()
        } else {
            self.all_gpus
                .iter()
                .map(|g| g.name.as_str())
                .collect::<Vec<_>>()
                .join(" / ")
        }
    }
}

/// Capacidades detectadas no sistema atual (armazenadas em cache no AppState).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemCapabilities {
    pub os_name: String,
    pub cpu_arch: String,
    pub display_session: String,
    pub gamemode: ToolAvailability,
    pub gamemode_executable: Option<PathBuf>,
    pub mangohud: ToolAvailability,
    pub mangohud_executable: Option<PathBuf>,
    pub mangohud_config_path: Option<PathBuf>,
    pub vulkan_available: bool,
    pub vulkan_detail: String,
    pub zink: ToolAvailability,
    pub gpu: GpuDetectionReport,
}

/// Wrapper externo a ser encadeado antes do executável Java (sem shell).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandWrapper {
    pub program: String,
    pub args: Vec<String>,
}

/// Resultado resolvido da aplicação dos tweaks de desempenho para um lançamento específico.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ResolvedTweaks {
    pub wrappers: Vec<CommandWrapper>,
    pub env_vars: BTreeMap<String, String>,
    pub applied_descriptions: Vec<String>,
    pub warnings: Vec<String>,
}
