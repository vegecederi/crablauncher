use super::*;
use crab_launcher::core::components::model::ModloaderSpec;
use crab_launcher::core::instance::ManagedModpackInfo;
use crab_launcher::core::mods::platform::{
    fetch_and_cache_mod_icon, fetch_and_cache_remote_image, fetch_modrinth_project_details,
    ModProvider, ModSortBy,
};
use crab_launcher::core::mods::rich_text::{parse_rich_description, RichBlock};
use crab_launcher::services::modpack::{self, PackSearchFilters, Project, Release, StageProgress};
use std::{
    collections::HashSet,
    io::Cursor,
    path::{Path, PathBuf},
    sync::Mutex,
};

const PACK_LOADERS: &[(&str, &str)] = &[
    ("fabric", "Fabric"),
    ("neoforge", "NeoForge"),
    ("forge", "Forge"),
    ("quilt", "Quilt"),
];

const PACK_CATEGORIES: &[(&str, &str)] = &[
    ("adventure", "Adventure"),
    ("challenging", "Challenging"),
    ("combat", "Combat"),
    ("kitchen-sink", "Kitchen Sink"),
    ("lightweight", "Lightweight"),
    ("magic", "Magic"),
    ("multiplayer", "Multiplayer"),
    ("optimization", "Optimization"),
    ("quests", "Quests"),
    ("technology", "Technology"),
];

struct PackBrowserState {
    results: Vec<Project>,
    total_hits: usize,
    offset: usize,
    sort: ModSortBy,
    selected_loaders: HashSet<String>,
    selected_categories: HashSet<String>,
    detail_request: u64,
}

impl Default for PackBrowserState {
    fn default() -> Self {
        Self {
            results: Vec::new(),
            total_hits: 0,
            offset: 0,
            sort: ModSortBy::Relevance,
            selected_loaders: HashSet::new(),
            selected_categories: HashSet::new(),
            detail_request: 0,
        }
    }
}

fn begin(win: &AddInstanceWindow) -> i32 {
    let request = win.get_pack_request().wrapping_add(1);
    win.set_pack_request(request);
    win.set_error_message("".into());
    request
}

fn clear_selection(win: &AddInstanceWindow) {
    win.set_pack_ready(false);
    win.set_has_pack_icon(false);
    win.set_pack_icon_url("".into());
    win.set_pack_project("".into());
    win.set_pack_release("".into());
    win.set_pack_release_label("Select a version".into());
    win.set_pack_detail_title("".into());
    win.set_pack_detail_author("".into());
    win.set_pack_description("".into());
    win.set_pack_detail_body("".into());
    win.set_pack_detail_blocks(ModelRc::default());
    win.set_pack_detail_homepage("".into());
    win.set_pack_detail_issues_url("".into());
    win.set_pack_detail_source_url("".into());
    win.set_pack_detail_discord_url("".into());
    win.set_pack_detail_wiki_url("".into());
    win.set_pack_releases(ModelRc::default());
}

fn sync_filter_models(win: &AddInstanceWindow, state: &PackBrowserState) {
    let loaders: Vec<FilterOptionItem> = PACK_LOADERS
        .iter()
        .map(|(id, label)| FilterOptionItem {
            id: (*id).into(),
            label: (*label).into(),
            checked: state.selected_loaders.contains(*id),
        })
        .collect();
    let categories: Vec<FilterOptionItem> = PACK_CATEGORIES
        .iter()
        .map(|(id, label)| FilterOptionItem {
            id: (*id).into(),
            label: (*label).into(),
            checked: state.selected_categories.contains(*id),
        })
        .collect();
    win.set_pack_filter_modloaders(ModelRc::from(Rc::new(VecModel::from(loaders))));
    win.set_pack_filter_categories(ModelRc::from(Rc::new(VecModel::from(categories))));
}

fn build_pack_list_model(projects: &[Project], cache_dir: &Path) -> ModelRc<PackResultItem> {
    let items: Vec<PackResultItem> = projects
        .iter()
        .map(|p| {
            let safe_id: String = p
                .project_id
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
                .collect();
            let icon_file = cache_dir
                .join("mod-icons")
                .join(format!("modrinth-{safe_id}.png"));
            let (icon, has_icon) = Image::load_from_path(&icon_file)
                .ok()
                .map(|img| (img, true))
                .unwrap_or_else(|| (Image::default(), false));
            PackResultItem {
                id: p.project_id.as_str().into(),
                title: p.title.as_str().into(),
                description: p.description.as_str().into(),
                author: p.author.as_str().into(),
                homepage_url: p.homepage_url().into(),
                icon_url: p.icon_url.as_deref().unwrap_or_default().into(),
                icon,
                has_icon,
            }
        })
        .collect();
    ModelRc::from(Rc::new(VecModel::from(items)))
}

fn preview(win: &AddInstanceWindow, bytes: Option<&[u8]>) {
    win.set_has_pack_icon(false);
    if let Some(img) = bytes.and_then(decode_slint_image) {
        win.set_pack_icon(img);
        win.set_has_pack_icon(true);
    }
}

pub(super) fn decode_slint_image(bytes: &[u8]) -> Option<Image> {
    let buffer = decode_icon(bytes)?;
    let pixels = slint::SharedPixelBuffer::clone_from_slice(
        buffer.as_raw(),
        buffer.width(),
        buffer.height(),
    );
    Some(Image::from_rgba8(pixels))
}

fn decode_icon(bytes: &[u8]) -> Option<image::RgbaImage> {
    let format = image::guess_format(bytes).ok()?;
    if !matches!(
        format,
        image::ImageFormat::Png | image::ImageFormat::Jpeg | image::ImageFormat::WebP
    ) {
        return None;
    }
    let mut reader = image::ImageReader::with_format(Cursor::new(bytes), format);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(1024);
    limits.max_image_height = Some(1024);
    limits.max_alloc = Some(8 * 1024 * 1024);
    reader.limits(limits);
    Some(reader.decode().ok()?.into_rgba8())
}

fn latest(window: &Weak<AddInstanceWindow>, request: i32) -> Option<AddInstanceWindow> {
    window.upgrade().filter(|w| w.get_pack_request() == request)
}

fn sort_from_id(id: &str) -> (ModSortBy, &'static str) {
    match id {
        "downloads" => (ModSortBy::Downloads, "Sort by Downloads"),
        "updated" => (ModSortBy::Updated, "Sort by Updated"),
        "newest" => (ModSortBy::Newest, "Sort by Newest"),
        _ => (ModSortBy::Relevance, "Sort by Relevance"),
    }
}

