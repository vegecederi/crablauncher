use crate::core::themes::model::{ResolvedTheme, RgbaColor, ThemeManifest};
use crate::error::{LauncherError, Result};
use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};
use tracing::warn;

const BUILTIN_THEMES: &[(&str, &str)] = &[
    (
        "crab-light",
        include_str!("../../../assets/themes/crab-light/theme.toml"),
    ),
    (
        "crab-dark",
        include_str!("../../../assets/themes/crab-dark/theme.toml"),
    ),
    (
        "nord",
        include_str!("../../../assets/themes/nord/theme.toml"),
    ),
    (
        "legacy",
        include_str!("../../../assets/themes/legacy/theme.toml"),
    ),
    (
        "deep-dark",
        include_str!("../../../assets/themes/deep-dark/theme.toml"),
    ),
    (
        "amoled",
        include_str!("../../../assets/themes/amoled/theme.toml"),
    ),
    (
        "vice",
        include_str!("../../../assets/themes/vice/theme.toml"),
    ),
];

struct RawThemeEntry {
    manifest: ThemeManifest,
    is_builtin: bool,
    source_dir: Option<PathBuf>,
}

/// Descobre todos os temas oficiais embutidos e temas personalizados em `<data_dir>/themes/`,
/// resolvendo herança (`base`), validando cores/caminhos e detectando ciclos.
pub fn discover_and_load_themes(user_themes_dir: &Path) -> Vec<ResolvedTheme> {
    let mut raw_map: HashMap<String, RawThemeEntry> = HashMap::new();

    // 1. Temas oficiais embutidos no binário
    for (id, toml_str) in BUILTIN_THEMES {
        match parse_and_validate_manifest(toml_str) {
            Ok(manifest) => {
                raw_map.insert(
                    manifest.metadata.id.clone(),
                    RawThemeEntry {
                        manifest,
                        is_builtin: true,
                        source_dir: None,
                    },
                );
            }
            Err(err) => warn!("Failed to parse builtin theme '{id}': {err}"),
        }
    }

    // 2. Temas personalizados do usuário
    if user_themes_dir.is_dir() {
        if let Ok(entries) = std::fs::read_dir(user_themes_dir) {
            for entry in entries.flatten() {
                let dir = entry.path();
                if !dir.is_dir() {
                    continue;
                }
                let manifest_path = dir.join("theme.toml");
                if !manifest_path.is_file() {
                    continue;
                }
                match std::fs::read_to_string(&manifest_path)
                    .ok()
                    .and_then(|s| parse_and_validate_manifest(&s).ok())
                {
                    Some(manifest) => {
                        if raw_map.contains_key(&manifest.metadata.id) {
                            warn!(
                                "Skipping custom theme with duplicate ID '{}' at {}",
                                manifest.metadata.id,
                                dir.display()
                            );
                            continue;
                        }
                        raw_map.insert(
                            manifest.metadata.id.clone(),
                            RawThemeEntry {
                                manifest,
                                is_builtin: false,
                                source_dir: Some(dir),
                            },
                        );
                    }
                    None => {
                        warn!(
                            "Skipping invalid custom theme manifest at {}",
                            manifest_path.display()
                        );
                    }
                }
            }
        }
    }

    let mut resolved = Vec::new();
    for id in raw_map.keys() {
        let mut visited = HashSet::new();
        if let Ok(theme) = resolve_theme_recursive(id, &raw_map, &mut visited) {
            resolved.push(theme);
        }
    }

    // Ordena: oficiais primeiro (na ordem padrão), depois personalizados por nome.
    let order = [
        "crab-light",
        "crab-dark",
        "amoled",
        "nord",
        "legacy",
        "deep-dark",
    ];
    resolved.sort_by(|a, b| {
        let pos_a = order.iter().position(|&x| x == a.id).unwrap_or(99);
        let pos_b = order.iter().position(|&x| x == b.id).unwrap_or(99);
        pos_a.cmp(&pos_b).then_with(|| a.name.cmp(&b.name))
    });
    resolved
}

/// Faz parse e valida um manifesto TOML (proteção contra path traversal e valores inválidos).
pub fn parse_and_validate_manifest(toml_str: &str) -> Result<ThemeManifest> {
    let manifest: ThemeManifest = toml::from_str(toml_str)
        .map_err(|err| LauncherError::UnsafePath(format!("Invalid theme.toml: {err}")))?;

    if manifest.metadata.id.trim().is_empty() || manifest.metadata.name.trim().is_empty() {
        return Err(LauncherError::UnsafePath(
            "Theme metadata id and name are required.".to_string(),
        ));
    }

    if let Some(wp) = &manifest.wallpaper {
        if !wp.path.is_empty() {
            let p = Path::new(&wp.path);
            if p.is_absolute()
                || p.components().any(|c| {
                    matches!(
                        c,
                        Component::ParentDir | Component::RootDir | Component::Prefix(_)
                    )
                })
            {
                return Err(LauncherError::UnsafePath(format!(
                    "Unsafe wallpaper path in theme '{}': {}",
                    manifest.metadata.id, wp.path
                )));
            }
        }
    }

    Ok(manifest)
}

