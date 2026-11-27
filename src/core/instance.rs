use crate::core::config::{InstanceSortBy, LaunchBehavior, LauncherConfig};
use crate::core::tweaks::model::PerformanceTweaks;
use crate::core::version::VersionType;
use crate::error::{LauncherError, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const CURRENT_SCHEMA_VERSION: u32 = 1;
pub const MAX_INSTANCE_NAME_LEN: usize = 64;
const MAX_DIR_NAME_LEN: usize = 64;

/// Origem de uma configuração efetiva (Global vs Sobrescrita da Instância).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingSource {
    Global,
    InstanceOverride,
}

impl SettingSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::Global => "Global",
            Self::InstanceOverride => "Instance Override",
        }
    }
}

/// Estado de instalação de uma instância.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallationStatus {
    #[default]
    NotInstalled,
    Installing,
    Installed,
    Incomplete,
    Failed,
}

impl InstallationStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::NotInstalled => "Not installed",
            Self::Installing => "Installing...",
            Self::Installed => "Installed",
            Self::Incomplete => "Incomplete",
            Self::Failed => "Failed",
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Self::NotInstalled => "not_installed",
            Self::Installing => "installing",
            Self::Installed => "installed",
            Self::Incomplete => "incomplete",
            Self::Failed => "failed",
        }
    }
}

/// Estatísticas locais de tempo de jogo de uma instância (sem telemetria externa).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct PlaytimeStats {
    #[serde(default)]
    pub total_playtime_secs: u64,
    #[serde(default)]
    pub last_session_secs: u64,
    #[serde(default)]
    pub launch_count: u64,
}

impl PlaytimeStats {
    /// Formata uma duração em segundos de forma compacta (`< 1m`, `14m`, `2h 15m`).
    pub fn format_duration(secs: u64) -> String {
        if secs == 0 {
            return "Never played".to_string();
        }
        if secs < 60 {
            return "< 1m".to_string();
        }
        let total_mins = secs / 60;
        let hours = total_mins / 60;
        let mins = total_mins % 60;
        if hours == 0 {
            format!("{mins}m")
        } else if mins == 0 {
            format!("{hours}h")
        } else {
            format!("{hours}h {mins}m")
        }
    }

    pub fn formatted_total(&self) -> String {
        Self::format_duration(self.total_playtime_secs)
    }
}

/// Bloco de metadados sobre o estado da instalação.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct InstallationInfo {
    #[serde(default)]
    pub status: InstallationStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

/// Sobrescritas individuais de uma instância sobre as configurações globais do launcher.
/// Todos os campos usam `#[serde(default)]` para compatibilidade total com instâncias existentes.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct InstanceOverrides {
    // Java
    #[serde(default)]
    pub override_java: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub java_path: Option<String>,

    // Memory
    #[serde(default)]
    pub override_memory: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_memory_mb: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_memory_mb: Option<u32>,

    // JVM Arguments
    #[serde(default)]
    pub override_jvm_args: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub use_default_jvm_args: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jvm_args: Option<String>,

    // Game Window
    #[serde(default)]
    pub override_window: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window_width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window_height: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_fullscreen: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub use_custom_resolution: Option<bool>,

    // Launcher Behavior
    #[serde(default)]
    pub override_launcher_behavior: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_launch_behavior: Option<LaunchBehavior>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reopen_on_exit: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reopen_on_crash: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reopen_on_kill: Option<bool>,

    // Console
    #[serde(default)]
    pub override_console: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub show_console_on_launch: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub show_console_on_crash: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub close_console_on_exit: Option<bool>,

    // Linux Performance Tweaks
    #[serde(default)]
    pub override_tweaks: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tweaks: Option<PerformanceTweaks>,
}

impl InstanceOverrides {
    /// Conta quantas categorias de sobrescrita estão ativas nesta instância.
    pub fn active_override_count(&self) -> usize {
        [
            self.override_java,
            self.override_memory,
            self.override_jvm_args,
            self.override_window,
            self.override_launcher_behavior,
            self.override_console,
            self.override_tweaks,
        ]
        .into_iter()
        .filter(|b| *b)
        .count()
    }
}

