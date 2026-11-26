use crate::core::tweaks::model::{
    CommandWrapper, GpuOffloadMethod, PerformanceTweaks, ResolvedTweaks, SystemCapabilities,
    ToolAvailability,
};

/// Resolve wrappers externos e variáveis de ambiente exclusivas do processo filho
/// para GameMode, MangoHud, GPU dedicada e Mesa Zink.
/// Suporta composição simultânea quando compatíveis, nunca mutando o ambiente do Crab.
pub fn resolve_performance_tweaks(
    tweaks: &PerformanceTweaks,
    caps: &SystemCapabilities,
    is_legacy_lwjgl: bool,
) -> ResolvedTweaks {
    let mut resolved = ResolvedTweaks::default();

    // 1. Discrete GPU Offload (switcherooctl ou variáveis específicas do driver/vendor)
    if tweaks.use_discrete_gpu {
        if caps.gpu.has_discrete_gpu() {
            match caps.gpu.offload_method {
                GpuOffloadMethod::Switcherooctl => {
                    resolved.wrappers.push(CommandWrapper {
                        program: "switcherooctl".to_string(),
                        args: vec!["launch".to_string()],
                    });
                    resolved.applied_descriptions.push(format!(
                        "Discrete GPU ({} via switcherooctl)",
                        caps.gpu
                            .discrete_gpu
                            .as_ref()
                            .map(|g| g.name.as_str())
                            .unwrap_or("dGPU")
                    ));
                }
                GpuOffloadMethod::NvidiaPrime => {
                    resolved
                        .env_vars
                        .insert("__NV_PRIME_RENDER_OFFLOAD".into(), "1".into());
                    resolved
                        .env_vars
                        .insert("__GLX_VENDOR_LIBRARY_NAME".into(), "nvidia".into());
                    resolved
                        .env_vars
                        .insert("__VK_LAYER_NV_optimus".into(), "NVIDIA_only".into());
                    resolved.applied_descriptions.push(format!(
                        "Discrete GPU ({} via NVIDIA PRIME Render Offload)",
                        caps.gpu
                            .discrete_gpu
                            .as_ref()
                            .map(|g| g.name.as_str())
                            .unwrap_or("NVIDIA")
                    ));
                }
                GpuOffloadMethod::MesaDriPrime => {
                    resolved.env_vars.insert("DRI_PRIME".into(), "1".into());
                    resolved.applied_descriptions.push(format!(
                        "Discrete GPU ({} via Mesa DRI_PRIME=1)",
                        caps.gpu
                            .discrete_gpu
                            .as_ref()
                            .map(|g| g.name.as_str())
                            .unwrap_or("dGPU")
                    ));
                }
                GpuOffloadMethod::None => {
                    resolved.warnings.push(
                        "Discrete GPU enabled, but no discrete GPU offload method is available."
                            .to_string(),
                    );
                }
            }
        } else {
            resolved.warnings.push(format!(
                "Discrete GPU requested, but unavailable: {}",
                caps.gpu.detail
            ));
        }
    }

    // 2. Feral GameMode (wrapper `gamemoderun`, aplicado somente ao processo do jogo)
    if tweaks.enable_gamemode {
        match &caps.gamemode {
            ToolAvailability::Available => {
                let prog = caps
                    .gamemode_executable
                    .as_ref()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "gamemoderun".to_string());
                resolved.wrappers.push(CommandWrapper {
                    program: prog,
                    args: Vec::new(),
                });
                resolved
                    .applied_descriptions
                    .push("Feral GameMode (gamemoderun)".to_string());
            }
            ToolAvailability::NotInstalled => {
                resolved.warnings.push(
                    "Feral GameMode is enabled but 'gamemoderun' is not installed; skipping."
                        .to_string(),
                );
            }
            ToolAvailability::Unavailable(reason) => {
                resolved.warnings.push(format!(
                    "Feral GameMode is unavailable ({reason}); skipping."
                ));
            }
        }
    }

    // 3. MangoHud (wrapper `mangohud`, com `--dlsym` para compatibilidade OpenGL no Minecraft)
    if tweaks.enable_mangohud {
        match &caps.mangohud {
            ToolAvailability::Available => {
                let prog = caps
                    .mangohud_executable
                    .as_ref()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "mangohud".to_string());
                // Minecraft usa OpenGL (ou OpenGL via Zink); `--dlsym` é o mecanismo oficial
                // do MangoHud para interceptar carregamento dinâmico de símbolos OpenGL no LWJGL.
                resolved.wrappers.push(CommandWrapper {
                    program: prog,
                    args: vec!["--dlsym".to_string()],
                });
                resolved
                    .applied_descriptions
                    .push("MangoHud (mangohud --dlsym)".to_string());
                if is_legacy_lwjgl {
                    resolved.warnings.push(
                        "MangoHud may have compatibility issues with legacy LWJGL 2 Minecraft versions."
                            .to_string(),
                    );
                }
            }
            ToolAvailability::NotInstalled => {
                resolved.warnings.push(
                    "MangoHud is enabled but 'mangohud' is not installed; skipping.".to_string(),
                );
            }
            ToolAvailability::Unavailable(reason) => {
                resolved
                    .warnings
                    .push(format!("MangoHud is unavailable ({reason}); skipping."));
            }
        }
    }

    // 4. Mesa Zink (OpenGL sobre Vulkan apenas para o processo filho do Minecraft)
    if tweaks.use_zink {
        // Quando combinado com NVIDIA PRIME, `__GLX_VENDOR_LIBRARY_NAME` precisa apontar para `mesa`
        // para que o carregador GLX encaminhe OpenGL ao Mesa Zink sobre o driver Vulkan.
        resolved
            .env_vars
            .insert("MESA_LOADER_DRIVER_OVERRIDE".into(), "zink".into());
        resolved
            .env_vars
            .insert("__GLX_VENDOR_LIBRARY_NAME".into(), "mesa".into());
        resolved
            .env_vars
            .insert("GALLIUM_DRIVER".into(), "zink".into());
        resolved
            .applied_descriptions
            .push("Mesa Zink (OpenGL over Vulkan)".to_string());

        if !caps.zink.is_available() {
            resolved.warnings.push(format!(
                "Mesa Zink compatibility could not be confirmed: {}",
                caps.zink.short_label()
            ));
        }
    }

    resolved
}
