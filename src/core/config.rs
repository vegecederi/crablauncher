use crate::core::java::providers::JavaProviderKind;
use crate::core::tweaks::model::PerformanceTweaks;
use crate::error::{LauncherError, Result};
use serde::{Deserialize, Serialize};

pub const CONFIG_SCHEMA_VERSION: u32 = 4;

/// Critério de ordenação da grade de instâncias (aplicado dentro de cada grupo).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstanceSortBy {
    #[default]
    CreatedAt,
    Name,
    LastLaunched,
    Manual,
}

impl InstanceSortBy {
    pub fn to_index(self) -> i32 {
        match self {
            Self::Name => 0,
            Self::CreatedAt => 1,
            Self::LastLaunched => 2,
            Self::Manual => 3,
        }
    }

    pub fn from_index(idx: i32) -> Self {
        match idx {
            0 => Self::Name,
            1 => Self::CreatedAt,
            2 => Self::LastLaunched,
            3 => Self::Manual,
            _ => Self::CreatedAt,
        }
    }
}

/// Tamanho dos ícones na grade de instâncias.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstanceIconSize {
    Small,
    #[default]
    Medium,
    Large,
}

impl InstanceIconSize {
    pub fn to_index(self) -> i32 {
        match self {
            Self::Small => 0,
            Self::Medium => 1,
            Self::Large => 2,
        }
    }

    pub fn from_index(idx: i32) -> Self {
        match idx {
            0 => Self::Small,
            2 => Self::Large,
            _ => Self::Medium,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortOrder {
    Ascending,
    #[default]
    Descending,
}

/// Espaçamento horizontal e vertical entre os blocos de instâncias na grade.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstanceGridSpacing {
    Compact,
    #[default]
    Normal,
    Spacious,
}

impl InstanceGridSpacing {
    pub fn to_index(self) -> i32 {
        match self {
            Self::Compact => 0,
            Self::Normal => 1,
            Self::Spacious => 2,
        }
    }

    pub fn from_index(idx: i32) -> Self {
        match idx {
            0 => Self::Compact,
            2 => Self::Spacious,
            _ => Self::Normal,
        }
    }
}

/// Posição encaixável da toolbar principal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolbarPosition {
    #[default]
    Top,
    Bottom,
}

impl ToolbarPosition {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Top => "top",
            Self::Bottom => "bottom",
        }
    }

    pub fn from_str_opt(s: &str) -> Self {
        match s {
            "bottom" => Self::Bottom,
            _ => Self::Top,
        }
    }
}

/// Posição encaixável do painel lateral de gerenciamento da instância.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SidebarPosition {
    #[default]
    Right,
    Left,
}

impl SidebarPosition {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Right => "right",
            Self::Left => "left",
        }
    }

    pub fn from_str_opt(s: &str) -> Self {
        match s {
            "left" => Self::Left,
            _ => Self::Right,
        }
    }
}

/// Posição encaixável da barra de status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatusBarPosition {
    #[default]
    Bottom,
    Top,
}

impl StatusBarPosition {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Bottom => "bottom",
            Self::Top => "top",
        }
    }

    pub fn from_str_opt(s: &str) -> Self {
        match s {
            "top" => Self::Top,
            _ => Self::Bottom,
        }
    }
}

/// Preferências de layout da janela principal (separadas das definições de tema).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct LayoutSettings {
    #[serde(default)]
    pub toolbar_position: ToolbarPosition,
    #[serde(default)]
    pub sidebar_position: SidebarPosition,
    #[serde(default)]
    pub status_bar_position: StatusBarPosition,
}

impl LayoutSettings {
    pub fn reset_defaults(&mut self) {
        self.toolbar_position = ToolbarPosition::Top;
        self.sidebar_position = SidebarPosition::Right;
        self.status_bar_position = StatusBarPosition::Bottom;
    }
}

/// Comportamento ao renomear uma instância.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RenameFolderPolicy {
    #[default]
    Never,
    Always,
    Ask,
}

/// Comportamento da janela principal do launcher quando o Minecraft inicia.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LaunchBehavior {
    #[default]
    KeepOpen,
    Minimize,
    Hide,
    Close,
}