/// Configurações efetivas já resolvidas (herança global + overrides da instância).
/// Utilizada de forma centralizada pelo Launch Engine e pelo Launch Summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectiveInstanceSettings {
    pub manual_java_path: Option<String>,
    pub java_source: SettingSource,

    pub min_memory_mb: u32,
    pub max_memory_mb: u32,
    pub memory_source: SettingSource,

    pub use_default_jvm_args: bool,
    pub jvm_args: String,
    pub jvm_args_source: SettingSource,

    pub window_width: u32,
    pub window_height: u32,
    pub start_fullscreen: bool,
    pub use_custom_resolution: bool,
    pub window_source: SettingSource,

    pub on_launch_behavior: LaunchBehavior,
    pub reopen_on_exit: bool,
    pub reopen_on_crash: bool,
    pub reopen_on_kill: bool,
    pub launcher_behavior_source: SettingSource,

    pub show_console_on_launch: bool,
    pub show_console_on_crash: bool,
    pub close_console_on_exit: bool,
    pub console_autoscroll: bool,
    pub console_word_wrap: bool,
    pub confirm_before_clearing_console: bool,
    pub console_max_lines: usize,
    pub preserve_complete_logs: bool,
    pub console_source: SettingSource,

    pub check_java_compatibility: bool,
    pub warn_incompatible_java: bool,
    pub verify_before_launch: bool,
    pub auto_repair_missing: bool,
    pub allow_multiple_instances: bool,
    pub confirm_before_stopping: bool,

    pub tweaks: PerformanceTweaks,
    pub tweaks_source: SettingSource,
}

impl EffectiveInstanceSettings {
    pub fn resolve(global: &LauncherConfig, overrides: &InstanceOverrides) -> Self {
        let (manual_java_path, java_source) = if overrides.override_java {
            (
                overrides.java_path.clone().filter(|p| !p.trim().is_empty()),
                SettingSource::InstanceOverride,
            )
        } else if !global.java.auto_select {
            (
                global
                    .java
                    .manual_path
                    .clone()
                    .filter(|p| !p.trim().is_empty()),
                SettingSource::Global,
            )
        } else {
            (None, SettingSource::Global)
        };

        let (min_memory_mb, max_memory_mb, memory_source) = if overrides.override_memory {
            let min = overrides
                .min_memory_mb
                .unwrap_or(global.minecraft.min_memory_mb);
            let max = overrides
                .max_memory_mb
                .unwrap_or(global.minecraft.max_memory_mb)
                .max(min);
            (min, max, SettingSource::InstanceOverride)
        } else {
            (
                global.minecraft.min_memory_mb,
                global
                    .minecraft
                    .max_memory_mb
                    .max(global.minecraft.min_memory_mb),
                SettingSource::Global,
            )
        };

        let (use_default_jvm_args, jvm_args, jvm_args_source) = if overrides.override_jvm_args {
            let use_def = overrides.use_default_jvm_args.unwrap_or(false);
            let args = if use_def {
                String::new()
            } else {
                overrides.jvm_args.clone().unwrap_or_default()
            };
            (use_def, args, SettingSource::InstanceOverride)
        } else {
            let use_def = global.minecraft.use_default_jvm_args;
            let args = if use_def {
                String::new()
            } else {
                global.minecraft.jvm_args.clone()
            };
            (use_def, args, SettingSource::Global)
        };

        let (window_width, window_height, start_fullscreen, use_custom_resolution, window_source) =
            if overrides.override_window {
                (
                    overrides
                        .window_width
                        .unwrap_or(global.minecraft.window_width),
                    overrides
                        .window_height
                        .unwrap_or(global.minecraft.window_height),
                    overrides
                        .start_fullscreen
                        .unwrap_or(global.minecraft.start_fullscreen),
                    overrides
                        .use_custom_resolution
                        .unwrap_or(global.minecraft.use_custom_resolution),
                    SettingSource::InstanceOverride,
                )
            } else {
                (
                    global.minecraft.window_width,
                    global.minecraft.window_height,
                    global.minecraft.start_fullscreen,
                    global.minecraft.use_custom_resolution,
                    SettingSource::Global,
                )
            };

        let (
            on_launch_behavior,
            reopen_on_exit,
            reopen_on_crash,
            reopen_on_kill,
            launcher_behavior_source,
        ) = if overrides.override_launcher_behavior {
            (
                overrides
                    .on_launch_behavior
                    .unwrap_or(global.minecraft.on_launch_behavior),
                overrides
                    .reopen_on_exit
                    .unwrap_or(global.minecraft.reopen_on_exit),
                overrides
                    .reopen_on_crash
                    .unwrap_or(global.minecraft.reopen_on_crash),
                overrides
                    .reopen_on_kill
                    .unwrap_or(global.minecraft.reopen_on_kill),
                SettingSource::InstanceOverride,
            )
        } else {
            (
                global.minecraft.on_launch_behavior,
                global.minecraft.reopen_on_exit,
                global.minecraft.reopen_on_crash,
                global.minecraft.reopen_on_kill,
                SettingSource::Global,
            )
        };

        let (show_console_on_launch, show_console_on_crash, close_console_on_exit, console_source) =
            if overrides.override_console {
                (
                    overrides
                        .show_console_on_launch
                        .unwrap_or(global.minecraft.show_console_on_launch),
                    overrides
                        .show_console_on_crash
                        .unwrap_or(global.minecraft.show_console_on_crash),
                    overrides
                        .close_console_on_exit
                        .unwrap_or(global.minecraft.close_console_on_exit),
                    SettingSource::InstanceOverride,
                )
            } else {
                (
                    global.minecraft.show_console_on_launch,
                    global.minecraft.show_console_on_crash,
                    global.minecraft.close_console_on_exit,
                    SettingSource::Global,
                )
            };

        let (tweaks, tweaks_source) = if overrides.override_tweaks {
            (
                overrides
                    .tweaks
                    .clone()
                    .unwrap_or_else(|| global.minecraft.tweaks.clone()),
                SettingSource::InstanceOverride,
            )
        } else {
            (global.minecraft.tweaks.clone(), SettingSource::Global)
        };

        Self {
            manual_java_path,
            java_source,
            min_memory_mb,
            max_memory_mb,
            memory_source,
            use_default_jvm_args,
            jvm_args,
            jvm_args_source,
            window_width,
            window_height,
            start_fullscreen,
            use_custom_resolution,
            window_source,
            on_launch_behavior,
            reopen_on_exit,
            reopen_on_crash,
            reopen_on_kill,
            launcher_behavior_source,
            show_console_on_launch,
            show_console_on_crash,
            close_console_on_exit,
            console_autoscroll: global.minecraft.console_autoscroll,
            console_word_wrap: global.minecraft.console_word_wrap,
            confirm_before_clearing_console: global.minecraft.confirm_before_clearing_console,
            console_max_lines: global.minecraft.console_max_lines,
            preserve_complete_logs: global.minecraft.preserve_complete_logs,
            console_source,
            check_java_compatibility: global.minecraft.check_java_compatibility,
            warn_incompatible_java: global.minecraft.warn_incompatible_java,
            verify_before_launch: global.minecraft.verify_before_launch,
            auto_repair_missing: global.minecraft.auto_repair_missing,
            allow_multiple_instances: global.minecraft.allow_multiple_instances,
            confirm_before_stopping: global.minecraft.confirm_before_stopping,
            tweaks,
            tweaks_source,
        }
    }
}