fn search(
    win: &AddInstanceWindow,
    svc: &Services,
    browser: &Arc<Mutex<PackBrowserState>>,
    append: bool,
) {
    if win.get_is_creating() || win.get_active_source() != 2 {
        return;
    }
    let request = if append {
        win.get_pack_request()
    } else {
        let req = begin(win);
        clear_selection(win);
        win.set_instance_name("".into());
        req
    };

    let (filters, sort, offset) = {
        let Ok(mut st) = browser.lock() else {
            return;
        };
        if !append {
            st.results.clear();
            st.total_hits = 0;
            st.offset = 0;
        }
        let mc_ver = win.get_pack_mc_version_filter().trim().to_string();
        let filters = PackSearchFilters {
            game_version: (!mc_ver.is_empty()).then_some(mc_ver),
            loaders: st.selected_loaders.iter().cloned().collect(),
            categories: st.selected_categories.iter().cloned().collect(),
        };
        (filters, st.sort, st.offset)
    };

    if append {
        win.set_pack_loading_more(true);
    } else {
        win.set_pack_searching(true);
        win.set_pack_loading_more(false);
        win.set_pack_can_load_more(false);
        win.set_pack_message("Searching Modrinth...".into());
        win.set_pack_results(ModelRc::default());
    }

    let query = win.get_pack_query().to_string();
    let weak = win.as_weak();
    let client = svc.http_client.clone();
    let cache_dir = svc.paths.cache_dir.clone();
    let browser = Arc::clone(browser);

    svc.rt.spawn(async move {
        let result = modpack::search_filtered_paged(&client, &query, &filters, sort, offset).await;
        match result {
            Ok((hits, total_hits)) => {
                let icon_jobs: Vec<(String, String)> = hits
                    .iter()
                    .filter_map(|p| {
                        p.icon_url
                            .as_deref()
                            .filter(|u| !u.trim().is_empty())
                            .map(|u| (p.project_id.clone(), u.to_string()))
                    })
                    .collect();

                let (all_results, can_more) = {
                    let Ok(mut st) = browser.lock() else {
                        return;
                    };
                    if append {
                        st.results.extend(hits);
                    } else {
                        st.results = hits;
                    }
                    st.total_hits = total_hits;
                    st.offset = st.results.len();
                    let can_more = st.results.len() < st.total_hits;
                    (st.results.clone(), can_more)
                };

                let weak_ui = weak.clone();
                let cache_ui = cache_dir.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    let Some(win) = latest(&weak_ui, request) else {
                        return;
                    };
                    win.set_pack_searching(false);
                    win.set_pack_loading_more(false);
                    win.set_pack_can_load_more(can_more);
                    win.set_pack_message(if all_results.is_empty() {
                        "No modpacks found.".into()
                    } else {
                        "".into()
                    });
                    win.set_pack_results(build_pack_list_model(&all_results, &cache_ui));
                });

                // Carrega ícones em paralelo e atualiza o modelo assim que os arquivos entram em cache.
                let client_icons = client.clone();
                let cache_icons = cache_dir.clone();
                let browser_icons = Arc::clone(&browser);
                let weak_icons = weak.clone();
                tokio::spawn(async move {
                    use futures_util::{stream, StreamExt};
                    stream::iter(icon_jobs.into_iter().map(|(pid, url)| {
                        let client_c = client_icons.clone();
                        let cache_c = cache_icons.clone();
                        async move {
                            let _ = fetch_and_cache_mod_icon(
                                &client_c,
                                &cache_c,
                                ModProvider::Modrinth,
                                &pid,
                                &url,
                            )
                            .await;
                        }
                    }))
                    .buffer_unordered(6)
                    .collect::<Vec<()>>()
                    .await;

                    let snapshot = browser_icons
                        .lock()
                        .ok()
                        .map(|st| st.results.clone())
                        .unwrap_or_default();
                    let _ = slint::invoke_from_event_loop(move || {
                        let Some(win) = latest(&weak_icons, request) else {
                            return;
                        };
                        win.set_pack_results(build_pack_list_model(&snapshot, &cache_icons));
                    });
                });
            }
            Err(err) => {
                let msg = err.to_string();
                let _ = slint::invoke_from_event_loop(move || {
                    let Some(win) = latest(&weak, request) else {
                        return;
                    };
                    win.set_pack_searching(false);
                    win.set_pack_loading_more(false);
                    win.set_pack_message("".into());
                    win.set_error_message(msg.into());
                });
            }
        }
    });
}

fn inspect(win: &AddInstanceWindow, svc: &Services, browse: bool) {
    if win.get_is_creating() || win.get_pack_loading() || win.get_active_source() != 1 {
        return;
    }
    let request = begin(win);
    win.set_pack_loading(true);
    win.set_pack_message("Reading modpack...".into());
    let current_path = win.get_pack_path().to_string();
    let weak = win.as_weak();
    svc.rt.spawn(async move {
        let result = tokio::task::spawn_blocking(move || -> Result<_> {
            let path = if browse {
                #[cfg(target_os = "linux")]
                {
                    crab_launcher::platform::linux::pick_modpack_file_native()?
                }
                #[cfg(not(target_os = "linux"))]
                {
                    return Err(LauncherError::Launch("Enter a local archive path.".into()));
                }
            } else {
                Some(PathBuf::from(current_path.trim()))
            };
            let Some(path) = path else { return Ok(None) };
            let pack = modpack::inspect(&path)?;
            Ok(Some((path, pack)))
        })
        .await
        .map_err(|e| LauncherError::Launch(e.to_string()))
        .and_then(|r| r);
        let _ = slint::invoke_from_event_loop(move || {
            let Some(win) = latest(&weak, request) else {
                return;
            };
            win.set_pack_loading(false);
            win.set_pack_message("".into());
            match result {
                Ok(Some((path, pack))) => {
                    win.set_pack_path(path.to_string_lossy().as_ref().into());
                    win.set_instance_name(pack.name.as_str().into());
                    preview(&win, pack.icon.as_deref());
                    win.set_pack_message(
                        format!(
                            "Minecraft {}{}",
                            pack.minecraft,
                            pack.loader
                                .as_ref()
                                .map(|(kind, version)| format!(" · {kind} {version}"))
                                .unwrap_or_default()
                        )
                        .into(),
                    );
                    win.set_pack_ready(true);
                }
                Ok(None) => {}
                Err(err) => win.set_error_message(err.to_string().into()),
            }
        });
    });
}

async fn fetch_icon(client: &Client, url: &str, folder: &Path) -> Option<Vec<u8>> {
    if url.is_empty() {
        return None;
    }
    let path = folder.join("project-icon");
    if let Err(err) = modpack::download_icon(client, url, &path).await {
        tracing::warn!("Failed to download modpack icon: {err}");
        return None;
    }
    tokio::fs::read(path).await.ok()
}

fn update_task_progress_for_modpack(op_id: u64, progress: &StageProgress) {
    let stage = progress.stage.clone();
    let current = progress.current_file.clone();
    let indeterminate = progress.indeterminate;
    let completed = progress.completed_files;
    let total = progress.total_files;
    let downloaded = progress.downloaded_bytes;
    let total_bytes = progress.total_bytes;

    let _ = slint::invoke_from_event_loop(move || {
        with_task_progress_op(op_id, |tp| {
            tp.set_task_stage(stage.into());
            tp.set_current_file(current.into());
            tp.set_indeterminate(indeterminate);
            if !indeterminate && total > 0 {
                let ratio = if total_bytes > 0 {
                    (downloaded as f32 / total_bytes as f32).clamp(0.0, 1.0)
                } else {
                    (completed as f32 / total as f32).clamp(0.0, 1.0)
                };
                tp.set_progress(ratio);
                tp.set_overall_progress(ratio * 0.85);
                tp.set_percent_text(format!("{:.0}%", ratio * 100.0).into());
                tp.set_overall_percent_text(format!("{:.0}%", ratio * 85.0).into());
                tp.set_files_text(format!("{completed}/{total} files").into());
                if total_bytes > 0 {
                    tp.set_bytes_text(
                        format!(
                            "{} / {}",
                            format_bytes(downloaded),
                            format_bytes(total_bytes)
                        )
                        .into(),
                    );
                }
            }
        });
    });
}