impl LaunchBehavior {
    pub fn to_index(self) -> i32 {
        match self {
            Self::KeepOpen => 0,
            Self::Minimize => 1,
            Self::Hide => 2,
            Self::Close => 3,
        }
    }

    pub fn from_index(idx: i32) -> Self {
        match idx {
            1 => Self::Minimize,
            2 => Self::Hide,
            3 => Self::Close,
            _ => Self::KeepOpen,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::KeepOpen => "Keep Launcher Open",
            Self::Minimize => "Minimize Launcher",
            Self::Hide => "Hide Launcher",
            Self::Close => "Close Launcher",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProxyMode {
    #[default]
    System,
    None,
    Manual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProxyType {
    #[default]
    Http,
    Socks5,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneralSettings {
    #[serde(default)]
    pub sort_by: InstanceSortBy,
    #[serde(default)]
    pub sort_order: SortOrder,
    #[serde(default = "default_true")]
    pub confirm_instance_delete: bool,
    #[serde(default = "default_true")]
    pub confirm_group_delete: bool,
    #[serde(default = "default_true")]
    pub remember_selected_instance: bool,
    #[serde(default = "default_true")]
    pub remember_expanded_groups: bool,
    #[serde(default)]
    pub launch_on_double_click: bool,
    #[serde(default)]
    pub rename_folder_policy: RenameFolderPolicy,
    /// Ordem manual preservada entre alternâncias de critério de ordenação.
    #[serde(default)]
    pub manual_order: Vec<String>,
}

impl Default for GeneralSettings {
    fn default() -> Self {
        Self {
            sort_by: InstanceSortBy::CreatedAt,
            sort_order: SortOrder::Descending,
            confirm_instance_delete: true,
            confirm_group_delete: true,
            remember_selected_instance: true,
            remember_expanded_groups: true,
            launch_on_double_click: false,
            rename_folder_policy: RenameFolderPolicy::Never,
            manual_order: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppearanceSettings {
    #[serde(default = "default_theme_id")]
    pub theme_id: String,
    #[serde(default = "default_icon_pack_id")]
    pub icon_pack_id: String,
    #[serde(default = "default_accent")]
    pub accent_color: String,
    #[serde(default = "default_true")]
    pub compact_density: bool,
    #[serde(default)]
    pub icon_size: InstanceIconSize,
    #[serde(default)]
    pub grid_spacing: InstanceGridSpacing,
    #[serde(default = "default_true")]
    pub show_instance_name: bool,
    #[serde(default)]
    pub show_mc_version: bool,
    #[serde(default)]
    pub show_modloader_badge: bool,
    #[serde(default = "default_true")]
    pub show_status_indicator: bool,
    #[serde(default = "default_true")]
    pub show_groups: bool,
    #[serde(default)]
    pub hide_instance_search: bool,
    #[serde(default = "default_true")]
    pub show_status_bar: bool,
    #[serde(default = "default_true")]
    pub show_toolbar_labels: bool,
    #[serde(default)]
    pub compact_toolbar: bool,
    #[serde(default)]
    pub disable_special_effects: bool,
    #[serde(default = "default_wallpaper_opacity")]
    pub wallpaper_opacity: u32,
}

fn default_wallpaper_opacity() -> u32 {
    100
}

fn default_theme_id() -> String {
    "crab-dark".to_string()
}

fn default_icon_pack_id() -> String {
    "lucide".to_string()
}

impl Default for AppearanceSettings {
    fn default() -> Self {
        Self {
            theme_id: default_theme_id(),
            icon_pack_id: default_icon_pack_id(),
            accent_color: default_accent(),
            compact_density: true,
            icon_size: InstanceIconSize::Medium,
            grid_spacing: InstanceGridSpacing::Normal,
            show_instance_name: true,
            show_mc_version: false,
            show_modloader_badge: false,
            show_status_indicator: true,
            show_groups: true,
            hide_instance_search: false,
            show_status_bar: true,
            show_toolbar_labels: true,
            compact_toolbar: false,
            disable_special_effects: false,
            wallpaper_opacity: default_wallpaper_opacity(),
        }
    }
}

impl AppearanceSettings {
    /// Restaura apenas as preferências de layout da toolbar e barra de status.
    pub fn reset_toolbar_layout(&mut self) {
        self.hide_instance_search = false;
        self.show_status_bar = true;
        self.show_toolbar_labels = true;
        self.compact_toolbar = false;
    }

    /// Restaura apenas as preferências de aparência da grade de instâncias.
    pub fn reset_grid_appearance(&mut self) {
        self.icon_size = InstanceIconSize::Medium;
        self.grid_spacing = InstanceGridSpacing::Normal;
        self.show_instance_name = true;
        self.show_mc_version = false;
        self.show_modloader_badge = false;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JavaSettings {
    #[serde(default = "default_true")]
    pub auto_select: bool,
    #[serde(default)]
    pub preferred_provider: JavaProviderKind,
    #[serde(default = "default_true")]
    pub prefer_jre: bool,
    #[serde(default = "default_true")]
    pub prefer_installed: bool,
    #[serde(default)]
    pub manual_path: Option<String>,
    #[serde(default)]
    pub custom_paths: Vec<String>,
    #[serde(default = "default_true")]
    pub auto_download: bool,
}

impl Default for JavaSettings {
    fn default() -> Self {
        Self {
            auto_select: true,
            preferred_provider: JavaProviderKind::Mojang,
            prefer_jre: true,
            prefer_installed: true,
            manual_path: None,
            custom_paths: Vec::new(),
            auto_download: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MinecraftSettings {
    // Memory & JVM
    #[serde(default = "default_min_memory_mb")]
    pub min_memory_mb: u32,
    #[serde(default = "default_max_memory_mb")]
    pub max_memory_mb: u32,
    #[serde(default = "default_true")]
    pub warn_insufficient_memory: bool,
    #[serde(default = "default_true")]
    pub use_default_jvm_args: bool,
    #[serde(default)]
    pub jvm_args: String,

    // Game Window
    #[serde(default = "default_window_width")]
    pub window_width: u32,
    #[serde(default = "default_window_height")]
    pub window_height: u32,
    #[serde(default)]
    pub start_maximized: bool,
    #[serde(default)]
    pub start_fullscreen: bool,
    #[serde(default = "default_true")]
    pub use_custom_resolution: bool,

    // Launcher Behavior
    #[serde(default)]
    pub on_launch_behavior: LaunchBehavior,
    #[serde(default = "default_true")]
    pub reopen_on_exit: bool,
    #[serde(default = "default_true")]
    pub reopen_on_crash: bool,
    #[serde(default)]
    pub reopen_on_kill: bool,
    /// Campo legado da versão de schema 2, mantido apenas para migração sem perda.
    #[serde(default, skip_serializing)]
    pub restore_on_exit: Option<bool>,

    // Launch Preferences
    #[serde(default = "default_true")]
    pub check_java_compatibility: bool,
    #[serde(default = "default_true")]
    pub warn_incompatible_java: bool,
    #[serde(default = "default_true")]
    pub verify_before_launch: bool,
    #[serde(default = "default_true")]
    pub auto_repair_missing: bool,
    #[serde(default = "default_true")]
    pub allow_multiple_instances: bool,
    #[serde(default)]
    pub confirm_before_stopping: bool,

    // Console Behavior
    #[serde(default = "default_true")]
    pub show_console_on_launch: bool,
    #[serde(default = "default_true")]
    pub show_console_on_crash: bool,
    #[serde(default)]
    pub close_console_on_exit: bool,
    #[serde(default = "default_true")]
    pub console_autoscroll: bool,
    #[serde(default = "default_true")]
    pub console_word_wrap: bool,
    #[serde(default)]
    pub confirm_before_clearing_console: bool,

    // Console Appearance
    #[serde(default = "default_console_font_family")]
    pub console_font_family: String,
    #[serde(default = "default_console_font_size")]
    pub console_font_size: u32,
    #[serde(default = "default_console_line_spacing")]
    pub console_line_spacing: u32,
    #[serde(default = "default_true")]
    pub console_use_theme_font: bool,
    #[serde(default = "default_true")]
    pub console_colorize_levels: bool,
    #[serde(default = "default_true")]
    pub console_show_timestamps: bool,

    // Console Logging
    #[serde(default = "default_console_max_lines")]
    pub console_max_lines: usize,
    #[serde(default = "default_true")]
    pub preserve_complete_logs: bool,

    // Linux Performance Tweaks
    #[serde(default)]
    pub tweaks: PerformanceTweaks,
}

impl Default for MinecraftSettings {
    fn default() -> Self {
        Self {
            min_memory_mb: default_min_memory_mb(),
            max_memory_mb: default_max_memory_mb(),
            warn_insufficient_memory: true,
            use_default_jvm_args: true,
            jvm_args: String::new(),
            window_width: default_window_width(),
            window_height: default_window_height(),
            start_maximized: false,
            start_fullscreen: false,
            use_custom_resolution: true,
            on_launch_behavior: LaunchBehavior::KeepOpen,
            reopen_on_exit: true,
            reopen_on_crash: true,
            reopen_on_kill: false,
            restore_on_exit: None,
            check_java_compatibility: true,
            warn_incompatible_java: true,
            verify_before_launch: true,
            auto_repair_missing: true,
            allow_multiple_instances: true,
            confirm_before_stopping: false,
            show_console_on_launch: true,
            show_console_on_crash: true,
            close_console_on_exit: false,
            console_autoscroll: true,
            console_word_wrap: true,
            confirm_before_clearing_console: false,
            console_font_family: default_console_font_family(),
            console_font_size: default_console_font_size(),
            console_line_spacing: default_console_line_spacing(),
            console_use_theme_font: true,
            console_colorize_levels: true,
            console_show_timestamps: true,
            console_max_lines: default_console_max_lines(),
            preserve_complete_logs: true,
            tweaks: PerformanceTweaks::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadSettings {
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent: usize,
    #[serde(default)]
    pub speed_limit_kbps: u64,
    #[serde(default = "default_connect_timeout")]
    pub connect_timeout_secs: u64,
    #[serde(default = "default_retry_attempts")]
    pub retry_attempts: u32,
    #[serde(default = "default_true")]
    pub auto_retry: bool,
    #[serde(default = "default_true")]
    pub verify_downloads: bool,
}

impl Default for DownloadSettings {
    fn default() -> Self {
        Self {
            max_concurrent: default_max_concurrent(),
            speed_limit_kbps: 0,
            connect_timeout_secs: default_connect_timeout(),
            retry_attempts: default_retry_attempts(),
            auto_retry: true,
            verify_downloads: true,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolsSettings {
    #[serde(default)]
    pub file_manager: Option<String>,
    #[serde(default)]
    pub text_editor: Option<String>,
    #[serde(default)]
    pub terminal: Option<String>,
    #[serde(default)]
    pub browser: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxySettings {
    #[serde(default)]
    pub mode: ProxyMode,
    #[serde(default)]
    pub proxy_type: ProxyType,
    #[serde(default)]
    pub host: String,
    #[serde(default = "default_proxy_port")]
    pub port: u16,
    #[serde(default)]
    pub username: Option<String>,
}

impl Default for ProxySettings {
    fn default() -> Self {
        Self {
            mode: ProxyMode::System,
            proxy_type: ProxyType::Http,
            host: String::new(),
            port: default_proxy_port(),
            username: None,
        }
    }
}

/// Configuração global versionada do launcher persistida em `config.json`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FolderSettings {
    #[serde(default)]
    pub instances_dir: String,
    #[serde(default)]
    pub mods_dir: String,
    #[serde(default)]
    pub icons_dir: String,
    #[serde(default)]
    pub java_runtimes_dir: String,
    #[serde(default)]
    pub skins_dir: String,
    #[serde(default)]
    pub downloads_dir: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsoleSettings {
    #[serde(default = "default_console_history_limit")]
    pub log_history_limit: u32,
    #[serde(default = "default_true")]
    pub stop_logging_on_overflow: bool,
}

impl Default for ConsoleSettings {
    fn default() -> Self {
        Self {
            log_history_limit: default_console_history_limit(),
            stop_logging_on_overflow: true,
        }
    }
}

fn default_console_history_limit() -> u32 {
    150_000
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskSettings {
    #[serde(default = "default_concurrent_task_limit")]
    pub concurrent_task_limit: u32,
    #[serde(default = "default_concurrent_download_limit")]
    pub concurrent_download_limit: u32,
    #[serde(default = "default_retry_limit")]
    pub retry_limit: u32,
    #[serde(default = "default_http_timeout_secs")]
    pub http_timeout_secs: u32,
}

impl Default for TaskSettings {
    fn default() -> Self {
        Self {
            concurrent_task_limit: default_concurrent_task_limit(),
            concurrent_download_limit: default_concurrent_download_limit(),
            retry_limit: default_retry_limit(),
            http_timeout_secs: default_http_timeout_secs(),
        }
    }
}

fn default_concurrent_task_limit() -> u32 {
    10
}

fn default_concurrent_download_limit() -> u32 {
    6
}

fn default_retry_limit() -> u32 {
    1
}

fn default_http_timeout_secs() -> u32 {
    60
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LauncherConfig {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default = "default_language")]
    pub language: String,
    #[serde(default)]
    pub last_selected_instance: Option<String>,
    #[serde(default)]
    pub general: GeneralSettings,
    #[serde(default)]
    pub folders: FolderSettings,
    #[serde(default)]
    pub console: ConsoleSettings,
    #[serde(default)]
    pub tasks: TaskSettings,
    #[serde(default)]
    pub appearance: AppearanceSettings,
    #[serde(default)]
    pub layout: LayoutSettings,
    #[serde(default)]
    pub java: JavaSettings,
    #[serde(default)]
    pub minecraft: MinecraftSettings,
    #[serde(default)]
    pub downloads: DownloadSettings,
    #[serde(default)]
    pub tools: ToolsSettings,
    #[serde(default)]
    pub proxy: ProxySettings,
}

fn default_schema_version() -> u32 {
    CONFIG_SCHEMA_VERSION
}

fn default_language() -> String {
    "en".to_string()
}

fn default_accent() -> String {
    "#ff751f".to_string()
}

fn default_true() -> bool {
    true
}

fn default_min_memory_mb() -> u32 {
    512
}

fn default_max_memory_mb() -> u32 {
    2048
}

fn default_window_width() -> u32 {
    854
}

fn default_window_height() -> u32 {
    480
}

fn default_console_font_family() -> String {
    "JetBrains Mono".to_string()
}

fn default_console_font_size() -> u32 {
    12
}

fn default_console_line_spacing() -> u32 {
    2
}

fn default_console_max_lines() -> usize {
    4000
}

fn default_max_concurrent() -> usize {
    16
}

fn default_connect_timeout() -> u64 {
    10
}

fn default_retry_attempts() -> u32 {
    3
}

fn default_proxy_port() -> u16 {
    8080
}

impl Default for LauncherConfig {
    fn default() -> Self {
        Self {
            schema_version: CONFIG_SCHEMA_VERSION,
            language: default_language(),
            last_selected_instance: None,
            general: GeneralSettings::default(),
            folders: FolderSettings::default(),
            console: ConsoleSettings::default(),
            tasks: TaskSettings::default(),
            appearance: AppearanceSettings::default(),
            layout: LayoutSettings::default(),
            java: JavaSettings::default(),
            minecraft: MinecraftSettings::default(),
            downloads: DownloadSettings::default(),
            tools: ToolsSettings::default(),
            proxy: ProxySettings::default(),
        }
    }
}

impl LauncherConfig {
    /// Migra configurações de schemas anteriores preservando todas as preferências do usuário.
    /// Retorna `true` se houve alteração de schema ou campos migrados.
    pub fn migrate(&mut self) -> bool {
        let mut changed = false;
        if self.schema_version < CONFIG_SCHEMA_VERSION {
            if let Some(legacy_restore) = self.minecraft.restore_on_exit.take() {
                self.minecraft.reopen_on_exit = legacy_restore;
                self.minecraft.reopen_on_crash = legacy_restore;
            }
            if !self.minecraft.jvm_args.trim().is_empty() {
                self.minecraft.use_default_jvm_args = false;
            }
            if self.schema_version < 4 {
                // Novo padrão visual do grid oculta a versão abaixo do nome.
                self.appearance.show_mc_version = false;
            }
            self.schema_version = CONFIG_SCHEMA_VERSION;
            changed = true;
        }
        if self.appearance.wallpaper_opacity > 100 {
            self.appearance.wallpaper_opacity = 100;
        }
        if self.minecraft.console_font_family.trim().is_empty() {
            self.minecraft.console_font_family = default_console_font_family();
            changed = true;
        }
        changed
    }

    /// Valida os valores de configuração antes de persistir.
    pub fn validate(&self) -> Result<()> {
        if self.minecraft.min_memory_mb < 128 {
            return Err(LauncherError::Launch(
                "Minimum RAM must be at least 128 MiB.".to_string(),
            ));
        }
        if self.minecraft.max_memory_mb < self.minecraft.min_memory_mb {
            return Err(LauncherError::Launch(
                "Maximum RAM cannot be smaller than minimum RAM.".to_string(),
            ));
        }
        if self.minecraft.window_width < 320 || self.minecraft.window_height < 240 {
            return Err(LauncherError::Launch(
                "Window resolution must be at least 320x240.".to_string(),
            ));
        }
        if self.minecraft.console_font_size < 8 || self.minecraft.console_font_size > 32 {
            return Err(LauncherError::Launch(
                "Console font size must be between 8 and 32 px.".to_string(),
            ));
        }
        if self.minecraft.console_line_spacing > 16 {
            return Err(LauncherError::Launch(
                "Console line spacing must be between 0 and 16 px.".to_string(),
            ));
        }
        if self.minecraft.console_max_lines < 100 || self.minecraft.console_max_lines > 50_000 {
            return Err(LauncherError::Launch(
                "Maximum visible log lines must be between 100 and 50000.".to_string(),
            ));
        }
        if self.downloads.max_concurrent == 0 || self.downloads.max_concurrent > 64 {
            return Err(LauncherError::Launch(
                "Max concurrent downloads must be between 1 and 64.".to_string(),
            ));
        }
        if self.downloads.connect_timeout_secs < 3 || self.downloads.connect_timeout_secs > 120 {
            return Err(LauncherError::Launch(
                "Connection timeout must be between 3 and 120 seconds.".to_string(),
            ));
        }
        if self.proxy.mode == ProxyMode::Manual && self.proxy.host.trim().is_empty() {
            return Err(LauncherError::Launch(
                "Proxy host cannot be empty when manual proxy is enabled.".to_string(),
            ));
        }
        Ok(())
    }
}

/// Detecta argumentos JVM que conflitam com a memória gerenciada pelo launcher (`-Xms`, `-Xmx`).
pub fn detect_conflicting_jvm_args(raw: &str) -> Vec<String> {
    raw.split_whitespace()
        .filter(|tok| is_managed_memory_flag(tok))
        .map(|s| s.to_string())
        .collect()
}

/// Separa argumentos JVM válidos dos argumentos de memória conflitantes (`-Xms`/`-Xmx`).
pub fn strip_conflicting_memory_args(raw: &str) -> (Vec<String>, Vec<String>) {
    let mut kept = Vec::new();
    let mut stripped = Vec::new();
    for tok in raw.split_whitespace() {
        if is_managed_memory_flag(tok) {
            stripped.push(tok.to_string());
        } else {
            kept.push(tok.to_string());
        }
    }
    (kept, stripped)
}

fn is_managed_memory_flag(tok: &str) -> bool {
    tok.starts_with("-Xms")
        || tok.starts_with("-Xmx")
        || tok.starts_with("-XX:InitialHeapSize")
        || tok.starts_with("-XX:MaxHeapSize")
}

/// Valida limites de memória em relação à arquitetura da JVM (ex.: 32-bit x86 limita heap a ~2048 MiB).
pub fn validate_memory_for_java_arch(min_mb: u32, max_mb: u32, arch: &str) -> Result<()> {
    if min_mb < 128 {
        return Err(LauncherError::Launch(
            "Minimum RAM must be at least 128 MiB.".to_string(),
        ));
    }
    if max_mb < min_mb {
        return Err(LauncherError::Launch(
            "Maximum RAM cannot be smaller than minimum RAM.".to_string(),
        ));
    }
    let lower = arch.trim().to_ascii_lowercase();
    let is_32bit = matches!(lower.as_str(), "x86" | "i386" | "i686" | "arm" | "arm32");
    if is_32bit && max_mb > 2048 {
        return Err(LauncherError::Launch(format!(
            "Maximum RAM ({max_mb} MiB) exceeds the 2048 MiB practical limit for 32-bit Java ({arch})."
        )));
    }
    Ok(())
}
