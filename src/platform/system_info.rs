use crate::core::tweaks::model::SystemCapabilities;

/// Detecta a quantidade aproximada de memória RAM física total do sistema em MiB.
pub fn detect_system_memory_mib() -> Option<u64> {
    read_proc_meminfo_mib("MemTotal:")
}

/// Detecta a memória RAM disponível do sistema em MiB (via `/proc/meminfo` no Linux).
pub fn detect_available_memory_mib() -> Option<u64> {
    read_proc_meminfo_mib("MemAvailable:")
}

/// Detecta a resolução nativa do monitor principal quando disponível (sem presumir valores fixos).
pub fn detect_native_monitor_resolution() -> Option<(u32, u32)> {
    #[cfg(target_os = "linux")]
    {
        // 1. Lê modos nativos diretamente de `/sys/class/drm/card*-*/modes` (primeira linha = modo preferido)
        if let Ok(entries) = std::fs::read_dir("/sys/class/drm") {
            for entry in entries.flatten() {
                let path = entry.path();
                let status_path = path.join("status");
                let modes_path = path.join("modes");
                if let Ok(status) = std::fs::read_to_string(&status_path) {
                    if status.trim() == "connected" {
                        if let Ok(modes) = std::fs::read_to_string(&modes_path) {
                            if let Some(first) = modes.lines().next() {
                                if let Some((w_s, h_s)) = first.trim().split_once('x') {
                                    if let (Ok(w), Ok(h)) = (w_s.parse::<u32>(), h_s.parse::<u32>())
                                    {
                                        if w >= 640 && h >= 480 {
                                            return Some((w, h));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        None
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

fn read_proc_meminfo_mib(prefix: &str) -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        let content = std::fs::read_to_string("/proc/meminfo").ok()?;
        for line in content.lines() {
            if let Some(rest) = line.strip_prefix(prefix) {
                let kb: u64 = rest.split_whitespace().next()?.parse().ok()?;
                return Some(kb / 1024);
            }
        }
        None
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = prefix;
        None
    }
}

/// Formata um valor em MiB como `XXXX MiB (X.X GiB)`.
pub fn format_mib_display(mib: Option<u64>) -> String {
    mib.map(|v| format!("{v} MiB ({:.1} GiB)", v as f64 / 1024.0))
        .unwrap_or_else(|| "Unknown".to_string())
}

/// Gera resumo seguro de diagnóstico do sistema (sem informações sensíveis).
pub fn format_system_info_report() -> String {
    let caps = crate::platform::detect_system_capabilities();
    format_system_info_report_from_caps(&caps)
}

/// Gera relatório completo de diagnóstico a partir de capacidades já em cache.
pub fn format_system_info_report_from_caps(caps: &SystemCapabilities) -> String {
    let total_ram = format_mib_display(detect_system_memory_mib());
    let avail_ram = format_mib_display(detect_available_memory_mib());

    format!(
        "Crab {}\nOperating System: {}\nCPU Architecture: {}\nPhysical Memory: {} (Available: {})\nGraphics Adapter: {}\nDisplay Session: {}\nGameMode: {}\nMangoHud: {}\nVulkan: {}\nMesa Zink: {}\nUI Toolkit: Slint 1.19\nLicense: MIT",
        env!("CARGO_PKG_VERSION"),
        caps.os_name,
        caps.cpu_arch,
        total_ram,
        avail_ram,
        caps.gpu.primary_adapter_label(),
        caps.display_session,
        caps.gamemode.short_label(),
        caps.mangohud.short_label(),
        caps.vulkan_detail,
        caps.zink.short_label(),
    )
}