#[allow(clippy::too_many_arguments)]
async fn import(
    svc: &Services,
    op_id: u64,
    source: i32,
    local: PathBuf,
    project: String,
    project_title: String,
    project_homepage: String,
    release: String,
    icon_url: String,
    name: String,
    group: Option<String>,
) -> Result<(InstanceMetadata, Option<(ModloaderKind, String)>)> {
    let root = svc.paths.instances_dir();
    tokio::fs::create_dir_all(&root)
        .await
        .map_err(|e| LauncherError::Launch(format!("Could not prepare instances folder: {e}")))?;
    let temporary = tempfile::Builder::new()
        .prefix(".crab-import-")
        .tempdir_in(&root)
        .map_err(|e| LauncherError::Launch(format!("Could not stage modpack: {e}")))?;
    let staging = temporary.path().join("instance");

    let (source_path, resolved_release_name) = if source == 2 {
        update_task_progress_for_modpack(
            op_id,
            &StageProgress {
                stage: "Downloading modpack archive from Modrinth...".into(),
                completed_files: 0,
                total_files: 0,
                downloaded_bytes: 0,
                total_bytes: 0,
                current_file: String::new(),
                indeterminate: true,
            },
        );
        let releases = modpack::releases(&svc.http_client, &project).await?;
        let selected_rel = releases.iter().find(|r| r.id == release).ok_or_else(|| {
            LauncherError::Launch(
                "This Modrinth version no longer offers a modpack archive.".into(),
            )
        })?;
        let (url, hash) = selected_rel.archive().ok_or_else(|| {
            LauncherError::Launch(
                "This Modrinth version no longer offers a modpack archive.".into(),
            )
        })?;
        let rel_label = selected_rel.display_label();
        let path = temporary.path().join("pack.mrpack");
        modpack::download_archive(&svc.http_client, url, hash, &path).await?;
        (path, Some(rel_label))
    } else {
        (local, None)
    };

    let path = source_path.clone();
    let pack = tokio::task::spawn_blocking(move || modpack::inspect(&path))
        .await
        .map_err(|e| LauncherError::Launch(e.to_string()))??;
    let version_type = svc
        .state
        .lock()
        .ok()
        .and_then(|s| {
            s.versions
                .iter()
                .find(|v| v.id == pack.minecraft)
                .map(|v| v.version_type)
        })
        .unwrap_or(VersionType::Release);

    let remote_icon = fetch_icon(&svc.http_client, &icon_url, temporary.path()).await;
    let game_dir = staging.join(".minecraft");
    let managed_files = modpack::stage_with_progress(
        &svc.http_client,
        &pack,
        source_path,
        game_dir.clone(),
        move |p| {
            update_task_progress_for_modpack(op_id, &p);
        },
    )
    .await?;

    // Enriquece o ModIndex e baixa ícones dos mods instalados via hash SHA-1 em lote.
    update_task_progress_for_modpack(
        op_id,
        &StageProgress {
            stage: "Fetching mod metadata and icons...".into(),
            completed_files: 0,
            total_files: 0,
            downloaded_bytes: 0,
            total_bytes: 0,
            current_file: String::new(),
            indeterminate: true,
        },
    );
    let mods_dir = game_dir.join("mods");
    let _ =
        modpack::enrich_installed_mods_metadata(&svc.http_client, &mods_dir, &svc.paths.cache_dir)
            .await;

    let icon = remote_icon
        .as_deref()
        .filter(|bytes| decode_icon(bytes).is_some())
        .or_else(|| {
            pack.icon
                .as_deref()
                .filter(|bytes| decode_icon(bytes).is_some())
        });
    let spec = pack.loader.as_ref().map(|(kind, version)| ModloaderSpec {
        kind: *kind,
        version: version.clone(),
    });
    // Apenas instâncias instaladas via Modrinth Modpack recebem a aba de gerenciamento Modpack.
    let modpack_info = if source == 2 {
        Some(ManagedModpackInfo {
            provider: "Modrinth".to_string(),
            pack_name: if project_title.trim().is_empty() {
                pack.name.clone()
            } else {
                project_title
            },
            pack_id: project,
            version_id: release,
            version_name: resolved_release_name.unwrap_or_else(|| pack.minecraft.clone()),
            homepage_url: project_homepage,
            managed_files,
        })
    } else {
        None
    };
    let instance = svc
        .instance_manager
        .publish_import(
            &staging,
            &name,
            &pack.minecraft,
            version_type,
            group,
            spec,
            modpack_info,
            icon,
        )
        .await?;
    Ok((instance, pack.loader))
}

fn rich_blocks_to_ui(blocks: Vec<(RichBlock, Option<PathBuf>)>) -> Vec<DescriptionBlockItem> {
    blocks
        .into_iter()
        .map(|(blk, img_path)| match blk {
            RichBlock::Heading(t) => DescriptionBlockItem {
                kind: "heading".into(),
                text: t.into(),
                url: "".into(),
                image: Image::default(),
                has_image: false,
            },
            RichBlock::Subheading(t) => DescriptionBlockItem {
                kind: "subheading".into(),
                text: t.into(),
                url: "".into(),
                image: Image::default(),
                has_image: false,
            },
            RichBlock::Paragraph(t) => DescriptionBlockItem {
                kind: "paragraph".into(),
                text: t.into(),
                url: "".into(),
                image: Image::default(),
                has_image: false,
            },
            RichBlock::Bullet(t) => DescriptionBlockItem {
                kind: "bullet".into(),
                text: t.into(),
                url: "".into(),
                image: Image::default(),
                has_image: false,
            },
            RichBlock::Link { label, url } => DescriptionBlockItem {
                kind: "link".into(),
                text: label.into(),
                url: url.into(),
                image: Image::default(),
                has_image: false,
            },
            RichBlock::Image { link_url, alt, .. } => {
                let (img, has) = img_path
                    .and_then(|p| Image::load_from_path(&p).ok())
                    .map(|i| (i, true))
                    .unwrap_or_else(|| (Image::default(), false));
                DescriptionBlockItem {
                    kind: "image".into(),
                    text: alt.into(),
                    url: link_url.unwrap_or_default().into(),
                    image: img,
                    has_image: has,
                }
            }
            RichBlock::Separator => DescriptionBlockItem {
                kind: "separator".into(),
                text: "".into(),
                url: "".into(),
                image: Image::default(),
                has_image: false,
            },
        })
        .collect()
}

