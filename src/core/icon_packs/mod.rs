use crate::error::{LauncherError, Result};
use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};
use zip::ZipArchive;

pub const DEFAULT_ICON_PACK_ID: &str = "lucide";

const BUILTIN_ICON_PACKS: &[(&str, &str)] = &[
    (
        "lucide",
        include_str!("../../../assets/icon-packs/lucide/manifest.json"),
    ),
    (
        "lucide-dark",
        include_str!("../../../assets/icon-packs/lucide-dark/manifest.json"),
    ),
    (
        "phosphor-fill",
        include_str!("../../../assets/icon-packs/phosphor-fill/manifest.json"),
    ),
    (
        "tabler",
        include_str!("../../../assets/icon-packs/tabler/manifest.json"),
    ),
    (
        "fluent-system",
        include_str!("../../../assets/icon-packs/fluent-system/manifest.json"),
    ),
];

/// Lista canônica de chaves de ícones temáticos suportados pelos Icon Packs.
pub const THEMED_ICON_KEYS: &[&str] = &[
    "arrow-down",
    "arrow-up",
    "calendar",
    "check",
    "chevron-down",
    "chevron-left",
    "chevron-right",
    "chevron-up",
    "clock",
    "coffee",
    "copy",
    "cube",
    "download",
    "edit",
    "external-link",
    "file-text",
    "flask-conical",
    "folder-plus",
    "folder",
    "info",
    "layers",
    "layout-grid",
    "palette",
    "play",
    "plus",
    "puzzle",
    "refresh",
    "save",
    "scan-search",
    "search",
    "settings",
    "sliders-horizontal",
    "square",
    "star",
    "terminal",
    "trash",
    "user-plus",
    "user",
    "wrench",
    "x",
];

