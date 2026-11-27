use crate::core::auth::account::LaunchCredentials;
use crate::core::components::manager::{build_patched_client_jar, ComponentManifest};
use crate::core::config::strip_conflicting_memory_args;
use crate::core::instance::{EffectiveInstanceSettings, InstanceMetadata};
use crate::core::java::resolver::required_java_major;
use crate::core::launch::arguments::resolve_version_arguments;
use crate::core::launch::classpath::{
    build_classpath_entries, classpath_separator, format_classpath,
};
use crate::core::launch::command_builder::build_launch_command;
use crate::core::minecraft::libraries::{resolve_libraries, TargetPlatform};
use crate::core::minecraft::metadata::VersionMetadata;
use crate::core::tweaks::model::SystemCapabilities;
use crate::core::tweaks::resolver::resolve_performance_tweaks;
use crate::error::{LauncherError, Result};
use crate::platform::paths::AppPaths;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Plano completo e imutável de execução de uma instância do Minecraft.
#[derive(Debug, Clone)]
pub struct LaunchPlan {
    pub instance_id: String,
    pub instance_name: String,
    pub java_executable: PathBuf,
    pub required_java_major: u32,
    pub working_dir: PathBuf,
    pub game_dir: PathBuf,
    pub natives_dir: PathBuf,
    pub classpath: Vec<PathBuf>,
    pub main_class: String,
    pub jvm_args: Vec<String>,
    pub game_args: Vec<String>,
    pub sensitive_values: Vec<String>,
    pub warnings: Vec<String>,
    pub is_legacy_lwjgl: bool,
    pub apply_legacy_fullscreen_in_options: bool,
}

/// Linha estruturada para exibição na janela View Launch Configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchSummaryRow {
    pub label: String,
    pub value: String,
    pub source: String,
}

/// Resumo efetivo de execução gerado a partir dos mesmos resolvedores do Launch Engine (sem tokens).
#[derive(Debug, Clone)]
pub struct LaunchSummary {
    pub instance_name: String,
    pub rows: Vec<LaunchSummaryRow>,
    pub warnings: Vec<String>,
    pub plain_text: String,
}

/// Lê a versão major do bytecode Java (ex.: 51=Java 7, 52=Java 8) de uma entrada `.class` dentro de um JAR.
pub fn jar_class_major_version(jar_path: &Path, class_entry: &str) -> Option<u16> {
    let f = std::fs::File::open(jar_path).ok()?;
    let mut archive = zip::ZipArchive::new(f).ok()?;
    let mut entry = archive.by_name(class_entry).ok()?;
    let mut header = [0u8; 8];
    std::io::Read::read_exact(&mut entry, &mut header).ok()?;
    if header[0..4] != [0xCA, 0xFE, 0xBA, 0xBE] {
        return None;
    }
    Some(u16::from_be_bytes([header[6], header[7]]))
}

/// Materializa assets virtuais para versões Legacy (`pre-1.6` / `legacy`, ex.: Beta 1.7.3)
/// copiando de `assets/objects/<h[0..2]>/<h>` para `assets/virtual/legacy/` e `.minecraft/resources/`
/// quando ausentes, garantindo áudio funcional sem re-baixar nem alterar objetos compartilhados.
pub fn materialize_legacy_assets(
    paths: &AppPaths,
    asset_index_id: &str,
    virtual_dir: &Path,
    game_dir: &Path,
) {
    let index_path = paths.asset_index_file(asset_index_id);
    let Ok(raw) = std::fs::read_to_string(&index_path) else {
        return;
    };
    let Ok(val) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return;
    };
    let Some(objects) = val.get("objects").and_then(|o| o.as_object()) else {
        return;
    };
    let resources_dir = game_dir.join("resources");
    for (rel_path, entry) in objects {
        let Some(hash) = entry.get("hash").and_then(|h| h.as_str()) else {
            continue;
        };
        if hash.len() < 2 || rel_path.contains("..") {
            continue;
        }
        let src = paths.asset_object_file(hash);
        if !src.is_file() {
            continue;
        }
        for target_root in [virtual_dir, &resources_dir] {
            let dst = target_root.join(rel_path);
            if !dst.is_file() {
                if let Some(parent) = dst.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let _ = std::fs::copy(&src, &dst);
            }
        }
    }
}

/// Constrói o plano de execução sem iniciar nenhum processo.
pub fn build_launch_plan(
    instance: &InstanceMetadata,
    version: &VersionMetadata,
    java_executable: PathBuf,
    credentials: &LaunchCredentials,
    settings: &EffectiveInstanceSettings,
    paths: &AppPaths,
    platform: &TargetPlatform,
) -> Result<LaunchPlan> {
    build_launch_plan_with_options(
        instance,
        version,
        java_executable,
        credentials,
        settings,
        paths,
        platform,
        false,
    )
}

