use crate::error::{LauncherError, Result};
use chrono::{DateTime, Local};
use std::io::Read;
use std::path::{Path, PathBuf};
use zip::ZipArchive;

/// Resultado da verificação de compatibilidade de um Resource Pack com a versão do Minecraft.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackCompatibility {
    Compatible,
    Unknown,
    Incompatible { reason: String },
}

/// Metadados de um Texture Pack / Resource Pack ou Shader Pack local.
#[derive(Debug, Clone)]
pub struct LocalPackInfo {
    pub file_name: String,
    pub display_name: String,
    pub description: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub size_bytes: u64,
    pub last_modified: String,
    pub icon_png: Option<Vec<u8>>,
    pub pack_format: Option<u32>,
    pub supported_formats: Option<(u32, u32)>,
    pub pack_format_display: String,
}

/// Determina o diretório apropriado para Texture/Resource Packs conforme a versão do Minecraft:
/// `.minecraft/texturepacks` para versões antigas (< 1.6) e `.minecraft/resourcepacks` para 1.6+.
pub fn texture_packs_dir(minecraft_dir: &Path, mc_version: &str) -> PathBuf {
    let is_legacy = if mc_version.starts_with("b1.") || mc_version.starts_with("a1.") {
        true
    } else if let Some(rest) = mc_version.strip_prefix("1.") {
        rest.split('.')
            .next()
            .and_then(|minor| minor.parse::<u32>().ok())
            .map(|minor| minor <= 5)
            .unwrap_or(false)
    } else {
        false
    };
    if is_legacy {
        minecraft_dir.join("texturepacks")
    } else {
        minecraft_dir.join("resourcepacks")
    }
}

pub fn shader_packs_dir(minecraft_dir: &Path) -> PathBuf {
    minecraft_dir.join("shaderpacks")
}

/// Mapeia uma versão release do Minecraft (1.6+) para o `pack_format` esperado de resource packs.
/// Retorna `None` para versões legadas (< 1.6), snapshots ou versões desconhecidas.
pub fn mc_version_to_resource_pack_format(mc_version: &str) -> Option<u32> {
    let clean = mc_version.trim();
    let rest = clean.strip_prefix("1.")?;
    let mut parts = rest.split('.');
    let minor: u32 = parts.next()?.parse().ok()?;
    let patch: u32 = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);

    match (minor, patch) {
        (6..=8, _) => Some(1),
        (9..=10, _) => Some(2),
        (11..=12, _) => Some(3),
        (13..=14, _) => Some(4),
        (15, _) | (16, 0..=1) => Some(5),
        (16, _) => Some(6),
        (17, _) => Some(7),
        (18, _) => Some(8),
        (19, 0..=2) => Some(9),
        (19, 3) => Some(12),
        (19, _) => Some(13),
        (20, 0..=1) => Some(15),
        (20, 2) => Some(18),
        (20, 3..=4) => Some(22),
        (20, _) => Some(32),
        (21, 0..=1) => Some(34),
        (21, 2..=3) => Some(42),
        (21, 4) => Some(46),
        (21, 5) => Some(55),
        (21, 6) => Some(63),
        (21, 7..=8) => Some(64),
        (21, 9..=10) => Some(69),
        (21, 11) => Some(75),
        _ => None,
    }
}

/// Avalia a compatibilidade de um resource pack considerando `pack_format`, `supported_formats`
/// e o formato esperado pela versão do Minecraft da instância.
pub fn check_resource_pack_compatibility(
    pack_format: Option<u32>,
    supported_formats: Option<(u32, u32)>,
    mc_version: &str,
) -> PackCompatibility {
    let Some(expected) = mc_version_to_resource_pack_format(mc_version) else {
        return PackCompatibility::Unknown;
    };

    if let Some((min_fmt, max_fmt)) = supported_formats {
        let lo = min_fmt.min(max_fmt);
        let hi = min_fmt.max(max_fmt);
        if expected >= lo && expected <= hi {
            return PackCompatibility::Compatible;
        }
        let range_label = if lo == hi {
            format!("format {lo}")
        } else {
            format!("formats {lo}-{hi}")
        };
        return PackCompatibility::Incompatible {
            reason: format!(
                "Incompatible with Minecraft {mc_version} (requires pack_format {expected}, pack supports {range_label})"
            ),
        };
    }

    if let Some(pf) = pack_format {
        if pf == expected {
            return PackCompatibility::Compatible;
        }
        return PackCompatibility::Incompatible {
            reason: format!(
                "Incompatible with Minecraft {mc_version} (requires pack_format {expected}, pack declares format {pf})"
            ),
        };
    }

    PackCompatibility::Unknown
}

/// Verifica se a instância possui suporte conhecido a shaders (Iris ou Oculus em `.minecraft/mods`).
pub fn has_known_shader_mod(minecraft_dir: &Path) -> bool {
    let mods_dir = minecraft_dir.join("mods");
    let Ok(entries) = std::fs::read_dir(mods_dir) else {
        return false;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_lowercase();
        if name.ends_with(".jar") && (name.contains("iris") || name.contains("oculus")) {
            return true;
        }
    }
    false
}