macro_rules! match_pack_icons {
    ($dir:literal, $key:expr) => {
        match $key {
            "arrow-down" => Some(include_bytes!(concat!(
                "../../../assets/icon-packs/",
                $dir,
                "/icons/arrow-down.svg"
            ))),
            "arrow-up" => Some(include_bytes!(concat!(
                "../../../assets/icon-packs/",
                $dir,
                "/icons/arrow-up.svg"
            ))),
            "calendar" => Some(include_bytes!(concat!(
                "../../../assets/icon-packs/",
                $dir,
                "/icons/calendar.svg"
            ))),
            "check" => Some(include_bytes!(concat!(
                "../../../assets/icon-packs/",
                $dir,
                "/icons/check.svg"
            ))),
            "chevron-down" => Some(include_bytes!(concat!(
                "../../../assets/icon-packs/",
                $dir,
                "/icons/chevron-down.svg"
            ))),
            "chevron-left" => Some(include_bytes!(concat!(
                "../../../assets/icon-packs/",
                $dir,
                "/icons/chevron-left.svg"
            ))),
            "chevron-right" => Some(include_bytes!(concat!(
                "../../../assets/icon-packs/",
                $dir,
                "/icons/chevron-right.svg"
            ))),
            "chevron-up" => Some(include_bytes!(concat!(
                "../../../assets/icon-packs/",
                $dir,
                "/icons/chevron-up.svg"
            ))),
            "clock" => Some(include_bytes!(concat!(
                "../../../assets/icon-packs/",
                $dir,
                "/icons/clock.svg"
            ))),
            "coffee" => Some(include_bytes!(concat!(
                "../../../assets/icon-packs/",
                $dir,
                "/icons/coffee.svg"
            ))),
            "copy" => Some(include_bytes!(concat!(
                "../../../assets/icon-packs/",
                $dir,
                "/icons/copy.svg"
            ))),
            "cube" => Some(include_bytes!(concat!(
                "../../../assets/icon-packs/",
                $dir,
                "/icons/cube.svg"
            ))),
            "download" => Some(include_bytes!(concat!(
                "../../../assets/icon-packs/",
                $dir,
                "/icons/download.svg"
            ))),
            "edit" => Some(include_bytes!(concat!(
                "../../../assets/icon-packs/",
                $dir,
                "/icons/edit.svg"
            ))),
            "external-link" => Some(include_bytes!(concat!(
                "../../../assets/icon-packs/",
                $dir,
                "/icons/external-link.svg"
            ))),
            "file-text" => Some(include_bytes!(concat!(
                "../../../assets/icon-packs/",
                $dir,
                "/icons/file-text.svg"
            ))),
            "flask-conical" => Some(include_bytes!(concat!(
                "../../../assets/icon-packs/",
                $dir,
                "/icons/flask-conical.svg"
            ))),
            "folder-plus" => Some(include_bytes!(concat!(
                "../../../assets/icon-packs/",
                $dir,
                "/icons/folder-plus.svg"
            ))),
            "folder" => Some(include_bytes!(concat!(
                "../../../assets/icon-packs/",
                $dir,
                "/icons/folder.svg"
            ))),
            "info" => Some(include_bytes!(concat!(
                "../../../assets/icon-packs/",
                $dir,
                "/icons/info.svg"
            ))),
            "layers" => Some(include_bytes!(concat!(
                "../../../assets/icon-packs/",
                $dir,
                "/icons/layers.svg"
            ))),
            "layout-grid" => Some(include_bytes!(concat!(
                "../../../assets/icon-packs/",
                $dir,
                "/icons/layout-grid.svg"
            ))),
            "palette" => Some(include_bytes!(concat!(
                "../../../assets/icon-packs/",
                $dir,
                "/icons/palette.svg"
            ))),
            "play" => Some(include_bytes!(concat!(
                "../../../assets/icon-packs/",
                $dir,
                "/icons/play.svg"
            ))),
            "plus" => Some(include_bytes!(concat!(
                "../../../assets/icon-packs/",
                $dir,
                "/icons/plus.svg"
            ))),
            "puzzle" => Some(include_bytes!(concat!(
                "../../../assets/icon-packs/",
                $dir,
                "/icons/puzzle.svg"
            ))),
            "refresh" => Some(include_bytes!(concat!(
                "../../../assets/icon-packs/",
                $dir,
                "/icons/refresh.svg"
            ))),
            "save" => Some(include_bytes!(concat!(
                "../../../assets/icon-packs/",
                $dir,
                "/icons/save.svg"
            ))),
            "scan-search" => Some(include_bytes!(concat!(
                "../../../assets/icon-packs/",
                $dir,
                "/icons/scan-search.svg"
            ))),
            "search" => Some(include_bytes!(concat!(
                "../../../assets/icon-packs/",
                $dir,
                "/icons/search.svg"
            ))),
            "settings" => Some(include_bytes!(concat!(
                "../../../assets/icon-packs/",
                $dir,
                "/icons/settings.svg"
            ))),
            "sliders-horizontal" => Some(include_bytes!(concat!(
                "../../../assets/icon-packs/",
                $dir,
                "/icons/sliders-horizontal.svg"
            ))),
            "square" => Some(include_bytes!(concat!(
                "../../../assets/icon-packs/",
                $dir,
                "/icons/square.svg"
            ))),
            "star" => Some(include_bytes!(concat!(
                "../../../assets/icon-packs/",
                $dir,
                "/icons/star.svg"
            ))),
            "terminal" => Some(include_bytes!(concat!(
                "../../../assets/icon-packs/",
                $dir,
                "/icons/terminal.svg"
            ))),
            "trash" => Some(include_bytes!(concat!(
                "../../../assets/icon-packs/",
                $dir,
                "/icons/trash.svg"
            ))),
            "user-plus" => Some(include_bytes!(concat!(
                "../../../assets/icon-packs/",
                $dir,
                "/icons/user-plus.svg"
            ))),
            "user" => Some(include_bytes!(concat!(
                "../../../assets/icon-packs/",
                $dir,
                "/icons/user.svg"
            ))),
            "wrench" => Some(include_bytes!(concat!(
                "../../../assets/icon-packs/",
                $dir,
                "/icons/wrench.svg"
            ))),
            "x" => Some(include_bytes!(concat!(
                "../../../assets/icon-packs/",
                $dir,
                "/icons/x.svg"
            ))),
            _ => None,
        }
    };
}