pub(super) fn wire(main: &MainWindow, add: &AddInstanceWindow, svc: &Services) {
    let browser = Arc::new(Mutex::new(PackBrowserState::default()));
    if let Ok(st) = browser.lock() {
        sync_filter_models(add, &st);
    }

    {
        let weak = add.as_weak();
        add.window().on_close_requested(move || {
            if weak.upgrade().is_some_and(|win| win.get_is_creating()) {
                slint::CloseRequestResponse::KeepWindowShown
            } else {
                slint::CloseRequestResponse::HideWindow
            }
        });
    }
    {
        let weak = add.as_weak();
        let svc = svc.clone();
        let browser = Arc::clone(&browser);
        add.on_source_changed(move |source| {
            let Some(win) = weak.upgrade() else { return };
            begin(&win);
            clear_selection(&win);
            win.set_active_source(source);
            win.set_pack_loading(false);
            win.set_pack_searching(false);
            win.set_pack_loading_more(false);
            win.set_pack_can_load_more(false);
            win.set_pack_message("".into());
            win.set_pack_path("".into());
            win.set_pack_query("".into());
            win.set_pack_results(ModelRc::default());
            win.set_instance_name(if source == 0 {
                win.get_selected_version_id()
            } else {
                "".into()
            });
            win.set_user_edited_name(false);
            if source == 2 {
                if let Ok(st) = browser.lock() {
                    sync_filter_models(&win, &st);
                }
                search(&win, &svc, &browser, false);
            }
        });
    }
    for browse in [false, true] {
        let weak = add.as_weak();
        let svc = svc.clone();
        let callback = move || {
            if let Some(win) = weak.upgrade() {
                inspect(&win, &svc, browse);
            }
        };
        if browse {
            add.on_browse_pack(callback);
        } else {
            add.on_inspect_pack(callback);
        }
    }
    {
        let weak = add.as_weak();
        add.on_pack_path_edited(move || {
            if let Some(win) = weak.upgrade() {
                begin(&win);
                clear_selection(&win);
                win.set_pack_loading(false);
                win.set_pack_message("".into());
            }
        });
    }
    {
        let weak = add.as_weak();
        let svc = svc.clone();
        let browser = Arc::clone(&browser);
        add.on_search_packs(move || {
            if let Some(win) = weak.upgrade() {
                search(&win, &svc, &browser, false);
            }
        });
    }
    {
        let weak = add.as_weak();
        let svc = svc.clone();
        let browser = Arc::clone(&browser);
        add.on_pack_load_more(move || {
            if let Some(win) = weak.upgrade() {
                search(&win, &svc, &browser, true);
            }
        });
    }
    {
        let weak = add.as_weak();
        let svc = svc.clone();
        let browser = Arc::clone(&browser);
        add.on_pack_sort_changed(move |sort_id| {
            let Some(win) = weak.upgrade() else { return };
            let (sort, label) = sort_from_id(sort_id.as_str());
            win.set_pack_sort_id(sort_id);
            win.set_pack_sort_label(label.into());
            if let Ok(mut st) = browser.lock() {
                st.sort = sort;
            }
            search(&win, &svc, &browser, false);
        });
    }
    {
        let weak = add.as_weak();
        let svc = svc.clone();
        let browser = Arc::clone(&browser);
        add.on_pack_filters_changed(move || {
            if let Some(win) = weak.upgrade() {
                search(&win, &svc, &browser, false);
            }
        });
    }
    {
        let weak = add.as_weak();
        let svc = svc.clone();
        let browser = Arc::clone(&browser);
        add.on_pack_toggle_filter_loader(move |id| {
            let Some(win) = weak.upgrade() else { return };
            if let Ok(mut st) = browser.lock() {
                let k = id.to_string();
                if !st.selected_loaders.remove(&k) {
                    st.selected_loaders.insert(k);
                }
                sync_filter_models(&win, &st);
            }
            search(&win, &svc, &browser, false);
        });
    }
    {
        let weak = add.as_weak();
        let svc = svc.clone();
        let browser = Arc::clone(&browser);
        add.on_pack_toggle_filter_category(move |id| {
            let Some(win) = weak.upgrade() else { return };
            if let Ok(mut st) = browser.lock() {
                let k = id.to_string();
                if !st.selected_categories.remove(&k) {
                    st.selected_categories.insert(k);
                }
                sync_filter_models(&win, &st);
            }
            search(&win, &svc, &browser, false);
        });
    }
    {
        let weak = add.as_weak();
        let svc = svc.clone();
        let browser = Arc::clone(&browser);
        add.on_pack_reset_filters(move || {
            let Some(win) = weak.upgrade() else { return };
            win.set_pack_mc_version_filter("".into());
            if let Ok(mut st) = browser.lock() {
                st.selected_loaders.clear();
                st.selected_categories.clear();
                sync_filter_models(&win, &st);
            }
            search(&win, &svc, &browser, false);
        });
    }
    {
        add.on_open_pack_url(move |url| {
            if url.starts_with("https://") || url.starts_with("http://") {
                let _ = open::that(url.as_str());
            }
        });
    }
    {
        let weak = add.as_weak();
        let svc = svc.clone();
        let browser = Arc::clone(&browser);
        add.on_select_pack(move |project_id| {
            let Some(win) = weak.upgrade() else { return };
            let pid = project_id.to_string();
            let Some((hit, detail_req)) = browser.lock().ok().and_then(|mut st| {
                let hit = st.results.iter().find(|p| p.project_id == pid).cloned()?;
                st.detail_request = st.detail_request.wrapping_add(1);
                Some((hit, st.detail_request))
            }) else {
                return;
            };

            let search_req = win.get_pack_request();
            win.set_pack_ready(false);
            win.set_pack_project(hit.project_id.as_str().into());
            win.set_instance_name(hit.title.as_str().into());
            win.set_pack_detail_title(hit.title.as_str().into());
            win.set_pack_detail_author(hit.author.as_str().into());
            win.set_pack_description(hit.description.as_str().into());
            win.set_pack_detail_body("".into());
            win.set_pack_detail_blocks(ModelRc::default());
            win.set_pack_detail_homepage(hit.homepage_url().into());
            win.set_pack_detail_issues_url("".into());
            win.set_pack_detail_source_url("".into());
            win.set_pack_detail_discord_url("".into());
            win.set_pack_detail_wiki_url("".into());
            win.set_pack_icon_url(hit.icon_url.as_deref().unwrap_or_default().into());
            win.set_pack_releases(ModelRc::default());
            win.set_pack_release("".into());
            win.set_pack_release_label("Loading versions...".into());
            win.set_pack_loading(true);
            win.set_error_message("".into());

            // Usa imediatamente o ícone já em cache no topo se disponível.
            let safe_id: String = hit
                .project_id
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
                .collect();
            let cached_icon = svc
                .paths
                .cache_dir
                .join("mod-icons")
                .join(format!("modrinth-{safe_id}.png"));
            if let Ok(img) = Image::load_from_path(&cached_icon) {
                win.set_pack_icon(img);
                win.set_has_pack_icon(true);
            } else {
                win.set_has_pack_icon(false);
            }

            let client = svc.http_client.clone();
            let cache_dir = svc.paths.cache_dir.clone();
            let weak = weak.clone();
            let browser = Arc::clone(&browser);

            svc.rt.spawn(async move {
                let icon_url = hit.icon_url.clone().unwrap_or_default();
                let icon_task = async {
                    if !icon_url.is_empty() {
                        fetch_and_cache_mod_icon(
                            &client,
                            &cache_dir,
                            ModProvider::Modrinth,
                            &hit.project_id,
                            &icon_url,
                        )
                        .await
                    } else {
                        None
                    }
                };
                let details_task = fetch_modrinth_project_details(&client, &hit.project_id);
                let releases_task = modpack::releases(&client, &hit.project_id);
                let (details_res, releases, cached_icon_path) =
                    tokio::join!(details_task, releases_task, icon_task);

                let details_opt = details_res.ok();
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

                let _ = slint::invoke_from_event_loop(move || {
                    let is_latest = browser
                        .lock()
                        .ok()
                        .is_some_and(|st| st.detail_request == detail_req);
                    if !is_latest {
                        return;
                    }
                    let Some(win) = latest(&weak, search_req) else {
                        return;
                    };
                    win.set_pack_loading(false);
                    if let Some(p) = cached_icon_path {
                        if let Ok(img) = Image::load_from_path(&p) {
                            win.set_pack_icon(img);
                            win.set_has_pack_icon(true);
                        }
                    }
                    if let Some(d) = details_opt {
                        win.set_pack_detail_body(render_markdown_plain(&d.body_markdown).into());
                        win.set_pack_detail_issues_url(d.issues_url.unwrap_or_default().into());
                        win.set_pack_detail_source_url(d.source_url.unwrap_or_default().into());
                        win.set_pack_detail_discord_url(d.discord_url.unwrap_or_default().into());
                        win.set_pack_detail_wiki_url(d.wiki_url.unwrap_or_default().into());
                        if let Some(blocks) = rich_blocks_with_imgs {
                            win.set_pack_detail_blocks(ModelRc::from(Rc::new(VecModel::from(
                                rich_blocks_to_ui(blocks),
                            ))));
                        }
                    }
                    match releases {
                        Ok(releases) if !releases.is_empty() => {
                            let first_id = releases[0].id.clone();
                            let first_label = releases[0].display_label();
                            win.set_pack_release(first_id.into());
                            win.set_pack_release_label(first_label.into());
                            win.set_pack_releases(ModelRc::from(Rc::new(VecModel::from(
                                releases
                                    .into_iter()
                                    .map(|r| DropdownOption {
                                        label: r.display_label().into(),
                                        id: r.id.into(),
                                        enabled: true,
                                        ..Default::default()
                                    })
                                    .collect::<Vec<_>>(),
                            ))));
                            win.set_pack_ready(true);
                        }
                        Ok(_) => {
                            win.set_pack_release_label("No installable versions".into());
                            win.set_pack_message("No installable releases found.".into());
                        }
                        Err(err) => {
                            win.set_pack_release_label("Failed to load versions".into());
                            win.set_error_message(err.to_string().into());
                        }
                    }
                });
            });
        });
    }
    {
        let weak = add.as_weak();
        add.on_select_pack_release(move |id| {
            if let Some(win) = weak.upgrade() {
                if let Some(option) = win.get_pack_releases().iter().find(|o| o.id == id) {
                    win.set_pack_release(id);
                    win.set_pack_release_label(option.label);
                }
            }
        });
    }
    {
        let weak = add.as_weak();
        let main = main.as_weak();
        let svc = svc.clone();
        add.on_install_pack(move || {
            let Some(win) = weak.upgrade() else { return };
            if win.get_is_creating() || !win.get_pack_ready() || win.get_pack_loading() {
                return;
            }
            let name = match validate_instance_name(&win.get_instance_name()) {
                Ok(name) => name,
                Err(err) => {
                    win.set_error_message(err.to_string().into());
                    return;
                }
            };
            win.set_is_creating(true);
            win.set_error_message("".into());
            let source = win.get_active_source();
            let local = PathBuf::from(win.get_pack_path().as_str().trim());
            let project = win.get_pack_project().to_string();
            let project_title = win.get_pack_detail_title().to_string();
            let project_homepage = win.get_pack_detail_homepage().to_string();
            let release = win.get_pack_release().to_string();
            let icon = win.get_pack_icon_url().to_string();
            let group = win.get_selected_group_id().to_string();
            let group = (!group.is_empty()).then_some(group);

            let op_id = next_task_op_id();
            let title_display = name.clone();
            with_task_progress(|tp| {
                tp.set_task_title(format!("Installing Modpack: {title_display}").into());
                tp.set_task_kind("modpack".into());
                tp.set_task_stage("Preparing modpack installation...".into());
                tp.set_step_text("Downloading modpack components".into());
                tp.set_task_state("Running".into());
                tp.set_overall_progress(0.0);
                tp.set_overall_percent_text("0%".into());
                tp.set_progress(0.0);
                tp.set_percent_text("0%".into());
                tp.set_indeterminate(true);
                tp.set_files_text("".into());
                tp.set_bytes_text("".into());
                tp.set_speed_text("".into());
                tp.set_current_file("".into());
                tp.set_error_message("".into());
                tp.set_elapsed_seconds(0);
                tp.set_is_finished(false);
                tp.set_is_paused(false);
                tp.set_is_cancelling(false);
                tp.set_can_pause(false);
                tp.set_download_items(ModelRc::default());
                let _ = tp.show();
            });

            let (svc, weak, main) = (svc.clone(), weak.clone(), main.clone());
            let rt = svc.rt.clone();
            rt.spawn(async move {
                match import(
                    &svc,
                    op_id,
                    source,
                    local,
                    project,
                    project_title,
                    project_homepage,
                    release,
                    icon,
                    name,
                    group,
                )
                .await
                {
                    Ok((instance, loader)) => {
                        let id = instance.id.clone();
                        let cfg = svc.state.lock().ok().map(|mut state| {
                            state.instances.insert(0, instance);
                            state.config.last_selected_instance = Some(id.clone());
                            state.config.clone()
                        });
                        if let Some(cfg) = cfg {
                            if let Err(err) = save_config(&svc.paths, &cfg).await {
                                tracing::warn!("Failed to save the selected instance: {err}");
                            }
                        }
                        let selected = id.clone();
                        let state = svc.state.clone();
                        let main_ui = main.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(win) = weak.upgrade() {
                                win.set_is_creating(false);
                                let _ = win.hide();
                            }
                            if let Some(win) = main_ui.upgrade() {
                                refresh_main_ui(&win, &state, Some(&selected));
                            }
                        });
                        start_instance_installation_with_loader(&main, &svc, id, loader);
                    }
                    Err(err) => {
                        let msg = err.to_string();
                        let _ = slint::invoke_from_event_loop(move || {
                            with_task_progress_op(op_id, |tp| {
                                tp.set_task_state("Failed".into());
                                tp.set_error_message(msg.as_str().into());
                                tp.set_indeterminate(false);
                                tp.set_is_finished(true);
                            });
                            if let Some(win) = weak.upgrade() {
                                win.set_is_creating(false);
                                win.set_error_message(msg.into());
                            }
                        });
                    }
                }
            });
        });
    }
}