/// Constrói o plano de execução com suporte explícito ao modo Demo (`is_demo_user` / `--demo`).
#[allow(clippy::too_many_arguments)]
pub fn build_launch_plan_with_options(
    instance: &InstanceMetadata,
    version: &VersionMetadata,
    java_executable: PathBuf,
    credentials: &LaunchCredentials,
    settings: &EffectiveInstanceSettings,
    paths: &AppPaths,
    platform: &TargetPlatform,
    is_demo: bool,
) -> Result<LaunchPlan> {
    let mut main_class = version.main_class.clone().ok_or_else(|| {
        LauncherError::Launch(format!("Version '{}' is missing mainClass.", version.id))
    })?;

    let game_dir = paths.instance_game_dir(&instance.dir_name);
    let instance_dir = paths.instance_dir(&instance.dir_name);
    let natives_dir = paths.version_natives_dir(&version.id, &platform.tag());
    let mut classpath_entries = build_classpath_entries(version, paths, platform);

    // Aplica Replace Minecraft.jar / Add to Minecraft.jar isolados por instância (sem alterar o JAR compartilhado)
    let comp_manifest = ComponentManifest::load_or_init(&instance_dir, instance);
    let base_jar = paths.version_jar_file(&version.id);
    let effective_jar = build_patched_client_jar(&base_jar, &instance_dir, &comp_manifest)
        .unwrap_or_else(|_| base_jar.clone());
    if effective_jar != base_jar {
        if let Some(slot) = classpath_entries.iter_mut().find(|p| *p == &base_jar) {
            *slot = effective_jar.clone();
        } else {
            classpath_entries.push(effective_jar.clone());
        }
    }
    let classpath_str = format_classpath(&classpath_entries);

    // Se o JAR efetivo (ex.: Beta 1.7.3 customizado) contém bytecode Java 8+ (major > 51),
    // o launchwrapper-1.5 (ASM 4.1) falha com IllegalArgumentException; inicializa Minecraft diretamente.
    let bypass_legacy_launchwrapper = main_class == "net.minecraft.launchwrapper.Launch"
        && jar_class_major_version(&effective_jar, "net/minecraft/client/Minecraft.class")
            .map(|major| major > 51)
            .unwrap_or(false);
    if bypass_legacy_launchwrapper {
        main_class = "net.minecraft.client.Minecraft".to_string();
    }

    let auth_session = if credentials.is_offline {
        "0".to_string()
    } else {
        format!("token:{}:{}", credentials.access_token, credentials.uuid)
    };

    let is_legacy_assets = matches!(version.assets.as_deref(), Some("legacy") | Some("pre-1.6"))
        || matches!(version.asset_index.id.as_str(), "legacy" | "pre-1.6");
    let game_assets_dir = if is_legacy_assets {
        let virtual_dir = paths.assets_dir().join("virtual").join("legacy");
        materialize_legacy_assets(paths, &version.asset_index.id, &virtual_dir, &game_dir);
        virtual_dir
    } else {
        paths.assets_dir()
    };

    let mut vars: HashMap<String, String> = HashMap::new();
    vars.insert("auth_player_name".into(), credentials.username.clone());
    vars.insert("auth_uuid".into(), credentials.uuid.clone());
    vars.insert("auth_access_token".into(), credentials.access_token.clone());
    vars.insert("auth_session".into(), auth_session.clone());
    vars.insert("auth_xuid".into(), credentials.xuid.clone());
    vars.insert("clientid".into(), "crab".into());
    vars.insert("user_type".into(), credentials.user_type.clone());
    vars.insert("user_properties".into(), "{}".into());
    vars.insert("version_name".into(), version.id.clone());
    vars.insert("version_type".into(), version.version_type.clone());
    vars.insert("game_directory".into(), game_dir.to_string_lossy().into());
    vars.insert(
        "assets_root".into(),
        paths.assets_dir().to_string_lossy().into(),
    );
    vars.insert(
        "game_assets".into(),
        game_assets_dir.to_string_lossy().into(),
    );
    vars.insert("assets_index_name".into(), version.asset_index.id.clone());
    vars.insert(
        "natives_directory".into(),
        natives_dir.to_string_lossy().into(),
    );
    vars.insert("launcher_name".into(), "Crab".into());
    vars.insert("launcher_version".into(), env!("CARGO_PKG_VERSION").into());
    vars.insert("classpath".into(), classpath_str);
    vars.insert("classpath_separator".into(), classpath_separator().into());
    vars.insert(
        "library_directory".into(),
        paths.libraries_dir().to_string_lossy().into(),
    );
    vars.insert("resolution_width".into(), settings.window_width.to_string());
    vars.insert(
        "resolution_height".into(),
        settings.window_height.to_string(),
    );

    // Quando fullscreen está ativo, evita passar resolução fixa que conflitaria em algumas versões.
    let use_resolution = settings.use_custom_resolution && !settings.start_fullscreen;
    let mut features = HashMap::new();
    features.insert("has_custom_resolution".into(), use_resolution);
    features.insert("is_demo_user".into(), is_demo);

    let (meta_jvm_args, mut game_args) =
        resolve_version_arguments(version, platform, &features, &vars)?;

    let supports_cli_window_flags = version.arguments.is_some()
        || game_args.iter().any(|a| a == "--version")
        || main_class.contains("net.minecraft.client.main.Main");

    // Versões 1.6–1.12.2 usam `minecraftArguments` sem `rules/features`; injeta `--demo` explicitamente.
    if is_demo && supports_cli_window_flags && !game_args.iter().any(|a| a == "--demo") {
        game_args.push("--demo".to_string());
    }

    let mut apply_legacy_fullscreen_in_options = false;
    if settings.start_fullscreen {
        if supports_cli_window_flags {
            if !game_args.iter().any(|a| a == "--fullscreen") {
                game_args.push("--fullscreen".to_string());
            }
        } else {
            // Versões pré-1.6 não aceitam `--fullscreen` via CLI; ajustamos apenas `fullscreen:true` em options.txt.
            apply_legacy_fullscreen_in_options = true;
        }
    } else if use_resolution
        && supports_cli_window_flags
        && !game_args.iter().any(|a| a == "--width")
    {
        game_args.push("--width".to_string());
        game_args.push(settings.window_width.to_string());
        game_args.push("--height".to_string());
        game_args.push(settings.window_height.to_string());
    }

    // Argumentos JVM conservadores: memória gerenciada + Java Agents da instância + argumentos opcionais + metadados oficiais.
    let mut jvm_args = vec![
        format!("-Xms{}M", settings.min_memory_mb),
        format!("-Xmx{}M", settings.max_memory_mb),
    ];
    if bypass_legacy_launchwrapper {
        jvm_args.push(format!("-Duser.home={}", instance_dir.display()));
        game_args = vec![credentials.username.clone(), auth_session.clone()];
    }
    jvm_args.extend(comp_manifest.java_agent_args(&instance_dir));

    let mut warnings = Vec::new();
    if !settings.use_default_jvm_args {
        let (clean_custom, stripped) = strip_conflicting_memory_args(&settings.jvm_args);
        if !stripped.is_empty() {
            warnings.push(format!(
                "Ignored conflicting memory JVM argument(s) managed by Crab: {}",
                stripped.join(", ")
            ));
        }
        jvm_args.extend(clean_custom);
    }

    jvm_args.extend(meta_jvm_args);

    let is_legacy_lwjgl = version
        .libraries
        .iter()
        .any(|l| l.name.starts_with("org.lwjgl.lwjgl:lwjgl:2."));

    let mut sensitive_values = Vec::new();
    if !credentials.is_offline && !credentials.access_token.is_empty() {
        sensitive_values.push(credentials.access_token.clone());
    }

    Ok(LaunchPlan {
        instance_id: instance.id.clone(),
        instance_name: instance.name.clone(),
        java_executable,
        required_java_major: required_java_major(version),
        working_dir: game_dir.clone(),
        game_dir,
        natives_dir,
        classpath: classpath_entries,
        main_class,
        jvm_args,
        game_args,
        sensitive_values,
        warnings,
        is_legacy_lwjgl,
        apply_legacy_fullscreen_in_options,
    })
}

