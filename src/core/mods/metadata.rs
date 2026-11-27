use crate::core::components::model::ModloaderKind;
use serde::Deserialize;
use std::io::Read;
use std::path::Path;

/// Metadados extraídos de forma segura (sem executar código) de um arquivo JAR de mod.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalMod {
    pub file_name: String,
    pub enabled: bool,
    pub mod_id: String,
    pub name: String,
    pub version: String,
    pub author: String,
    pub description: String,
    pub icon_bytes: Option<Vec<u8>>,
    pub loader: Option<ModloaderKind>,
    pub warning: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum FabricAuthorEntry {
    Name(String),
    Obj { name: String },
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum FabricIconEntry {
    Path(String),
    Sized(std::collections::HashMap<String, String>),
}

impl FabricIconEntry {
    fn best_path(&self) -> Option<&str> {
        match self {
            Self::Path(path) => Some(path.as_str()),
            Self::Sized(map) => map
                .iter()
                .max_by_key(|(size, _)| size.parse::<u32>().unwrap_or(0))
                .map(|(_, path)| path.as_str()),
        }
    }
}

#[derive(Debug, Deserialize)]
struct FabricModJson {
    id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    authors: Vec<FabricAuthorEntry>,
    #[serde(default)]
    icon: Option<FabricIconEntry>,
}

/// Inspeciona um arquivo `.jar` ou `.jar.disabled` lendo apenas `fabric.mod.json`,
/// `quilt.mod.json`, `META-INF/neoforge.mods.toml` ou `META-INF/mods.toml`.
pub fn inspect_mod_jar(path: &Path, expected_loader: Option<ModloaderKind>) -> LocalMod {
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown.jar")
        .to_string();
    let enabled = !file_name.ends_with(".disabled");

    let mut fallback = LocalMod {
        file_name: file_name.clone(),
        enabled,
        mod_id: file_name.clone(),
        name: file_name
            .trim_end_matches(".disabled")
            .trim_end_matches(".jar")
            .to_string(),
        version: "Unknown".to_string(),
        author: String::new(),
        description: String::new(),
        icon_bytes: None,
        loader: None,
        warning: None,
    };

    let Ok(file) = std::fs::File::open(path) else {
        fallback.warning = Some("Unreadable file".to_string());
        return fallback;
    };
    let Ok(mut archive) = zip::ZipArchive::new(file) else {
        fallback.warning = Some("Invalid JAR archive".to_string());
        return fallback;
    };

    // 1. Fabric (`fabric.mod.json`)
    if let Some(content) = read_zip_text(&mut archive, "fabric.mod.json") {
        if let Some(fm) = parse_fabric_mod_json(&content) {
            let icon_path = fm
                .icon
                .as_ref()
                .and_then(|icon| icon.best_path())
                .map(str::to_owned);
            fallback.mod_id = fm.id;
            fallback.name = fm.name.unwrap_or_else(|| fallback.mod_id.clone());
            fallback.version = fm.version.unwrap_or_else(|| "Unknown".to_string());
            fallback.description = fm.description.unwrap_or_default().trim().to_string();
            let author_names: Vec<String> = fm
                .authors
                .into_iter()
                .map(|a| match a {
                    FabricAuthorEntry::Name(s) => s,
                    FabricAuthorEntry::Obj { name } => name,
                })
                .filter(|s| !s.trim().is_empty())
                .collect();
            fallback.author = author_names.join(", ");
            if let Some(icon_path) = icon_path {
                fallback.icon_bytes = read_zip_icon(&mut archive, &icon_path);
            }
            fallback.loader = Some(ModloaderKind::Fabric);
            fallback.warning = check_loader_compat(fallback.loader, expected_loader);
            return fallback;
        }
    }

    // 2. Quilt (`quilt.mod.json`)
    if let Some(content) = read_zip_text(&mut archive, "quilt.mod.json") {
        if let Some((mod_id, name, version, description, author, icon_path)) =
            parse_quilt_mod_json(&content)
        {
            fallback.mod_id = mod_id;
            fallback.name = name;
            fallback.version = version;
            fallback.description = description;
            fallback.author = author;
            if let Some(icon_path) = icon_path {
                fallback.icon_bytes = read_zip_icon(&mut archive, &icon_path);
            }
            fallback.loader = Some(ModloaderKind::Quilt);
            fallback.warning = check_loader_compat(fallback.loader, expected_loader);
            return fallback;
        }
    }

    // 3. NeoForge (`META-INF/neoforge.mods.toml`)
    if let Some(content) = read_zip_text(&mut archive, "META-INF/neoforge.mods.toml") {
        let logo_path = parse_mods_toml_fields(&content, &mut fallback);
        if let Some(logo_path) = logo_path {
            fallback.icon_bytes = read_zip_icon(&mut archive, &logo_path);
        }
        fallback.loader = Some(ModloaderKind::NeoForge);
        fallback.warning = check_loader_compat(fallback.loader, expected_loader);
        return fallback;
    }

    // 4. Forge (`META-INF/mods.toml`)
    if let Some(content) = read_zip_text(&mut archive, "META-INF/mods.toml") {
        let logo_path = parse_mods_toml_fields(&content, &mut fallback);
        if let Some(logo_path) = logo_path {
            fallback.icon_bytes = read_zip_icon(&mut archive, &logo_path);
        }
        fallback.loader = Some(ModloaderKind::Forge);
        fallback.warning = check_loader_compat(fallback.loader, expected_loader);
        return fallback;
    }

    fallback.warning = Some("Unknown mod metadata".to_string());
    fallback
}

fn read_zip_text(archive: &mut zip::ZipArchive<std::fs::File>, name: &str) -> Option<String> {
    let entry = archive.by_name(name).ok()?;
    let mut content = String::new();
    entry.take(1024 * 1024).read_to_string(&mut content).ok()?;
    Some(content)
}

fn read_zip_icon(archive: &mut zip::ZipArchive<std::fs::File>, raw_path: &str) -> Option<Vec<u8>> {
    const MAX_MOD_ICON_BYTES: u64 = 2 * 1024 * 1024;
    let normalized = raw_path
        .trim()
        .trim_start_matches("./")
        .trim_start_matches('/');
    if normalized.is_empty() || normalized.contains("..") {
        return None;
    }
    let entry = archive.by_name(normalized).ok()?;
    if entry.size() > MAX_MOD_ICON_BYTES {
        return None;
    }
    let mut bytes = Vec::new();
    entry
        .take(MAX_MOD_ICON_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > MAX_MOD_ICON_BYTES {
        return None;
    }
    Some(bytes)
}

// Alguns JARs reais trazem quebras de linha literais dentro de strings JSON ou BOM UTF-8.
fn parse_fabric_mod_json(raw: &str) -> Option<FabricModJson> {
    let cleaned = raw.trim_start_matches('\u{feff}');
    if let Ok(parsed) = serde_json::from_str::<FabricModJson>(cleaned) {
        return Some(parsed);
    }
    let escaped = escape_literal_newlines_in_json_strings(cleaned);
    serde_json::from_str::<FabricModJson>(&escaped).ok()
}

fn escape_literal_newlines_in_json_strings(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut in_string = false;
    let mut escaped = false;
    for ch in input.chars() {
        if in_string {
            if escaped {
                out.push(ch);
                escaped = false;
            } else if ch == '\\' {
                out.push(ch);
                escaped = true;
            } else if ch == '"' {
                out.push(ch);
                in_string = false;
            } else if ch == '\n' {
                out.push_str("\\n");
            } else if ch == '\r' {
                out.push_str("\\r");
            } else if ch == '\t' {
                out.push_str("\\t");
            } else {
                out.push(ch);
            }
        } else {
            if ch == '"' {
                in_string = true;
            }
            out.push(ch);
        }
    }
    out
}

fn parse_quilt_mod_json(
    raw: &str,
) -> Option<(String, String, String, String, String, Option<String>)> {
    let cleaned = escape_literal_newlines_in_json_strings(raw.trim_start_matches('\u{feff}'));
    let val: serde_json::Value = serde_json::from_str(&cleaned).ok()?;
    let loader = val.get("quilt_loader")?;
    let id = loader.get("id")?.as_str()?.to_string();
    let version = loader
        .get("version")
        .and_then(|v| v.as_str())
        .unwrap_or("Unknown")
        .to_string();
    let meta = loader.get("metadata");
    let name = meta
        .and_then(|m| m.get("name"))
        .and_then(|v| v.as_str())
        .unwrap_or(&id)
        .to_string();
    let description = meta
        .and_then(|m| m.get("description"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let author = meta
        .and_then(|m| m.get("contributors"))
        .and_then(|v| v.as_object())
        .map(|obj| obj.keys().cloned().collect::<Vec<_>>().join(", "))
        .unwrap_or_default();
    let icon_path = meta.and_then(|m| m.get("icon")).and_then(|icon| {
        if let Some(s) = icon.as_str() {
            Some(s.to_string())
        } else {
            icon.as_object()?
                .iter()
                .max_by_key(|(k, _)| k.parse::<u32>().unwrap_or(0))
                .and_then(|(_, v)| v.as_str().map(str::to_owned))
        }
    });
    Some((id, name, version, description, author, icon_path))
}

fn parse_mods_toml_fields(toml_text: &str, target: &mut LocalMod) -> Option<String> {
    let mut logo_file = None;
    for line in toml_text.lines() {
        let trimmed = line.trim();
        if let Some(val) = extract_toml_str(trimmed, "modId") {
            target.mod_id = val;
        } else if let Some(val) = extract_toml_str(trimmed, "displayName") {
            target.name = val;
        } else if let Some(val) = extract_toml_str(trimmed, "version") {
            if !val.contains("${") {
                target.version = val;
            }
        } else if let Some(val) = extract_toml_str(trimmed, "authors") {
            target.author = val;
        } else if let Some(val) = extract_toml_str(trimmed, "description") {
            target.description = val;
        } else if let Some(val) = extract_toml_str(trimmed, "logoFile") {
            if !val.trim().is_empty() {
                logo_file = Some(val);
            }
        }
    }
    logo_file
}

fn extract_toml_str(line: &str, key: &str) -> Option<String> {
    let rest = line.strip_prefix(key)?.trim_start();
    let rest = rest.strip_prefix('=')?.trim();
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn check_loader_compat(
    mod_loader: Option<ModloaderKind>,
    instance_loader: Option<ModloaderKind>,
) -> Option<String> {
    match (mod_loader, instance_loader) {
        (Some(m), Some(i)) if m != i => Some(format!(
            "Built for {}, instance uses {}",
            m.short_name(),
            i.short_name()
        )),
        (Some(m), None) => Some(format!("Requires {} (instance is Vanilla)", m.short_name())),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::{write::SimpleFileOptions, ZipWriter};

    #[test]
    fn fabric_mod_json_reads_icon_and_handles_literal_newlines() {
        let tmp = tempfile::tempdir().unwrap();
        let jar_path = tmp.path().join("bettergrassify.jar");
        let mut zip = ZipWriter::new(std::fs::File::create(&jar_path).unwrap());
        zip.start_file("fabric.mod.json", SimpleFileOptions::default())
            .unwrap();
        zip.write_all(
            b"{\n  \"id\": \"bettergrassify\",\n  \"name\": \"BetterGrassify\",\n  \"version\": \"1.8.8\",\n  \"description\": \"Line 1\nLine 2\",\n  \"icon\": \"assets/bettergrassify/icon.png\",\n  \"authors\": [\"AuthorOne\"]\n}",
        )
        .unwrap();
        zip.start_file(
            "assets/bettergrassify/icon.png",
            SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(b"PNGDATA").unwrap();
        zip.finish().unwrap();

        let info = inspect_mod_jar(&jar_path, Some(ModloaderKind::Fabric));
        assert_eq!(info.name, "BetterGrassify");
        assert_eq!(info.version, "1.8.8");
        assert_eq!(info.author, "AuthorOne");
        assert_eq!(info.icon_bytes.as_deref(), Some(b"PNGDATA".as_slice()));
        assert!(info.warning.is_none());
    }
}