/// Metadados de um modpack gerenciado instalado via provedor (ex.: Modrinth).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManagedModpackInfo {
    pub provider: String,
    pub pack_name: String,
    pub pack_id: String,
    pub version_id: String,
    pub version_name: String,
    #[serde(default)]
    pub homepage_url: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub managed_files: Vec<String>,
}

/// Metadados persistidos em `instances/<dir_name>/instance.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstanceMetadata {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub group_id: Option<String>,
    #[serde(default)]
    pub icon_key: Option<String>,
    pub minecraft_version: String,
    pub version_type: VersionType,
    #[serde(default)]
    pub modloader: Option<String>,
    #[serde(default)]
    pub modloader_spec: Option<crate::core::components::model::ModloaderSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modpack: Option<ManagedModpackInfo>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_played: Option<DateTime<Utc>>,
    #[serde(default)]
    pub playtime: PlaytimeStats,
    #[serde(default)]
    pub installation: InstallationInfo,
    #[serde(default)]
    pub overrides: InstanceOverrides,
    /// Nome físico do diretório em `instances/`. Não precisa ser serializado no JSON,
    /// pois é determinado pelo diretório onde `instance.json` reside.
    #[serde(skip)]
    pub dir_name: String,
}

fn default_schema_version() -> u32 {
    CURRENT_SCHEMA_VERSION
}

