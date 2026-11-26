use crate::core::presets::model::{JvmPreset, JVM_PRESET_SCHEMA_VERSION};
use crate::error::{LauncherError, Result};
use std::fs;
use std::path::{Path, PathBuf};
use tracing::warn;

const OFFICIAL_DEFAULT_TOML: &str = include_str!("../../../assets/presets/jvm/default.toml");
const OFFICIAL_G1GC_TOML: &str = include_str!("../../../assets/presets/jvm/g1gc.toml");
const OFFICIAL_ZGC_TOML: &str = include_str!("../../../assets/presets/jvm/zgc.toml");
const OFFICIAL_PARALLEL_TOML: &str = include_str!("../../../assets/presets/jvm/parallel-gc.toml");

/// Carrega os 4 presets oficiais embutidos do Crab (somente leitura).
pub fn load_official_presets() -> Vec<JvmPreset> {
    let raw_list = [
        OFFICIAL_DEFAULT_TOML,
        OFFICIAL_G1GC_TOML,
        OFFICIAL_ZGC_TOML,
        OFFICIAL_PARALLEL_TOML,
    ];
    let mut presets = Vec::new();
    for raw in raw_list {
        if let Ok(mut p) = toml::from_str::<JvmPreset>(raw) {
            p.is_official = true;
            presets.push(p);
        }
    }
    presets
}

/// Carrega todos os presets disponíveis: oficiais primeiro, depois personalizados válidos
/// em `<data_dir>/presets/jvm/custom-presets/`.
pub fn discover_jvm_presets(custom_dir: &Path) -> Vec<JvmPreset> {
    let mut all = load_official_presets();
    let official_ids: Vec<String> = all.iter().map(|p| p.id.clone()).collect();

    if !custom_dir.is_dir() {
        return all;
    }

    let mut custom_list = Vec::new();
    if let Ok(entries) = fs::read_dir(custom_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("toml") {
                continue;
            }
            match fs::read_to_string(&path) {
                Ok(content) => match toml::from_str::<JvmPreset>(&content) {
                    Ok(mut preset) => {
                        preset.is_official = false;
                        if official_ids.contains(&preset.id) || preset.id == "custom" {
                            warn!(
                                "Skipping custom JVM preset with reserved id '{}' at {}",
                                preset.id,
                                path.display()
                            );
                            continue;
                        }
                        if let Err(err) = preset.validate() {
                            warn!(
                                "Skipping invalid custom JVM preset at {}: {err}",
                                path.display()
                            );
                            continue;
                        }
                        custom_list.push(preset);
                    }
                    Err(err) => {
                        warn!(
                            "Failed to parse custom JVM preset {}: {err}",
                            path.display()
                        );
                    }
                },
                Err(err) => {
                    warn!("Failed to read custom JVM preset {}: {err}", path.display());
                }
            }
        }
    }

    custom_list.sort_by_key(|a| a.name.to_lowercase());
    all.extend(custom_list);
    all
}

/// Gera um identificador seguro de arquivo a partir do nome do preset.
pub fn slugify_preset_id(name: &str) -> String {
    let mut out = String::new();
    for ch in name.trim().to_ascii_lowercase().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
        } else if (ch == ' ' || ch == '-' || ch == '_') && !out.ends_with('-') {
            out.push('-');
        }
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.is_empty()
        || matches!(
            trimmed.as_str(),
            "default" | "g1gc" | "zgc" | "parallel-gc" | "custom"
        )
    {
        format!("custom-{}", uuid::Uuid::new_v4().simple())
    } else {
        trimmed
    }
}

/// Salva um preset personalizado em `<custom_dir>/<id>.toml` de forma atômica.
pub fn save_custom_preset(custom_dir: &Path, preset: &JvmPreset) -> Result<PathBuf> {
    if preset.is_official {
        return Err(LauncherError::Launch(
            "Official JVM presets are read-only. Duplicate it to create a custom variant."
                .to_string(),
        ));
    }
    preset.validate()?;
    fs::create_dir_all(custom_dir).map_err(|source| LauncherError::Io {
        path: custom_dir.to_path_buf(),
        source,
    })?;

    let target = custom_dir.join(format!("{}.toml", preset.id));
    let tmp = custom_dir.join(format!("{}.toml.tmp", preset.id));
    let content = toml::to_string_pretty(preset)
        .map_err(|e| LauncherError::Launch(format!("Failed to serialize preset: {e}")))?;

    fs::write(&tmp, content).map_err(|source| LauncherError::Io {
        path: tmp.clone(),
        source,
    })?;
    fs::rename(&tmp, &target).map_err(|source| LauncherError::Io {
        path: target.clone(),
        source,
    })?;
    Ok(target)
}