/// Atualiza exclusivamente a entrada `fullscreen:true` em `.minecraft/options.txt` para versões legadas
/// que não suportam o argumento `--fullscreen` na linha de comando, preservando todas as demais linhas intactas.
pub fn ensure_legacy_fullscreen_option(game_dir: &Path) -> Result<()> {
    let options_path = game_dir.join("options.txt");
    if !options_path.exists() {
        std::fs::create_dir_all(game_dir).map_err(|source| LauncherError::Io {
            path: game_dir.to_path_buf(),
            source,
        })?;
        std::fs::write(&options_path, "fullscreen:true\n").map_err(|source| LauncherError::Io {
            path: options_path,
            source,
        })?;
        return Ok(());
    }

    let content = std::fs::read_to_string(&options_path).map_err(|source| LauncherError::Io {
        path: options_path.clone(),
        source,
    })?;

    let mut found = false;
    let mut lines: Vec<String> = Vec::new();
    for line in content.lines() {
        if line.starts_with("fullscreen:") {
            lines.push("fullscreen:true".to_string());
            found = true;
        } else {
            lines.push(line.to_string());
        }
    }
    if !found {
        lines.push("fullscreen:true".to_string());
    }
    let mut out = lines.join("\n");
    out.push('\n');
    std::fs::write(&options_path, out).map_err(|source| LauncherError::Io {
        path: options_path,
        source,
    })?;
    Ok(())
}

