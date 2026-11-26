use crate::core::components::model::ModloaderKind;
use crate::core::mods::metadata::{inspect_mod_jar, LocalMod};
use crate::error::{LauncherError, Result};
use std::path::Path;
use tokio::fs;

/// Lista todos os mods `.jar` e `.jar.disabled` no diretório `mods/` da instância.
pub fn list_local_mods(mods_dir: &Path, expected_loader: Option<ModloaderKind>) -> Vec<LocalMod> {
    if !mods_dir.is_dir() {
        return Vec::new();
    }
    let Ok(entries) = std::fs::read_dir(mods_dir) else {
        return Vec::new();
    };

    let mut mods = Vec::new();
    for entry in entries.flatten() {
        let p = entry.path();
        if !p.is_file() {
            continue;
        }
        let Some(name) = p.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name.ends_with(".jar") || name.ends_with(".jar.disabled") {
            mods.push(inspect_mod_jar(&p, expected_loader));
        }
    }

    mods.sort_by_key(|a| a.name.to_lowercase());
    mods
}

/// Alterna um mod entre habilitado (`.jar`) e desabilitado (`.jar.disabled`) de forma reversível.
pub async fn toggle_local_mod(mods_dir: &Path, file_name: &str) -> Result<String> {
    validate_file_name(file_name)?;
    let src = mods_dir.join(file_name);
    if !src.is_file() {
        return Err(LauncherError::UnsafePath(format!(
            "Mod file not found: {file_name}"
        )));
    }

    let new_name = if let Some(stripped) = file_name.strip_suffix(".disabled") {
        stripped.to_string()
    } else {
        format!("{file_name}.disabled")
    };

    let dst = mods_dir.join(&new_name);
    fs::rename(&src, &dst)
        .await
        .map_err(|source| LauncherError::Io { path: dst, source })?;
    Ok(new_name)
}

/// Remove um arquivo de mod do diretório `mods/` da instância.
pub async fn remove_local_mod(mods_dir: &Path, file_name: &str) -> Result<()> {
    validate_file_name(file_name)?;
    let target = mods_dir.join(file_name);
    if target.is_file() {
        fs::remove_file(&target)
            .await
            .map_err(|source| LauncherError::Io {
                path: target,
                source,
            })?;
    }
    Ok(())
}

/// Copia um arquivo `.jar` externo para `mods/` sem sobrescrever arquivos existentes.
pub async fn import_local_mod(mods_dir: &Path, source_jar: &Path) -> Result<String> {
    let raw_name = source_jar
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| LauncherError::UnsafePath("Invalid mod filename.".to_string()))?;
    if !raw_name.ends_with(".jar") {
        return Err(LauncherError::UnsafePath(
            "Only .jar mod files can be imported.".to_string(),
        ));
    }

    fs::create_dir_all(mods_dir)
        .await
        .map_err(|source| LauncherError::Io {
            path: mods_dir.to_path_buf(),
            source,
        })?;

    let dest = mods_dir.join(raw_name);
    if dest.exists() {
        return Err(LauncherError::UnsafePath(format!(
            "Mod '{raw_name}' already exists in this instance."
        )));
    }

    fs::copy(source_jar, &dest)
        .await
        .map_err(|source| LauncherError::Io { path: dest, source })?;
    Ok(raw_name.to_string())
}

fn validate_file_name(name: &str) -> Result<()> {
    if name.contains('/') || name.contains('\\') || name.contains("..") {
        return Err(LauncherError::UnsafePath(format!(
            "Unsafe mod filename: '{name}'"
        )));
    }
    Ok(())
}
