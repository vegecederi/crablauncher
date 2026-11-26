use crate::error::{LauncherError, Result};
use directories::ProjectDirs;
use std::path::{Path, PathBuf};

/// Caminhos padronizados do aplicativo respeitando XDG no Linux,
/// AppData no Windows e Application Support/Caches no macOS.
#[derive(Debug, Clone)]
pub struct AppPaths {
    pub config_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub data_dir: PathBuf,
    pub instances_override: Option<PathBuf>,
    pub mods_override: Option<PathBuf>,
    pub icons_override: Option<PathBuf>,
    pub runtimes_override: Option<PathBuf>,
    pub skins_override: Option<PathBuf>,
    pub downloads_override: Option<PathBuf>,
}

impl AppPaths {
    /// Resolve os diretórios padrão do sistema para o Crab.
    pub fn resolve() -> Result<Self> {
        let proj = ProjectDirs::from("org", "crab", "crab")
            .ok_or(LauncherError::DirectoriesUnavailable)?;

        Ok(Self {
            config_dir: proj.config_dir().to_path_buf(),
            cache_dir: proj.cache_dir().to_path_buf(),
            data_dir: proj.data_dir().to_path_buf(),
            instances_override: None,
            mods_override: None,
            icons_override: None,
            runtimes_override: None,
            skins_override: None,
            downloads_override: None,
        })
    }

    /// Constrói os caminhos a partir de uma raiz explícita (útil para testes e modo portátil).
    pub fn from_root(root: impl AsRef<Path>) -> Self {
        let root = root.as_ref();
        Self {
            config_dir: root.to_path_buf(),
            cache_dir: root.join("cache"),
            data_dir: root.to_path_buf(),
            instances_override: None,
            mods_override: None,
            icons_override: None,
            runtimes_override: None,
            skins_override: None,
            downloads_override: None,
        }
    }

    /// Resolve um caminho configurado (absoluto ou relativo a `data_dir`).
    pub fn resolve_custom_dir(&self, raw: &str) -> Option<PathBuf> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return None;
        }
        let p = PathBuf::from(trimmed);
        if p.is_absolute() {
            Some(p)
        } else {
            Some(self.data_dir.join(p))
        }
    }

    /// Aplica os diretórios personalizados de `FolderSettings`.
    pub fn apply_folder_settings(&mut self, folders: &crate::core::config::FolderSettings) {
        self.instances_override = self.resolve_custom_dir(&folders.instances_dir);
        self.mods_override = self.resolve_custom_dir(&folders.mods_dir);
        self.icons_override = self.resolve_custom_dir(&folders.icons_dir);
        self.runtimes_override = self.resolve_custom_dir(&folders.java_runtimes_dir);
        self.skins_override = self.resolve_custom_dir(&folders.skins_dir);
        self.downloads_override = self.resolve_custom_dir(&folders.downloads_dir);
    }

    pub fn config_file(&self) -> PathBuf {
        self.config_dir.join("config.json")
    }

    pub fn accounts_file(&self) -> PathBuf {
        self.config_dir.join("accounts.json")
    }

    pub fn groups_file(&self) -> PathBuf {
        self.data_dir.join("groups.json")
    }

    pub fn manifest_cache_file(&self) -> PathBuf {
        self.cache_dir.join("version_manifest_v2.json")
    }

    pub fn instances_dir(&self) -> PathBuf {
        self.instances_override
            .clone()
            .unwrap_or_else(|| self.data_dir.join("instances"))
    }

    pub fn mods_cache_dir(&self) -> PathBuf {
        self.mods_override
            .clone()
            .unwrap_or_else(|| self.data_dir.join("mods"))
    }

    pub fn skins_dir(&self) -> PathBuf {
        self.skins_override
            .clone()
            .unwrap_or_else(|| self.data_dir.join("skins"))
    }

    pub fn downloads_dir(&self) -> PathBuf {
        self.downloads_override
            .clone()
            .unwrap_or_else(|| self.cache_dir.join("downloads"))
    }

    pub fn instance_dir(&self, dir_name: &str) -> PathBuf {
        self.instances_dir().join(dir_name)
    }

    pub fn instance_metadata_file(&self, dir_name: &str) -> PathBuf {
        self.instance_dir(dir_name).join("instance.json")
    }

    /// Diretório de jogo isolado (`.minecraft`) de uma instância específica.
    pub fn instance_game_dir(&self, dir_name: &str) -> PathBuf {
        self.instance_dir(dir_name).join(".minecraft")
    }

    pub fn assets_dir(&self) -> PathBuf {
        self.data_dir.join("assets")
    }

    pub fn assets_indexes_dir(&self) -> PathBuf {
        self.assets_dir().join("indexes")
    }

    pub fn asset_index_file(&self, index_id: &str) -> PathBuf {
        self.assets_indexes_dir().join(format!("{index_id}.json"))
    }

    pub fn assets_objects_dir(&self) -> PathBuf {
        self.assets_dir().join("objects")
    }

    pub fn asset_object_file(&self, hash: &str) -> PathBuf {
        let prefix = hash.get(0..2).unwrap_or("00");
        self.assets_objects_dir().join(prefix).join(hash)
    }

    pub fn libraries_dir(&self) -> PathBuf {
        self.data_dir.join("libraries")
    }

    pub fn library_file(&self, relative_maven_path: &str) -> PathBuf {
        self.libraries_dir().join(relative_maven_path)
    }

    pub fn versions_dir(&self) -> PathBuf {
        self.data_dir.join("versions")
    }

    pub fn version_dir(&self, version_id: &str) -> PathBuf {
        self.versions_dir().join(version_id)
    }

    pub fn version_metadata_file(&self, version_id: &str) -> PathBuf {
        self.version_dir(version_id)
            .join(format!("{version_id}.json"))
    }

    pub fn version_jar_file(&self, version_id: &str) -> PathBuf {
        self.version_dir(version_id)
            .join(format!("{version_id}.jar"))
    }

    /// Diretório gerenciado e isolado por versão/plataforma para extração de bibliotecas nativas.
    pub fn version_natives_dir(&self, version_id: &str, platform_tag: &str) -> PathBuf {
        self.version_dir(version_id)
            .join("natives")
            .join(platform_tag)
    }

    pub fn runtimes_dir(&self) -> PathBuf {
        self.runtimes_override
            .clone()
            .unwrap_or_else(|| self.data_dir.join("runtimes"))
    }

    pub fn icons_dir(&self) -> PathBuf {
        self.icons_override
            .clone()
            .unwrap_or_else(|| self.data_dir.join("icons"))
    }

    pub fn themes_dir(&self) -> PathBuf {
        self.data_dir.join("themes")
    }

    pub fn icon_packs_dir(&self) -> PathBuf {
        self.data_dir.join("icon-packs")
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.data_dir.join("logs")
    }

    pub fn jvm_presets_dir(&self) -> PathBuf {
        self.data_dir
            .join("presets")
            .join("jvm")
            .join("custom-presets")
    }
}