/// Verificação incremental rápida de arquivos obrigatórios antes de iniciar o jogo.
/// Verifica existência e tamanho mínimo do client JAR, asset index e bibliotecas aplicáveis sem re-hashear todos os assets.
pub fn verify_required_files_incremental(
    version: &VersionMetadata,
    paths: &AppPaths,
    platform: &TargetPlatform,
) -> Vec<PathBuf> {
    let mut missing = Vec::new();

    let client_jar = paths.version_jar_file(&version.id);
    if !file_exists_with_expected_size(&client_jar, version.downloads.client.size) {
        missing.push(client_jar);
    }

    let asset_index_file = paths.asset_index_file(&version.asset_index.id);
    if !file_exists_with_expected_size(&asset_index_file, Some(version.asset_index.size)) {
        missing.push(asset_index_file);
    }

    let empty_features = HashMap::new();
    for lib in resolve_libraries(&version.libraries, platform, &empty_features) {
        let p = paths.libraries_dir().join(&lib.relative_path);
        if !file_exists_with_expected_size(&p, lib.size) {
            missing.push(p);
        }
    }

    missing
}

fn file_exists_with_expected_size(path: &Path, expected_size: Option<u64>) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() || meta.len() == 0 {
        return false;
    }
    if let Some(exp) = expected_size {
        if exp > 0 && meta.len() != exp {
            return false;
        }
    }
    true
}