#[derive(Default)]
struct EditModpackState {
    instance_id: String,
    pack_id: String,
    releases: Vec<Release>,
    request_seq: u64,
}

fn apply_changelog_to_edit(win: &EditInstanceWindow, changelog: Option<&str>) {
    let raw = changelog.map(str::trim).unwrap_or("");
    if raw.is_empty() {
        win.set_modpack_changelog_plain("".into());
        win.set_modpack_changelog_blocks(ModelRc::default());
        return;
    }
    let parsed: Vec<(RichBlock, Option<PathBuf>)> = parse_rich_description(raw)
        .into_iter()
        .filter(|b| !matches!(b, RichBlock::Image { .. }))
        .map(|b| (b, None))
        .collect();
    win.set_modpack_changelog_plain(render_markdown_plain(raw).into());
    win.set_modpack_changelog_blocks(ModelRc::from(Rc::new(VecModel::from(rich_blocks_to_ui(
        parsed,
    )))));
}

pub(super) fn populate_edit_modpack_tab(win: &EditInstanceWindow, inst: &InstanceMetadata) {
    if let Some(mp) = inst
        .modpack
        .as_ref()
        .filter(|m| m.provider.eq_ignore_ascii_case("modrinth"))
    {
        win.set_has_modpack(true);
        win.set_modpack_provider(mp.provider.as_str().into());
        win.set_modpack_name(mp.pack_name.as_str().into());
        win.set_modpack_id(mp.pack_id.as_str().into());
        win.set_modpack_version_id(mp.version_id.as_str().into());
        win.set_modpack_version_name(mp.version_name.as_str().into());
        let homepage = if mp.homepage_url.trim().is_empty() {
            format!("https://modrinth.com/modpack/{}", mp.pack_id)
        } else {
            mp.homepage_url.clone()
        };
        win.set_modpack_homepage_url(homepage.into());
        win.set_modpack_selected_version_id(mp.version_id.as_str().into());
        win.set_modpack_selected_version_label(format!("{} (Current)", mp.version_name).into());
        win.set_modpack_version_options(ModelRc::default());
        win.set_modpack_changelog_plain("".into());
        win.set_modpack_changelog_blocks(ModelRc::default());
        win.set_modpack_loading_versions(false);
        win.set_modpack_updating(false);
    } else {
        win.set_has_modpack(false);
        win.set_modpack_provider("".into());
        win.set_modpack_name("".into());
        win.set_modpack_id("".into());
        win.set_modpack_version_id("".into());
        win.set_modpack_version_name("".into());
        win.set_modpack_homepage_url("".into());
        win.set_modpack_version_options(ModelRc::default());
        win.set_modpack_selected_version_id("".into());
        win.set_modpack_selected_version_label("".into());
        win.set_modpack_changelog_plain("".into());
        win.set_modpack_changelog_blocks(ModelRc::default());
        win.set_modpack_loading_versions(false);
        win.set_modpack_updating(false);
        if win.get_active_page() == 9 {
            win.set_active_page(0);
        }
    }
}

