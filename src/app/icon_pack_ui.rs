use super::*;
use crab_launcher::core::icon_packs::load_icon_svg_bytes;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;

thread_local! {
    static ICON_IMAGE_CACHE: RefCell<HashMap<(String, String), slint::Image>> =
        RefCell::new(HashMap::new());
}

const PREVIEW_ICON_KEYS: &[&str] = &[
    "plus",
    "folder",
    "settings",
    "play",
    "edit",
    "puzzle",
    "file-text",
    "search",
    "refresh",
    "trash",
];

fn load_cached_icon(pack_id: &str, icon_key: &str, user_dir: &Path) -> Option<slint::Image> {
    let cache_key = (pack_id.to_string(), icon_key.to_string());
    if let Some(img) = ICON_IMAGE_CACHE.with(|c| c.borrow().get(&cache_key).cloned()) {
        return Some(img);
    }
    let bytes = load_icon_svg_bytes(pack_id, icon_key, user_dir)?;
    let img = slint::Image::load_from_svg_data(&bytes).ok()?;
    ICON_IMAGE_CACHE.with(|c| {
        c.borrow_mut().insert(cache_key, img.clone());
    });
    Some(img)
}

/// Limpa o cache de ícones (chamado ao clicar Refresh em Icon Packs para recarregar packs customizados).
pub(super) fn clear_icon_cache() {
    ICON_IMAGE_CACHE.with(|c| c.borrow_mut().clear());
}

/// Gera o modelo com os 10 ícones de amostra para o painel Icon Preview.
pub(super) fn build_preview_images(pack_id: &str, user_dir: &Path) -> ModelRc<slint::Image> {
    let imgs: Vec<slint::Image> = PREVIEW_ICON_KEYS
        .iter()
        .map(|k| load_cached_icon(pack_id, k, user_dir).unwrap_or_default())
        .collect();
    ModelRc::from(Rc::new(VecModel::from(imgs)))
}

/// Aplica o Icon Pack ativo (cor + todos os 40 SVGs dinâmicos) ao global `Theme` de uma janela Slint.
pub(super) fn apply_icon_pack_to_global(theme_global: &Theme, pack_id: &str, user_dir: &Path) {
    theme_global.set_icon_pack_id(SharedString::from(pack_id));
    theme_global.set_icon_pack_dark(pack_id == "lucide-dark");

    macro_rules! set_icon {
        ($key:literal, $setter:ident) => {
            if let Some(img) = load_cached_icon(pack_id, $key, user_dir) {
                theme_global.$setter(img);
            }
        };
    }

    set_icon!("arrow-down", set_icon_arrow_down);
    set_icon!("arrow-up", set_icon_arrow_up);
    set_icon!("calendar", set_icon_calendar);
    set_icon!("check", set_icon_check);
    set_icon!("chevron-down", set_icon_chevron_down);
    set_icon!("chevron-left", set_icon_chevron_left);
    set_icon!("chevron-right", set_icon_chevron_right);
    set_icon!("chevron-up", set_icon_chevron_up);
    set_icon!("clock", set_icon_clock);
    set_icon!("coffee", set_icon_coffee);
    set_icon!("copy", set_icon_copy);
    set_icon!("cube", set_icon_cube);
    set_icon!("download", set_icon_download);
    set_icon!("edit", set_icon_edit);
    set_icon!("external-link", set_icon_external_link);
    set_icon!("file-text", set_icon_file_text);
    set_icon!("flask-conical", set_icon_flask_conical);
    set_icon!("folder-plus", set_icon_folder_plus);
    set_icon!("folder", set_icon_folder);
    set_icon!("info", set_icon_info);
    set_icon!("layers", set_icon_layers);
    set_icon!("layout-grid", set_icon_layout_grid);
    set_icon!("palette", set_icon_palette);
    set_icon!("play", set_icon_play);
    set_icon!("plus", set_icon_plus);
    set_icon!("puzzle", set_icon_puzzle);
    set_icon!("refresh", set_icon_refresh);
    set_icon!("save", set_icon_save);
    set_icon!("scan-search", set_icon_scan_search);
    set_icon!("search", set_icon_search);
    set_icon!("settings", set_icon_settings);
    set_icon!("sliders-horizontal", set_icon_sliders_horizontal);
    set_icon!("square", set_icon_square);
    set_icon!("star", set_icon_star);
    set_icon!("terminal", set_icon_terminal);
    set_icon!("trash", set_icon_trash);
    set_icon!("user-plus", set_icon_user_plus);
    set_icon!("user", set_icon_user);
    set_icon!("wrench", set_icon_wrench);
    set_icon!("x", set_icon_x);
}
