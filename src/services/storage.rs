use crate::core::config::LauncherConfig;
use crate::error::{LauncherError, Result};
use crate::platform::paths::AppPaths;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::path::Path;
use tokio::fs;
use tracing::warn;

/// Garante que toda a estrutura de diretórios base do launcher exista no disco.
pub async fn ensure_base_directories(paths: &AppPaths) -> Result<()> {
    let dirs = [
        paths.config_dir.clone(),
        paths.cache_dir.clone(),
        paths.instances_dir(),
        paths.assets_indexes_dir(),
        paths.assets_objects_dir(),
        paths.libraries_dir(),
        paths.versions_dir(),
        paths.runtimes_dir(),
        paths.logs_dir(),
    ];

    for dir in dirs {
        fs::create_dir_all(&dir)
            .await
            .map_err(|source| LauncherError::Io { path: dir, source })?;
    }

    Ok(())
}

/// Escreve um arquivo JSON de forma segura usando arquivo temporário + rename
/// para evitar corrupção em caso de encerramento inesperado.
pub async fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .await
            .map_err(|source| LauncherError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
    }

    let bytes = serde_json::to_vec_pretty(value)?;
    let tmp_path = path.with_extension("json.tmp");

    fs::write(&tmp_path, bytes)
        .await
        .map_err(|source| LauncherError::Io {
            path: tmp_path.clone(),
            source,
        })?;

    fs::rename(&tmp_path, path)
        .await
        .map_err(|source| LauncherError::Io {
            path: path.to_path_buf(),
            source,
        })?;

    Ok(())
}

/// Lê e desserializa um arquivo JSON do disco.
pub async fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let bytes = fs::read(path).await.map_err(|source| LauncherError::Io {
        path: path.to_path_buf(),
        source,
    })?;

    serde_json::from_slice(&bytes).map_err(|source| LauncherError::JsonParse {
        path: path.to_path_buf(),
        source,
    })
}

/// Carrega `config.json`. Caso não exista ou esteja corrompido, retorna a configuração padrão.
pub async fn load_config(paths: &AppPaths) -> LauncherConfig {
    let path = paths.config_file();
    if !path.exists() {
        let default = LauncherConfig::default();
        if let Err(err) = write_json_atomic(&path, &default).await {
            warn!("Failed to write initial config.json: {err}");
        }
        return default;
    }

    match read_json::<LauncherConfig>(&path).await {
        Ok(mut cfg) => {
            if cfg.migrate() {
                if let Err(err) = write_json_atomic(&path, &cfg).await {
                    warn!("Failed to persist migrated config.json: {err}");
                }
            }
            cfg
        }
        Err(err) => {
            warn!(
                "Invalid or corrupted config.json at {}: {err}. Falling back to defaults.",
                path.display()
            );
            LauncherConfig::default()
        }
    }
}

/// Persiste `config.json`.
pub async fn save_config(paths: &AppPaths, config: &LauncherConfig) -> Result<()> {
    write_json_atomic(&paths.config_file(), config).await
}