async fn run_modpack_update(
    svc: &Services,
    op_id: u64,
    instance_id: &str,
    existing_mp: ManagedModpackInfo,
    remote_release: Option<Release>,
    local_archive: Option<PathBuf>,
) -> Result<(InstanceMetadata, Option<(ModloaderKind, String)>)> {
    let current_inst = svc.instance_manager.find_by_id(instance_id).await?;
    let root = svc.paths.instances_dir();
    tokio::fs::create_dir_all(&root)
        .await
        .map_err(|e| LauncherError::Launch(format!("Could not prepare instances folder: {e}")))?;
    let temporary = tempfile::Builder::new()
        .prefix(".crab-import-")
        .tempdir_in(&root)
        .map_err(|e| LauncherError::Launch(format!("Could not stage modpack update: {e}")))?;

    let (source_path, next_version_id, next_version_name) = if let Some(rel) = remote_release {
        update_task_progress_for_modpack(
            op_id,
            &StageProgress {
                stage: "Downloading modpack update from Modrinth...".into(),
                completed_files: 0,
                total_files: 0,
                downloaded_bytes: 0,
                total_bytes: 0,
                current_file: String::new(),
                indeterminate: true,
            },
        );
        let (url, hash) = rel.archive().ok_or_else(|| {
            LauncherError::Launch("This Modrinth version does not offer a modpack archive.".into())
        })?;
        let path = temporary.path().join("update.mrpack");
        modpack::download_archive(&svc.http_client, url, hash, &path).await?;
        (path, rel.id.clone(), rel.display_label())
    } else if let Some(local) = local_archive {
        let label = local
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Local File")
            .to_string();
        (
            local,
            format!("local-{}", chrono::Utc::now().timestamp()),
            label,
        )
    } else {
        return Err(LauncherError::Launch(
            "No modpack update source selected.".into(),
        ));
    };

    let path_for_inspect = source_path.clone();
    let pack = tokio::task::spawn_blocking(move || modpack::inspect(&path_for_inspect))
        .await
        .map_err(|e| LauncherError::Launch(e.to_string()))??;
    let version_type = svc
        .state
        .lock()
        .ok()
        .and_then(|s| {
            s.versions
                .iter()
                .find(|v| v.id == pack.minecraft)
                .map(|v| v.version_type)
        })
        .unwrap_or(VersionType::Release);

    let staged_game = temporary.path().join(".minecraft");
    let new_managed_files = modpack::stage_with_progress(
        &svc.http_client,
        &pack,
        source_path,
        staged_game.clone(),
        move |p| update_task_progress_for_modpack(op_id, &p),
    )
    .await?;

    update_task_progress_for_modpack(
        op_id,
        &StageProgress {
            stage: "Applying modpack files to instance...".into(),
            completed_files: 0,
            total_files: 0,
            downloaded_bytes: 0,
            total_bytes: 0,
            current_file: String::new(),
            indeterminate: true,
        },
    );

    let target_game = svc.paths.instance_game_dir(&current_inst.dir_name);
    modpack::sync_staged_update(
        staged_game,
        target_game.clone(),
        existing_mp.managed_files.clone(),
        new_managed_files.clone(),
    )
    .await?;

    update_task_progress_for_modpack(
        op_id,
        &StageProgress {
            stage: "Refreshing mod metadata and icons...".into(),
            completed_files: 0,
            total_files: 0,
            downloaded_bytes: 0,
            total_bytes: 0,
            current_file: String::new(),
            indeterminate: true,
        },
    );
    let mods_dir = target_game.join("mods");
    let _ =
        modpack::enrich_installed_mods_metadata(&svc.http_client, &mods_dir, &svc.paths.cache_dir)
            .await;

    let spec = pack.loader.as_ref().map(|(kind, version)| ModloaderSpec {
        kind: *kind,
        version: version.clone(),
    });
    let updated_mp = ManagedModpackInfo {
        provider: existing_mp.provider,
        pack_name: existing_mp.pack_name,
        pack_id: existing_mp.pack_id,
        version_id: next_version_id,
        version_name: next_version_name,
        homepage_url: existing_mp.homepage_url,
        managed_files: new_managed_files,
    };

    let updated = svc
        .instance_manager
        .apply_modpack_update(instance_id, &pack.minecraft, version_type, spec, updated_mp)
        .await?;

    // Atualiza components.json para refletir nova versão de Minecraft/modloader na aba Version.
    let inst_dir = svc.paths.instance_dir(&updated.dir_name);
    let comp_path = inst_dir.join("components.json");
    if comp_path.is_file() {
        let _ = tokio::fs::remove_file(&comp_path).await;
    }
    let _ = ComponentManifest::load_or_init(&inst_dir, &updated);

    Ok((updated, pack.loader))
}

