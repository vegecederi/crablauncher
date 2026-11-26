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
}

/// Inspeciona um arquivo `.jar` ou `.jar.disabled` lendo apenas `fabric.mod.json`,
/// `META-INF/neoforge.mods.toml` ou `META-INF/mods.toml`.
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
    if let Ok(mut entry) = archive.by_name("fabric.mod.json") {
        let mut content = String::new();
        if entry.read_to_string(&mut content).is_ok() {
            if let Ok(fm) = serde_json::from_str::<FabricModJson>(&content) {
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
                fallback.loader = Some(ModloaderKind::Fabric);
                fallback.warning = check_loader_compat(fallback.loader, expected_loader);
                return fallback;
            }
        }
    }

    // 2. NeoForge (`META-INF/neoforge.mods.toml`)
    if let Ok(mut entry) = archive.by_name("META-INF/neoforge.mods.toml") {
        let mut content = String::new();
        if entry.read_to_string(&mut content).is_ok() {
            parse_mods_toml_fields(&content, &mut fallback);
            fallback.loader = Some(ModloaderKind::NeoForge);
            fallback.warning = check_loader_compat(fallback.loader, expected_loader);
            return fallback;
        }
    }

    // 3. Forge (`META-INF/mods.toml`)
    if let Ok(mut entry) = archive.by_name("META-INF/mods.toml") {
        let mut content = String::new();
        if entry.read_to_string(&mut content).is_ok() {
            parse_mods_toml_fields(&content, &mut fallback);
            fallback.loader = Some(ModloaderKind::Forge);
            fallback.warning = check_loader_compat(fallback.loader, expected_loader);
            return fallback;
        }
    }

    fallback.warning = Some("Unknown mod metadata".to_string());
    fallback
}

fn parse_mods_toml_fields(toml_text: &str, target: &mut LocalMod) {
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
        }
    }
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
