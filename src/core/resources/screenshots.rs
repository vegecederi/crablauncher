use crate::error::{LauncherError, Result};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct ScreenshotInfo {
    pub file_name: String,
    pub display_name: String,
    pub path: PathBuf,
    pub size_bytes: u64,
}

pub fn screenshots_dir(minecraft_dir: &Path) -> PathBuf {
    minecraft_dir.join("screenshots")
}

/// Lista capturas de tela em `.minecraft/screenshots/` ordenadas da mais recente para a mais antiga.
pub fn list_screenshots(minecraft_dir: &Path) -> Vec<ScreenshotInfo> {
    let dir = screenshots_dir(minecraft_dir);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut items: Vec<(std::time::SystemTime, ScreenshotInfo)> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .unwrap_or_default();
        if !matches!(ext.as_str(), "png" | "jpg" | "jpeg") {
            continue;
        }
        let file_name = entry.file_name().to_string_lossy().to_string();
        let display_name = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| file_name.clone());
        let meta = entry.metadata().ok();
        let size_bytes = meta.as_ref().map(|m| m.len()).unwrap_or(0);
        let mtime = meta
            .and_then(|m| m.modified().ok())
            .unwrap_or(std::time::UNIX_EPOCH);
        items.push((
            mtime,
            ScreenshotInfo {
                file_name,
                display_name,
                path,
                size_bytes,
            },
        ));
    }
    items.sort_by_key(|(t, _)| std::cmp::Reverse(*t));
    items.into_iter().map(|(_, info)| info).collect()
}

/// Renomeia uma captura de tela preservando sua extensão original.
pub fn rename_screenshot(
    minecraft_dir: &Path,
    old_file_name: &str,
    new_stem: &str,
) -> Result<PathBuf> {
    let trimmed = new_stem.trim();
    if trimmed.is_empty()
        || trimmed.contains('/')
        || trimmed.contains('\\')
        || trimmed.contains("..")
    {
        return Err(LauncherError::InvalidMetadata(
            "Invalid screenshot name.".into(),
        ));
    }
    let dir = screenshots_dir(minecraft_dir);
    let old_path = dir.join(old_file_name);
    if !old_path.is_file() {
        return Err(LauncherError::InvalidMetadata(
            "Screenshot file not found.".into(),
        ));
    }
    let ext = old_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("png");
    let new_path = dir.join(format!("{trimmed}.{ext}"));
    if new_path.exists() {
        return Err(LauncherError::InvalidMetadata(
            "A screenshot with that name already exists.".into(),
        ));
    }
    std::fs::rename(&old_path, &new_path).map_err(|source| LauncherError::Io {
        path: new_path.clone(),
        source,
    })?;
    Ok(new_path)
}

/// Exclui uma captura de tela dentro de `.minecraft/screenshots/`.
pub fn delete_screenshot(minecraft_dir: &Path, file_name: &str) -> Result<()> {
    let dir = screenshots_dir(minecraft_dir);
    let path = dir.join(file_name);
    if !path.starts_with(&dir) {
        return Err(LauncherError::InvalidMetadata(
            "Refusing to delete path outside screenshots directory.".into(),
        ));
    }
    std::fs::remove_file(&path).map_err(|source| LauncherError::Io { path, source })
}
