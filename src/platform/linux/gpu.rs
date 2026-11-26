use crate::core::tweaks::model::{GpuDetectionReport, GpuDevice, GpuOffloadMethod, GpuVendor};
use crate::platform::linux::gamemode::find_in_path;
use std::fs;
use std::path::Path;
use std::process::Command;

/// Detecta adaptadores gráficos no Linux via `/sys/class/drm` e `lspci`
/// e seleciona o mecanismo adequado de offload para GPU dedicada.
pub fn detect_gpus() -> GpuDetectionReport {
    let switcherooctl_available = find_in_path("switcherooctl").is_some();
    let drm_gpus = probe_drm_gpus();
    let lspci_names = probe_lspci_gpu_names();

    let mut devices = Vec::new();
    for (idx, mut dev) in drm_gpus.into_iter().enumerate() {
        if let Some(name) = match_lspci_name(&dev.pci_slot, dev.vendor, &lspci_names) {
            dev.name = name;
        } else if dev.name.is_empty() {
            dev.name = format!("{} GPU #{}", dev.vendor.label(), idx + 1);
        }
        devices.push(dev);
    }

    // Fallback para `lspci` caso `/sys/class/drm` não exponha entradas PCI (ex.: container/VM).
    if devices.is_empty() {
        for (slot, name) in &lspci_names {
            let vendor = infer_vendor_from_name(name);
            devices.push(GpuDevice {
                name: name.clone(),
                vendor,
                pci_slot: slot.clone(),
                is_boot_vga: devices.is_empty(),
            });
        }
    }

    classify_gpu_report(devices, switcherooctl_available)
}

/// Classifica GPUs integradas vs dedicadas e escolhe o método de offload compatível.
/// Função pura para facilitar testes automatizados sem depender do hardware do host.
pub fn classify_gpu_report(
    devices: Vec<GpuDevice>,
    switcherooctl_available: bool,
) -> GpuDetectionReport {
    if devices.is_empty() {
        return GpuDetectionReport {
            integrated_gpu: None,
            discrete_gpu: None,
            all_gpus: Vec::new(),
            offload_method: GpuOffloadMethod::None,
            switcherooctl_available,
            detail: "No graphics adapters detected.".to_string(),
        };
    }

    if devices.len() == 1 {
        let only = devices[0].clone();
        return GpuDetectionReport {
            integrated_gpu: Some(only.clone()),
            discrete_gpu: None,
            all_gpus: devices,
            offload_method: GpuOffloadMethod::None,
            switcherooctl_available,
            detail: format!(
                "Single GPU detected ({}); discrete GPU offload is not required.",
                only.name
            ),
        };
    }

    // Em sistemas multi-GPU, a GPU de boot (`boot_vga=1`) ou Intel geralmente é a integrada;
    // a secundária (especialmente NVIDIA ou AMD não-boot) é tratada como dedicada.
    let integrated = devices
        .iter()
        .find(|d| d.is_boot_vga)
        .or_else(|| devices.iter().find(|d| d.vendor == GpuVendor::Intel))
        .or_else(|| devices.first())
        .cloned();

    let discrete = devices
        .iter()
        .find(|d| {
            integrated
                .as_ref()
                .map(|i| i.pci_slot != d.pci_slot || i.name != d.name)
                .unwrap_or(true)
        })
        .cloned();

    let offload_method = match &discrete {
        Some(dgpu) => {
            if switcherooctl_available {
                GpuOffloadMethod::Switcherooctl
            } else if dgpu.vendor == GpuVendor::Nvidia {
                GpuOffloadMethod::NvidiaPrime
            } else {
                GpuOffloadMethod::MesaDriPrime
            }
        }
        None => GpuOffloadMethod::None,
    };

    let detail = match (&integrated, &discrete) {
        (Some(igpu), Some(dgpu)) => format!(
            "Integrated: {} | Discrete: {} | Method: {}",
            igpu.name,
            dgpu.name,
            offload_method.label()
        ),
        _ => "Discrete GPU unavailable.".to_string(),
    };

    GpuDetectionReport {
        integrated_gpu: integrated,
        discrete_gpu: discrete,
        all_gpus: devices,
        offload_method,
        switcherooctl_available,
        detail,
    }
}