/// Cria um novo preset personalizado a partir de nome, descrição, argumentos e Java mínimo.
pub fn create_custom_preset(
    custom_dir: &Path,
    name: &str,
    description: &str,
    raw_args: &str,
    min_java_major: u32,
) -> Result<JvmPreset> {
    let mut id = slugify_preset_id(name);
    if custom_dir.join(format!("{id}.toml")).exists() {
        id = format!("{id}-{}", &uuid::Uuid::new_v4().simple().to_string()[..6]);
    }
    let args: Vec<String> = raw_args.split_whitespace().map(|s| s.to_string()).collect();
    let preset = JvmPreset {
        schema_version: JVM_PRESET_SCHEMA_VERSION,
        id,
        name: name.trim().to_string(),
        description: description.trim().to_string(),
        args,
        min_java_major,
        max_java_major: None,
        supported_arches: Vec::new(),
        source: "Custom".to_string(),
        version: "1.0.0".to_string(),
        is_official: false,
    };
    save_custom_preset(custom_dir, &preset)?;
    Ok(preset)
}

/// Duplica qualquer preset (inclusive oficial) como um novo preset personalizado editável.
pub fn duplicate_preset(custom_dir: &Path, source: &JvmPreset) -> Result<JvmPreset> {
    let copy_name = format!("{} (Copy)", source.name);
    let mut id = slugify_preset_id(&copy_name);
    if custom_dir.join(format!("{id}.toml")).exists() {
        id = format!("{id}-{}", &uuid::Uuid::new_v4().simple().to_string()[..6]);
    }
    let mut dup = source.clone();
    dup.id = id;
    dup.name = copy_name;
    dup.source = "Custom".to_string();
    dup.is_official = false;
    save_custom_preset(custom_dir, &dup)?;
    Ok(dup)
}

/// Remove um preset personalizado (bloqueia remoção de presets oficiais).
pub fn delete_custom_preset(custom_dir: &Path, preset_id: &str) -> Result<()> {
    if matches!(
        preset_id,
        "default" | "g1gc" | "zgc" | "parallel-gc" | "custom"
    ) {
        return Err(LauncherError::Launch(
            "Official JVM presets cannot be deleted.".to_string(),
        ));
    }
    let target = custom_dir.join(format!("{preset_id}.toml"));
    if target.exists() {
        fs::remove_file(&target).map_err(|source| LauncherError::Io {
            path: target,
            source,
        })?;
    }
    Ok(())
}

/// Importa e valida um arquivo `.toml` externo de preset JVM para o diretório gerenciado.
pub fn import_custom_preset(custom_dir: &Path, source_file: &Path) -> Result<JvmPreset> {
    let content = fs::read_to_string(source_file).map_err(|source| LauncherError::Io {
        path: source_file.to_path_buf(),
        source,
    })?;
    let mut preset: JvmPreset = toml::from_str(&content)
        .map_err(|e| LauncherError::Launch(format!("Invalid JVM preset TOML file: {e}")))?;
    preset.is_official = false;
    preset.source = "Custom".to_string();
    if matches!(
        preset.id.as_str(),
        "default" | "g1gc" | "zgc" | "parallel-gc" | "custom"
    ) || custom_dir.join(format!("{}.toml", preset.id)).exists()
    {
        preset.id = format!(
            "{}-{}",
            slugify_preset_id(&preset.name),
            &uuid::Uuid::new_v4().simple().to_string()[..6]
        );
    }
    save_custom_preset(custom_dir, &preset)?;
    Ok(preset)
}

/// Exporta um preset para um arquivo `.toml` escolhido pelo usuário.
pub fn export_preset(preset: &JvmPreset, dest_file: &Path) -> Result<()> {
    let content = toml::to_string_pretty(preset)
        .map_err(|e| LauncherError::Launch(format!("Failed to serialize preset: {e}")))?;
    fs::write(dest_file, content).map_err(|source| LauncherError::Io {
        path: dest_file.to_path_buf(),
        source,
    })?;
    Ok(())
}
