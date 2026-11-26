use crab_launcher::core::auth::account::MinecraftAccount;
use crab_launcher::core::auth::microsoft::{
    distribution_client_id, generate_qr_matrix, poll_device_code_token, start_device_code_flow,
};
use crab_launcher::core::auth::minecraft::AccountManager;
use crab_launcher::core::auth::token_store::OsTokenStore;
use crab_launcher::core::components::manager::{build_patched_client_jar, ComponentManifest};
use crab_launcher::core::components::model::ModloaderKind;
use crab_launcher::core::components::profile::{compose_version_metadata, LoaderProfile};
use crab_launcher::core::config::{
    detect_conflicting_jvm_args, validate_memory_for_java_arch, InstanceGridSpacing,
    InstanceIconSize, InstanceSortBy, LaunchBehavior, LauncherConfig, SidebarPosition,
    StatusBarPosition, ToolbarPosition,
};
use crab_launcher::core::groups::{GroupManager, InstanceGroup};
use crab_launcher::core::icon_packs::{
    discover_icon_packs, import_icon_pack_zip, DEFAULT_ICON_PACK_ID,
};
use crab_launcher::core::installation::installer::InstanceInstaller;
use crab_launcher::core::installation::progress::{format_bytes, InstallProgress};
use crab_launcher::core::instance::{
    sort_instances_in_place, validate_instance_name, EffectiveInstanceSettings, InstallationStatus,
    InstanceMetadata, InstanceOverrides,
};
use crab_launcher::core::java::discovery::discover_java_runtimes;
use crab_launcher::core::java::installer::{
    install_java_build_with_progress, install_temurin_runtime, JavaInstallProgress,
};
use crab_launcher::core::java::providers::{
    fetch_provider_builds, AvailableJavaBuild, JavaPackageType, JavaProviderKind,
};
use crab_launcher::core::java::resolver::{required_java_major, select_compatible_runtime};
use crab_launcher::core::java::runtime::{probe_java_executable, test_jvm_arguments, JavaRuntime};
use crab_launcher::core::launch::command_builder::build_launch_command;
use crab_launcher::core::launch::console::{ConsoleBuffer, LogLevel};
use crab_launcher::core::launch::planner::{
    build_launch_plan, build_launch_summary, ensure_legacy_fullscreen_option,
    verify_required_files_incremental,
};
use crab_launcher::core::launch::process::{ProcessEvent, ProcessManager};
use crab_launcher::core::launch::supervisor::{
    spawn_supervised_launch, ProcessExitKind, SupervisorLaunchSpec,
};
use crab_launcher::core::manifest::ManifestSource;
use crab_launcher::core::minecraft::libraries::TargetPlatform;
use crab_launcher::core::minecraft::metadata::VersionMetadata;
use crab_launcher::core::modloaders::installer::{
    install_modloader_with_progress, remove_modloader, ModloaderInstallProgress,
};
use crab_launcher::core::modloaders::provider::{fetch_loader_versions, AvailableLoaderVersion};
use crab_launcher::core::mods::manager::{
    import_local_mod, list_local_mods, remove_local_mod, toggle_local_mod,
};
use crab_launcher::core::mods::platform::{
    download_and_record_content_with_progress, export_mods_list_markdown, fetch_and_cache_mod_icon,
    fetch_and_cache_remote_image, fetch_curseforge_versions, fetch_modrinth_project_details,
    fetch_modrinth_versions, render_markdown_plain, resolve_required_dependencies,
    search_curseforge_content, search_modrinth_filtered_paged, ContentKind, ModIndex, ModProvider,
    ModRemoteVersion, ModSearchResult, ModSortBy, SearchFilters,
};
use crab_launcher::core::mods::rich_text::{parse_rich_description, RichBlock};
use crab_launcher::core::presets::manager::{
    create_custom_preset, delete_custom_preset, discover_jvm_presets, duplicate_preset,
    export_preset, import_custom_preset, save_custom_preset,
};
use crab_launcher::core::presets::model::JvmPreset;
use crab_launcher::core::resources::logs::{
    delete_session_log, list_session_logs, logs_dir, read_log_lines,
};
use crab_launcher::core::resources::packs::{
    check_resource_pack_compatibility, has_known_shader_mod, import_pack, list_packs, remove_pack,
    shader_packs_dir, texture_packs_dir, PackCompatibility,
};
use crab_launcher::core::resources::screenshots::{
    delete_screenshot, list_screenshots, rename_screenshot, screenshots_dir,
};
use crab_launcher::core::resources::servers::{load_servers, save_servers, MinecraftServerEntry};
use crab_launcher::core::resources::worlds::{
    backup_world_copy, delete_world, export_world_zip, import_world_zip, list_worlds,
    read_world_seed, rename_world, reset_world_icon, saves_dir,
};
use crab_launcher::core::themes::loader::{discover_and_load_themes, import_theme_zip};
use crab_launcher::core::themes::model::{ResolvedTheme, RgbaColor};
use crab_launcher::core::tweaks::model::{PerformanceTweaks, SystemCapabilities};
use crab_launcher::core::tweaks::resolver::resolve_performance_tweaks;
use crab_launcher::core::version::{MinecraftVersion, VersionType};
use crab_launcher::error::{LauncherError, Result};
use crab_launcher::platform::paths::AppPaths;
use crab_launcher::platform::system_info::{
    detect_available_memory_mib, detect_native_monitor_resolution, detect_system_memory_mib,
    format_mib_display, format_system_info_report_from_caps,
};
use crab_launcher::platform::{
    detect_system_capabilities, open_in_file_manager, open_mangohud_config, pick_icon_file,
    pick_java_executable, pick_toml_file, save_toml_file,
};
use crab_launcher::services::download::{DownloadManager, DEFAULT_MAX_CONCURRENT_DOWNLOADS};
use crab_launcher::services::http::build_http_client;
use crab_launcher::services::instance_manager::InstanceManager;
use crab_launcher::services::manifest_service::ManifestService;
use crab_launcher::services::storage::{
    ensure_base_directories, load_config, read_json, save_config,
};
use reqwest::Client;
use slint::Model;
use slint::{
    ComponentHandle, Image, ModelRc, Rgb8Pixel, SharedPixelBuffer, SharedString, VecModel, Weak,
};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use tokio::runtime::Handle;
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

slint::include_modules!();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SelectJavaTarget {
    GlobalSettings,
    EditInstance,
}

struct AppState {
    instances: Vec<InstanceMetadata>,
    groups: Vec<InstanceGroup>,
    versions: Vec<MinecraftVersion>,
    latest_release: Option<String>,
    manifest_loaded: bool,
    config: LauncherConfig,
    java_runtimes: Vec<JavaRuntime>,
    java_catalog: Vec<AvailableJavaBuild>,
    loader_catalog: Vec<AvailableLoaderVersion>,
    add_loader_cache: HashMap<(ModloaderKind, String), Vec<AvailableLoaderVersion>>,
    select_icon_for_add: bool,
    themes: Vec<ResolvedTheme>,
    accounts: Vec<MinecraftAccount>,
    jvm_presets: Vec<JvmPreset>,
    native_resolution: Option<(u32, u32)>,
    capabilities: SystemCapabilities,
    select_java_target: SelectJavaTarget,
    last_launch_summary_text: String,
    active_installs: HashMap<String, CancellationToken>,
    active_install_cancel: Option<CancellationToken>,
    focused_install_id: Option<String>,
    running_instances: HashSet<String>,
    session_starts: HashMap<String, std::time::Instant>,
    console_buffers: HashMap<String, ConsoleBuffer>,
    focused_console_id: Option<String>,
    auth_cancel: Option<CancellationToken>,
}

impl AppState {
    fn new(capabilities: SystemCapabilities) -> Self {
        Self {
            instances: Vec::new(),
            groups: Vec::new(),
            versions: Vec::new(),
            latest_release: None,
            manifest_loaded: false,
            config: LauncherConfig::default(),
            java_runtimes: Vec::new(),
            java_catalog: Vec::new(),
            loader_catalog: Vec::new(),
            add_loader_cache: HashMap::new(),
            select_icon_for_add: false,
            themes: Vec::new(),
            accounts: Vec::new(),
            jvm_presets: Vec::new(),
            native_resolution: detect_native_monitor_resolution(),
            capabilities,
            select_java_target: SelectJavaTarget::EditInstance,
            last_launch_summary_text: String::new(),
            active_installs: HashMap::new(),
            active_install_cancel: None,
            focused_install_id: None,
            running_instances: HashSet::new(),
            session_starts: HashMap::new(),
            console_buffers: HashMap::new(),
            focused_console_id: None,
            auth_cancel: None,
        }
    }
}

thread_local! {
    static TASK_PROGRESS_WIN: std::cell::RefCell<Option<Weak<TaskProgressWindow>>> =
        const { std::cell::RefCell::new(None) };
    static REVIEW_MODS_WIN: std::cell::RefCell<Option<Weak<ReviewModsWindow>>> =
        const { std::cell::RefCell::new(None) };
}

static ACTIVE_TASK_OP_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn next_task_op_id() -> u64 {
    crab_launcher::services::download::set_global_download_paused(false);
    TASK_PAUSED.store(false, std::sync::atomic::Ordering::SeqCst);
    ACTIVE_TASK_OP_ID.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1
}

fn with_task_progress<F: FnOnce(&TaskProgressWindow)>(f: F) {
    TASK_PROGRESS_WIN.with(|cell| {
        if let Some(weak) = cell.borrow().as_ref() {
            if let Some(w) = weak.upgrade() {
                f(&w);
            }
        }
    });
}

#[allow(dead_code)]
fn with_task_progress_op<F: FnOnce(&TaskProgressWindow)>(op_id: u64, f: F) {
    if ACTIVE_TASK_OP_ID.load(std::sync::atomic::Ordering::SeqCst) != op_id {
        return;
    }
    with_task_progress(f);
}

fn center_window_over(child: &slint::Window, parent: &slint::Window) {
    let p_pos = parent.position();
    let p_size = parent.size();
    let c_size = child.size();
    let cw = if c_size.width > 0 { c_size.width } else { 400 };
    let ch = if c_size.height > 0 {
        c_size.height
    } else {
        200
    };
    let x = p_pos.x + ((p_size.width as i32 - cw as i32) / 2);
    let y = p_pos.y + ((p_size.height as i32 - ch as i32) / 2);
    child.set_position(slint::PhysicalPosition::new(x.max(0), y.max(0)));
}

#[derive(Clone)]
struct ThemeBroadcaster {
    main: Weak<MainWindow>,
    add: Weak<AddInstanceWindow>,
    settings: Weak<SettingsWindow>,
    ms_auth: Weak<MicrosoftAuthWindow>,
    install_java: Weak<InstallJavaWindow>,
    install_loader: Weak<InstallLoaderWindow>,
    change_group: Weak<ChangeGroupWindow>,
    select_java: Weak<SelectJavaWindow>,
    edit: Weak<EditInstanceWindow>,
    console: Weak<ConsoleWindow>,
    launch_summary: Weak<LaunchSummaryWindow>,
    manage_presets: Weak<ManageJvmPresetsWindow>,
    select_icon: Weak<SelectIconWindow>,
    task_progress: Weak<TaskProgressWindow>,
    download_mods: Weak<DownloadModsWindow>,
    rename_instance: Weak<RenameInstanceWindow>,
    change_version: Weak<ChangeVersionWindow>,
    remove_account: Weak<RemoveAccountWindow>,
    confirm_dialog: Weak<ConfirmDialogWindow>,
    text_input_dialog: Weak<TextInputDialogWindow>,
    server_dialog: Weak<ServerDialogWindow>,
    add_offline_account: Weak<AddOfflineAccountWindow>,
    copy_instance: Weak<CopyInstanceWindow>,
    review_mods: Weak<ReviewModsWindow>,
    check_updates: Weak<CheckUpdatesWindow>,
    verify_dependencies: Weak<VerifyDependenciesWindow>,
}

impl ThemeBroadcaster {
    /// Aplica o tema resolvido a TODAS as janelas Slint registradas (cada Window possui sua própria instância do global Theme).
    fn apply(&self, theme: &ResolvedTheme) {
        if let Some(w) = self.main.upgrade() {
            apply_theme_to_global(&w.global::<Theme>(), theme);
        }
        if let Some(w) = self.add.upgrade() {
            apply_theme_to_global(&w.global::<Theme>(), theme);
        }
        if let Some(w) = self.settings.upgrade() {
            apply_theme_to_global(&w.global::<Theme>(), theme);
        }
        if let Some(w) = self.ms_auth.upgrade() {
            apply_theme_to_global(&w.global::<Theme>(), theme);
        }
        if let Some(w) = self.install_java.upgrade() {
            apply_theme_to_global(&w.global::<Theme>(), theme);
        }
        if let Some(w) = self.install_loader.upgrade() {
            apply_theme_to_global(&w.global::<Theme>(), theme);
        }
        if let Some(w) = self.change_group.upgrade() {
            apply_theme_to_global(&w.global::<Theme>(), theme);
        }
        if let Some(w) = self.select_java.upgrade() {
            apply_theme_to_global(&w.global::<Theme>(), theme);
        }
        if let Some(w) = self.edit.upgrade() {
            apply_theme_to_global(&w.global::<Theme>(), theme);
        }
        if let Some(w) = self.console.upgrade() {
            apply_theme_to_global(&w.global::<Theme>(), theme);
        }
        if let Some(w) = self.launch_summary.upgrade() {
            apply_theme_to_global(&w.global::<Theme>(), theme);
        }
        if let Some(w) = self.manage_presets.upgrade() {
            apply_theme_to_global(&w.global::<Theme>(), theme);
        }
        if let Some(w) = self.select_icon.upgrade() {
            apply_theme_to_global(&w.global::<Theme>(), theme);
        }
        if let Some(w) = self.task_progress.upgrade() {
            apply_theme_to_global(&w.global::<Theme>(), theme);
        }
        if let Some(w) = self.download_mods.upgrade() {
            apply_theme_to_global(&w.global::<Theme>(), theme);
        }
        if let Some(w) = self.rename_instance.upgrade() {
            apply_theme_to_global(&w.global::<Theme>(), theme);
        }
        if let Some(w) = self.change_version.upgrade() {
            apply_theme_to_global(&w.global::<Theme>(), theme);
        }
        if let Some(w) = self.remove_account.upgrade() {
            apply_theme_to_global(&w.global::<Theme>(), theme);
        }
        if let Some(w) = self.confirm_dialog.upgrade() {
            apply_theme_to_global(&w.global::<Theme>(), theme);
        }
        if let Some(w) = self.text_input_dialog.upgrade() {
            apply_theme_to_global(&w.global::<Theme>(), theme);
        }
        if let Some(w) = self.server_dialog.upgrade() {
            apply_theme_to_global(&w.global::<Theme>(), theme);
        }
        if let Some(w) = self.add_offline_account.upgrade() {
            apply_theme_to_global(&w.global::<Theme>(), theme);
        }
        if let Some(w) = self.copy_instance.upgrade() {
            apply_theme_to_global(&w.global::<Theme>(), theme);
        }
        if let Some(w) = self.review_mods.upgrade() {
            apply_theme_to_global(&w.global::<Theme>(), theme);
        }
        if let Some(w) = self.check_updates.upgrade() {
            apply_theme_to_global(&w.global::<Theme>(), theme);
        }
        if let Some(w) = self.verify_dependencies.upgrade() {
            apply_theme_to_global(&w.global::<Theme>(), theme);
        }
    }

    /// Aplica o Icon Pack ativo a TODAS as janelas Slint registradas imediatamente.
    fn apply_icon_pack(&self, pack_id: &str) {
        if let Some(w) = self.main.upgrade() {
            apply_icon_pack_to_global(&w.global::<Theme>(), pack_id);
        }
        if let Some(w) = self.add.upgrade() {
            apply_icon_pack_to_global(&w.global::<Theme>(), pack_id);
        }
        if let Some(w) = self.settings.upgrade() {
            apply_icon_pack_to_global(&w.global::<Theme>(), pack_id);
        }
        if let Some(w) = self.ms_auth.upgrade() {
            apply_icon_pack_to_global(&w.global::<Theme>(), pack_id);
        }
        if let Some(w) = self.install_java.upgrade() {
            apply_icon_pack_to_global(&w.global::<Theme>(), pack_id);
        }
        if let Some(w) = self.install_loader.upgrade() {
            apply_icon_pack_to_global(&w.global::<Theme>(), pack_id);
        }
        if let Some(w) = self.change_group.upgrade() {
            apply_icon_pack_to_global(&w.global::<Theme>(), pack_id);
        }
        if let Some(w) = self.select_java.upgrade() {
            apply_icon_pack_to_global(&w.global::<Theme>(), pack_id);
        }
        if let Some(w) = self.edit.upgrade() {
            apply_icon_pack_to_global(&w.global::<Theme>(), pack_id);
        }
        if let Some(w) = self.console.upgrade() {
            apply_icon_pack_to_global(&w.global::<Theme>(), pack_id);
        }
        if let Some(w) = self.launch_summary.upgrade() {
            apply_icon_pack_to_global(&w.global::<Theme>(), pack_id);
        }
        if let Some(w) = self.manage_presets.upgrade() {
            apply_icon_pack_to_global(&w.global::<Theme>(), pack_id);
        }
        if let Some(w) = self.select_icon.upgrade() {
            apply_icon_pack_to_global(&w.global::<Theme>(), pack_id);
        }
        if let Some(w) = self.task_progress.upgrade() {
            apply_icon_pack_to_global(&w.global::<Theme>(), pack_id);
        }
        if let Some(w) = self.download_mods.upgrade() {
            apply_icon_pack_to_global(&w.global::<Theme>(), pack_id);
        }
        if let Some(w) = self.rename_instance.upgrade() {
            apply_icon_pack_to_global(&w.global::<Theme>(), pack_id);
        }
        if let Some(w) = self.change_version.upgrade() {
            apply_icon_pack_to_global(&w.global::<Theme>(), pack_id);
        }
        if let Some(w) = self.remove_account.upgrade() {
            apply_icon_pack_to_global(&w.global::<Theme>(), pack_id);
        }
        if let Some(w) = self.confirm_dialog.upgrade() {
            apply_icon_pack_to_global(&w.global::<Theme>(), pack_id);
        }
        if let Some(w) = self.text_input_dialog.upgrade() {
            apply_icon_pack_to_global(&w.global::<Theme>(), pack_id);
        }
        if let Some(w) = self.server_dialog.upgrade() {
            apply_icon_pack_to_global(&w.global::<Theme>(), pack_id);
        }
        if let Some(w) = self.add_offline_account.upgrade() {
            apply_icon_pack_to_global(&w.global::<Theme>(), pack_id);
        }
        if let Some(w) = self.copy_instance.upgrade() {
            apply_icon_pack_to_global(&w.global::<Theme>(), pack_id);
        }
        if let Some(w) = self.review_mods.upgrade() {
            apply_icon_pack_to_global(&w.global::<Theme>(), pack_id);
        }
    }
}

fn apply_icon_pack_to_global(theme_global: &Theme, pack_id: &str) {
    theme_global.set_icon_pack_id(SharedString::from(pack_id));
    theme_global.set_icon_pack_dark(pack_id == "lucide-dark");
}

#[derive(Clone)]
struct Services {
    rt: Handle,
    paths: AppPaths,
    http_client: Client,
    instance_manager: InstanceManager,
    group_manager: GroupManager,
    manifest_service: ManifestService,
    download_manager: DownloadManager,
    installer: InstanceInstaller,
    account_manager: AccountManager,
    process_manager: ProcessManager,
    state: Arc<Mutex<AppState>>,
}

/// Inicializa todos os serviços, configura os callbacks das janelas Slint e executa o event loop.
pub fn run(rt: Handle) -> Result<()> {
    let paths = AppPaths::resolve()?;
    let http_client = build_http_client()?;
    let instance_manager = InstanceManager::new(paths.clone());
    let group_manager = GroupManager::new(paths.clone());
    let manifest_service = ManifestService::new(http_client.clone(), paths.clone());
    let download_manager =
        DownloadManager::new(http_client.clone(), DEFAULT_MAX_CONCURRENT_DOWNLOADS);
    let installer = InstanceInstaller::new(
        paths.clone(),
        manifest_service.clone(),
        instance_manager.clone(),
        download_manager.clone(),
        DEFAULT_MAX_CONCURRENT_DOWNLOADS,
    );
    let token_store = OsTokenStore::new();
    let account_manager = AccountManager::new(http_client.clone(), paths.clone(), token_store);
    let process_manager = ProcessManager::new();
    let initial_caps = detect_system_capabilities();
    let state = Arc::new(Mutex::new(AppState::new(initial_caps.clone())));

    let services = Services {
        rt: rt.clone(),
        paths: paths.clone(),
        http_client,
        instance_manager,
        group_manager,
        manifest_service,
        download_manager,
        installer,
        account_manager,
        process_manager,
        state,
    };

    // Integração desktop Linux (ícone XDG + arquivo .desktop sem exigir root)
    let _ = crab_launcher::platform::desktop::ensure_linux_desktop_integration(include_bytes!(
        "../assets/app/logo-icon.svg"
    ));

    let main_window = MainWindow::new().expect("Failed to create MainWindow");
    let add_window = Rc::new(AddInstanceWindow::new().expect("Failed to create AddInstanceWindow"));
    let settings_window = Rc::new(SettingsWindow::new().expect("Failed to create SettingsWindow"));
    let ms_auth_window =
        Rc::new(MicrosoftAuthWindow::new().expect("Failed to create MicrosoftAuthWindow"));
    let install_java_window =
        Rc::new(InstallJavaWindow::new().expect("Failed to create InstallJavaWindow"));
    let install_loader_window =
        Rc::new(InstallLoaderWindow::new().expect("Failed to create InstallLoaderWindow"));
    let change_group_window =
        Rc::new(ChangeGroupWindow::new().expect("Failed to create ChangeGroupWindow"));
    let select_java_window =
        Rc::new(SelectJavaWindow::new().expect("Failed to create SelectJavaWindow"));
    let edit_window =
        Rc::new(EditInstanceWindow::new().expect("Failed to create EditInstanceWindow"));
    let console_window = Rc::new(ConsoleWindow::new().expect("Failed to create ConsoleWindow"));
    let launch_summary_window =
        Rc::new(LaunchSummaryWindow::new().expect("Failed to create LaunchSummaryWindow"));
    let manage_presets_window =
        Rc::new(ManageJvmPresetsWindow::new().expect("Failed to create ManageJvmPresetsWindow"));
    let select_icon_window =
        Rc::new(SelectIconWindow::new().expect("Failed to create SelectIconWindow"));
    let task_progress_window =
        Rc::new(TaskProgressWindow::new().expect("Failed to create TaskProgressWindow"));
    let download_mods_window =
        Rc::new(DownloadModsWindow::new().expect("Failed to create DownloadModsWindow"));
    let rename_instance_window =
        Rc::new(RenameInstanceWindow::new().expect("Failed to create RenameInstanceWindow"));
    let change_version_window =
        Rc::new(ChangeVersionWindow::new().expect("Failed to create ChangeVersionWindow"));
    let remove_account_window =
        Rc::new(RemoveAccountWindow::new().expect("Failed to create RemoveAccountWindow"));
    let confirm_dialog_window =
        Rc::new(ConfirmDialogWindow::new().expect("Failed to create ConfirmDialogWindow"));
    let text_input_dialog_window =
        Rc::new(TextInputDialogWindow::new().expect("Failed to create TextInputDialogWindow"));
    let server_dialog_window =
        Rc::new(ServerDialogWindow::new().expect("Failed to create ServerDialogWindow"));
    let add_offline_account_window =
        Rc::new(AddOfflineAccountWindow::new().expect("Failed to create AddOfflineAccountWindow"));
    let copy_instance_window =
        Rc::new(CopyInstanceWindow::new().expect("Failed to create CopyInstanceWindow"));
    let review_mods_window =
        Rc::new(ReviewModsWindow::new().expect("Failed to create ReviewModsWindow"));
    let check_updates_window =
        Rc::new(CheckUpdatesWindow::new().expect("Failed to create CheckUpdatesWindow"));
    let verify_dependencies_window = Rc::new(
        VerifyDependenciesWindow::new().expect("Failed to create VerifyDependenciesWindow"),
    );
    {
        let tp_weak = task_progress_window.as_weak();
        TASK_PROGRESS_WIN.with(|cell| {
            *cell.borrow_mut() = Some(tp_weak.clone());
        });
        task_progress_window.on_hide_clicked(move || {
            if let Some(w) = tp_weak.upgrade() {
                let _ = w.hide();
            }
        });
    }
    {
        let tp_weak = task_progress_window.as_weak();
        task_progress_window.on_pause_clicked(move || {
            TASK_PAUSED.store(true, std::sync::atomic::Ordering::SeqCst);
            crab_launcher::services::download::set_global_download_paused(true);
            if let Some(w) = tp_weak.upgrade() {
                w.set_is_paused(true);
                w.set_task_state(SharedString::from("Paused"));
            }
        });
    }
    {
        let tp_weak = task_progress_window.as_weak();
        task_progress_window.on_resume_clicked(move || {
            TASK_PAUSED.store(false, std::sync::atomic::Ordering::SeqCst);
            crab_launcher::services::download::set_global_download_paused(false);
            if let Some(w) = tp_weak.upgrade() {
                w.set_is_paused(false);
                w.set_task_state(SharedString::from("Running"));
            }
        });
    }
    {
        let tp_weak = task_progress_window.as_weak();
        let svc = services.clone();
        task_progress_window.on_abort_confirmed(move || {
            TASK_PAUSED.store(false, std::sync::atomic::Ordering::SeqCst);
            crab_launcher::services::download::set_global_download_paused(false);
            if let Ok(mut g) = svc.state.lock() {
                if let Some(token) = g.active_install_cancel.take() {
                    token.cancel();
                }
                for (_, token) in g.active_installs.drain() {
                    token.cancel();
                }
            }
            if let Some(w) = tp_weak.upgrade() {
                w.set_is_paused(false);
                w.set_is_cancelling(true);
                w.set_task_state(SharedString::from("Cancelling..."));
            }
        });
    }

    let theme_broadcaster = ThemeBroadcaster {
        main: main_window.as_weak(),
        add: add_window.as_weak(),
        settings: settings_window.as_weak(),
        ms_auth: ms_auth_window.as_weak(),
        install_java: install_java_window.as_weak(),
        install_loader: install_loader_window.as_weak(),
        change_group: change_group_window.as_weak(),
        select_java: select_java_window.as_weak(),
        edit: edit_window.as_weak(),
        console: console_window.as_weak(),
        launch_summary: launch_summary_window.as_weak(),
        manage_presets: manage_presets_window.as_weak(),
        select_icon: select_icon_window.as_weak(),
        task_progress: task_progress_window.as_weak(),
        download_mods: download_mods_window.as_weak(),
        rename_instance: rename_instance_window.as_weak(),
        change_version: change_version_window.as_weak(),
        remove_account: remove_account_window.as_weak(),
        confirm_dialog: confirm_dialog_window.as_weak(),
        text_input_dialog: text_input_dialog_window.as_weak(),
        server_dialog: server_dialog_window.as_weak(),
        add_offline_account: add_offline_account_window.as_weak(),
        copy_instance: copy_instance_window.as_weak(),
        review_mods: review_mods_window.as_weak(),
        check_updates: check_updates_window.as_weak(),
        verify_dependencies: verify_dependencies_window.as_weak(),
    };

    main_window.set_app_version(SharedString::from(concat!("v", env!("CARGO_PKG_VERSION"))));
    main_window.set_status_text(SharedString::from("Loading..."));
    settings_window.set_app_version(SharedString::from(concat!("v", env!("CARGO_PKG_VERSION"))));
    settings_window.set_data_dir_display(SharedString::from(
        paths.data_dir.to_string_lossy().as_ref(),
    ));
    settings_window.set_config_dir_display(SharedString::from(
        paths.config_dir.to_string_lossy().as_ref(),
    ));
    settings_window.set_cache_dir_display(SharedString::from(
        paths.cache_dir.to_string_lossy().as_ref(),
    ));
    apply_capabilities_to_settings(&settings_window, &initial_caps);

    if !services.account_manager.is_token_store_persistent() {
        settings_window.set_accounts_error_message(SharedString::from(
            "OS secret service unavailable: sessions will only persist in memory until Crab closes.",
        ));
    }

    wire_main_callbacks(
        &main_window,
        &add_window,
        &settings_window,
        &edit_window,
        &change_group_window,
        &console_window,
        &launch_summary_window,
        &select_icon_window,
        &services,
    );
    wire_add_window_callbacks(&main_window, &add_window, &select_icon_window, &services);
    wire_settings_callbacks(
        &main_window,
        &settings_window,
        &install_java_window,
        &select_java_window,
        &console_window,
        &manage_presets_window,
        &theme_broadcaster,
        &services,
    );
    wire_install_java_callbacks(&settings_window, &install_java_window, &services);
    wire_microsoft_auth_callbacks(&main_window, &settings_window, &ms_auth_window, &services);
    wire_edit_instance_callbacks(
        &main_window,
        &edit_window,
        &settings_window,
        &install_java_window,
        &install_loader_window,
        &change_group_window,
        &select_java_window,
        &console_window,
        &manage_presets_window,
        &services,
    );
    wire_select_java_callbacks(
        &settings_window,
        &edit_window,
        &select_java_window,
        &services,
    );
    wire_change_group_callbacks(&main_window, &edit_window, &change_group_window, &services);
    wire_install_loader_callbacks(
        &main_window,
        &edit_window,
        &install_loader_window,
        &services,
    );
    wire_console_callbacks(&main_window, &console_window, &services);
    wire_launch_summary_callbacks(&launch_summary_window, &services);
    wire_manage_jvm_presets_callbacks(
        &settings_window,
        &edit_window,
        &manage_presets_window,
        &services,
    );
    wire_select_icon_callbacks(&main_window, &add_window, &select_icon_window, &services);
    wire_download_mods_callbacks(
        &edit_window,
        &download_mods_window,
        &check_updates_window,
        &verify_dependencies_window,
        &services,
    );
    // AddInstanceWindow -> Group dropdown
    {
        let aw = add_window.as_weak();
        let svc = services.clone();
        add_window.on_group_selected(move |gid| {
            if let Some(a) = aw.upgrade() {
                let label = if gid.is_empty() {
                    "Ungrouped".to_string()
                } else {
                    svc.state
                        .lock()
                        .ok()
                        .and_then(|g| {
                            g.groups
                                .iter()
                                .find(|gr| gr.id == gid.as_str())
                                .map(|gr| gr.name.clone())
                        })
                        .unwrap_or_else(|| "Ungrouped".into())
                };
                a.set_selected_group_id(gid);
                a.set_selected_group_label(SharedString::from(label));
            }
        });
    }
    // Instance Settings -> Group dropdown (Click & Apply imediato)
    {
        let ew = edit_window.as_weak();
        let mw = main_window.as_weak();
        let svc = services.clone();
        edit_window.on_group_selected(move |gid| {
            let Some(e) = ew.upgrade() else { return };
            let inst_id = e.get_instance_id();
            if inst_id.is_empty() {
                return;
            }
            let label = if gid.is_empty() {
                "Ungrouped".to_string()
            } else {
                svc.state
                    .lock()
                    .ok()
                    .and_then(|g| {
                        g.groups
                            .iter()
                            .find(|gr| gr.id == gid.as_str())
                            .map(|gr| gr.name.clone())
                    })
                    .unwrap_or_else(|| "Ungrouped".into())
            };
            e.set_group_name_display(SharedString::from(label));
            e.set_selected_group_id(gid.clone());
            if let Some(m) = mw.upgrade() {
                m.invoke_move_instance_to_group(inst_id, gid);
            }
        });
    }
    // Servers -> Join: inicia a instância diretamente
    {
        let ew = edit_window.as_weak();
        let mw = main_window.as_weak();
        edit_window.on_join_server_clicked(move |_ip| {
            if let (Some(e), Some(m)) = (ew.upgrade(), mw.upgrade()) {
                let id = e.get_instance_id();
                if !id.is_empty() {
                    m.invoke_launch_instance_clicked(id);
                }
            }
        });
    }
    wire_rename_change_version_and_settings_accounts(
        &main_window,
        &settings_window,
        &edit_window,
        &rename_instance_window,
        &change_version_window,
        &remove_account_window,
        &theme_broadcaster,
        &services,
    );
    wire_native_dialog_windows(
        &main_window,
        &settings_window,
        &edit_window,
        &change_group_window,
        &select_icon_window,
        &download_mods_window,
        &confirm_dialog_window,
        &text_input_dialog_window,
        &server_dialog_window,
        &add_offline_account_window,
        &copy_instance_window,
        &review_mods_window,
        &services,
    );

    // Fechar a janela principal explicitamente encerra o event loop;
    // ocultar via "Hide Launcher" chama `main_window.hide()` sem encerrar o processo.
    main_window.window().on_close_requested(|| {
        let _ = slint::quit_event_loop();
        slint::CloseRequestResponse::HideWindow
    });

    // Carregamento inicial assíncrono
    {
        let main_weak = main_window.as_weak();
        let console_weak = console_window.as_weak();
        let broadcaster = theme_broadcaster.clone();
        let svc = services.clone();
        rt.spawn(async move {
            if let Err(err) = ensure_base_directories(&svc.paths).await {
                error!("Failed to initialize base directories: {err}");
                set_status(&main_weak, format!("Initialization error: {err}"));
                return;
            }

            let config = load_config(&svc.paths).await;
            let groups = svc.group_manager.load().await;
            let accounts = svc.account_manager.load_accounts().await;
            let instances = svc
                .instance_manager
                .list_instances()
                .await
                .unwrap_or_default();
            let java_runtimes = discover_java_runtimes(&svc.paths, &config.java.custom_paths).await;

            let selected_id = config
                .last_selected_instance
                .clone()
                .filter(|id| instances.iter().any(|i| &i.id == id))
                .or_else(|| instances.first().map(|i| i.id.clone()));

            let lang = config.language.clone();
            let hide_search = config.appearance.hide_instance_search;
            let show_status = config.appearance.show_status_bar;
            let show_labels = config.appearance.show_toolbar_labels;
            let compact_tb = config.appearance.compact_toolbar;
            let tb_pos = config.layout.toolbar_position.as_str().to_string();
            let sb_pos = config.layout.sidebar_position.as_str().to_string();
            let st_pos = config.layout.status_bar_position.as_str().to_string();
            let cfg_for_console = config.clone();

            let themes = discover_and_load_themes(&svc.paths.themes_dir());
            let jvm_presets = discover_jvm_presets(&svc.paths.jvm_presets_dir());
            let active_theme = themes
                .iter()
                .find(|t| t.id == config.appearance.theme_id)
                .cloned()
                .unwrap_or_default();
            let active_icon_pack_id = config.appearance.icon_pack_id.clone();

            if let Ok(mut guard) = svc.state.lock() {
                guard.config = config;
                guard.groups = groups;
                guard.accounts = accounts;
                guard.instances = instances;
                guard.java_runtimes = java_runtimes;
                guard.themes = themes;
                guard.jvm_presets = jvm_presets;
            }

            let state_clone = svc.state.clone();
            let _ = slint::invoke_from_event_loop(move || {
                broadcaster.apply(&active_theme);
                broadcaster.apply_icon_pack(&active_icon_pack_id);
                if let Some(cw) = console_weak.upgrade() {
                    apply_console_appearance(&cw, &cfg_for_console);
                }
                if let Some(win) = main_weak.upgrade() {
                    win.global::<Strings>().set_lang(SharedString::from(lang));
                    win.set_hide_instance_search(hide_search);
                    win.set_show_status_bar(show_status);
                    win.set_show_toolbar_labels(show_labels);
                    win.set_compact_toolbar(compact_tb);
                    win.set_toolbar_position(SharedString::from(tb_pos));
                    win.set_sidebar_position(SharedString::from(sb_pos));
                    win.set_status_bar_position(SharedString::from(st_pos));
                    apply_grid_appearance(&win, &cfg_for_console);
                    refresh_main_ui(&win, &state_clone, selected_id.as_deref());
                    win.set_status_text(SharedString::from("Ready"));
                }
            });
        });
    }

    info!("Starting Crab UI");
    main_window.show().expect("Failed to show Crab MainWindow");
    slint::run_event_loop_until_quit().expect("Slint event loop exited with error");
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn wire_main_callbacks(
    main_window: &MainWindow,
    add_window: &Rc<AddInstanceWindow>,
    settings_window: &Rc<SettingsWindow>,
    edit_window: &Rc<EditInstanceWindow>,
    change_group_window: &Rc<ChangeGroupWindow>,
    console_window: &Rc<ConsoleWindow>,
    launch_summary_window: &Rc<LaunchSummaryWindow>,
    select_icon_window: &Rc<SelectIconWindow>,
    svc: &Services,
) {
    // Add Instance
    {
        let add_win = Rc::clone(add_window);
        let main_weak = main_window.as_weak();
        let svc = svc.clone();

        main_window.on_add_instance_clicked(move || {
            add_win.set_instance_name(SharedString::new());
            add_win.set_user_edited_name(false);
            add_win.set_selected_icon_key(SharedString::new());
            add_win.set_has_selected_icon(false);
            add_win.set_search_query(SharedString::new());
            add_win.set_show_releases(true);
            add_win.set_show_snapshots(false);
            add_win.set_show_betas(false);
            add_win.set_show_alphas(false);
            add_win.set_selected_modloader(0);
            add_win.set_loader_search_query(SharedString::new());
            add_win.set_selected_loader_version(SharedString::new());
            add_win.set_loader_loading(false);
            add_win.set_loader_status_text(SharedString::new());
            add_win.set_loader_versions(ModelRc::from(Rc::new(VecModel::from(Vec::new()))));
            add_win.set_error_message(SharedString::new());
            add_win.set_is_creating(false);

            let already_loaded = svc.state.lock().map(|g| g.manifest_loaded).unwrap_or(false);
            if already_loaded {
                refresh_filtered_versions(&add_win, &svc.state, true);
            } else {
                trigger_manifest_fetch(&main_weak, &add_win.as_weak(), &svc);
            }
            let mut group_opts = vec![DropdownOption {
                id: SharedString::new(),
                label: SharedString::from("Ungrouped"),
                detail: SharedString::new(),
                icon: Image::default(),
                has_icon: false,
                enabled: true,
            }];
            if let Ok(g) = svc.state.lock() {
                for gr in &g.groups {
                    group_opts.push(DropdownOption {
                        id: SharedString::from(&gr.id),
                        label: SharedString::from(&gr.name),
                        detail: SharedString::new(),
                        icon: Image::default(),
                        has_icon: false,
                        enabled: true,
                    });
                }
            }
            add_win.set_group_options(ModelRc::from(Rc::new(VecModel::from(group_opts))));
            add_win.set_selected_group_id(SharedString::new());
            add_win.set_selected_group_label(SharedString::from("Ungrouped"));
            let _ = add_win.show();
        });
    }

    // Instance Search (filtra por nome, versão, modloader ou grupo sem alterar grupos persistidos)
    {
        let main_weak = main_window.as_weak();
        let svc = svc.clone();
        main_window.on_instance_search_changed(move |_| {
            if let Some(win) = main_weak.upgrade() {
                let selected = svc
                    .state
                    .lock()
                    .ok()
                    .and_then(|g| g.config.last_selected_instance.clone());
                refresh_main_ui(&win, &svc.state, selected.as_deref());
            }
        });
    }

    // Toolbar Context Menu Preferences (persiste imediatamente em config.json)
    {
        let svc = svc.clone();
        main_window.on_toolbar_prefs_changed(
            move |hide_search, show_status_bar, show_labels, compact| {
                let cfg_opt = svc.state.lock().ok().map(|mut g| {
                    g.config.appearance.hide_instance_search = hide_search;
                    g.config.appearance.show_status_bar = show_status_bar;
                    g.config.appearance.show_toolbar_labels = show_labels;
                    g.config.appearance.compact_toolbar = compact;
                    g.config.clone()
                });
                if let Some(cfg) = cfg_opt {
                    let paths = svc.paths.clone();
                    svc.rt.spawn(async move {
                        let _ = save_config(&paths, &cfg).await;
                    });
                }
            },
        );
    }

    // Layout Customization Mode persistence (independente dos temas)
    {
        let svc = svc.clone();
        main_window.on_layout_saved(move |tb_pos, sb_pos, st_pos| {
            let cfg_opt = svc.state.lock().ok().map(|mut g| {
                g.config.layout.toolbar_position = ToolbarPosition::from_str_opt(&tb_pos);
                g.config.layout.sidebar_position = SidebarPosition::from_str_opt(&sb_pos);
                g.config.layout.status_bar_position = StatusBarPosition::from_str_opt(&st_pos);
                g.config.clone()
            });
            if let Some(cfg) = cfg_opt {
                let paths = svc.paths.clone();
                svc.rt.spawn(async move {
                    let _ = save_config(&paths, &cfg).await;
                });
            }
        });
    }

    // Settings
    {
        let settings_win = Rc::clone(settings_window);
        let svc = svc.clone();
        main_window.on_settings_clicked(move || {
            settings_win.set_active_tab(0);
            populate_settings_window(&settings_win, &svc.state);
            let _ = settings_win.show();
        });
    }

    // Manage Accounts (abre diretamente Settings -> Accounts)
    {
        let settings_win = Rc::clone(settings_window);
        let svc = svc.clone();
        main_window.on_accounts_clicked(move || {
            settings_win.set_active_tab(6);
            sync_settings_accounts_ui(&settings_win, &svc);
            let _ = settings_win.show();
        });
    }

    // Open System Folder (menu Folders da toolbar)
    {
        let main_weak = main_window.as_weak();
        let svc = svc.clone();
        main_window.on_open_system_folder(move |key| {
            let dir = match key.as_str() {
                "launcher" => svc.paths.data_dir.clone(),
                "instances" => svc.paths.instances_dir(),
                "libraries" => svc.paths.libraries_dir(),
                "assets" => svc.paths.assets_dir(),
                "runtimes" => svc.paths.runtimes_dir(),
                "icons" => svc.paths.icon_packs_dir(),
                "themes" => svc.paths.themes_dir(),
                "logs" => svc.paths.logs_dir(),
                _ => svc.paths.data_dir.clone(),
            };
            let main_weak = main_weak.clone();
            svc.rt.spawn_blocking(move || {
                if let Err(err) = open_in_file_manager(&dir) {
                    set_status(&main_weak, format!("Failed to open folder: {err}"));
                }
            });
        });
    }

    // Select Account (menu suspenso Account Selector da toolbar)
    {
        let main_weak = main_window.as_weak();
        let svc = svc.clone();
        main_window.on_select_account(move |id| {
            let id_str = id.to_string();
            let main_weak = main_weak.clone();
            let svc = svc.clone();
            let rt = svc.rt.clone();
            rt.spawn(async move {
                if let Ok(accounts) = svc.account_manager.set_default_account(&id_str).await {
                    if let Ok(mut guard) = svc.state.lock() {
                        guard.accounts = accounts;
                    }
                    let state = svc.state.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(main) = main_weak.upgrade() {
                            update_active_account_label(&main, &state);
                        }
                    });
                }
            });
        });
    }

    // Create Offline Account
    {
        let main_weak = main_window.as_weak();
        let svc = svc.clone();
        main_window.on_create_offline_account(move |username| {
            let raw = username.to_string();
            let main_weak = main_weak.clone();
            let svc = svc.clone();
            let rt = svc.rt.clone();
            rt.spawn(async move {
                match svc.account_manager.create_offline_account(&raw).await {
                    Ok(created) => {
                        let accounts = svc.account_manager.load_accounts().await;
                        if let Ok(mut guard) = svc.state.lock() {
                            guard.accounts = accounts;
                        }
                        let state = svc.state.clone();
                        let uname = created.username;
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(main) = main_weak.upgrade() {
                                main.set_show_offline_account_dialog(false);
                                main.set_offline_account_error(SharedString::new());
                                update_active_account_label(&main, &state);
                                main.set_status_text(SharedString::from(format!(
                                    "Added offline account '{uname}'."
                                )));
                            }
                        });
                    }
                    Err(err) => {
                        let msg = err.to_string();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(main) = main_weak.upgrade() {
                                main.set_offline_account_error(SharedString::from(msg));
                            }
                        });
                    }
                }
            });
        });
    }

    // Select Instance
    {
        let main_weak = main_window.as_weak();
        let svc = svc.clone();
        main_window.on_select_instance(move |id| {
            let id_str = id.to_string();
            let cfg_opt = svc.state.lock().ok().map(|mut guard| {
                guard.config.last_selected_instance = Some(id_str.clone());
                guard.config.clone()
            });
            if let Some(win) = main_weak.upgrade() {
                refresh_selection_ui(&win, &svc.state, Some(&id_str));
            }
            if let Some(cfg) = cfg_opt {
                let paths = svc.paths.clone();
                svc.rt.spawn(async move {
                    let _ = save_config(&paths, &cfg).await;
                });
            }
        });
    }

    // Launch Instance
    {
        let main_weak = main_window.as_weak();
        let console_weak = console_window.as_weak();
        let svc = svc.clone();
        main_window.on_launch_instance_clicked(move |id| {
            start_launch_flow(&main_weak, &console_weak, &svc, id.to_string());
        });
    }

    // Stop Game (respeita Confirm Before Stopping Minecraft)
    {
        let main_weak = main_window.as_weak();
        let svc = svc.clone();
        main_window.on_stop_instance_clicked(move |id| {
            let id_str = id.to_string();
            let (confirm, name) = svc
                .state
                .lock()
                .ok()
                .map(|g| {
                    let inst_name = g
                        .instances
                        .iter()
                        .find(|i| i.id == id_str)
                        .map(|i| i.name.clone())
                        .unwrap_or_else(|| "Minecraft".to_string());
                    (g.config.minecraft.confirm_before_stopping, inst_name)
                })
                .unwrap_or((false, "Minecraft".to_string()));

            if confirm {
                if let Some(win) = main_weak.upgrade() {
                    win.set_stop_confirm_instance_id(SharedString::from(id_str));
                    win.set_stop_confirm_instance_name(SharedString::from(name));
                    win.set_show_stop_confirm_dialog(true);
                }
            } else if svc.process_manager.stop_instance(&id_str) {
                set_status(&main_weak, "Stopping Minecraft...");
            }
        });
    }

    // Confirm Stop Game
    {
        let main_weak = main_window.as_weak();
        let svc = svc.clone();
        main_window.on_confirm_stop_instance(move |id| {
            if svc.process_manager.stop_instance(&id) {
                set_status(&main_weak, "Stopping Minecraft...");
            }
        });
    }

    // View Launch Configuration (usa exatamente os mesmos resolvedores do Launch Engine)
    {
        let ls_win = Rc::clone(launch_summary_window);
        let svc = svc.clone();
        main_window.on_view_launch_config_clicked(move |id| {
            if populate_launch_summary_window(&ls_win, &svc, &id) {
                let _ = ls_win.show();
            }
        });
    }

    // Open Console
    {
        let console_win = Rc::clone(console_window);
        let svc = svc.clone();
        main_window.on_open_console_clicked(move |id| {
            let id_str = id.to_string();
            if let Ok(mut guard) = svc.state.lock() {
                guard.focused_console_id = Some(id_str.clone());
            }
            refresh_console_window(&console_win, &svc.state, &id_str);
            let _ = console_win.show();
        });
    }

    // Install / Repair Instance
    {
        let main_weak = main_window.as_weak();
        let svc = svc.clone();
        main_window.on_install_instance_clicked(move |id| {
            start_instance_installation(&main_weak, &svc, id.to_string());
        });
    }
    {
        let main_weak = main_window.as_weak();
        let svc = svc.clone();
        main_window.on_repair_instance_clicked(move |id| {
            start_instance_installation(&main_weak, &svc, id.to_string());
        });
    }

    // Edit Instance
    {
        let edit_win = Rc::clone(edit_window);
        let svc = svc.clone();
        main_window.on_edit_instance_clicked(move |id| {
            let id_str = id.to_string();
            edit_win.set_active_page(0);
            if populate_edit_instance_window(&edit_win, &svc, &id_str) {
                let _ = edit_win.show();
            }
        });
    }

    // Change Icon (abre janela dedicada SelectIconWindow para instância existente)
    {
        let si_win = Rc::clone(select_icon_window);
        let svc = svc.clone();
        main_window.on_change_icon_clicked(move |id| {
            if let Ok(mut g) = svc.state.lock() {
                g.select_icon_for_add = false;
            }
            if populate_select_icon_window(&si_win, &svc, &id) {
                let _ = si_win.show();
            }
        });
    }

    // Drag & Drop de instância para outro grupo (ou sem grupo quando group_id == "")
    {
        let main_weak = main_window.as_weak();
        let svc = svc.clone();
        main_window.on_move_instance_to_group(move |inst_id, group_id| {
            let id = inst_id.to_string();
            let gid = {
                let s = group_id.to_string();
                if s.is_empty() {
                    None
                } else {
                    Some(s)
                }
            };
            let main_weak = main_weak.clone();
            let svc = svc.clone();
            let rt = svc.rt.clone();
            rt.spawn(async move {
                if let Ok(updated) = svc.instance_manager.set_instance_group(&id, gid).await {
                    if let Ok(mut guard) = svc.state.lock() {
                        if let Some(item) = guard.instances.iter_mut().find(|i| i.id == id) {
                            *item = updated;
                        }
                    }
                    let state = svc.state.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(main) = main_weak.upgrade() {
                            refresh_main_ui(&main, &state, Some(&id));
                        }
                    });
                }
            });
        });
    }

    // Change Group (abre janela dedicada ChangeGroupWindow)
    {
        let cg_win = Rc::clone(change_group_window);
        let svc = svc.clone();
        main_window.on_change_group_clicked(move |id| {
            if populate_change_group_window(&cg_win, &svc.state, &id) {
                let _ = cg_win.show();
            }
        });
    }

    // Cancel Install
    {
        let main_weak = main_window.as_weak();
        let svc = svc.clone();
        main_window.on_cancel_install_clicked(move || {
            if let Ok(guard) = svc.state.lock() {
                if let Some(id) = &guard.focused_install_id {
                    if let Some(token) = guard.active_installs.get(id) {
                        token.cancel();
                    }
                }
            }
            set_status(&main_weak, "Cancelling installation...");
        });
    }

    // Open Folder
    {
        let main_weak = main_window.as_weak();
        let svc = svc.clone();
        main_window.on_open_folder_clicked(move |id| {
            let id_str = id.to_string();
            let main_weak = main_weak.clone();
            let im = svc.instance_manager.clone();
            svc.rt.spawn(async move {
                match im.instance_folder(&id_str).await {
                    Ok(folder) => {
                        let main_weak_inner = main_weak.clone();
                        let _ = tokio::task::spawn_blocking(move || {
                            if let Err(err) = open_in_file_manager(&folder) {
                                set_status(
                                    &main_weak_inner,
                                    format!("Failed to open folder: {err}"),
                                );
                            }
                        })
                        .await;
                    }
                    Err(err) => set_status(&main_weak, format!("Failed to locate folder: {err}")),
                }
            });
        });
    }

    // Copy Instance
    {
        let main_weak = main_window.as_weak();
        let svc = svc.clone();
        main_window.on_copy_instance_confirmed(
            move |src_id, new_name, saves, config, rpacks, screens| {
                let src = src_id.to_string();
                let name = new_name.to_string();
                let main_weak = main_weak.clone();
                let svc = svc.clone();
                let group_id = svc.state.lock().ok().and_then(|g| {
                    g.instances
                        .iter()
                        .find(|i| i.id == src)
                        .and_then(|i| i.group_id.clone())
                });
                svc.rt.spawn(async move {
                    match svc
                        .instance_manager
                        .copy_instance(&src, &name, group_id, saves, config, rpacks, screens)
                        .await
                    {
                        Ok(created) => {
                            let created_id = created.id.clone();
                            let created_name = created.name.clone();
                            if let Ok(mut guard) = svc.state.lock() {
                                guard.instances.insert(0, created);
                                guard.config.last_selected_instance = Some(created_id.clone());
                            }
                            let state = svc.state.clone();
                            let _ = slint::invoke_from_event_loop(move || {
                                if let Some(win) = main_weak.upgrade() {
                                    win.set_show_copy_dialog(false);
                                    win.set_copy_error(SharedString::new());
                                    refresh_main_ui(&win, &state, Some(&created_id));
                                    win.set_status_text(SharedString::from(format!(
                                        "Copied instance to '{created_name}'."
                                    )));
                                }
                            });
                        }
                        Err(err) => {
                            let msg = err.to_string();
                            let _ = slint::invoke_from_event_loop(move || {
                                if let Some(win) = main_weak.upgrade() {
                                    win.set_copy_error(SharedString::from(msg));
                                }
                            });
                        }
                    }
                });
            },
        );
    }

    // Delete Instance
    {
        let main_weak = main_window.as_weak();
        let svc = svc.clone();
        main_window.on_delete_confirmed(move |id| {
            let id_str = id.to_string();
            let main_weak = main_weak.clone();
            let svc = svc.clone();
            if let Ok(mut guard) = svc.state.lock() {
                if let Some(token) = guard.active_installs.remove(&id_str) {
                    token.cancel();
                }
            }
            svc.process_manager.stop_instance(&id_str);
            svc.rt.spawn(async move {
                match svc.instance_manager.delete_instance(&id_str).await {
                    Ok(()) => {
                        let (next_selected, cfg) = if let Ok(mut guard) = svc.state.lock() {
                            guard.instances.retain(|i| i.id != id_str);
                            let next = guard.instances.first().map(|i| i.id.clone());
                            guard.config.last_selected_instance = next.clone();
                            (next, guard.config.clone())
                        } else {
                            (None, LauncherConfig::default())
                        };
                        let _ = save_config(&svc.paths, &cfg).await;
                        let state = svc.state.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(win) = main_weak.upgrade() {
                                refresh_main_ui(&win, &state, next_selected.as_deref());
                                win.set_status_text(SharedString::from("Instance deleted"));
                            }
                        });
                    }
                    Err(err) => set_status(&main_weak, format!("Failed to delete instance: {err}")),
                }
            });
        });
    }

    // Group Create / Rename
    {
        let main_weak = main_window.as_weak();
        let svc = svc.clone();
        main_window.on_group_dialog_confirmed(move |target_id, name| {
            let target = target_id.to_string();
            let name_str = name.to_string();
            let main_weak = main_weak.clone();
            let svc = svc.clone();
            let rt = svc.rt.clone();
            rt.spawn(async move {
                let res = if target.is_empty() {
                    svc.group_manager.create_group(&name_str).await.map(|_| ())
                } else {
                    svc.group_manager
                        .rename_group(&target, &name_str)
                        .await
                        .map(|_| ())
                };
                match res {
                    Ok(()) => {
                        let groups = svc.group_manager.load().await;
                        let selected = svc.state.lock().ok().and_then(|mut g| {
                            g.groups = groups;
                            g.config.last_selected_instance.clone()
                        });
                        let state = svc.state.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(win) = main_weak.upgrade() {
                                win.set_show_group_dialog(false);
                                win.set_group_dialog_error(SharedString::new());
                                refresh_main_ui(&win, &state, selected.as_deref());
                            }
                        });
                    }
                    Err(err) => {
                        let msg = err.to_string();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(win) = main_weak.upgrade() {
                                win.set_group_dialog_error(SharedString::from(msg));
                            }
                        });
                    }
                }
            });
        });
    }

    // Toggle Group Collapsed
    {
        let main_weak = main_window.as_weak();
        let svc = svc.clone();
        main_window.on_toggle_group_collapsed(move |group_id| {
            let gid = group_id.to_string();
            let new_state = svc.state.lock().ok().and_then(|mut g| {
                let grp = g.groups.iter_mut().find(|x| x.id == gid)?;
                grp.collapsed = !grp.collapsed;
                Some((grp.collapsed, g.config.last_selected_instance.clone()))
            });
            if let Some((collapsed, selected)) = new_state {
                if let Some(win) = main_weak.upgrade() {
                    refresh_main_ui(&win, &svc.state, selected.as_deref());
                }
                let gm = svc.group_manager.clone();
                svc.rt.spawn(async move {
                    let _ = gm.set_collapsed(&gid, collapsed).await;
                });
            }
        });
    }

    // Move Group Up / Down
    {
        let main_weak = main_window.as_weak();
        let svc = svc.clone();
        main_window.on_move_group_up(move |group_id| {
            move_group_order(&main_weak, &svc, group_id.to_string(), true);
        });
    }
    {
        let main_weak = main_window.as_weak();
        let svc = svc.clone();
        main_window.on_move_group_down(move |group_id| {
            move_group_order(&main_weak, &svc, group_id.to_string(), false);
        });
    }

    // Delete Group (moves its instances to Ungrouped without touching files)
    {
        let main_weak = main_window.as_weak();
        let svc = svc.clone();
        main_window.on_delete_group_clicked(move |group_id| {
            let gid = group_id.to_string();
            let main_weak = main_weak.clone();
            let svc = svc.clone();
            let rt = svc.rt.clone();
            rt.spawn(async move {
                if svc.group_manager.delete_group(&gid).await.is_ok() {
                    let _ = svc.instance_manager.clear_group_from_instances(&gid).await;
                    let groups = svc.group_manager.load().await;
                    let instances = svc
                        .instance_manager
                        .list_instances()
                        .await
                        .unwrap_or_default();
                    let selected = svc.state.lock().ok().and_then(|mut g| {
                        g.groups = groups;
                        g.instances = instances;
                        g.config.last_selected_instance.clone()
                    });
                    let state = svc.state.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(win) = main_weak.upgrade() {
                            refresh_main_ui(&win, &state, selected.as_deref());
                            win.set_status_text(SharedString::from(
                                "Group deleted; instances moved to Ungrouped.",
                            ));
                        }
                    });
                }
            });
        });
    }
}

fn move_group_order(main_weak: &Weak<MainWindow>, svc: &Services, gid: String, up: bool) {
    let main_weak = main_weak.clone();
    let svc = svc.clone();
    svc.rt.spawn(async move {
        if let Ok(groups) = svc.group_manager.move_group(&gid, up).await {
            let selected = svc.state.lock().ok().and_then(|mut g| {
                g.groups = groups;
                g.config.last_selected_instance.clone()
            });
            let state = svc.state.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(win) = main_weak.upgrade() {
                    refresh_main_ui(&win, &state, selected.as_deref());
                }
            });
        }
    });
}

fn wire_add_window_callbacks(
    main_window: &MainWindow,
    add_window: &Rc<AddInstanceWindow>,
    select_icon_window: &Rc<SelectIconWindow>,
    svc: &Services,
) {
    // Change Icon no Add Instance (reutiliza SelectIconWindow)
    {
        let add_weak = add_window.as_weak();
        let si_win = Rc::clone(select_icon_window);
        let svc = svc.clone();
        add_window.on_change_icon_clicked(move || {
            if let Ok(mut g) = svc.state.lock() {
                g.select_icon_for_add = true;
            }
            let current_key = add_weak
                .upgrade()
                .map(|w| w.get_selected_icon_key().to_string())
                .unwrap_or_default();
            populate_select_icon_window_for_key(&si_win, &svc, "", "New Instance", &current_key);
            let _ = si_win.show();
        });
    }

    // Modloader selection & Minecraft version change -> busca assíncrona com cache
    {
        let add_weak = add_window.as_weak();
        let svc = svc.clone();
        add_window.on_version_selected(move |mc_ver| {
            if let Some(w) = add_weak.upgrade() {
                let ml_idx = w.get_selected_modloader();
                if let Some(kind) = modloader_kind_from_add_idx(ml_idx) {
                    trigger_add_loader_fetch(&add_weak, &svc, kind, mc_ver.to_string());
                }
            }
        });
    }
    {
        let add_weak = add_window.as_weak();
        let svc = svc.clone();
        add_window.on_modloader_changed(move |ml_idx| {
            let Some(w) = add_weak.upgrade() else { return };
            w.set_loader_search_query(SharedString::new());
            if ml_idx == 0 {
                w.set_selected_loader_version(SharedString::new());
                w.set_loader_loading(false);
                w.set_loader_status_text(SharedString::new());
                w.set_loader_versions(ModelRc::from(Rc::new(VecModel::from(Vec::new()))));
                return;
            }
            let mc_ver = w.get_selected_version_id().to_string();
            if mc_ver.is_empty() {
                w.set_loader_status_text(SharedString::from("Select a Minecraft version first."));
                return;
            }
            if let Some(kind) = modloader_kind_from_add_idx(ml_idx) {
                trigger_add_loader_fetch(&add_weak, &svc, kind, mc_ver);
            }
        });
    }
    {
        let add_weak = add_window.as_weak();
        let svc = svc.clone();
        add_window.on_loader_search_changed(move || {
            let Some(w) = add_weak.upgrade() else { return };
            let Some(kind) = modloader_kind_from_add_idx(w.get_selected_modloader()) else {
                return;
            };
            let mc_ver = w.get_selected_version_id().to_string();
            if let Some(list) = svc
                .state
                .lock()
                .ok()
                .and_then(|g| g.add_loader_cache.get(&(kind, mc_ver.clone())).cloned())
            {
                apply_add_loader_versions(&w, kind, &mc_ver, &list);
            }
        });
    }
    {
        let add_weak = add_window.as_weak();
        add_window.on_loader_version_selected(move |ver| {
            if let Some(w) = add_weak.upgrade() {
                w.set_selected_loader_version(ver);
            }
        });
    }

    {
        let add_weak = add_window.as_weak();
        let state = svc.state.clone();
        add_window.on_filters_changed(move || {
            if let Some(win) = add_weak.upgrade() {
                refresh_filtered_versions(&win, &state, false);
            }
        });
    }
    {
        let main_weak = main_window.as_weak();
        let add_weak = add_window.as_weak();
        let svc = svc.clone();
        add_window.on_refresh_versions(move || {
            trigger_manifest_fetch(&main_weak, &add_weak, &svc);
        });
    }
    {
        let add_weak = add_window.as_weak();
        add_window.on_cancel_clicked(move || {
            if let Some(win) = add_weak.upgrade() {
                let _ = win.hide();
            }
        });
    }
    {
        let main_weak = main_window.as_weak();
        let add_weak = add_window.as_weak();
        let svc = svc.clone();
        add_window.on_create_clicked(move || {
            let Some(add_win) = add_weak.upgrade() else {
                return;
            };
            let raw_name = add_win.get_instance_name().to_string();
            let selected_version_id = add_win.get_selected_version_id().to_string();
            let selected_icon_key = {
                let k = add_win.get_selected_icon_key().to_string();
                if k.is_empty() {
                    None
                } else {
                    Some(k)
                }
            };
            let selected_loader: Option<(ModloaderKind, String)> =
                modloader_kind_from_add_idx(add_win.get_selected_modloader()).and_then(|kind| {
                    let lv = add_win.get_selected_loader_version().to_string();
                    if lv.is_empty() {
                        None
                    } else {
                        Some((kind, lv))
                    }
                });

            if let Err(err) = validate_instance_name(&raw_name) {
                add_win.set_error_message(SharedString::from(err.to_string()));
                return;
            }
            if selected_version_id.trim().is_empty() {
                add_win.set_error_message(SharedString::from("Please select a Minecraft version."));
                return;
            }
            if add_win.get_selected_modloader() != 0 && selected_loader.is_none() {
                add_win.set_error_message(SharedString::from(
                    "Please select a compatible modloader version.",
                ));
                return;
            }

            let version_type = svc
                .state
                .lock()
                .ok()
                .and_then(|guard| {
                    guard
                        .versions
                        .iter()
                        .find(|v| v.id == selected_version_id)
                        .map(|v| v.version_type)
                })
                .unwrap_or(VersionType::Release);

            add_win.set_error_message(SharedString::new());
            add_win.set_is_creating(true);

            let main_weak = main_weak.clone();
            let add_weak = add_weak.clone();
            let rt = svc.rt.clone();
            let svc = svc.clone();

            rt.spawn(async move {
                // Todas as novas instâncias são criadas inicialmente em Ungrouped.
                match svc
                    .instance_manager
                    .create_instance(&raw_name, &selected_version_id, version_type)
                    .await
                {
                    Ok(mut created) => {
                        if let Some(icon_key) = selected_icon_key {
                            if let Ok(with_icon) = svc
                                .instance_manager
                                .set_instance_icon_key(&created.id, Some(icon_key))
                                .await
                            {
                                created = with_icon;
                            }
                        }

                        let cfg = if let Ok(mut guard) = svc.state.lock() {
                            guard.instances.insert(0, created.clone());
                            guard.config.last_selected_instance = Some(created.id.clone());
                            guard.config.clone()
                        } else {
                            LauncherConfig::default()
                        };
                        let _ = save_config(&svc.paths, &cfg).await;

                        let created_id = created.id.clone();
                        let created_name = created.name.clone();
                        let main_weak_ui = main_weak.clone();
                        let state = svc.state.clone();

                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(win) = add_weak.upgrade() {
                                win.set_is_creating(false);
                                let _ = win.hide();
                            }
                            if let Some(main) = main_weak_ui.upgrade() {
                                refresh_main_ui(&main, &state, Some(&created_id));
                                main.set_status_text(SharedString::from(format!(
                                    "Created '{}'. Starting installation...",
                                    created_name
                                )));
                            }
                        });

                        start_instance_installation_with_loader(
                            &main_weak,
                            &svc,
                            created.id,
                            selected_loader,
                        );
                    }
                    Err(err) => {
                        let msg = err.to_string();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(win) = add_weak.upgrade() {
                                win.set_is_creating(false);
                                win.set_error_message(SharedString::from(msg));
                            }
                        });
                    }
                }
            });
        });
    }
}

#[allow(clippy::too_many_arguments)]
fn wire_settings_callbacks(
    main_window: &MainWindow,
    settings_window: &Rc<SettingsWindow>,
    install_java_window: &Rc<InstallJavaWindow>,
    select_java_window: &Rc<SelectJavaWindow>,
    console_window: &Rc<ConsoleWindow>,
    manage_presets_window: &Rc<ManageJvmPresetsWindow>,
    theme_broadcaster: &ThemeBroadcaster,
    svc: &Services,
) {
    // Language dropdown (dinâmico + persistido ao salvar)
    {
        let main_weak = main_window.as_weak();
        let sw = settings_window.as_weak();
        settings_window.on_language_selected(move |lang_id| {
            let lid = lang_id.to_string();
            let label = language_display_label(&lid);
            if let Some(win) = sw.upgrade() {
                win.set_language(SharedString::from(&lid));
                win.set_language_label(SharedString::from(label));
                win.invoke_save_clicked();
            }
            if let Some(main) = main_weak.upgrade() {
                main.global::<Strings>().set_lang(SharedString::from(lid));
            }
        });
    }

    // Preferred Java Provider dropdown (aplica e persiste imediatamente)
    {
        let sw = settings_window.as_weak();
        let svc = svc.clone();
        settings_window.on_java_provider_selected(move |prov_id| {
            let (idx, label) = match prov_id.as_str() {
                "adoptium" => (1, "Eclipse Adoptium"),
                "azul" => (2, "Azul Zulu"),
                "semeru" => (3, "IBM Semeru"),
                _ => (0, "Mojang"),
            };
            if let Some(win) = sw.upgrade() {
                win.set_java_preferred_provider(idx);
                win.set_java_provider_id(prov_id);
                win.set_java_provider_label(SharedString::from(label));
            }
            let cfg_opt = svc.state.lock().ok().map(|mut g| {
                g.config.java.preferred_provider = JavaProviderKind::from_index(idx);
                g.config.clone()
            });
            if let Some(cfg) = cfg_opt {
                let paths = svc.paths.clone();
                svc.rt.spawn(async move {
                    let _ = save_config(&paths, &cfg).await;
                });
            }
        });
    }

    // Resolution Preset dropdown
    {
        let sw = settings_window.as_weak();
        let state = svc.state.clone();
        settings_window.on_resolution_preset_selected(move |preset_id| {
            let Some(win) = sw.upgrade() else { return };
            let native = state.lock().ok().and_then(|g| g.native_resolution);
            let pid = preset_id.to_string();
            if let Some((w, h)) = apply_resolution_preset(&pid, native) {
                win.set_window_width(SharedString::from(w.to_string()));
                win.set_window_height(SharedString::from(h.to_string()));
            }
            let (id, label) = resolution_preset_display(&pid, native);
            win.set_resolution_preset_id(SharedString::from(id));
            win.set_resolution_preset_label(SharedString::from(label));
            win.invoke_save_clicked();
        });
    }
    {
        let sw = settings_window.as_weak();
        let state = svc.state.clone();
        settings_window.on_resolution_fields_edited(move || {
            let Some(win) = sw.upgrade() else { return };
            let native = state.lock().ok().and_then(|g| g.native_resolution);
            let w: u32 = win.get_window_width().trim().parse().unwrap_or(0);
            let h: u32 = win.get_window_height().trim().parse().unwrap_or(0);
            let (id, label) = match_resolution_preset(w, h, native);
            win.set_resolution_preset_id(SharedString::from(id));
            win.set_resolution_preset_label(SharedString::from(label));
            win.invoke_save_clicked();
        });
    }

    // Memory Preset dropdown
    {
        let sw = settings_window.as_weak();
        settings_window.on_memory_preset_selected(move |preset_id| {
            let Some(win) = sw.upgrade() else { return };
            let pid = preset_id.to_string();
            if let Ok(mib) = pid.parse::<u32>() {
                win.set_max_memory_mb(SharedString::from(mib.to_string()));
                let min_mb: u32 = win.get_min_memory_mb().trim().parse().unwrap_or(512);
                if min_mb > mib {
                    win.set_min_memory_mb(SharedString::from(mib.min(512).to_string()));
                }
                let (id, label) = match_memory_preset(mib);
                win.set_memory_preset_id(SharedString::from(id));
                win.set_memory_preset_label(SharedString::from(label));
                win.set_memory_warning(SharedString::from(memory_warning_text(
                    mib,
                    detect_system_memory_mib(),
                    detect_available_memory_mib(),
                )));
            } else {
                win.set_memory_preset_id(SharedString::from("custom"));
                win.set_memory_preset_label(SharedString::from("Custom"));
            }
        });
    }
    {
        let sw = settings_window.as_weak();
        settings_window.on_memory_fields_edited(move || {
            let Some(win) = sw.upgrade() else { return };
            let max_mb: u32 = win.get_max_memory_mb().trim().parse().unwrap_or(0);
            let (id, label) = match_memory_preset(max_mb);
            win.set_memory_preset_id(SharedString::from(id));
            win.set_memory_preset_label(SharedString::from(label));
            win.set_memory_warning(SharedString::from(memory_warning_text(
                max_mb,
                detect_system_memory_mib(),
                detect_available_memory_mib(),
            )));
        });
    }

    // JVM Argument Preset dropdown
    {
        let sw = settings_window.as_weak();
        let state = svc.state.clone();
        settings_window.on_jvm_preset_selected(move |preset_id| {
            let Some(win) = sw.upgrade() else { return };
            let pid = preset_id.to_string();
            if pid == "custom" {
                win.set_use_default_jvm_args(false);
                win.set_jvm_preset_id(SharedString::from("custom"));
                win.set_jvm_preset_label(SharedString::from("Custom"));
                win.set_jvm_preset_description(SharedString::from(
                    "Custom user-defined JVM arguments.",
                ));
                win.set_jvm_preset_compat(SharedString::new());
                return;
            }
            let found = state
                .lock()
                .ok()
                .and_then(|g| g.jvm_presets.iter().find(|p| p.id == pid).cloned());
            if let Some(p) = found {
                win.set_use_default_jvm_args(p.id == "default");
                win.set_jvm_preset_id(SharedString::from(&p.id));
                win.set_jvm_preset_label(SharedString::from(&p.name));
                win.set_jvm_preset_description(SharedString::from(&p.description));
                win.set_jvm_preset_compat(SharedString::from(p.compatibility_summary()));
                win.set_jvm_args(SharedString::from(p.args_string()));
                win.set_jvm_conflict_warning(SharedString::new());
            }
        });
    }
    {
        let sw = settings_window.as_weak();
        settings_window.on_jvm_args_edited(move || {
            let Some(win) = sw.upgrade() else { return };
            win.set_use_default_jvm_args(false);
            win.set_jvm_preset_id(SharedString::from("custom"));
            win.set_jvm_preset_label(SharedString::from("Custom"));
            let conflicts = detect_conflicting_jvm_args(&win.get_jvm_args());
            win.set_jvm_conflict_warning(SharedString::from(if conflicts.is_empty() {
                String::new()
            } else {
                format!(
                    "Conflicting memory flags detected ({}): managed by Crab's Memory fields.",
                    conflicts.join(", ")
                )
            }));
        });
    }

    // Manage JVM Presets / Save as Preset
    {
        let mp_win = Rc::clone(manage_presets_window);
        let svc = svc.clone();
        settings_window.on_manage_jvm_presets_clicked(move || {
            populate_manage_presets_window(&mp_win, &svc, None);
            let _ = mp_win.show();
        });
    }
    {
        let mp_win = Rc::clone(manage_presets_window);
        let svc = svc.clone();
        settings_window.on_save_as_jvm_preset_clicked(move |raw_args| {
            let dir = svc.paths.jvm_presets_dir();
            if let Ok(created) = create_custom_preset(
                &dir,
                "Custom Preset",
                "Saved from Global Minecraft Settings",
                &raw_args,
                8,
            ) {
                let presets = discover_jvm_presets(&dir);
                if let Ok(mut g) = svc.state.lock() {
                    g.jvm_presets = presets;
                }
                populate_manage_presets_window(&mp_win, &svc, Some(&created.id));
                let _ = mp_win.show();
            }
        });
    }
    // Detect Java (abre SelectJavaWindow mirando Global Settings)
    {
        let sj_win = Rc::clone(select_java_window);
        let svc = svc.clone();
        settings_window.on_detect_java_selector_clicked(move || {
            if let Ok(mut g) = svc.state.lock() {
                g.select_java_target = SelectJavaTarget::GlobalSettings;
            }
            populate_select_java_window(&sj_win, &svc.state);
            let _ = sj_win.show();
        });
    }

    // Browse Java Executable
    {
        let settings_weak = settings_window.as_weak();
        let svc = svc.clone();
        settings_window.on_browse_java_clicked(move || {
            let settings_weak = settings_weak.clone();
            svc.rt.spawn_blocking(move || match pick_java_executable() {
                Ok(Some(path)) => {
                    let s = path.to_string_lossy().into_owned();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(w) = settings_weak.upgrade() {
                            w.set_java_auto_select(false);
                            w.set_java_manual_path(SharedString::from(s));
                        }
                    });
                }
                Ok(None) => {}
                Err(err) => {
                    let msg = err.to_string();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(w) = settings_weak.upgrade() {
                            w.set_java_status_message(SharedString::from(msg));
                        }
                    });
                }
            });
        });
    }

    // Remove Managed Java Runtime
    {
        let settings_weak = settings_window.as_weak();
        let svc = svc.clone();
        settings_window.on_remove_managed_java_clicked(move |path_str| {
            let p = PathBuf::from(path_str.to_string().trim());
            let runtimes_root = svc.paths.runtimes_dir();
            if !p.starts_with(&runtimes_root) {
                if let Some(w) = settings_weak.upgrade() {
                    w.set_java_status_message(SharedString::from(
                        "Only Crab-managed runtimes inside runtimes/ can be removed here.",
                    ));
                }
                return;
            }
            // Encontra o diretório raiz imediato do runtime gerenciado sob `runtimes/`
            let managed_dir = p
                .strip_prefix(&runtimes_root)
                .ok()
                .and_then(|rel| rel.components().next())
                .map(|c| runtimes_root.join(c));
            let Some(dir_to_remove) = managed_dir else {
                return;
            };
            let settings_weak = settings_weak.clone();
            let svc = svc.clone();
            let rt = svc.rt.clone();
            rt.spawn(async move {
                let res = tokio::fs::remove_dir_all(&dir_to_remove).await;
                let custom = svc
                    .state
                    .lock()
                    .map(|g| g.config.java.custom_paths.clone())
                    .unwrap_or_default();
                let runtimes = discover_java_runtimes(&svc.paths, &custom).await;
                if let Ok(mut g) = svc.state.lock() {
                    g.java_runtimes = runtimes;
                    if g.config.java.manual_path.as_deref() == Some(p.to_string_lossy().as_ref()) {
                        g.config.java.manual_path = None;
                    }
                }
                let msg = match res {
                    Ok(()) => "Removed managed Java runtime.".to_string(),
                    Err(err) => format!("Failed to remove managed runtime: {err}"),
                };
                let state = svc.state.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(w) = settings_weak.upgrade() {
                        w.set_java_manual_path(SharedString::new());
                        populate_java_list(&w, &state);
                        w.set_java_status_message(SharedString::from(msg));
                    }
                });
            });
        });
    }

    // Test JVM Arguments
    {
        let settings_weak = settings_window.as_weak();
        let svc = svc.clone();
        settings_window.on_test_jvm_args_clicked(move |java_path, min_s, max_s, raw_args| {
            let min_mb: u32 = min_s.trim().parse().unwrap_or(512);
            let max_mb: u32 = max_s.trim().parse().unwrap_or(2048);
            let raw = raw_args.to_string();
            let manual = java_path.trim().to_string();

            let conflicts = detect_conflicting_jvm_args(&raw);
            if let Some(w) = settings_weak.upgrade() {
                if conflicts.is_empty() {
                    w.set_jvm_conflict_warning(SharedString::new());
                } else {
                    w.set_jvm_conflict_warning(SharedString::from(format!(
                        "Conflicting memory flags detected ({}): managed by Crab's Memory fields.",
                        conflicts.join(", ")
                    )));
                }
                w.set_jvm_test_result(SharedString::from("Testing JVM arguments..."));
            }

            let target_java = if !manual.is_empty() {
                Some(PathBuf::from(manual))
            } else {
                svc.state
                    .lock()
                    .ok()
                    .and_then(|g| g.java_runtimes.first().map(|r| r.path.clone()))
            };

            let Some(exec) = target_java else {
                if let Some(w) = settings_weak.upgrade() {
                    w.set_jvm_test_result(SharedString::from(
                        "No Java runtime available to test arguments.",
                    ));
                }
                return;
            };

            let settings_weak = settings_weak.clone();
            svc.rt.spawn(async move {
                let msg = match test_jvm_arguments(&exec, min_mb, max_mb, &raw).await {
                    Ok(ok) => ok,
                    Err(err) => err.to_string(),
                };
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(w) = settings_weak.upgrade() {
                        w.set_jvm_test_result(SharedString::from(msg));
                    }
                });
            });
        });
    }

    // Copy JVM Arguments
    {
        let settings_weak = settings_window.as_weak();
        settings_window.on_copy_jvm_args_clicked(move |args| {
            if let Ok(mut cb) = arboard::Clipboard::new() {
                let _ = cb.set_text(args.trim().to_string());
            }
            if let Some(w) = settings_weak.upgrade() {
                w.set_jvm_test_result(SharedString::from("Copied JVM arguments to clipboard."));
            }
        });
    }

    // Open Logs Folder
    {
        let svc = svc.clone();
        settings_window.on_open_logs_folder_clicked(move || {
            let dir = svc.paths.logs_dir();
            svc.rt.spawn_blocking(move || {
                let _ = std::fs::create_dir_all(&dir);
                let _ = open_in_file_manager(&dir);
            });
        });
    }

    // Copy Latest Log
    {
        let main_weak = main_window.as_weak();
        let svc = svc.clone();
        settings_window.on_copy_latest_log_clicked(move || {
            let text_opt = svc.state.lock().ok().and_then(|g| {
                let id = g
                    .focused_console_id
                    .as_ref()
                    .or(g.config.last_selected_instance.as_ref())?;
                if let Some(buf) = g.console_buffers.get(id) {
                    let t = buf.to_plain_text();
                    if !t.is_empty() {
                        return Some(t);
                    }
                }
                let inst = g.instances.iter().find(|i| &i.id == id)?;
                let p = svc
                    .paths
                    .instance_game_dir(&inst.dir_name)
                    .join("logs")
                    .join("crab-latest.log");
                std::fs::read_to_string(p).ok()
            });
            match text_opt {
                Some(txt) if !txt.is_empty() => {
                    if let Ok(mut cb) = arboard::Clipboard::new() {
                        let _ = cb.set_text(txt);
                    }
                    set_status(&main_weak, "Copied latest log to clipboard.");
                }
                _ => set_status(&main_weak, "No recent Minecraft log available to copy."),
            }
        });
    }

    // Open MangoHud Configuration
    {
        let settings_weak = settings_window.as_weak();
        let svc = svc.clone();
        settings_window.on_open_mangohud_config_clicked(move || {
            let settings_weak = settings_weak.clone();
            svc.rt.spawn_blocking(move || {
                let feedback = match open_mangohud_config() {
                    Ok(path) => format!("Opened MangoHud config: {}", path.display()),
                    Err(err) => err.to_string(),
                };
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(w) = settings_weak.upgrade() {
                        w.set_tweaks_feedback(SharedString::from(feedback));
                    }
                });
            });
        });
    }

    // Refresh System Information (atualiza cache de capacidades sob demanda)
    {
        let settings_weak = settings_window.as_weak();
        let svc = svc.clone();
        settings_window.on_refresh_system_info_clicked(move || {
            let settings_weak = settings_weak.clone();
            let svc = svc.clone();
            svc.rt.spawn_blocking(move || {
                let caps = detect_system_capabilities();
                if let Ok(mut g) = svc.state.lock() {
                    g.capabilities = caps.clone();
                }
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(w) = settings_weak.upgrade() {
                        apply_capabilities_to_settings(&w, &caps);
                        w.set_tweaks_feedback(SharedString::from("System information refreshed."));
                    }
                });
            });
        });
    }

    // Open Install Java window
    {
        let ij_win = Rc::clone(install_java_window);
        let svc = svc.clone();
        settings_window.on_open_install_java_clicked(move || {
            ij_win.set_selected_provider(0);
            ij_win.set_selected_build_index(-1);
            ij_win.set_error_message(SharedString::new());
            ij_win.set_status_message(SharedString::new());
            let _ = ij_win.show();
            trigger_java_catalog_fetch(&ij_win.as_weak(), &svc, JavaProviderKind::Mojang);
        });
    }

    // Copy System Information
    {
        let main_weak = main_window.as_weak();
        let svc = svc.clone();
        settings_window.on_copy_system_info_clicked(move || {
            let report = svc
                .state
                .lock()
                .map(|g| format_system_info_report_from_caps(&g.capabilities))
                .unwrap_or_default();
            if let Ok(mut cb) = arboard::Clipboard::new() {
                let _ = cb.set_text(report);
            }
            set_status(&main_weak, "System information copied to clipboard.");
        });
    }

    // Apply Theme (dinamicamente em TODAS as janelas registradas + persistência em config.json)
    {
        let settings_weak = settings_window.as_weak();
        let broadcaster = theme_broadcaster.clone();
        let svc = svc.clone();
        settings_window.on_apply_theme_clicked(move |theme_id| {
            let tid = theme_id.to_string();
            let found = svc.state.lock().ok().and_then(|mut g| {
                let th = g.themes.iter().find(|t| t.id == tid)?.clone();
                g.config.appearance.theme_id = th.id.clone();
                Some((th, g.config.clone()))
            });
            if let Some((theme, cfg)) = found {
                broadcaster.apply(&theme);
                if let Some(sw) = settings_weak.upgrade() {
                    populate_themes_list(&sw, &svc.state);
                }
                let paths = svc.paths.clone();
                svc.rt.spawn(async move {
                    let _ = save_config(&paths, &cfg).await;
                });
            }
        });
    }

    // Open Themes Folder
    {
        let svc = svc.clone();
        settings_window.on_open_themes_folder_clicked(move || {
            let dir = svc.paths.themes_dir();
            svc.rt.spawn_blocking(move || {
                let _ = std::fs::create_dir_all(&dir);
                let _ = open_in_file_manager(&dir);
            });
        });
    }

    // Import Theme (.zip)
    {
        let settings_weak = settings_window.as_weak();
        let svc = svc.clone();
        settings_window.on_import_theme_clicked(move || {
            let dest_dir = svc.paths.themes_dir();
            let settings_weak = settings_weak.clone();
            let svc = svc.clone();
            let rt = svc.rt.clone();
            rt.spawn(async move {
                let res = tokio::task::spawn_blocking(move || {
                    let Some(src) =
                        pick_file_with_filter("Import Theme ZIP", "ZIP (*.zip) | *.zip")
                    else {
                        return Ok(false);
                    };
                    import_theme_zip(&src, &dest_dir).map(|_| true)
                })
                .await;
                if let Ok(Ok(true)) = res {
                    let themes = discover_and_load_themes(&svc.paths.themes_dir());
                    if let Ok(mut g) = svc.state.lock() {
                        g.themes = themes;
                    }
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(sw) = settings_weak.upgrade() {
                            populate_themes_list(&sw, &svc.state);
                        }
                    });
                }
            });
        });
    }

    // Refresh Themes
    {
        let settings_weak = settings_window.as_weak();
        let svc = svc.clone();
        settings_window.on_refresh_themes_clicked(move || {
            let themes = discover_and_load_themes(&svc.paths.themes_dir());
            if let Ok(mut g) = svc.state.lock() {
                g.themes = themes;
            }
            if let Some(sw) = settings_weak.upgrade() {
                populate_themes_list(&sw, &svc.state);
            }
        });
    }

    // Auto-detect Java
    {
        let settings_weak = settings_window.as_weak();
        let svc = svc.clone();
        settings_window.on_auto_detect_java(move || {
            let settings_weak = settings_weak.clone();
            let svc = svc.clone();
            if let Some(win) = settings_weak.upgrade() {
                win.set_java_busy(true);
                win.set_java_status_message(SharedString::from("Scanning Java installations..."));
            }
            svc.rt.spawn(async move {
                let custom = svc
                    .state
                    .lock()
                    .map(|g| g.config.java.custom_paths.clone())
                    .unwrap_or_default();
                let runtimes = discover_java_runtimes(&svc.paths, &custom).await;
                let count = runtimes.len();
                if let Ok(mut guard) = svc.state.lock() {
                    guard.java_runtimes = runtimes;
                }
                let state = svc.state.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(win) = settings_weak.upgrade() {
                        win.set_java_busy(false);
                        populate_java_list(&win, &state);
                        win.set_java_status_message(SharedString::from(format!(
                            "Detected {count} Java runtime(s)."
                        )));
                    }
                });
            });
        });
    }

    // Add Java Installation
    {
        let settings_weak = settings_window.as_weak();
        let svc = svc.clone();
        settings_window.on_add_java_clicked(move |path_str| {
            let p = PathBuf::from(path_str.to_string().trim());
            let settings_weak = settings_weak.clone();
            let svc = svc.clone();
            let rt = svc.rt.clone();
            rt.spawn(async move {
                match probe_java_executable(&p, false).await {
                    Ok(rt) => {
                        let cfg = if let Ok(mut guard) = svc.state.lock() {
                            let s = p.to_string_lossy().into_owned();
                            if !guard.config.java.custom_paths.contains(&s) {
                                guard.config.java.custom_paths.push(s);
                            }
                            if !guard.java_runtimes.iter().any(|r| r.path == rt.path) {
                                guard.java_runtimes.push(rt.clone());
                            }
                            guard.config.clone()
                        } else {
                            LauncherConfig::default()
                        };
                        let _ = save_config(&svc.paths, &cfg).await;
                        let state = svc.state.clone();
                        let label = rt.display_label();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(win) = settings_weak.upgrade() {
                                populate_java_list(&win, &state);
                                win.set_java_status_message(SharedString::from(format!(
                                    "Added {label}"
                                )));
                            }
                        });
                    }
                    Err(err) => {
                        let msg = err.to_string();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(win) = settings_weak.upgrade() {
                                win.set_java_status_message(SharedString::from(msg));
                            }
                        });
                    }
                }
            });
        });
    }

    // Download Java
    {
        let settings_weak = settings_window.as_weak();
        let svc = svc.clone();
        settings_window.on_download_java_clicked(move |major| {
            let major = major.max(8) as u32;
            let settings_weak = settings_weak.clone();
            let svc = svc.clone();
            if let Some(win) = settings_weak.upgrade() {
                win.set_java_busy(true);
                win.set_java_status_message(SharedString::from(format!(
                    "Downloading Eclipse Temurin Java {major}..."
                )));
            }
            svc.rt.spawn(async move {
                let cancel = CancellationToken::new();
                match install_temurin_runtime(major, &svc.paths, &svc.download_manager, &cancel)
                    .await
                {
                    Ok(rt) => {
                        let label = rt.display_label();
                        if let Ok(mut guard) = svc.state.lock() {
                            guard.java_runtimes.retain(|r| r.path != rt.path);
                            guard.java_runtimes.insert(0, rt);
                        }
                        let state = svc.state.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(win) = settings_weak.upgrade() {
                                win.set_java_busy(false);
                                populate_java_list(&win, &state);
                                win.set_java_status_message(SharedString::from(format!(
                                    "Installed {label}"
                                )));
                            }
                        });
                    }
                    Err(err) => {
                        let msg = format!("Java download failed: {err}");
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(win) = settings_weak.upgrade() {
                                win.set_java_busy(false);
                                win.set_java_status_message(SharedString::from(msg));
                            }
                        });
                    }
                }
            });
        });
    }

    // Test Java
    {
        let settings_weak = settings_window.as_weak();
        let svc = svc.clone();
        settings_window.on_test_java_clicked(move |path_str| {
            let p = PathBuf::from(path_str.to_string().trim());
            let settings_weak = settings_weak.clone();
            svc.rt.spawn(async move {
                let msg = match probe_java_executable(&p, false).await {
                    Ok(rt) => format!("Java OK: {}", rt.display_label()),
                    Err(err) => format!("Java test failed: {err}"),
                };
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(win) = settings_weak.upgrade() {
                        win.set_java_status_message(SharedString::from(msg));
                    }
                });
            });
        });
    }

    // Settings -> Browse custom folder & Reset Folder Paths
    {
        let sw = settings_window.as_weak();
        settings_window.on_browse_folder_clicked(move |key| {
            let sw_clone = sw.clone();
            let key_str = key.to_string();
            std::thread::spawn(move || {
                let picked = std::process::Command::new("zenity")
                    .args(["--file-selection", "--directory", "--title=Select Folder"])
                    .output()
                    .ok()
                    .filter(|o| o.status.success())
                    .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                    .filter(|s| !s.is_empty());
                if let Some(dir) = picked {
                    let _ = std::fs::create_dir_all(&dir);
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(w) = sw_clone.upgrade() {
                            let val = SharedString::from(dir);
                            match key_str.as_str() {
                                "instances" => w.set_folder_instances(val),
                                "mods" => w.set_folder_mods(val),
                                "icons" => w.set_folder_icons(val),
                                "runtimes" => w.set_folder_java_runtimes(val),
                                "skins" => w.set_folder_skins(val),
                                "downloads" => w.set_folder_downloads(val),
                                _ => {}
                            }
                            w.invoke_save_clicked();
                        }
                    });
                }
            });
        });
    }
    {
        let sw = settings_window.as_weak();
        settings_window.on_reset_folder_paths_clicked(move || {
            if let Some(w) = sw.upgrade() {
                w.set_folder_instances(SharedString::new());
                w.set_folder_mods(SharedString::new());
                w.set_folder_icons(SharedString::new());
                w.set_folder_java_runtimes(SharedString::new());
                w.set_folder_skins(SharedString::new());
                w.set_folder_downloads(SharedString::new());
                w.invoke_save_clicked();
            }
        });
    }

    // Save Settings
    {
        let main_weak = main_window.as_weak();
        let settings_weak = settings_window.as_weak();
        let console_weak = console_window.as_weak();
        let svc = svc.clone();
        settings_window.on_save_clicked(move || {
            let Some(win) = settings_weak.upgrade() else {
                return;
            };
            let min_mem: u32 = match win.get_min_memory_mb().trim().parse() {
                Ok(v) => v,
                Err(_) => {
                    win.set_error_message(SharedString::from("Invalid minimum RAM value."));
                    return;
                }
            };
            let max_mem: u32 = match win.get_max_memory_mb().trim().parse() {
                Ok(v) => v,
                Err(_) => {
                    win.set_error_message(SharedString::from("Invalid maximum RAM value."));
                    return;
                }
            };
            let width: u32 = match win.get_window_width().trim().parse() {
                Ok(v) => v,
                Err(_) => {
                    win.set_error_message(SharedString::from("Invalid window width."));
                    return;
                }
            };
            let height: u32 = match win.get_window_height().trim().parse() {
                Ok(v) => v,
                Err(_) => {
                    win.set_error_message(SharedString::from("Invalid window height."));
                    return;
                }
            };
            let console_font_size: u32 = match win.get_console_font_size().trim().parse() {
                Ok(v) => v,
                Err(_) => {
                    win.set_error_message(SharedString::from("Invalid console font size."));
                    return;
                }
            };
            let console_line_spacing: u32 = match win.get_console_line_spacing().trim().parse() {
                Ok(v) => v,
                Err(_) => {
                    win.set_error_message(SharedString::from("Invalid console line spacing."));
                    return;
                }
            };
            let console_max_lines: usize = match win.get_console_max_lines().trim().parse() {
                Ok(v) => v,
                Err(_) => {
                    win.set_error_message(SharedString::from("Invalid maximum visible log lines."));
                    return;
                }
            };
            let max_dl: usize = match win.get_max_concurrent_downloads().trim().parse() {
                Ok(v) => v,
                Err(_) => {
                    win.set_error_message(SharedString::from(
                        "Invalid concurrent downloads value.",
                    ));
                    return;
                }
            };

            let mut updated = svc
                .state
                .lock()
                .map(|g| g.config.clone())
                .unwrap_or_default();
            updated.language = win.get_language().to_string();
            updated.general.confirm_instance_delete = win.get_confirm_instance_delete();
            updated.general.rename_folder_policy = match win.get_rename_folder_policy() {
                0 => crab_launcher::core::config::RenameFolderPolicy::Ask,
                1 => crab_launcher::core::config::RenameFolderPolicy::Always,
                _ => crab_launcher::core::config::RenameFolderPolicy::Never,
            };
            updated.folders.instances_dir = win.get_folder_instances().to_string();
            updated.folders.mods_dir = win.get_folder_mods().to_string();
            updated.folders.icons_dir = win.get_folder_icons().to_string();
            updated.folders.java_runtimes_dir = win.get_folder_java_runtimes().to_string();
            updated.folders.skins_dir = win.get_folder_skins().to_string();
            updated.folders.downloads_dir = win.get_folder_downloads().to_string();
            if let Ok(v) = win.get_console_history_limit().trim().parse::<u32>() {
                updated.console.log_history_limit = v.clamp(100, 1_000_000);
            }
            updated.console.stop_logging_on_overflow = win.get_console_stop_on_overflow();
            if let Ok(v) = win.get_concurrent_task_limit().trim().parse::<u32>() {
                updated.tasks.concurrent_task_limit = v.clamp(1, 64);
            }
            if let Ok(v) = win.get_concurrent_download_limit().trim().parse::<u32>() {
                updated.tasks.concurrent_download_limit = v.clamp(1, 32);
            }
            if let Ok(v) = win.get_retry_limit().trim().parse::<u32>() {
                updated.tasks.retry_limit = v.min(10);
            }
            if let Ok(v) = win.get_http_timeout_secs().trim().parse::<u32>() {
                updated.tasks.http_timeout_secs = v.clamp(5, 600);
            }
            svc.download_manager.apply_task_settings(
                updated.tasks.concurrent_download_limit as usize,
                updated.tasks.retry_limit,
                updated.tasks.http_timeout_secs as u64,
            );
            updated.general.confirm_group_delete = win.get_confirm_group_delete();
            updated.general.launch_on_double_click = win.get_launch_on_double_click();
            updated.java.auto_select = win.get_java_auto_select();
            updated.java.auto_download = win.get_java_auto_download();
            updated.java.preferred_provider =
                JavaProviderKind::from_index(win.get_java_preferred_provider());
            let manual = win.get_java_manual_path().trim().to_string();
            updated.java.manual_path = if manual.is_empty() {
                None
            } else {
                Some(manual)
            };

            // Minecraft — General
            updated.minecraft.window_width = width;
            updated.minecraft.window_height = height;
            updated.minecraft.start_fullscreen = win.get_start_fullscreen();
            updated.minecraft.use_custom_resolution = win.get_use_custom_resolution();
            updated.minecraft.on_launch_behavior =
                LaunchBehavior::from_index(win.get_on_launch_behavior());
            updated.minecraft.reopen_on_exit = win.get_reopen_on_exit();
            updated.minecraft.reopen_on_crash = win.get_reopen_on_crash();
            updated.minecraft.reopen_on_kill = win.get_reopen_on_kill();
            updated.minecraft.check_java_compatibility = win.get_check_java_compatibility();
            updated.minecraft.warn_incompatible_java = win.get_warn_incompatible_java();
            updated.minecraft.verify_before_launch = win.get_verify_before_launch();
            updated.minecraft.auto_repair_missing = win.get_auto_repair_missing();
            updated.minecraft.allow_multiple_instances = win.get_allow_multiple_instances();
            updated.minecraft.confirm_before_stopping = win.get_confirm_before_stopping();

            // Minecraft — Java
            updated.minecraft.min_memory_mb = min_mem;
            updated.minecraft.max_memory_mb = max_mem;
            updated.minecraft.warn_insufficient_memory = win.get_warn_insufficient_memory();
            updated.minecraft.jvm_preset_id = win.get_jvm_preset_id().to_string();
            updated.minecraft.use_default_jvm_args = updated.minecraft.jvm_preset_id == "default";
            updated.minecraft.jvm_args = win.get_jvm_args().trim().to_string();

            // Appearance — Instance Grid
            updated.appearance.icon_size = InstanceIconSize::from_index(win.get_grid_icon_size());
            updated.appearance.grid_spacing =
                InstanceGridSpacing::from_index(win.get_grid_spacing());
            updated.appearance.show_instance_name = win.get_grid_show_name();
            updated.appearance.show_mc_version = win.get_grid_show_version();
            updated.appearance.show_modloader_badge = win.get_grid_show_modloader();
            updated.general.sort_by = InstanceSortBy::from_index(win.get_grid_sort_by());

            // Minecraft — Console
            updated.minecraft.show_console_on_launch = win.get_show_console_on_launch();
            updated.minecraft.show_console_on_crash = win.get_show_console_on_crash();
            updated.minecraft.close_console_on_exit = win.get_close_console_on_exit();
            updated.minecraft.console_autoscroll = win.get_console_autoscroll();
            updated.minecraft.console_word_wrap = win.get_console_word_wrap();
            updated.minecraft.confirm_before_clearing_console =
                win.get_confirm_before_clearing_console();
            let font_fam = win.get_console_font_family().trim().to_string();
            updated.minecraft.console_font_family = if font_fam.is_empty() {
                "JetBrains Mono".to_string()
            } else {
                font_fam
            };
            updated.minecraft.console_font_size = console_font_size;
            updated.minecraft.console_line_spacing = console_line_spacing;
            updated.minecraft.console_use_theme_font = win.get_console_use_theme_font();
            updated.minecraft.console_colorize_levels = win.get_console_colorize_levels();
            updated.minecraft.console_show_timestamps = win.get_console_show_timestamps();
            updated.minecraft.console_max_lines = console_max_lines;
            updated.minecraft.preserve_complete_logs = win.get_preserve_complete_logs();

            // Minecraft — Tweaks
            updated.minecraft.tweaks = PerformanceTweaks {
                enable_gamemode: win.get_enable_gamemode(),
                enable_mangohud: win.get_enable_mangohud(),
                use_discrete_gpu: win.get_use_discrete_gpu(),
                use_zink: win.get_use_zink(),
            };

            updated.downloads.max_concurrent = max_dl;

            if let Err(err) = updated.validate() {
                win.set_error_message(SharedString::from(err.to_string()));
                return;
            }

            win.set_error_message(SharedString::new());
            let lang = updated.language.clone();
            if let Ok(mut guard) = svc.state.lock() {
                guard.config = updated.clone();
            }
            if let Some(main) = main_weak.upgrade() {
                main.global::<Strings>().set_lang(SharedString::from(lang));
                apply_grid_appearance(&main, &updated);
                let sel = updated.last_selected_instance.clone();
                refresh_main_ui(&main, &svc.state, sel.as_deref());
            }
            if let Some(cw) = console_weak.upgrade() {
                apply_console_appearance(&cw, &updated);
            }

            let paths = svc.paths.clone();
            let main_weak = main_weak.clone();
            svc.rt.spawn(async move {
                if let Err(err) = save_config(&paths, &updated).await {
                    set_status(&main_weak, format!("Failed to save settings: {err}"));
                } else {
                    set_status(&main_weak, "Settings saved.");
                }
            });
        });
    }

    {
        let settings_weak = settings_window.as_weak();
        settings_window.on_close_clicked(move || {
            if let Some(win) = settings_weak.upgrade() {
                let _ = win.hide();
            }
        });
    }
}

fn render_qr_slint_image(content: &str) -> Option<Image> {
    let (width, modules) = generate_qr_matrix(content).ok()?;
    let quiet = 2usize;
    let total = width + quiet * 2;
    let mut buf = SharedPixelBuffer::<Rgb8Pixel>::new(total as u32, total as u32);
    let pixels = buf.make_mut_slice();

    for y in 0..total {
        for x in 0..total {
            let is_dark = if x >= quiet && y >= quiet && x < quiet + width && y < quiet + width {
                modules[(y - quiet) * width + (x - quiet)]
            } else {
                false
            };
            pixels[y * total + x] = if is_dark {
                Rgb8Pixel::new(20, 20, 20)
            } else {
                Rgb8Pixel::new(255, 255, 255)
            };
        }
    }
    Some(Image::from_rgb8(buf))
}

fn wire_microsoft_auth_callbacks(
    main_window: &MainWindow,
    settings_window: &Rc<SettingsWindow>,
    ms_auth_window: &Rc<MicrosoftAuthWindow>,
    svc: &Services,
) {
    // Add Microsoft Account (abre janela dedicada com QR Code local e Device Code Flow)
    {
        let main_weak = main_window.as_weak();
        let sw_weak = settings_window.as_weak();
        let auth_weak = ms_auth_window.as_weak();
        let svc = svc.clone();
        settings_window.on_add_microsoft_account_clicked(move || {
            let Some(auth_win) = auth_weak.upgrade() else {
                return;
            };
            auth_win.set_user_code(SharedString::new());
            auth_win.set_verification_url(SharedString::new());
            auth_win.set_has_qr(false);
            auth_win.set_copied_feedback(false);
            auth_win.set_error_message(SharedString::new());

            let Some(client_id) = distribution_client_id() else {
                auth_win.set_status_message(SharedString::new());
                auth_win.set_error_message(SharedString::from(
                    "Microsoft authentication is not available in this build.",
                ));
                let _ = auth_win.show();
                return;
            };

            let cancel = CancellationToken::new();
            if let Ok(mut guard) = svc.state.lock() {
                guard.auth_cancel = Some(cancel.clone());
            }

            if let Some(sw) = sw_weak.upgrade() {
                sw.set_accounts_error_message(SharedString::new());
            }
            auth_win.set_status_message(SharedString::from("Requesting sign-in code..."));
            let _ = auth_win.show();

            let main_weak = main_weak.clone();
            let auth_weak = auth_weak.clone();
            let sw_weak = sw_weak.clone();
            let svc = svc.clone();
            let rt = svc.rt.clone();

            rt.spawn(async move {
                let device = match start_device_code_flow(&svc.http_client, &client_id).await {
                    Ok(d) => d,
                    Err(err) => {
                        let msg = err.to_string();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(aw) = auth_weak.upgrade() {
                                aw.set_status_message(SharedString::new());
                                aw.set_error_message(SharedString::from(msg));
                            }
                        });
                        return;
                    }
                };

                let code = device.user_code.clone();
                let qr_url = device.qr_target_url().to_string();
                let open_url = qr_url.clone();
                let auth_weak_code = auth_weak.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(aw) = auth_weak_code.upgrade() {
                        aw.set_user_code(SharedString::from(code));
                        aw.set_verification_url(SharedString::from(open_url));
                        if let Some(img) = render_qr_slint_image(&qr_url) {
                            aw.set_qr_image(img);
                            aw.set_has_qr(true);
                        }
                        aw.set_status_message(SharedString::from(
                            "Waiting for authentication in your browser...",
                        ));
                    }
                });

                let token_res =
                    poll_device_code_token(&svc.http_client, &client_id, &device, &cancel).await;
                let ms_token = match token_res {
                    Ok(t) => t,
                    Err(LauncherError::Cancelled) => {
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(aw) = auth_weak.upgrade() {
                                let _ = aw.hide();
                            }
                        });
                        return;
                    }
                    Err(err) => {
                        let msg = err.to_string();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(aw) = auth_weak.upgrade() {
                                aw.set_status_message(SharedString::new());
                                aw.set_error_message(SharedString::from(msg));
                            }
                        });
                        return;
                    }
                };

                let refresh = ms_token.refresh_token.unwrap_or_default();
                match svc
                    .account_manager
                    .complete_microsoft_login(&ms_token.access_token, &refresh)
                    .await
                {
                    Ok(_) => {
                        let accounts = svc.account_manager.load_accounts().await;
                        if let Ok(mut guard) = svc.state.lock() {
                            guard.accounts = accounts;
                            guard.auth_cancel = None;
                        }
                        let state = svc.state.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(aw) = auth_weak.upgrade() {
                                let _ = aw.hide();
                            }
                            if let Some(sw) = sw_weak.upgrade() {
                                sync_settings_accounts_ui(&sw, &svc);
                            }
                            if let Some(main) = main_weak.upgrade() {
                                update_active_account_label(&main, &state);
                            }
                        });
                    }
                    Err(err) => {
                        let msg = err.to_string();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(aw) = auth_weak.upgrade() {
                                aw.set_status_message(SharedString::new());
                                aw.set_error_message(SharedString::from(msg));
                            }
                        });
                    }
                }
            });
        });
    }

    // Callbacks da janela MicrosoftAuthWindow
    {
        let auth_weak = ms_auth_window.as_weak();
        ms_auth_window.on_copy_code_clicked(move |code| {
            if let Ok(mut clipboard) = arboard::Clipboard::new() {
                let _ = clipboard.set_text(code.to_string());
            }
            if let Some(aw) = auth_weak.upgrade() {
                aw.set_copied_feedback(true);
            }
        });
    }

    {
        let svc = svc.clone();
        ms_auth_window.on_open_browser_clicked(move |url| {
            let u = url.to_string();
            svc.rt.spawn_blocking(move || {
                let _ = open::that(u);
            });
        });
    }

    {
        let auth_weak = ms_auth_window.as_weak();
        let svc = svc.clone();
        ms_auth_window.on_cancel_clicked(move || {
            if let Ok(mut guard) = svc.state.lock() {
                if let Some(c) = guard.auth_cancel.take() {
                    c.cancel();
                }
            }
            if let Some(aw) = auth_weak.upgrade() {
                let _ = aw.hide();
            }
        });
    }
}

#[allow(clippy::too_many_arguments)]
fn wire_edit_instance_callbacks(
    main_window: &MainWindow,
    edit_window: &Rc<EditInstanceWindow>,
    settings_window: &Rc<SettingsWindow>,
    install_java_window: &Rc<InstallJavaWindow>,
    install_loader_window: &Rc<InstallLoaderWindow>,
    change_group_window: &Rc<ChangeGroupWindow>,
    select_java_window: &Rc<SelectJavaWindow>,
    console_window: &Rc<ConsoleWindow>,
    manage_presets_window: &Rc<ManageJvmPresetsWindow>,
    svc: &Services,
) {
    // Callbacks das abas estendidas (Texture Packs, Shader Packs, Worlds, Servers)
    wire_edit_resources_callbacks(edit_window, svc);
    wire_edit_components_callbacks(main_window, edit_window, install_loader_window, svc);

    // Resolution Preset dropdown (instância)
    {
        let ew = edit_window.as_weak();
        let state = svc.state.clone();
        edit_window.on_resolution_preset_selected(move |preset_id| {
            let Some(win) = ew.upgrade() else { return };
            let native = state.lock().ok().and_then(|g| g.native_resolution);
            let pid = preset_id.to_string();
            if let Some((w, h)) = apply_resolution_preset(&pid, native) {
                win.set_window_width(SharedString::from(w.to_string()));
                win.set_window_height(SharedString::from(h.to_string()));
            }
            let (id, label) = resolution_preset_display(&pid, native);
            win.set_resolution_preset_id(SharedString::from(id));
            win.set_resolution_preset_label(SharedString::from(label));
        });
    }
    {
        let ew = edit_window.as_weak();
        let state = svc.state.clone();
        edit_window.on_resolution_fields_edited(move || {
            let Some(win) = ew.upgrade() else { return };
            let native = state.lock().ok().and_then(|g| g.native_resolution);
            let w: u32 = win.get_window_width().trim().parse().unwrap_or(0);
            let h: u32 = win.get_window_height().trim().parse().unwrap_or(0);
            let (id, label) = match_resolution_preset(w, h, native);
            win.set_resolution_preset_id(SharedString::from(id));
            win.set_resolution_preset_label(SharedString::from(label));
        });
    }

    // Memory Preset dropdown (instância)
    {
        let ew = edit_window.as_weak();
        edit_window.on_memory_preset_selected(move |preset_id| {
            let Some(win) = ew.upgrade() else { return };
            let pid = preset_id.to_string();
            if let Ok(mib) = pid.parse::<u32>() {
                win.set_max_memory_mb(SharedString::from(mib.to_string()));
                let min_mb: u32 = win.get_min_memory_mb().trim().parse().unwrap_or(512);
                if min_mb > mib {
                    win.set_min_memory_mb(SharedString::from(mib.min(512).to_string()));
                }
                let (id, label) = match_memory_preset(mib);
                win.set_memory_preset_id(SharedString::from(id));
                win.set_memory_preset_label(SharedString::from(label));
                win.set_memory_warning(SharedString::from(memory_warning_text(
                    mib,
                    detect_system_memory_mib(),
                    detect_available_memory_mib(),
                )));
            } else {
                win.set_memory_preset_id(SharedString::from("custom"));
                win.set_memory_preset_label(SharedString::from("Custom"));
            }
        });
    }
    {
        let ew = edit_window.as_weak();
        edit_window.on_memory_fields_edited(move || {
            let Some(win) = ew.upgrade() else { return };
            let max_mb: u32 = win.get_max_memory_mb().trim().parse().unwrap_or(0);
            let (id, label) = match_memory_preset(max_mb);
            win.set_memory_preset_id(SharedString::from(id));
            win.set_memory_preset_label(SharedString::from(label));
            win.set_memory_warning(SharedString::from(memory_warning_text(
                max_mb,
                detect_system_memory_mib(),
                detect_available_memory_mib(),
            )));
        });
    }

    // JVM Argument Preset dropdown (instância)
    {
        let ew = edit_window.as_weak();
        let state = svc.state.clone();
        edit_window.on_jvm_preset_selected(move |preset_id| {
            let Some(win) = ew.upgrade() else { return };
            let pid = preset_id.to_string();
            if pid == "custom" {
                win.set_use_default_jvm_args(false);
                win.set_jvm_preset_id(SharedString::from("custom"));
                win.set_jvm_preset_label(SharedString::from("Custom"));
                win.set_jvm_preset_description(SharedString::from(
                    "Custom user-defined JVM arguments.",
                ));
                win.set_jvm_preset_compat(SharedString::new());
                return;
            }
            let found = state
                .lock()
                .ok()
                .and_then(|g| g.jvm_presets.iter().find(|p| p.id == pid).cloned());
            if let Some(p) = found {
                win.set_use_default_jvm_args(p.id == "default");
                win.set_jvm_preset_id(SharedString::from(&p.id));
                win.set_jvm_preset_label(SharedString::from(&p.name));
                win.set_jvm_preset_description(SharedString::from(&p.description));
                win.set_jvm_preset_compat(SharedString::from(p.compatibility_summary()));
                win.set_jvm_args(SharedString::from(p.args_string()));
            }
        });
    }
    {
        let ew = edit_window.as_weak();
        edit_window.on_jvm_args_edited(move || {
            let Some(win) = ew.upgrade() else { return };
            win.set_use_default_jvm_args(false);
            win.set_jvm_preset_id(SharedString::from("custom"));
            win.set_jvm_preset_label(SharedString::from("Custom"));
        });
    }
    {
        let mp_win = Rc::clone(manage_presets_window);
        let svc = svc.clone();
        edit_window.on_manage_jvm_presets_clicked(move || {
            populate_manage_presets_window(&mp_win, &svc, None);
            let _ = mp_win.show();
        });
    }
    {
        let mp_win = Rc::clone(manage_presets_window);
        let svc = svc.clone();
        edit_window.on_save_as_jvm_preset_clicked(move |raw_args| {
            let dir = svc.paths.jvm_presets_dir();
            if let Ok(created) = create_custom_preset(
                &dir,
                "Instance Preset",
                "Saved from Instance Settings",
                &raw_args,
                8,
            ) {
                let presets = discover_jvm_presets(&dir);
                if let Ok(mut g) = svc.state.lock() {
                    g.jvm_presets = presets;
                }
                populate_manage_presets_window(&mp_win, &svc, Some(&created.id));
                let _ = mp_win.show();
            }
        });
    }
    // Detect Java (abre SelectJavaWindow mirando EditInstance)
    {
        let sj_win = Rc::clone(select_java_window);
        let svc = svc.clone();
        edit_window.on_detect_java_clicked(move || {
            if let Ok(mut g) = svc.state.lock() {
                g.select_java_target = SelectJavaTarget::EditInstance;
            }
            populate_select_java_window(&sj_win, &svc.state);
            let _ = sj_win.show();
        });
    }

    // Browse Java Executable (instância)
    {
        let edit_weak = edit_window.as_weak();
        let svc = svc.clone();
        edit_window.on_browse_java_clicked(move || {
            let edit_weak = edit_weak.clone();
            svc.rt.spawn_blocking(move || match pick_java_executable() {
                Ok(Some(path)) => {
                    let s = path.to_string_lossy().into_owned();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(w) = edit_weak.upgrade() {
                            w.set_java_path(SharedString::from(s));
                        }
                    });
                }
                Ok(None) => {}
                Err(err) => {
                    let msg = err.to_string();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(w) = edit_weak.upgrade() {
                            w.set_java_test_result(SharedString::from(msg));
                        }
                    });
                }
            });
        });
    }

    // Test JVM Arguments (instância)
    {
        let edit_weak = edit_window.as_weak();
        let svc = svc.clone();
        edit_window.on_test_jvm_args_clicked(move |java_path, min_s, max_s, raw_args| {
            let min_mb: u32 = min_s.trim().parse().unwrap_or(512);
            let max_mb: u32 = max_s.trim().parse().unwrap_or(2048);
            let raw = raw_args.to_string();
            let manual = java_path.trim().to_string();
            let target_java = if !manual.is_empty() {
                Some(PathBuf::from(manual))
            } else {
                svc.state
                    .lock()
                    .ok()
                    .and_then(|g| g.java_runtimes.first().map(|r| r.path.clone()))
            };
            let Some(exec) = target_java else {
                if let Some(w) = edit_weak.upgrade() {
                    w.set_jvm_test_result(SharedString::from(
                        "No Java runtime available to test arguments.",
                    ));
                }
                return;
            };
            if let Some(w) = edit_weak.upgrade() {
                w.set_jvm_test_result(SharedString::from("Testing JVM arguments..."));
            }
            let edit_weak = edit_weak.clone();
            svc.rt.spawn(async move {
                let msg = match test_jvm_arguments(&exec, min_mb, max_mb, &raw).await {
                    Ok(ok) => ok,
                    Err(err) => err.to_string(),
                };
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(w) = edit_weak.upgrade() {
                        w.set_jvm_test_result(SharedString::from(msg));
                    }
                });
            });
        });
    }

    // Copy JVM Arguments (instância)
    {
        let edit_weak = edit_window.as_weak();
        edit_window.on_copy_jvm_args_clicked(move |args| {
            if let Ok(mut cb) = arboard::Clipboard::new() {
                let _ = cb.set_text(args.trim().to_string());
            }
            if let Some(w) = edit_weak.upgrade() {
                w.set_jvm_test_result(SharedString::from("Copied JVM arguments to clipboard."));
            }
        });
    }

    // Open MangoHud Configuration (instância)
    {
        let edit_weak = edit_window.as_weak();
        let svc = svc.clone();
        edit_window.on_open_mangohud_config_clicked(move || {
            let edit_weak = edit_weak.clone();
            svc.rt.spawn_blocking(move || {
                if let Err(err) = open_mangohud_config() {
                    let msg = err.to_string();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(w) = edit_weak.upgrade() {
                            w.set_error_message(SharedString::from(msg));
                        }
                    });
                }
            });
        });
    }

    // Reset to Global (limpa todas as sobrescritas da instância e recarrega os valores globais efetivos)
    {
        let edit_weak = edit_window.as_weak();
        let svc = svc.clone();
        edit_window.on_reset_to_global_clicked(move || {
            let Some(w) = edit_weak.upgrade() else {
                return;
            };
            let global_mc = svc
                .state
                .lock()
                .map(|g| g.config.minecraft.clone())
                .unwrap_or_default();
            w.set_override_window(false);
            w.set_window_width(SharedString::from(global_mc.window_width.to_string()));
            w.set_window_height(SharedString::from(global_mc.window_height.to_string()));
            w.set_start_fullscreen(global_mc.start_fullscreen);
            w.set_use_custom_resolution(global_mc.use_custom_resolution);
            w.set_override_launcher_behavior(false);
            w.set_on_launch_behavior(global_mc.on_launch_behavior.to_index());
            w.set_reopen_on_exit(global_mc.reopen_on_exit);
            w.set_reopen_on_crash(global_mc.reopen_on_crash);
            w.set_reopen_on_kill(global_mc.reopen_on_kill);
            w.set_override_java(false);
            w.set_override_memory(false);
            w.set_min_memory_mb(SharedString::from(global_mc.min_memory_mb.to_string()));
            w.set_max_memory_mb(SharedString::from(global_mc.max_memory_mb.to_string()));
            w.set_override_jvm_args(false);
            w.set_use_default_jvm_args(global_mc.use_default_jvm_args);
            w.set_jvm_args(SharedString::from(&global_mc.jvm_args));
            w.set_override_console(false);
            w.set_show_console_on_launch(global_mc.show_console_on_launch);
            w.set_show_console_on_crash(global_mc.show_console_on_crash);
            w.set_close_console_on_exit(global_mc.close_console_on_exit);
            w.set_override_tweaks(false);
            w.set_enable_gamemode(global_mc.tweaks.enable_gamemode);
            w.set_enable_mangohud(global_mc.tweaks.enable_mangohud);
            w.set_use_discrete_gpu(global_mc.tweaks.use_discrete_gpu);
            w.set_use_zink(global_mc.tweaks.use_zink);
        });
    }

    // Open Global Settings
    {
        let sw = Rc::clone(settings_window);
        let svc = svc.clone();
        edit_window.on_open_global_settings_clicked(move || {
            populate_settings_window(&sw, &svc.state);
            let _ = sw.show();
        });
    }

    // Change Group
    {
        let edit_weak = edit_window.as_weak();
        let cg_win = Rc::clone(change_group_window);
        let svc = svc.clone();
        edit_window.on_change_group_clicked(move || {
            if let Some(ew) = edit_weak.upgrade() {
                if populate_change_group_window(&cg_win, &svc.state, &ew.get_instance_id()) {
                    let _ = cg_win.show();
                }
            }
        });
    }

    // Open Java Downloader
    {
        let ij_win = Rc::clone(install_java_window);
        let svc = svc.clone();
        edit_window.on_open_java_downloader_clicked(move || {
            ij_win.set_selected_provider(0);
            ij_win.set_selected_build_index(-1);
            ij_win.set_error_message(SharedString::new());
            ij_win.set_status_message(SharedString::new());
            let _ = ij_win.show();
            trigger_java_catalog_fetch(&ij_win.as_weak(), &svc, JavaProviderKind::Mojang);
        });
    }

    // Test Java Settings
    {
        let edit_weak = edit_window.as_weak();
        let svc = svc.clone();
        edit_window.on_test_java_clicked(move |path_str| {
            let p = PathBuf::from(path_str.to_string().trim());
            let edit_weak = edit_weak.clone();
            svc.rt.spawn(async move {
                let msg = match probe_java_executable(&p, false).await {
                    Ok(rt) => format!("Java OK: {}", rt.display_label()),
                    Err(err) => format!("Java test failed: {err}"),
                };
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(w) = edit_weak.upgrade() {
                        w.set_java_test_result(SharedString::from(msg));
                    }
                });
            });
        });
    }

    // Install Loader...
    {
        let edit_weak = edit_window.as_weak();
        let il_win = Rc::clone(install_loader_window);
        let svc = svc.clone();
        edit_window.on_install_loader_clicked(move || {
            let Some(ew) = edit_weak.upgrade() else {
                return;
            };
            let id = ew.get_instance_id().to_string();
            let inst_info = svc.state.lock().ok().and_then(|g| {
                g.instances
                    .iter()
                    .find(|i| i.id == id)
                    .map(|i| (i.name.clone(), i.minecraft_version.clone()))
            });
            let Some((name, mc_ver)) = inst_info else {
                return;
            };
            il_win.set_instance_id(SharedString::from(&id));
            il_win.set_instance_name(SharedString::from(name));
            il_win.set_minecraft_version(SharedString::from(&mc_ver));
            il_win.set_selected_loader(0);
            il_win.set_selected_version_index(-1);
            il_win.set_error_message(SharedString::new());
            il_win.set_status_message(SharedString::new());
            let _ = il_win.show();
            trigger_loader_catalog_fetch(&il_win.as_weak(), &svc, ModloaderKind::Fabric, mc_ver);
        });
    }

    // Remove Loader
    {
        let main_weak = main_window.as_weak();
        let edit_weak = edit_window.as_weak();
        let svc = svc.clone();
        edit_window.on_remove_loader_clicked(move || {
            let Some(ew) = edit_weak.upgrade() else {
                return;
            };
            let id = ew.get_instance_id().to_string();
            let main_weak = main_weak.clone();
            let edit_weak = edit_weak.clone();
            let svc = svc.clone();
            let rt = svc.rt.clone();
            rt.spawn(async move {
                if let Err(err) = remove_modloader(&id, &svc.paths, &svc.instance_manager).await {
                    let msg = err.to_string();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(w) = edit_weak.upgrade() {
                            w.set_error_message(SharedString::from(msg));
                        }
                    });
                    return;
                }
                if let Ok(list) = svc.instance_manager.list_instances().await {
                    if let Ok(mut g) = svc.state.lock() {
                        g.instances = list;
                    }
                }
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(w) = edit_weak.upgrade() {
                        populate_edit_instance_window(&w, &svc, &id);
                    }
                    if let Some(main) = main_weak.upgrade() {
                        refresh_main_ui(&main, &svc.state, Some(&id));
                    }
                });
            });
        });
    }

    // Open .minecraft folder
    {
        let edit_weak = edit_window.as_weak();
        let svc = svc.clone();
        edit_window.on_open_mc_folder_clicked(move || {
            let Some(ew) = edit_weak.upgrade() else {
                return;
            };
            let id = ew.get_instance_id().to_string();
            let dir = svc.state.lock().ok().and_then(|g| {
                g.instances
                    .iter()
                    .find(|i| i.id == id)
                    .map(|i| svc.paths.instance_game_dir(&i.dir_name))
            });
            if let Some(d) = dir {
                svc.rt.spawn_blocking(move || {
                    let _ = open_in_file_manager(&d);
                });
            }
        });
    }

    // Repair / Reload
    {
        let main_weak = main_window.as_weak();
        let edit_weak = edit_window.as_weak();
        let svc = svc.clone();
        edit_window.on_repair_clicked(move || {
            if let Some(ew) = edit_weak.upgrade() {
                start_instance_installation(&main_weak, &svc, ew.get_instance_id().to_string());
            }
        });
    }
    {
        let edit_weak = edit_window.as_weak();
        let svc = svc.clone();
        edit_window.on_reload_version_clicked(move || {
            if let Some(ew) = edit_weak.upgrade() {
                let id = ew.get_instance_id().to_string();
                populate_edit_instance_window(&ew, &svc, &id);
            }
        });
    }

    // Mods: Add / Toggle / Remove / Open Folder / Refresh
    {
        let edit_weak = edit_window.as_weak();
        let svc = svc.clone();
        edit_window.on_add_mod_clicked(move |src_path| {
            let Some(ew) = edit_weak.upgrade() else {
                return;
            };
            let id = ew.get_instance_id().to_string();
            let mods_dir = svc.state.lock().ok().and_then(|g| {
                g.instances
                    .iter()
                    .find(|i| i.id == id)
                    .map(|i| svc.paths.instance_game_dir(&i.dir_name).join("mods"))
            });
            let Some(mods_dir) = mods_dir else {
                return;
            };
            let src = PathBuf::from(src_path.to_string().trim());
            let edit_weak = edit_weak.clone();
            let svc = svc.clone();
            let rt = svc.rt.clone();
            rt.spawn(async move {
                let res = import_local_mod(&mods_dir, &src).await;
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(w) = edit_weak.upgrade() {
                        match res {
                            Ok(_) => populate_edit_instance_window(&w, &svc, &id),
                            Err(err) => {
                                w.set_error_message(SharedString::from(err.to_string()));
                                false
                            }
                        };
                    }
                });
            });
        });
    }
    {
        let edit_weak = edit_window.as_weak();
        let svc = svc.clone();
        edit_window.on_toggle_mod_clicked(move |file_name| {
            let Some(ew) = edit_weak.upgrade() else {
                return;
            };
            let id = ew.get_instance_id().to_string();
            let mods_dir = svc.state.lock().ok().and_then(|g| {
                g.instances
                    .iter()
                    .find(|i| i.id == id)
                    .map(|i| svc.paths.instance_game_dir(&i.dir_name).join("mods"))
            });
            let Some(mods_dir) = mods_dir else {
                return;
            };
            let fname = file_name.to_string();
            let edit_weak = edit_weak.clone();
            let svc = svc.clone();
            let rt = svc.rt.clone();
            rt.spawn(async move {
                let _ = toggle_local_mod(&mods_dir, &fname).await;
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(w) = edit_weak.upgrade() {
                        populate_edit_instance_window(&w, &svc, &id);
                    }
                });
            });
        });
    }
    {
        let edit_weak = edit_window.as_weak();
        let svc = svc.clone();
        edit_window.on_remove_mod_clicked(move |file_name| {
            let Some(ew) = edit_weak.upgrade() else {
                return;
            };
            let id = ew.get_instance_id().to_string();
            let mods_dir = svc.state.lock().ok().and_then(|g| {
                g.instances
                    .iter()
                    .find(|i| i.id == id)
                    .map(|i| svc.paths.instance_game_dir(&i.dir_name).join("mods"))
            });
            let Some(mods_dir) = mods_dir else {
                return;
            };
            let fname = file_name.to_string();
            let edit_weak = edit_weak.clone();
            let svc = svc.clone();
            let rt = svc.rt.clone();
            rt.spawn(async move {
                let _ = remove_local_mod(&mods_dir, &fname).await;
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(w) = edit_weak.upgrade() {
                        w.set_selected_mod_file(SharedString::new());
                        populate_edit_instance_window(&w, &svc, &id);
                    }
                });
            });
        });
    }
    {
        let edit_weak = edit_window.as_weak();
        let svc = svc.clone();
        edit_window.on_open_mods_folder_clicked(move || {
            let Some(ew) = edit_weak.upgrade() else {
                return;
            };
            let id = ew.get_instance_id().to_string();
            let dir = svc.state.lock().ok().and_then(|g| {
                g.instances
                    .iter()
                    .find(|i| i.id == id)
                    .map(|i| svc.paths.instance_game_dir(&i.dir_name).join("mods"))
            });
            if let Some(d) = dir {
                svc.rt.spawn_blocking(move || {
                    let _ = open_in_file_manager(&d);
                });
            }
        });
    }
    {
        let edit_weak = edit_window.as_weak();
        let svc = svc.clone();
        edit_window.on_refresh_mods_clicked(move || {
            if let Some(ew) = edit_weak.upgrade() {
                let id = ew.get_instance_id().to_string();
                populate_edit_instance_window(&ew, &svc, &id);
            }
        });
    }

    // Launch / Stop na barra inferior
    {
        let main_weak = main_window.as_weak();
        let edit_weak = edit_window.as_weak();
        let console_weak = console_window.as_weak();
        let svc = svc.clone();
        edit_window.on_launch_clicked(move || {
            if let Some(ew) = edit_weak.upgrade() {
                start_launch_flow(
                    &main_weak,
                    &console_weak,
                    &svc,
                    ew.get_instance_id().to_string(),
                );
            }
        });
    }
    {
        let edit_weak = edit_window.as_weak();
        let svc = svc.clone();
        edit_window.on_stop_clicked(move || {
            if let Some(ew) = edit_weak.upgrade() {
                svc.process_manager.stop_instance(&ew.get_instance_id());
            }
        });
    }
    {
        let edit_weak = edit_window.as_weak();
        edit_window.on_cancel_clicked(move || {
            if let Some(win) = edit_weak.upgrade() {
                let _ = win.hide();
            }
        });
    }

    {
        let main_weak = main_window.as_weak();
        let edit_weak = edit_window.as_weak();
        let svc = svc.clone();
        edit_window.on_save_clicked(move || {
            let Some(win) = edit_weak.upgrade() else {
                return;
            };
            let id = win.get_instance_id().to_string();
            let name = win.get_instance_name().to_string();
            if let Err(err) = validate_instance_name(&name) {
                win.set_error_message(SharedString::from(err.to_string()));
                return;
            }

            let group_id = {
                let g = win.get_selected_group_id().to_string();
                if g.is_empty() {
                    None
                } else {
                    Some(g)
                }
            };

            let min_mem: Option<u32> = win.get_min_memory_mb().trim().parse().ok();
            let max_mem: Option<u32> = win.get_max_memory_mb().trim().parse().ok();
            if win.get_override_memory() {
                let (Some(mn), Some(mx)) = (min_mem, max_mem) else {
                    win.set_error_message(SharedString::from("Invalid memory values."));
                    return;
                };
                if mn < 128 || mx < mn {
                    win.set_error_message(SharedString::from(
                        "Minimum RAM must be at least 128 MiB and not exceed maximum RAM.",
                    ));
                    return;
                }
            }

            let win_w: Option<u32> = win.get_window_width().trim().parse().ok();
            let win_h: Option<u32> = win.get_window_height().trim().parse().ok();
            if win.get_override_window() && win.get_use_custom_resolution() {
                let (Some(w), Some(h)) = (win_w, win_h) else {
                    win.set_error_message(SharedString::from("Invalid window dimensions."));
                    return;
                };
                if w < 320 || h < 240 {
                    win.set_error_message(SharedString::from(
                        "Window resolution must be at least 320x240.",
                    ));
                    return;
                }
            }

            let overrides = InstanceOverrides {
                override_java: win.get_override_java(),
                java_path: {
                    let s = win.get_java_path().trim().to_string();
                    if s.is_empty() {
                        None
                    } else {
                        Some(s)
                    }
                },
                override_memory: win.get_override_memory(),
                min_memory_mb: min_mem,
                max_memory_mb: max_mem,
                override_jvm_args: win.get_override_jvm_args(),
                use_default_jvm_args: Some(win.get_jvm_preset_id().as_str() == "default"),
                jvm_preset_id: Some(win.get_jvm_preset_id().to_string()),
                jvm_args: Some(win.get_jvm_args().trim().to_string()),
                override_window: win.get_override_window(),
                window_width: win_w,
                window_height: win_h,
                start_fullscreen: Some(win.get_start_fullscreen()),
                use_custom_resolution: Some(win.get_use_custom_resolution()),
                override_launcher_behavior: win.get_override_launcher_behavior(),
                on_launch_behavior: Some(LaunchBehavior::from_index(win.get_on_launch_behavior())),
                reopen_on_exit: Some(win.get_reopen_on_exit()),
                reopen_on_crash: Some(win.get_reopen_on_crash()),
                reopen_on_kill: Some(win.get_reopen_on_kill()),
                override_console: win.get_override_console(),
                show_console_on_launch: Some(win.get_show_console_on_launch()),
                show_console_on_crash: Some(win.get_show_console_on_crash()),
                close_console_on_exit: Some(win.get_close_console_on_exit()),
                override_tweaks: win.get_override_tweaks(),
                tweaks: Some(PerformanceTweaks {
                    enable_gamemode: win.get_enable_gamemode(),
                    enable_mangohud: win.get_enable_mangohud(),
                    use_discrete_gpu: win.get_use_discrete_gpu(),
                    use_zink: win.get_use_zink(),
                }),
            };

            win.set_error_message(SharedString::new());
            let main_weak = main_weak.clone();
            let edit_weak = edit_weak.clone();
            let svc = svc.clone();

            svc.rt.spawn(async move {
                match svc
                    .instance_manager
                    .update_instance_settings(&id, &name, group_id, overrides)
                    .await
                {
                    Ok(updated) => {
                        if let Ok(mut guard) = svc.state.lock() {
                            if let Some(item) = guard.instances.iter_mut().find(|i| i.id == id) {
                                *item = updated.clone();
                            }
                        }
                        let state = svc.state.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ew) = edit_weak.upgrade() {
                                ew.set_error_message(SharedString::new());
                            }
                            if let Some(main) = main_weak.upgrade() {
                                refresh_main_ui(&main, &state, Some(&updated.id));
                                main.set_status_text(SharedString::from(format!(
                                    "Saved settings for '{}'",
                                    updated.name
                                )));
                            }
                        });
                    }
                    Err(err) => {
                        let msg = err.to_string();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ew) = edit_weak.upgrade() {
                                ew.set_error_message(SharedString::from(msg));
                            }
                        });
                    }
                }
            });
        });
    }
}

fn wire_console_callbacks(
    main_window: &MainWindow,
    console_window: &Rc<ConsoleWindow>,
    svc: &Services,
) {
    {
        let cw = console_window.as_weak();
        let state = svc.state.clone();
        console_window.on_search_changed(move |_| {
            if let Some(win) = cw.upgrade() {
                let id = state
                    .lock()
                    .ok()
                    .and_then(|g| g.focused_console_id.clone())
                    .unwrap_or_default();
                refresh_console_window(&win, &state, &id);
            }
        });
    }

    {
        let state = svc.state.clone();
        console_window.on_copy_log_clicked(move || {
            let text = state
                .lock()
                .ok()
                .and_then(|g| {
                    let id = g.focused_console_id.as_ref()?;
                    g.console_buffers.get(id).map(|b| b.to_plain_text())
                })
                .unwrap_or_default();
            if let Ok(mut clipboard) = arboard::Clipboard::new() {
                let _ = clipboard.set_text(text);
            }
        });
    }

    {
        let cw = console_window.as_weak();
        let state = svc.state.clone();
        console_window.on_clear_clicked(move || {
            let id = if let Ok(mut g) = state.lock() {
                let id = g.focused_console_id.clone().unwrap_or_default();
                if let Some(buf) = g.console_buffers.get_mut(&id) {
                    buf.clear();
                }
                id
            } else {
                String::new()
            };
            if let Some(win) = cw.upgrade() {
                refresh_console_window(&win, &state, &id);
            }
        });
    }

    {
        let svc = svc.clone();
        console_window.on_open_log_folder_clicked(move || {
            let id = svc
                .state
                .lock()
                .ok()
                .and_then(|g| g.focused_console_id.clone());
            if let Some(id) = id {
                let im = svc.instance_manager.clone();
                let paths = svc.paths.clone();
                svc.rt.spawn(async move {
                    if let Ok(inst) = im.find_by_id(&id).await {
                        let logs_dir = paths.instance_game_dir(&inst.dir_name).join("logs");
                        let _ = tokio::task::spawn_blocking(move || {
                            let _ = open_in_file_manager(&logs_dir);
                        })
                        .await;
                    }
                });
            }
        });
    }

    {
        let main_weak = main_window.as_weak();
        let svc = svc.clone();
        console_window.on_stop_game_clicked(move || {
            let (id_opt, confirm, name) = svc
                .state
                .lock()
                .ok()
                .map(|g| {
                    let id = g.focused_console_id.clone();
                    let nm = id
                        .as_deref()
                        .and_then(|iid| g.instances.iter().find(|i| i.id == iid))
                        .map(|i| i.name.clone())
                        .unwrap_or_else(|| "Minecraft".to_string());
                    (id, g.config.minecraft.confirm_before_stopping, nm)
                })
                .unwrap_or((None, false, "Minecraft".to_string()));
            if let Some(id) = id_opt {
                if confirm {
                    if let Some(main) = main_weak.upgrade() {
                        main.set_stop_confirm_instance_id(SharedString::from(id));
                        main.set_stop_confirm_instance_name(SharedString::from(name));
                        main.set_show_stop_confirm_dialog(true);
                    }
                } else {
                    svc.process_manager.stop_instance(&id);
                }
            }
        });
    }
}

fn wire_launch_summary_callbacks(summary_window: &Rc<LaunchSummaryWindow>, svc: &Services) {
    {
        let sw = summary_window.as_weak();
        let state = svc.state.clone();
        summary_window.on_copy_clicked(move || {
            let text = state
                .lock()
                .map(|g| g.last_launch_summary_text.clone())
                .unwrap_or_default();
            if let Ok(mut cb) = arboard::Clipboard::new() {
                let _ = cb.set_text(text);
            }
            if let Some(win) = sw.upgrade() {
                win.set_copy_feedback(SharedString::from(
                    "Launch configuration copied to clipboard.",
                ));
            }
        });
    }
    {
        let sw = summary_window.as_weak();
        summary_window.on_close_clicked(move || {
            if let Some(win) = sw.upgrade() {
                let _ = win.hide();
            }
        });
    }
}

fn start_launch_flow(
    main_weak: &Weak<MainWindow>,
    console_weak: &Weak<ConsoleWindow>,
    svc: &Services,
    instance_id: String,
) {
    // Nunca permite duas execuções simultâneas da mesma instância.
    if svc.process_manager.is_running(&instance_id) {
        return;
    }

    let allow_multiple = svc
        .state
        .lock()
        .map(|g| g.config.minecraft.allow_multiple_instances)
        .unwrap_or(true);
    if !allow_multiple && svc.process_manager.any_running() {
        set_status(
            main_weak,
            "Another instance is already running. Enable 'Allow multiple different instances' in Settings to run more than one.",
        );
        return;
    }

    let main_weak = main_weak.clone();
    let console_weak = console_weak.clone();
    let svc = svc.clone();
    let rt = svc.rt.clone();

    rt.spawn(async move {
        let launch_cancel = CancellationToken::new();
        if let Ok(mut g) = svc.state.lock() {
            g.active_install_cancel = Some(launch_cancel.clone());
        }
        let fail_launch = |main_weak: &Weak<MainWindow>, svc: &Services, msg: String| {
            if let Ok(mut g) = svc.state.lock() {
                g.active_install_cancel = None;
            }
            set_status(main_weak, format!("Launch failed: {msg}"));
            let _ = slint::invoke_from_event_loop(move || {
                with_task_progress(|tp| {
                    tp.set_task_state(SharedString::from("Failed"));
                    tp.set_indeterminate(false);
                    tp.set_is_cancelling(false);
                    tp.set_is_finished(true);
                    tp.set_error_message(SharedString::from(msg));
                });
            });
        };
        let update_launch_step = |step: u32, stage: String, overall: f32, indeterminate: bool| {
            let _ = slint::invoke_from_event_loop(move || {
                with_task_progress(|tp| {
                    tp.set_step_text(SharedString::from(format!("Executing task {step} of 4")));
                    tp.set_task_stage(SharedString::from(stage));
                    tp.set_overall_progress(overall);
                    tp.set_overall_percent_text(SharedString::from(format!(
                        "{:.0}%",
                        overall * 100.0
                    )));
                    tp.set_indeterminate(indeterminate);
                });
            });
        };

        set_status(&main_weak, "Checking installation...");
        let instance = match svc.instance_manager.find_by_id(&instance_id).await {
            Ok(i) => i,
            Err(err) => {
                fail_launch(&main_weak, &svc, err.to_string());
                return;
            }
        };

        let inst_title = format!("Launching {}", instance.name);
        let _ = slint::invoke_from_event_loop(move || {
            with_task_progress(|tp| {
                tp.set_task_title(SharedString::from(inst_title));
                tp.set_task_kind(SharedString::from("launch"));
                tp.set_step_text(SharedString::from("Executing task 1 of 4"));
                tp.set_task_stage(SharedString::from("Verifying Minecraft files & assets..."));
                tp.set_task_state(SharedString::from("Running"));
                tp.set_overall_progress(0.10);
                tp.set_overall_percent_text(SharedString::from("10%"));
                tp.set_progress(0.0);
                tp.set_percent_text(SharedString::from("0%"));
                tp.set_bytes_text(SharedString::new());
                tp.set_speed_text(SharedString::new());
                tp.set_files_text(SharedString::new());
                tp.set_error_message(SharedString::new());
                tp.set_indeterminate(true);
                tp.set_is_paused(false);
                tp.set_is_cancelling(false);
                tp.set_is_finished(false);
                let _ = tp.show();
            });
        });

        let (config, mut runtimes, caps) = svc
            .state
            .lock()
            .map(|g| {
                (
                    g.config.clone(),
                    g.java_runtimes.clone(),
                    g.capabilities.clone(),
                )
            })
            .unwrap_or_else(|_| {
                (
                    LauncherConfig::default(),
                    Vec::new(),
                    detect_system_capabilities(),
                )
            });

        let effective = EffectiveInstanceSettings::resolve(&config, &instance.overrides);

        // 1. Garante que a instalação está presente com progresso real na TaskProgressWindow
        if instance.installation.status != InstallationStatus::Installed {
            if let Err(err) = svc
                .installer
                .install_instance(&instance_id, launch_cancel.clone(), move |p: InstallProgress| {
                    let _ = slint::invoke_from_event_loop(move || {
                        let ratio = if !p.indeterminate && p.total_files > 0 {
                            if p.total_bytes > 0 {
                                (p.downloaded_bytes as f32 / p.total_bytes as f32).clamp(0.0, 1.0)
                            } else {
                                (p.completed_files as f32 / p.total_files as f32).clamp(0.0, 1.0)
                            }
                        } else {
                            0.0
                        };
                        let overall = 0.10 + ratio * 0.40;
                        with_task_progress(|tp| {
                            tp.set_task_stage(SharedString::from(&p.message));
                            tp.set_indeterminate(p.indeterminate);
                            tp.set_overall_progress(overall);
                            tp.set_overall_percent_text(SharedString::from(format!(
                                "{:.0}%",
                                overall * 100.0
                            )));
                            if !p.indeterminate && p.total_files > 0 {
                                tp.set_progress(ratio);
                                tp.set_percent_text(SharedString::from(format!(
                                    "{:.0}%",
                                    ratio * 100.0
                                )));
                                tp.set_files_text(SharedString::from(format!(
                                    "{}/{} files",
                                    p.completed_files, p.total_files
                                )));
                                if p.total_bytes > 0 {
                                    tp.set_bytes_text(SharedString::from(format!(
                                        "{} / {}",
                                        format_bytes(p.downloaded_bytes),
                                        format_bytes(p.total_bytes)
                                    )));
                                    tp.set_speed_text(SharedString::from(format!(
                                        "{}/s",
                                        format_bytes(p.bytes_per_sec)
                                    )));
                                }
                            }
                        });
                    });
                })
                .await
            {
                fail_launch(&main_weak, &svc, err.to_string());
                return;
            }
        }
        if launch_cancel.is_cancelled() {
            fail_launch(&main_weak, &svc, "Launch cancelled.".to_string());
            return;
        }
        update_launch_step(
            2,
            "Checking libraries, assets & Java runtime...".to_string(),
            0.55,
            true,
        );

        // 2. Carrega metadados da versão e compõe com modloader (Fabric/Forge/NeoForge) se instalado
        let version_json = svc
            .paths
            .version_metadata_file(&instance.minecraft_version);
        let base_meta: VersionMetadata = match read_json(&version_json).await {
            Ok(v) => v,
            Err(err) => {
                set_status(&main_weak, format!("Launch failed: {err}"));
                return;
            }
        };
        let loader_profile_path = svc
            .paths
            .instance_dir(&instance.dir_name)
            .join("loader-profile.json");
        let loader_profile: Option<LoaderProfile> = if loader_profile_path.exists() {
            read_json(&loader_profile_path).await.ok()
        } else {
            None
        };
        let version_meta = compose_version_metadata(&base_meta, loader_profile.as_ref());

        let platform = TargetPlatform::current();
        if effective.verify_before_launch {
            let missing = verify_required_files_incremental(&version_meta, &svc.paths, &platform);
            if !missing.is_empty() {
                if effective.auto_repair_missing {
                    set_status(
                        &main_weak,
                        format!("Repairing {} missing file(s)...", missing.len()),
                    );
                    let cancel = CancellationToken::new();
                    if let Err(err) = svc
                        .installer
                        .install_instance(&instance_id, cancel, |_| {})
                        .await
                    {
                        set_status(&main_weak, format!("Auto-repair failed: {err}"));
                        return;
                    }
                } else {
                    set_status(
                        &main_weak,
                        format!(
                            "Launch blocked: {} required game file(s) missing. Run Repair or enable Auto-Repair.",
                            missing.len()
                        ),
                    );
                    return;
                }
            }
        }

        // 3. Resolve runtime Java compatível e valida compatibilidade / arquitetura
        set_status(&main_weak, "Resolving Java runtime...");
        let required_major = required_java_major(&version_meta);
        let mut prelaunch_warnings: Vec<String> = Vec::new();

        let java_exec = if let Some(manual) = &effective.manual_java_path {
            PathBuf::from(manual)
        } else {
            if runtimes.is_empty() {
                runtimes = discover_java_runtimes(&svc.paths, &config.java.custom_paths).await;
            }
            if let Some(found) = select_compatible_runtime(&runtimes, required_major) {
                found.path.clone()
            } else if config.java.auto_download {
                set_status(&main_weak, format!("Downloading Java {required_major}..."));
                let cancel = CancellationToken::new();
                match install_temurin_runtime(
                    required_major,
                    &svc.paths,
                    &svc.download_manager,
                    &cancel,
                )
                .await
                {
                    Ok(rt) => rt.path,
                    Err(err) => {
                        set_status(&main_weak, format!("Launch failed: {err}"));
                        return;
                    }
                }
            } else {
                set_status(
                    &main_weak,
                    format!(
                        "Launch failed: Java {required_major} required. Configure or download Java in Settings."
                    ),
                );
                return;
            }
        };

        if effective.check_java_compatibility {
            if let Ok(probed) = probe_java_executable(&java_exec, false).await {
                if let Err(mem_err) = validate_memory_for_java_arch(
                    effective.min_memory_mb,
                    effective.max_memory_mb,
                    &probed.arch,
                ) {
                    set_status(&main_weak, format!("Launch failed: {mem_err}"));
                    return;
                }
                let compatible = probed.major_version == required_major
                    || (required_major >= 17 && probed.major_version > required_major);
                if !compatible && effective.warn_incompatible_java {
                    prelaunch_warnings.push(format!(
                        "Java compatibility warning: Minecraft {} expects Java {}, but '{}' is Java {}.",
                        instance.minecraft_version,
                        required_major,
                        probed.path.display(),
                        probed.major_version
                    ));
                }
            }
        }

        if config.minecraft.warn_insufficient_memory {
            if let Some(avail) = detect_available_memory_mib() {
                if effective.max_memory_mb as u64 > avail {
                    prelaunch_warnings.push(format!(
                        "Memory warning: Configured maximum RAM ({} MiB) exceeds currently available system memory ({} MiB).",
                        effective.max_memory_mb, avail
                    ));
                }
            }
        }

        // 4. Autenticação (Microsoft ou Offline)
        set_status(&main_weak, "Authenticating...");
        update_launch_step(3, "Authenticating & building launch plan...".to_string(), 0.72, true);
        let client_id = distribution_client_id();
        let creds = match svc
            .account_manager
            .resolve_launch_credentials(client_id.as_deref())
            .await
        {
            Ok(c) => c,
            Err(err) => {
                fail_launch(&main_weak, &svc, err.to_string());
                return;
            }
        };

        // 5. Constrói LaunchPlan e LaunchCommand centralizado (com wrappers Linux e env)
        set_status(&main_weak, "Preparing launch...");
        let mut plan = match build_launch_plan(
            &instance,
            &version_meta,
            java_exec,
            &creds,
            &effective,
            &svc.paths,
            &platform,
        ) {
            Ok(p) => p,
            Err(err) => {
                fail_launch(&main_weak, &svc, err.to_string());
                return;
            }
        };
        plan.warnings.extend(prelaunch_warnings);

        if plan.apply_legacy_fullscreen_in_options {
            if let Err(err) = ensure_legacy_fullscreen_option(&plan.game_dir) {
                plan.warnings.push(format!(
                    "Could not set legacy fullscreen in options.txt: {err}"
                ));
            }
        }

        let resolved_tweaks =
            resolve_performance_tweaks(&effective.tweaks, &caps, plan.is_legacy_lwjgl);
        let command = build_launch_command(&plan, &resolved_tweaks);

        // 6. Modo Close Launcher: estabelece supervisor headless ANTES de encerrar a interface gráfica
        if effective.on_launch_behavior == LaunchBehavior::Close {
            let launcher_exe = match std::env::current_exe() {
                Ok(p) => p,
                Err(err) => {
                    set_status(
                        &main_weak,
                        format!("Launch failed: could not locate Crab executable for supervisor: {err}"),
                    );
                    return;
                }
            };

            let spec = SupervisorLaunchSpec {
                instance_id: instance_id.clone(),
                instance_name: instance.name.clone(),
                command,
                log_file_path: plan.game_dir.join("logs").join("crab-latest.log"),
                sensitive_values: plan.sensitive_values.clone(),
                reopen_on_exit: effective.reopen_on_exit,
                reopen_on_crash: effective.reopen_on_crash,
                reopen_on_kill: effective.reopen_on_kill,
                launcher_executable: launcher_exe,
                reopen_lock_dir: svc.paths.cache_dir.join("supervisor"),
                instance_metadata_path: Some(
                    svc.paths.instance_metadata_file(&instance.dir_name),
                ),
            };

            set_status(&main_weak, "Starting supervised Minecraft process...");
            match spawn_supervised_launch(&spec, &svc.paths.cache_dir).await {
                Ok(_pid) => {
                    // Supervisor confirmou que o Minecraft iniciou com sucesso; encerra a GUI.
                    let _ = slint::invoke_from_event_loop(|| {
                        let _ = slint::quit_event_loop();
                    });
                }
                Err(err) => {
                    // Se o lançamento falhar, mantém a interface gráfica aberta com o erro.
                    set_status(&main_weak, format!("Launch failed: {err}"));
                }
            }
            return;
        }

        // 7. Modos KeepOpen / Minimize / Hide: inicia processo gerenciado pelo ProcessManager
        set_status(&main_weak, "Starting Minecraft...");
        update_launch_step(4, "Starting Minecraft process...".to_string(), 0.92, true);
        let _ = svc.instance_manager.record_last_played(&instance_id).await;
        if let Ok(mut g) = svc.state.lock() {
            g.active_install_cancel = None;
        }
        let max_lines = effective.console_max_lines.max(100);
        if let Ok(mut guard) = svc.state.lock() {
            guard.running_instances.insert(instance_id.clone());
            guard
                .console_buffers
                .insert(instance_id.clone(), ConsoleBuffer::new(max_lines));
            guard.focused_console_id = Some(instance_id.clone());
        }

        let show_console = effective.show_console_on_launch;
        let launch_behavior = effective.on_launch_behavior;
        let main_weak_start = main_weak.clone();
        let console_weak_start = console_weak.clone();
        let state_start = svc.state.clone();
        let id_start = instance_id.clone();

        let _ = slint::invoke_from_event_loop(move || {
            with_task_progress(|tp| {
                tp.set_overall_progress(1.0);
                tp.set_overall_percent_text(SharedString::from("100%"));
                tp.set_is_finished(true);
                let _ = tp.hide();
            });
            if let Some(main) = main_weak_start.upgrade() {
                refresh_main_ui(&main, &state_start, Some(&id_start));
                main.set_status_text(SharedString::from("Running."));
                match launch_behavior {
                    LaunchBehavior::KeepOpen => {}
                    LaunchBehavior::Minimize => main.window().set_minimized(true),
                    LaunchBehavior::Hide => {
                        let _ = main.hide();
                    }
                    LaunchBehavior::Close => {}
                }
            }
            if show_console {
                if let Some(cw) = console_weak_start.upgrade() {
                    refresh_console_window(&cw, &state_start, &id_start);
                    let _ = cw.show();
                }
            }
        });

        let main_weak_ev = main_weak.clone();
        let console_weak_ev = console_weak.clone();
        let state_ev = svc.state.clone();
        let im_ev = svc.instance_manager.clone();
        let rt_ev = svc.rt.clone();
        let reopen_exit = effective.reopen_on_exit;
        let reopen_crash = effective.reopen_on_crash;
        let reopen_kill = effective.reopen_on_kill;
        let show_console_crash = effective.show_console_on_crash;
        let close_console_exit = effective.close_console_on_exit;

        if let Ok(mut guard) = svc.state.lock() {
            guard
                .session_starts
                .insert(instance_id.clone(), std::time::Instant::now());
        }

        if let Err(err) = svc
            .process_manager
            .spawn_command(plan, command, effective.preserve_complete_logs, move |event| {
                match event {
                    ProcessEvent::Log {
                        instance_id,
                        level,
                        line,
                    } => {
                        if let Ok(mut guard) = state_ev.lock() {
                            let buf = guard
                                .console_buffers
                                .entry(instance_id.clone())
                                .or_insert_with(|| ConsoleBuffer::new(max_lines));
                            buf.push(level, line);
                        }
                        let cw = console_weak_ev.clone();
                        let st = state_ev.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(win) = cw.upgrade() {
                                refresh_console_window(&win, &st, &instance_id);
                            }
                        });
                    }
                    ProcessEvent::Exited {
                        instance_id,
                        exit_code: _,
                        exit_kind,
                    } => {
                        let msg = exit_kind.status_message();
                        let log_lvl = if exit_kind.is_crash() {
                            LogLevel::Error
                        } else {
                            LogLevel::Info
                        };
                        let elapsed_secs = if let Ok(mut guard) = state_ev.lock() {
                            guard.running_instances.remove(&instance_id);
                            if let Some(buf) = guard.console_buffers.get_mut(&instance_id) {
                                buf.push(log_lvl, msg.clone());
                            }
                            guard
                                .session_starts
                                .remove(&instance_id)
                                .map(|s| s.elapsed().as_secs())
                        } else {
                            None
                        };

                        // Registra tempo real de jogo somente para sessões que iniciaram com sucesso
                        if let Some(secs) = elapsed_secs {
                            let im_play = im_ev.clone();
                            let st_play = state_ev.clone();
                            let id_play = instance_id.clone();
                            let mw_play = main_weak_ev.clone();
                            rt_ev.spawn(async move {
                                if let Ok(updated) = im_play.record_playtime_session(&id_play, secs).await {
                                    if let Ok(mut g) = st_play.lock() {
                                        if let Some(item) = g.instances.iter_mut().find(|i| i.id == id_play) {
                                            *item = updated;
                                        }
                                    }
                                    let _ = slint::invoke_from_event_loop(move || {
                                        if let Some(main) = mw_play.upgrade() {
                                            refresh_main_ui(&main, &st_play, Some(&id_play));
                                        }
                                    });
                                }
                            });
                        }
                        let should_restore =
                            exit_kind.should_reopen(reopen_exit, reopen_crash, reopen_kill);
                        let mw = main_weak_ev.clone();
                        let cw = console_weak_ev.clone();
                        let st = state_ev.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(main) = mw.upgrade() {
                                refresh_main_ui(&main, &st, Some(&instance_id));
                                main.set_status_text(SharedString::from(&msg));
                                if should_restore {
                                    match launch_behavior {
                                        LaunchBehavior::Minimize => {
                                            main.window().set_minimized(false);
                                        }
                                        LaunchBehavior::Hide => {
                                            let _ = main.show();
                                        }
                                        _ => {}
                                    }
                                } else if launch_behavior == LaunchBehavior::Hide {
                                    // Se o launcher estava oculto e não deve reabrir, encerra o loop
                                    // caso nenhuma outra instância esteja em execução.
                                    let any_left = st
                                        .lock()
                                        .map(|g| !g.running_instances.is_empty())
                                        .unwrap_or(false);
                                    if !any_left {
                                        let _ = slint::quit_event_loop();
                                    }
                                }
                            }
                            if let Some(win) = cw.upgrade() {
                                refresh_console_window(&win, &st, &instance_id);
                                if exit_kind.is_crash() && show_console_crash {
                                    let _ = win.show();
                                } else if exit_kind == ProcessExitKind::Normal && close_console_exit
                                {
                                    let _ = win.hide();
                                }
                            }
                        });
                    }
                }
            })
            .await
        {
            if let Ok(mut guard) = svc.state.lock() {
                guard.running_instances.remove(&instance_id);
                guard.session_starts.remove(&instance_id);
            }
            // Falha ao iniciar: restaura janela principal se estava minimizada/oculta
            let mw = main_weak.clone();
            let err_text = format!("Launch failed: {err}");
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(main) = mw.upgrade() {
                    main.window().set_minimized(false);
                    let _ = main.show();
                    main.set_status_text(SharedString::from(err_text));
                }
            });
        }
    });
}

fn start_instance_installation(main_weak: &Weak<MainWindow>, svc: &Services, instance_id: String) {
    start_instance_installation_with_loader(main_weak, svc, instance_id, None);
}

fn start_instance_installation_with_loader(
    main_weak: &Weak<MainWindow>,
    svc: &Services,
    instance_id: String,
    modloader: Option<(ModloaderKind, String)>,
) {
    let cancel = CancellationToken::new();

    if let Ok(mut guard) = svc.state.lock() {
        if guard.active_installs.contains_key(&instance_id) {
            return;
        }
        guard
            .active_installs
            .insert(instance_id.clone(), cancel.clone());
        guard.focused_install_id = Some(instance_id.clone());
        if let Some(inst) = guard.instances.iter_mut().find(|i| i.id == instance_id) {
            inst.set_installation_status(InstallationStatus::Installing, None);
        }
        let selected_id = guard.config.last_selected_instance.clone();
        let main_weak_ui = main_weak.clone();
        let state = svc.state.clone();
        drop(guard);
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(win) = main_weak_ui.upgrade() {
                refresh_main_ui(&win, &state, selected_id.as_deref());
                win.set_install_active(true);
                win.set_install_indeterminate(true);
                win.set_install_operation_text(SharedString::from("Resolving version metadata..."));
                win.set_install_files_text(SharedString::new());
                win.set_install_transfer_text(SharedString::new());
            }
        });
    }

    let main_weak = main_weak.clone();
    let svc = svc.clone();

    let total_steps = if modloader.is_some() { 2 } else { 1 };
    let _ = slint::invoke_from_event_loop(move || {
        with_task_progress(|tp| {
            tp.set_task_title(SharedString::from("Installing Minecraft"));
            tp.set_task_kind(SharedString::from("minecraft"));
            tp.set_task_stage(SharedString::from("Preparing installation..."));
            tp.set_step_text(SharedString::from(format!("Step 1 of {total_steps}")));
            tp.set_task_state(SharedString::from("Running"));
            tp.set_overall_progress(0.0);
            tp.set_overall_percent_text(SharedString::from("0%"));
            tp.set_progress(0.0);
            tp.set_percent_text(SharedString::from("0%"));
            tp.set_indeterminate(true);
            tp.set_is_paused(false);
            tp.set_is_cancelling(false);
            tp.set_is_finished(false);
            let _ = tp.show();
        });
    });

    svc.rt.spawn(async move {
        let progress_weak = main_weak.clone();
        let res = svc
            .installer
            .install_instance(&instance_id, cancel.clone(), move |p: InstallProgress| {
                let weak = progress_weak.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    let ratio = if !p.indeterminate && p.total_files > 0 {
                        if p.total_bytes > 0 {
                            (p.downloaded_bytes as f32 / p.total_bytes as f32).clamp(0.0, 1.0)
                        } else {
                            (p.completed_files as f32 / p.total_files as f32).clamp(0.0, 1.0)
                        }
                    } else {
                        0.0
                    };
                    let overall = if total_steps == 2 { ratio * 0.7 } else { ratio };
                    with_task_progress(|tp| {
                        tp.set_task_stage(SharedString::from(&p.message));
                        tp.set_indeterminate(p.indeterminate);
                        tp.set_overall_progress(overall);
                        tp.set_overall_percent_text(SharedString::from(format!(
                            "{:.0}%",
                            overall * 100.0
                        )));
                        if !p.indeterminate && p.total_files > 0 {
                            tp.set_progress(ratio);
                            tp.set_percent_text(SharedString::from(format!(
                                "{:.0}%",
                                ratio * 100.0
                            )));
                            tp.set_files_text(SharedString::from(format!(
                                "{}/{} files",
                                p.completed_files, p.total_files
                            )));
                            if p.total_bytes > 0 {
                                tp.set_bytes_text(SharedString::from(format!(
                                    "{} / {}",
                                    format_bytes(p.downloaded_bytes),
                                    format_bytes(p.total_bytes)
                                )));
                                tp.set_speed_text(SharedString::from(format!(
                                    "{}/s",
                                    format_bytes(p.bytes_per_sec)
                                )));
                            }
                        }
                    });
                    if let Some(win) = weak.upgrade() {
                        win.set_install_active(true);
                        win.set_install_operation_text(SharedString::from(&p.message));
                        win.set_install_indeterminate(p.indeterminate);
                        if !p.indeterminate && p.total_files > 0 {
                            win.set_install_progress(ratio);
                            win.set_install_files_text(SharedString::from(format!(
                                "{}/{} files",
                                p.completed_files, p.total_files
                            )));
                            if p.total_bytes > 0 {
                                win.set_install_transfer_text(SharedString::from(format!(
                                    "{} / {} • {}/s",
                                    format_bytes(p.downloaded_bytes),
                                    format_bytes(p.total_bytes),
                                    format_bytes(p.bytes_per_sec)
                                )));
                            }
                        }
                    }
                });
            })
            .await;

        let final_res = match res {
            Ok(()) => {
                if let Some((kind, loader_ver)) = modloader {
                    set_status(
                        &main_weak,
                        format!("Installing {} {}...", kind.label(), loader_ver),
                    );
                    let stage_msg = format!("Installing {} {}...", kind.label(), loader_ver);
                    let _ = slint::invoke_from_event_loop(move || {
                        with_task_progress(|tp| {
                            tp.set_task_kind(SharedString::from("modloader"));
                            tp.set_step_text(SharedString::from("Step 2 of 2"));
                            tp.set_task_stage(SharedString::from(stage_msg));
                            tp.set_overall_progress(0.75);
                            tp.set_overall_percent_text(SharedString::from("75%"));
                            tp.set_indeterminate(true);
                        });
                    });
                    let java_exec = svc
                        .state
                        .lock()
                        .ok()
                        .and_then(|g| g.java_runtimes.first().map(|r| r.path.clone()));
                    match install_modloader_with_progress(
                        &instance_id,
                        kind,
                        &loader_ver,
                        java_exec.as_deref(),
                        &svc.paths,
                        &svc.http_client,
                        &svc.download_manager,
                        &svc.instance_manager,
                        &cancel,
                        move |mp: ModloaderInstallProgress| {
                            let _ = slint::invoke_from_event_loop(move || {
                                with_task_progress(|tp| {
                                    tp.set_task_stage(SharedString::from(&mp.stage));
                                    tp.set_indeterminate(mp.indeterminate);
                                    if !mp.indeterminate && mp.total_files > 0 {
                                        let r = (mp.completed_files as f32 / mp.total_files as f32)
                                            .clamp(0.0, 1.0);
                                        let overall = 0.70 + r * 0.30;
                                        tp.set_progress(r);
                                        tp.set_percent_text(SharedString::from(format!(
                                            "{:.0}%",
                                            r * 100.0
                                        )));
                                        tp.set_overall_progress(overall);
                                        tp.set_overall_percent_text(SharedString::from(format!(
                                            "{:.0}%",
                                            overall * 100.0
                                        )));
                                        tp.set_files_text(SharedString::from(format!(
                                            "{}/{} files",
                                            mp.completed_files, mp.total_files
                                        )));
                                    }
                                });
                            });
                        },
                    )
                    .await
                    {
                        Ok(_) => Ok(()),
                        Err(err) => {
                            let _ = svc
                                .instance_manager
                                .update_installation_status(
                                    &instance_id,
                                    InstallationStatus::Failed,
                                    Some(format!("Modloader install failed: {err}")),
                                )
                                .await;
                            Err(err)
                        }
                    }
                } else {
                    Ok(())
                }
            }
            Err(err) => Err(err),
        };

        let (tp_state, tp_ok) = match &final_res {
            Ok(()) => ("Completed", true),
            Err(LauncherError::Cancelled) => ("Cancelled", false),
            Err(_) => ("Failed", false),
        };
        let status_msg = match &final_res {
            Ok(()) => "Installation complete.".to_string(),
            Err(LauncherError::Cancelled) => "Installation cancelled.".to_string(),
            Err(err) => format!("Installation failed: {err}"),
        };
        let tp_stage = status_msg.clone();
        let _ = slint::invoke_from_event_loop(move || {
            with_task_progress(|tp| {
                tp.set_task_state(SharedString::from(tp_state));
                tp.set_task_stage(SharedString::from(&tp_stage));
                tp.set_indeterminate(false);
                tp.set_is_cancelling(false);
                tp.set_is_finished(true);
                if tp_ok {
                    tp.set_overall_progress(1.0);
                    tp.set_overall_percent_text(SharedString::from("100%"));
                    tp.set_progress(1.0);
                    tp.set_percent_text(SharedString::from("100%"));
                    tp.set_error_message(SharedString::new());
                    let _ = tp.hide();
                } else {
                    tp.set_error_message(SharedString::from(tp_stage));
                }
            });
        });

        let refreshed = svc.instance_manager.list_instances().await.ok();

        let (selected_id, any_active) = if let Ok(mut guard) = svc.state.lock() {
            guard.active_installs.remove(&instance_id);
            if guard.focused_install_id.as_deref() == Some(&instance_id) {
                guard.focused_install_id = guard.active_installs.keys().next().cloned();
            }
            if let Some(list) = refreshed {
                guard.instances = list;
            }
            (
                guard.config.last_selected_instance.clone(),
                !guard.active_installs.is_empty(),
            )
        } else {
            (None, false)
        };

        let state = svc.state.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(win) = main_weak.upgrade() {
                refresh_main_ui(&win, &state, selected_id.as_deref());
                if !any_active {
                    win.set_install_active(false);
                }
                win.set_status_text(SharedString::from(status_msg));
            }
        });
    });
}

fn trigger_manifest_fetch(
    main_weak: &Weak<MainWindow>,
    add_weak: &Weak<AddInstanceWindow>,
    svc: &Services,
) {
    if let Some(win) = add_weak.upgrade() {
        win.set_is_loading(true);
        win.set_error_message(SharedString::new());
        win.set_notice_message(SharedString::new());
    }
    set_status(main_weak, "Fetching Minecraft version manifest...");

    let main_weak = main_weak.clone();
    let add_weak = add_weak.clone();
    let svc = svc.clone();

    svc.rt.spawn(async move {
        match svc.manifest_service.fetch_or_cached().await {
            Ok(loaded) => {
                let versions = loaded.manifest.all_versions();
                let latest_release = Some(loaded.manifest.latest.release.clone());
                let source = loaded.source;

                if let Ok(mut guard) = svc.state.lock() {
                    guard.versions = versions;
                    guard.latest_release = latest_release;
                    guard.manifest_loaded = true;
                }

                let state = svc.state.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(win) = add_weak.upgrade() {
                        win.set_is_loading(false);
                        match source {
                            ManifestSource::Network => win.set_notice_message(SharedString::new()),
                            ManifestSource::CacheFallback => {
                                win.set_notice_message(SharedString::from(
                                    "Offline mode: showing cached Minecraft versions.",
                                ));
                            }
                        }
                        refresh_filtered_versions(&win, &state, true);
                    }
                    if let Some(main) = main_weak.upgrade() {
                        let status = match source {
                            ManifestSource::Network => "Ready",
                            ManifestSource::CacheFallback => {
                                "Using cached Minecraft versions (offline)"
                            }
                        };
                        main.set_status_text(SharedString::from(status));
                    }
                });
            }
            Err(err) => {
                let msg = err.to_string();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(win) = add_weak.upgrade() {
                        win.set_is_loading(false);
                        win.set_error_message(SharedString::from(msg));
                    }
                    if let Some(main) = main_weak.upgrade() {
                        main.set_status_text(SharedString::from(
                            "Failed to fetch Minecraft versions",
                        ));
                    }
                });
            }
        }
    });
}

fn refresh_filtered_versions(
    add_win: &AddInstanceWindow,
    state: &Arc<Mutex<AppState>>,
    select_default_release: bool,
) {
    let query = add_win.get_search_query().to_string().to_lowercase();
    let show_releases = add_win.get_show_releases();
    let show_snapshots = add_win.get_show_snapshots();
    let show_betas = add_win.get_show_betas();
    let show_alphas = add_win.get_show_alphas();
    let current_selected = add_win.get_selected_version_id().to_string();

    let (filtered, latest_release) = match state.lock() {
        Ok(guard) => {
            let latest_rel = guard.latest_release.clone();
            let items: Vec<VersionItem> = guard
                .versions
                .iter()
                .filter(|v| match v.version_type {
                    VersionType::Release => show_releases,
                    VersionType::Snapshot => show_snapshots,
                    VersionType::OldBeta => show_betas,
                    VersionType::OldAlpha => show_alphas,
                    _ => false,
                })
                .filter(|v| query.is_empty() || v.id.to_lowercase().contains(&query))
                .map(|v| VersionItem {
                    recommended: latest_rel.as_deref() == Some(v.id.as_str()),
                    id: SharedString::from(&v.id),
                    version_type: SharedString::from(v.version_type.label()),
                    release_date: SharedString::from(v.formatted_release_date()),
                    is_snapshot: v.version_type == VersionType::Snapshot,
                })
                .collect();
            (items, latest_rel)
        }
        Err(_) => (Vec::new(), None),
    };

    let next_selected = if select_default_release {
        latest_release
            .filter(|id| filtered.iter().any(|item| item.id.as_str() == id))
            .or_else(|| filtered.first().map(|item| item.id.to_string()))
            .unwrap_or_default()
    } else if filtered
        .iter()
        .any(|item| item.id.as_str() == current_selected)
    {
        current_selected
    } else {
        filtered
            .first()
            .map(|item| item.id.to_string())
            .unwrap_or_default()
    };

    add_win.set_versions(ModelRc::from(Rc::new(VecModel::from(filtered))));
    add_win.set_selected_version_id(SharedString::from(&next_selected));
    if !add_win.get_user_edited_name() && !next_selected.is_empty() {
        add_win.set_instance_name(SharedString::from(&next_selected));
    }
}

fn refresh_main_ui(win: &MainWindow, state: &Arc<Mutex<AppState>>, selected_id: Option<&str>) {
    let Ok(guard) = state.lock() else {
        return;
    };

    let icons_dir = AppPaths::resolve().map(|p| p.icons_dir()).ok();
    let group_map: HashMap<&str, &str> = guard
        .groups
        .iter()
        .map(|g| (g.id.as_str(), g.name.as_str()))
        .collect();

    let to_ui_item = |inst: &InstanceMetadata| -> InstanceItem {
        let gname = inst
            .group_id
            .as_deref()
            .and_then(|gid| group_map.get(gid).copied())
            .unwrap_or("Ungrouped");
        let modloader_str = inst
            .modloader_spec
            .as_ref()
            .map(|s| format!("{} {}", s.kind.short_name(), s.version))
            .unwrap_or_default();
        let modloader_kind_str = inst
            .modloader_spec
            .as_ref()
            .map(|s| match s.kind {
                ModloaderKind::Fabric => "fabric",
                ModloaderKind::Forge => "forge",
                ModloaderKind::NeoForge => "neoforge",
                ModloaderKind::Quilt => "quilt",
                ModloaderKind::LiteLoader => "liteloader",
            })
            .unwrap_or("");
        let (custom_icon, has_custom_icon) =
            load_instance_icon_image(inst.icon_key.as_deref(), icons_dir.as_deref());
        let playtime_text = if inst.playtime.total_playtime_secs > 0 {
            inst.playtime.formatted_total()
        } else {
            String::new()
        };
        let last_played_text = inst
            .last_played
            .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
            .unwrap_or_default();

        InstanceItem {
            id: SharedString::from(&inst.id),
            name: SharedString::from(&inst.name),
            minecraft_version: SharedString::from(&inst.minecraft_version),
            version_type: SharedString::from(inst.version_type.label()),
            modloader: SharedString::from(modloader_str),
            modloader_kind: SharedString::from(modloader_kind_str),
            custom_icon,
            has_custom_icon,
            playtime_text: SharedString::from(playtime_text),
            last_played_text: SharedString::from(last_played_text),
            group_id: SharedString::from(inst.group_id.as_deref().unwrap_or("")),
            group_name: SharedString::from(gname),
            install_status_code: SharedString::from(inst.installation.status.code()),
            install_status_label: SharedString::from(inst.installation.status.label()),
            install_error: SharedString::from(
                inst.installation.last_error.as_deref().unwrap_or(""),
            ),
            is_running: guard.running_instances.contains(&inst.id),
        }
    };

    let query = win.get_instance_search_query().to_string().to_lowercase();
    let matches_query = |inst: &InstanceMetadata| -> bool {
        if query.is_empty() {
            return true;
        }
        let gname = inst
            .group_id
            .as_deref()
            .and_then(|gid| group_map.get(gid).copied())
            .unwrap_or("ungrouped");
        let loader = inst.modloader.as_deref().unwrap_or("vanilla");
        inst.name.to_lowercase().contains(&query)
            || inst.minecraft_version.to_lowercase().contains(&query)
            || loader.to_lowercase().contains(&query)
            || gname.to_lowercase().contains(&query)
    };

    let mut sorted_instances = guard.instances.clone();
    sort_instances_in_place(
        &mut sorted_instances,
        guard.config.general.sort_by,
        &guard.config.general.manual_order,
    );

    let all_items: Vec<InstanceItem> = sorted_instances
        .iter()
        .filter(|i| matches_query(i))
        .map(to_ui_item)
        .collect();
    win.set_instances(ModelRc::from(Rc::new(VecModel::from(all_items))));

    let has_groups = !guard.groups.is_empty();
    win.set_has_groups(has_groups);

    if has_groups {
        let mut sections: Vec<GroupSectionItem> = Vec::new();
        for g in &guard.groups {
            let group_instances: Vec<InstanceItem> = sorted_instances
                .iter()
                .filter(|i| i.group_id.as_deref() == Some(g.id.as_str()) && matches_query(i))
                .map(to_ui_item)
                .collect();
            if !query.is_empty() && group_instances.is_empty() {
                continue;
            }
            sections.push(GroupSectionItem {
                id: SharedString::from(&g.id),
                name: SharedString::from(&g.name),
                collapsed: g.collapsed,
                is_ungrouped: false,
                instances: ModelRc::from(Rc::new(VecModel::from(group_instances))),
            });
        }

        // Instâncias sem grupo ou com group_id órfão vão para "Ungrouped".
        let ungrouped: Vec<InstanceItem> = sorted_instances
            .iter()
            .filter(|i| {
                i.group_id
                    .as_deref()
                    .map(|gid| !group_map.contains_key(gid))
                    .unwrap_or(true)
                    && matches_query(i)
            })
            .map(to_ui_item)
            .collect();
        if !ungrouped.is_empty() {
            sections.push(GroupSectionItem {
                id: SharedString::new(),
                name: SharedString::from("Ungrouped"),
                collapsed: false,
                is_ungrouped: true,
                instances: ModelRc::from(Rc::new(VecModel::from(ungrouped))),
            });
        }
        win.set_group_sections(ModelRc::from(Rc::new(VecModel::from(sections))));
    } else {
        win.set_group_sections(ModelRc::from(Rc::new(VecModel::from(Vec::new()))));
    }

    drop(guard);
    refresh_selection_ui(win, state, selected_id);
    update_active_account_label(win, state);
}

fn refresh_selection_ui(win: &MainWindow, state: &Arc<Mutex<AppState>>, selected_id: Option<&str>) {
    let Ok(guard) = state.lock() else {
        return;
    };
    let icons_dir = AppPaths::resolve().map(|p| p.icons_dir()).ok();
    let selected = selected_id.and_then(|id| guard.instances.iter().find(|i| i.id == id));
    match selected {
        Some(inst) => {
            let gname = inst
                .group_id
                .as_deref()
                .and_then(|gid| guard.groups.iter().find(|g| g.id == gid))
                .map(|g| g.name.as_str())
                .unwrap_or("Ungrouped");
            let modloader_str = inst
                .modloader_spec
                .as_ref()
                .map(|s| format!("{} {}", s.kind.short_name(), s.version))
                .unwrap_or_default();
            let (custom_icon, has_custom_icon) =
                load_instance_icon_image(inst.icon_key.as_deref(), icons_dir.as_deref());
            let playtime_str = if inst.playtime.total_playtime_secs > 0 {
                inst.playtime.formatted_total()
            } else {
                String::new()
            };
            let last_played_str = inst
                .last_played
                .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
                .unwrap_or_default();

            win.set_selected_instance_id(SharedString::from(&inst.id));
            win.set_selected_instance_name(SharedString::from(&inst.name));
            win.set_selected_instance_version(SharedString::from(&inst.minecraft_version));
            win.set_selected_instance_modloader(SharedString::from(modloader_str));
            win.set_selected_instance_playtime(SharedString::from(playtime_str));
            win.set_selected_instance_last_played(SharedString::from(last_played_str));
            win.set_selected_instance_icon(custom_icon);
            win.set_selected_instance_has_icon(has_custom_icon);
            win.set_selected_instance_type(SharedString::from(inst.version_type.label()));
            win.set_selected_instance_group_name(SharedString::from(gname));
            win.set_selected_instance_status_code(SharedString::from(
                inst.installation.status.code(),
            ));
            win.set_selected_instance_status_label(SharedString::from(
                inst.installation.status.label(),
            ));
            win.set_selected_instance_error(SharedString::from(
                inst.installation.last_error.as_deref().unwrap_or(""),
            ));
            win.set_selected_instance_running(guard.running_instances.contains(&inst.id));
        }
        None => {
            win.set_selected_instance_id(SharedString::new());
            win.set_selected_instance_name(SharedString::new());
            win.set_selected_instance_version(SharedString::new());
            win.set_selected_instance_modloader(SharedString::new());
            win.set_selected_instance_playtime(SharedString::new());
            win.set_selected_instance_last_played(SharedString::new());
            win.set_selected_instance_has_icon(false);
            win.set_selected_instance_type(SharedString::new());
            win.set_selected_instance_group_name(SharedString::from("Ungrouped"));
            win.set_selected_instance_status_code(SharedString::from("not_installed"));
            win.set_selected_instance_status_label(SharedString::from("Not installed"));
            win.set_selected_instance_error(SharedString::new());
            win.set_selected_instance_running(false);
        }
    }
}

fn update_active_account_label(win: &MainWindow, state: &Arc<Mutex<AppState>>) {
    if let Ok(guard) = state.lock() {
        let active = guard
            .accounts
            .iter()
            .find(|a| a.is_default)
            .or_else(|| guard.accounts.first());

        match active {
            Some(acc) => {
                win.set_has_active_account(true);
                win.set_active_account_name(SharedString::from(&acc.username));
            }
            None => {
                win.set_has_active_account(false);
                win.set_active_account_name(SharedString::new());
            }
        }

        let items: Vec<AccountItem> = guard
            .accounts
            .iter()
            .map(|a| AccountItem {
                id: SharedString::from(&a.id),
                username: SharedString::from(&a.username),
                kind: SharedString::from(a.kind.label()),
                status: SharedString::from(a.status.label()),
                is_default: a.is_default,
            })
            .collect();
        win.set_accounts(ModelRc::from(Rc::new(VecModel::from(items))));
    }
}

fn apply_console_appearance(win: &ConsoleWindow, cfg: &LauncherConfig) {
    win.set_autoscroll(cfg.minecraft.console_autoscroll);
    win.set_word_wrap(cfg.minecraft.console_word_wrap);
    win.set_show_timestamps(cfg.minecraft.console_show_timestamps);
    win.set_colorize_levels(cfg.minecraft.console_colorize_levels);
    win.set_use_theme_font(cfg.minecraft.console_use_theme_font);
    win.set_custom_font_family(SharedString::from(&cfg.minecraft.console_font_family));
    let sz = cfg.minecraft.console_font_size as f32;
    win.set_console_font_size(sz);
    win.set_base_font_size(sz);
    win.set_line_spacing(cfg.minecraft.console_line_spacing as f32);
    win.set_confirm_before_clear(cfg.minecraft.confirm_before_clearing_console);
}

fn apply_capabilities_to_settings(win: &SettingsWindow, caps: &SystemCapabilities) {
    let total_mib = detect_system_memory_mib();
    let avail_mib = detect_available_memory_mib();
    let total_str = format_mib_display(total_mib);
    let avail_str = format_mib_display(avail_mib);

    win.set_is_linux(cfg!(target_os = "linux"));
    win.set_system_ram_display(SharedString::from(&total_str));
    win.set_system_avail_ram_display(SharedString::from(&avail_str));
    win.set_system_ram_total_mib(total_mib.unwrap_or(16384) as i32);

    win.set_gamemode_available(caps.gamemode.is_available());
    win.set_gamemode_status(SharedString::from(caps.gamemode.short_label()));
    win.set_mangohud_available(caps.mangohud.is_available());
    win.set_mangohud_status(SharedString::from(caps.mangohud.short_label()));
    win.set_mangohud_has_config(caps.mangohud_config_path.is_some());
    win.set_discrete_gpu_available(caps.gpu.has_discrete_gpu());
    win.set_gpu_detail(SharedString::from(&caps.gpu.detail));
    win.set_zink_available(caps.zink.is_available());
    win.set_zink_status(SharedString::from(caps.zink.short_label()));

    win.set_sys_os(SharedString::from(&caps.os_name));
    win.set_sys_arch(SharedString::from(&caps.cpu_arch));
    win.set_sys_ram(SharedString::from(format!(
        "{total_str} (Available: {avail_str})"
    )));
    win.set_sys_gpu(SharedString::from(caps.gpu.primary_adapter_label()));
    win.set_sys_session(SharedString::from(&caps.display_session));
    win.set_sys_gamemode(SharedString::from(caps.gamemode.short_label()));
    win.set_sys_mangohud(SharedString::from(caps.mangohud.short_label()));
    win.set_sys_vulkan(SharedString::from(&caps.vulkan_detail));
    win.set_system_info_text(SharedString::from(format_system_info_report_from_caps(
        caps,
    )));
}

fn populate_settings_window(win: &SettingsWindow, state: &Arc<Mutex<AppState>>) {
    if let Ok(guard) = state.lock() {
        let cfg = &guard.config;
        win.set_language(SharedString::from(&cfg.language));
        win.set_language_options(ModelRc::from(Rc::new(VecModel::from(
            build_language_options(),
        ))));
        win.set_language_label(SharedString::from(language_display_label(&cfg.language)));
        win.set_confirm_instance_delete(cfg.general.confirm_instance_delete);
        win.set_rename_folder_policy(match cfg.general.rename_folder_policy {
            crab_launcher::core::config::RenameFolderPolicy::Ask => 0,
            crab_launcher::core::config::RenameFolderPolicy::Always => 1,
            crab_launcher::core::config::RenameFolderPolicy::Never => 2,
        });
        win.set_folder_instances(SharedString::from(&cfg.folders.instances_dir));
        win.set_folder_mods(SharedString::from(&cfg.folders.mods_dir));
        win.set_folder_icons(SharedString::from(&cfg.folders.icons_dir));
        win.set_folder_java_runtimes(SharedString::from(&cfg.folders.java_runtimes_dir));
        win.set_folder_skins(SharedString::from(&cfg.folders.skins_dir));
        win.set_folder_downloads(SharedString::from(&cfg.folders.downloads_dir));
        win.set_console_history_limit(SharedString::from(
            cfg.console.log_history_limit.to_string(),
        ));
        win.set_console_stop_on_overflow(cfg.console.stop_logging_on_overflow);
        win.set_concurrent_task_limit(SharedString::from(
            cfg.tasks.concurrent_task_limit.to_string(),
        ));
        win.set_concurrent_download_limit(SharedString::from(
            cfg.tasks.concurrent_download_limit.to_string(),
        ));
        win.set_retry_limit(SharedString::from(cfg.tasks.retry_limit.to_string()));
        win.set_http_timeout_secs(SharedString::from(cfg.tasks.http_timeout_secs.to_string()));
        win.set_confirm_group_delete(cfg.general.confirm_group_delete);
        win.set_launch_on_double_click(cfg.general.launch_on_double_click);

        // Java
        win.set_java_auto_select(cfg.java.auto_select);
        win.set_java_auto_download(cfg.java.auto_download);
        let (provider_idx, provider_id, provider_lbl) = match cfg.java.preferred_provider {
            JavaProviderKind::Mojang => (0, "mojang", "Mojang"),
            JavaProviderKind::Adoptium => (1, "adoptium", "Eclipse Adoptium"),
            JavaProviderKind::Azul => (2, "azul", "Azul Zulu"),
            JavaProviderKind::Semeru => (3, "semeru", "IBM Semeru"),
        };
        win.set_java_preferred_provider(provider_idx);
        win.set_java_provider_options(ModelRc::from(Rc::new(VecModel::from(
            build_java_provider_options(),
        ))));
        win.set_java_provider_id(SharedString::from(provider_id));
        win.set_java_provider_label(SharedString::from(provider_lbl));
        let manual_str = cfg.java.manual_path.as_deref().unwrap_or("");
        win.set_java_manual_path(SharedString::from(manual_str));
        let selected_summary = if cfg.java.auto_select {
            "Automatic (compatible with Minecraft version and modloader)".to_string()
        } else if let Some(rt) = guard
            .java_runtimes
            .iter()
            .find(|r| r.path.to_string_lossy() == manual_str)
        {
            rt.display_label()
        } else if !manual_str.is_empty() {
            manual_str.to_string()
        } else {
            "Not configured".to_string()
        };
        win.set_java_selected_summary(SharedString::from(selected_summary));

        // Minecraft — General
        win.set_window_width(SharedString::from(cfg.minecraft.window_width.to_string()));
        win.set_window_height(SharedString::from(cfg.minecraft.window_height.to_string()));
        win.set_start_fullscreen(cfg.minecraft.start_fullscreen);
        win.set_use_custom_resolution(cfg.minecraft.use_custom_resolution);
        let res_opts = build_resolution_preset_options(guard.native_resolution);
        let (res_id, res_lbl) = match_resolution_preset(
            cfg.minecraft.window_width,
            cfg.minecraft.window_height,
            guard.native_resolution,
        );
        win.set_resolution_preset_options(ModelRc::from(Rc::new(VecModel::from(res_opts))));
        win.set_resolution_preset_id(SharedString::from(res_id));
        win.set_resolution_preset_label(SharedString::from(res_lbl));
        win.set_on_launch_behavior(cfg.minecraft.on_launch_behavior.to_index());
        win.set_reopen_on_exit(cfg.minecraft.reopen_on_exit);
        win.set_reopen_on_crash(cfg.minecraft.reopen_on_crash);
        win.set_reopen_on_kill(cfg.minecraft.reopen_on_kill);
        win.set_check_java_compatibility(cfg.minecraft.check_java_compatibility);
        win.set_warn_incompatible_java(cfg.minecraft.warn_incompatible_java);
        win.set_verify_before_launch(cfg.minecraft.verify_before_launch);
        win.set_auto_repair_missing(cfg.minecraft.auto_repair_missing);
        win.set_allow_multiple_instances(cfg.minecraft.allow_multiple_instances);
        win.set_confirm_before_stopping(cfg.minecraft.confirm_before_stopping);

        // Minecraft — Java
        win.set_min_memory_mb(SharedString::from(cfg.minecraft.min_memory_mb.to_string()));
        win.set_max_memory_mb(SharedString::from(cfg.minecraft.max_memory_mb.to_string()));
        let total_mib = detect_system_memory_mib();
        let avail_mib = detect_available_memory_mib();
        let mem_opts = build_memory_preset_options(total_mib, avail_mib);
        let (mem_id, mem_lbl) = match_memory_preset(cfg.minecraft.max_memory_mb);
        win.set_memory_preset_options(ModelRc::from(Rc::new(VecModel::from(mem_opts))));
        win.set_memory_preset_id(SharedString::from(mem_id));
        win.set_memory_preset_label(SharedString::from(mem_lbl));
        win.set_memory_warning(SharedString::from(memory_warning_text(
            cfg.minecraft.max_memory_mb,
            total_mib,
            avail_mib,
        )));
        win.set_warn_insufficient_memory(cfg.minecraft.warn_insufficient_memory);
        win.set_use_default_jvm_args(cfg.minecraft.use_default_jvm_args);
        win.set_jvm_args(SharedString::from(&cfg.minecraft.jvm_args));

        let (sel_major, sel_arch) = guard
            .java_runtimes
            .iter()
            .max_by_key(|r| r.major_version)
            .map(|r| (r.major_version, r.arch.as_str()))
            .unwrap_or((21, std::env::consts::ARCH));
        let jvm_opts = build_jvm_preset_options(&guard.jvm_presets, sel_major, sel_arch);
        win.set_jvm_preset_options(ModelRc::from(Rc::new(VecModel::from(jvm_opts))));
        let pid = &cfg.minecraft.jvm_preset_id;
        if let Some(p) = guard.jvm_presets.iter().find(|p| &p.id == pid) {
            win.set_jvm_preset_id(SharedString::from(&p.id));
            win.set_jvm_preset_label(SharedString::from(&p.name));
            win.set_jvm_preset_description(SharedString::from(&p.description));
            win.set_jvm_preset_compat(SharedString::from(p.compatibility_summary()));
        } else {
            win.set_jvm_preset_id(SharedString::from("custom"));
            win.set_jvm_preset_label(SharedString::from("Custom"));
            win.set_jvm_preset_description(SharedString::from(
                "Custom user-defined JVM arguments.",
            ));
            win.set_jvm_preset_compat(SharedString::new());
        }

        // Appearance — Instance Grid
        win.set_grid_icon_size(cfg.appearance.icon_size.to_index());
        win.set_grid_spacing(cfg.appearance.grid_spacing.to_index());
        win.set_grid_show_name(cfg.appearance.show_instance_name);
        win.set_grid_show_version(cfg.appearance.show_mc_version);
        win.set_grid_show_modloader(cfg.appearance.show_modloader_badge);
        win.set_grid_sort_by(cfg.general.sort_by.to_index());
        let conflicts = detect_conflicting_jvm_args(&cfg.minecraft.jvm_args);
        win.set_jvm_conflict_warning(SharedString::from(if conflicts.is_empty() {
            String::new()
        } else {
            format!(
                "Conflicting memory flags detected ({}): managed by Crab's Memory fields.",
                conflicts.join(", ")
            )
        }));
        win.set_jvm_test_result(SharedString::new());

        // Minecraft — Console
        win.set_show_console_on_launch(cfg.minecraft.show_console_on_launch);
        win.set_show_console_on_crash(cfg.minecraft.show_console_on_crash);
        win.set_close_console_on_exit(cfg.minecraft.close_console_on_exit);
        win.set_console_autoscroll(cfg.minecraft.console_autoscroll);
        win.set_console_word_wrap(cfg.minecraft.console_word_wrap);
        win.set_confirm_before_clearing_console(cfg.minecraft.confirm_before_clearing_console);
        win.set_console_font_family(SharedString::from(&cfg.minecraft.console_font_family));
        win.set_console_font_size(SharedString::from(
            cfg.minecraft.console_font_size.to_string(),
        ));
        win.set_console_line_spacing(SharedString::from(
            cfg.minecraft.console_line_spacing.to_string(),
        ));
        win.set_console_use_theme_font(cfg.minecraft.console_use_theme_font);
        win.set_console_colorize_levels(cfg.minecraft.console_colorize_levels);
        win.set_console_show_timestamps(cfg.minecraft.console_show_timestamps);
        win.set_console_max_lines(SharedString::from(
            cfg.minecraft.console_max_lines.to_string(),
        ));
        win.set_preserve_complete_logs(cfg.minecraft.preserve_complete_logs);

        // Minecraft — Tweaks
        win.set_enable_gamemode(cfg.minecraft.tweaks.enable_gamemode);
        win.set_enable_mangohud(cfg.minecraft.tweaks.enable_mangohud);
        win.set_use_discrete_gpu(cfg.minecraft.tweaks.use_discrete_gpu);
        win.set_use_zink(cfg.minecraft.tweaks.use_zink);
        win.set_tweaks_feedback(SharedString::new());
        apply_capabilities_to_settings(win, &guard.capabilities);

        // Downloads
        win.set_max_concurrent_downloads(SharedString::from(
            cfg.downloads.max_concurrent.to_string(),
        ));
        win.set_error_message(SharedString::new());
    }
    populate_java_list(win, state);
    populate_themes_list(win, state);
}

fn populate_themes_list(win: &SettingsWindow, state: &Arc<Mutex<AppState>>) {
    if let Ok(guard) = state.lock() {
        let active_id = &guard.config.appearance.theme_id;
        let items: Vec<ThemeItem> = guard
            .themes
            .iter()
            .map(|t| ThemeItem {
                id: SharedString::from(&t.id),
                name: SharedString::from(&t.name),
                author: SharedString::from(&t.author),
                version: SharedString::from(&t.version),
                description: SharedString::from(&t.description),
                is_builtin: t.is_builtin,
                is_active: &t.id == active_id,
                swatch_bg: to_slint_color(t.bg_primary),
                swatch_surface: to_slint_color(t.bg_secondary),
                swatch_border: to_slint_color(t.border),
                swatch_accent: to_slint_color(t.accent),
                swatch_text: to_slint_color(t.text_primary),
                swatch_text_sec: to_slint_color(t.text_secondary),
            })
            .collect();
        win.set_themes(ModelRc::from(Rc::new(VecModel::from(items))));
        win.set_selected_theme_id(SharedString::from(active_id));
    }
}

fn to_slint_color(c: RgbaColor) -> slint::Color {
    slint::Color::from_argb_u8(c.a, c.r, c.g, c.b)
}

fn apply_theme_to_global(theme_global: &Theme, resolved: &ResolvedTheme) {
    theme_global.set_bg_primary(to_slint_color(resolved.bg_primary));
    theme_global.set_bg_secondary(to_slint_color(resolved.bg_secondary));
    theme_global.set_bg_interactive(to_slint_color(resolved.bg_interactive));
    theme_global.set_bg_interactive_hover(to_slint_color(resolved.bg_interactive_hover));
    theme_global.set_bg_interactive_pressed(to_slint_color(resolved.bg_interactive_pressed));
    theme_global.set_border(to_slint_color(resolved.border));
    theme_global.set_border_subtle(to_slint_color(resolved.border_subtle));
    theme_global.set_accent(to_slint_color(resolved.accent));
    theme_global.set_accent_hover(to_slint_color(resolved.accent_hover));
    theme_global.set_accent_pressed(to_slint_color(resolved.accent_pressed));
    theme_global.set_accent_muted(to_slint_color(resolved.accent_muted));
    theme_global.set_text_on_accent(to_slint_color(resolved.text_on_accent));
    theme_global.set_text_primary(to_slint_color(resolved.text_primary));
    theme_global.set_text_secondary(to_slint_color(resolved.text_secondary));
    theme_global.set_text_disabled(to_slint_color(resolved.text_disabled));
    theme_global.set_danger(to_slint_color(resolved.danger));
    theme_global.set_success(to_slint_color(resolved.success));
    theme_global.set_warning_text(to_slint_color(resolved.warning_text));
    theme_global.set_console_bg(to_slint_color(resolved.console_bg));
    let is_vice = resolved.id == "vice";
    theme_global.set_is_vice_theme(is_vice);
    theme_global.set_display_font_family(SharedString::from(if is_vice {
        "Pricedown"
    } else {
        "Inter"
    }));
    theme_global.set_chrome_bg(to_slint_color(resolved.bg_secondary));
    theme_global.set_radius_sm(resolved.border_radius as f32);
    theme_global.set_radius_md((resolved.border_radius + 1) as f32);
}

fn populate_select_java_window(win: &SelectJavaWindow, state: &Arc<Mutex<AppState>>) {
    let query = win.get_search_query().to_string().to_lowercase();
    if let Ok(guard) = state.lock() {
        let items: Vec<JavaRuntimeItem> = guard
            .java_runtimes
            .iter()
            .filter(|rt| {
                query.is_empty()
                    || rt.display_label().to_lowercase().contains(&query)
                    || rt.path.to_string_lossy().to_lowercase().contains(&query)
            })
            .map(|rt| JavaRuntimeItem {
                path: SharedString::from(rt.path.to_string_lossy().as_ref()),
                label: SharedString::from(rt.display_label()),
                major: rt.major_version as i32,
            })
            .collect();
        win.set_runtimes(ModelRc::from(Rc::new(VecModel::from(items))));
    }
}

fn wire_select_java_callbacks(
    settings_window: &Rc<SettingsWindow>,
    edit_window: &Rc<EditInstanceWindow>,
    sj_window: &Rc<SelectJavaWindow>,
    svc: &Services,
) {
    {
        let sj_weak = sj_window.as_weak();
        let state = svc.state.clone();
        sj_window.on_search_changed(move |_| {
            if let Some(w) = sj_weak.upgrade() {
                populate_select_java_window(&w, &state);
            }
        });
    }
    {
        let sj_weak = sj_window.as_weak();
        let svc = svc.clone();
        sj_window.on_refresh_clicked(move || {
            if let Some(w) = sj_weak.upgrade() {
                w.set_is_scanning(true);
            }
            let sj_weak = sj_weak.clone();
            let svc = svc.clone();
            let rt = svc.rt.clone();
            rt.spawn(async move {
                let custom = svc
                    .state
                    .lock()
                    .map(|g| g.config.java.custom_paths.clone())
                    .unwrap_or_default();
                let runtimes = discover_java_runtimes(&svc.paths, &custom).await;
                if let Ok(mut g) = svc.state.lock() {
                    g.java_runtimes = runtimes;
                }
                let state = svc.state.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(w) = sj_weak.upgrade() {
                        w.set_is_scanning(false);
                        populate_select_java_window(&w, &state);
                    }
                });
            });
        });
    }
    {
        let settings_weak = settings_window.as_weak();
        let edit_weak = edit_window.as_weak();
        let sj_weak = sj_window.as_weak();
        let state = svc.state.clone();
        sj_window.on_select_clicked(move |path| {
            let target = state
                .lock()
                .map(|g| g.select_java_target)
                .unwrap_or(SelectJavaTarget::EditInstance);
            match target {
                SelectJavaTarget::GlobalSettings => {
                    if let Some(sw) = settings_weak.upgrade() {
                        sw.set_java_auto_select(false);
                        sw.set_java_manual_path(path.clone());
                        let label = state
                            .lock()
                            .ok()
                            .and_then(|g| {
                                g.java_runtimes
                                    .iter()
                                    .find(|r| r.path.to_string_lossy() == path.as_str())
                                    .map(|r| r.display_label())
                            })
                            .unwrap_or_else(|| path.to_string());
                        sw.set_java_selected_summary(SharedString::from(label));
                    }
                }
                SelectJavaTarget::EditInstance => {
                    if let Some(ew) = edit_weak.upgrade() {
                        ew.set_java_path(path);
                    }
                }
            }
            if let Some(w) = sj_weak.upgrade() {
                let _ = w.hide();
            }
        });
    }
    {
        let sj_weak = sj_window.as_weak();
        sj_window.on_cancel_clicked(move || {
            if let Some(w) = sj_weak.upgrade() {
                let _ = w.hide();
            }
        });
    }
}

fn populate_java_list(win: &SettingsWindow, state: &Arc<Mutex<AppState>>) {
    if let Ok(guard) = state.lock() {
        let items: Vec<JavaRuntimeItem> = guard
            .java_runtimes
            .iter()
            .map(|rt| JavaRuntimeItem {
                path: SharedString::from(rt.path.to_string_lossy().as_ref()),
                label: SharedString::from(rt.display_label()),
                major: rt.major_version as i32,
            })
            .collect();
        win.set_java_runtimes(ModelRc::from(Rc::new(VecModel::from(items))));
    }
}

fn build_group_options(state: &Arc<Mutex<AppState>>) -> Vec<GroupOption> {
    let mut opts = vec![GroupOption {
        id: SharedString::new(),
        name: SharedString::from("Ungrouped"),
    }];
    if let Ok(guard) = state.lock() {
        for g in &guard.groups {
            opts.push(GroupOption {
                id: SharedString::from(&g.id),
                name: SharedString::from(&g.name),
            });
        }
    }
    opts
}

fn populate_edit_instance_window(
    win: &EditInstanceWindow,
    svc: &Services,
    instance_id: &str,
) -> bool {
    let opts = build_group_options(&svc.state);
    let Ok(guard) = svc.state.lock() else {
        return false;
    };
    let Some(inst) = guard
        .instances
        .iter()
        .find(|i| i.id == instance_id)
        .cloned()
    else {
        return false;
    };
    let is_running = guard.running_instances.contains(instance_id);
    let global_cfg = guard.config.clone();
    let global_mc = guard.config.minecraft.clone();
    let caps = guard.capabilities.clone();
    drop(guard);

    // Ícone da instância para a aba Version + recursos locais (packs, worlds, servers)
    let (inst_icon_img, has_inst_icon) =
        load_instance_icon_image(inst.icon_key.as_deref(), Some(&svc.paths.icons_dir()));
    win.set_instance_icon(inst_icon_img);
    win.set_has_instance_icon(has_inst_icon);
    refresh_edit_resources_ui(win, svc, &inst);

    let version_json_path = svc.paths.version_metadata_file(&inst.minecraft_version);
    let version_meta: Option<VersionMetadata> = std::fs::read(&version_json_path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok());
    // Popula componentes persistidos em `<instance_dir>/components.json` (respeitando ordem e customizações)
    let inst_dir = svc.paths.instance_dir(&inst.dir_name);
    let comp_manifest = ComponentManifest::load_or_init(&inst_dir, &inst);
    let comps: Vec<ComponentItem> = comp_manifest
        .components
        .iter()
        .map(|c| ComponentItem {
            uid: SharedString::from(&c.uid),
            name: SharedString::from(&c.name),
            version: SharedString::from(&c.version),
            status: SharedString::from(if c.customized {
                "Customized"
            } else {
                c.kind.label()
            }),
            required: c.required,
        })
        .collect();
    win.set_components(ModelRc::from(Rc::new(VecModel::from(comps))));
    win.set_has_modloader(inst.modloader_spec.is_some());
    win.set_is_running(is_running);

    // Popula mods locais em `.minecraft/mods/` enriquecidos com metadados do ModIndex
    let mods_dir = svc.paths.instance_game_dir(&inst.dir_name).join("mods");
    let mod_index = ModIndex::load(&mods_dir);
    let mod_q = win.get_mod_search_query().to_string().to_lowercase();
    let expected_loader = inst.modloader_spec.as_ref().map(|s| s.kind);
    let mods: Vec<LocalModItem> = list_local_mods(&mods_dir, expected_loader)
        .into_iter()
        .filter(|m| mod_q.is_empty() || m.name.to_lowercase().contains(&mod_q))
        .map(|m| {
            let rec = mod_index.find_by_file(&m.file_name);
            let provider = rec.map(|r| r.provider.label()).unwrap_or("Local");
            let homepage_url = rec.map(|r| r.homepage_url.clone()).unwrap_or_default();
            let last_modified = std::fs::metadata(mods_dir.join(&m.file_name))
                .and_then(|md| md.modified())
                .ok()
                .map(|st| {
                    let dt: chrono::DateTime<chrono::Local> = st.into();
                    dt.format("%Y-%m-%d").to_string()
                })
                .unwrap_or_default();
            let (icon, has_icon) = rec
                .and_then(|r| {
                    let safe_id: String = r
                        .project_id
                        .chars()
                        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
                        .collect();
                    let p = svc.paths.cache_dir.join("mod-icons").join(format!(
                        "{}-{}.png",
                        r.provider.label().to_lowercase(),
                        safe_id
                    ));
                    Image::load_from_path(&p).ok()
                })
                .map(|img| (img, true))
                .unwrap_or_else(|| (Image::default(), false));
            let author = rec
                .map(|r| r.author.clone())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| m.author.clone());
            let description = rec
                .map(|r| r.description.clone())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| m.description.clone());
            LocalModItem {
                file_name: SharedString::from(m.file_name),
                name: SharedString::from(m.name),
                version: SharedString::from(m.version),
                loader: SharedString::from(m.loader.map(|l| l.short_name()).unwrap_or("Unknown")),
                last_modified: SharedString::from(last_modified),
                provider: SharedString::from(provider),
                homepage_url: SharedString::from(homepage_url),
                author: SharedString::from(author),
                description: SharedString::from(description),
                icon,
                has_icon,
                enabled: m.enabled,
                selected: false,
                warning: SharedString::from(m.warning.unwrap_or_default()),
            }
        })
        .collect();
    if let Some(first) = mods.first() {
        win.set_selected_mod_file(first.file_name.clone());
        win.set_selected_mod_name(first.name.clone());
        win.set_selected_mod_author(first.author.clone());
        win.set_selected_mod_description(first.description.clone());
        win.set_selected_mod_url(first.homepage_url.clone());
        win.set_selected_mod_provider(first.provider.clone());
        win.set_selected_mod_icon(first.icon.clone());
        win.set_selected_mod_has_icon(first.has_icon);
    } else {
        win.set_selected_mod_file(SharedString::new());
    }
    win.set_mods(ModelRc::from(Rc::new(VecModel::from(mods))));

    let group_name_display = svc
        .state
        .lock()
        .ok()
        .and_then(|g| {
            inst.group_id
                .as_deref()
                .and_then(|gid| g.groups.iter().find(|x| x.id == gid))
                .map(|x| x.name.clone())
        })
        .unwrap_or_else(|| "Ungrouped".to_string());
    let total_mib = detect_system_memory_mib();
    let avail_mib = detect_available_memory_mib();
    let ram_display = format_mib_display(total_mib);
    let avail_display = format_mib_display(avail_mib);
    let effective = EffectiveInstanceSettings::resolve(&global_cfg, &inst.overrides);

    let group_dd: Vec<DropdownOption> = opts
        .iter()
        .map(|g| DropdownOption {
            id: g.id.clone(),
            label: g.name.clone(),
            detail: SharedString::new(),
            icon: Image::default(),
            has_icon: false,
            enabled: true,
        })
        .collect();
    win.set_groups(ModelRc::from(Rc::new(VecModel::from(opts))));
    win.set_group_dropdown_options(ModelRc::from(Rc::new(VecModel::from(group_dd))));
    win.set_instance_id(SharedString::from(&inst.id));
    win.set_instance_name(SharedString::from(&inst.name));
    win.set_group_name_display(SharedString::from(group_name_display));
    win.set_system_ram_display(SharedString::from(ram_display));
    win.set_system_avail_ram_display(SharedString::from(avail_display));
    win.set_system_ram_total_mib(total_mib.unwrap_or(16384) as i32);
    win.set_java_test_result(SharedString::new());
    win.set_jvm_test_result(SharedString::new());
    win.set_show_reset_confirm(false);
    win.set_selected_group_id(SharedString::from(inst.group_id.as_deref().unwrap_or("")));

    // General: Window + Launcher Behavior (exibe valores globais efetivos quando override está desmarcado)
    win.set_override_window(inst.overrides.override_window);
    win.set_window_width(SharedString::from(effective.window_width.to_string()));
    win.set_window_height(SharedString::from(effective.window_height.to_string()));
    win.set_start_fullscreen(effective.start_fullscreen);
    win.set_use_custom_resolution(effective.use_custom_resolution);
    let native_res = svc.state.lock().ok().and_then(|g| g.native_resolution);
    let res_opts = build_resolution_preset_options(native_res);
    let (res_id, res_lbl) =
        match_resolution_preset(effective.window_width, effective.window_height, native_res);
    win.set_resolution_preset_options(ModelRc::from(Rc::new(VecModel::from(res_opts))));
    win.set_resolution_preset_id(SharedString::from(res_id));
    win.set_resolution_preset_label(SharedString::from(res_lbl));

    win.set_override_launcher_behavior(inst.overrides.override_launcher_behavior);
    win.set_on_launch_behavior(effective.on_launch_behavior.to_index());
    win.set_reopen_on_exit(effective.reopen_on_exit);
    win.set_reopen_on_crash(effective.reopen_on_crash);
    win.set_reopen_on_kill(effective.reopen_on_kill);

    // Java
    win.set_override_java(inst.overrides.override_java);
    win.set_java_path(SharedString::from(if inst.overrides.override_java {
        inst.overrides.java_path.as_deref().unwrap_or("")
    } else {
        global_cfg.java.manual_path.as_deref().unwrap_or("")
    }));
    win.set_override_memory(inst.overrides.override_memory);
    win.set_min_memory_mb(SharedString::from(effective.min_memory_mb.to_string()));
    win.set_max_memory_mb(SharedString::from(effective.max_memory_mb.to_string()));
    let mem_opts = build_memory_preset_options(total_mib, avail_mib);
    let (mem_id, mem_lbl) = match_memory_preset(effective.max_memory_mb);
    win.set_memory_preset_options(ModelRc::from(Rc::new(VecModel::from(mem_opts))));
    win.set_memory_preset_id(SharedString::from(mem_id));
    win.set_memory_preset_label(SharedString::from(mem_lbl));
    win.set_memory_warning(SharedString::from(memory_warning_text(
        effective.max_memory_mb,
        total_mib,
        avail_mib,
    )));
    win.set_override_jvm_args(inst.overrides.override_jvm_args);
    win.set_use_default_jvm_args(effective.use_default_jvm_args);
    win.set_jvm_args(SharedString::from(if inst.overrides.override_jvm_args {
        inst.overrides
            .jvm_args
            .as_deref()
            .unwrap_or(&global_mc.jvm_args)
    } else {
        &global_mc.jvm_args
    }));
    let req_major = version_meta.as_ref().map(required_java_major).unwrap_or(17);
    if let Ok(g) = svc.state.lock() {
        let jvm_opts = build_jvm_preset_options(&g.jvm_presets, req_major, std::env::consts::ARCH);
        win.set_jvm_preset_options(ModelRc::from(Rc::new(VecModel::from(jvm_opts))));
        if let Some(p) = g
            .jvm_presets
            .iter()
            .find(|p| p.id == effective.jvm_preset_id)
        {
            win.set_jvm_preset_id(SharedString::from(&p.id));
            win.set_jvm_preset_label(SharedString::from(&p.name));
            win.set_jvm_preset_description(SharedString::from(&p.description));
            win.set_jvm_preset_compat(SharedString::from(p.compatibility_summary()));
        } else {
            win.set_jvm_preset_id(SharedString::from("custom"));
            win.set_jvm_preset_label(SharedString::from("Custom"));
            win.set_jvm_preset_description(SharedString::from(
                "Custom user-defined JVM arguments.",
            ));
            win.set_jvm_preset_compat(SharedString::new());
        }
    }

    // Console
    win.set_override_console(inst.overrides.override_console);
    win.set_show_console_on_launch(effective.show_console_on_launch);
    win.set_show_console_on_crash(effective.show_console_on_crash);
    win.set_close_console_on_exit(effective.close_console_on_exit);

    // Tweaks
    win.set_is_linux(cfg!(target_os = "linux"));
    win.set_override_tweaks(inst.overrides.override_tweaks);
    win.set_enable_gamemode(effective.tweaks.enable_gamemode);
    win.set_enable_mangohud(effective.tweaks.enable_mangohud);
    win.set_use_discrete_gpu(effective.tweaks.use_discrete_gpu);
    win.set_use_zink(effective.tweaks.use_zink);
    win.set_gamemode_available(caps.gamemode.is_available());
    win.set_gamemode_status(SharedString::from(caps.gamemode.short_label()));
    win.set_mangohud_available(caps.mangohud.is_available());
    win.set_mangohud_status(SharedString::from(caps.mangohud.short_label()));
    win.set_discrete_gpu_available(caps.gpu.has_discrete_gpu());
    win.set_gpu_detail(SharedString::from(&caps.gpu.detail));
    win.set_zink_available(caps.zink.is_available());
    win.set_zink_status(SharedString::from(caps.zink.short_label()));

    win.set_error_message(SharedString::new());
    true
}

fn populate_launch_summary_window(
    win: &LaunchSummaryWindow,
    svc: &Services,
    instance_id: &str,
) -> bool {
    let Ok(guard) = svc.state.lock() else {
        return false;
    };
    let Some(inst) = guard
        .instances
        .iter()
        .find(|i| i.id == instance_id)
        .cloned()
    else {
        return false;
    };
    let config = guard.config.clone();
    let runtimes = guard.java_runtimes.clone();
    let caps = guard.capabilities.clone();
    drop(guard);

    let effective = EffectiveInstanceSettings::resolve(&config, &inst.overrides);
    let version_json = svc.paths.version_metadata_file(&inst.minecraft_version);
    let base_meta: Option<VersionMetadata> = std::fs::read(&version_json)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok());
    let loader_profile_path = svc
        .paths
        .instance_dir(&inst.dir_name)
        .join("loader-profile.json");
    let loader_profile: Option<LoaderProfile> = if loader_profile_path.exists() {
        std::fs::read(&loader_profile_path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
    } else {
        None
    };
    let composed_meta = base_meta
        .as_ref()
        .map(|bm| compose_version_metadata(bm, loader_profile.as_ref()));

    let required_major = composed_meta
        .as_ref()
        .map(required_java_major)
        .unwrap_or(17);

    let (java_label, java_path) = if let Some(manual) = &effective.manual_java_path {
        let p = PathBuf::from(manual);
        let lbl = runtimes
            .iter()
            .find(|r| r.path == p)
            .map(|r| r.display_label())
            .unwrap_or_else(|| manual.clone());
        (lbl, Some(p))
    } else if let Some(found) = select_compatible_runtime(&runtimes, required_major) {
        (
            format!("Auto: {}", found.display_label()),
            Some(found.path.clone()),
        )
    } else {
        (
            format!("Auto (Java {required_major} required — will download/select on launch)"),
            None,
        )
    };

    let platform = TargetPlatform::current();
    let summary = build_launch_summary(
        &inst,
        composed_meta.as_ref(),
        &effective,
        &java_label,
        java_path,
        &caps,
        &svc.paths,
        &platform,
    );

    if let Ok(mut guard) = svc.state.lock() {
        guard.last_launch_summary_text = summary.plain_text.clone();
    }

    let items: Vec<LaunchSummaryRowItem> = summary
        .rows
        .into_iter()
        .map(|r| LaunchSummaryRowItem {
            label: SharedString::from(r.label),
            value: SharedString::from(r.value),
            source: SharedString::from(r.source),
        })
        .collect();

    win.set_instance_name(SharedString::from(summary.instance_name));
    win.set_rows(ModelRc::from(Rc::new(VecModel::from(items))));
    win.set_warnings_text(SharedString::from(summary.warnings.join("\n")));
    win.set_copy_feedback(SharedString::new());
    true
}

fn refresh_console_window(win: &ConsoleWindow, state: &Arc<Mutex<AppState>>, instance_id: &str) {
    let query = win.get_search_query().to_string().to_lowercase();
    if let Ok(guard) = state.lock() {
        let name = guard
            .instances
            .iter()
            .find(|i| i.id == instance_id)
            .map(|i| i.name.as_str())
            .unwrap_or("Minecraft");
        let is_running = guard.running_instances.contains(instance_id);
        win.set_instance_name(SharedString::from(name));
        win.set_is_running(is_running);
        win.set_process_status(SharedString::from(if is_running {
            "Running"
        } else {
            "Stopped"
        }));

        let lines: Vec<ConsoleLogLine> = guard
            .console_buffers
            .get(instance_id)
            .map(|buf| {
                buf.entries()
                    .iter()
                    .filter(|e| query.is_empty() || e.message.to_lowercase().contains(&query))
                    .map(|e| ConsoleLogLine {
                        timestamp: SharedString::from(&e.timestamp),
                        level: SharedString::from(e.level.code()),
                        message: SharedString::from(&e.message),
                    })
                    .collect()
            })
            .unwrap_or_default();
        win.set_lines(ModelRc::from(Rc::new(VecModel::from(lines))));
    }
}

fn set_status(main_weak: &Weak<MainWindow>, message: impl Into<String>) {
    let msg = message.into();
    let main_weak = main_weak.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(win) = main_weak.upgrade() {
            win.set_status_text(SharedString::from(msg));
        }
    });
}

fn wire_install_java_callbacks(
    settings_window: &Rc<SettingsWindow>,
    ij_window: &Rc<InstallJavaWindow>,
    svc: &Services,
) {
    {
        let ij_weak = ij_window.as_weak();
        let svc = svc.clone();
        ij_window.on_provider_changed(move |idx| {
            trigger_java_catalog_fetch(&ij_weak, &svc, JavaProviderKind::from_index(idx));
        });
    }
    {
        let ij_weak = ij_window.as_weak();
        let state = svc.state.clone();
        ij_window.on_major_selected(move |_| {
            if let Some(win) = ij_weak.upgrade() {
                refresh_install_java_ui(&win, &state, false);
            }
        });
    }
    {
        let ij_weak = ij_window.as_weak();
        let state = svc.state.clone();
        ij_window.on_filters_changed(move || {
            if let Some(win) = ij_weak.upgrade() {
                refresh_install_java_ui(&win, &state, false);
            }
        });
    }
    {
        let ij_weak = ij_window.as_weak();
        let svc = svc.clone();
        ij_window.on_refresh_clicked(move || {
            let idx = ij_weak
                .upgrade()
                .map(|w| w.get_selected_provider())
                .unwrap_or(0);
            trigger_java_catalog_fetch(&ij_weak, &svc, JavaProviderKind::from_index(idx));
        });
    }
    {
        let ij_weak = ij_window.as_weak();
        let state = svc.state.clone();
        ij_window.on_recommended_clicked(move || {
            if let Some(win) = ij_weak.upgrade() {
                win.set_filter_recommended(true);
                refresh_install_java_ui(&win, &state, true);
            }
        });
    }
    {
        let ij_weak = ij_window.as_weak();
        ij_window.on_cancel_clicked(move || {
            if let Some(win) = ij_weak.upgrade() {
                let _ = win.hide();
            }
        });
    }
    {
        let settings_weak = settings_window.as_weak();
        let ij_weak = ij_window.as_weak();
        let svc = svc.clone();
        ij_window.on_download_clicked(move |idx| {
            let build = svc
                .state
                .lock()
                .ok()
                .and_then(|g| g.java_catalog.get(idx as usize).cloned());
            let Some(build) = build else {
                return;
            };
            if let Some(win) = ij_weak.upgrade() {
                win.set_is_installing(true);
                win.set_status_message(SharedString::from(format!(
                    "Installing {} {}...",
                    build.distribution, build.full_version
                )));
            }
            let settings_weak = settings_weak.clone();
            let ij_weak = ij_weak.clone();
            let svc = svc.clone();
            let rt = svc.rt.clone();
            rt.spawn(async move {
                let cancel = CancellationToken::new();
                if let Ok(mut g) = svc.state.lock() {
                    g.active_install_cancel = Some(cancel.clone());
                }
                let title = format!(
                    "Installing Java ({} {})",
                    build.distribution, build.full_version
                );
                let _ = slint::invoke_from_event_loop(move || {
                    with_task_progress(|tp| {
                        tp.set_task_title(SharedString::from(title));
                        tp.set_task_kind(SharedString::from("java"));
                        tp.set_step_text(SharedString::from("Step 1 of 2"));
                        tp.set_task_stage(SharedString::from(
                            "Downloading and extracting Java runtime...",
                        ));
                        tp.set_task_state(SharedString::from("Running"));
                        tp.set_overall_progress(0.25);
                        tp.set_overall_percent_text(SharedString::from("25%"));
                        tp.set_indeterminate(true);
                        tp.set_is_paused(false);
                        tp.set_is_cancelling(false);
                        tp.set_is_finished(false);
                        let _ = tp.show();
                    });
                });
                let res = install_java_build_with_progress(
                    &build,
                    &svc.paths,
                    &svc.http_client,
                    &svc.download_manager,
                    &cancel,
                    move |jp: JavaInstallProgress| {
                        let _ = slint::invoke_from_event_loop(move || {
                            with_task_progress(|tp| {
                                tp.set_step_text(SharedString::from(format!(
                                    "Executing task {} of {}",
                                    jp.step, jp.total_steps
                                )));
                                tp.set_task_stage(SharedString::from(&jp.stage));
                                tp.set_indeterminate(jp.indeterminate);
                                let step_ratio = if !jp.indeterminate {
                                    if jp.total_bytes > 0 {
                                        (jp.downloaded_bytes as f32 / jp.total_bytes as f32)
                                            .clamp(0.0, 1.0)
                                    } else if jp.total_files > 0 {
                                        (jp.completed_files as f32 / jp.total_files as f32)
                                            .clamp(0.0, 1.0)
                                    } else {
                                        0.0
                                    }
                                } else {
                                    0.0
                                };
                                let overall = if jp.step == 1 {
                                    step_ratio * 0.80
                                } else {
                                    0.85
                                };
                                tp.set_overall_progress(overall);
                                tp.set_overall_percent_text(SharedString::from(format!(
                                    "{:.0}%",
                                    overall * 100.0
                                )));
                                if !jp.indeterminate {
                                    tp.set_progress(step_ratio);
                                    tp.set_percent_text(SharedString::from(format!(
                                        "{:.0}%",
                                        step_ratio * 100.0
                                    )));
                                }
                                if jp.total_bytes > 0 {
                                    tp.set_bytes_text(SharedString::from(format!(
                                        "{} / {}",
                                        format_bytes(jp.downloaded_bytes),
                                        format_bytes(jp.total_bytes)
                                    )));
                                    tp.set_speed_text(SharedString::from(format!(
                                        "{}/s",
                                        format_bytes(jp.bytes_per_sec)
                                    )));
                                }
                                if jp.total_files > 1 {
                                    tp.set_files_text(SharedString::from(format!(
                                        "{}/{} files",
                                        jp.completed_files, jp.total_files
                                    )));
                                }
                            });
                        });
                    },
                )
                .await;
                if let Ok(mut g) = svc.state.lock() {
                    g.active_install_cancel = None;
                }
                let (tp_state, tp_stage, tp_ok) = match &res {
                    Ok(_) => ("Completed", "Java runtime installed.".to_string(), true),
                    Err(LauncherError::Cancelled) => (
                        "Cancelled",
                        "Java installation cancelled.".to_string(),
                        false,
                    ),
                    Err(e) => ("Failed", format!("Java installation failed: {e}"), false),
                };
                let _ = slint::invoke_from_event_loop(move || {
                    with_task_progress(|tp| {
                        tp.set_task_state(SharedString::from(tp_state));
                        tp.set_task_stage(SharedString::from(&tp_stage));
                        tp.set_indeterminate(false);
                        tp.set_is_cancelling(false);
                        tp.set_is_finished(true);
                        if tp_ok {
                            tp.set_overall_progress(1.0);
                            tp.set_overall_percent_text(SharedString::from("100%"));
                            tp.set_progress(1.0);
                            tp.set_percent_text(SharedString::from("100%"));
                            tp.set_error_message(SharedString::new());
                            let _ = tp.hide();
                        } else {
                            tp.set_error_message(SharedString::from(tp_stage));
                        }
                    });
                });
                match res {
                    Ok(rt) => {
                        let label = rt.display_label();
                        if let Ok(mut guard) = svc.state.lock() {
                            guard.java_runtimes.retain(|r| r.path != rt.path);
                            guard.java_runtimes.insert(0, rt);
                        }
                        let state = svc.state.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(win) = ij_weak.upgrade() {
                                win.set_is_installing(false);
                                win.set_status_message(SharedString::from(format!(
                                    "Installed {label}"
                                )));
                            }
                            if let Some(sw) = settings_weak.upgrade() {
                                populate_java_list(&sw, &state);
                                sw.set_java_status_message(SharedString::from(format!(
                                    "Installed {label}"
                                )));
                            }
                        });
                    }
                    Err(err) => {
                        let msg = format!("Install failed: {err}");
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(win) = ij_weak.upgrade() {
                                win.set_is_installing(false);
                                win.set_error_message(SharedString::from(msg));
                            }
                        });
                    }
                }
            });
        });
    }
}

fn trigger_java_catalog_fetch(
    ij_weak: &Weak<InstallJavaWindow>,
    svc: &Services,
    provider: JavaProviderKind,
) {
    if let Some(win) = ij_weak.upgrade() {
        win.set_is_loading(true);
        win.set_error_message(SharedString::new());
        win.set_selected_build_index(-1);
        win.set_status_message(SharedString::from(format!(
            "Loading {} catalog...",
            provider.label()
        )));
    }
    let ij_weak = ij_weak.clone();
    let svc = svc.clone();
    svc.rt.spawn(async move {
        match fetch_provider_builds(&svc.http_client, provider).await {
            Ok(builds) => {
                let count = builds.len();
                if let Ok(mut guard) = svc.state.lock() {
                    guard.java_catalog = builds;
                }
                let state = svc.state.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(win) = ij_weak.upgrade() {
                        win.set_is_loading(false);
                        win.set_status_message(SharedString::from(format!(
                            "{count} build(s) available."
                        )));
                        refresh_install_java_ui(&win, &state, true);
                    }
                });
            }
            Err(err) => {
                let msg = err.to_string();
                if let Ok(mut guard) = svc.state.lock() {
                    guard.java_catalog.clear();
                }
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(win) = ij_weak.upgrade() {
                        win.set_is_loading(false);
                        win.set_builds(ModelRc::from(Rc::new(VecModel::from(Vec::new()))));
                        win.set_major_versions(ModelRc::from(Rc::new(VecModel::from(Vec::new()))));
                        win.set_error_message(SharedString::from(msg));
                    }
                });
            }
        }
    });
}

fn refresh_install_java_ui(
    win: &InstallJavaWindow,
    state: &Arc<Mutex<AppState>>,
    reset_major: bool,
) {
    let major_query = win.get_major_search().to_string().to_lowercase();
    let build_query = win.get_build_search().to_string().to_lowercase();
    let want_jre = win.get_filter_jre();
    let want_jdk = win.get_filter_jdk();
    let only_rec = win.get_filter_recommended();

    let Ok(guard) = state.lock() else {
        return;
    };

    let mut majors: Vec<i32> = guard
        .java_catalog
        .iter()
        .map(|b| b.major_version as i32)
        .collect();
    majors.sort_by(|a, b| b.cmp(a));
    majors.dedup();
    if !major_query.is_empty() {
        majors.retain(|m| m.to_string().contains(&major_query));
    }

    let selected_major = if reset_major || !majors.contains(&win.get_selected_major()) {
        majors.first().copied().unwrap_or(0)
    } else {
        win.get_selected_major()
    };

    let items: Vec<JavaBuildItem> = guard
        .java_catalog
        .iter()
        .enumerate()
        .filter(|(_, b)| selected_major == 0 || b.major_version as i32 == selected_major)
        .filter(|(_, b)| match b.package_type {
            JavaPackageType::Jre => want_jre,
            JavaPackageType::Jdk => want_jdk,
        })
        .filter(|(_, b)| !only_rec || b.is_recommended)
        .filter(|(_, b)| {
            build_query.is_empty()
                || b.full_version.to_lowercase().contains(&build_query)
                || b.build_number
                    .as_deref()
                    .unwrap_or("")
                    .to_lowercase()
                    .contains(&build_query)
        })
        .map(|(idx, b)| JavaBuildItem {
            index: idx as i32,
            version: SharedString::from(&b.full_version),
            build: SharedString::from(b.build_number.as_deref().unwrap_or("-")),
            released: SharedString::from(b.release_date.as_deref().unwrap_or("-")),
            pkg_type: SharedString::from(b.package_type.label()),
            arch: SharedString::from(&b.arch),
            is_recommended: b.is_recommended,
        })
        .collect();

    win.set_major_versions(ModelRc::from(Rc::new(VecModel::from(majors))));
    win.set_selected_major(selected_major);
    win.set_builds(ModelRc::from(Rc::new(VecModel::from(items))));
}

fn wire_install_loader_callbacks(
    main_window: &MainWindow,
    edit_window: &Rc<EditInstanceWindow>,
    il_window: &Rc<InstallLoaderWindow>,
    svc: &Services,
) {
    {
        let il_weak = il_window.as_weak();
        let svc = svc.clone();
        il_window.on_loader_changed(move |idx| {
            let Some(kind) = ModloaderKind::from_index(idx) else {
                return;
            };
            let mc_ver = il_weak
                .upgrade()
                .map(|w| w.get_minecraft_version().to_string())
                .unwrap_or_default();
            trigger_loader_catalog_fetch(&il_weak, &svc, kind, mc_ver);
        });
    }
    {
        let il_weak = il_window.as_weak();
        let state = svc.state.clone();
        il_window.on_filters_changed(move || {
            if let Some(w) = il_weak.upgrade() {
                refresh_install_loader_ui(&w, &state);
            }
        });
    }
    {
        let il_weak = il_window.as_weak();
        let svc = svc.clone();
        il_window.on_refresh_clicked(move || {
            let Some(w) = il_weak.upgrade() else {
                return;
            };
            let Some(kind) = ModloaderKind::from_index(w.get_selected_loader()) else {
                return;
            };
            trigger_loader_catalog_fetch(
                &il_weak,
                &svc,
                kind,
                w.get_minecraft_version().to_string(),
            );
        });
    }
    {
        let il_weak = il_window.as_weak();
        let state = svc.state.clone();
        il_window.on_recommended_clicked(move || {
            if let Some(w) = il_weak.upgrade() {
                w.set_show_experimental(false);
                refresh_install_loader_ui(&w, &state);
            }
        });
    }
    {
        let il_weak = il_window.as_weak();
        il_window.on_cancel_clicked(move || {
            if let Some(w) = il_weak.upgrade() {
                let _ = w.hide();
            }
        });
    }
    {
        let main_weak = main_window.as_weak();
        let edit_weak = edit_window.as_weak();
        let il_weak = il_window.as_weak();
        let svc = svc.clone();
        il_window.on_install_clicked(move |idx| {
            let Some(w) = il_weak.upgrade() else {
                return;
            };
            let instance_id = w.get_instance_id().to_string();
            let chosen = svc
                .state
                .lock()
                .ok()
                .and_then(|g| g.loader_catalog.get(idx as usize).cloned());
            let Some(chosen) = chosen else {
                return;
            };

            w.set_is_installing(true);
            w.set_error_message(SharedString::new());
            w.set_status_message(SharedString::from(format!(
                "Installing {} {}...",
                chosen.loader.label(),
                chosen.version
            )));

            let java_exec = svc
                .state
                .lock()
                .ok()
                .and_then(|g| g.java_runtimes.first().map(|r| r.path.clone()));

            let main_weak = main_weak.clone();
            let edit_weak = edit_weak.clone();
            let il_weak = il_weak.clone();
            let svc = svc.clone();
            let rt = svc.rt.clone();

            rt.spawn(async move {
                let cancel = CancellationToken::new();
                if let Ok(mut g) = svc.state.lock() {
                    g.active_install_cancel = Some(cancel.clone());
                }
                let title = format!("Installing {} {}", chosen.loader.label(), chosen.version);
                let _ = slint::invoke_from_event_loop(move || {
                    with_task_progress(|tp| {
                        tp.set_task_title(SharedString::from(title));
                        tp.set_task_kind(SharedString::from("modloader"));
                        tp.set_step_text(SharedString::from("Step 1 of 2"));
                        tp.set_task_stage(SharedString::from(
                            "Downloading and installing modloader...",
                        ));
                        tp.set_task_state(SharedString::from("Running"));
                        tp.set_overall_progress(0.25);
                        tp.set_overall_percent_text(SharedString::from("25%"));
                        tp.set_indeterminate(true);
                        tp.set_is_paused(false);
                        tp.set_is_cancelling(false);
                        tp.set_is_finished(false);
                        let _ = tp.show();
                    });
                });
                let res = install_modloader_with_progress(
                    &instance_id,
                    chosen.loader,
                    &chosen.version,
                    java_exec.as_deref(),
                    &svc.paths,
                    &svc.http_client,
                    &svc.download_manager,
                    &svc.instance_manager,
                    &cancel,
                    move |mp: ModloaderInstallProgress| {
                        let _ = slint::invoke_from_event_loop(move || {
                            with_task_progress(|tp| {
                                tp.set_step_text(SharedString::from(format!(
                                    "Executing task {} of {}",
                                    mp.step, mp.total_steps
                                )));
                                tp.set_task_stage(SharedString::from(&mp.stage));
                                tp.set_indeterminate(mp.indeterminate);
                                let step_frac = if !mp.indeterminate && mp.total_files > 0 {
                                    (mp.completed_files as f32 / mp.total_files as f32)
                                        .clamp(0.0, 1.0)
                                } else {
                                    0.0
                                };
                                let overall = ((mp.step.saturating_sub(1) as f32 + step_frac)
                                    / mp.total_steps.max(1) as f32)
                                    .clamp(0.0, 1.0);
                                tp.set_overall_progress(overall);
                                tp.set_overall_percent_text(SharedString::from(format!(
                                    "{:.0}%",
                                    overall * 100.0
                                )));
                                if !mp.indeterminate && mp.total_files > 0 {
                                    tp.set_progress(step_frac);
                                    tp.set_percent_text(SharedString::from(format!(
                                        "{:.0}%",
                                        step_frac * 100.0
                                    )));
                                    tp.set_files_text(SharedString::from(format!(
                                        "{}/{} files",
                                        mp.completed_files, mp.total_files
                                    )));
                                }
                            });
                        });
                    },
                )
                .await;
                if let Ok(mut g) = svc.state.lock() {
                    g.active_install_cancel = None;
                }
                let (tp_state, tp_stage, tp_ok) = match &res {
                    Ok(_) => ("Completed", "Modloader installed.".to_string(), true),
                    Err(LauncherError::Cancelled) => (
                        "Cancelled",
                        "Modloader installation cancelled.".to_string(),
                        false,
                    ),
                    Err(e) => (
                        "Failed",
                        format!("Modloader installation failed: {e}"),
                        false,
                    ),
                };
                let _ = slint::invoke_from_event_loop(move || {
                    with_task_progress(|tp| {
                        tp.set_task_state(SharedString::from(tp_state));
                        tp.set_task_stage(SharedString::from(&tp_stage));
                        tp.set_indeterminate(false);
                        tp.set_is_cancelling(false);
                        tp.set_is_finished(true);
                        if tp_ok {
                            tp.set_overall_progress(1.0);
                            tp.set_overall_percent_text(SharedString::from("100%"));
                            tp.set_progress(1.0);
                            tp.set_percent_text(SharedString::from("100%"));
                            tp.set_error_message(SharedString::new());
                            let _ = tp.hide();
                        } else {
                            tp.set_error_message(SharedString::from(tp_stage));
                        }
                    });
                });

                match res {
                    Ok(spec) => {
                        if let Ok(list) = svc.instance_manager.list_instances().await {
                            if let Ok(mut g) = svc.state.lock() {
                                g.instances = list;
                            }
                        }
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(il) = il_weak.upgrade() {
                                il.set_is_installing(false);
                                let _ = il.hide();
                            }
                            if let Some(ew) = edit_weak.upgrade() {
                                populate_edit_instance_window(&ew, &svc, &instance_id);
                            }
                            if let Some(main) = main_weak.upgrade() {
                                refresh_main_ui(&main, &svc.state, Some(&instance_id));
                                main.set_status_text(SharedString::from(format!(
                                    "Installed {} {}.",
                                    spec.kind.label(),
                                    spec.version
                                )));
                            }
                        });
                    }
                    Err(err) => {
                        let msg = err.to_string();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(il) = il_weak.upgrade() {
                                il.set_is_installing(false);
                                il.set_error_message(SharedString::from(msg));
                            }
                        });
                    }
                }
            });
        });
    }
}

fn trigger_loader_catalog_fetch(
    il_weak: &Weak<InstallLoaderWindow>,
    svc: &Services,
    kind: ModloaderKind,
    mc_version: String,
) {
    if let Some(w) = il_weak.upgrade() {
        w.set_is_loading(true);
        w.set_error_message(SharedString::new());
        w.set_selected_version_index(-1);
        w.set_status_message(SharedString::from(format!(
            "Loading {} versions for {}...",
            kind.short_name(),
            mc_version
        )));
    }
    let il_weak = il_weak.clone();
    let svc = svc.clone();
    svc.rt.spawn(async move {
        match fetch_loader_versions(&svc.http_client, kind, &mc_version).await {
            Ok(list) => {
                let count = list.len();
                if let Ok(mut g) = svc.state.lock() {
                    g.loader_catalog = list;
                }
                let state = svc.state.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(w) = il_weak.upgrade() {
                        w.set_is_loading(false);
                        w.set_status_message(SharedString::from(format!(
                            "{count} compatible version(s)."
                        )));
                        refresh_install_loader_ui(&w, &state);
                    }
                });
            }
            Err(err) => {
                let msg = err.to_string();
                if let Ok(mut g) = svc.state.lock() {
                    g.loader_catalog.clear();
                }
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(w) = il_weak.upgrade() {
                        w.set_is_loading(false);
                        w.set_versions(ModelRc::from(Rc::new(VecModel::from(Vec::new()))));
                        w.set_error_message(SharedString::from(msg));
                    }
                });
            }
        }
    });
}

fn refresh_install_loader_ui(win: &InstallLoaderWindow, state: &Arc<Mutex<AppState>>) {
    let query = win.get_search_query().to_string().to_lowercase();
    let show_exp = win.get_show_experimental();

    let Ok(guard) = state.lock() else {
        return;
    };

    let items: Vec<LoaderVersionItem> = guard
        .loader_catalog
        .iter()
        .enumerate()
        .filter(|(_, v)| show_exp || v.stable)
        .filter(|(_, v)| query.is_empty() || v.version.to_lowercase().contains(&query))
        .map(|(idx, v)| LoaderVersionItem {
            index: idx as i32,
            version: SharedString::from(&v.version),
            release_type: SharedString::from(if v.stable { "Stable" } else { "Experimental" }),
            status: SharedString::from(if v.recommended {
                "Recommended"
            } else {
                "Compatible"
            }),
            recommended: v.recommended,
        })
        .collect();

    let default_idx = items
        .iter()
        .find(|i| i.recommended)
        .or_else(|| items.first())
        .map(|i| i.index)
        .unwrap_or(-1);

    win.set_versions(ModelRc::from(Rc::new(VecModel::from(items))));
    win.set_selected_version_index(default_idx);
}

fn populate_change_group_window(
    win: &ChangeGroupWindow,
    state: &Arc<Mutex<AppState>>,
    instance_id: &str,
) -> bool {
    let query = win.get_search_query().to_string().to_lowercase();
    let Ok(guard) = state.lock() else {
        return false;
    };
    let Some(inst) = guard.instances.iter().find(|i| i.id == instance_id) else {
        return false;
    };

    let mut opts = Vec::new();
    if query.is_empty() || "no group".contains(&query) || "ungrouped".contains(&query) {
        opts.push(GroupOption {
            id: SharedString::new(),
            name: SharedString::from("No Group"),
        });
    }
    for g in &guard.groups {
        if query.is_empty() || g.name.to_lowercase().contains(&query) {
            opts.push(GroupOption {
                id: SharedString::from(&g.id),
                name: SharedString::from(&g.name),
            });
        }
    }

    win.set_instance_id(SharedString::from(&inst.id));
    win.set_instance_name(SharedString::from(&inst.name));
    win.set_selected_group_id(SharedString::from(inst.group_id.as_deref().unwrap_or("")));
    win.set_groups(ModelRc::from(Rc::new(VecModel::from(opts))));
    win.set_error_message(SharedString::new());
    true
}

fn wire_change_group_callbacks(
    main_window: &MainWindow,
    edit_window: &Rc<EditInstanceWindow>,
    cg_window: &Rc<ChangeGroupWindow>,
    svc: &Services,
) {
    {
        let cg_weak = cg_window.as_weak();
        let state = svc.state.clone();
        cg_window.on_search_changed(move |_| {
            if let Some(w) = cg_weak.upgrade() {
                let id = w.get_instance_id().to_string();
                let current_sel = w.get_selected_group_id();
                populate_change_group_window(&w, &state, &id);
                w.set_selected_group_id(current_sel);
            }
        });
    }
    {
        let main_weak = main_window.as_weak();
        let cg_weak = cg_window.as_weak();
        let svc = svc.clone();
        cg_window.on_create_group_clicked(move |name| {
            let name_str = name.to_string();
            let main_weak = main_weak.clone();
            let cg_weak = cg_weak.clone();
            let svc = svc.clone();
            let rt = svc.rt.clone();
            rt.spawn(async move {
                match svc.group_manager.create_group(&name_str).await {
                    Ok(created) => {
                        let groups = svc.group_manager.load().await;
                        let selected_inst = svc.state.lock().ok().and_then(|mut g| {
                            g.groups = groups;
                            g.config.last_selected_instance.clone()
                        });
                        let new_gid = created.id;
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(w) = cg_weak.upgrade() {
                                let id = w.get_instance_id().to_string();
                                populate_change_group_window(&w, &svc.state, &id);
                                w.set_selected_group_id(SharedString::from(&new_gid));
                            }
                            if let Some(main) = main_weak.upgrade() {
                                refresh_main_ui(&main, &svc.state, selected_inst.as_deref());
                            }
                        });
                    }
                    Err(err) => {
                        let msg = err.to_string();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(w) = cg_weak.upgrade() {
                                w.set_error_message(SharedString::from(msg));
                            }
                        });
                    }
                }
            });
        });
    }
    {
        let main_weak = main_window.as_weak();
        let cg_weak = cg_window.as_weak();
        let svc = svc.clone();
        cg_window.on_rename_group_clicked(move |group_id, new_name| {
            let gid = group_id.to_string();
            let name_str = new_name.to_string();
            let main_weak = main_weak.clone();
            let cg_weak = cg_weak.clone();
            let svc = svc.clone();
            let rt = svc.rt.clone();
            rt.spawn(async move {
                match svc.group_manager.rename_group(&gid, &name_str).await {
                    Ok(_) => {
                        let groups = svc.group_manager.load().await;
                        if let Ok(mut guard) = svc.state.lock() {
                            guard.groups = groups;
                        }
                        let state = svc.state.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(w) = cg_weak.upgrade() {
                                let inst_id = w.get_instance_id().to_string();
                                let sel = w.get_selected_group_id();
                                populate_change_group_window(&w, &state, &inst_id);
                                w.set_selected_group_id(sel);
                            }
                            if let Some(main) = main_weak.upgrade() {
                                refresh_main_ui(&main, &state, None);
                            }
                        });
                    }
                    Err(err) => {
                        let msg = err.to_string();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(w) = cg_weak.upgrade() {
                                w.set_error_message(SharedString::from(msg));
                            }
                        });
                    }
                }
            });
        });
    }
    {
        let main_weak = main_window.as_weak();
        let cg_weak = cg_window.as_weak();
        let svc = svc.clone();
        cg_window.on_delete_group_clicked(move |group_id| {
            let gid = group_id.to_string();
            let main_weak = main_weak.clone();
            let cg_weak = cg_weak.clone();
            let svc = svc.clone();
            let rt = svc.rt.clone();
            rt.spawn(async move {
                if svc.group_manager.delete_group(&gid).await.is_ok() {
                    let groups = svc.group_manager.load().await;
                    if let Ok(mut guard) = svc.state.lock() {
                        guard.groups = groups;
                        for inst in &mut guard.instances {
                            if inst.group_id.as_deref() == Some(gid.as_str()) {
                                inst.group_id = None;
                            }
                        }
                    }
                    let state = svc.state.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(w) = cg_weak.upgrade() {
                            let inst_id = w.get_instance_id().to_string();
                            populate_change_group_window(&w, &state, &inst_id);
                        }
                        if let Some(main) = main_weak.upgrade() {
                            refresh_main_ui(&main, &state, None);
                        }
                    });
                }
            });
        });
    }
    {
        let main_weak = main_window.as_weak();
        let edit_weak = edit_window.as_weak();
        let cg_weak = cg_window.as_weak();
        let svc = svc.clone();
        cg_window.on_apply_clicked(move |inst_id, group_id| {
            let id = inst_id.to_string();
            let gid = {
                let s = group_id.to_string();
                if s.is_empty() {
                    None
                } else {
                    Some(s)
                }
            };
            let main_weak = main_weak.clone();
            let edit_weak = edit_weak.clone();
            let cg_weak = cg_weak.clone();
            let svc = svc.clone();
            let rt = svc.rt.clone();
            rt.spawn(async move {
                if let Ok(updated) = svc.instance_manager.set_instance_group(&id, gid).await {
                    if let Ok(mut guard) = svc.state.lock() {
                        if let Some(item) = guard.instances.iter_mut().find(|i| i.id == id) {
                            *item = updated;
                        }
                    }
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(w) = cg_weak.upgrade() {
                            let _ = w.hide();
                        }
                        if let Some(ew) = edit_weak.upgrade() {
                            if ew.get_instance_id().as_str() == id {
                                populate_edit_instance_window(&ew, &svc, &id);
                            }
                        }
                        if let Some(main) = main_weak.upgrade() {
                            refresh_main_ui(&main, &svc.state, Some(&id));
                        }
                    });
                }
            });
        });
    }
    {
        let cg_weak = cg_window.as_weak();
        cg_window.on_cancel_clicked(move || {
            if let Some(w) = cg_weak.upgrade() {
                let _ = w.hide();
            }
        });
    }
}

fn apply_grid_appearance(win: &MainWindow, cfg: &LauncherConfig) {
    let icon_px = match cfg.appearance.icon_size {
        InstanceIconSize::Small => 36.0,
        InstanceIconSize::Medium => 44.0,
        InstanceIconSize::Large => 54.0,
    };
    let gap_px = match cfg.appearance.grid_spacing {
        InstanceGridSpacing::Compact => 4.0,
        InstanceGridSpacing::Normal => 10.0,
        InstanceGridSpacing::Spacious => 18.0,
    };
    win.set_grid_icon_px(icon_px);
    win.set_grid_gap(gap_px);
    win.set_grid_show_name(cfg.appearance.show_instance_name);
    win.set_grid_show_version(cfg.appearance.show_mc_version);
    win.set_grid_show_modloader(cfg.appearance.show_modloader_badge);
}

fn load_instance_icon_image(
    icon_key: Option<&str>,
    icons_dir: Option<&std::path::Path>,
) -> (Image, bool) {
    if let (Some(key), Some(dir)) = (icon_key, icons_dir) {
        if !key.trim().is_empty()
            && !key.contains('/')
            && !key.contains('\\')
            && !key.contains("..")
        {
            let p = dir.join(key);
            if p.is_file() {
                if let Ok(img) = Image::load_from_path(&p) {
                    return (img, true);
                }
            }
        }
    }
    (Image::default(), false)
}

fn build_language_options() -> Vec<DropdownOption> {
    const SUPPORTED_LANGUAGES: &[(&str, &str)] =
        &[("en", "English"), ("pt-BR", "Português (Brasil)")];
    SUPPORTED_LANGUAGES
        .iter()
        .map(|(id, label)| DropdownOption {
            id: SharedString::from(*id),
            label: SharedString::from(*label),
            detail: SharedString::new(),
            enabled: true,
            icon: Image::default(),
            has_icon: false,
        })
        .collect()
}

fn language_display_label(lang_id: &str) -> &'static str {
    match lang_id {
        "pt-BR" => "Português (Brasil)",
        _ => "English",
    }
}

fn build_java_provider_options() -> Vec<DropdownOption> {
    vec![
        DropdownOption {
            id: "mojang".into(),
            label: "Mojang".into(),
            detail: "Official".into(),
            enabled: true,
            icon: Image::default(),
            has_icon: false,
        },
        DropdownOption {
            id: "adoptium".into(),
            label: "Eclipse Adoptium".into(),
            detail: "Temurin".into(),
            enabled: true,
            icon: Image::default(),
            has_icon: false,
        },
        DropdownOption {
            id: "azul".into(),
            label: "Azul Zulu".into(),
            detail: "OpenJDK".into(),
            enabled: true,
            icon: Image::default(),
            has_icon: false,
        },
        DropdownOption {
            id: "semeru".into(),
            label: "IBM Semeru".into(),
            detail: "OpenJ9".into(),
            enabled: true,
            icon: Image::default(),
            has_icon: false,
        },
    ]
}

fn build_resolution_preset_options(native: Option<(u32, u32)>) -> Vec<DropdownOption> {
    let mut opts = vec![
        DropdownOption {
            id: "854x480".into(),
            label: "854 × 480".into(),
            detail: "Default".into(),
            enabled: true,
            icon: Image::default(),
            has_icon: false,
        },
        DropdownOption {
            id: "1280x720".into(),
            label: "1280 × 720 — HD".into(),
            detail: "16:9".into(),
            enabled: true,
            icon: Image::default(),
            has_icon: false,
        },
        DropdownOption {
            id: "1600x900".into(),
            label: "1600 × 900 — HD+".into(),
            detail: "16:9".into(),
            enabled: true,
            icon: Image::default(),
            has_icon: false,
        },
        DropdownOption {
            id: "1920x1080".into(),
            label: "1920 × 1080 — Full HD".into(),
            detail: "16:9".into(),
            enabled: true,
            icon: Image::default(),
            has_icon: false,
        },
        DropdownOption {
            id: "2560x1440".into(),
            label: "2560 × 1440 — QHD".into(),
            detail: "16:9".into(),
            enabled: true,
            icon: Image::default(),
            has_icon: false,
        },
        DropdownOption {
            id: "3840x2160".into(),
            label: "3840 × 2160 — 4K UHD".into(),
            detail: "16:9".into(),
            enabled: true,
            icon: Image::default(),
            has_icon: false,
        },
    ];
    match native {
        Some((w, h)) => opts.push(DropdownOption {
            id: "native".into(),
            label: "Native Resolution".into(),
            detail: format!("{w}×{h}").into(),
            enabled: true,
            icon: Image::default(),
            has_icon: false,
        }),
        None => opts.push(DropdownOption {
            id: "native".into(),
            label: "Native Resolution".into(),
            detail: "Unavailable".into(),
            enabled: false,
            icon: Image::default(),
            has_icon: false,
        }),
    }
    opts.push(DropdownOption {
        id: "custom".into(),
        label: "Custom".into(),
        detail: String::new().into(),
        enabled: true,
        icon: Image::default(),
        has_icon: false,
    });
    opts
}

fn apply_resolution_preset(id: &str, native: Option<(u32, u32)>) -> Option<(u32, u32)> {
    match id {
        "854x480" => Some((854, 480)),
        "1280x720" => Some((1280, 720)),
        "1600x900" => Some((1600, 900)),
        "1920x1080" => Some((1920, 1080)),
        "2560x1440" => Some((2560, 1440)),
        "3840x2160" => Some((3840, 2160)),
        "native" => native,
        _ => None,
    }
}

fn resolution_preset_display(id: &str, native: Option<(u32, u32)>) -> (String, String) {
    match id {
        "854x480" => ("854x480".into(), "854 × 480".into()),
        "1280x720" => ("1280x720".into(), "1280 × 720 — HD".into()),
        "1600x900" => ("1600x900".into(), "1600 × 900 — HD+".into()),
        "1920x1080" => ("1920x1080".into(), "1920 × 1080 — Full HD".into()),
        "2560x1440" => ("2560x1440".into(), "2560 × 1440 — QHD".into()),
        "3840x2160" => ("3840x2160".into(), "3840 × 2160 — 4K UHD".into()),
        "native" => (
            "native".into(),
            native
                .map(|(w, h)| format!("Native Resolution ({w}×{h})"))
                .unwrap_or_else(|| "Native Resolution".into()),
        ),
        _ => ("custom".into(), "Custom".into()),
    }
}

fn match_resolution_preset(w: u32, h: u32, native: Option<(u32, u32)>) -> (String, String) {
    match (w, h) {
        (854, 480) => resolution_preset_display("854x480", native),
        (1280, 720) => resolution_preset_display("1280x720", native),
        (1600, 900) => resolution_preset_display("1600x900", native),
        (1920, 1080) => resolution_preset_display("1920x1080", native),
        (2560, 1440) => resolution_preset_display("2560x1440", native),
        (3840, 2160) => resolution_preset_display("3840x2160", native),
        _ if native == Some((w, h)) => resolution_preset_display("native", native),
        _ => ("custom".into(), "Custom".into()),
    }
}

fn build_memory_preset_options(
    total_mib: Option<u64>,
    avail_mib: Option<u64>,
) -> Vec<DropdownOption> {
    let presets: &[(u32, &str)] = &[
        (1024, "1 GiB — 1024 MiB"),
        (2048, "2 GiB — 2048 MiB"),
        (3072, "3 GiB — 3072 MiB"),
        (4096, "4 GiB — 4096 MiB"),
        (6144, "6 GiB — 6144 MiB"),
        (8192, "8 GiB — 8192 MiB"),
        (12288, "12 GiB — 12288 MiB"),
        (16384, "16 GiB — 16384 MiB"),
    ];
    let mut opts = Vec::new();
    for (mib, label) in presets {
        let exceeds_total = total_mib.map(|t| (*mib as u64) >= t).unwrap_or(false);
        let low_headroom = total_mib
            .map(|t| t.saturating_sub(*mib as u64) < 1024)
            .unwrap_or(false)
            || avail_mib.map(|a| (*mib as u64) > a).unwrap_or(false);
        let detail = if exceeds_total {
            "Exceeds RAM"
        } else if low_headroom {
            "Low OS headroom"
        } else {
            ""
        };
        opts.push(DropdownOption {
            id: mib.to_string().into(),
            label: (*label).into(),
            detail: detail.into(),
            enabled: !exceeds_total,
            icon: Image::default(),
            has_icon: false,
        });
    }
    opts.push(DropdownOption {
        id: "custom".into(),
        label: "Custom".into(),
        detail: String::new().into(),
        enabled: true,
        icon: Image::default(),
        has_icon: false,
    });
    opts
}

fn match_memory_preset(max_mb: u32) -> (String, String) {
    match max_mb {
        1024 => ("1024".into(), "1 GiB — 1024 MiB".into()),
        2048 => ("2048".into(), "2 GiB — 2048 MiB".into()),
        3072 => ("3072".into(), "3 GiB — 3072 MiB".into()),
        4096 => ("4096".into(), "4 GiB — 4096 MiB".into()),
        6144 => ("6144".into(), "6 GiB — 6144 MiB".into()),
        8192 => ("8192".into(), "8 GiB — 8192 MiB".into()),
        12288 => ("12288".into(), "12 GiB — 12288 MiB".into()),
        16384 => ("16384".into(), "16 GiB — 16384 MiB".into()),
        _ => ("custom".into(), "Custom".into()),
    }
}

fn memory_warning_text(max_mb: u32, total_mib: Option<u64>, avail_mib: Option<u64>) -> String {
    if let Some(total) = total_mib {
        if max_mb as u64 >= total {
            return format!(
                "Warning: {max_mb} MiB exceeds or equals total physical RAM ({total} MiB)."
            );
        }
        if total.saturating_sub(max_mb as u64) < 1024 {
            return format!(
                "Warning: Allocating {max_mb} MiB leaves less than 1 GiB for the operating system."
            );
        }
    }
    if let Some(avail) = avail_mib {
        if max_mb as u64 > avail {
            return format!("Note: {max_mb} MiB exceeds currently available RAM ({avail} MiB).");
        }
    }
    String::new()
}

fn build_jvm_preset_options(
    presets: &[JvmPreset],
    java_major: u32,
    arch: &str,
) -> Vec<DropdownOption> {
    let mut opts: Vec<DropdownOption> = presets
        .iter()
        .map(|p| {
            let compat = p.check_compatibility(java_major, arch);
            let detail = match &compat {
                Ok(()) => {
                    if p.is_official {
                        "Official".to_string()
                    } else {
                        "Custom".to_string()
                    }
                }
                Err(_) => format!("Requires Java {}+", p.min_java_major),
            };
            DropdownOption {
                id: SharedString::from(&p.id),
                label: SharedString::from(&p.name),
                detail: SharedString::from(detail),
                enabled: compat.is_ok(),
                icon: Image::default(),
                has_icon: false,
            }
        })
        .collect();
    opts.push(DropdownOption {
        id: "custom".into(),
        label: "Custom".into(),
        detail: "Editable".into(),
        enabled: true,
        icon: Image::default(),
        has_icon: false,
    });
    opts
}

fn populate_manage_presets_window(
    win: &ManageJvmPresetsWindow,
    svc: &Services,
    select_id: Option<&str>,
) {
    let presets = discover_jvm_presets(&svc.paths.jvm_presets_dir());
    if let Ok(mut g) = svc.state.lock() {
        g.jvm_presets = presets.clone();
    }
    let items: Vec<JvmPresetListItem> = presets
        .iter()
        .map(|p| JvmPresetListItem {
            id: SharedString::from(&p.id),
            name: SharedString::from(&p.name),
            description: SharedString::from(&p.description),
            args: SharedString::from(p.args_string()),
            min_java: p.min_java_major as i32,
            compatibility: SharedString::from(p.compatibility_summary()),
            is_official: p.is_official,
        })
        .collect();
    win.set_presets(ModelRc::from(Rc::new(VecModel::from(items))));
    let target_id = select_id
        .map(str::to_string)
        .or_else(|| presets.first().map(|p| p.id.clone()))
        .unwrap_or_default();
    if let Some(p) = presets.iter().find(|p| p.id == target_id) {
        win.set_selected_id(SharedString::from(&p.id));
        win.set_selected_is_official(p.is_official);
        win.set_edit_name(SharedString::from(&p.name));
        win.set_edit_description(SharedString::from(&p.description));
        win.set_edit_args(SharedString::from(p.args_string()));
        win.set_edit_min_java(SharedString::from(p.min_java_major.to_string()));
    }
    win.set_error_message(SharedString::new());
}

fn wire_manage_jvm_presets_callbacks(
    settings_window: &Rc<SettingsWindow>,
    edit_window: &Rc<EditInstanceWindow>,
    mp_window: &Rc<ManageJvmPresetsWindow>,
    svc: &Services,
) {
    {
        let mp_weak = mp_window.as_weak();
        let state = svc.state.clone();
        mp_window.on_select_preset(move |id| {
            let pid = id.to_string();
            let found = state
                .lock()
                .ok()
                .and_then(|g| g.jvm_presets.iter().find(|p| p.id == pid).cloned());
            if let (Some(w), Some(p)) = (mp_weak.upgrade(), found) {
                w.set_selected_is_official(p.is_official);
                w.set_edit_name(SharedString::from(&p.name));
                w.set_edit_description(SharedString::from(&p.description));
                w.set_edit_args(SharedString::from(p.args_string()));
                w.set_edit_min_java(SharedString::from(p.min_java_major.to_string()));
                w.set_error_message(SharedString::new());
            }
        });
    }
    {
        let mp_weak = mp_window.as_weak();
        let svc = svc.clone();
        mp_window.on_create_preset_clicked(move || {
            let dir = svc.paths.jvm_presets_dir();
            if let Some(w) = mp_weak.upgrade() {
                match create_custom_preset(&dir, "New Custom Preset", "Custom JVM arguments", "", 8)
                {
                    Ok(created) => {
                        populate_manage_presets_window(&w, &svc, Some(&created.id));
                        w.set_status_message(SharedString::from("Created custom preset."));
                    }
                    Err(err) => w.set_error_message(SharedString::from(err.to_string())),
                }
            }
        });
    }
    {
        let mp_weak = mp_window.as_weak();
        let sw_weak = settings_window.as_weak();
        let ew_weak = edit_window.as_weak();
        let svc = svc.clone();
        mp_window.on_save_preset_clicked(move |id, name, desc, args, min_java_s| {
            let Some(w) = mp_weak.upgrade() else { return };
            let min_java: u32 = match min_java_s.trim().parse() {
                Ok(v) => v,
                Err(_) => {
                    w.set_error_message(SharedString::from("Invalid minimum Java version."));
                    return;
                }
            };
            let preset = JvmPreset {
                schema_version: 1,
                id: id.to_string(),
                name: name.trim().to_string(),
                description: desc.trim().to_string(),
                args: args.split_whitespace().map(str::to_string).collect(),
                min_java_major: min_java,
                max_java_major: None,
                supported_arches: Vec::new(),
                source: "Custom".to_string(),
                version: "1.0.0".to_string(),
                is_official: false,
            };
            match save_custom_preset(&svc.paths.jvm_presets_dir(), &preset) {
                Ok(_) => {
                    populate_manage_presets_window(&w, &svc, Some(&preset.id));
                    w.set_status_message(SharedString::from("Saved preset."));
                    if let Some(sw) = sw_weak.upgrade() {
                        populate_settings_window(&sw, &svc.state);
                    }
                    if let Some(ew) = ew_weak.upgrade() {
                        let iid = ew.get_instance_id().to_string();
                        if !iid.is_empty() {
                            populate_edit_instance_window(&ew, &svc, &iid);
                        }
                    }
                }
                Err(err) => w.set_error_message(SharedString::from(err.to_string())),
            }
        });
    }
    {
        let mp_weak = mp_window.as_weak();
        let svc = svc.clone();
        mp_window.on_duplicate_preset_clicked(move |id| {
            let Some(w) = mp_weak.upgrade() else { return };
            let src = svc
                .state
                .lock()
                .ok()
                .and_then(|g| g.jvm_presets.iter().find(|p| p.id == id.as_str()).cloned());
            if let Some(s) = src {
                match duplicate_preset(&svc.paths.jvm_presets_dir(), &s) {
                    Ok(dup) => {
                        populate_manage_presets_window(&w, &svc, Some(&dup.id));
                        w.set_status_message(SharedString::from("Duplicated preset."));
                    }
                    Err(err) => w.set_error_message(SharedString::from(err.to_string())),
                }
            }
        });
    }
    {
        let mp_weak = mp_window.as_weak();
        let svc = svc.clone();
        mp_window.on_delete_preset_clicked(move |id| {
            let Some(w) = mp_weak.upgrade() else { return };
            match delete_custom_preset(&svc.paths.jvm_presets_dir(), &id) {
                Ok(()) => {
                    populate_manage_presets_window(&w, &svc, None);
                    w.set_status_message(SharedString::from("Deleted custom preset."));
                }
                Err(err) => w.set_error_message(SharedString::from(err.to_string())),
            }
        });
    }
    {
        let mp_weak = mp_window.as_weak();
        let svc = svc.clone();
        mp_window.on_import_preset_clicked(move || {
            let mp_weak = mp_weak.clone();
            let svc = svc.clone();
            let rt = svc.rt.clone();
            rt.spawn_blocking(move || match pick_toml_file() {
                Ok(Some(path)) => {
                    let res = import_custom_preset(&svc.paths.jvm_presets_dir(), &path);
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(w) = mp_weak.upgrade() {
                            match res {
                                Ok(imported) => {
                                    populate_manage_presets_window(&w, &svc, Some(&imported.id));
                                    w.set_status_message(SharedString::from(
                                        "Imported JVM preset.",
                                    ));
                                }
                                Err(err) => {
                                    w.set_error_message(SharedString::from(err.to_string()));
                                }
                            }
                        }
                    });
                }
                Ok(None) => {}
                Err(err) => {
                    let msg = err.to_string();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(w) = mp_weak.upgrade() {
                            w.set_error_message(SharedString::from(msg));
                        }
                    });
                }
            });
        });
    }
    {
        let mp_weak = mp_window.as_weak();
        let svc = svc.clone();
        mp_window.on_export_preset_clicked(move |id| {
            let found = svc
                .state
                .lock()
                .ok()
                .and_then(|g| g.jvm_presets.iter().find(|p| p.id == id.as_str()).cloned());
            let Some(preset) = found else { return };
            let mp_weak = mp_weak.clone();
            svc.rt
                .spawn_blocking(move || match save_toml_file(&preset.id) {
                    Ok(Some(dest)) => {
                        let res = export_preset(&preset, &dest);
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(w) = mp_weak.upgrade() {
                                match res {
                                    Ok(()) => w.set_status_message(SharedString::from(
                                        "Exported JVM preset.",
                                    )),
                                    Err(err) => {
                                        w.set_error_message(SharedString::from(err.to_string()));
                                    }
                                }
                            }
                        });
                    }
                    Ok(None) => {}
                    Err(err) => {
                        let msg = err.to_string();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(w) = mp_weak.upgrade() {
                                w.set_error_message(SharedString::from(msg));
                            }
                        });
                    }
                });
        });
    }
    {
        let mp_weak = mp_window.as_weak();
        mp_window.on_close_clicked(move || {
            if let Some(w) = mp_weak.upgrade() {
                let _ = w.hide();
            }
        });
    }
}

fn populate_select_icon_window(win: &SelectIconWindow, svc: &Services, instance_id: &str) -> bool {
    let inst_info = svc.state.lock().ok().and_then(|g| {
        g.instances
            .iter()
            .find(|i| i.id == instance_id)
            .map(|i| (i.name.clone(), i.icon_key.clone().unwrap_or_default()))
    });
    let Some((inst_name, current_key)) = inst_info else {
        return false;
    };
    populate_select_icon_window_for_key(win, svc, instance_id, &inst_name, &current_key);
    true
}

fn populate_select_icon_window_for_key(
    win: &SelectIconWindow,
    svc: &Services,
    instance_id: &str,
    inst_name: &str,
    current_key: &str,
) {
    let query = win.get_search_query().to_string().to_lowercase();
    let mut items = Vec::new();
    if query.is_empty() || "default".contains(&query) || "cube".contains(&query) {
        items.push(SelectableIconItem {
            key: SharedString::new(),
            label: SharedString::from("Default"),
            preview: Image::default(),
            is_default: true,
        });
    }

    for (fname, display_name, path) in svc.instance_manager.list_managed_icons_with_names() {
        if !query.is_empty()
            && !display_name.to_lowercase().contains(&query)
            && !fname.to_lowercase().contains(&query)
        {
            continue;
        }
        if let Ok(img) = Image::load_from_path(&path) {
            items.push(SelectableIconItem {
                key: SharedString::from(&fname),
                label: SharedString::from(&display_name),
                preview: img,
                is_default: false,
            });
        }
    }

    win.set_instance_id(SharedString::from(instance_id));
    win.set_instance_name(SharedString::from(inst_name));
    win.set_selected_key(SharedString::from(current_key));
    win.set_icons(ModelRc::from(Rc::new(VecModel::from(items))));
    win.set_error_message(SharedString::new());
}

fn modloader_kind_from_add_idx(idx: i32) -> Option<ModloaderKind> {
    match idx {
        1 => Some(ModloaderKind::Fabric),
        2 => Some(ModloaderKind::Forge),
        3 => Some(ModloaderKind::NeoForge),
        4 => Some(ModloaderKind::Quilt),
        5 => Some(ModloaderKind::LiteLoader),
        _ => None,
    }
}

fn trigger_add_loader_fetch(
    add_weak: &Weak<AddInstanceWindow>,
    svc: &Services,
    kind: ModloaderKind,
    mc_version: String,
) {
    // Verifica cache em memória antes de consultar a rede
    let cached = svc
        .state
        .lock()
        .ok()
        .and_then(|g| g.add_loader_cache.get(&(kind, mc_version.clone())).cloned());
    if let Some(list) = cached {
        if let Some(w) = add_weak.upgrade() {
            apply_add_loader_versions(&w, kind, &mc_version, &list);
        }
        return;
    }

    if let Some(w) = add_weak.upgrade() {
        w.set_loader_loading(true);
        w.set_selected_loader_version(SharedString::new());
        w.set_loader_status_text(SharedString::from(format!(
            "Loading {} versions for {}...",
            kind.short_name(),
            mc_version
        )));
        w.set_loader_versions(ModelRc::from(Rc::new(VecModel::from(Vec::new()))));
    }

    let add_weak = add_weak.clone();
    let svc = svc.clone();
    let rt = svc.rt.clone();
    rt.spawn(async move {
        let res = fetch_loader_versions(&svc.http_client, kind, &mc_version).await;
        match res {
            Ok(list) => {
                if let Ok(mut g) = svc.state.lock() {
                    g.add_loader_cache
                        .insert((kind, mc_version.clone()), list.clone());
                }
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(w) = add_weak.upgrade() {
                        if modloader_kind_from_add_idx(w.get_selected_modloader()) == Some(kind)
                            && w.get_selected_version_id().as_str() == mc_version
                        {
                            apply_add_loader_versions(&w, kind, &mc_version, &list);
                        }
                    }
                });
            }
            Err(err) => {
                let msg = format!("Failed to load {} versions: {err}", kind.short_name());
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(w) = add_weak.upgrade() {
                        w.set_loader_loading(false);
                        w.set_selected_loader_version(SharedString::new());
                        w.set_loader_status_text(SharedString::from(msg));
                    }
                });
            }
        }
    });
}

fn apply_add_loader_versions(
    win: &AddInstanceWindow,
    kind: ModloaderKind,
    mc_version: &str,
    list: &[AvailableLoaderVersion],
) {
    win.set_loader_loading(false);
    if list.is_empty() {
        win.set_loader_versions(ModelRc::from(Rc::new(VecModel::from(Vec::new()))));
        win.set_selected_loader_version(SharedString::new());
        win.set_loader_status_text(SharedString::from(format!(
            "{} is not available for Minecraft {mc_version}.",
            kind.short_name()
        )));
        return;
    }

    let query = win.get_loader_search_query().to_string().to_lowercase();
    let items: Vec<AddLoaderVersionItem> = list
        .iter()
        .filter(|v| query.is_empty() || v.version.to_lowercase().contains(&query))
        .map(|v| {
            let tag = if v.recommended {
                "Recommended"
            } else if v.stable {
                "Stable"
            } else {
                "Experimental"
            };
            AddLoaderVersionItem {
                version: SharedString::from(&v.version),
                status_label: SharedString::from(tag),
                recommended: v.recommended,
                stable: v.stable,
            }
        })
        .collect();

    let current = win.get_selected_loader_version().to_string();
    let next_ver = if items.iter().any(|i| i.version.as_str() == current) {
        current
    } else {
        items
            .iter()
            .find(|i| i.recommended)
            .or_else(|| items.first())
            .map(|i| i.version.to_string())
            .unwrap_or_default()
    };

    win.set_loader_versions(ModelRc::from(Rc::new(VecModel::from(items))));
    win.set_selected_loader_version(SharedString::from(&next_ver));
    win.set_loader_status_text(SharedString::new());
}

fn wire_select_icon_callbacks(
    main_window: &MainWindow,
    add_window: &Rc<AddInstanceWindow>,
    si_window: &Rc<SelectIconWindow>,
    svc: &Services,
) {
    {
        let si_weak = si_window.as_weak();
        let svc = svc.clone();
        si_window.on_search_changed(move |_| {
            if let Some(w) = si_weak.upgrade() {
                let id = w.get_instance_id().to_string();
                populate_select_icon_window(&w, &svc, &id);
            }
        });
    }
    {
        let main_weak = main_window.as_weak();
        let si_weak = si_window.as_weak();
        let svc = svc.clone();
        si_window.on_import_icon_clicked(move || {
            let Some(w) = si_weak.upgrade() else { return };
            let inst_id = w.get_instance_id().to_string();
            let main_weak = main_weak.clone();
            let si_weak = si_weak.clone();
            let svc = svc.clone();
            let rt = svc.rt.clone();
            rt.spawn(async move {
                let picked = tokio::task::spawn_blocking(pick_icon_file).await;
                match picked {
                    Ok(Ok(Some(path))) => {
                        match svc
                            .instance_manager
                            .import_instance_icon(&inst_id, &path)
                            .await
                        {
                            Ok(updated) => {
                                if let Ok(mut g) = svc.state.lock() {
                                    if let Some(item) =
                                        g.instances.iter_mut().find(|i| i.id == inst_id)
                                    {
                                        *item = updated;
                                    }
                                }
                                let _ = slint::invoke_from_event_loop(move || {
                                    if let Some(sw) = si_weak.upgrade() {
                                        populate_select_icon_window(&sw, &svc, &inst_id);
                                    }
                                    if let Some(main) = main_weak.upgrade() {
                                        refresh_main_ui(&main, &svc.state, Some(&inst_id));
                                    }
                                });
                            }
                            Err(err) => {
                                let msg = err.to_string();
                                let _ = slint::invoke_from_event_loop(move || {
                                    if let Some(sw) = si_weak.upgrade() {
                                        sw.set_error_message(SharedString::from(msg));
                                    }
                                });
                            }
                        }
                    }
                    Ok(Ok(None)) => {}
                    Ok(Err(err)) => {
                        let msg = err.to_string();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(sw) = si_weak.upgrade() {
                                sw.set_error_message(SharedString::from(msg));
                            }
                        });
                    }
                    Err(_) => {}
                }
            });
        });
    }
    {
        let svc = svc.clone();
        si_window.on_open_icons_folder_clicked(move || {
            let dir = svc.paths.icons_dir();
            svc.rt.spawn_blocking(move || {
                let _ = std::fs::create_dir_all(&dir);
                let _ = open_in_file_manager(&dir);
            });
        });
    }
    {
        let main_weak = main_window.as_weak();
        let si_weak = si_window.as_weak();
        let svc = svc.clone();
        si_window.on_reset_default_clicked(move || {
            if let Some(w) = si_weak.upgrade() {
                let id = w.get_instance_id().to_string();
                let main_weak = main_weak.clone();
                let si_weak = si_weak.clone();
                let svc = svc.clone();
                svc.rt.spawn(async move {
                    if let Ok(updated) = svc.instance_manager.set_instance_icon_key(&id, None).await
                    {
                        if let Ok(mut g) = svc.state.lock() {
                            if let Some(item) = g.instances.iter_mut().find(|i| i.id == id) {
                                *item = updated;
                            }
                        }
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(sw) = si_weak.upgrade() {
                                let _ = sw.hide();
                            }
                            if let Some(main) = main_weak.upgrade() {
                                refresh_main_ui(&main, &svc.state, Some(&id));
                            }
                        });
                    }
                });
            }
        });
    }
    {
        let main_weak = main_window.as_weak();
        let add_weak = add_window.as_weak();
        let si_weak = si_window.as_weak();
        let svc = svc.clone();
        si_window.on_apply_clicked(move |key| {
            let Some(w) = si_weak.upgrade() else { return };
            let for_add = svc
                .state
                .lock()
                .map(|g| g.select_icon_for_add)
                .unwrap_or(false);
            if for_add {
                if let Some(aw) = add_weak.upgrade() {
                    let k = key.to_string();
                    let (img, has) = load_instance_icon_image(
                        if k.is_empty() { None } else { Some(k.as_str()) },
                        Some(&svc.paths.icons_dir()),
                    );
                    aw.set_selected_icon_key(SharedString::from(k));
                    aw.set_selected_icon_preview(img);
                    aw.set_has_selected_icon(has);
                }
                let _ = w.hide();
                return;
            }

            let id = w.get_instance_id().to_string();
            let icon_key = if key.is_empty() {
                None
            } else {
                Some(key.to_string())
            };
            let main_weak = main_weak.clone();
            let si_weak = si_weak.clone();
            let svc = svc.clone();
            let rt = svc.rt.clone();
            rt.spawn(async move {
                if let Ok(updated) = svc
                    .instance_manager
                    .set_instance_icon_key(&id, icon_key)
                    .await
                {
                    if let Ok(mut g) = svc.state.lock() {
                        if let Some(item) = g.instances.iter_mut().find(|i| i.id == id) {
                            *item = updated;
                        }
                    }
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(sw) = si_weak.upgrade() {
                            let _ = sw.hide();
                        }
                        if let Some(main) = main_weak.upgrade() {
                            refresh_main_ui(&main, &svc.state, Some(&id));
                        }
                    });
                }
            });
        });
    }
    // Request Delete Icon (verifica uso por instâncias e abre diálogo de confirmação)
    {
        let si_weak = si_window.as_weak();
        let svc = svc.clone();
        si_window.on_request_delete_icon(move |key, label| {
            let k = key.to_string();
            if k.is_empty() {
                return;
            }
            let display = svc
                .instance_manager
                .list_managed_icons_with_names()
                .into_iter()
                .find(|(fk, _, _)| fk == &k)
                .map(|(_, d, _)| d)
                .unwrap_or_else(|| label.to_string());
            let usage = svc
                .state
                .lock()
                .map(|g| {
                    g.instances
                        .iter()
                        .filter(|i| i.icon_key.as_deref() == Some(k.as_str()))
                        .count()
                })
                .unwrap_or(0);
            if let Some(w) = si_weak.upgrade() {
                w.set_delete_target_key(SharedString::from(k));
                w.set_delete_target_label(SharedString::from(display));
                w.set_delete_usage_count(usage as i32);
                w.set_show_delete_dialog(true);
            }
        });
    }

    // Confirm Delete Icon (substitui por padrão em todas as instâncias e remove arquivo gerenciado)
    {
        let main_weak = main_window.as_weak();
        let si_weak = si_window.as_weak();
        let svc = svc.clone();
        si_window.on_confirm_delete_icon(move |key| {
            let k = key.to_string();
            let Some(w) = si_weak.upgrade() else { return };
            let inst_id = w.get_instance_id().to_string();
            let main_weak = main_weak.clone();
            let si_weak = si_weak.clone();
            let svc = svc.clone();
            let rt = svc.rt.clone();
            rt.spawn(async move {
                match svc.instance_manager.delete_managed_icon(&k).await {
                    Ok(_) => {
                        if let Ok(list) = svc.instance_manager.list_instances().await {
                            if let Ok(mut g) = svc.state.lock() {
                                g.instances = list;
                            }
                        }
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(sw) = si_weak.upgrade() {
                                if sw.get_selected_key().as_str() == k {
                                    sw.set_selected_key(SharedString::new());
                                }
                                populate_select_icon_window(&sw, &svc, &inst_id);
                            }
                            if let Some(main) = main_weak.upgrade() {
                                refresh_main_ui(&main, &svc.state, Some(&inst_id));
                            }
                        });
                    }
                    Err(err) => {
                        let msg = err.to_string();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(sw) = si_weak.upgrade() {
                                sw.set_error_message(SharedString::from(msg));
                            }
                        });
                    }
                }
            });
        });
    }

    // Rename Icon (mantém identificador interno estável, atualiza nome legível)
    {
        let si_weak = si_window.as_weak();
        let svc = svc.clone();
        si_window.on_rename_icon_confirmed(move |key, new_name| {
            let Some(w) = si_weak.upgrade() else { return };
            let inst_id = w.get_instance_id().to_string();
            match svc.instance_manager.rename_managed_icon(&key, &new_name) {
                Ok(()) => {
                    populate_select_icon_window(&w, &svc, &inst_id);
                }
                Err(err) => {
                    w.set_error_message(SharedString::from(err.to_string()));
                }
            }
        });
    }

    {
        let si_weak = si_window.as_weak();
        si_window.on_cancel_clicked(move || {
            if let Some(w) = si_weak.upgrade() {
                let _ = w.hide();
            }
        });
    }
}

fn find_instance_by_id(svc: &Services, id: &str) -> Option<(InstanceMetadata, bool)> {
    svc.state.lock().ok().and_then(|g| {
        let inst = g.instances.iter().find(|i| i.id == id)?.clone();
        let running = g.running_instances.contains(id);
        Some((inst, running))
    })
}

fn decode_base64_bytes(input: &str) -> Option<Vec<u8>> {
    let mut buf = Vec::with_capacity(input.len() * 3 / 4);
    let mut val: u32 = 0;
    let mut bits: i32 = -8;
    for b in input.bytes() {
        if b.is_ascii_whitespace() {
            continue;
        }
        if b == b'=' {
            break;
        }
        let d = match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        };
        val = (val << 6) | (d as u32);
        bits += 6;
        if bits >= 0 {
            buf.push(((val >> bits) & 0xFF) as u8);
            bits -= 8;
        }
    }
    Some(buf)
}

fn load_pack_icon(
    cache_dir: &std::path::Path,
    rec: Option<&crab_launcher::core::mods::platform::InstalledModRecord>,
    icon_png: Option<&[u8]>,
) -> (Image, bool) {
    if let Some(r) = rec {
        let icon_file = cache_dir.join("mod-icons").join(format!(
            "{}-{}.png",
            r.provider.label().to_lowercase(),
            r.project_id
        ));
        if icon_file.is_file() {
            if let Ok(img) = Image::load_from_path(&icon_file) {
                return (img, true);
            }
        }
    }
    if let Some(bytes) = icon_png {
        if let Ok(dyn_img) = image::load_from_memory(bytes) {
            let rgba = dyn_img.to_rgba8();
            let (w, h) = rgba.dimensions();
            let buf = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(
                rgba.as_raw(),
                w,
                h,
            );
            return (Image::from_rgba8(buf), true);
        }
    }
    (Image::default(), false)
}

fn refresh_edit_resources_ui(win: &EditInstanceWindow, svc: &Services, inst: &InstanceMetadata) {
    let mc_dir = svc.paths.instance_game_dir(&inst.dir_name);

    // Texture Packs / Resource Packs
    let tp_q = win.get_texture_pack_search().to_string().to_lowercase();
    let tp_dir = texture_packs_dir(&mc_dir, &inst.minecraft_version);
    let tp_index = ModIndex::load(&tp_dir);
    let tp_items: Vec<PackItem> = list_packs(&tp_dir)
        .into_iter()
        .filter(|p| {
            let rec = tp_index.find_by_file(&p.file_name);
            let name = rec
                .map(|r| r.title.as_str())
                .filter(|t| !t.is_empty())
                .unwrap_or(p.display_name.as_str());
            tp_q.is_empty()
                || name.to_lowercase().contains(&tp_q)
                || p.file_name.to_lowercase().contains(&tp_q)
        })
        .map(|p| {
            let rec = tp_index.find_by_file(&p.file_name);
            let display_name = rec
                .map(|r| r.title.as_str())
                .filter(|t| !t.is_empty())
                .unwrap_or(p.display_name.as_str())
                .to_string();
            let version_or_format = match (
                rec.map(|r| r.version_number.as_str())
                    .filter(|v| !v.is_empty()),
                p.pack_format_display.as_str(),
            ) {
                (Some(v), "") => v.to_string(),
                (Some(v), fmt) => format!("{v} ({fmt})"),
                (None, fmt) if !fmt.is_empty() => fmt.to_string(),
                _ => "—".to_string(),
            };
            let provider = rec.map(|r| r.provider.label()).unwrap_or("Local");
            let homepage_url = rec.map(|r| r.homepage_url.as_str()).unwrap_or("");
            let author = rec.map(|r| r.author.as_str()).unwrap_or("");
            let description = if !p.description.is_empty() {
                p.description.as_str()
            } else {
                rec.map(|r| r.description.as_str()).unwrap_or("")
            };
            let (icon, has_icon) = load_pack_icon(&svc.paths.cache_dir, rec, p.icon_png.as_deref());
            let (incompatible, incompatibility_reason) = match check_resource_pack_compatibility(
                p.pack_format,
                p.supported_formats,
                &inst.minecraft_version,
            ) {
                PackCompatibility::Incompatible { reason } => (true, reason),
                PackCompatibility::Compatible | PackCompatibility::Unknown => {
                    (false, String::new())
                }
            };
            PackItem {
                file_name: SharedString::from(&p.file_name),
                display_name: SharedString::from(display_name),
                version_or_format: SharedString::from(version_or_format),
                last_modified: SharedString::from(&p.last_modified),
                provider: SharedString::from(provider),
                homepage_url: SharedString::from(homepage_url),
                author: SharedString::from(author),
                description: SharedString::from(description),
                size_display: SharedString::from(format_bytes(p.size_bytes)),
                icon,
                has_icon,
                selected: false,
                incompatible,
                incompatibility_reason: SharedString::from(incompatibility_reason),
            }
        })
        .collect();
    win.set_texture_packs(ModelRc::from(Rc::new(VecModel::from(tp_items))));

    // Shader Packs
    let sp_q = win.get_shader_pack_search().to_string().to_lowercase();
    let sp_dir = shader_packs_dir(&mc_dir);
    let sp_index = ModIndex::load(&sp_dir);
    let sp_items: Vec<PackItem> = list_packs(&sp_dir)
        .into_iter()
        .filter(|p| {
            let rec = sp_index.find_by_file(&p.file_name);
            let name = rec
                .map(|r| r.title.as_str())
                .filter(|t| !t.is_empty())
                .unwrap_or(p.display_name.as_str());
            sp_q.is_empty()
                || name.to_lowercase().contains(&sp_q)
                || p.file_name.to_lowercase().contains(&sp_q)
        })
        .map(|p| {
            let rec = sp_index.find_by_file(&p.file_name);
            let display_name = rec
                .map(|r| r.title.as_str())
                .filter(|t| !t.is_empty())
                .unwrap_or(p.display_name.as_str())
                .to_string();
            let version_or_format = rec
                .map(|r| r.version_number.as_str())
                .filter(|v| !v.is_empty())
                .unwrap_or("—");
            let provider = rec.map(|r| r.provider.label()).unwrap_or("Local");
            let homepage_url = rec.map(|r| r.homepage_url.as_str()).unwrap_or("");
            let author = rec.map(|r| r.author.as_str()).unwrap_or("");
            let description = rec
                .map(|r| r.description.as_str())
                .filter(|d| !d.is_empty())
                .unwrap_or(p.description.as_str());
            let (icon, has_icon) = load_pack_icon(&svc.paths.cache_dir, rec, p.icon_png.as_deref());
            PackItem {
                file_name: SharedString::from(&p.file_name),
                display_name: SharedString::from(display_name),
                version_or_format: SharedString::from(version_or_format),
                last_modified: SharedString::from(&p.last_modified),
                provider: SharedString::from(provider),
                homepage_url: SharedString::from(homepage_url),
                author: SharedString::from(author),
                description: SharedString::from(description),
                size_display: SharedString::from(format_bytes(p.size_bytes)),
                icon,
                has_icon,
                selected: false,
                incompatible: false,
                incompatibility_reason: SharedString::new(),
            }
        })
        .collect();
    win.set_has_shader_mod(has_known_shader_mod(&mc_dir));
    win.set_shader_packs(ModelRc::from(Rc::new(VecModel::from(sp_items))));

    // Worlds (com ícone real icon.png e última modificação)
    let w_q = win.get_world_search().to_string().to_lowercase();
    let w_items: Vec<WorldItem> = list_worlds(&mc_dir)
        .into_iter()
        .filter(|w| w_q.is_empty() || w.display_name.to_lowercase().contains(&w_q))
        .map(|w| {
            let icon_path = w.path.join("icon.png");
            let (icon, has_icon) = if icon_path.is_file() {
                match Image::load_from_path(&icon_path) {
                    Ok(img) => (img, true),
                    Err(_) => (Image::default(), false),
                }
            } else {
                (Image::default(), false)
            };
            let last_modified = if w.last_played_ms > 0 {
                chrono::DateTime::from_timestamp_millis(w.last_played_ms)
                    .map(|dt| {
                        dt.with_timezone(&chrono::Local)
                            .format("%Y-%m-%d %H:%M")
                            .to_string()
                    })
                    .unwrap_or_default()
            } else {
                std::fs::metadata(w.path.join("level.dat"))
                    .and_then(|m| m.modified())
                    .ok()
                    .map(|st| {
                        let dt: chrono::DateTime<chrono::Local> = st.into();
                        dt.format("%Y-%m-%d %H:%M").to_string()
                    })
                    .unwrap_or_default()
            };
            WorldItem {
                folder_name: SharedString::from(&w.folder_name),
                display_name: SharedString::from(&w.display_name),
                mc_version: SharedString::from(&w.mc_version),
                game_mode: SharedString::from(&w.game_mode),
                last_modified: SharedString::from(last_modified),
                size_display: SharedString::from(format_bytes(w.size_bytes)),
                icon,
                has_icon,
            }
        })
        .collect();
    win.set_worlds(ModelRc::from(Rc::new(VecModel::from(w_items))));

    // Servers (servers.dat NBT com ícone real decodificado de base64)
    let srv_items: Vec<ServerItem> = load_servers(&mc_dir)
        .unwrap_or_default()
        .into_iter()
        .enumerate()
        .map(|(idx, s)| {
            let (icon, has_icon) = s
                .icon_base64
                .as_deref()
                .and_then(decode_base64_bytes)
                .and_then(|bytes| image::load_from_memory(&bytes).ok())
                .map(|dyn_img| {
                    let rgba = dyn_img.to_rgba8();
                    let (w, h) = rgba.dimensions();
                    let buf = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(
                        rgba.as_raw(),
                        w,
                        h,
                    );
                    (Image::from_rgba8(buf), true)
                })
                .unwrap_or_else(|| (Image::default(), false));
            ServerItem {
                index: idx as i32,
                name: SharedString::from(&s.name),
                ip: SharedString::from(&s.ip),
                icon,
                has_icon,
            }
        })
        .collect();
    win.set_servers(ModelRc::from(Rc::new(VecModel::from(srv_items))));
}

fn pick_zip_dialog(title: &str, save_mode: bool) -> Option<PathBuf> {
    let mut cmd = std::process::Command::new("zenity");
    cmd.arg("--file-selection").arg("--title").arg(title);
    if save_mode {
        cmd.arg("--save").arg("--confirm-overwrite");
    }
    cmd.arg("--file-filter=ZIP archives (*.zip) | *.zip");
    let out = cmd.output().ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(PathBuf::from(s))
    }
}

fn wire_edit_resources_callbacks(edit_window: &Rc<EditInstanceWindow>, svc: &Services) {
    // Refresh helpers
    for kind in 0..4 {
        let ew = edit_window.as_weak();
        let svc = svc.clone();
        let handler = move || {
            let Some(w) = ew.upgrade() else { return };
            let id = w.get_instance_id().to_string();
            if let Some((inst, _)) = find_instance_by_id(&svc, &id) {
                refresh_edit_resources_ui(&w, &svc, &inst);
            }
        };
        match kind {
            0 => edit_window.on_refresh_texture_packs_clicked(handler),
            1 => edit_window.on_refresh_shader_packs_clicked(handler),
            2 => edit_window.on_refresh_worlds_clicked(handler),
            _ => edit_window.on_refresh_servers_clicked(handler),
        }
    }

    // Texture Packs: Import / Remove / Open Folder
    {
        let ew = edit_window.as_weak();
        let svc = svc.clone();
        edit_window.on_import_texture_pack_clicked(move || {
            let Some(w) = ew.upgrade() else { return };
            let id = w.get_instance_id().to_string();
            let Some((inst, _)) = find_instance_by_id(&svc, &id) else {
                return;
            };
            let dest = texture_packs_dir(
                &svc.paths.instance_game_dir(&inst.dir_name),
                &inst.minecraft_version,
            );
            let ew = ew.clone();
            let svc = svc.clone();
            let rt = svc.rt.clone();
            rt.spawn(async move {
                let res = tokio::task::spawn_blocking(move || {
                    let Some(src) = pick_zip_dialog("Import Texture Pack", false) else {
                        return Ok(false);
                    };
                    import_pack(&src, &dest).map(|_| true)
                })
                .await;
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(w) = ew.upgrade() {
                        match res {
                            Ok(Ok(true)) => {
                                w.set_error_message(SharedString::new());
                                refresh_edit_resources_ui(&w, &svc, &inst);
                            }
                            Ok(Err(e)) => w.set_error_message(SharedString::from(e.to_string())),
                            _ => {}
                        }
                    }
                });
            });
        });
    }
    {
        let ew = edit_window.as_weak();
        let svc = svc.clone();
        edit_window.on_remove_texture_pack_clicked(move |fname| {
            let Some(w) = ew.upgrade() else { return };
            let id = w.get_instance_id().to_string();
            let Some((inst, _)) = find_instance_by_id(&svc, &id) else {
                return;
            };
            let dir = texture_packs_dir(
                &svc.paths.instance_game_dir(&inst.dir_name),
                &inst.minecraft_version,
            );
            let target = dir.join(fname.as_str());
            if let Err(e) = remove_pack(&target, &dir) {
                w.set_error_message(SharedString::from(e.to_string()));
            } else {
                let mut idx = ModIndex::load(&dir);
                idx.remove_by_file(fname.as_str());
                let _ = idx.save(&dir);
                w.set_selected_texture_pack(SharedString::new());
                refresh_edit_resources_ui(&w, &svc, &inst);
            }
        });
    }
    {
        let ew = edit_window.as_weak();
        let svc = svc.clone();
        edit_window.on_open_texture_packs_folder_clicked(move || {
            let Some(w) = ew.upgrade() else { return };
            let id = w.get_instance_id().to_string();
            if let Some((inst, _)) = find_instance_by_id(&svc, &id) {
                let dir = texture_packs_dir(
                    &svc.paths.instance_game_dir(&inst.dir_name),
                    &inst.minecraft_version,
                );
                let _ = std::fs::create_dir_all(&dir);
                let _ = open_in_file_manager(&dir);
            }
        });
    }

    // Shader Packs: Import / Remove / Open Folder
    {
        let ew = edit_window.as_weak();
        let svc = svc.clone();
        edit_window.on_import_shader_pack_clicked(move || {
            let Some(w) = ew.upgrade() else { return };
            let id = w.get_instance_id().to_string();
            let Some((inst, _)) = find_instance_by_id(&svc, &id) else {
                return;
            };
            let dest = shader_packs_dir(&svc.paths.instance_game_dir(&inst.dir_name));
            let ew = ew.clone();
            let svc = svc.clone();
            let rt = svc.rt.clone();
            rt.spawn(async move {
                let res = tokio::task::spawn_blocking(move || {
                    let Some(src) = pick_zip_dialog("Import Shader Pack", false) else {
                        return Ok(false);
                    };
                    import_pack(&src, &dest).map(|_| true)
                })
                .await;
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(w) = ew.upgrade() {
                        match res {
                            Ok(Ok(true)) => {
                                w.set_error_message(SharedString::new());
                                refresh_edit_resources_ui(&w, &svc, &inst);
                            }
                            Ok(Err(e)) => w.set_error_message(SharedString::from(e.to_string())),
                            _ => {}
                        }
                    }
                });
            });
        });
    }
    {
        let ew = edit_window.as_weak();
        let svc = svc.clone();
        edit_window.on_remove_shader_pack_clicked(move |fname| {
            let Some(w) = ew.upgrade() else { return };
            let id = w.get_instance_id().to_string();
            let Some((inst, _)) = find_instance_by_id(&svc, &id) else {
                return;
            };
            let dir = shader_packs_dir(&svc.paths.instance_game_dir(&inst.dir_name));
            let target = dir.join(fname.as_str());
            if let Err(e) = remove_pack(&target, &dir) {
                w.set_error_message(SharedString::from(e.to_string()));
            } else {
                let mut idx = ModIndex::load(&dir);
                idx.remove_by_file(fname.as_str());
                let _ = idx.save(&dir);
                w.set_selected_shader_pack(SharedString::new());
                refresh_edit_resources_ui(&w, &svc, &inst);
            }
        });
    }
    {
        let ew = edit_window.as_weak();
        let svc = svc.clone();
        edit_window.on_open_shader_packs_folder_clicked(move || {
            let Some(w) = ew.upgrade() else { return };
            let id = w.get_instance_id().to_string();
            if let Some((inst, _)) = find_instance_by_id(&svc, &id) {
                let dir = shader_packs_dir(&svc.paths.instance_game_dir(&inst.dir_name));
                let _ = std::fs::create_dir_all(&dir);
                let _ = open_in_file_manager(&dir);
            }
        });
    }

    // Worlds: Import / Export / Backup / Rename / Delete / Open Folder
    {
        let ew = edit_window.as_weak();
        let svc = svc.clone();
        edit_window.on_import_world_clicked(move || {
            let Some(w) = ew.upgrade() else { return };
            let id = w.get_instance_id().to_string();
            let Some((inst, _)) = find_instance_by_id(&svc, &id) else {
                return;
            };
            let mc_dir = svc.paths.instance_game_dir(&inst.dir_name);
            let ew = ew.clone();
            let svc = svc.clone();
            let rt = svc.rt.clone();
            rt.spawn(async move {
                let res = tokio::task::spawn_blocking(move || {
                    let Some(src) = pick_zip_dialog("Import World ZIP", false) else {
                        return Ok(false);
                    };
                    import_world_zip(&src, &mc_dir).map(|_| true)
                })
                .await;
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(w) = ew.upgrade() {
                        match res {
                            Ok(Ok(true)) => {
                                w.set_error_message(SharedString::new());
                                refresh_edit_resources_ui(&w, &svc, &inst);
                            }
                            Ok(Err(e)) => w.set_error_message(SharedString::from(e.to_string())),
                            _ => {}
                        }
                    }
                });
            });
        });
    }
    {
        let ew = edit_window.as_weak();
        let svc = svc.clone();
        edit_window.on_export_world_clicked(move |folder| {
            let Some(w) = ew.upgrade() else { return };
            let id = w.get_instance_id().to_string();
            let Some((inst, _)) = find_instance_by_id(&svc, &id) else {
                return;
            };
            let world_dir =
                saves_dir(&svc.paths.instance_game_dir(&inst.dir_name)).join(folder.as_str());
            let ew = ew.clone();
            svc.rt.spawn(async move {
                let res = tokio::task::spawn_blocking(move || {
                    let Some(dest) = pick_zip_dialog("Export World ZIP", true) else {
                        return Ok(());
                    };
                    export_world_zip(&world_dir, &dest)
                })
                .await;
                if let Ok(Err(e)) = res {
                    let msg = e.to_string();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(w) = ew.upgrade() {
                            w.set_error_message(SharedString::from(msg));
                        }
                    });
                }
            });
        });
    }
    {
        let ew = edit_window.as_weak();
        let svc = svc.clone();
        edit_window.on_backup_world_clicked(move |folder| {
            let folder_str = folder.trim().to_string();
            if folder_str.is_empty() {
                return;
            }
            let Some(w) = ew.upgrade() else { return };
            let id = w.get_instance_id().to_string();
            let Some((inst, _)) = find_instance_by_id(&svc, &id) else {
                return;
            };
            w.set_error_message(SharedString::new());
            w.set_status_message(SharedString::from(format!(
                "Backing up world '{}'...",
                folder_str
            )));
            let mc_dir = svc.paths.instance_game_dir(&inst.dir_name);
            let world_dir = saves_dir(&mc_dir).join(&folder_str);
            let ew = ew.clone();
            let svc_bg = svc.clone();
            let inst_bg = inst.clone();
            svc.rt.spawn(async move {
                let res =
                    tokio::task::spawn_blocking(move || backup_world_copy(&world_dir, &mc_dir))
                        .await;
                match res {
                    Ok(Ok(dest)) => {
                        let backup_folder = dest
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_else(|| format!("{folder_str} (Backup)"));
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(w) = ew.upgrade() {
                                w.set_error_message(SharedString::new());
                                w.set_status_message(SharedString::from(format!(
                                    "Backup created: {}",
                                    backup_folder
                                )));
                                refresh_edit_resources_ui(&w, &svc_bg, &inst_bg);
                            }
                        });
                    }
                    Ok(Err(e)) => {
                        let msg = format!("World backup failed: {e}");
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(w) = ew.upgrade() {
                                w.set_status_message(SharedString::new());
                                w.set_error_message(SharedString::from(msg));
                            }
                        });
                    }
                    Err(e) => {
                        let msg = format!("World backup task error: {e}");
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(w) = ew.upgrade() {
                                w.set_status_message(SharedString::new());
                                w.set_error_message(SharedString::from(msg));
                            }
                        });
                    }
                }
            });
        });
    }
    {
        let ew = edit_window.as_weak();
        let svc = svc.clone();
        edit_window.on_open_world_datapacks_clicked(move |folder| {
            let Some(w) = ew.upgrade() else { return };
            let id = w.get_instance_id().to_string();
            if let Some((inst, _)) = find_instance_by_id(&svc, &id) {
                let dp_dir = saves_dir(&svc.paths.instance_game_dir(&inst.dir_name))
                    .join(folder.as_str())
                    .join("datapacks");
                let _ = std::fs::create_dir_all(&dp_dir);
                let _ = open_in_file_manager(&dp_dir);
            }
        });
    }
    {
        let ew = edit_window.as_weak();
        let svc = svc.clone();
        edit_window.on_reset_world_icon_clicked(move |folder| {
            let Some(w) = ew.upgrade() else { return };
            let id = w.get_instance_id().to_string();
            let Some((inst, _)) = find_instance_by_id(&svc, &id) else {
                return;
            };
            let world_dir =
                saves_dir(&svc.paths.instance_game_dir(&inst.dir_name)).join(folder.as_str());
            match reset_world_icon(&world_dir) {
                Ok(true) => {
                    w.set_status_message(SharedString::from("World icon reset."));
                    refresh_edit_resources_ui(&w, &svc, &inst);
                }
                Ok(false) => {
                    w.set_status_message(SharedString::from(
                        "World already uses the default icon.",
                    ));
                }
                Err(e) => w.set_error_message(SharedString::from(e.to_string())),
            }
        });
    }
    {
        let ew = edit_window.as_weak();
        let svc = svc.clone();
        edit_window.on_copy_world_seed_clicked(move |folder| {
            let Some(w) = ew.upgrade() else { return };
            let id = w.get_instance_id().to_string();
            let Some((inst, _)) = find_instance_by_id(&svc, &id) else {
                return;
            };
            let world_dir =
                saves_dir(&svc.paths.instance_game_dir(&inst.dir_name)).join(folder.as_str());
            match read_world_seed(&world_dir) {
                Ok(seed) => {
                    let seed_str = seed.to_string();
                    match arboard::Clipboard::new().and_then(|mut cb| cb.set_text(&seed_str)) {
                        Ok(()) => {
                            w.set_error_message(SharedString::new());
                            w.set_status_message(SharedString::from(format!(
                                "Seed copied to clipboard: {seed_str}"
                            )));
                        }
                        Err(e) => w.set_error_message(SharedString::from(format!(
                            "Failed to copy seed: {e}"
                        ))),
                    }
                }
                Err(e) => w.set_error_message(SharedString::from(e.to_string())),
            }
        });
    }
    {
        let ew = edit_window.as_weak();
        let svc = svc.clone();
        edit_window.on_rename_world_clicked(move |folder, new_name| {
            let Some(w) = ew.upgrade() else { return };
            let id = w.get_instance_id().to_string();
            let Some((inst, running)) = find_instance_by_id(&svc, &id) else {
                return;
            };
            let world_dir =
                saves_dir(&svc.paths.instance_game_dir(&inst.dir_name)).join(folder.as_str());
            if let Err(e) = rename_world(&world_dir, &new_name, running) {
                w.set_error_message(SharedString::from(e.to_string()));
            } else {
                w.set_error_message(SharedString::new());
                refresh_edit_resources_ui(&w, &svc, &inst);
            }
        });
    }
    {
        let ew = edit_window.as_weak();
        let svc = svc.clone();
        edit_window.on_delete_world_clicked(move |folder| {
            let Some(w) = ew.upgrade() else { return };
            let id = w.get_instance_id().to_string();
            let Some((inst, running)) = find_instance_by_id(&svc, &id) else {
                return;
            };
            let mc_dir = svc.paths.instance_game_dir(&inst.dir_name);
            let world_dir = saves_dir(&mc_dir).join(folder.as_str());
            if let Err(e) = delete_world(&world_dir, &mc_dir, running) {
                w.set_error_message(SharedString::from(e.to_string()));
            } else {
                w.set_selected_world_folder(SharedString::new());
                refresh_edit_resources_ui(&w, &svc, &inst);
            }
        });
    }
    {
        let ew = edit_window.as_weak();
        let svc = svc.clone();
        edit_window.on_open_world_folder_clicked(move |folder| {
            let Some(w) = ew.upgrade() else { return };
            let id = w.get_instance_id().to_string();
            if let Some((inst, _)) = find_instance_by_id(&svc, &id) {
                let world_dir =
                    saves_dir(&svc.paths.instance_game_dir(&inst.dir_name)).join(folder.as_str());
                let _ = open_in_file_manager(&world_dir);
            }
        });
    }

    // Servers (servers.dat NBT): Add / Edit / Remove / Move Up / Move Down
    {
        let ew = edit_window.as_weak();
        let svc = svc.clone();
        edit_window.on_add_server_clicked(move |name, ip| {
            let Some(w) = ew.upgrade() else { return };
            let id = w.get_instance_id().to_string();
            let Some((inst, running)) = find_instance_by_id(&svc, &id) else {
                return;
            };
            let mc_dir = svc.paths.instance_game_dir(&inst.dir_name);
            let mut list = load_servers(&mc_dir).unwrap_or_default();
            list.push(MinecraftServerEntry::new(name.to_string(), ip.to_string()));
            if let Err(e) = save_servers(&mc_dir, &list, running) {
                w.set_error_message(SharedString::from(e.to_string()));
            } else {
                w.set_error_message(SharedString::new());
                refresh_edit_resources_ui(&w, &svc, &inst);
            }
        });
    }
    {
        let ew = edit_window.as_weak();
        let svc = svc.clone();
        edit_window.on_edit_server_clicked(move |idx, name, ip| {
            let Some(w) = ew.upgrade() else { return };
            let id = w.get_instance_id().to_string();
            let Some((inst, running)) = find_instance_by_id(&svc, &id) else {
                return;
            };
            let mc_dir = svc.paths.instance_game_dir(&inst.dir_name);
            let mut list = load_servers(&mc_dir).unwrap_or_default();
            if let Some(entry) = list.get_mut(idx as usize) {
                entry.name = name.to_string();
                entry.ip = ip.to_string();
                if let Err(e) = save_servers(&mc_dir, &list, running) {
                    w.set_error_message(SharedString::from(e.to_string()));
                } else {
                    refresh_edit_resources_ui(&w, &svc, &inst);
                }
            }
        });
    }
    {
        let ew = edit_window.as_weak();
        let svc = svc.clone();
        edit_window.on_remove_server_clicked(move |idx| {
            let Some(w) = ew.upgrade() else { return };
            let id = w.get_instance_id().to_string();
            let Some((inst, running)) = find_instance_by_id(&svc, &id) else {
                return;
            };
            let mc_dir = svc.paths.instance_game_dir(&inst.dir_name);
            let mut list = load_servers(&mc_dir).unwrap_or_default();
            if (idx as usize) < list.len() {
                list.remove(idx as usize);
                if let Err(e) = save_servers(&mc_dir, &list, running) {
                    w.set_error_message(SharedString::from(e.to_string()));
                } else {
                    w.set_selected_server_index(-1);
                    refresh_edit_resources_ui(&w, &svc, &inst);
                }
            }
        });
    }
    {
        let ew = edit_window.as_weak();
        let svc = svc.clone();
        edit_window.on_move_server_up_clicked(move |idx| {
            let Some(w) = ew.upgrade() else { return };
            let id = w.get_instance_id().to_string();
            let Some((inst, running)) = find_instance_by_id(&svc, &id) else {
                return;
            };
            let mc_dir = svc.paths.instance_game_dir(&inst.dir_name);
            let mut list = load_servers(&mc_dir).unwrap_or_default();
            let i = idx as usize;
            if i > 0 && i < list.len() {
                list.swap(i, i - 1);
                if save_servers(&mc_dir, &list, running).is_ok() {
                    w.set_selected_server_index((i - 1) as i32);
                    refresh_edit_resources_ui(&w, &svc, &inst);
                }
            }
        });
    }
    {
        let ew = edit_window.as_weak();
        let svc = svc.clone();
        edit_window.on_move_server_down_clicked(move |idx| {
            let Some(w) = ew.upgrade() else { return };
            let id = w.get_instance_id().to_string();
            let Some((inst, running)) = find_instance_by_id(&svc, &id) else {
                return;
            };
            let mc_dir = svc.paths.instance_game_dir(&inst.dir_name);
            let mut list = load_servers(&mc_dir).unwrap_or_default();
            let i = idx as usize;
            if i + 1 < list.len() {
                list.swap(i, i + 1);
                if save_servers(&mc_dir, &list, running).is_ok() {
                    w.set_selected_server_index((i + 1) as i32);
                    refresh_edit_resources_ui(&w, &svc, &inst);
                }
            }
        });
    }
}

#[derive(Default)]
struct DownloadModsSession {
    results: Vec<ModSearchResult>,
    total_hits: usize,
    versions: Vec<ModRemoteVersion>,
    queue: Vec<(ModSearchResult, ModRemoteVersion)>,
    resolved_plan: Vec<(String, ModRemoteVersion, String, bool)>, // (title, version, homepage, is_dep)
    filters: SearchFilters,
}

fn default_filters_for_instance(inst: &InstanceMetadata, kind: ContentKind) -> SearchFilters {
    let loaders = if kind.uses_modloader_filter() {
        inst.modloader_spec
            .as_ref()
            .map(|s| vec![s.kind.short_name().to_lowercase()])
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    SearchFilters {
        categories: Vec::new(),
        loaders,
        game_version: Some(inst.minecraft_version.clone()),
        client_side: false,
        server_side: false,
        hide_installed: false,
    }
}

fn format_filter_summary(filters: &SearchFilters, kind: ContentKind) -> String {
    let mut parts = Vec::new();
    if let Some(v) = filters.game_version.as_deref() {
        if !v.is_empty() {
            parts.push(format!("Minecraft {v}"));
        }
    }
    if kind.uses_modloader_filter() && !filters.loaders.is_empty() {
        parts.push(filters.loaders.join("/"));
    }
    if !filters.categories.is_empty() {
        parts.push(format!("{} cat.", filters.categories.len()));
    }
    if filters.client_side {
        parts.push("Client".into());
    }
    if filters.server_side {
        parts.push("Server".into());
    }
    if filters.hide_installed {
        parts.push("Hide Installed".into());
    }
    if parts.is_empty() {
        "All Filters".into()
    } else {
        parts.join(" • ")
    }
}

fn format_downloads(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

fn trigger_mod_platform_search(
    dm_weak: &Weak<DownloadModsWindow>,
    session: &Arc<Mutex<DownloadModsSession>>,
    svc: &Services,
) {
    trigger_mod_platform_search_page(dm_weak, session, svc, false);
}

fn content_target_dir(svc: &Services, inst: &InstanceMetadata, kind: ContentKind) -> PathBuf {
    let mc_dir = svc.paths.instance_game_dir(&inst.dir_name);
    match kind {
        ContentKind::Mod => mc_dir.join("mods"),
        ContentKind::ResourcePack => texture_packs_dir(&mc_dir, &inst.minecraft_version),
        ContentKind::ShaderPack => shader_packs_dir(&mc_dir),
    }
}

static TASK_PAUSED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static CONTENT_SEARCH_GEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn sync_downloader_filter_panel(
    dm_win: &DownloadModsWindow,
    filters: &SearchFilters,
    kind: ContentKind,
) {
    dm_win.set_show_modloaders(kind.uses_modloader_filter());
    dm_win.set_mc_version_filter(SharedString::from(
        filters.game_version.as_deref().unwrap_or(""),
    ));
    dm_win.set_filter_client_side(filters.client_side);
    dm_win.set_filter_server_side(filters.server_side);
    dm_win.set_hide_installed(filters.hide_installed);
    let cats: Vec<FilterOptionItem> = categories_for_kind(kind)
        .iter()
        .map(|(id, label)| FilterOptionItem {
            id: SharedString::from(*id),
            label: SharedString::from(*label),
            checked: filters.categories.iter().any(|c| c == id),
        })
        .collect();
    dm_win.set_filter_categories(ModelRc::from(Rc::new(VecModel::from(cats))));
    let loaders: Vec<FilterOptionItem> = [
        ("fabric", "Fabric"),
        ("forge", "Forge"),
        ("neoforge", "NeoForge"),
        ("quilt", "Quilt"),
        ("liteloader", "LiteLoader"),
    ]
    .iter()
    .map(|(id, label)| FilterOptionItem {
        id: SharedString::from(*id),
        label: SharedString::from(*label),
        checked: filters.loaders.iter().any(|l| l == id),
    })
    .collect();
    dm_win.set_filter_modloaders(ModelRc::from(Rc::new(VecModel::from(loaders))));
}

fn open_content_downloader(
    dm_win: &Rc<DownloadModsWindow>,
    session: &Arc<Mutex<DownloadModsSession>>,
    svc: &Services,
    inst: &InstanceMetadata,
    kind: ContentKind,
    initial_query: &str,
) {
    let default_filters = default_filters_for_instance(inst, kind);
    let summary = format_filter_summary(&default_filters, kind);
    sync_downloader_filter_panel(dm_win, &default_filters, kind);
    if let Ok(mut g) = session.lock() {
        *g = DownloadModsSession {
            filters: default_filters,
            ..Default::default()
        };
    }
    dm_win.set_instance_id(SharedString::from(&inst.id));
    dm_win.set_instance_name(SharedString::from(&inst.name));
    dm_win.set_content_title(SharedString::from(kind.window_title()));
    dm_win.set_content_kind(SharedString::from(kind.as_id()));
    dm_win.set_filter_summary(SharedString::from(summary));
    dm_win.set_show_filters_panel(false);
    dm_win.set_active_provider(0);
    dm_win.set_search_query(SharedString::from(initial_query));
    dm_win.set_selected_project_id(SharedString::new());
    dm_win.set_queued_count(0);
    dm_win.set_show_review_modal(false);
    dm_win.set_status_message(SharedString::new());
    dm_win.set_error_message(SharedString::new());
    let _ = dm_win.show();
    trigger_mod_platform_search(&dm_win.as_weak(), session, svc);
}

fn trigger_mod_platform_search_page(
    dm_weak: &Weak<DownloadModsWindow>,
    session: &Arc<Mutex<DownloadModsSession>>,
    svc: &Services,
    append: bool,
) {
    let Some(w) = dm_weak.upgrade() else { return };
    let inst_id = w.get_instance_id().to_string();
    let Some((inst, _)) = find_instance_by_id(svc, &inst_id) else {
        return;
    };
    let kind = ContentKind::from_id(w.get_content_kind().as_str());
    let provider = if w.get_active_provider() == 1 {
        ModProvider::CurseForge
    } else {
        ModProvider::Modrinth
    };
    let query = w.get_search_query().to_string();
    let sort = ModSortBy::from_id(w.get_sort_id().as_str());
    let filters = session
        .lock()
        .map(|g| g.filters.clone())
        .unwrap_or_else(|_| default_filters_for_instance(&inst, kind));
    let mc_ver = filters.game_version.clone().unwrap_or_default();
    let loader = if kind.uses_modloader_filter() {
        filters.loaders.first().and_then(|l| match l.as_str() {
            "fabric" => Some(ModloaderKind::Fabric),
            "forge" => Some(ModloaderKind::Forge),
            "neoforge" => Some(ModloaderKind::NeoForge),
            "quilt" => Some(ModloaderKind::Quilt),
            "liteloader" => Some(ModloaderKind::LiteLoader),
            _ => None,
        })
    } else {
        None
    };
    let target_dir = content_target_dir(svc, &inst, kind);
    let installed_ids =
        ModIndex::load(&target_dir).installed_project_ids_verified(&target_dir, provider);
    let offset = if append {
        session.lock().map(|g| g.results.len()).unwrap_or(0)
    } else {
        0
    };

    if append {
        w.set_is_loading_more(true);
    } else {
        w.set_is_searching(true);
        w.set_error_message(SharedString::new());
    }

    let req_gen = CONTENT_SEARCH_GEN.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
    let dm_weak = dm_weak.clone();
    let session = session.clone();
    let client = svc.http_client.clone();
    let cache_dir = svc.paths.cache_dir.clone();
    let rt = svc.rt.clone();
    rt.spawn(async move {
        let res = match provider {
            ModProvider::Modrinth => {
                search_modrinth_filtered_paged(
                    &client,
                    kind,
                    &query,
                    &filters,
                    sort,
                    offset,
                    &installed_ids,
                )
                .await
            }
            ModProvider::CurseForge => search_curseforge_content(
                &client,
                kind,
                &query,
                &mc_ver,
                loader,
                sort,
                &installed_ids,
            )
            .await
            .map(|mut hits| {
                if filters.hide_installed {
                    hits.retain(|h| !h.installed);
                }
                let len = hits.len();
                (hits, len)
            }),
            ModProvider::Local => Ok((Vec::new(), 0)),
        };

        // Baixa e decodifica todos os ícones reais concorrentemente em `.png` no cache local
        let hits_with_icons = match res {
            Ok((hits, total_hits)) => {
                let futs = hits.iter().map(|h| {
                    let c = client.clone();
                    let cd = cache_dir.clone();
                    let prov = h.provider;
                    let pid = h.project_id.clone();
                    let url = h.icon_url.clone();
                    async move {
                        if let Some(u) = url.as_deref() {
                            fetch_and_cache_mod_icon(&c, &cd, prov, &pid, u).await
                        } else {
                            None
                        }
                    }
                });
                let icon_paths = futures_util::future::join_all(futs).await;
                Ok((hits, total_hits, icon_paths))
            }
            Err(e) => Err(e),
        };

        let _ = slint::invoke_from_event_loop(move || {
            // Descarta resultados de requisições obsoletas
            if CONTENT_SEARCH_GEN.load(std::sync::atomic::Ordering::SeqCst) != req_gen {
                return;
            }
            let Some(w) = dm_weak.upgrade() else { return };
            w.set_is_searching(false);
            w.set_is_loading_more(false);
            match hits_with_icons {
                Ok((hits, total_hits, icon_paths)) => {
                    let queued_ids: std::collections::HashSet<String> = session
                        .lock()
                        .map(|g| g.queue.iter().map(|(m, _)| m.project_id.clone()).collect())
                        .unwrap_or_default();
                    let new_items: Vec<RemoteModListItem> = hits
                        .iter()
                        .zip(icon_paths)
                        .map(|(h, ipath)| {
                            let (icon, has_icon) = ipath
                                .and_then(|p| Image::load_from_path(&p).ok())
                                .map(|img| (img, true))
                                .unwrap_or_else(|| (Image::default(), false));
                            RemoteModListItem {
                                project_id: SharedString::from(&h.project_id),
                                title: SharedString::from(&h.title),
                                description: SharedString::from(&h.description),
                                author: SharedString::from(&h.author),
                                downloads_display: SharedString::from(format_downloads(
                                    h.downloads,
                                )),
                                homepage_url: SharedString::from(&h.homepage_url),
                                icon,
                                has_icon,
                                installed: h.installed,
                                selected_for_download: queued_ids.contains(&h.project_id),
                            }
                        })
                        .collect();

                    let total_loaded = if let Ok(mut g) = session.lock() {
                        if append {
                            g.results.extend(hits);
                        } else {
                            g.results = hits;
                        }
                        g.total_hits = total_hits;
                        g.results.len()
                    } else {
                        new_items.len()
                    };

                    if append {
                        let existing_rc = w.get_results();
                        let mut combined: Vec<RemoteModListItem> = (0..existing_rc.row_count())
                            .filter_map(|i| existing_rc.row_data(i))
                            .collect();
                        combined.extend(new_items);
                        w.set_results(ModelRc::from(Rc::new(VecModel::from(combined))));
                    } else {
                        w.set_results(ModelRc::from(Rc::new(VecModel::from(new_items))));
                    }
                    w.set_can_load_more(total_loaded < total_hits);
                }
                Err(err) => {
                    if !append {
                        w.set_results(ModelRc::from(Rc::new(VecModel::from(Vec::new()))));
                    }
                    w.set_can_load_more(false);
                    w.set_error_message(SharedString::from(err.to_string()));
                }
            }
        });
    });
}

struct PendingUpdateItem {
    id: String,
    old_file_name: String,
    title: String,
    homepage_url: String,
    author: String,
    description: String,
    version: ModRemoteVersion,
    selected: bool,
}

struct PendingDependencyItem {
    id: String,
    title: String,
    homepage_url: String,
    version: ModRemoteVersion,
    selected: bool,
}

fn categories_for_kind(kind: ContentKind) -> &'static [(&'static str, &'static str)] {
    match kind {
        ContentKind::Mod => &[
            ("adventure", "Adventure"),
            ("decoration", "Decoration"),
            ("equipment", "Equipment"),
            ("food", "Food"),
            ("game-mechanics", "Game Mechanics"),
            ("library", "Library"),
            ("magic", "Magic"),
            ("management", "Management"),
            ("optimization", "Optimization"),
            ("storage", "Storage"),
            ("technology", "Technology"),
            ("transportation", "Transportation"),
            ("utility", "Utility"),
            ("worldgen", "Worldgen"),
        ],
        ContentKind::ResourcePack => &[
            ("16x", "16x"),
            ("32x", "32x"),
            ("64x", "64x"),
            ("128x", "128x"),
            ("combat", "Combat"),
            ("gui", "GUI"),
            ("models", "Models"),
            ("realistic", "Realistic"),
            ("simplistic", "Simplistic"),
            ("vanilla-like", "Vanilla-like"),
        ],
        ContentKind::ShaderPack => &[
            ("cartoon", "Cartoon"),
            ("fantasy", "Fantasy"),
            ("path-tracing", "Path Tracing"),
            ("realistic", "Realistic"),
            ("semi-realistic", "Semi-Realistic"),
            ("vanilla-like", "Vanilla-like"),
        ],
    }
}

fn open_check_updates_window(
    ew: &Weak<EditInstanceWindow>,
    cu_win: &Rc<CheckUpdatesWindow>,
    pending_updates: &Arc<Mutex<(String, ContentKind, Vec<PendingUpdateItem>)>>,
    svc: &Services,
    kind: ContentKind,
) {
    let Some(w) = ew.upgrade() else { return };
    let id = w.get_instance_id().to_string();
    let Some((inst, _)) = find_instance_by_id(svc, &id) else {
        return;
    };
    let label = match kind {
        ContentKind::Mod => "Mods",
        ContentKind::ResourcePack => "Resource Packs",
        ContentKind::ShaderPack => "Shader Packs",
    };
    cu_win.set_heading_title(SharedString::from(format!("Check for Updates — {label}")));
    cu_win.set_is_checking(true);
    cu_win.set_status_text(SharedString::from("Checking for updates..."));
    cu_win.set_updates(ModelRc::from(Rc::new(VecModel::from(Vec::new()))));
    center_window_over(cu_win.window(), w.window());
    let _ = cu_win.show();

    let target_dir = content_target_dir(svc, &inst, kind);
    let index = ModIndex::load(&target_dir);
    if index.mods.is_empty() {
        cu_win.set_is_checking(false);
        cu_win.set_status_text(SharedString::from(format!(
            "No Modrinth/CurseForge metadata found for installed {}.",
            label.to_lowercase()
        )));
        return;
    }

    let mc_ver = inst.minecraft_version.clone();
    let loader = if kind.uses_modloader_filter() {
        inst.modloader_spec.as_ref().map(|s| s.kind)
    } else {
        None
    };
    let client = svc.http_client.clone();
    let cache_dir = svc.paths.cache_dir.clone();
    let cu_weak = cu_win.as_weak();
    let pending_updates = pending_updates.clone();
    let inst_id = inst.id.clone();
    let rt = svc.rt.clone();

    rt.spawn(async move {
        let mut found: Vec<(PendingUpdateItem, Option<PathBuf>)> = Vec::new();
        for rec in &index.mods {
            let versions = match rec.provider {
                ModProvider::Modrinth => {
                    fetch_modrinth_versions(&client, &rec.project_id, &mc_ver, loader)
                        .await
                        .ok()
                }
                ModProvider::CurseForge => {
                    fetch_curseforge_versions(&client, &rec.project_id, &mc_ver, loader)
                        .await
                        .ok()
                }
                ModProvider::Local => None,
            };
            if let Some(latest) = versions.and_then(|v| v.into_iter().next()) {
                if latest.version_id != rec.version_id {
                    let icon_file = cache_dir.join("mod-icons").join(format!(
                        "{}-{}.png",
                        rec.provider.label().to_lowercase(),
                        rec.project_id
                    ));
                    let icon_path = icon_file.is_file().then_some(icon_file);
                    found.push((
                        PendingUpdateItem {
                            id: rec.project_id.clone(),
                            old_file_name: rec.file_name.clone(),
                            title: if rec.title.is_empty() {
                                rec.file_name.clone()
                            } else {
                                rec.title.clone()
                            },
                            homepage_url: rec.homepage_url.clone(),
                            author: rec.author.clone(),
                            description: rec.description.clone(),
                            version: latest,
                            selected: true,
                        },
                        icon_path,
                    ));
                }
            }
        }

        let count = found.len();
        let _ = slint::invoke_from_event_loop(move || {
            let ui_items: Vec<UpdateCandidateItem> = found
                .iter()
                .map(|(item, ipath)| {
                    let (icon, has_icon) = ipath
                        .as_ref()
                        .and_then(|p| Image::load_from_path(p).ok())
                        .map(|img| (img, true))
                        .unwrap_or_else(|| (Image::default(), false));
                    let installed_ver = index
                        .mods
                        .iter()
                        .find(|r| r.project_id == item.id)
                        .map(|r| r.version_number.as_str())
                        .unwrap_or("—");
                    UpdateCandidateItem {
                        id: SharedString::from(&item.id),
                        name: SharedString::from(&item.title),
                        installed_version: SharedString::from(installed_ver),
                        available_version: SharedString::from(&item.version.version_number),
                        provider: SharedString::from(item.version.provider.label()),
                        selected: item.selected,
                        icon,
                        has_icon,
                    }
                })
                .collect();

            if let Ok(mut g) = pending_updates.lock() {
                *g = (
                    inst_id,
                    kind,
                    found.into_iter().map(|(item, _)| item).collect(),
                );
            }
            if let Some(cu) = cu_weak.upgrade() {
                cu.set_is_checking(false);
                cu.set_updates(ModelRc::from(Rc::new(VecModel::from(ui_items))));
                if count == 0 {
                    cu.set_status_text(SharedString::from(format!(
                        "All tracked {} are up to date.",
                        label.to_lowercase()
                    )));
                } else {
                    cu.set_status_text(SharedString::from(format!("{count} update(s) available.")));
                }
            }
        });
    });
}

fn wire_download_mods_callbacks(
    edit_window: &Rc<EditInstanceWindow>,
    dm_window: &Rc<DownloadModsWindow>,
    check_updates_window: &Rc<CheckUpdatesWindow>,
    verify_deps_window: &Rc<VerifyDependenciesWindow>,
    svc: &Services,
) {
    let session = Arc::new(Mutex::new(DownloadModsSession::default()));
    let pending_updates: Arc<Mutex<(String, ContentKind, Vec<PendingUpdateItem>)>> =
        Arc::new(Mutex::new((String::new(), ContentKind::Mod, Vec::new())));
    let pending_deps: Arc<Mutex<(String, Vec<PendingDependencyItem>)>> =
        Arc::new(Mutex::new((String::new(), Vec::new())));

    // Painel lateral retrátil de filtros integrado ao DownloadModsWindow (aplicação imediata)
    {
        let dm_weak = dm_window.as_weak();
        let session = session.clone();
        let svc = svc.clone();
        dm_window.on_toggle_filter_category(move |cid| {
            let cid_s = cid.to_string();
            if let Ok(mut g) = session.lock() {
                if let Some(pos) = g.filters.categories.iter().position(|c| c == &cid_s) {
                    g.filters.categories.remove(pos);
                } else {
                    g.filters.categories.push(cid_s);
                }
            }
            if let Some(dm) = dm_weak.upgrade() {
                let model = dm.get_filter_categories();
                for i in 0..model.row_count() {
                    if let Some(mut item) = model.row_data(i) {
                        if item.id == cid {
                            item.checked = !item.checked;
                            model.set_row_data(i, item);
                            break;
                        }
                    }
                }
            }
            trigger_mod_platform_search(&dm_weak, &session, &svc);
        });
    }
    {
        let dm_weak = dm_window.as_weak();
        let session = session.clone();
        let svc = svc.clone();
        dm_window.on_toggle_filter_modloader(move |lid| {
            let lid_s = lid.to_string();
            if let Ok(mut g) = session.lock() {
                if let Some(pos) = g.filters.loaders.iter().position(|l| l == &lid_s) {
                    g.filters.loaders.remove(pos);
                } else {
                    g.filters.loaders.push(lid_s);
                }
            }
            if let Some(dm) = dm_weak.upgrade() {
                let model = dm.get_filter_modloaders();
                for i in 0..model.row_count() {
                    if let Some(mut item) = model.row_data(i) {
                        if item.id == lid {
                            item.checked = !item.checked;
                            model.set_row_data(i, item);
                            break;
                        }
                    }
                }
            }
            trigger_mod_platform_search(&dm_weak, &session, &svc);
        });
    }
    {
        let dm_weak = dm_window.as_weak();
        let session = session.clone();
        let svc = svc.clone();
        dm_window.on_filters_changed(move || {
            let Some(dm) = dm_weak.upgrade() else { return };
            let ver = dm.get_mc_version_filter().trim().to_string();
            if let Ok(mut g) = session.lock() {
                g.filters.game_version = if ver.is_empty() { None } else { Some(ver) };
                g.filters.client_side = dm.get_filter_client_side();
                g.filters.server_side = dm.get_filter_server_side();
                g.filters.hide_installed = dm.get_hide_installed();
            }
            trigger_mod_platform_search(&dm_weak, &session, &svc);
        });
    }
    {
        let dm_weak = dm_window.as_weak();
        let session = session.clone();
        let svc = svc.clone();
        dm_window.on_reset_filters_clicked(move || {
            let Some(dm) = dm_weak.upgrade() else { return };
            let inst_id = dm.get_instance_id().to_string();
            let kind = ContentKind::from_id(dm.get_content_kind().as_str());
            let defaults = find_instance_by_id(&svc, &inst_id)
                .map(|(inst, _)| default_filters_for_instance(&inst, kind))
                .unwrap_or_default();
            sync_downloader_filter_panel(&dm, &defaults, kind);
            if let Ok(mut g) = session.lock() {
                g.filters = defaults;
            }
            trigger_mod_platform_search(&dm_weak, &session, &svc);
        });
    }

    // CheckUpdatesWindow wiring
    {
        let cu_weak = check_updates_window.as_weak();
        let pending = pending_updates.clone();
        check_updates_window.on_toggle_item(move |id| {
            let id_s = id.to_string();
            if let Ok(mut g) = pending.lock() {
                if let Some(item) = g.2.iter_mut().find(|x| x.id == id_s) {
                    item.selected = !item.selected;
                }
            }
            if let Some(cu) = cu_weak.upgrade() {
                let model = cu.get_updates();
                for i in 0..model.row_count() {
                    if let Some(mut row) = model.row_data(i) {
                        if row.id == id {
                            row.selected = !row.selected;
                            model.set_row_data(i, row);
                            break;
                        }
                    }
                }
            }
        });
    }
    {
        let cu_weak = check_updates_window.as_weak();
        check_updates_window.on_cancelled(move || {
            if let Some(cu) = cu_weak.upgrade() {
                let _ = cu.hide();
            }
        });
    }
    {
        let cu_weak = check_updates_window.as_weak();
        let ew = edit_window.as_weak();
        let pending = pending_updates.clone();
        let svc = svc.clone();
        check_updates_window.on_install_selected(move || {
            let (inst_id, kind, selected_items) = {
                let Ok(g) = pending.lock() else { return };
                let items: Vec<PendingUpdateItem> =
                    g.2.iter()
                        .filter(|x| x.selected)
                        .map(|x| PendingUpdateItem {
                            id: x.id.clone(),
                            old_file_name: x.old_file_name.clone(),
                            title: x.title.clone(),
                            homepage_url: x.homepage_url.clone(),
                            author: x.author.clone(),
                            description: x.description.clone(),
                            version: x.version.clone(),
                            selected: true,
                        })
                        .collect();
                (g.0.clone(), g.1, items)
            };
            if selected_items.is_empty() {
                return;
            }
            let Some((inst, _)) = find_instance_by_id(&svc, &inst_id) else {
                return;
            };
            if let Some(cu) = cu_weak.upgrade() {
                let _ = cu.hide();
            }
            let target_dir = content_target_dir(&svc, &inst, kind);
            let total = selected_items.len();
            let dm = svc.download_manager.clone();
            let ew = ew.clone();
            let svc = svc.clone();
            let rt = svc.rt.clone();

            TASK_PROGRESS_WIN.with(|cell| {
                if let Some(tp) = cell.borrow().as_ref().and_then(|w| w.upgrade()) {
                    tp.set_task_title(SharedString::from("Installing Updates"));
                    tp.set_step_text(SharedString::from(format!("Executing task 1 of {total}")));
                    tp.set_task_stage(SharedString::from("Downloading updates..."));
                    tp.set_overall_progress(0.0);
                    tp.set_progress(0.0);
                    tp.set_indeterminate(false);
                    tp.set_error_message(SharedString::new());
                    tp.set_is_cancelling(false);
                    let _ = tp.show();
                }
            });

            rt.spawn(async move {
                let cancel = CancellationToken::new();
                let mut last_err = None;
                for (idx, item) in selected_items.into_iter().enumerate() {
                    let step = idx + 1;
                    let title_clone = item.title.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        TASK_PROGRESS_WIN.with(|cell| {
                            if let Some(tp) = cell.borrow().as_ref().and_then(|w| w.upgrade()) {
                                tp.set_step_text(SharedString::from(format!(
                                    "Executing task {step} of {total}"
                                )));
                                tp.set_task_stage(SharedString::from(format!(
                                    "Updating {}...",
                                    title_clone
                                )));
                                tp.set_overall_progress((idx as f32) / (total.max(1) as f32));
                                tp.set_progress(0.0);
                            }
                        });
                    });
                    let total_size = item.version.file_size.unwrap_or(0);
                    let res = download_and_record_content_with_progress(
                        &dm,
                        &target_dir,
                        &item.version,
                        &item.title,
                        &item.homepage_url,
                        &item.author,
                        &item.description,
                        &cancel,
                        move |bytes| {
                            if total_size > 0 {
                                let frac = (bytes as f32 / total_size as f32).clamp(0.0, 1.0);
                                let _ = slint::invoke_from_event_loop(move || {
                                    TASK_PROGRESS_WIN.with(|cell| {
                                        if let Some(tp) =
                                            cell.borrow().as_ref().and_then(|w| w.upgrade())
                                        {
                                            tp.set_progress(frac);
                                        }
                                    });
                                });
                            }
                        },
                    )
                    .await;
                    match res {
                        Ok(_) => {
                            if item.old_file_name != item.version.file_name {
                                let _ = std::fs::remove_file(target_dir.join(&item.old_file_name));
                            }
                        }
                        Err(e) => last_err = Some(e.to_string()),
                    }
                }
                let _ = slint::invoke_from_event_loop(move || {
                    TASK_PROGRESS_WIN.with(|cell| {
                        if let Some(tp) = cell.borrow().as_ref().and_then(|w| w.upgrade()) {
                            if let Some(err) = last_err {
                                tp.set_error_message(SharedString::from(err));
                            } else {
                                tp.set_overall_progress(1.0);
                                tp.set_progress(1.0);
                                let _ = tp.hide();
                            }
                        }
                    });
                    if let Some(e) = ew.upgrade() {
                        populate_edit_instance_window(&e, &svc, &inst_id);
                    }
                });
            });
        });
    }

    // VerifyDependenciesWindow wiring (Mods)
    {
        let ew = edit_window.as_weak();
        let vd_win = Rc::clone(verify_deps_window);
        let pending_deps = pending_deps.clone();
        let svc = svc.clone();
        edit_window.on_verify_dependencies_clicked(move || {
            let Some(w) = ew.upgrade() else { return };
            let id = w.get_instance_id().to_string();
            let Some((inst, _)) = find_instance_by_id(&svc, &id) else {
                return;
            };
            if inst.modloader_spec.is_none() {
                return;
            }
            vd_win.set_is_checking(true);
            vd_win.set_status_text(SharedString::from("Verifying dependencies..."));
            vd_win.set_issues(ModelRc::from(Rc::new(VecModel::from(Vec::new()))));
            center_window_over(vd_win.window(), w.window());
            let _ = vd_win.show();

            let mods_dir = content_target_dir(&svc, &inst, ContentKind::Mod);
            let index = ModIndex::load(&mods_dir);
            if index.mods.is_empty() {
                vd_win.set_is_checking(false);
                vd_win.set_status_text(SharedString::from(
                    "No Modrinth/CurseForge metadata found for installed mods.",
                ));
                return;
            }
            let mc_ver = inst.minecraft_version.clone();
            let loader = inst.modloader_spec.as_ref().map(|s| s.kind);
            let client = svc.http_client.clone();
            let cache_dir = svc.paths.cache_dir.clone();
            let vd_weak = vd_win.as_weak();
            let pending_deps = pending_deps.clone();
            let inst_id = inst.id.clone();
            let rt = svc.rt.clone();

            rt.spawn(async move {
                let mut installed_ids =
                    index.installed_project_ids_verified(&mods_dir, ModProvider::Modrinth);
                installed_ids.extend(
                    index.installed_project_ids_verified(&mods_dir, ModProvider::CurseForge),
                );

                let mut required_by_map: HashMap<String, Vec<String>> = HashMap::new();
                let mut missing_versions: HashMap<String, ModRemoteVersion> = HashMap::new();

                for rec in &index.mods {
                    let versions = match rec.provider {
                        ModProvider::Modrinth => {
                            fetch_modrinth_versions(&client, &rec.project_id, &mc_ver, loader)
                                .await
                                .ok()
                        }
                        ModProvider::CurseForge => {
                            fetch_curseforge_versions(&client, &rec.project_id, &mc_ver, loader)
                                .await
                                .ok()
                        }
                        ModProvider::Local => None,
                    };
                    let Some(ver) = versions.and_then(|list| {
                        list.iter()
                            .find(|v| v.version_id == rec.version_id)
                            .cloned()
                            .or_else(|| list.into_iter().next())
                    }) else {
                        continue;
                    };
                    let parent_name = if rec.title.is_empty() {
                        rec.file_name.clone()
                    } else {
                        rec.title.clone()
                    };
                    for dep in &ver.required_dependencies {
                        if installed_ids.contains(&dep.project_id) {
                            continue;
                        }
                        required_by_map
                            .entry(dep.project_id.clone())
                            .or_default()
                            .push(parent_name.clone());
                        if !missing_versions.contains_key(&dep.project_id) {
                            let dep_vers = match dep.provider {
                                ModProvider::Modrinth => fetch_modrinth_versions(
                                    &client,
                                    &dep.project_id,
                                    &mc_ver,
                                    loader,
                                )
                                .await
                                .ok(),
                                ModProvider::CurseForge => fetch_curseforge_versions(
                                    &client,
                                    &dep.project_id,
                                    &mc_ver,
                                    loader,
                                )
                                .await
                                .ok(),
                                ModProvider::Local => None,
                            };
                            if let Some(best) = dep_vers.and_then(|l| l.into_iter().next()) {
                                missing_versions.insert(dep.project_id.clone(), best);
                            }
                        }
                    }
                }

                let mut found: Vec<(PendingDependencyItem, String, Option<PathBuf>)> = Vec::new();
                for (pid, ver) in missing_versions {
                    let req_by = required_by_map
                        .get(&pid)
                        .map(|v| v.join(", "))
                        .unwrap_or_default();
                    let icon_file = cache_dir.join("mod-icons").join(format!(
                        "{}-{}.png",
                        ver.provider.label().to_lowercase(),
                        pid
                    ));
                    let icon_path = icon_file.is_file().then_some(icon_file);
                    let homepage = match ver.provider {
                        ModProvider::Modrinth => format!("https://modrinth.com/mod/{pid}"),
                        _ => String::new(),
                    };
                    found.push((
                        PendingDependencyItem {
                            id: pid,
                            title: ver.name.clone(),
                            homepage_url: homepage,
                            version: ver,
                            selected: true,
                        },
                        req_by,
                        icon_path,
                    ));
                }

                let count = found.len();
                let _ = slint::invoke_from_event_loop(move || {
                    let ui_items: Vec<DependencyIssueItem> = found
                        .iter()
                        .map(|(item, req_by, ipath)| {
                            let (icon, has_icon) = ipath
                                .as_ref()
                                .and_then(|p| Image::load_from_path(p).ok())
                                .map(|img| (img, true))
                                .unwrap_or_else(|| (Image::default(), false));
                            DependencyIssueItem {
                                id: SharedString::from(&item.id),
                                name: SharedString::from(&item.title),
                                version: SharedString::from(&item.version.version_number),
                                status: SharedString::from("Missing"),
                                required_by: SharedString::from(req_by),
                                provider: SharedString::from(item.version.provider.label()),
                                selected: item.selected,
                                icon,
                                has_icon,
                            }
                        })
                        .collect();

                    if let Ok(mut g) = pending_deps.lock() {
                        *g = (
                            inst_id,
                            found.into_iter().map(|(item, _, _)| item).collect(),
                        );
                    }
                    if let Some(vd) = vd_weak.upgrade() {
                        vd.set_is_checking(false);
                        vd.set_issues(ModelRc::from(Rc::new(VecModel::from(ui_items))));
                        if count == 0 {
                            vd.set_status_text(SharedString::from(
                                "All required mod dependencies are satisfied.",
                            ));
                        } else {
                            vd.set_status_text(SharedString::from(format!(
                                "{count} missing required dependency(ies) found."
                            )));
                        }
                    }
                });
            });
        });
    }
    {
        let vd_weak = verify_deps_window.as_weak();
        let pending = pending_deps.clone();
        verify_deps_window.on_toggle_item(move |id| {
            let id_s = id.to_string();
            if let Ok(mut g) = pending.lock() {
                if let Some(item) = g.1.iter_mut().find(|x| x.id == id_s) {
                    item.selected = !item.selected;
                }
            }
            if let Some(vd) = vd_weak.upgrade() {
                let model = vd.get_issues();
                for i in 0..model.row_count() {
                    if let Some(mut row) = model.row_data(i) {
                        if row.id == id {
                            row.selected = !row.selected;
                            model.set_row_data(i, row);
                            break;
                        }
                    }
                }
            }
        });
    }
    {
        let vd_weak = verify_deps_window.as_weak();
        verify_deps_window.on_cancelled(move || {
            if let Some(vd) = vd_weak.upgrade() {
                let _ = vd.hide();
            }
        });
    }
    {
        let vd_weak = verify_deps_window.as_weak();
        let ew = edit_window.as_weak();
        let pending = pending_deps.clone();
        let svc = svc.clone();
        verify_deps_window.on_install_selected(move || {
            let (inst_id, selected_items) = {
                let Ok(g) = pending.lock() else { return };
                let items: Vec<PendingDependencyItem> =
                    g.1.iter()
                        .filter(|x| x.selected)
                        .map(|x| PendingDependencyItem {
                            id: x.id.clone(),
                            title: x.title.clone(),
                            homepage_url: x.homepage_url.clone(),
                            version: x.version.clone(),
                            selected: true,
                        })
                        .collect();
                (g.0.clone(), items)
            };
            if selected_items.is_empty() {
                return;
            }
            let Some((inst, _)) = find_instance_by_id(&svc, &inst_id) else {
                return;
            };
            if let Some(vd) = vd_weak.upgrade() {
                let _ = vd.hide();
            }
            let target_dir = content_target_dir(&svc, &inst, ContentKind::Mod);
            let total = selected_items.len();
            let dm = svc.download_manager.clone();
            let ew = ew.clone();
            let svc = svc.clone();
            let rt = svc.rt.clone();

            TASK_PROGRESS_WIN.with(|cell| {
                if let Some(tp) = cell.borrow().as_ref().and_then(|w| w.upgrade()) {
                    tp.set_task_title(SharedString::from("Installing Dependencies"));
                    tp.set_step_text(SharedString::from(format!("Executing task 1 of {total}")));
                    tp.set_task_stage(SharedString::from("Downloading dependencies..."));
                    tp.set_overall_progress(0.0);
                    tp.set_progress(0.0);
                    tp.set_indeterminate(false);
                    tp.set_error_message(SharedString::new());
                    tp.set_is_cancelling(false);
                    let _ = tp.show();
                }
            });

            rt.spawn(async move {
                let cancel = CancellationToken::new();
                let mut last_err = None;
                for (idx, item) in selected_items.into_iter().enumerate() {
                    let step = idx + 1;
                    let title_clone = item.title.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        TASK_PROGRESS_WIN.with(|cell| {
                            if let Some(tp) = cell.borrow().as_ref().and_then(|w| w.upgrade()) {
                                tp.set_step_text(SharedString::from(format!(
                                    "Executing task {step} of {total}"
                                )));
                                tp.set_task_stage(SharedString::from(format!(
                                    "Downloading {}...",
                                    title_clone
                                )));
                                tp.set_overall_progress((idx as f32) / (total.max(1) as f32));
                                tp.set_progress(0.0);
                            }
                        });
                    });
                    let total_size = item.version.file_size.unwrap_or(0);
                    if let Err(e) = download_and_record_content_with_progress(
                        &dm,
                        &target_dir,
                        &item.version,
                        &item.title,
                        &item.homepage_url,
                        "",
                        "",
                        &cancel,
                        move |bytes| {
                            if total_size > 0 {
                                let frac = (bytes as f32 / total_size as f32).clamp(0.0, 1.0);
                                let _ = slint::invoke_from_event_loop(move || {
                                    TASK_PROGRESS_WIN.with(|cell| {
                                        if let Some(tp) =
                                            cell.borrow().as_ref().and_then(|w| w.upgrade())
                                        {
                                            tp.set_progress(frac);
                                        }
                                    });
                                });
                            }
                        },
                    )
                    .await
                    {
                        last_err = Some(e.to_string());
                    }
                }
                let _ = slint::invoke_from_event_loop(move || {
                    TASK_PROGRESS_WIN.with(|cell| {
                        if let Some(tp) = cell.borrow().as_ref().and_then(|w| w.upgrade()) {
                            if let Some(err) = last_err {
                                tp.set_error_message(SharedString::from(err));
                            } else {
                                tp.set_overall_progress(1.0);
                                tp.set_progress(1.0);
                                let _ = tp.hide();
                            }
                        }
                    });
                    if let Some(e) = ew.upgrade() {
                        populate_edit_instance_window(&e, &svc, &inst_id);
                    }
                });
            });
        });
    }

    // EditInstanceWindow -> abrir Downloader Unificado (Mods, Resource Packs, Shaders)
    for kind in [
        ContentKind::Mod,
        ContentKind::ResourcePack,
        ContentKind::ShaderPack,
    ] {
        let ew = edit_window.as_weak();
        let dm_win = Rc::clone(dm_window);
        let session = session.clone();
        let svc = svc.clone();
        let handler = move || {
            let Some(w) = ew.upgrade() else { return };
            let inst_id = w.get_instance_id().to_string();
            let Some((inst, _)) = find_instance_by_id(&svc, &inst_id) else {
                return;
            };
            open_content_downloader(&dm_win, &session, &svc, &inst, kind, "");
        };
        match kind {
            ContentKind::Mod => edit_window.on_download_mods_clicked(handler),
            ContentKind::ResourcePack => edit_window.on_download_texture_packs_clicked(handler),
            ContentKind::ShaderPack => edit_window.on_download_shader_packs_clicked(handler),
        }
    }

    // EditInstanceWindow -> Check for Updates (abre CheckUpdatesWindow para Mods, Resource Packs, Shaders)
    for kind in [
        ContentKind::Mod,
        ContentKind::ResourcePack,
        ContentKind::ShaderPack,
    ] {
        let ew = edit_window.as_weak();
        let cu_win = Rc::clone(check_updates_window);
        let pending = pending_updates.clone();
        let svc = svc.clone();
        let handler = move || open_check_updates_window(&ew, &cu_win, &pending, &svc, kind);
        match kind {
            ContentKind::Mod => edit_window.on_check_mod_updates_clicked(handler),
            ContentKind::ResourcePack => edit_window.on_check_texture_pack_updates_clicked(handler),
            ContentKind::ShaderPack => edit_window.on_check_shader_pack_updates_clicked(handler),
        }
    }

    // EditInstanceWindow -> Change Version (abre downloader unificado focado no item)
    for kind in [
        ContentKind::Mod,
        ContentKind::ResourcePack,
        ContentKind::ShaderPack,
    ] {
        let ew = edit_window.as_weak();
        let dm_win = Rc::clone(dm_window);
        let session = session.clone();
        let svc = svc.clone();
        let handler = move |fname: SharedString| {
            let Some(w) = ew.upgrade() else { return };
            let id = w.get_instance_id().to_string();
            let Some((inst, _)) = find_instance_by_id(&svc, &id) else {
                return;
            };
            let target_dir = content_target_dir(&svc, &inst, kind);
            let idx = ModIndex::load(&target_dir);
            let search_term = idx
                .find_by_file(fname.as_str())
                .map(|r| r.title.clone())
                .unwrap_or_else(|| {
                    fname
                        .trim_end_matches(".disabled")
                        .trim_end_matches(".jar")
                        .trim_end_matches(".zip")
                        .to_string()
                });
            open_content_downloader(&dm_win, &session, &svc, &inst, kind, &search_term);
        };
        match kind {
            ContentKind::Mod => edit_window.on_change_mod_version_clicked(handler),
            ContentKind::ResourcePack => {
                edit_window.on_change_texture_pack_version_clicked(handler)
            }
            ContentKind::ShaderPack => edit_window.on_change_shader_pack_version_clicked(handler),
        }
    }

    // EditInstanceWindow -> View Homepage (Resource Packs e Shaders)
    for kind in [ContentKind::ResourcePack, ContentKind::ShaderPack] {
        let ew = edit_window.as_weak();
        let svc = svc.clone();
        let handler = move |fname: SharedString| {
            let Some(w) = ew.upgrade() else { return };
            let id = w.get_instance_id().to_string();
            let Some((inst, _)) = find_instance_by_id(&svc, &id) else {
                return;
            };
            let target_dir = content_target_dir(&svc, &inst, kind);
            let idx = ModIndex::load(&target_dir);
            if let Some(rec) = idx.find_by_file(fname.as_str()) {
                if !rec.homepage_url.is_empty() {
                    let _ = open::that(&rec.homepage_url);
                    return;
                }
            }
            w.set_error_message(SharedString::from(
                "No homepage URL recorded for this local pack.",
            ));
        };
        match kind {
            ContentKind::ResourcePack => edit_window.on_view_texture_pack_homepage_clicked(handler),
            ContentKind::ShaderPack => edit_window.on_view_shader_pack_homepage_clicked(handler),
            ContentKind::Mod => {}
        }
    }

    // EditInstanceWindow -> Add File (.jar via zenity)
    {
        let ew = edit_window.as_weak();
        let svc = svc.clone();
        edit_window.on_add_mod_file_clicked(move || {
            let Some(w) = ew.upgrade() else { return };
            let id = w.get_instance_id().to_string();
            let Some((inst, _)) = find_instance_by_id(&svc, &id) else {
                return;
            };
            let mods_dir = svc.paths.instance_game_dir(&inst.dir_name).join("mods");
            let ew = ew.clone();
            let svc = svc.clone();
            let rt = svc.rt.clone();
            rt.spawn(async move {
                let res = tokio::task::spawn_blocking(move || {
                    let out = std::process::Command::new("zenity")
                        .args([
                            "--file-selection",
                            "--title=Add Mod JAR",
                            "--file-filter=Mod JAR (*.jar) | *.jar",
                        ])
                        .output()
                        .ok()?;
                    if !out.status.success() {
                        return None;
                    }
                    let src = PathBuf::from(String::from_utf8_lossy(&out.stdout).trim());
                    let fname = src.file_name()?;
                    let _ = std::fs::create_dir_all(&mods_dir);
                    std::fs::copy(&src, mods_dir.join(fname)).ok()?;
                    Some(())
                })
                .await;
                if let Ok(Some(())) = res {
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(w) = ew.upgrade() {
                            populate_edit_instance_window(&w, &svc, &id);
                        }
                    });
                }
            });
        });
    }

    // EditInstanceWindow -> View Homepage / Export List / Open Configs Folder
    {
        let ew = edit_window.as_weak();
        let svc = svc.clone();
        edit_window.on_view_mod_homepage_clicked(move |fname| {
            let Some(w) = ew.upgrade() else { return };
            let id = w.get_instance_id().to_string();
            let Some((inst, _)) = find_instance_by_id(&svc, &id) else {
                return;
            };
            let mods_dir = svc.paths.instance_game_dir(&inst.dir_name).join("mods");
            let idx = ModIndex::load(&mods_dir);
            if let Some(rec) = idx.find_by_file(fname.as_str()) {
                if !rec.homepage_url.is_empty() {
                    let _ = open::that(&rec.homepage_url);
                    return;
                }
            }
            w.set_error_message(SharedString::from(
                "No homepage URL recorded for this local mod.",
            ));
        });
    }
    {
        let ew = edit_window.as_weak();
        let svc = svc.clone();
        edit_window.on_export_mods_list_clicked(move || {
            let Some(w) = ew.upgrade() else { return };
            let id = w.get_instance_id().to_string();
            let Some((inst, _)) = find_instance_by_id(&svc, &id) else {
                return;
            };
            let mods_dir = svc.paths.instance_game_dir(&inst.dir_name).join("mods");
            let idx = ModIndex::load(&mods_dir);
            let rows: Vec<(String, String, String, bool)> =
                list_local_mods(&mods_dir, inst.modloader_spec.as_ref().map(|s| s.kind))
                    .into_iter()
                    .map(|m| {
                        let prov = idx
                            .find_by_file(&m.file_name)
                            .map(|r| r.provider.label().to_string())
                            .unwrap_or_else(|| "Local".into());
                        (m.name, m.version, prov, m.enabled)
                    })
                    .collect();
            let md = export_mods_list_markdown(&inst.name, &rows);
            let out_path = svc
                .paths
                .instance_game_dir(&inst.dir_name)
                .join("mods-list.md");
            if std::fs::write(&out_path, md).is_ok() {
                let _ = open_in_file_manager(&out_path);
            }
        });
    }
    {
        let ew = edit_window.as_weak();
        let svc = svc.clone();
        edit_window.on_open_configs_folder_clicked(move || {
            let Some(w) = ew.upgrade() else { return };
            let id = w.get_instance_id().to_string();
            if let Some((inst, _)) = find_instance_by_id(&svc, &id) {
                let cfg_dir = svc.paths.instance_game_dir(&inst.dir_name).join("config");
                let _ = std::fs::create_dir_all(&cfg_dir);
                let _ = open_in_file_manager(&cfg_dir);
            }
        });
    }

    // DownloadModsWindow callbacks
    {
        let dm_weak = dm_window.as_weak();
        let session = session.clone();
        let svc = svc.clone();
        dm_window.on_provider_changed(move |_| {
            trigger_mod_platform_search(&dm_weak, &session, &svc);
        });
    }
    {
        let dm_weak = dm_window.as_weak();
        let session = session.clone();
        let svc = svc.clone();
        dm_window.on_search_edited(move || {
            let gen = CONTENT_SEARCH_GEN.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
            let dm_weak = dm_weak.clone();
            let session = session.clone();
            let rt = svc.rt.clone();
            let svc = svc.clone();
            rt.spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                if CONTENT_SEARCH_GEN.load(std::sync::atomic::Ordering::SeqCst) != gen {
                    return;
                }
                let _ = slint::invoke_from_event_loop(move || {
                    trigger_mod_platform_search(&dm_weak, &session, &svc);
                });
            });
        });
    }
    {
        let dm_weak = dm_window.as_weak();
        let session = session.clone();
        let svc = svc.clone();
        dm_window.on_search_triggered(move || {
            trigger_mod_platform_search(&dm_weak, &session, &svc);
        });
    }
    {
        let dm_weak = dm_window.as_weak();
        let session = session.clone();
        let svc = svc.clone();
        dm_window.on_load_more_triggered(move || {
            trigger_mod_platform_search_page(&dm_weak, &session, &svc, true);
        });
    }
    {
        let dm_weak = dm_window.as_weak();
        let session = session.clone();
        let svc = svc.clone();
        dm_window.on_sort_changed(move |sid| {
            if let Some(w) = dm_weak.upgrade() {
                let lbl = match sid.as_str() {
                    "downloads" => "Sort by Downloads",
                    "updated" => "Sort by Updated",
                    "newest" => "Sort by Newest",
                    _ => "Sort by Relevance",
                };
                w.set_sort_id(sid);
                w.set_sort_label(SharedString::from(lbl));
            }
            trigger_mod_platform_search(&dm_weak, &session, &svc);
        });
    }
    {
        let dm_weak = dm_window.as_weak();
        let session = session.clone();
        let svc = svc.clone();
        dm_window.on_mod_selected(move |pid| {
            let Some(w) = dm_weak.upgrade() else { return };
            let inst_id = w.get_instance_id().to_string();
            let Some((inst, _)) = find_instance_by_id(&svc, &inst_id) else {
                return;
            };
            let hit_opt = session.lock().ok().and_then(|g| {
                g.results
                    .iter()
                    .find(|r| r.project_id == pid.as_str())
                    .cloned()
            });
            let Some(hit) = hit_opt else { return };
            let queued = session
                .lock()
                .map(|g| g.queue.iter().any(|(m, _)| m.project_id == hit.project_id))
                .unwrap_or(false);
            w.set_detail_title(SharedString::from(&hit.title));
            w.set_detail_author(SharedString::from(&hit.author));
            w.set_detail_description(SharedString::from(&hit.description));
            w.set_detail_body(SharedString::from("Loading project details..."));
            w.set_detail_homepage(SharedString::from(&hit.homepage_url));
            w.set_detail_issues_url(SharedString::new());
            w.set_detail_source_url(SharedString::new());
            w.set_detail_discord_url(SharedString::new());
            w.set_detail_wiki_url(SharedString::new());
            w.set_detail_installed(hit.installed);
            w.set_detail_queued(queued);
            w.set_selected_version_label(SharedString::from("Loading versions..."));
            w.set_version_options(ModelRc::from(Rc::new(VecModel::from(Vec::new()))));

            let kind = ContentKind::from_id(w.get_content_kind().as_str());
            let mc_ver = inst.minecraft_version.clone();
            let loader = if kind.uses_modloader_filter() {
                inst.modloader_spec.as_ref().map(|s| s.kind)
            } else {
                None
            };
            let client = svc.http_client.clone();
            let cache_dir = svc.paths.cache_dir.clone();
            let dm_weak = dm_weak.clone();
            let session = session.clone();
            let rt = svc.rt.clone();
            rt.spawn(async move {
                let details_opt = if hit.provider == ModProvider::Modrinth {
                    fetch_modrinth_project_details(&client, &hit.project_id)
                        .await
                        .ok()
                } else {
                    None
                };
                let rich_blocks_with_imgs = if let Some(ref d) = details_opt {
                    let parsed = parse_rich_description(&d.body_markdown);
                    let img_dir = cache_dir.join("mod-images");
                    let mut img_count = 0usize;
                    let mut out = Vec::with_capacity(parsed.len());
                    for blk in parsed {
                        let cached_path = match &blk {
                            RichBlock::Image { image_url, .. } if img_count < 6 => {
                                img_count += 1;
                                let key = format!("{}-img-{}", hit.project_id, img_count);
                                fetch_and_cache_remote_image(&client, &img_dir, &key, image_url)
                                    .await
                            }
                            _ => None,
                        };
                        out.push((blk, cached_path));
                    }
                    Some(out)
                } else {
                    None
                };
                let res = match hit.provider {
                    ModProvider::Modrinth => {
                        fetch_modrinth_versions(&client, &hit.project_id, &mc_ver, loader).await
                    }
                    ModProvider::CurseForge => {
                        fetch_curseforge_versions(&client, &hit.project_id, &mc_ver, loader).await
                    }
                    ModProvider::Local => Ok(Vec::new()),
                };
                let _ = slint::invoke_from_event_loop(move || {
                    let Some(w) = dm_weak.upgrade() else { return };
                    if let Some(d) = details_opt {
                        w.set_detail_body(SharedString::from(render_markdown_plain(
                            &d.body_markdown,
                        )));
                        w.set_detail_issues_url(SharedString::from(
                            d.issues_url.unwrap_or_default(),
                        ));
                        w.set_detail_source_url(SharedString::from(
                            d.source_url.unwrap_or_default(),
                        ));
                        w.set_detail_discord_url(SharedString::from(
                            d.discord_url.unwrap_or_default(),
                        ));
                        w.set_detail_wiki_url(SharedString::from(d.wiki_url.unwrap_or_default()));
                        if let Some(blocks) = rich_blocks_with_imgs {
                            let ui_blocks: Vec<DescriptionBlockItem> = blocks
                                .into_iter()
                                .map(|(blk, img_path)| match blk {
                                    RichBlock::Heading(t) => DescriptionBlockItem {
                                        kind: SharedString::from("heading"),
                                        text: SharedString::from(t),
                                        url: SharedString::new(),
                                        image: Image::default(),
                                        has_image: false,
                                    },
                                    RichBlock::Subheading(t) => DescriptionBlockItem {
                                        kind: SharedString::from("subheading"),
                                        text: SharedString::from(t),
                                        url: SharedString::new(),
                                        image: Image::default(),
                                        has_image: false,
                                    },
                                    RichBlock::Paragraph(t) => DescriptionBlockItem {
                                        kind: SharedString::from("paragraph"),
                                        text: SharedString::from(t),
                                        url: SharedString::new(),
                                        image: Image::default(),
                                        has_image: false,
                                    },
                                    RichBlock::Bullet(t) => DescriptionBlockItem {
                                        kind: SharedString::from("bullet"),
                                        text: SharedString::from(t),
                                        url: SharedString::new(),
                                        image: Image::default(),
                                        has_image: false,
                                    },
                                    RichBlock::Link { label, url } => DescriptionBlockItem {
                                        kind: SharedString::from("link"),
                                        text: SharedString::from(label),
                                        url: SharedString::from(url),
                                        image: Image::default(),
                                        has_image: false,
                                    },
                                    RichBlock::Image { link_url, alt, .. } => {
                                        let (img, has) = img_path
                                            .and_then(|p| Image::load_from_path(&p).ok())
                                            .map(|i| (i, true))
                                            .unwrap_or_else(|| (Image::default(), false));
                                        DescriptionBlockItem {
                                            kind: SharedString::from("image"),
                                            text: SharedString::from(alt),
                                            url: SharedString::from(link_url.unwrap_or_default()),
                                            image: img,
                                            has_image: has,
                                        }
                                    }
                                    RichBlock::Separator => DescriptionBlockItem {
                                        kind: SharedString::from("separator"),
                                        text: SharedString::new(),
                                        url: SharedString::new(),
                                        image: Image::default(),
                                        has_image: false,
                                    },
                                })
                                .collect();
                            w.set_detail_blocks(ModelRc::from(Rc::new(VecModel::from(ui_blocks))));
                        }
                    } else {
                        w.set_detail_body(SharedString::from(&hit.description));
                        w.set_detail_blocks(ModelRc::from(Rc::new(VecModel::from(Vec::new()))));
                    }
                    if let Ok(versions) = res {
                        let opts: Vec<DropdownOption> = versions
                            .iter()
                            .map(|v| DropdownOption {
                                id: SharedString::from(&v.version_id),
                                label: SharedString::from(&v.version_number),
                                detail: SharedString::new(),
                                enabled: true,
                                icon: Image::default(),
                                has_icon: false,
                            })
                            .collect();
                        let first_id = versions
                            .first()
                            .map(|v| v.version_id.clone())
                            .unwrap_or_default();
                        let first_lbl = versions
                            .first()
                            .map(|v| v.version_number.clone())
                            .unwrap_or_else(|| "No compatible versions".into());
                        if let Ok(mut g) = session.lock() {
                            g.versions = versions;
                        }
                        w.set_version_options(ModelRc::from(Rc::new(VecModel::from(opts))));
                        w.set_selected_version_id(SharedString::from(&first_id));
                        w.set_selected_version_label(SharedString::from(&first_lbl));
                    }
                });
            });
        });
    }
    {
        let dm_weak = dm_window.as_weak();
        let session = session.clone();
        dm_window.on_version_selected(move |vid| {
            if let Some(w) = dm_weak.upgrade() {
                if let Ok(g) = session.lock() {
                    if let Some(v) = g.versions.iter().find(|x| x.version_id == vid.as_str()) {
                        w.set_selected_version_label(SharedString::from(&v.version_number));
                    }
                }
            }
        });
    }
    {
        let dm_weak = dm_window.as_weak();
        let session = session.clone();
        let svc = svc.clone();
        dm_window.on_toggle_queue_clicked(move |pid| {
            let Some(w) = dm_weak.upgrade() else { return };
            let vid = w.get_selected_version_id().to_string();
            let pid_str = pid.to_string();
            let (already_removed, hit_opt, matched_ver, queued_len) = {
                let Ok(mut g) = session.lock() else { return };
                if let Some(pos) = g.queue.iter().position(|(m, _)| m.project_id == pid_str) {
                    g.queue.remove(pos);
                    (true, None, None, g.queue.len())
                } else {
                    let hit = g.results.iter().find(|r| r.project_id == pid_str).cloned();
                    // Somente usa versão já carregada se pertencer estritamente ao mesmo project_id
                    let ver = g
                        .versions
                        .iter()
                        .find(|v| v.project_id == pid_str && v.version_id == vid)
                        .or_else(|| g.versions.iter().find(|v| v.project_id == pid_str))
                        .cloned();
                    if let (Some(h), Some(v)) = (&hit, &ver) {
                        g.queue.push((h.clone(), v.clone()));
                    }
                    (false, hit, ver, g.queue.len())
                }
            };

            if already_removed {
                if w.get_selected_project_id().as_str() == pid_str {
                    w.set_detail_queued(false);
                }
                w.set_queued_count(queued_len as i32);
                let results_rc = w.get_results();
                for i in 0..results_rc.row_count() {
                    if let Some(mut item) = results_rc.row_data(i) {
                        if item.project_id == pid {
                            item.selected_for_download = false;
                            results_rc.set_row_data(i, item);
                            break;
                        }
                    }
                }
                return;
            }

            if matched_ver.is_some() {
                if w.get_selected_project_id().as_str() == pid_str {
                    w.set_detail_queued(true);
                }
                w.set_queued_count(queued_len as i32);
                let results_rc = w.get_results();
                for i in 0..results_rc.row_count() {
                    if let Some(mut item) = results_rc.row_data(i) {
                        if item.project_id == pid {
                            item.selected_for_download = true;
                            results_rc.set_row_data(i, item);
                            break;
                        }
                    }
                }
                return;
            }

            // Se o usuário marcou a checkbox diretamente na lista sem selecionar o item antes,
            // busca assincronamente a versão compatível específica daquele project_id
            let Some(hit) = hit_opt else { return };
            let inst_id = w.get_instance_id().to_string();
            let Some((inst, _)) = find_instance_by_id(&svc, &inst_id) else {
                return;
            };
            let kind = ContentKind::from_id(w.get_content_kind().as_str());
            let mc_ver = inst.minecraft_version.clone();
            let loader = if kind.uses_modloader_filter() {
                inst.modloader_spec.as_ref().map(|s| s.kind)
            } else {
                None
            };
            let client = svc.http_client.clone();
            let dm_weak = dm_weak.clone();
            let session = session.clone();
            let rt = svc.rt.clone();
            rt.spawn(async move {
                let versions = match hit.provider {
                    ModProvider::Modrinth => {
                        fetch_modrinth_versions(&client, &hit.project_id, &mc_ver, loader)
                            .await
                            .ok()
                    }
                    ModProvider::CurseForge => {
                        fetch_curseforge_versions(&client, &hit.project_id, &mc_ver, loader)
                            .await
                            .ok()
                    }
                    ModProvider::Local => None,
                };
                if let Some(best) = versions.and_then(|v| v.into_iter().next()) {
                    let pid_clone = hit.project_id.clone();
                    let new_len = if let Ok(mut g) = session.lock() {
                        if !g.queue.iter().any(|(m, _)| m.project_id == pid_clone) {
                            g.queue.push((hit, best));
                        }
                        g.queue.len()
                    } else {
                        0
                    };
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(w) = dm_weak.upgrade() {
                            w.set_queued_count(new_len as i32);
                            if w.get_selected_project_id().as_str() == pid_clone {
                                w.set_detail_queued(true);
                            }
                            let results_rc = w.get_results();
                            for i in 0..results_rc.row_count() {
                                if let Some(mut item) = results_rc.row_data(i) {
                                    if item.project_id.as_str() == pid_clone {
                                        item.selected_for_download = true;
                                        results_rc.set_row_data(i, item);
                                        break;
                                    }
                                }
                            }
                        }
                    });
                }
            });
        });
    }
    {
        dm_window.on_open_homepage_clicked(move |url| {
            let _ = open::that(url.as_str());
        });
    }
    {
        let dm_weak = dm_window.as_weak();
        let session = session.clone();
        let svc = svc.clone();
        dm_window.on_review_clicked(move || {
            let Some(w) = dm_weak.upgrade() else { return };
            let inst_id = w.get_instance_id().to_string();
            let Some((inst, _)) = find_instance_by_id(&svc, &inst_id) else {
                return;
            };
            let queue_snapshot = session.lock().map(|g| g.queue.clone()).unwrap_or_default();
            if queue_snapshot.is_empty() {
                return;
            }
            let kind = ContentKind::from_id(w.get_content_kind().as_str());
            let mc_ver = inst.minecraft_version.clone();
            let loader = if kind.uses_modloader_filter() {
                inst.modloader_spec.as_ref().map(|s| s.kind)
            } else {
                None
            };
            let target_dir = content_target_dir(&svc, &inst, kind);
            let idx = ModIndex::load(&target_dir);
            let mut installed_ids = idx.installed_project_ids(ModProvider::Modrinth);
            installed_ids.extend(idx.installed_project_ids(ModProvider::CurseForge));

            let client = svc.http_client.clone();
            let cache_dir_ui = svc.paths.cache_dir.clone();
            let dm_weak = dm_weak.clone();
            let session = session.clone();
            let rt = svc.rt.clone();
            rt.spawn(async move {
                let selected_versions: Vec<ModRemoteVersion> =
                    queue_snapshot.iter().map(|(_, v)| v.clone()).collect();
                let deps = resolve_required_dependencies(
                    &client,
                    &selected_versions,
                    &mc_ver,
                    loader,
                    &installed_ids,
                )
                .await;

                let mut plan = Vec::new();
                let mut ui_items = Vec::new();
                for (m, v) in queue_snapshot {
                    ui_items.push(ReviewModItem {
                        title: SharedString::from(&m.title),
                        version_number: SharedString::from(&v.version_number),
                        file_name: SharedString::from(&v.file_name),
                        is_dependency: false,
                    });
                    plan.push((m.title, v, m.homepage_url, false));
                }
                for d in deps {
                    ui_items.push(ReviewModItem {
                        title: SharedString::from(&d.name),
                        version_number: SharedString::from(&d.version_number),
                        file_name: SharedString::from(&d.file_name),
                        is_dependency: true,
                    });
                    plan.push((d.name.clone(), d, String::new(), true));
                }
                let plan_for_ui: Vec<(String, String, String, String, bool)> = plan
                    .iter()
                    .map(|(title, ver, _, is_dep)| {
                        (
                            title.clone(),
                            ver.version_number.clone(),
                            ver.provider.label().to_string(),
                            ver.project_id.clone(),
                            *is_dep,
                        )
                    })
                    .collect();
                if let Ok(mut g) = session.lock() {
                    g.resolved_plan = plan;
                }
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(w) = dm_weak.upgrade() {
                        w.set_review_items(ModelRc::from(Rc::new(VecModel::from(ui_items))));
                        w.set_show_review_modal(true);
                        REVIEW_MODS_WIN.with(|cell| {
                            if let Some(rw) = cell.borrow().as_ref().and_then(|x| x.upgrade()) {
                                let items: Vec<ReviewItemEntry> = plan_for_ui
                                    .iter()
                                    .map(|(title, version, provider, pid, is_dep)| {
                                        let icon_file = cache_dir_ui.join("mod-icons").join(
                                            format!("{}-{}.png", provider.to_lowercase(), pid),
                                        );
                                        let (icon, has_icon) = if icon_file.is_file() {
                                            Image::load_from_path(&icon_file)
                                                .map(|img| (img, true))
                                                .unwrap_or_else(|_| (Image::default(), false))
                                        } else {
                                            (Image::default(), false)
                                        };
                                        ReviewItemEntry {
                                            title: SharedString::from(title),
                                            version: SharedString::from(version),
                                            provider: SharedString::from(provider),
                                            is_dependency: *is_dep,
                                            icon,
                                            has_icon,
                                        }
                                    })
                                    .collect();
                                rw.set_review_items(ModelRc::from(Rc::new(VecModel::from(items))));
                                rw.set_is_installing(false);
                                center_window_over(rw.window(), w.window());
                                let _ = rw.show();
                            }
                        });
                    }
                });
            });
        });
    }
    {
        let ew = edit_window.as_weak();
        let dm_weak = dm_window.as_weak();
        let session = session.clone();
        let svc = svc.clone();
        dm_window.on_confirm_download_clicked(move || {
            let Some(w) = dm_weak.upgrade() else { return };
            let inst_id = w.get_instance_id().to_string();
            let Some((inst, _)) = find_instance_by_id(&svc, &inst_id) else {
                return;
            };
            let plan = session
                .lock()
                .map(|g| g.resolved_plan.clone())
                .unwrap_or_default();
            if plan.is_empty() {
                return;
            }
            let kind = ContentKind::from_id(w.get_content_kind().as_str());
            let meta_by_pid: std::collections::HashMap<String, (String, String)> = session
                .lock()
                .map(|g| {
                    g.queue
                        .iter()
                        .map(|(m, _)| {
                            (
                                m.project_id.clone(),
                                (m.author.clone(), m.description.clone()),
                            )
                        })
                        .collect()
                })
                .unwrap_or_default();
            w.set_is_downloading(true);
            w.set_status_message(SharedString::from("Downloading selected content..."));

            let total = plan.len();
            let window_title = kind.window_title();
            let cache_dir_tp = svc.paths.cache_dir.clone();
            let initial_rows: Vec<DownloadTaskRow> = plan
                .iter()
                .map(|(title, ver, _, _)| {
                    let prov = ver.provider.label();
                    let icon_file = cache_dir_tp.join("mod-icons").join(format!(
                        "{}-{}.png",
                        prov.to_lowercase(),
                        ver.project_id
                    ));
                    let (icon, has_icon) = if icon_file.is_file() {
                        Image::load_from_path(&icon_file)
                            .map(|img| (img, true))
                            .unwrap_or_else(|_| (Image::default(), false))
                    } else {
                        (Image::default(), false)
                    };
                    DownloadTaskRow {
                        id: SharedString::from(&ver.project_id),
                        name: SharedString::from(title),
                        provider: SharedString::from(prov),
                        icon,
                        has_icon,
                        progress: 0.0,
                        status: SharedString::from("Queued"),
                        bytes_text: SharedString::new(),
                    }
                })
                .collect();

            let _op_id = next_task_op_id();
            let cancel = CancellationToken::new();
            if let Ok(mut st) = svc.state.lock() {
                st.active_install_cancel = Some(cancel.clone());
            }

            TASK_PROGRESS_WIN.with(|cell| {
                if let Some(tp) = cell.borrow().as_ref().and_then(|tw| tw.upgrade()) {
                    tp.set_task_title(SharedString::from(window_title));
                    tp.set_step_text(SharedString::from(format!("Executing task 1 of {total}")));
                    tp.set_task_stage(SharedString::from("Downloading selected content..."));
                    tp.set_overall_progress(0.0);
                    tp.set_overall_percent_text(SharedString::from("0%"));
                    tp.set_progress(0.0);
                    tp.set_indeterminate(false);
                    tp.set_is_paused(false);
                    tp.set_is_cancelling(false);
                    tp.set_is_finished(false);
                    tp.set_error_message(SharedString::new());
                    tp.set_completed_count(0);
                    tp.set_total_count(total as i32);
                    tp.set_download_items(ModelRc::from(Rc::new(VecModel::from(initial_rows))));
                    center_window_over(tp.window(), w.window());
                    let _ = tp.show();
                }
            });

            let target_dir = content_target_dir(&svc, &inst, kind);
            let dm = svc.download_manager.clone();
            let dm_weak = dm_weak.clone();
            let ew = ew.clone();
            let session = session.clone();
            let svc = svc.clone();
            let rt = svc.rt.clone();
            rt.spawn(async move {
                let mut ok_count = 0usize;
                let mut last_err = None;
                for (idx, (title, ver, homepage, _)) in plan.into_iter().enumerate() {
                    while TASK_PAUSED.load(std::sync::atomic::Ordering::Relaxed)
                        && !cancel.is_cancelled()
                    {
                        tokio::time::sleep(std::time::Duration::from_millis(120)).await;
                    }
                    if cancel.is_cancelled() {
                        last_err = Some("Download aborted by user.".to_string());
                        break;
                    }
                    let step = idx + 1;
                    let title_ui = title.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        TASK_PROGRESS_WIN.with(|cell| {
                            if let Some(tp) = cell.borrow().as_ref().and_then(|tw| tw.upgrade()) {
                                tp.set_step_text(SharedString::from(format!(
                                    "Executing task {step} of {total}"
                                )));
                                tp.set_task_stage(SharedString::from(format!(
                                    "Downloading {}...",
                                    title_ui
                                )));
                                let model = tp.get_download_items();
                                if let Some(mut row) = model.row_data(idx) {
                                    row.status = SharedString::new();
                                    model.set_row_data(idx, row);
                                }
                            }
                        });
                    });
                    let (author, desc) = meta_by_pid
                        .get(&ver.project_id)
                        .cloned()
                        .unwrap_or_default();
                    let total_size = ver.file_size.unwrap_or(0);
                    let res = download_and_record_content_with_progress(
                        &dm,
                        &target_dir,
                        &ver,
                        &title,
                        &homepage,
                        &author,
                        &desc,
                        &cancel,
                        move |bytes| {
                            let frac = if total_size > 0 {
                                (bytes as f32 / total_size as f32).clamp(0.0, 1.0)
                            } else {
                                0.5
                            };
                            let overall = ((idx as f32) + frac) / (total.max(1) as f32);
                            let overall_pct = format!("{}%", (overall * 100.0).round() as u32);
                            let _ = slint::invoke_from_event_loop(move || {
                                TASK_PROGRESS_WIN.with(|cell| {
                                    if let Some(tp) =
                                        cell.borrow().as_ref().and_then(|tw| tw.upgrade())
                                    {
                                        tp.set_overall_progress(overall);
                                        tp.set_overall_percent_text(SharedString::from(
                                            overall_pct,
                                        ));
                                        tp.set_progress(frac);
                                        let model = tp.get_download_items();
                                        if let Some(mut row) = model.row_data(idx) {
                                            row.progress = frac;
                                            model.set_row_data(idx, row);
                                        }
                                    }
                                });
                            });
                        },
                    )
                    .await;
                    match res {
                        Ok(_) => {
                            ok_count += 1;
                            let done_now = ok_count;
                            let overall = (done_now as f32) / (total.max(1) as f32);
                            let overall_pct = format!("{}%", (overall * 100.0).round() as u32);
                            let _ = slint::invoke_from_event_loop(move || {
                                TASK_PROGRESS_WIN.with(|cell| {
                                    if let Some(tp) =
                                        cell.borrow().as_ref().and_then(|tw| tw.upgrade())
                                    {
                                        tp.set_completed_count(done_now as i32);
                                        tp.set_overall_progress(overall);
                                        tp.set_overall_percent_text(SharedString::from(
                                            overall_pct,
                                        ));
                                        let model = tp.get_download_items();
                                        if let Some(mut row) = model.row_data(idx) {
                                            row.progress = 1.0;
                                            row.status = SharedString::from("Done");
                                            model.set_row_data(idx, row);
                                        }
                                    }
                                });
                            });
                        }
                        Err(e) => {
                            last_err = Some(e.to_string());
                            let _ = slint::invoke_from_event_loop(move || {
                                TASK_PROGRESS_WIN.with(|cell| {
                                    if let Some(tp) =
                                        cell.borrow().as_ref().and_then(|tw| tw.upgrade())
                                    {
                                        let model = tp.get_download_items();
                                        if let Some(mut row) = model.row_data(idx) {
                                            row.status = SharedString::from("Failed");
                                            model.set_row_data(idx, row);
                                        }
                                    }
                                });
                            });
                        }
                    }
                }
                if let Ok(mut g) = session.lock() {
                    g.queue.clear();
                    g.resolved_plan.clear();
                }
                let _ = slint::invoke_from_event_loop(move || {
                    TASK_PROGRESS_WIN.with(|cell| {
                        if let Some(tp) = cell.borrow().as_ref().and_then(|tw| tw.upgrade()) {
                            tp.set_is_finished(true);
                            if let Some(ref err) = last_err {
                                tp.set_error_message(SharedString::from(err.as_str()));
                            } else {
                                tp.set_overall_progress(1.0);
                                tp.set_overall_percent_text(SharedString::from("100%"));
                                tp.set_progress(1.0);
                                tp.set_download_items(ModelRc::from(Rc::new(VecModel::from(
                                    Vec::new(),
                                ))));
                                tp.set_total_count(0);
                                let _ = tp.hide();
                            }
                        }
                    });
                    if let Some(w) = dm_weak.upgrade() {
                        w.set_is_downloading(false);
                        w.set_show_review_modal(false);
                        w.set_queued_count(0);
                        if let Some(err) = last_err {
                            w.set_error_message(SharedString::from(err));
                        } else {
                            w.set_status_message(SharedString::from(format!(
                                "Downloaded {ok_count} item(s) successfully."
                            )));
                            let _ = w.hide();
                        }
                    }
                    if let Some(e) = ew.upgrade() {
                        populate_edit_instance_window(&e, &svc, &inst_id);
                    }
                });
            });
        });
    }
    {
        let dm_weak = dm_window.as_weak();
        dm_window.on_close_clicked(move || {
            if let Some(w) = dm_weak.upgrade() {
                let _ = w.hide();
            }
        });
    }
}

fn pick_file_with_filter(title: &str, filter: &str) -> Option<PathBuf> {
    let out = std::process::Command::new("zenity")
        .args([
            "--file-selection",
            &format!("--title={title}"),
            &format!("--file-filter={filter}"),
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(PathBuf::from(s))
    }
}

fn wire_edit_components_callbacks(
    main_window: &MainWindow,
    edit_window: &Rc<EditInstanceWindow>,
    _install_loader_window: &Rc<InstallLoaderWindow>,
    svc: &Services,
) {
    // Move Up / Move Down / Remove / Customize / Edit / Revert
    {
        let ew = edit_window.as_weak();
        let svc = svc.clone();
        edit_window.on_move_component_up_clicked(move |uid| {
            let Some(w) = ew.upgrade() else { return };
            let id = w.get_instance_id().to_string();
            let Some((inst, _)) = find_instance_by_id(&svc, &id) else {
                return;
            };
            let inst_dir = svc.paths.instance_dir(&inst.dir_name);
            let mut m = ComponentManifest::load_or_init(&inst_dir, &inst);
            if m.move_up(uid.as_str()) {
                let _ = m.save(&inst_dir);
                populate_edit_instance_window(&w, &svc, &id);
            }
        });
    }
    {
        let ew = edit_window.as_weak();
        let svc = svc.clone();
        edit_window.on_move_component_down_clicked(move |uid| {
            let Some(w) = ew.upgrade() else { return };
            let id = w.get_instance_id().to_string();
            let Some((inst, _)) = find_instance_by_id(&svc, &id) else {
                return;
            };
            let inst_dir = svc.paths.instance_dir(&inst.dir_name);
            let mut m = ComponentManifest::load_or_init(&inst_dir, &inst);
            if m.move_down(uid.as_str()) {
                let _ = m.save(&inst_dir);
                populate_edit_instance_window(&w, &svc, &id);
            }
        });
    }
    {
        let ew = edit_window.as_weak();
        let svc = svc.clone();
        edit_window.on_remove_component_clicked(move |uid| {
            let Some(w) = ew.upgrade() else { return };
            let id = w.get_instance_id().to_string();
            let Some((inst, _)) = find_instance_by_id(&svc, &id) else {
                return;
            };
            let inst_dir = svc.paths.instance_dir(&inst.dir_name);
            let mut m = ComponentManifest::load_or_init(&inst_dir, &inst);
            match m.remove(&inst_dir, uid.as_str()) {
                Ok(true) => {
                    let _ = m.save(&inst_dir);
                    w.set_selected_component_uid(SharedString::new());
                    populate_edit_instance_window(&w, &svc, &id);
                }
                Err(e) => w.set_error_message(SharedString::from(e.to_string())),
                _ => {}
            }
        });
    }
    {
        let ew = edit_window.as_weak();
        let svc = svc.clone();
        edit_window.on_customize_component_clicked(move |uid| {
            let Some(w) = ew.upgrade() else { return };
            let id = w.get_instance_id().to_string();
            let Some((inst, _)) = find_instance_by_id(&svc, &id) else {
                return;
            };
            let inst_dir = svc.paths.instance_dir(&inst.dir_name);
            let mut m = ComponentManifest::load_or_init(&inst_dir, &inst);
            match m.customize(&inst_dir, uid.as_str()) {
                Ok(_) => {
                    let _ = m.save(&inst_dir);
                    populate_edit_instance_window(&w, &svc, &id);
                }
                Err(e) => w.set_error_message(SharedString::from(e.to_string())),
            }
        });
    }
    {
        let ew = edit_window.as_weak();
        let svc = svc.clone();
        edit_window.on_edit_component_clicked(move |uid| {
            let Some(w) = ew.upgrade() else { return };
            let id = w.get_instance_id().to_string();
            let Some((inst, _)) = find_instance_by_id(&svc, &id) else {
                return;
            };
            let inst_dir = svc.paths.instance_dir(&inst.dir_name);
            let mut m = ComponentManifest::load_or_init(&inst_dir, &inst);
            if let Ok(path) = m.customize(&inst_dir, uid.as_str()) {
                let _ = m.save(&inst_dir);
                let _ = open::that(&path);
            }
        });
    }
    {
        let ew = edit_window.as_weak();
        let svc = svc.clone();
        edit_window.on_revert_component_clicked(move |uid| {
            let Some(w) = ew.upgrade() else { return };
            let id = w.get_instance_id().to_string();
            let Some((inst, _)) = find_instance_by_id(&svc, &id) else {
                return;
            };
            let inst_dir = svc.paths.instance_dir(&inst.dir_name);
            let mut m = ComponentManifest::load_or_init(&inst_dir, &inst);
            if m.revert(&inst_dir, uid.as_str()).is_ok() {
                let _ = m.save(&inst_dir);
                populate_edit_instance_window(&w, &svc, &id);
            }
        });
    }

    // Change Version -> abre seletor de versões do Minecraft (não Install Loader)
    {
        let ew = edit_window.as_weak();
        let svc = svc.clone();
        edit_window.on_change_component_version_clicked(move |_| {
            let Some(w) = ew.upgrade() else { return };
            let versions: Vec<SharedString> = svc
                .state
                .lock()
                .map(|g| {
                    g.versions
                        .iter()
                        .map(|v| SharedString::from(&v.id))
                        .collect()
                })
                .unwrap_or_default();
            w.set_available_mc_versions(ModelRc::from(Rc::new(VecModel::from(versions))));
            w.set_show_change_mc_version_dialog(true);
        });
    }
    {
        let main_weak = main_window.as_weak();
        let ew = edit_window.as_weak();
        let svc = svc.clone();
        edit_window.on_change_mc_version_confirmed(move |new_ver| {
            let Some(w) = ew.upgrade() else { return };
            let id = w.get_instance_id().to_string();
            let ver_str = new_ver.to_string();
            if let Ok(mut g) = svc.state.lock() {
                if let Some(inst) = g.instances.iter_mut().find(|i| i.id == id) {
                    inst.minecraft_version = ver_str.clone();
                    let inst_dir = svc.paths.instance_dir(&inst.dir_name);
                    let m = ComponentManifest::load_or_init(&inst_dir, inst);
                    let _ = m.save(&inst_dir);
                    let _ = std::fs::write(
                        svc.paths.instance_metadata_file(&inst.dir_name),
                        serde_json::to_string_pretty(inst).unwrap_or_default(),
                    );
                }
            }
            populate_edit_instance_window(&w, &svc, &id);
            if let Some(main) = main_weak.upgrade() {
                refresh_main_ui(&main, &svc.state, Some(&id));
            }
        });
    }

    // Screenshots callbacks (grid assíncrono, duplo clique, Copy Image, Copy File, Rename, Delete, Open Folder)
    {
        let ew = edit_window.as_weak();
        let svc = svc.clone();
        edit_window.on_refresh_screenshots_clicked(move || {
            let Some(w) = ew.upgrade() else { return };
            let id = w.get_instance_id().to_string();
            let Some((inst, _)) = find_instance_by_id(&svc, &id) else {
                return;
            };
            let mc_dir = svc.paths.instance_game_dir(&inst.dir_name);
            let ew = ew.clone();
            let rt = svc.rt.clone();
            rt.spawn(async move {
                let list = tokio::task::spawn_blocking(move || list_screenshots(&mc_dir))
                    .await
                    .unwrap_or_default();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(w) = ew.upgrade() {
                        let items: Vec<ScreenshotItem> = list
                            .into_iter()
                            .map(|s| {
                                let (thumb, has) = Image::load_from_path(&s.path)
                                    .map(|i| (i, true))
                                    .unwrap_or_else(|_| (Image::default(), false));
                                ScreenshotItem {
                                    file_name: SharedString::from(s.file_name),
                                    display_name: SharedString::from(s.display_name),
                                    thumbnail: thumb,
                                    has_thumbnail: has,
                                }
                            })
                            .collect();
                        w.set_screenshots(ModelRc::from(Rc::new(VecModel::from(items))));
                    }
                });
            });
        });
    }
    {
        let ew = edit_window.as_weak();
        let svc = svc.clone();
        edit_window.on_open_screenshot_clicked(move |fname| {
            let Some(w) = ew.upgrade() else { return };
            let id = w.get_instance_id().to_string();
            if let Some((inst, _)) = find_instance_by_id(&svc, &id) {
                let p = screenshots_dir(&svc.paths.instance_game_dir(&inst.dir_name))
                    .join(fname.as_str());
                let _ = open::that(&p);
            }
        });
    }
    {
        let ew = edit_window.as_weak();
        let svc = svc.clone();
        edit_window.on_copy_screenshot_image_clicked(move |fname| {
            let Some(w) = ew.upgrade() else { return };
            let id = w.get_instance_id().to_string();
            if let Some((inst, _)) = find_instance_by_id(&svc, &id) {
                let p = screenshots_dir(&svc.paths.instance_game_dir(&inst.dir_name))
                    .join(fname.as_str());
                if let Ok(img) = image::open(&p) {
                    let rgba = img.to_rgba8();
                    let (width, height) = rgba.dimensions();
                    if let Ok(mut cb) = arboard::Clipboard::new() {
                        let _ = cb.set_image(arboard::ImageData {
                            width: width as usize,
                            height: height as usize,
                            bytes: std::borrow::Cow::Owned(rgba.into_raw()),
                        });
                    }
                }
            }
        });
    }
    {
        let ew = edit_window.as_weak();
        let svc = svc.clone();
        edit_window.on_copy_screenshot_file_clicked(move |fname| {
            let Some(w) = ew.upgrade() else { return };
            let id = w.get_instance_id().to_string();
            if let Some((inst, _)) = find_instance_by_id(&svc, &id) {
                let p = screenshots_dir(&svc.paths.instance_game_dir(&inst.dir_name))
                    .join(fname.as_str());
                if let Ok(mut cb) = arboard::Clipboard::new() {
                    let _ = cb.set_text(p.to_string_lossy().to_string());
                }
            }
        });
    }
    {
        let ew = edit_window.as_weak();
        let svc = svc.clone();
        edit_window.on_rename_screenshot_clicked(move |old_name, new_stem| {
            let Some(w) = ew.upgrade() else { return };
            let id = w.get_instance_id().to_string();
            if let Some((inst, _)) = find_instance_by_id(&svc, &id) {
                let mc_dir = svc.paths.instance_game_dir(&inst.dir_name);
                match rename_screenshot(&mc_dir, old_name.as_str(), new_stem.as_str()) {
                    Ok(_) => w.invoke_refresh_screenshots_clicked(),
                    Err(e) => w.set_error_message(SharedString::from(e.to_string())),
                }
            }
        });
    }
    {
        let ew = edit_window.as_weak();
        let svc = svc.clone();
        edit_window.on_delete_screenshot_clicked(move |fname| {
            let Some(w) = ew.upgrade() else { return };
            let id = w.get_instance_id().to_string();
            if let Some((inst, _)) = find_instance_by_id(&svc, &id) {
                let mc_dir = svc.paths.instance_game_dir(&inst.dir_name);
                match delete_screenshot(&mc_dir, fname.as_str()) {
                    Ok(()) => {
                        w.set_selected_screenshot_file(SharedString::new());
                        w.invoke_refresh_screenshots_clicked();
                    }
                    Err(e) => w.set_error_message(SharedString::from(e.to_string())),
                }
            }
        });
    }
    {
        let ew = edit_window.as_weak();
        let svc = svc.clone();
        edit_window.on_open_screenshots_folder_clicked(move || {
            let Some(w) = ew.upgrade() else { return };
            let id = w.get_instance_id().to_string();
            if let Some((inst, _)) = find_instance_by_id(&svc, &id) {
                let dir = screenshots_dir(&svc.paths.instance_game_dir(&inst.dir_name));
                let _ = std::fs::create_dir_all(&dir);
                let _ = open_in_file_manager(&dir);
            }
        });
    }

    // Add to Minecraft.jar / Replace Minecraft.jar / Add Agents / Add Empty / Import Components
    {
        let ew = edit_window.as_weak();
        let svc = svc.clone();
        edit_window.on_add_to_mc_jar_clicked(move || {
            let Some(w) = ew.upgrade() else { return };
            let id = w.get_instance_id().to_string();
            let Some((inst, _)) = find_instance_by_id(&svc, &id) else {
                return;
            };
            let inst_dir = svc.paths.instance_dir(&inst.dir_name);
            let base_jar = svc.paths.version_jar_file(&inst.minecraft_version);
            let ew = ew.clone();
            let svc = svc.clone();
            let rt = svc.rt.clone();
            rt.spawn(async move {
                let res = tokio::task::spawn_blocking(move || {
                    let Some(src) = pick_file_with_filter(
                        "Add to Minecraft.jar",
                        "JAR/ZIP (*.jar *.zip) | *.jar *.zip",
                    ) else {
                        return Ok(false);
                    };
                    let mut m = ComponentManifest::load_or_init(&inst_dir, &inst);
                    m.add_jar_mod(&inst_dir, &src)?;
                    m.save(&inst_dir)?;
                    if base_jar.is_file() {
                        let _ = build_patched_client_jar(&base_jar, &inst_dir, &m);
                    }
                    Ok::<bool, crab_launcher::error::LauncherError>(true)
                })
                .await;
                if let Ok(Ok(true)) = res {
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(w) = ew.upgrade() {
                            populate_edit_instance_window(&w, &svc, &id);
                        }
                    });
                }
            });
        });
    }
    {
        let ew = edit_window.as_weak();
        let svc = svc.clone();
        edit_window.on_replace_mc_jar_clicked(move || {
            let Some(w) = ew.upgrade() else { return };
            let id = w.get_instance_id().to_string();
            let Some((inst, _)) = find_instance_by_id(&svc, &id) else {
                return;
            };
            let inst_dir = svc.paths.instance_dir(&inst.dir_name);
            let ew = ew.clone();
            let svc = svc.clone();
            let rt = svc.rt.clone();
            rt.spawn(async move {
                let res = tokio::task::spawn_blocking(move || {
                    let Some(src) =
                        pick_file_with_filter("Replace Minecraft.jar", "JAR (*.jar) | *.jar")
                    else {
                        return Ok(false);
                    };
                    let mut m = ComponentManifest::load_or_init(&inst_dir, &inst);
                    m.replace_minecraft_jar(&inst_dir, &src)?;
                    m.save(&inst_dir)?;
                    Ok::<bool, crab_launcher::error::LauncherError>(true)
                })
                .await;
                if let Ok(Ok(true)) = res {
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(w) = ew.upgrade() {
                            populate_edit_instance_window(&w, &svc, &id);
                        }
                    });
                }
            });
        });
    }
    {
        let ew = edit_window.as_weak();
        let svc = svc.clone();
        edit_window.on_add_agents_clicked(move || {
            let Some(w) = ew.upgrade() else { return };
            let id = w.get_instance_id().to_string();
            let Some((inst, _)) = find_instance_by_id(&svc, &id) else {
                return;
            };
            let inst_dir = svc.paths.instance_dir(&inst.dir_name);
            let ew = ew.clone();
            let svc = svc.clone();
            let rt = svc.rt.clone();
            rt.spawn(async move {
                let res = tokio::task::spawn_blocking(move || {
                    let Some(src) =
                        pick_file_with_filter("Add Java Agent", "Java Agent JAR (*.jar) | *.jar")
                    else {
                        return Ok(false);
                    };
                    let mut m = ComponentManifest::load_or_init(&inst_dir, &inst);
                    m.add_java_agent(&inst_dir, &src)?;
                    m.save(&inst_dir)?;
                    Ok::<bool, crab_launcher::error::LauncherError>(true)
                })
                .await;
                if let Ok(Ok(true)) = res {
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(w) = ew.upgrade() {
                            populate_edit_instance_window(&w, &svc, &id);
                        }
                    });
                }
            });
        });
    }
    {
        let ew = edit_window.as_weak();
        let svc = svc.clone();
        edit_window.on_add_empty_component_clicked(move || {
            let Some(w) = ew.upgrade() else { return };
            let id = w.get_instance_id().to_string();
            let Some((inst, _)) = find_instance_by_id(&svc, &id) else {
                return;
            };
            let inst_dir = svc.paths.instance_dir(&inst.dir_name);
            let mut m = ComponentManifest::load_or_init(&inst_dir, &inst);
            let next_n = m.components.len() + 1;
            if m.add_empty_component(&inst_dir, &format!("Custom Component {next_n}"))
                .is_ok()
            {
                let _ = m.save(&inst_dir);
                populate_edit_instance_window(&w, &svc, &id);
            }
        });
    }
    {
        let ew = edit_window.as_weak();
        let svc = svc.clone();
        edit_window.on_import_components_clicked(move || {
            let Some(w) = ew.upgrade() else { return };
            let id = w.get_instance_id().to_string();
            let Some((inst, _)) = find_instance_by_id(&svc, &id) else {
                return;
            };
            let inst_dir = svc.paths.instance_dir(&inst.dir_name);
            let ew = ew.clone();
            let svc = svc.clone();
            let rt = svc.rt.clone();
            rt.spawn(async move {
                let res = tokio::task::spawn_blocking(move || {
                    let Some(src) =
                        pick_file_with_filter("Import Component JSON", "JSON (*.json) | *.json")
                    else {
                        return Ok(false);
                    };
                    let mut m = ComponentManifest::load_or_init(&inst_dir, &inst);
                    m.import_component_json(&inst_dir, &src)?;
                    m.save(&inst_dir)?;
                    Ok::<bool, crab_launcher::error::LauncherError>(true)
                })
                .await;
                if let Ok(Ok(true)) = res {
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(w) = ew.upgrade() {
                            populate_edit_instance_window(&w, &svc, &id);
                        }
                    });
                }
            });
        });
    }

    // Logs tab callbacks (histórico por sessão, busca, copiar, exportar, excluir, abrir pasta)
    {
        let ew = edit_window.as_weak();
        let svc = svc.clone();
        edit_window.on_refresh_logs_clicked(move || {
            let Some(w) = ew.upgrade() else { return };
            let id = w.get_instance_id().to_string();
            let Some((inst, _)) = find_instance_by_id(&svc, &id) else {
                return;
            };
            let mc_dir = svc.paths.instance_game_dir(&inst.dir_name);
            let sessions = list_session_logs(&mc_dir);
            let first_name = sessions
                .first()
                .map(|s| s.file_name.clone())
                .unwrap_or_default();
            let items: Vec<SessionLogItem> = sessions
                .into_iter()
                .map(|s| SessionLogItem {
                    file_name: SharedString::from(s.file_name),
                    timestamp: SharedString::from(s.timestamp),
                    size_display: SharedString::from(format_bytes(s.size_bytes)),
                })
                .collect();
            w.set_session_logs(ModelRc::from(Rc::new(VecModel::from(items))));
            if !first_name.is_empty() {
                w.set_selected_log_file(SharedString::from(&first_name));
                w.invoke_select_log_file(SharedString::from(&first_name));
            } else {
                w.set_selected_log_file(SharedString::new());
                w.set_session_log_lines(ModelRc::from(Rc::new(VecModel::from(Vec::new()))));
            }
        });
    }
    {
        let ew = edit_window.as_weak();
        let svc = svc.clone();
        edit_window.on_select_log_file(move |fname| {
            let Some(w) = ew.upgrade() else { return };
            let id = w.get_instance_id().to_string();
            let Some((inst, _)) = find_instance_by_id(&svc, &id) else {
                return;
            };
            let path = logs_dir(&svc.paths.instance_game_dir(&inst.dir_name)).join(fname.as_str());
            let q = w.get_log_search_query().to_string();
            let lines: Vec<SessionLogLine> = read_log_lines(&path, &q)
                .into_iter()
                .map(|l| SessionLogLine {
                    text: SharedString::from(l.text),
                    level: SharedString::from(l.level),
                })
                .collect();
            w.set_session_log_lines(ModelRc::from(Rc::new(VecModel::from(lines))));
        });
    }
    {
        let ew = edit_window.as_weak();
        let svc = svc.clone();
        edit_window.on_copy_log_clicked(move |fname| {
            let Some(w) = ew.upgrade() else { return };
            let id = w.get_instance_id().to_string();
            if let Some((inst, _)) = find_instance_by_id(&svc, &id) {
                let path =
                    logs_dir(&svc.paths.instance_game_dir(&inst.dir_name)).join(fname.as_str());
                if let Ok(content) = std::fs::read_to_string(&path) {
                    if let Ok(mut cb) = arboard::Clipboard::new() {
                        let _ = cb.set_text(content);
                    }
                }
            }
        });
    }
    {
        let ew = edit_window.as_weak();
        let svc = svc.clone();
        edit_window.on_export_log_clicked(move |fname| {
            let Some(w) = ew.upgrade() else { return };
            let id = w.get_instance_id().to_string();
            if let Some((inst, _)) = find_instance_by_id(&svc, &id) {
                let src =
                    logs_dir(&svc.paths.instance_game_dir(&inst.dir_name)).join(fname.as_str());
                let rt = svc.rt.clone();
                rt.spawn(async move {
                    let _ = tokio::task::spawn_blocking(move || {
                        let out = std::process::Command::new("zenity")
                            .args([
                                "--file-selection",
                                "--save",
                                "--confirm-overwrite",
                                "--title=Export Log",
                            ])
                            .output()
                            .ok()?;
                        if !out.status.success() {
                            return None;
                        }
                        let dst = PathBuf::from(String::from_utf8_lossy(&out.stdout).trim());
                        std::fs::copy(&src, &dst).ok()?;
                        Some(())
                    })
                    .await;
                });
            }
        });
    }
    {
        let ew = edit_window.as_weak();
        let svc = svc.clone();
        edit_window.on_delete_log_clicked(move |fname| {
            let Some(w) = ew.upgrade() else { return };
            let id = w.get_instance_id().to_string();
            if let Some((inst, _)) = find_instance_by_id(&svc, &id) {
                let mc_dir = svc.paths.instance_game_dir(&inst.dir_name);
                match delete_session_log(&mc_dir, fname.as_str()) {
                    Ok(()) => w.invoke_refresh_logs_clicked(),
                    Err(e) => w.set_error_message(SharedString::from(e.to_string())),
                }
            }
        });
    }
    {
        let ew = edit_window.as_weak();
        let svc = svc.clone();
        edit_window.on_open_logs_folder_clicked(move || {
            let Some(w) = ew.upgrade() else { return };
            let id = w.get_instance_id().to_string();
            if let Some((inst, _)) = find_instance_by_id(&svc, &id) {
                let dir = logs_dir(&svc.paths.instance_game_dir(&inst.dir_name));
                let _ = std::fs::create_dir_all(&dir);
                let _ = open_in_file_manager(&dir);
            }
        });
    }

    // Open Libraries / Download All
    {
        let svc = svc.clone();
        edit_window.on_open_libraries_folder_clicked(move || {
            let dir = svc.paths.libraries_dir();
            let _ = std::fs::create_dir_all(&dir);
            let _ = open_in_file_manager(&dir);
        });
    }
    {
        let main_weak = main_window.as_weak();
        let ew = edit_window.as_weak();
        let svc = svc.clone();
        edit_window.on_download_all_clicked(move || {
            let Some(w) = ew.upgrade() else { return };
            let id = w.get_instance_id().to_string();
            start_instance_installation(&main_weak, &svc, id);
        });
    }
}

fn sync_settings_icon_packs_ui(win: &SettingsWindow, svc: &Services) {
    let active_id = svc
        .state
        .lock()
        .map(|g| g.config.appearance.icon_pack_id.clone())
        .unwrap_or_else(|_| DEFAULT_ICON_PACK_ID.to_string());
    let packs = discover_icon_packs(&svc.paths.icon_packs_dir());
    let items: Vec<IconPackItem> = packs
        .into_iter()
        .map(|p| IconPackItem {
            is_active: p.id == active_id,
            id: SharedString::from(p.id),
            name: SharedString::from(p.name),
            author: SharedString::from(p.author),
            version: SharedString::from(p.version),
            description: SharedString::from(p.description),
            is_builtin: p.is_builtin,
        })
        .collect();
    win.set_active_icon_pack_id(SharedString::from(active_id));
    win.set_icon_packs(ModelRc::from(Rc::new(VecModel::from(items))));
}

fn sync_settings_accounts_ui(win: &SettingsWindow, svc: &Services) {
    let Ok(guard) = svc.state.lock() else {
        return;
    };
    let items: Vec<AccountItem> = guard
        .accounts
        .iter()
        .map(|a| AccountItem {
            id: SharedString::from(&a.id),
            username: SharedString::from(&a.username),
            kind: SharedString::from(a.kind.label()),
            status: SharedString::from(a.status.label()),
            is_default: a.is_default,
        })
        .collect();
    win.set_accounts(ModelRc::from(Rc::new(VecModel::from(items))));
}

fn populate_change_version_rows(
    cv_win: &ChangeVersionWindow,
    svc: &Services,
    cached_loader_rows: &[ChangeVersionRow],
) {
    let uid = cv_win.get_component_uid().to_string();
    let installed = cv_win.get_installed_version().to_string();
    let q = cv_win.get_search_query().to_string().to_lowercase();

    let rows: Vec<ChangeVersionRow> = if uid == "net.minecraft" {
        let show_rel = cv_win.get_filter_releases();
        let show_snap = cv_win.get_filter_snapshots();
        svc.state
            .lock()
            .map(|g| {
                g.versions
                    .iter()
                    .filter(|v| match v.version_type {
                        VersionType::Release => show_rel,
                        VersionType::Snapshot => show_snap,
                        _ => show_snap,
                    })
                    .filter(|v| q.is_empty() || v.id.to_lowercase().contains(&q))
                    .map(|v| ChangeVersionRow {
                        version: SharedString::from(&v.id),
                        version_type: SharedString::from(v.version_type.label()),
                        released: SharedString::from(v.formatted_release_date()),
                        is_installed: v.id == installed,
                    })
                    .collect()
            })
            .unwrap_or_default()
    } else if uid == "org.lwjgl" {
        [
            "Auto",
            "3.3.3",
            "3.3.2",
            "3.3.1",
            "3.2.2",
            "2.9.4-nightly-20150209",
        ]
        .into_iter()
        .filter(|v| q.is_empty() || v.to_lowercase().contains(&q))
        .map(|v| ChangeVersionRow {
            version: SharedString::from(v),
            version_type: SharedString::from("Library"),
            released: SharedString::new(),
            is_installed: v == installed,
        })
        .collect()
    } else {
        let show_exp = cv_win.get_filter_experimental();
        cached_loader_rows
            .iter()
            .filter(|r| show_exp || r.version_type.as_str() != "Experimental")
            .filter(|r| q.is_empty() || r.version.to_lowercase().contains(&q))
            .cloned()
            .collect()
    };

    cv_win.set_rows(ModelRc::from(Rc::new(VecModel::from(rows))));
}

#[allow(clippy::too_many_arguments)]
fn wire_rename_change_version_and_settings_accounts(
    main_window: &MainWindow,
    settings_window: &Rc<SettingsWindow>,
    edit_window: &Rc<EditInstanceWindow>,
    rename_window: &Rc<RenameInstanceWindow>,
    cv_window: &Rc<ChangeVersionWindow>,
    remove_account_window: &Rc<RemoveAccountWindow>,
    theme_broadcaster: &ThemeBroadcaster,
    svc: &Services,
) {
    // 0. Account Shortcuts (Ctrl+1..9, Ctrl+0 -> 1ª..10ª conta)
    {
        let main_weak = main_window.as_weak();
        let svc = svc.clone();
        main_window.on_select_account_by_index(move |idx| {
            if idx < 0 {
                return;
            }
            let acc_id_opt = svc
                .state
                .lock()
                .ok()
                .and_then(|g| g.accounts.get(idx as usize).map(|a| a.id.clone()));
            if let (Some(acc_id), Some(main)) = (acc_id_opt, main_weak.upgrade()) {
                main.invoke_select_account(SharedString::from(acc_id));
            }
        });
    }

    // 0B. RemoveAccountWindow (confirmação unificada para AccountsWindow e Settings -> Accounts)
    {
        let raw_open = Rc::clone(remove_account_window);
        let svc_open = svc.clone();
        let open_remove_dialog = move |account_id: SharedString| {
            let aid = account_id.to_string();
            let info = svc_open.state.lock().ok().and_then(|g| {
                g.accounts
                    .iter()
                    .find(|a| a.id == aid)
                    .map(|a| (a.id.clone(), a.username.clone(), a.kind.label().to_string()))
            });
            if let Some((id, username, kind_label)) = info {
                raw_open.set_account_id(SharedString::from(id));
                raw_open.set_account_username(SharedString::from(username));
                raw_open.set_account_type(SharedString::from(kind_label));
                let _ = raw_open.show();
            }
        };
        settings_window.on_remove_account_clicked(move |id| {
            open_remove_dialog(id);
        });
    }
    {
        let raw = Rc::clone(remove_account_window);
        remove_account_window.on_cancel_clicked(move || {
            let _ = raw.hide();
        });
    }
    {
        let raw = Rc::clone(remove_account_window);
        let main_weak = main_window.as_weak();
        let sw_weak = settings_window.as_weak();
        let svc = svc.clone();
        remove_account_window.on_confirm_remove(move |account_id| {
            let _ = raw.hide();
            let aid = account_id.to_string();
            let main_weak = main_weak.clone();
            let sw_weak = sw_weak.clone();
            let svc = svc.clone();
            let rt = svc.rt.clone();
            rt.spawn(async move {
                if svc.account_manager.remove_account(&aid).await.is_ok() {
                    let accounts = svc.account_manager.load_accounts().await;
                    if let Ok(mut g) = svc.state.lock() {
                        g.accounts = accounts;
                    }
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(main) = main_weak.upgrade() {
                            refresh_main_ui(&main, &svc.state, None);
                        }
                        if let Some(sw) = sw_weak.upgrade() {
                            sync_settings_accounts_ui(&sw, &svc);
                        }
                    });
                }
            });
        });
    }
    // 1. RenameInstanceWindow (acionada por F2 ou menu de contexto)
    {
        let rw = Rc::clone(rename_window);
        let svc = svc.clone();
        main_window.on_open_rename_instance_clicked(move |id| {
            let Some((inst, _)) = find_instance_by_id(&svc, id.as_str()) else {
                return;
            };
            rw.set_instance_id(SharedString::from(&inst.id));
            rw.set_name_input(SharedString::from(&inst.name));
            rw.set_error_message(SharedString::new());
            let _ = rw.show();
            rw.invoke_focus_name();
        });
    }
    {
        let main_weak = main_window.as_weak();
        let rw_weak = rename_window.as_weak();
        let svc = svc.clone();
        rename_window.on_confirm_rename(move |id, new_name| {
            let id_str = id.to_string();
            let name_str = new_name.to_string();
            let main_weak = main_weak.clone();
            let rw_weak = rw_weak.clone();
            let svc = svc.clone();
            let rt = svc.rt.clone();
            rt.spawn(async move {
                match svc
                    .instance_manager
                    .rename_instance(&id_str, &name_str)
                    .await
                {
                    Ok(updated) => {
                        if let Ok(mut g) = svc.state.lock() {
                            if let Some(item) = g.instances.iter_mut().find(|i| i.id == id_str) {
                                *item = updated;
                            }
                        }
                        let state = svc.state.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(w) = rw_weak.upgrade() {
                                let _ = w.hide();
                            }
                            if let Some(main) = main_weak.upgrade() {
                                refresh_main_ui(&main, &state, Some(&id_str));
                            }
                        });
                    }
                    Err(e) => {
                        let msg = e.to_string();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(w) = rw_weak.upgrade() {
                                w.set_error_message(SharedString::from(msg));
                            }
                        });
                    }
                }
            });
        });
    }
    {
        let rw_weak = rename_window.as_weak();
        rename_window.on_cancel_clicked(move || {
            if let Some(w) = rw_weak.upgrade() {
                let _ = w.hide();
            }
        });
    }

    // 2. Settings -> Accounts tab (reutiliza AccountManager e sincroniza com MainWindow & AccountsWindow)
    {
        let sw_weak = settings_window.as_weak();
        let svc = svc.clone();
        settings_window.on_refresh_accounts_clicked(move || {
            if let Some(sw) = sw_weak.upgrade() {
                sync_settings_accounts_ui(&sw, &svc);
            }
        });
    }
    {
        let main_weak = main_window.as_weak();
        let sw_weak = settings_window.as_weak();
        let svc = svc.clone();
        settings_window.on_add_offline_account_clicked(move |username| {
            let uname = username.to_string();
            let main_weak = main_weak.clone();
            let sw_weak = sw_weak.clone();
            let svc = svc.clone();
            let rt = svc.rt.clone();
            rt.spawn(async move {
                match svc.account_manager.create_offline_account(&uname).await {
                    Ok(_) => {
                        let accounts = svc.account_manager.load_accounts().await;
                        if let Ok(mut g) = svc.state.lock() {
                            g.accounts = accounts;
                        }
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(sw) = sw_weak.upgrade() {
                                sw.set_accounts_error_message(SharedString::new());
                                sync_settings_accounts_ui(&sw, &svc);
                            }
                            if let Some(main) = main_weak.upgrade() {
                                refresh_main_ui(&main, &svc.state, None);
                            }
                        });
                    }
                    Err(e) => {
                        let msg = e.to_string();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(sw) = sw_weak.upgrade() {
                                sw.set_accounts_error_message(SharedString::from(msg));
                            }
                        });
                    }
                }
            });
        });
    }
    {
        let main_weak = main_window.as_weak();
        let sw_weak = settings_window.as_weak();
        let svc = svc.clone();
        settings_window.on_set_default_account_clicked(move |acc_id| {
            let id = acc_id.to_string();
            let main_weak = main_weak.clone();
            let sw_weak = sw_weak.clone();
            let svc = svc.clone();
            let rt = svc.rt.clone();
            rt.spawn(async move {
                if let Ok(accounts) = svc.account_manager.set_default_account(&id).await {
                    if let Ok(mut g) = svc.state.lock() {
                        g.accounts = accounts;
                    }
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(sw) = sw_weak.upgrade() {
                            sync_settings_accounts_ui(&sw, &svc);
                        }
                        if let Some(main) = main_weak.upgrade() {
                            refresh_main_ui(&main, &svc.state, None);
                        }
                    });
                }
            });
        });
    }
    {
        let main_weak = main_window.as_weak();
        let sw_weak = settings_window.as_weak();
        let svc = svc.clone();
        settings_window.on_no_default_account_clicked(move || {
            if let Ok(mut g) = svc.state.lock() {
                for a in &mut g.accounts {
                    a.is_default = false;
                }
            }
            if let Some(sw) = sw_weak.upgrade() {
                sync_settings_accounts_ui(&sw, &svc);
            }
            if let Some(main) = main_weak.upgrade() {
                refresh_main_ui(&main, &svc.state, None);
            }
        });
    }

    // 2B. Settings -> Appearance -> Icons (Icon Packs: Lucide, Lucide Dark, custom ZIPs)
    {
        let sw_weak = settings_window.as_weak();
        let svc = svc.clone();
        settings_window.on_refresh_icon_packs_clicked(move || {
            if let Some(sw) = sw_weak.upgrade() {
                sync_settings_icon_packs_ui(&sw, &svc);
            }
        });
    }
    {
        let sw_weak = settings_window.as_weak();
        let broadcaster = theme_broadcaster.clone();
        let svc = svc.clone();
        settings_window.on_icon_pack_selected(move |pack_id| {
            let pid = pack_id.to_string();
            let cfg_opt = svc.state.lock().ok().map(|mut g| {
                g.config.appearance.icon_pack_id = pid.clone();
                g.config.clone()
            });
            broadcaster.apply_icon_pack(&pid);
            if let Some(sw) = sw_weak.upgrade() {
                sync_settings_icon_packs_ui(&sw, &svc);
            }
            if let Some(cfg) = cfg_opt {
                let paths = svc.paths.clone();
                let rt = svc.rt.clone();
                rt.spawn(async move {
                    let _ = save_config(&paths, &cfg).await;
                });
            }
        });
    }
    {
        let sw_weak = settings_window.as_weak();
        settings_window.on_reset_icon_pack_clicked(move || {
            if let Some(sw) = sw_weak.upgrade() {
                sw.invoke_icon_pack_selected(SharedString::from(DEFAULT_ICON_PACK_ID));
            }
        });
    }
    {
        let svc = svc.clone();
        settings_window.on_open_icon_packs_folder_clicked(move || {
            let dir = svc.paths.icon_packs_dir();
            let _ = std::fs::create_dir_all(&dir);
            let _ = open_in_file_manager(&dir);
        });
    }
    {
        let sw_weak = settings_window.as_weak();
        let svc = svc.clone();
        settings_window.on_import_icon_pack_clicked(move || {
            let dest_dir = svc.paths.icon_packs_dir();
            let sw_weak = sw_weak.clone();
            let svc = svc.clone();
            let rt = svc.rt.clone();
            rt.spawn(async move {
                let res = tokio::task::spawn_blocking(move || {
                    let Some(src) =
                        pick_file_with_filter("Import Icon Pack ZIP", "ZIP (*.zip) | *.zip")
                    else {
                        return Ok(false);
                    };
                    import_icon_pack_zip(&src, &dest_dir).map(|_| true)
                })
                .await;
                if let Ok(Ok(true)) = res {
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(sw) = sw_weak.upgrade() {
                            sync_settings_icon_packs_ui(&sw, &svc);
                        }
                    });
                }
            });
        });
    }

    // 3. Universal ChangeVersionWindow (Minecraft, LWJGL, Fabric, Forge, NeoForge)
    let cv_loader_cache: Arc<Mutex<Vec<ChangeVersionRow>>> = Arc::new(Mutex::new(Vec::new()));
    {
        let ew = edit_window.as_weak();
        let cv = Rc::clone(cv_window);
        let cache = cv_loader_cache.clone();
        let svc = svc.clone();
        edit_window.on_change_component_version_clicked(move |uid| {
            let Some(w) = ew.upgrade() else { return };
            let id = w.get_instance_id().to_string();
            let Some((inst, _)) = find_instance_by_id(&svc, &id) else {
                return;
            };
            let inst_dir = svc.paths.instance_dir(&inst.dir_name);
            let manifest = ComponentManifest::load_or_init(&inst_dir, &inst);
            let Some(comp) = manifest.components.iter().find(|c| c.uid == uid.as_str()) else {
                return;
            };
            let uid_str = comp.uid.clone();
            let is_mc = uid_str == "net.minecraft";
            let is_loader = uid_str.starts_with("loader.");

            cv.set_instance_id(SharedString::from(&inst.id));
            cv.set_component_uid(SharedString::from(&uid_str));
            cv.set_component_name(SharedString::from(&comp.name));
            cv.set_installed_version(SharedString::from(&comp.version));
            cv.set_selected_version(SharedString::from(&comp.version));
            cv.set_search_query(SharedString::new());
            cv.set_show_mc_filters(is_mc);
            cv.set_show_loader_filter(is_loader);
            cv.set_error_message(SharedString::new());
            cv.set_conflict_warning(SharedString::from(
                if is_mc && inst.modloader_spec.is_some() {
                    "Warning: Changing Minecraft version may require updating or reinstalling your modloader and mods."
                } else {
                    ""
                },
            ));
            let _ = cv.show();

            if is_loader {
                let kind = match uid_str.as_str() {
                    "loader.fabric" => Some(ModloaderKind::Fabric),
                    "loader.forge" => Some(ModloaderKind::Forge),
                    "loader.neoforge" => Some(ModloaderKind::NeoForge),
                    "loader.quilt" => Some(ModloaderKind::Quilt),
                    "loader.liteloader" => Some(ModloaderKind::LiteLoader),
                    _ => None,
                };
                if let Some(k) = kind {
                    cv.set_is_loading(true);
                    let cv_weak = cv.as_weak();
                    let cache = cache.clone();
                    let client = svc.http_client.clone();
                    let mc_ver = inst.minecraft_version.clone();
                    let installed = comp.version.clone();
                    let svc_c = svc.clone();
                    let rt = svc.rt.clone();
                    rt.spawn(async move {
                        let res = fetch_loader_versions(&client, k, &mc_ver).await;
                        let _ = slint::invoke_from_event_loop(move || {
                            let Some(cw) = cv_weak.upgrade() else { return };
                            cw.set_is_loading(false);
                            match res {
                                Ok(list) => {
                                    let rows: Vec<ChangeVersionRow> = list
                                        .into_iter()
                                        .map(|v| ChangeVersionRow {
                                            is_installed: v.version == installed,
                                            version: SharedString::from(v.version),
                                            version_type: SharedString::from(if v.recommended {
                                                "Recommended"
                                            } else if v.stable {
                                                "Stable"
                                            } else {
                                                "Experimental"
                                            }),
                                            released: SharedString::new(),
                                        })
                                        .collect();
                                    if let Ok(mut g) = cache.lock() {
                                        *g = rows.clone();
                                    }
                                    populate_change_version_rows(&cw, &svc_c, &rows);
                                }
                                Err(e) => {
                                    cw.set_error_message(SharedString::from(e.to_string()));
                                }
                            }
                        });
                    });
                }
            } else {
                cv.set_is_loading(false);
                populate_change_version_rows(&cv, &svc, &[]);
            }
        });
    }
    {
        let cv_weak = cv_window.as_weak();
        let cache = cv_loader_cache.clone();
        let svc = svc.clone();
        cv_window.on_filters_changed(move || {
            if let Some(cw) = cv_weak.upgrade() {
                let rows = cache.lock().map(|g| g.clone()).unwrap_or_default();
                populate_change_version_rows(&cw, &svc, &rows);
            }
        });
    }
    {
        let cv_weak = cv_window.as_weak();
        let cache = cv_loader_cache.clone();
        let svc = svc.clone();
        cv_window.on_refresh_clicked(move || {
            if let Some(cw) = cv_weak.upgrade() {
                let rows = cache.lock().map(|g| g.clone()).unwrap_or_default();
                populate_change_version_rows(&cw, &svc, &rows);
            }
        });
    }
    {
        let main_weak = main_window.as_weak();
        let ew = edit_window.as_weak();
        let cv_weak = cv_window.as_weak();
        let svc = svc.clone();
        cv_window.on_apply_clicked(move |inst_id, comp_uid, new_ver| {
            let id = inst_id.to_string();
            let uid = comp_uid.to_string();
            let ver = new_ver.to_string();
            if let Ok(mut g) = svc.state.lock() {
                if let Some(inst) = g.instances.iter_mut().find(|i| i.id == id) {
                    let inst_dir = svc.paths.instance_dir(&inst.dir_name);
                    let mut m = ComponentManifest::load_or_init(&inst_dir, inst);
                    if uid == "net.minecraft" {
                        inst.minecraft_version = ver.clone();
                    } else if uid.starts_with("loader.") {
                        if let Some(ref mut spec) = inst.modloader_spec {
                            spec.version = ver.clone();
                        }
                    }
                    if let Some(c) = m.components.iter_mut().find(|c| c.uid == uid) {
                        c.version = ver.clone();
                    }
                    let _ = m.save(&inst_dir);
                    let _ = std::fs::write(
                        svc.paths.instance_metadata_file(&inst.dir_name),
                        serde_json::to_string_pretty(inst).unwrap_or_default(),
                    );
                }
            }
            if let Some(cw) = cv_weak.upgrade() {
                let _ = cw.hide();
            }
            if let Some(w) = ew.upgrade() {
                populate_edit_instance_window(&w, &svc, &id);
            }
            if let Some(main) = main_weak.upgrade() {
                refresh_main_ui(&main, &svc.state, Some(&id));
            }
        });
    }
    {
        let cv_weak = cv_window.as_weak();
        cv_window.on_cancel_clicked(move || {
            if let Some(cw) = cv_weak.upgrade() {
                let _ = cw.hide();
            }
        });
    }
}

#[allow(clippy::too_many_arguments)]
fn wire_native_dialog_windows(
    main_window: &MainWindow,
    settings_window: &Rc<SettingsWindow>,
    edit_window: &Rc<EditInstanceWindow>,
    change_group_window: &Rc<ChangeGroupWindow>,
    select_icon_window: &Rc<SelectIconWindow>,
    download_mods_window: &Rc<DownloadModsWindow>,
    confirm_win: &Rc<ConfirmDialogWindow>,
    input_win: &Rc<TextInputDialogWindow>,
    server_win: &Rc<ServerDialogWindow>,
    offline_win: &Rc<AddOfflineAccountWindow>,
    copy_win: &Rc<CopyInstanceWindow>,
    review_win: &Rc<ReviewModsWindow>,
    svc: &Services,
) {
    type ConfirmAction = Rc<std::cell::RefCell<Option<Box<dyn FnOnce()>>>>;
    type InputAction = Rc<std::cell::RefCell<Option<Box<dyn FnOnce(String)>>>>;
    type ServerAction = Rc<std::cell::RefCell<Option<Box<dyn FnOnce(String, String)>>>>;

    let confirm_action: ConfirmAction = Rc::new(std::cell::RefCell::new(None));
    let input_action: InputAction = Rc::new(std::cell::RefCell::new(None));
    let server_action: ServerAction = Rc::new(std::cell::RefCell::new(None));

    // ConfirmDialogWindow handlers
    {
        let cw = Rc::clone(confirm_win);
        let act = Rc::clone(&confirm_action);
        confirm_win.on_confirmed(move || {
            let _ = cw.hide();
            if let Some(f) = act.borrow_mut().take() {
                f();
            }
        });
    }
    {
        let cw = Rc::clone(confirm_win);
        let act = Rc::clone(&confirm_action);
        confirm_win.on_cancelled(move || {
            *act.borrow_mut() = None;
            let _ = cw.hide();
        });
    }

    // TextInputDialogWindow handlers
    {
        let iw = Rc::clone(input_win);
        let act = Rc::clone(&input_action);
        input_win.on_confirmed(move |val| {
            let _ = iw.hide();
            if let Some(f) = act.borrow_mut().take() {
                f(val.to_string());
            }
        });
    }
    {
        let iw = Rc::clone(input_win);
        let act = Rc::clone(&input_action);
        input_win.on_cancelled(move || {
            *act.borrow_mut() = None;
            let _ = iw.hide();
        });
    }

    // ServerDialogWindow handlers
    {
        let sw = Rc::clone(server_win);
        let act = Rc::clone(&server_action);
        server_win.on_confirmed(move |name, addr| {
            let _ = sw.hide();
            if let Some(f) = act.borrow_mut().take() {
                f(name.to_string(), addr.to_string());
            }
        });
    }
    {
        let sw = Rc::clone(server_win);
        let act = Rc::clone(&server_action);
        server_win.on_cancelled(move || {
            *act.borrow_mut() = None;
            let _ = sw.hide();
        });
    }

    // AddOfflineAccountWindow (usada pela Toolbar e por Settings -> Accounts)
    {
        let ow = Rc::clone(offline_win);
        let main_weak = main_window.as_weak();
        let sw_weak = settings_window.as_weak();
        let svc = svc.clone();
        offline_win.on_create_clicked(move |username| {
            let uname = username.to_string();
            let ow_weak = ow.as_weak();
            let main_weak = main_weak.clone();
            let sw_weak = sw_weak.clone();
            let svc = svc.clone();
            let rt = svc.rt.clone();
            rt.spawn(async move {
                match svc.account_manager.create_offline_account(&uname).await {
                    Ok(_) => {
                        let accounts = svc.account_manager.load_accounts().await;
                        if let Ok(mut g) = svc.state.lock() {
                            g.accounts = accounts;
                        }
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(w) = ow_weak.upgrade() {
                                w.set_error_message(SharedString::new());
                                w.set_username(SharedString::new());
                                let _ = w.hide();
                            }
                            if let Some(sw) = sw_weak.upgrade() {
                                sync_settings_accounts_ui(&sw, &svc);
                            }
                            if let Some(main) = main_weak.upgrade() {
                                refresh_main_ui(&main, &svc.state, None);
                            }
                        });
                    }
                    Err(err) => {
                        let msg = err.to_string();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(w) = ow_weak.upgrade() {
                                w.set_error_message(SharedString::from(msg));
                            }
                        });
                    }
                }
            });
        });
    }
    {
        let ow = Rc::clone(offline_win);
        offline_win.on_cancel_clicked(move || {
            let _ = ow.hide();
        });
    }
    {
        let ow = Rc::clone(offline_win);
        let main_weak = main_window.as_weak();
        main_window.on_request_add_offline_account_dialog(move || {
            ow.set_username(SharedString::new());
            ow.set_error_message(SharedString::new());
            if let Some(p) = main_weak.upgrade() {
                center_window_over(ow.window(), p.window());
            }
            let _ = ow.show();
            ow.invoke_focus_username();
        });
    }
    {
        let ow = Rc::clone(offline_win);
        let sw_weak = settings_window.as_weak();
        settings_window.on_request_add_offline_account_dialog(move || {
            ow.set_username(SharedString::new());
            ow.set_error_message(SharedString::new());
            if let Some(p) = sw_weak.upgrade() {
                center_window_over(ow.window(), p.window());
            }
            let _ = ow.show();
            ow.invoke_focus_username();
        });
    }

    // CopyInstanceWindow
    {
        let cw = Rc::clone(copy_win);
        let main_weak = main_window.as_weak();
        main_window.on_request_copy_instance_dialog(move |id, name| {
            cw.set_source_instance_id(id);
            cw.set_new_name(SharedString::from(format!("{} Copy", name)));
            cw.set_copy_saves(true);
            cw.set_copy_config(true);
            cw.set_copy_resourcepacks(true);
            cw.set_copy_screenshots(false);
            cw.set_error_message(SharedString::new());
            if let Some(p) = main_weak.upgrade() {
                center_window_over(cw.window(), p.window());
            }
            let _ = cw.show();
            cw.invoke_focus_name();
        });
    }
    {
        let cw = Rc::clone(copy_win);
        copy_win.on_cancel_clicked(move || {
            let _ = cw.hide();
        });
    }
    {
        let cw = Rc::clone(copy_win);
        let main_weak = main_window.as_weak();
        copy_win.on_confirm_clicked(move |src_id, new_name, saves, cfg, rp, ss| {
            let _ = cw.hide();
            if let Some(main) = main_weak.upgrade() {
                main.invoke_copy_instance_confirmed(src_id, new_name, saves, cfg, rp, ss);
            }
        });
    }

    // MainWindow native dialogs: Delete Instance, Create/Rename Group, Delete Group, Stop Game
    {
        let cw = Rc::clone(confirm_win);
        let act = Rc::clone(&confirm_action);
        let main_weak = main_window.as_weak();
        main_window.on_request_delete_instance_dialog(move |id, name| {
            cw.set_window_title(SharedString::from("Delete Instance — Crab"));
            cw.set_heading(SharedString::from(format!("Delete '{}'?", name)));
            cw.set_message(SharedString::from(
                "This permanently deletes the instance directory, worlds, mods, and configs. Shared versions and libraries are preserved.",
            ));
            cw.set_confirm_label(SharedString::from("Delete"));
            cw.set_confirm_variant(SharedString::from("danger"));
            let mw = main_weak.clone();
            *act.borrow_mut() = Some(Box::new(move || {
                if let Some(m) = mw.upgrade() {
                    m.invoke_delete_confirmed(id);
                }
            }));
            if let Some(p) = main_weak.upgrade() {
                center_window_over(cw.window(), p.window());
            }
            let _ = cw.show();
        });
    }
    {
        let iw = Rc::clone(input_win);
        let act = Rc::clone(&input_action);
        let main_weak = main_window.as_weak();
        main_window.on_request_group_dialog(move |title, target_id, initial| {
            iw.set_window_title(SharedString::from(format!("{} — Crab", title)));
            iw.set_heading(title.clone());
            iw.set_subtitle(SharedString::from("Organize your instances into groups."));
            iw.set_input_text(initial);
            iw.set_placeholder(SharedString::from("Group name..."));
            iw.set_confirm_label(SharedString::from(if target_id.is_empty() {
                "Create"
            } else {
                "Rename"
            }));
            iw.set_error_message(SharedString::new());
            let mw = main_weak.clone();
            *act.borrow_mut() = Some(Box::new(move |val| {
                if let Some(m) = mw.upgrade() {
                    m.invoke_group_dialog_confirmed(target_id, SharedString::from(val));
                }
            }));
            if let Some(p) = main_weak.upgrade() {
                center_window_over(iw.window(), p.window());
            }
            let _ = iw.show();
            iw.invoke_focus_input();
        });
    }
    {
        let cw = Rc::clone(confirm_win);
        let act = Rc::clone(&confirm_action);
        let main_weak = main_window.as_weak();
        main_window.on_request_delete_group_dialog(move |gid, gname| {
            cw.set_window_title(SharedString::from("Delete Group — Crab"));
            cw.set_heading(SharedString::from(format!("Delete Group '{}'?", gname)));
            cw.set_message(SharedString::from(
                "All instances in this group will be moved to Ungrouped. No instance files will be deleted.",
            ));
            cw.set_confirm_label(SharedString::from("Delete Group"));
            cw.set_confirm_variant(SharedString::from("danger"));
            let mw = main_weak.clone();
            *act.borrow_mut() = Some(Box::new(move || {
                if let Some(m) = mw.upgrade() {
                    m.invoke_delete_group_clicked(gid);
                }
            }));
            if let Some(p) = main_weak.upgrade() {
                center_window_over(cw.window(), p.window());
            }
            let _ = cw.show();
        });
    }
    {
        let cw = Rc::clone(confirm_win);
        let act = Rc::clone(&confirm_action);
        let main_weak = main_window.as_weak();
        let svc = svc.clone();
        main_window.on_stop_instance_clicked(move |id| {
            let id_str = id.to_string();
            let (confirm, name) = svc
                .state
                .lock()
                .ok()
                .map(|g| {
                    let inst_name = g
                        .instances
                        .iter()
                        .find(|i| i.id == id_str)
                        .map(|i| i.name.clone())
                        .unwrap_or_else(|| "Minecraft".to_string());
                    (g.config.minecraft.confirm_before_stopping, inst_name)
                })
                .unwrap_or((false, "Minecraft".to_string()));
            if confirm {
                cw.set_window_title(SharedString::from("Stop Minecraft — Crab"));
                cw.set_heading(SharedString::from("Stop Minecraft?"));
                cw.set_message(SharedString::from(format!(
                    "Force stopping '{}' may cause unsaved world progress to be lost.",
                    name
                )));
                cw.set_confirm_label(SharedString::from("Stop Game"));
                cw.set_confirm_variant(SharedString::from("danger"));
                let mw = main_weak.clone();
                let id_for_cb = SharedString::from(&id_str);
                *act.borrow_mut() = Some(Box::new(move || {
                    if let Some(m) = mw.upgrade() {
                        m.invoke_confirm_stop_instance(id_for_cb);
                    }
                }));
                if let Some(p) = main_weak.upgrade() {
                    center_window_over(cw.window(), p.window());
                }
                let _ = cw.show();
            } else if svc.process_manager.stop_instance(&id_str) {
                set_status(&main_weak, "Stopping Minecraft...");
            }
        });
    }

    // EditInstanceWindow native dialogs & Mod Details link
    {
        edit_window.on_open_url_clicked(move |url| {
            let u = url.trim().to_string();
            if u.starts_with("https://") || u.starts_with("http://") {
                let _ = std::process::Command::new("xdg-open").arg(&u).spawn();
            }
        });
    }
    {
        let iw = Rc::clone(input_win);
        let act = Rc::clone(&input_action);
        let ew_weak = edit_window.as_weak();
        edit_window.on_request_rename_world_dialog(move |folder| {
            iw.set_window_title(SharedString::from("Rename World — Crab"));
            iw.set_heading(SharedString::from("Rename World"));
            iw.set_subtitle(SharedString::from("Enter a new name for this world."));
            iw.set_input_text(folder.clone());
            iw.set_placeholder(SharedString::from("World name..."));
            iw.set_confirm_label(SharedString::from("Rename"));
            iw.set_error_message(SharedString::new());
            let ew = ew_weak.clone();
            *act.borrow_mut() = Some(Box::new(move |val| {
                if let Some(w) = ew.upgrade() {
                    w.invoke_rename_world_clicked(folder, SharedString::from(val));
                }
            }));
            if let Some(p) = ew_weak.upgrade() {
                center_window_over(iw.window(), p.window());
            }
            let _ = iw.show();
            iw.invoke_focus_input();
        });
    }
    {
        let cw = Rc::clone(confirm_win);
        let act = Rc::clone(&confirm_action);
        let ew_weak = edit_window.as_weak();
        edit_window.on_request_delete_world_dialog(move |folder| {
            cw.set_window_title(SharedString::from("Delete World — Crab"));
            cw.set_heading(SharedString::from(format!("Delete World '{}'?", folder)));
            cw.set_message(SharedString::from(
                "Permanently delete this world folder? This cannot be undone.",
            ));
            cw.set_confirm_label(SharedString::from("Delete"));
            cw.set_confirm_variant(SharedString::from("danger"));
            let ew = ew_weak.clone();
            *act.borrow_mut() = Some(Box::new(move || {
                if let Some(w) = ew.upgrade() {
                    w.invoke_delete_world_clicked(folder);
                }
            }));
            if let Some(p) = ew_weak.upgrade() {
                center_window_over(cw.window(), p.window());
            }
            let _ = cw.show();
        });
    }
    {
        let iw = Rc::clone(input_win);
        let act = Rc::clone(&input_action);
        let ew_weak = edit_window.as_weak();
        edit_window.on_request_rename_screenshot_dialog(move |file| {
            iw.set_window_title(SharedString::from("Rename Screenshot — Crab"));
            iw.set_heading(SharedString::from("Rename Screenshot"));
            iw.set_subtitle(SharedString::from(
                "Enter a new filename for this screenshot.",
            ));
            iw.set_input_text(file.clone());
            iw.set_placeholder(SharedString::from("Screenshot name..."));
            iw.set_confirm_label(SharedString::from("Rename"));
            iw.set_error_message(SharedString::new());
            let ew = ew_weak.clone();
            *act.borrow_mut() = Some(Box::new(move |val| {
                if let Some(w) = ew.upgrade() {
                    w.invoke_rename_screenshot_clicked(file, SharedString::from(val));
                }
            }));
            if let Some(p) = ew_weak.upgrade() {
                center_window_over(iw.window(), p.window());
            }
            let _ = iw.show();
            iw.invoke_focus_input();
        });
    }
    {
        let sw = Rc::clone(server_win);
        let act = Rc::clone(&server_action);
        let ew_weak = edit_window.as_weak();
        edit_window.on_request_server_dialog(move |is_edit, idx, name, addr| {
            sw.set_server_name(name);
            sw.set_server_address(addr);
            sw.set_error_message(SharedString::new());
            let ew = ew_weak.clone();
            *act.borrow_mut() = Some(Box::new(move |n, a| {
                if let Some(w) = ew.upgrade() {
                    if is_edit {
                        w.invoke_edit_server_clicked(
                            idx,
                            SharedString::from(n),
                            SharedString::from(a),
                        );
                    } else {
                        w.invoke_add_server_clicked(SharedString::from(n), SharedString::from(a));
                    }
                }
            }));
            if let Some(p) = ew_weak.upgrade() {
                center_window_over(sw.window(), p.window());
            }
            let _ = sw.show();
            sw.invoke_focus_name();
        });
    }

    // ChangeGroupWindow Rename Group -> TextInputDialogWindow
    {
        let iw = Rc::clone(input_win);
        let act = Rc::clone(&input_action);
        let cgw_weak = change_group_window.as_weak();
        change_group_window.on_request_rename_group_dialog(move |gid, gname| {
            iw.set_window_title(SharedString::from("Rename Group — Crab"));
            iw.set_heading(SharedString::from("Rename Group"));
            iw.set_subtitle(SharedString::from(
                "Enter a new display name for this group.",
            ));
            iw.set_input_text(gname);
            iw.set_placeholder(SharedString::from("Group name..."));
            iw.set_confirm_label(SharedString::from("Rename"));
            iw.set_error_message(SharedString::new());
            let cgw = cgw_weak.clone();
            *act.borrow_mut() = Some(Box::new(move |val| {
                if let Some(w) = cgw.upgrade() {
                    w.invoke_rename_group_clicked(gid, SharedString::from(val));
                }
            }));
            if let Some(p) = cgw_weak.upgrade() {
                center_window_over(iw.window(), p.window());
            }
            let _ = iw.show();
            iw.invoke_focus_input();
        });
    }

    // SelectIconWindow Rename / Delete Icon -> TextInputDialogWindow / ConfirmDialogWindow
    {
        let iw = Rc::clone(input_win);
        let act = Rc::clone(&input_action);
        let siw_weak = select_icon_window.as_weak();
        select_icon_window.on_request_rename_icon_dialog(move |icon_id, icon_name| {
            iw.set_window_title(SharedString::from("Rename Icon — Crab"));
            iw.set_heading(SharedString::from("Rename Icon"));
            iw.set_subtitle(SharedString::from(
                "Enter a new label for this custom icon.",
            ));
            iw.set_input_text(icon_name);
            iw.set_placeholder(SharedString::from("Icon name..."));
            iw.set_confirm_label(SharedString::from("Save"));
            iw.set_error_message(SharedString::new());
            let siw = siw_weak.clone();
            *act.borrow_mut() = Some(Box::new(move |val| {
                if let Some(w) = siw.upgrade() {
                    w.invoke_rename_icon_confirmed(icon_id, SharedString::from(val));
                }
            }));
            if let Some(p) = siw_weak.upgrade() {
                center_window_over(iw.window(), p.window());
            }
            let _ = iw.show();
            iw.invoke_focus_input();
        });
    }
    {
        let cw = Rc::clone(confirm_win);
        let act = Rc::clone(&confirm_action);
        let siw_weak = select_icon_window.as_weak();
        select_icon_window.on_request_delete_icon_dialog(move |icon_id, icon_name, usage| {
            cw.set_window_title(SharedString::from("Delete Icon — Crab"));
            cw.set_heading(SharedString::from(format!("Delete Icon '{}'?", icon_name)));
            cw.set_message(SharedString::from(if usage > 0 {
                format!(
                    "This icon is currently used by {} instance(s). They will revert to the default icon.",
                    usage
                )
            } else {
                "Delete this custom icon from Crab Launcher?".to_string()
            }));
            cw.set_confirm_label(SharedString::from("Delete"));
            cw.set_confirm_variant(SharedString::from("danger"));
            let siw = siw_weak.clone();
            *act.borrow_mut() = Some(Box::new(move || {
                if let Some(w) = siw.upgrade() {
                    w.invoke_confirm_delete_icon(icon_id);
                }
            }));
            if let Some(p) = siw_weak.upgrade() {
                center_window_over(cw.window(), p.window());
            }
            let _ = cw.show();
        });
    }

    // Servers -> Delete com confirmação nativa
    {
        let cw = Rc::clone(confirm_win);
        let act = Rc::clone(&confirm_action);
        let ew = edit_window.as_weak();
        edit_window.on_request_delete_server_dialog(move |idx, name| {
            let label = if name.trim().is_empty() {
                "this server".to_string()
            } else {
                format!("'{name}'")
            };
            cw.set_window_title(SharedString::from("Delete Server — Crab"));
            cw.set_heading(SharedString::from("Delete Server"));
            cw.set_message(SharedString::from(format!(
                "Remove {label} from servers.dat?"
            )));
            cw.set_confirm_label(SharedString::from("Delete"));
            let ew_inner = ew.clone();
            *act.borrow_mut() = Some(Box::new(move || {
                if let Some(e) = ew_inner.upgrade() {
                    e.invoke_remove_server_clicked(idx);
                }
            }));
            if let Some(e) = ew.upgrade() {
                center_window_over(cw.window(), e.window());
            }
            let _ = cw.show();
        });
    }

    // Seleção múltipla e atalhos de teclado (Mods, Texture Packs, Shader Packs)
    #[derive(Default)]
    struct ListSelection {
        selected: HashSet<usize>,
        anchor: Option<usize>,
        cursor: Option<usize>,
    }
    let sel_state: Rc<std::cell::RefCell<HashMap<String, ListSelection>>> =
        Rc::new(std::cell::RefCell::new(HashMap::new()));

    fn sync_content_selection_ui(
        e: &EditInstanceWindow,
        kind: &str,
        sel: &HashSet<usize>,
        cursor: Option<usize>,
    ) {
        match kind {
            "mods" => {
                let model = e.get_mods();
                let count = model.row_count();
                for i in 0..count {
                    if let Some(mut row) = model.row_data(i) {
                        let is_sel = sel.contains(&i);
                        if row.selected != is_sel {
                            row.selected = is_sel;
                            model.set_row_data(i, row);
                        }
                    }
                }
                if let Some(ci) = cursor.and_then(|c| model.row_data(c)) {
                    e.set_selected_mod_file(ci.file_name);
                    e.set_selected_mod_name(ci.name);
                    e.set_selected_mod_author(ci.author);
                    e.set_selected_mod_description(ci.description);
                    e.set_selected_mod_url(ci.homepage_url);
                    e.set_selected_mod_provider(ci.provider);
                    e.set_selected_mod_icon(ci.icon);
                    e.set_selected_mod_has_icon(ci.has_icon);
                } else if sel.is_empty() {
                    e.set_selected_mod_file(SharedString::new());
                }
            }
            "resourcepacks" => {
                let model = e.get_texture_packs();
                let count = model.row_count();
                for i in 0..count {
                    if let Some(mut row) = model.row_data(i) {
                        let is_sel = sel.contains(&i);
                        if row.selected != is_sel {
                            row.selected = is_sel;
                            model.set_row_data(i, row);
                        }
                    }
                }
                if let Some(ci) = cursor.and_then(|c| model.row_data(c)) {
                    e.set_selected_texture_pack(ci.file_name);
                    e.set_selected_texture_pack_name(ci.display_name);
                    e.set_selected_texture_pack_author(ci.author);
                    e.set_selected_texture_pack_description(ci.description);
                    e.set_selected_texture_pack_url(ci.homepage_url);
                    e.set_selected_texture_pack_provider(ci.provider);
                    e.set_selected_texture_pack_icon(ci.icon);
                    e.set_selected_texture_pack_has_icon(ci.has_icon);
                } else if sel.is_empty() {
                    e.set_selected_texture_pack(SharedString::new());
                }
            }
            "shaderpacks" => {
                let model = e.get_shader_packs();
                let count = model.row_count();
                for i in 0..count {
                    if let Some(mut row) = model.row_data(i) {
                        let is_sel = sel.contains(&i);
                        if row.selected != is_sel {
                            row.selected = is_sel;
                            model.set_row_data(i, row);
                        }
                    }
                }
                if let Some(ci) = cursor.and_then(|c| model.row_data(c)) {
                    e.set_selected_shader_pack(ci.file_name);
                    e.set_selected_shader_pack_name(ci.display_name);
                    e.set_selected_shader_pack_author(ci.author);
                    e.set_selected_shader_pack_description(ci.description);
                    e.set_selected_shader_pack_url(ci.homepage_url);
                    e.set_selected_shader_pack_provider(ci.provider);
                    e.set_selected_shader_pack_icon(ci.icon);
                    e.set_selected_shader_pack_has_icon(ci.has_icon);
                } else if sel.is_empty() {
                    e.set_selected_shader_pack(SharedString::new());
                }
            }
            _ => {}
        }
    }

    {
        let ew = edit_window.as_weak();
        let state = Rc::clone(&sel_state);
        edit_window.on_content_item_clicked(move |kind, idx, ctrl, shift| {
            let Some(e) = ew.upgrade() else { return };
            let idx = idx.max(0) as usize;
            let k = kind.to_string();
            let mut map = state.borrow_mut();
            let entry = map.entry(k.clone()).or_default();
            if shift {
                let anchor = entry.anchor.unwrap_or(idx);
                entry.selected.clear();
                let (lo, hi) = if anchor <= idx {
                    (anchor, idx)
                } else {
                    (idx, anchor)
                };
                for i in lo..=hi {
                    entry.selected.insert(i);
                }
                entry.cursor = Some(idx);
            } else if ctrl {
                if !entry.selected.remove(&idx) {
                    entry.selected.insert(idx);
                }
                entry.anchor = Some(idx);
                entry.cursor = Some(idx);
            } else {
                entry.selected.clear();
                entry.selected.insert(idx);
                entry.anchor = Some(idx);
                entry.cursor = Some(idx);
            }
            sync_content_selection_ui(&e, &k, &entry.selected, entry.cursor);
        });
    }
    {
        let ew = edit_window.as_weak();
        let state = Rc::clone(&sel_state);
        edit_window.on_content_select_all(move |kind| {
            let Some(e) = ew.upgrade() else { return };
            let k = kind.to_string();
            let count = match k.as_str() {
                "mods" => e.get_mods().row_count(),
                "resourcepacks" => e.get_texture_packs().row_count(),
                "shaderpacks" => e.get_shader_packs().row_count(),
                _ => 0,
            };
            let mut map = state.borrow_mut();
            let entry = map.entry(k.clone()).or_default();
            entry.selected = (0..count).collect();
            if count > 0 && entry.cursor.is_none() {
                entry.cursor = Some(0);
                entry.anchor = Some(0);
            }
            sync_content_selection_ui(&e, &k, &entry.selected, entry.cursor);
        });
    }
    {
        let ew = edit_window.as_weak();
        let state = Rc::clone(&sel_state);
        edit_window.on_content_clear_selection(move |kind| {
            let Some(e) = ew.upgrade() else { return };
            let k = kind.to_string();
            let mut map = state.borrow_mut();
            let entry = map.entry(k.clone()).or_default();
            entry.selected.clear();
            entry.anchor = None;
            entry.cursor = None;
            sync_content_selection_ui(&e, &k, &entry.selected, None);
        });
    }
    {
        let ew = edit_window.as_weak();
        let state = Rc::clone(&sel_state);
        edit_window.on_content_navigate(move |kind, delta, shift| {
            let Some(e) = ew.upgrade() else { return };
            let k = kind.to_string();
            let count = match k.as_str() {
                "mods" => e.get_mods().row_count(),
                "resourcepacks" => e.get_texture_packs().row_count(),
                "shaderpacks" => e.get_shader_packs().row_count(),
                _ => 0,
            };
            if count == 0 {
                return;
            }
            let mut map = state.borrow_mut();
            let entry = map.entry(k.clone()).or_default();
            let cur = entry.cursor.unwrap_or(0) as i32;
            let next = (cur + delta).clamp(0, (count - 1) as i32) as usize;
            if shift {
                let anchor = entry.anchor.unwrap_or(cur.max(0) as usize);
                entry.anchor = Some(anchor);
                entry.selected.clear();
                let (lo, hi) = if anchor <= next {
                    (anchor, next)
                } else {
                    (next, anchor)
                };
                for i in lo..=hi {
                    entry.selected.insert(i);
                }
                entry.cursor = Some(next);
            } else {
                entry.selected.clear();
                entry.selected.insert(next);
                entry.anchor = Some(next);
                entry.cursor = Some(next);
            }
            sync_content_selection_ui(&e, &k, &entry.selected, entry.cursor);
        });
    }
    {
        let cw = Rc::clone(confirm_win);
        let act = Rc::clone(&confirm_action);
        let ew = edit_window.as_weak();
        let state = Rc::clone(&sel_state);
        edit_window.on_content_delete_selected(move |kind| {
            let Some(e) = ew.upgrade() else { return };
            let k = kind.to_string();
            let files: Vec<SharedString> = {
                let map = state.borrow();
                let indices: Vec<usize> = map
                    .get(&k)
                    .map(|s| {
                        let mut v: Vec<usize> = s.selected.iter().copied().collect();
                        v.sort_unstable();
                        v
                    })
                    .unwrap_or_default();
                match k.as_str() {
                    "mods" => {
                        let m = e.get_mods();
                        let mut out: Vec<SharedString> = indices
                            .into_iter()
                            .filter_map(|i| m.row_data(i).map(|r| r.file_name))
                            .collect();
                        if out.is_empty() && !e.get_selected_mod_file().is_empty() {
                            out.push(e.get_selected_mod_file());
                        }
                        out
                    }
                    "resourcepacks" => {
                        let m = e.get_texture_packs();
                        let mut out: Vec<SharedString> = indices
                            .into_iter()
                            .filter_map(|i| m.row_data(i).map(|r| r.file_name))
                            .collect();
                        if out.is_empty() && !e.get_selected_texture_pack().is_empty() {
                            out.push(e.get_selected_texture_pack());
                        }
                        out
                    }
                    "shaderpacks" => {
                        let m = e.get_shader_packs();
                        let mut out: Vec<SharedString> = indices
                            .into_iter()
                            .filter_map(|i| m.row_data(i).map(|r| r.file_name))
                            .collect();
                        if out.is_empty() && !e.get_selected_shader_pack().is_empty() {
                            out.push(e.get_selected_shader_pack());
                        }
                        out
                    }
                    _ => Vec::new(),
                }
            };
            if files.is_empty() {
                return;
            }
            let count = files.len();
            let label = match k.as_str() {
                "mods" => "mod(s)",
                "resourcepacks" => "resource pack(s)",
                _ => "shader pack(s)",
            };
            cw.set_window_title(SharedString::from("Delete Selected — Crab"));
            cw.set_heading(SharedString::from(format!("Delete {count} {label}")));
            cw.set_message(SharedString::from(format!(
                "Permanently delete {count} selected {label} from this instance?"
            )));
            cw.set_confirm_label(SharedString::from("Delete"));
            let ew_inner = ew.clone();
            let state_inner = Rc::clone(&state);
            *act.borrow_mut() = Some(Box::new(move || {
                if let Some(e) = ew_inner.upgrade() {
                    for f in &files {
                        match k.as_str() {
                            "mods" => e.invoke_remove_mod_clicked(f.clone()),
                            "resourcepacks" => e.invoke_remove_texture_pack_clicked(f.clone()),
                            "shaderpacks" => e.invoke_remove_shader_pack_clicked(f.clone()),
                            _ => {}
                        }
                    }
                    if let Some(entry) = state_inner.borrow_mut().get_mut(&k) {
                        entry.selected.clear();
                        entry.anchor = None;
                        entry.cursor = None;
                    }
                }
            }));
            center_window_over(cw.window(), e.window());
            let _ = cw.show();
        });
    }

    // ReviewModsWindow (janela nativa para revisão de mods selecionados no DownloadModsWindow)
    REVIEW_MODS_WIN.with(|cell| {
        *cell.borrow_mut() = Some(review_win.as_weak());
    });
    {
        let rw = Rc::clone(review_win);
        review_win.on_cancelled(move || {
            let _ = rw.hide();
        });
    }
    {
        let rw = Rc::clone(review_win);
        let dm_weak = download_mods_window.as_weak();
        review_win.on_confirm_install(move || {
            let _ = rw.hide();
            if let Some(dm) = dm_weak.upgrade() {
                dm.invoke_confirm_download_clicked();
            }
        });
    }
}