/// Constrói o resumo efetivo de lançamento (View Launch Configuration) usando exatamente
/// `EffectiveInstanceSettings` + `resolve_performance_tweaks` + `build_launch_command` (sem tokens).
#[allow(clippy::too_many_arguments)]
pub fn build_launch_summary(
    instance: &InstanceMetadata,
    version_meta: Option<&VersionMetadata>,
    effective: &EffectiveInstanceSettings,
    resolved_java_label: &str,
    resolved_java_path: Option<PathBuf>,
    caps: &SystemCapabilities,
    paths: &AppPaths,
    platform: &TargetPlatform,
) -> LaunchSummary {
    let modloader_label = instance
        .modloader_spec
        .as_ref()
        .map(|s| format!("{} {}", s.kind.label(), s.version))
        .unwrap_or_else(|| "Vanilla (None)".to_string());

    let is_legacy_lwjgl = version_meta
        .map(|v| {
            v.libraries
                .iter()
                .any(|l| l.name.starts_with("org.lwjgl.lwjgl:lwjgl:2."))
        })
        .unwrap_or(false);

    let resolved_tweaks = resolve_performance_tweaks(&effective.tweaks, caps, is_legacy_lwjgl);

    let resolution_value = if effective.use_custom_resolution {
        format!("{} × {}", effective.window_width, effective.window_height)
    } else {
        "Game Default".to_string()
    };

    let behavior_value = format!(
        "{} (Reopen: exit={}, crash={}, kill={})",
        effective.on_launch_behavior.label(),
        yes_no(effective.reopen_on_exit),
        yes_no(effective.reopen_on_crash),
        yes_no(effective.reopen_on_kill)
    );

    let jvm_args_value = if effective.use_default_jvm_args {
        "Launcher Defaults".to_string()
    } else if effective.jvm_args.trim().is_empty() {
        "Custom (empty)".to_string()
    } else {
        effective.jvm_args.trim().to_string()
    };

    let mut rows = vec![
        LaunchSummaryRow {
            label: "Minecraft Version".into(),
            value: instance.minecraft_version.clone(),
            source: "Instance".into(),
        },
        LaunchSummaryRow {
            label: "Modloader".into(),
            value: modloader_label,
            source: "Instance".into(),
        },
        LaunchSummaryRow {
            label: "Java Runtime".into(),
            value: resolved_java_label.to_string(),
            source: effective.java_source.label().into(),
        },
        LaunchSummaryRow {
            label: "Minimum RAM".into(),
            value: format!(
                "{} MiB (-Xms{}M)",
                effective.min_memory_mb, effective.min_memory_mb
            ),
            source: effective.memory_source.label().into(),
        },
        LaunchSummaryRow {
            label: "Maximum RAM".into(),
            value: format!(
                "{} MiB (-Xmx{}M)",
                effective.max_memory_mb, effective.max_memory_mb
            ),
            source: effective.memory_source.label().into(),
        },
        LaunchSummaryRow {
            label: "Window Resolution".into(),
            value: resolution_value,
            source: effective.window_source.label().into(),
        },
        LaunchSummaryRow {
            label: "Fullscreen".into(),
            value: yes_no(effective.start_fullscreen).into(),
            source: effective.window_source.label().into(),
        },
        LaunchSummaryRow {
            label: "Launcher Behavior".into(),
            value: behavior_value,
            source: effective.launcher_behavior_source.label().into(),
        },
        LaunchSummaryRow {
            label: "Feral GameMode".into(),
            value: tweak_summary_label(effective.tweaks.enable_gamemode, &caps.gamemode),
            source: effective.tweaks_source.label().into(),
        },
        LaunchSummaryRow {
            label: "MangoHud".into(),
            value: tweak_summary_label(effective.tweaks.enable_mangohud, &caps.mangohud),
            source: effective.tweaks_source.label().into(),
        },
        LaunchSummaryRow {
            label: "Discrete GPU".into(),
            value: if effective.tweaks.use_discrete_gpu {
                if caps.gpu.has_discrete_gpu() {
                    format!("Enabled ({})", caps.gpu.offload_method.label())
                } else {
                    "Enabled (Unavailable on this system)".to_string()
                }
            } else {
                "Disabled".to_string()
            },
            source: effective.tweaks_source.label().into(),
        },
        LaunchSummaryRow {
            label: "Mesa Zink".into(),
            value: if effective.tweaks.use_zink {
                format!("Enabled ({})", caps.zink.short_label())
            } else {
                "Disabled".to_string()
            },
            source: effective.tweaks_source.label().into(),
        },
        LaunchSummaryRow {
            label: "JVM Arguments".into(),
            value: jvm_args_value,
            source: effective.jvm_args_source.label().into(),
        },
    ];

    let mut warnings = resolved_tweaks.warnings.clone();

    // Constrói prévia de wrappers/env usando exatamente `build_launch_command` (com credenciais dummy seguras).
    if let (Some(vmeta), Some(java_path)) = (version_meta, resolved_java_path) {
        let dummy_creds = LaunchCredentials {
            username: "<player>".into(),
            uuid: "<uuid>".into(),
            access_token: "<redacted>".into(),
            xuid: "<redacted>".into(),
            user_type: "msa".into(),
            is_offline: true,
        };
        if let Ok(plan) = build_launch_plan(
            instance,
            vmeta,
            java_path,
            &dummy_creds,
            effective,
            paths,
            platform,
        ) {
            let cmd = build_launch_command(&plan, &resolved_tweaks);
            warnings = cmd.warnings.clone();
            if !cmd.applied_tweaks.is_empty() {
                rows.push(LaunchSummaryRow {
                    label: "Active Wrappers / Env".into(),
                    value: cmd.applied_tweaks.join(", "),
                    source: effective.tweaks_source.label().into(),
                });
            }
        }
    }

    let mut plain = format!("Crab Launch Configuration — {}\n", instance.name);
    plain.push_str(&"-".repeat(48));
    plain.push('\n');
    for r in &rows {
        plain.push_str(&format!("{}: {} [{}]\n", r.label, r.value, r.source));
    }
    if !warnings.is_empty() {
        plain.push_str("\nWarnings:\n");
        for w in &warnings {
            plain.push_str(&format!("- {w}\n"));
        }
    }

    LaunchSummary {
        instance_name: instance.name.clone(),
        rows,
        warnings,
        plain_text: plain,
    }
}

fn yes_no(b: bool) -> &'static str {
    if b {
        "Enabled"
    } else {
        "Disabled"
    }
}

fn tweak_summary_label(
    enabled: bool,
    avail: &crate::core::tweaks::model::ToolAvailability,
) -> String {
    if enabled {
        if avail.is_available() {
            "Enabled".to_string()
        } else {
            format!("Enabled ({})", avail.short_label())
        }
    } else {
        format!("Disabled ({})", avail.short_label())
    }
}