/// Lista Texture/Resource Packs ou Shader Packs (ZIPs e diretórios válidos).
pub fn list_packs(dir: &Path) -> Vec<LocalPackInfo> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut packs = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let file_name = entry.file_name().to_string_lossy().to_string();
        if file_name.starts_with('.') {
            continue;
        }
        let is_dir = path.is_dir();
        let is_zip = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("zip"))
            .unwrap_or(false);
        if !is_dir && !is_zip {
            continue;
        }

        let display_name = if is_zip {
            path.file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| file_name.clone())
        } else {
            file_name.clone()
        };

        let meta = if is_zip {
            read_zip_pack_metadata(&path)
        } else {
            read_dir_pack_metadata(&path)
        };

        let fs_meta = std::fs::metadata(&path).ok();
        let size_bytes = if is_dir {
            dir_size(&path)
        } else {
            fs_meta.as_ref().map(|m| m.len()).unwrap_or(0)
        };
        let last_modified = fs_meta
            .and_then(|m| m.modified().ok())
            .map(|t| {
                let dt: DateTime<Local> = t.into();
                dt.format("%Y-%m-%d %H:%M").to_string()
            })
            .unwrap_or_default();

        let pack_format_display =
            format_pack_format_label(meta.pack_format, meta.supported_formats);

        packs.push(LocalPackInfo {
            file_name,
            display_name,
            description: meta.description,
            path,
            is_dir,
            size_bytes,
            last_modified,
            icon_png: meta.icon_png,
            pack_format: meta.pack_format,
            supported_formats: meta.supported_formats,
            pack_format_display,
        });
    }
    packs.sort_by(|a, b| {
        a.display_name
            .to_lowercase()
            .cmp(&b.display_name.to_lowercase())
    });
    packs
}

fn format_pack_format_label(
    pack_format: Option<u32>,
    supported_formats: Option<(u32, u32)>,
) -> String {
    if let Some((min_f, max_f)) = supported_formats {
        let lo = min_f.min(max_f);
        let hi = min_f.max(max_f);
        if lo == hi {
            return format!("Format {lo}");
        }
        return format!("Format {lo}-{hi}");
    }
    if let Some(pf) = pack_format {
        return format!("Format {pf}");
    }
    String::new()
}

/// Importa um arquivo ZIP ou diretório para `dest_dir`, recusando sobrescritas silenciosas.
pub fn import_pack(source: &Path, dest_dir: &Path) -> Result<PathBuf> {
    let file_name = source
        .file_name()
        .ok_or_else(|| LauncherError::InvalidMetadata("Invalid pack path".into()))?;
    std::fs::create_dir_all(dest_dir).map_err(|e| LauncherError::Io {
        path: dest_dir.to_path_buf(),
        source: e,
    })?;
    let target = dest_dir.join(file_name);
    if target.exists() {
        return Err(LauncherError::InvalidMetadata(format!(
            "A pack named '{}' already exists.",
            file_name.to_string_lossy()
        )));
    }

    if source.is_dir() {
        copy_dir_recursive(source, &target)?;
    } else {
        // Valida que é um ZIP legível antes de copiar
        let file = std::fs::File::open(source).map_err(|e| LauncherError::Io {
            path: source.to_path_buf(),
            source: e,
        })?;
        let _ = ZipArchive::new(file)
            .map_err(|e| LauncherError::InvalidMetadata(format!("Invalid ZIP archive: {e}")))?;
        std::fs::copy(source, &target).map_err(|e| LauncherError::Io {
            path: target.clone(),
            source: e,
        })?;
    }
    Ok(target)
}

/// Remove um pack localizado dentro de `dest_dir`.
pub fn remove_pack(path: &Path, dest_dir: &Path) -> Result<()> {
    if !path.starts_with(dest_dir) {
        return Err(LauncherError::InvalidMetadata(
            "Refusing to delete path outside managed pack directory".into(),
        ));
    }
    if path.is_dir() {
        std::fs::remove_dir_all(path).map_err(|e| LauncherError::Io {
            path: path.to_path_buf(),
            source: e,
        })
    } else {
        std::fs::remove_file(path).map_err(|e| LauncherError::Io {
            path: path.to_path_buf(),
            source: e,
        })
    }
}

#[derive(Default)]
struct RawPackMetadata {
    description: String,
    icon_png: Option<Vec<u8>>,
    pack_format: Option<u32>,
    supported_formats: Option<(u32, u32)>,
}

fn read_zip_pack_metadata(path: &Path) -> RawPackMetadata {
    let Ok(file) = std::fs::File::open(path) else {
        return RawPackMetadata::default();
    };
    let Ok(mut archive) = ZipArchive::new(file) else {
        return RawPackMetadata::default();
    };
    let mut meta = archive
        .by_name("pack.mcmeta")
        .ok()
        .and_then(|mut entry| {
            let mut s = String::new();
            entry.read_to_string(&mut s).ok()?;
            parse_mcmeta(&s)
        })
        .unwrap_or_default();
    meta.icon_png = archive.by_name("pack.png").ok().and_then(|mut entry| {
        let mut buf = Vec::new();
        entry.read_to_end(&mut buf).ok()?;
        Some(buf)
    });
    meta
}