/// Retorna os bytes SVG embutidos de um ícone para um pacote oficial.
pub fn builtin_icon_svg_bytes(pack_id: &str, icon_key: &str) -> Option<&'static [u8]> {
    match pack_id {
        "phosphor-fill" => match_pack_icons!("phosphor-fill", icon_key),
        "tabler" => match_pack_icons!("tabler", icon_key),
        "fluent-system" => match_pack_icons!("fluent-system", icon_key),
        "lucide" | "lucide-dark" => match_pack_icons!("lucide", icon_key),
        _ => None,
    }
}

/// Carrega os bytes SVG de um ícone para qualquer pacote (oficial ou customizado em disco),
/// usando Lucide como fallback automático quando o ícone não existir no pacote.
pub fn load_icon_svg_bytes(
    pack_id: &str,
    icon_key: &str,
    user_icon_packs_dir: &Path,
) -> Option<Vec<u8>> {
    if let Some(bytes) = builtin_icon_svg_bytes(pack_id, icon_key) {
        return Some(bytes.to_vec());
    }
    if is_valid_pack_id(pack_id) && !icon_key.contains("..") && !icon_key.contains('/') {
        let custom_path = user_icon_packs_dir
            .join(pack_id)
            .join("icons")
            .join(format!("{icon_key}.svg"));
        if let Ok(raw) = std::fs::read(&custom_path) {
            return Some(raw);
        }
        // Também tenta localizar pelo campo `id` no manifest de subpastas customizadas
        for pack in discover_icon_packs(user_icon_packs_dir) {
            if pack.id == pack_id {
                if let Some(dir) = pack.icons_dir {
                    if let Ok(raw) = std::fs::read(dir.join(format!("{icon_key}.svg"))) {
                        return Some(raw);
                    }
                }
            }
        }
    }
    builtin_icon_svg_bytes(DEFAULT_ICON_PACK_ID, icon_key).map(|b| b.to_vec())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IconPackManifest {
    #[serde(default = "default_schema")]
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub preserve_colors: bool,
}

fn default_schema() -> u32 {
    1
}

#[derive(Debug, Clone)]
pub struct ResolvedIconPack {
    pub id: String,
    pub name: String,
    pub author: String,
    pub version: String,
    pub description: String,
    pub is_builtin: bool,
    pub preserve_colors: bool,
    pub icons_dir: Option<PathBuf>,
}

/// Descobre pacotes oficiais embutidos (`lucide`, `lucide-dark`) e pacotes personalizados
/// em `<data_dir>/icon-packs/<pack>/manifest.json`.
pub fn discover_icon_packs(user_icon_packs_dir: &Path) -> Vec<ResolvedIconPack> {
    let mut packs = Vec::new();

    for (_, json_str) in BUILTIN_ICON_PACKS {
        if let Ok(m) = serde_json::from_str::<IconPackManifest>(json_str) {
            packs.push(ResolvedIconPack {
                id: m.id,
                name: m.name,
                author: m.author,
                version: m.version,
                description: m.description,
                is_builtin: true,
                preserve_colors: m.preserve_colors,
                icons_dir: None,
            });
        }
    }

    if user_icon_packs_dir.is_dir() {
        if let Ok(entries) = std::fs::read_dir(user_icon_packs_dir) {
            for entry in entries.flatten() {
                let dir = entry.path();
                if !dir.is_dir() {
                    continue;
                }
                let manifest_path = dir.join("manifest.json");
                if !manifest_path.is_file() {
                    continue;
                }
                if let Ok(raw) = std::fs::read_to_string(&manifest_path) {
                    if let Ok(m) = serde_json::from_str::<IconPackManifest>(&raw) {
                        if !is_valid_pack_id(&m.id) || packs.iter().any(|p| p.id == m.id) {
                            continue;
                        }
                        packs.push(ResolvedIconPack {
                            id: m.id,
                            name: m.name,
                            author: m.author,
                            version: m.version,
                            description: m.description,
                            is_builtin: false,
                            preserve_colors: m.preserve_colors,
                            icons_dir: Some(dir.join("icons")),
                        });
                    }
                }
            }
        }
    }

    packs
}

/// Resolve o caminho de um ícone SVG para o pacote selecionado, usando `lucide` como fallback
/// caso o ícone esteja ausente em um pacote personalizado.
pub fn resolve_icon_svg_path(pack: &ResolvedIconPack, icon_file: &str) -> Option<PathBuf> {
    if icon_file.contains("..") || icon_file.contains('/') || icon_file.contains('\\') {
        return None;
    }
    if let Some(ref dir) = pack.icons_dir {
        let candidate = dir.join(icon_file);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    let builtin_dir = PathBuf::from(format!("assets/icon-packs/{}/icons", pack.id));
    let candidate = builtin_dir.join(icon_file);
    if candidate.is_file() {
        return Some(candidate);
    }
    // Fallback automático para Lucide
    let fallback = PathBuf::from("assets/icon-packs/lucide/icons").join(icon_file);
    if fallback.is_file() {
        Some(fallback)
    } else {
        None
    }
}

/// Importa um pacote de ícones personalizado a partir de um `.zip` (com proteção contra Zip Slip).
pub fn import_icon_pack_zip(
    zip_path: &Path,
    user_icon_packs_dir: &Path,
) -> Result<ResolvedIconPack> {
    std::fs::create_dir_all(user_icon_packs_dir).map_err(|source| LauncherError::Io {
        path: user_icon_packs_dir.to_path_buf(),
        source,
    })?;
    let stem = zip_path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "custom-icons".into());
    let safe_dir_name: String = stem
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let dest = user_icon_packs_dir.join(&safe_dir_name);
    if dest.exists() {
        return Err(LauncherError::InvalidMetadata(format!(
            "An icon pack named '{safe_dir_name}' already exists."
        )));
    }
    std::fs::create_dir_all(&dest).map_err(|source| LauncherError::Io {
        path: dest.clone(),
        source,
    })?;

    let file = std::fs::File::open(zip_path).map_err(|source| LauncherError::Io {
        path: zip_path.to_path_buf(),
        source,
    })?;
    let mut archive = ZipArchive::new(file)
        .map_err(|e| LauncherError::InvalidMetadata(format!("Invalid icon pack ZIP: {e}")))?;

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| LauncherError::InvalidMetadata(format!("ZIP entry error: {e}")))?;
        let rel = Path::new(entry.name());
        if rel.components().any(|c| {
            matches!(
                c,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        }) {
            let _ = std::fs::remove_dir_all(&dest);
            return Err(LauncherError::ZipSlipAttempt {
                entry: entry.name().to_string(),
            });
        }
        let out_path = dest.join(rel);
        if entry.is_dir() {
            std::fs::create_dir_all(&out_path).map_err(|source| LauncherError::Io {
                path: out_path,
                source,
            })?;
        } else {
            if let Some(p) = out_path.parent() {
                std::fs::create_dir_all(p).map_err(|source| LauncherError::Io {
                    path: p.to_path_buf(),
                    source,
                })?;
            }
            let mut out = std::fs::File::create(&out_path).map_err(|source| LauncherError::Io {
                path: out_path,
                source,
            })?;
            std::io::copy(&mut entry, &mut out).map_err(|source| LauncherError::Io {
                path: zip_path.to_path_buf(),
                source,
            })?;
        }
    }

    let manifest_path = dest.join("manifest.json");
    if !manifest_path.is_file() {
        let _ = std::fs::remove_dir_all(&dest);
        return Err(LauncherError::InvalidMetadata(
            "Icon pack ZIP is missing manifest.json.".into(),
        ));
    }
    let raw = std::fs::read_to_string(&manifest_path).map_err(|source| LauncherError::Io {
        path: manifest_path,
        source,
    })?;
    let m: IconPackManifest = serde_json::from_str(&raw)?;
    Ok(ResolvedIconPack {
        id: m.id,
        name: m.name,
        author: m.author,
        version: m.version,
        description: m.description,
        is_builtin: false,
        preserve_colors: m.preserve_colors,
        icons_dir: Some(dest.join("icons")),
    })
}

fn is_valid_pack_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}
