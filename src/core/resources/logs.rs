use crate::error::{LauncherError, Result};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct SessionLogInfo {
    pub file_name: String,
    pub display_name: String,
    pub timestamp: String,
    pub path: PathBuf,
    pub size_bytes: u64,
}

#[derive(Debug, Clone)]
pub struct ParsedLogLine {
    pub text: String,
    pub level: &'static str, // "ERROR" | "WARN" | "INFO"
}

pub fn logs_dir(minecraft_dir: &Path) -> PathBuf {
    minecraft_dir.join("logs")
}

/// Lista arquivos de log de execução em `.minecraft/logs/` ordenados do mais recente ao mais antigo.
pub fn list_session_logs(minecraft_dir: &Path) -> Vec<SessionLogInfo> {
    let dir = logs_dir(minecraft_dir);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut items: Vec<(std::time::SystemTime, SessionLogInfo)> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let file_name = entry.file_name().to_string_lossy().to_string();
        if !file_name.ends_with(".log") && !file_name.ends_with(".txt") {
            continue;
        }
        let meta = entry.metadata().ok();
        let size_bytes = meta.as_ref().map(|m| m.len()).unwrap_or(0);
        let mtime = meta
            .and_then(|m| m.modified().ok())
            .unwrap_or(std::time::UNIX_EPOCH);
        let dt: chrono::DateTime<chrono::Local> = mtime.into();
        let timestamp = dt.format("%Y-%m-%d %H:%M:%S").to_string();
        items.push((
            mtime,
            SessionLogInfo {
                display_name: file_name.clone(),
                file_name,
                timestamp,
                path,
                size_bytes,
            },
        ));
    }
    items.sort_by_key(|(t, _)| std::cmp::Reverse(*t));
    items.into_iter().map(|(_, info)| info).collect()
}

/// Lê e classifica linhas de um arquivo de log (`ERROR`, `WARN`, `INFO`), filtrando por `query`.
pub fn read_log_lines(path: &Path, query: &str) -> Vec<ParsedLogLine> {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let q = query.trim().to_lowercase();
    raw.lines()
        .filter(|l| q.is_empty() || l.to_lowercase().contains(&q))
        .take(2000)
        .map(|l| {
            let upper = l.to_ascii_uppercase();
            let level = if upper.contains("ERROR")
                || upper.contains("SEVERE")
                || upper.contains("GRAVE")
                || upper.contains("EXCEPTION")
                || upper.contains("FATAL")
            {
                "ERROR"
            } else if upper.contains("WARN") || upper.contains("AVISO") {
                "WARN"
            } else {
                "INFO"
            };
            ParsedLogLine {
                text: l.to_string(),
                level,
            }
        })
        .collect()
}

pub fn delete_session_log(minecraft_dir: &Path, file_name: &str) -> Result<()> {
    let dir = logs_dir(minecraft_dir);
    let path = dir.join(file_name);
    if !path.starts_with(&dir) {
        return Err(LauncherError::InvalidMetadata(
            "Refusing to delete log outside logs directory.".into(),
        ));
    }
    std::fs::remove_file(&path).map_err(|source| LauncherError::Io { path, source })
}
