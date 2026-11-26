use crate::core::tweaks::model::ToolAvailability;
use crate::platform::linux::gamemode::find_in_path;
use std::fs;
use std::path::Path;
use std::process::Command;

/// Detecta disponibilidade de drivers Vulkan (ICDs) no sistema Linux.
pub fn detect_vulkan() -> (bool, String) {
    let icd_dirs = [
        "/usr/share/vulkan/icd.d",
        "/etc/vulkan/icd.d",
        "/usr/local/share/vulkan/icd.d",
    ];
    let mut icd_names = Vec::new();
    for dir in icd_dirs {
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("json") {
                    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                        if !icd_names.contains(&stem.to_string()) {
                            icd_names.push(stem.to_string());
                        }
                    }
                }
            }
        }
    }

    if let Some(vinfo) = find_in_path("vulkaninfo") {
        if let Ok(out) = Command::new(vinfo).arg("--summary").output() {
            if out.status.success() {
                let stdout = String::from_utf8_lossy(&out.stdout);
                if let Some(dev_line) = stdout
                    .lines()
                    .find(|l| l.trim_start().starts_with("deviceName"))
                {
                    let dev = dev_line
                        .split_once('=')
                        .map(|(_, v)| v.trim())
                        .unwrap_or("Vulkan device");
                    return (true, format!("Available ({dev})"));
                }
                return (true, "Available (verified via vulkaninfo)".to_string());
            }
        }
    }

    if !icd_names.is_empty() {
        return (true, format!("ICD detected ({})", icd_names.join(", ")));
    }

    (false, "No Vulkan ICD detected".to_string())
}

/// Detecta se Mesa Zink (`zink_dri.so` ou Mesa Gallium megadriver) e Vulkan estão disponíveis.
pub fn detect_zink(vulkan_available: bool) -> ToolAvailability {
    if !vulkan_available {
        return ToolAvailability::Unavailable(
            "No functional Vulkan driver detected for Mesa Zink.".to_string(),
        );
    }

    let candidate_dirs = [
        "/usr/lib64/dri",
        "/usr/lib/dri",
        "/usr/lib/x86_64-linux-gnu/dri",
        "/usr/lib/aarch64-linux-gnu/dri",
        "/usr/lib64/gbm",
        "/usr/lib/gbm",
    ];

    for dir in candidate_dirs {
        let p = Path::new(dir);
        if p.join("zink_dri.so").exists() {
            return ToolAvailability::Available;
        }
        // Em versões recentes do Mesa (24.2+), Zink é integrado ao megadriver `libgallium-*.so`.
        if let Ok(entries) = fs::read_dir(p) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let s = name.to_string_lossy();
                if s.starts_with("zink_dri") || s.contains("libgallium") {
                    return ToolAvailability::Available;
                }
            }
        }
    }

    // Também verifica diretórios de bibliotecas do sistema para `libgallium`.
    for lib_dir in ["/usr/lib64", "/usr/lib", "/usr/lib/x86_64-linux-gnu"] {
        if let Ok(entries) = fs::read_dir(lib_dir) {
            for entry in entries.flatten() {
                if entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("libgallium-")
                {
                    return ToolAvailability::Available;
                }
            }
        }
    }

    ToolAvailability::Unavailable(
        "Mesa Zink driver could not be confirmed on this system.".to_string(),
    )
}