fn load_modpack_releases_for_edit(
    win: &EditInstanceWindow,
    svc: &Services,
    edit_state: &Arc<Mutex<EditModpackState>>,
) {
    if !win.get_has_modpack() {
        return;
    }
    let inst_id = win.get_instance_id().to_string();
    let pack_id = win.get_modpack_id().to_string();
    let current_ver_id = win.get_modpack_version_id().to_string();
    if pack_id.is_empty() {
        return;
    }

    let req_seq = {
        let Ok(mut st) = edit_state.lock() else {
            return;
        };
        if st.instance_id == inst_id && st.pack_id == pack_id && !st.releases.is_empty() {
            let selected = win.get_modpack_selected_version_id().to_string();
            let chosen = st
                .releases
                .iter()
                .find(|r| r.id == selected)
                .or_else(|| st.releases.iter().find(|r| r.id == current_ver_id))
                .or_else(|| st.releases.first());
            if let Some(rel) = chosen {
                apply_changelog_to_edit(win, rel.changelog.as_deref());
            }
            return;
        }
        st.instance_id = inst_id.clone();
        st.pack_id = pack_id.clone();
        st.releases.clear();
        st.request_seq = st.request_seq.wrapping_add(1);
        st.request_seq
    };

    win.set_modpack_loading_versions(true);
    let weak = win.as_weak();
    let client = svc.http_client.clone();
    let edit_state = Arc::clone(edit_state);
    svc.rt.spawn(async move {
        let res = modpack::releases(&client, &pack_id).await;
        let _ = slint::invoke_from_event_loop(move || {
            let Ok(mut st) = edit_state.lock() else {
                return;
            };
            if st.request_seq != req_seq {
                return;
            }
            let Some(win) = weak.upgrade() else { return };
            win.set_modpack_loading_versions(false);
            match res {
                Ok(releases) if !releases.is_empty() => {
                    let current_id = win.get_modpack_version_id().to_string();
                    let opts: Vec<DropdownOption> = releases
                        .iter()
                        .map(|r| DropdownOption {
                            id: r.id.as_str().into(),
                            label: r.update_dropdown_label(r.id == current_id).into(),
                            enabled: true,
                            ..Default::default()
                        })
                        .collect();
                    let initial = releases
                        .iter()
                        .find(|r| r.id == current_id)
                        .unwrap_or(&releases[0]);
                    win.set_modpack_selected_version_id(initial.id.as_str().into());
                    win.set_modpack_selected_version_label(
                        initial
                            .update_dropdown_label(initial.id == current_id)
                            .into(),
                    );
                    apply_changelog_to_edit(&win, initial.changelog.as_deref());
                    win.set_modpack_version_options(ModelRc::from(Rc::new(VecModel::from(opts))));
                    st.releases = releases;
                }
                Ok(_) => {
                    win.set_modpack_version_options(ModelRc::default());
                    win.set_modpack_changelog_plain("".into());
                    win.set_modpack_changelog_blocks(ModelRc::default());
                }
                Err(err) => {
                    win.set_error_message(format!("Failed to load modpack versions: {err}").into());
                }
            }
        });
    });
}

