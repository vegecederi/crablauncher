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
];

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