impl InstanceMetadata {
    /// Cria uma nova instância com UUID v4 único e estado inicial `not_installed`.
    pub fn new(
        name: impl AsRef<str>,
        minecraft_version: impl AsRef<str>,
        version_type: VersionType,
        dir_name: impl Into<String>,
    ) -> Result<Self> {
        let validated_name = validate_instance_name(name.as_ref())?;
        let version = minecraft_version.as_ref().trim();
        if version.is_empty() {
            return Err(LauncherError::InvalidVersion(
                "version cannot be empty".to_string(),
            ));
        }

        let now = Utc::now();
        Ok(Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            id: Uuid::new_v4().to_string(),
            name: validated_name,
            group_id: None,
            icon_key: None,
            minecraft_version: version.to_string(),
            version_type,
            modloader: None,
            modloader_spec: None,
            modpack: None,
            created_at: now,
            updated_at: now,
            last_played: None,
            playtime: PlaytimeStats::default(),
            installation: InstallationInfo::default(),
            overrides: InstanceOverrides::default(),
            dir_name: dir_name.into(),
        })
    }

    /// Atualiza apenas o nome de exibição, preservando o diretório físico da instância.
    pub fn rename(&mut self, new_name: impl AsRef<str>) -> Result<()> {
        let validated = validate_instance_name(new_name.as_ref())?;
        self.name = validated;
        self.updated_at = Utc::now();
        Ok(())
    }

    pub fn set_installation_status(
        &mut self,
        status: InstallationStatus,
        last_error: Option<String>,
    ) {
        self.installation.status = status;
        self.installation.last_error = last_error;
        self.updated_at = Utc::now();
    }
}

/// Ordena instâncias conforme o critério escolhido (dentro de cada grupo ou grade plana).
pub fn sort_instances_in_place(
    instances: &mut [InstanceMetadata],
    sort_by: InstanceSortBy,
    manual_order: &[String],
) {
    match sort_by {
        InstanceSortBy::Name => {
            instances.sort_by_key(|a| a.name.to_lowercase());
        }
        InstanceSortBy::CreatedAt => {
            instances.sort_by_key(|i| std::cmp::Reverse(i.created_at));
        }
        InstanceSortBy::LastLaunched => {
            instances.sort_by(|a, b| {
                b.last_played
                    .cmp(&a.last_played)
                    .then_with(|| b.created_at.cmp(&a.created_at))
            });
        }
        InstanceSortBy::Manual => {
            instances.sort_by(|a, b| {
                let pa = manual_order
                    .iter()
                    .position(|id| id == &a.id)
                    .unwrap_or(usize::MAX);
                let pb = manual_order
                    .iter()
                    .position(|id| id == &b.id)
                    .unwrap_or(usize::MAX);
                pa.cmp(&pb).then_with(|| b.created_at.cmp(&a.created_at))
            });
        }
    }
}

/// Atualiza `manual_order` movendo `source_id` para a posição de `target_id`.
pub fn reorder_manual_list(
    manual_order: &mut Vec<String>,
    all_ids: &[String],
    source_id: &str,
    target_id: &str,
) {
    if source_id == target_id {
        return;
    }
    for id in all_ids {
        if !manual_order.contains(id) {
            manual_order.push(id.clone());
        }
    }
    if let Some(src_pos) = manual_order.iter().position(|id| id == source_id) {
        let item = manual_order.remove(src_pos);
        if let Some(dst_pos) = manual_order.iter().position(|id| id == target_id) {
            manual_order.insert(dst_pos, item);
        } else {
            manual_order.push(item);
        }
    }
}

/// Valida e normaliza o nome de exibição de uma instância.
pub fn validate_instance_name(raw: &str) -> Result<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(LauncherError::InvalidInstanceName(
            "Instance name cannot be empty.".to_string(),
        ));
    }
    if trimmed.chars().count() > MAX_INSTANCE_NAME_LEN {
        return Err(LauncherError::InvalidInstanceName(format!(
            "Instance name cannot exceed {MAX_INSTANCE_NAME_LEN} characters."
        )));
    }
    if trimmed.chars().any(|c| c.is_control()) {
        return Err(LauncherError::InvalidInstanceName(
            "Instance name cannot contain control characters.".to_string(),
        ));
    }
    Ok(trimmed.to_string())
}

/// Sanitiza um nome de instância para uso seguro como nome de diretório,
/// bloqueando path traversal, caracteres inválidos e nomes reservados no Windows.
pub fn sanitize_dir_name(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for ch in raw.trim().chars() {
        let forbidden = matches!(
            ch,
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\0'
        ) || ch.is_control();
        if forbidden {
            out.push('_');
        } else {
            out.push(ch);
        }
    }

    while out.contains("..") {
        out = out.replace("..", "_");
    }
    let trimmed = out.trim_matches(|c: char| c == '.' || c.is_whitespace());

    let mut sanitized: String = trimmed.chars().take(MAX_DIR_NAME_LEN).collect();
    sanitized = sanitized
        .trim_matches(|c: char| c == '.' || c.is_whitespace())
        .to_string();

    if sanitized.is_empty()
        || sanitized.chars().all(|c| c == '_')
        || is_reserved_windows_name(&sanitized)
    {
        return "instance".to_string();
    }

    sanitized
}

fn is_reserved_windows_name(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    let stem = upper.split('.').next().unwrap_or(&upper);
    matches!(
        stem,
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    )
}