pub(super) fn wire_edit_modpack(main: &MainWindow, edit: &EditInstanceWindow, svc: &Services) {
    let edit_state = Arc::new(Mutex::new(EditModpackState::default()));

    {
        let weak = edit.as_weak();
        let svc = svc.clone();
        let edit_state = Arc::clone(&edit_state);
        edit.on_modpack_tab_opened(move || {
            if let Some(win) = weak.upgrade() {
                load_modpack_releases_for_edit(&win, &svc, &edit_state);
            }
        });
    }

    {
        let weak = edit.as_weak();
        let edit_state = Arc::clone(&edit_state);
        edit.on_modpack_version_selected(move |vid| {
            let Some(win) = weak.upgrade() else { return };
            let current_id = win.get_modpack_version_id().to_string();
            let chosen = edit_state
                .lock()
                .ok()
                .and_then(|st| st.releases.iter().find(|r| r.id == vid.as_str()).cloned());
            if let Some(rel) = chosen {
                win.set_modpack_selected_version_id(vid);
                win.set_modpack_selected_version_label(
                    rel.update_dropdown_label(rel.id == current_id).into(),
                );
                apply_changelog_to_edit(&win, rel.changelog.as_deref());
            }
        });
    }

    {
        edit.on_modpack_open_url_clicked(move |url| {
            if url.starts_with("https://") || url.starts_with("http://") {
                let _ = open::that(url.as_str());
            }
        });
    }

    // Update Pack (via versão selecionada do Modrinth)
    {
        let main_weak = main.as_weak();
        let edit_weak = edit.as_weak();
        let svc = svc.clone();
        let edit_state = Arc::clone(&edit_state);
        edit.on_modpack_update_clicked(move || {
            let Some(win) = edit_weak.upgrade() else {
                return;
            };
            if win.get_is_running() || win.get_modpack_updating() {
                return;
            }
            let inst_id = win.get_instance_id().to_string();
            let target_vid = win.get_modpack_selected_version_id().to_string();
            if target_vid.is_empty() || target_vid == win.get_modpack_version_id().as_str() {
                return;
            }
            let Some(release) = edit_state
                .lock()
                .ok()
                .and_then(|st| st.releases.iter().find(|r| r.id == target_vid).cloned())
            else {
                return;
            };
            let existing_mp = svc.state.lock().ok().and_then(|g| {
                g.instances
                    .iter()
                    .find(|i| i.id == inst_id)
                    .and_then(|i| i.modpack.clone())
            });
            let Some(existing_mp) = existing_mp else {
                return;
            };

            win.set_modpack_updating(true);
            win.set_error_message("".into());
            win.set_status_message("Updating modpack...".into());

            let op_id = next_task_op_id();
            let pack_name = existing_mp.pack_name.clone();
            with_task_progress(|tp| {
                tp.set_task_title(format!("Updating Modpack: {pack_name}").into());
                tp.set_task_kind("modpack".into());
                tp.set_task_stage("Preparing modpack update...".into());
                tp.set_step_text("Updating modpack components".into());
                tp.set_task_state("Running".into());
                tp.set_overall_progress(0.0);
                tp.set_overall_percent_text("0%".into());
                tp.set_progress(0.0);
                tp.set_percent_text("0%".into());
                tp.set_indeterminate(true);
                tp.set_files_text("".into());
                tp.set_bytes_text("".into());
                tp.set_speed_text("".into());
                tp.set_current_file("".into());
                tp.set_error_message("".into());
                tp.set_elapsed_seconds(0);
                tp.set_is_finished(false);
                tp.set_is_paused(false);
                tp.set_is_cancelling(false);
                tp.set_can_pause(false);
                tp.set_download_items(ModelRc::default());
                let _ = tp.show();
            });

            let svc_c = svc.clone();
            let edit_weak_c = edit_weak.clone();
            let main_weak_c = main_weak.clone();
            let edit_state_c = Arc::clone(&edit_state);
            svc.rt.spawn(async move {
                match run_modpack_update(&svc_c, op_id, &inst_id, existing_mp, Some(release), None)
                    .await
                {
                    Ok((updated, loader)) => {
                        if let Ok(mut g) = svc_c.state.lock() {
                            if let Some(slot) = g.instances.iter_mut().find(|i| i.id == updated.id)
                            {
                                *slot = updated.clone();
                            }
                        }
                        if let Ok(mut st) = edit_state_c.lock() {
                            st.releases.clear();
                        }
                        let updated_id = updated.id.clone();
                        let svc_ui = svc_c.clone();
                        let edit_ui = edit_weak_c.clone();
                        let main_ui = main_weak_c.clone();
                        let edit_state_ui = Arc::clone(&edit_state_c);
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(w) = edit_ui.upgrade() {
                                populate_edit_instance_window(&w, &svc_ui, &updated_id);
                                w.set_active_page(9);
                                w.set_status_message("Modpack updated successfully.".into());
                                load_modpack_releases_for_edit(&w, &svc_ui, &edit_state_ui);
                            }
                            if let Some(m) = main_ui.upgrade() {
                                refresh_main_ui(&m, &svc_ui.state, Some(&updated_id));
                            }
                        });
                        start_instance_installation_with_loader(
                            &main_weak_c,
                            &svc_c,
                            updated.id,
                            loader,
                        );
                    }
                    Err(err) => {
                        let msg = err.to_string();
                        let _ = slint::invoke_from_event_loop(move || {
                            with_task_progress_op(op_id, |tp| {
                                tp.set_task_state("Failed".into());
                                tp.set_error_message(msg.as_str().into());
                                tp.set_indeterminate(false);
                                tp.set_is_finished(true);
                            });
                            if let Some(w) = edit_weak_c.upgrade() {
                                w.set_modpack_updating(false);
                                w.set_status_message("".into());
                                w.set_error_message(msg.into());
                            }
                        });
                    }
                }
            });
        });
    }

    // Update From File (arquivo .mrpack ou .zip local)
    {
        let main_weak = main.as_weak();
        let edit_weak = edit.as_weak();
        let svc = svc.clone();
        let edit_state = Arc::clone(&edit_state);
        edit.on_modpack_update_from_file_clicked(move || {
            let Some(win) = edit_weak.upgrade() else {
                return;
            };
            if win.get_is_running() || win.get_modpack_updating() {
                return;
            }
            let inst_id = win.get_instance_id().to_string();
            let existing_mp = svc.state.lock().ok().and_then(|g| {
                g.instances
                    .iter()
                    .find(|i| i.id == inst_id)
                    .and_then(|i| i.modpack.clone())
            });
            let Some(existing_mp) = existing_mp else {
                return;
            };

            let svc_c = svc.clone();
            let edit_weak_c = edit_weak.clone();
            let main_weak_c = main_weak.clone();
            let edit_state_c = Arc::clone(&edit_state);
            svc.rt.spawn(async move {
                let picked = tokio::task::spawn_blocking(|| -> Result<Option<PathBuf>> {
                    #[cfg(target_os = "linux")]
                    {
                        crab_launcher::platform::linux::pick_modpack_file_native()
                    }
                    #[cfg(not(target_os = "linux"))]
                    {
                        Err(LauncherError::Launch(
                            "Native file picker is not configured for this platform.".into(),
                        ))
                    }
                })
                .await
                .map_err(|e| LauncherError::Launch(e.to_string()))
                .and_then(|r| r);

                let local_path = match picked {
                    Ok(Some(p)) => p,
                    Ok(None) => return,
                    Err(err) => {
                        let msg = err.to_string();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(w) = edit_weak_c.upgrade() {
                                w.set_error_message(msg.into());
                            }
                        });
                        return;
                    }
                };

                let op_id = next_task_op_id();
                let pack_name = existing_mp.pack_name.clone();
                let edit_ui_start = edit_weak_c.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(w) = edit_ui_start.upgrade() {
                        w.set_modpack_updating(true);
                        w.set_error_message("".into());
                        w.set_status_message("Updating modpack from file...".into());
                    }
                    with_task_progress(|tp| {
                        tp.set_task_title(format!("Updating Modpack: {pack_name}").into());
                        tp.set_task_kind("modpack".into());
                        tp.set_task_stage("Reading modpack archive...".into());
                        tp.set_step_text("Updating modpack from file".into());
                        tp.set_task_state("Running".into());
                        tp.set_overall_progress(0.0);
                        tp.set_overall_percent_text("0%".into());
                        tp.set_progress(0.0);
                        tp.set_percent_text("0%".into());
                        tp.set_indeterminate(true);
                        tp.set_files_text("".into());
                        tp.set_bytes_text("".into());
                        tp.set_speed_text("".into());
                        tp.set_current_file("".into());
                        tp.set_error_message("".into());
                        tp.set_elapsed_seconds(0);
                        tp.set_is_finished(false);
                        tp.set_is_paused(false);
                        tp.set_is_cancelling(false);
                        tp.set_can_pause(false);
                        tp.set_download_items(ModelRc::default());
                        let _ = tp.show();
                    });
                });

                match run_modpack_update(
                    &svc_c,
                    op_id,
                    &inst_id,
                    existing_mp,
                    None,
                    Some(local_path),
                )
                .await
                {
                    Ok((updated, loader)) => {
                        if let Ok(mut g) = svc_c.state.lock() {
                            if let Some(slot) = g.instances.iter_mut().find(|i| i.id == updated.id)
                            {
                                *slot = updated.clone();
                            }
                        }
                        if let Ok(mut st) = edit_state_c.lock() {
                            st.releases.clear();
                        }
                        let updated_id = updated.id.clone();
                        let svc_ui = svc_c.clone();
                        let edit_ui = edit_weak_c.clone();
                        let main_ui = main_weak_c.clone();
                        let edit_state_ui = Arc::clone(&edit_state_c);
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(w) = edit_ui.upgrade() {
                                populate_edit_instance_window(&w, &svc_ui, &updated_id);
                                w.set_active_page(9);
                                w.set_status_message("Modpack updated from file.".into());
                                load_modpack_releases_for_edit(&w, &svc_ui, &edit_state_ui);
                            }
                            if let Some(m) = main_ui.upgrade() {
                                refresh_main_ui(&m, &svc_ui.state, Some(&updated_id));
                            }
                        });
                        start_instance_installation_with_loader(
                            &main_weak_c,
                            &svc_c,
                            updated.id,
                            loader,
                        );
                    }
                    Err(err) => {
                        let msg = err.to_string();
                        let _ = slint::invoke_from_event_loop(move || {
                            with_task_progress_op(op_id, |tp| {
                                tp.set_task_state("Failed".into());
                                tp.set_error_message(msg.as_str().into());
                                tp.set_indeterminate(false);
                                tp.set_is_finished(true);
                            });
                            if let Some(w) = edit_weak_c.upgrade() {
                                w.set_modpack_updating(false);
                                w.set_status_message("".into());
                                w.set_error_message(msg.into());
                            }
                        });
                    }
                }
            });
        });
    }
}