fn read_dir_pack_metadata(path: &Path) -> RawPackMetadata {
    let mut meta = std::fs::read_to_string(path.join("pack.mcmeta"))
        .ok()
        .and_then(|s| parse_mcmeta(&s))
        .unwrap_or_default();
    meta.icon_png = std::fs::read(path.join("pack.png")).ok();
    meta
}

fn parse_mcmeta(raw: &str) -> Option<RawPackMetadata> {
    let cleaned = raw.trim_start_matches('\u{feff}');
    let v: serde_json::Value = serde_json::from_str(cleaned).ok()?;
    let pack = v.get("pack")?;

    let description = pack
        .get("description")
        .and_then(extract_mc_text)
        .map(|s| strip_mc_formatting(&s))
        .unwrap_or_default();

    let pack_format = pack
        .get("pack_format")
        .and_then(|f| f.as_u64())
        .map(|n| n as u32)
        .or_else(|| pack.get("min_format").and_then(extract_format_major));

    let supported_formats = pack
        .get("supported_formats")
        .and_then(parse_supported_formats)
        .or_else(|| {
            let min_f = pack.get("min_format").and_then(extract_format_major)?;
            let max_f = pack
                .get("max_format")
                .and_then(extract_format_major)
                .unwrap_or(min_f);
            Some((min_f, max_f))
        });

    Some(RawPackMetadata {
        description,
        icon_png: None,
        pack_format,
        supported_formats,
    })
}

fn extract_format_major(val: &serde_json::Value) -> Option<u32> {
    if let Some(n) = val.as_u64() {
        return Some(n as u32);
    }
    if let Some(arr) = val.as_array() {
        return arr.first().and_then(|v| v.as_u64()).map(|n| n as u32);
    }
    None
}

fn parse_supported_formats(val: &serde_json::Value) -> Option<(u32, u32)> {
    if let Some(n) = val.as_u64() {
        let f = n as u32;
        return Some((f, f));
    }
    if let Some(arr) = val.as_array() {
        if arr.len() >= 2 {
            let lo = arr[0].as_u64()? as u32;
            let hi = arr[1].as_u64()? as u32;
            return Some((lo, hi));
        } else if arr.len() == 1 {
            let f = arr[0].as_u64()? as u32;
            return Some((f, f));
        }
    }
    if let Some(obj) = val.as_object() {
        let lo = obj.get("min_inclusive")?.as_u64()? as u32;
        let hi = obj.get("max_inclusive")?.as_u64()? as u32;
        return Some((lo, hi));
    }
    None
}

fn extract_mc_text(val: &serde_json::Value) -> Option<String> {
    if let Some(s) = val.as_str() {
        return Some(s.trim().to_string());
    }
    if let Some(obj) = val.as_object() {
        let mut out = obj
            .get("text")
            .and_then(|t| t.as_str())
            .unwrap_or_default()
            .to_string();
        if let Some(extra) = obj.get("extra").and_then(|e| e.as_array()) {
            for item in extra {
                if let Some(part) = extract_mc_text(item) {
                    out.push_str(&part);
                }
            }
        }
        let trimmed = out.trim().to_string();
        if !trimmed.is_empty() {
            return Some(trimmed);
        }
    }
    if let Some(arr) = val.as_array() {
        let mut out = String::new();
        for item in arr {
            if let Some(part) = extract_mc_text(item) {
                out.push_str(&part);
            }
        }
        let trimmed = out.trim().to_string();
        if !trimmed.is_empty() {
            return Some(trimmed);
        }
    }
    None
}

/// Remove códigos de formatação do Minecraft (`§a`, `§l`, etc.) para exibição limpa na interface.
fn strip_mc_formatting(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '§' {
            let _ = chars.next();
            continue;
        }
        out.push(c);
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn dir_size(path: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };
    entries
        .flatten()
        .map(|e| {
            let p = e.path();
            if p.is_dir() {
                dir_size(&p)
            } else {
                e.metadata().map(|m| m.len()).unwrap_or(0)
            }
        })
        .sum()
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst).map_err(|e| LauncherError::Io {
        path: dst.to_path_buf(),
        source: e,
    })?;
    for entry in std::fs::read_dir(src)
        .map_err(|e| LauncherError::Io {
            path: src.to_path_buf(),
            source: e,
        })?
        .flatten()
    {
        let p = entry.path();
        let target = dst.join(entry.file_name());
        if p.is_dir() {
            copy_dir_recursive(&p, &target)?;
        } else {
            std::fs::copy(&p, &target).map_err(|e| LauncherError::Io {
                path: target,
                source: e,
            })?;
        }
    }
    Ok(())
}