fn probe_drm_gpus() -> Vec<GpuDevice> {
    let drm_dir = Path::new("/sys/class/drm");
    let Ok(entries) = fs::read_dir(drm_dir) else {
        return Vec::new();
    };

    let mut cards: Vec<GpuDevice> = Vec::new();
    for entry in entries.flatten() {
        let fname = entry.file_name();
        let Some(name_str) = fname.to_str() else {
            continue;
        };
        // Apenas `card0`, `card1`, etc. (ignora `card1-DP-1`, `renderD128`, `version`).
        if !name_str.starts_with("card") || name_str.contains('-') {
            continue;
        }
        let dev_dir = entry.path().join("device");
        let vendor_hex = fs::read_to_string(dev_dir.join("vendor"))
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase();
        if vendor_hex.is_empty() {
            continue;
        }
        let vendor = match vendor_hex.as_str() {
            "0x10de" => GpuVendor::Nvidia,
            "0x1002" | "0x1022" => GpuVendor::Amd,
            "0x8086" => GpuVendor::Intel,
            _ => GpuVendor::Other,
        };
        let is_boot_vga = fs::read_to_string(dev_dir.join("boot_vga"))
            .unwrap_or_default()
            .trim()
            == "1";
        let pci_slot = fs::canonicalize(&dev_dir)
            .ok()
            .and_then(|p| p.file_name().map(|s| s.to_string_lossy().into_owned()))
            .unwrap_or_default();

        cards.push(GpuDevice {
            name: String::new(),
            vendor,
            pci_slot,
            is_boot_vga,
        });
    }

    cards.sort_by(|a, b| a.pci_slot.cmp(&b.pci_slot));
    cards
}

fn probe_lspci_gpu_names() -> Vec<(String, String)> {
    let Some(lspci) = find_in_path("lspci") else {
        return Vec::new();
    };
    let Ok(out) = Command::new(lspci).arg("-D").output() else {
        return Vec::new();
    };
    if !out.status.success() {
        return Vec::new();
    }

    let stdout = String::from_utf8_lossy(&out.stdout);
    let mut results = Vec::new();
    for line in stdout.lines() {
        let lower = line.to_ascii_lowercase();
        if !(lower.contains("vga compatible controller")
            || lower.contains("3d controller")
            || lower.contains("display controller"))
        {
            continue;
        }
        let mut parts = line.splitn(2, ' ');
        let slot = parts.next().unwrap_or("").trim().to_string();
        let rest = parts.next().unwrap_or("");
        let desc = rest
            .split_once(':')
            .map(|(_, d)| d.trim())
            .unwrap_or(rest.trim())
            .to_string();
        results.push((slot, desc));
    }
    results
}

fn match_lspci_name(
    pci_slot: &str,
    vendor: GpuVendor,
    entries: &[(String, String)],
) -> Option<String> {
    if !pci_slot.is_empty() {
        if let Some((_, name)) = entries
            .iter()
            .find(|(slot, _)| slot.eq_ignore_ascii_case(pci_slot))
        {
            return Some(name.clone());
        }
    }
    entries
        .iter()
        .find(|(_, name)| infer_vendor_from_name(name) == vendor)
        .map(|(_, name)| name.clone())
}

fn infer_vendor_from_name(name: &str) -> GpuVendor {
    let lower = name.to_ascii_lowercase();
    if lower.contains("nvidia") || lower.contains("geforce") || lower.contains("quadro") {
        GpuVendor::Nvidia
    } else if lower.contains("amd") || lower.contains("radeon") || lower.contains("ati ") {
        GpuVendor::Amd
    } else if lower.contains("intel") {
        GpuVendor::Intel
    } else {
        GpuVendor::Other
    }
}