fn resolve_theme_recursive(
    id: &str,
    map: &HashMap<String, RawThemeEntry>,
    visited: &mut HashSet<String>,
) -> Result<ResolvedTheme> {
    if !visited.insert(id.to_string()) {
        return Err(LauncherError::UnsafePath(format!(
            "Circular theme inheritance detected at '{id}'"
        )));
    }

    let entry = map
        .get(id)
        .ok_or_else(|| LauncherError::UnsafePath(format!("Theme '{id}' not found")))?;

    let mut base = if let Some(base_id) = &entry.manifest.metadata.base {
        resolve_theme_recursive(base_id, map, visited)?
    } else {
        ResolvedTheme::default()
    };

    base.id = entry.manifest.metadata.id.clone();
    base.name = entry.manifest.metadata.name.clone();
    base.author = entry.manifest.metadata.author.clone();
    base.version = entry.manifest.metadata.version.clone();
    base.description = entry.manifest.metadata.description.clone();
    base.is_builtin = entry.is_builtin;
    base.source_dir = entry.source_dir.clone();

    let c = &entry.manifest.colors;
    apply_color(&mut base.bg_primary, c.background.as_deref());
    apply_color(&mut base.bg_secondary, c.surface.as_deref());
    apply_color(&mut base.bg_interactive, c.surface_elevated.as_deref());
    apply_color(&mut base.bg_interactive_hover, c.hover.as_deref());
    apply_color(&mut base.bg_interactive_pressed, c.pressed.as_deref());
    apply_color(&mut base.border, c.border.as_deref());
    apply_color(&mut base.border_subtle, c.border_subtle.as_deref());
    apply_color(&mut base.accent, c.accent.as_deref());
    apply_color(&mut base.accent_hover, c.accent_hover.as_deref());
    apply_color(&mut base.accent_pressed, c.accent_pressed.as_deref());
    apply_color(&mut base.accent_muted, c.accent_muted.as_deref());
    apply_color(&mut base.text_on_accent, c.accent_foreground.as_deref());
    apply_color(&mut base.text_primary, c.text_primary.as_deref());
    apply_color(&mut base.text_secondary, c.text_secondary.as_deref());
    apply_color(&mut base.text_disabled, c.text_disabled.as_deref());
    apply_color(&mut base.danger, c.danger.as_deref());
    apply_color(&mut base.success, c.success.as_deref());
    apply_color(&mut base.warning_text, c.warning.as_deref());
    apply_color(&mut base.console_bg, c.console_bg.as_deref());

    if let Some(radius) = entry.manifest.geometry.border_radius {
        base.border_radius = radius.min(12);
    }

    Ok(base)
}

fn apply_color(target: &mut RgbaColor, hex: Option<&str>) {
    if let Some(h) = hex {
        if let Some(parsed) = RgbaColor::parse_hex(h) {
            *target = parsed;
        }
    }
}

/// Importa um tema personalizado em formato `.zip` contendo `theme.toml` para `user_themes_dir`.
pub fn import_theme_zip(
    zip_path: &Path,
    user_themes_dir: &Path,
) -> std::result::Result<String, String> {
    let file = std::fs::File::open(zip_path).map_err(|e| format!("Failed to open ZIP: {e}"))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("Invalid theme ZIP: {e}"))?;

    let mut manifest_raw: Option<String> = None;
    let mut root_prefix = String::new();
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| format!("ZIP read error: {e}"))?;
        let name = entry.name().replace('\\', "/");
        if name == "theme.toml" || name.ends_with("/theme.toml") {
            let mut s = String::new();
            std::io::Read::read_to_string(&mut entry, &mut s)
                .map_err(|e| format!("Failed to read theme.toml: {e}"))?;
            if let Some(idx) = name.rfind("theme.toml") {
                root_prefix = name[..idx].to_string();
            }
            manifest_raw = Some(s);
            break;
        }
    }

    let raw = manifest_raw.ok_or_else(|| "ZIP does not contain theme.toml".to_string())?;
    let manifest: ThemeManifest =
        toml::from_str(&raw).map_err(|e| format!("Invalid theme.toml: {e}"))?;
    let safe_id: String = manifest
        .metadata
        .id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    if safe_id.is_empty() {
        return Err("Theme metadata.id is empty or invalid".into());
    }

    let dest_dir = user_themes_dir.join(&safe_id);
    std::fs::create_dir_all(&dest_dir)
        .map_err(|e| format!("Failed to create theme directory: {e}"))?;

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| format!("ZIP read error: {e}"))?;
        let name = entry.name().replace('\\', "/");
        let rel = name.strip_prefix(&root_prefix).unwrap_or(&name);
        if rel.is_empty() || rel.ends_with('/') {
            continue;
        }
        let rel_path = Path::new(rel);
        if rel_path.is_absolute()
            || rel_path
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Err(format!("Unsafe path inside theme ZIP: {rel}"));
        }
        let target = dest_dir.join(rel_path);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create directory: {e}"))?;
        }
        let mut out =
            std::fs::File::create(&target).map_err(|e| format!("Failed to create file: {e}"))?;
        std::io::copy(&mut entry, &mut out).map_err(|e| format!("Failed to extract file: {e}"))?;
    }

    Ok(safe_id)
}
