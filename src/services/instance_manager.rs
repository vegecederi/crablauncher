use crate::core::instance::{
    sanitize_dir_name, validate_instance_name, InstallationStatus, InstanceMetadata,
};
use crate::core::version::VersionType;
use crate::error::{LauncherError, Result};
use crate::platform::paths::AppPaths;
use crate::services::storage::{read_json, write_json_atomic};
use std::path::{Component, Path, PathBuf};
use tokio::fs;
use tracing::{info, warn};

const MINECRAFT_SUBDIRS: &[&str] = &[
    "saves",
    "mods",
    "resourcepacks",
    "shaderpacks",
    "config",
    "screenshots",
    "logs",
];

/// Gerencia o ciclo de vida e a persistência das instâncias em disco.
#[derive(Clone)]
pub struct InstanceManager {
    paths: AppPaths,
}

impl InstanceManager {
    pub fn new(paths: AppPaths) -> Self {
        Self { paths }
    }

    /// Carrega todas as instâncias válidas em `instances/`.
    /// Instalações que ficaram em estado `installing` após encerramento abrupto
    /// são marcadas como `incomplete` automaticamente.
    pub async fn list_instances(&self) -> Result<Vec<InstanceMetadata>> {
        let instances_dir = self.paths.instances_dir();
        if !instances_dir.exists() {
            return Ok(Vec::new());
        }

        let mut entries =
            fs::read_dir(&instances_dir)
                .await
                .map_err(|source| LauncherError::Io {
                    path: instances_dir.clone(),
                    source,
                })?;

        let mut instances = Vec::new();
        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|source| LauncherError::Io {
                path: instances_dir.clone(),
                source,
            })?
        {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            let Some(dir_name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };

            let metadata_path = path.join("instance.json");
            if !metadata_path.exists() {
                warn!(
                    "Skipping instance directory without instance.json: {}",
                    path.display()
                );
                continue;
            }

            match read_json::<InstanceMetadata>(&metadata_path).await {
                Ok(mut meta) => {
                    meta.dir_name = dir_name.to_string();

                    // Recupera instalações interrompidas por fechamento inesperado.
                    if meta.installation.status == InstallationStatus::Installing {
                        warn!(
                            "Instance '{}' ({}) was interrupted during installation; marking as incomplete.",
                            meta.name, meta.id
                        );
                        meta.set_installation_status(
                            InstallationStatus::Incomplete,
                            Some("Previous installation was interrupted.".to_string()),
                        );
                        if let Err(err) = write_json_atomic(&metadata_path, &meta).await {
                            warn!("Failed to persist recovered instance state: {err}");
                        }
                    }

                    instances.push(meta);
                }
                Err(err) => {
                    warn!(
                        "Skipping corrupted instance metadata at {}: {err}",
                        metadata_path.display()
                    );
                }
            }
        }

        instances.sort_by_key(|a| std::cmp::Reverse(a.created_at));
        Ok(instances)
    }

    /// Cria uma nova instância com diretório isolado e sanitizado em `instances/<dir_name>/.minecraft/`.
    pub async fn create_instance(
        &self,
        name: &str,
        minecraft_version: &str,
        version_type: VersionType,
    ) -> Result<InstanceMetadata> {
        let validated_name = validate_instance_name(name)?;
        let dir_name = self.allocate_unique_dir_name(&validated_name)?;
        let metadata =
            InstanceMetadata::new(&validated_name, minecraft_version, version_type, &dir_name)?;

        let instance_dir = self.safe_instance_dir(&dir_name)?;
        let game_dir = self.paths.instance_game_dir(&dir_name);

        for subdir in MINECRAFT_SUBDIRS {
            let sub_path = game_dir.join(subdir);
            fs::create_dir_all(&sub_path)
                .await
                .map_err(|source| LauncherError::Io {
                    path: sub_path,
                    source,
                })?;
        }

        let metadata_path = self.paths.instance_metadata_file(&dir_name);
        write_json_atomic(&metadata_path, &metadata).await?;

        info!(
            "Created instance '{}' (id={}, dir={}, mc={}) at {}",
            metadata.name,
            metadata.id,
            metadata.dir_name,
            metadata.minecraft_version,
            instance_dir.display()
        );
        Ok(metadata)
    }

    /// Renomeia a instância atualizando apenas `instance.json`, sem mover arquivos no disco.
    pub async fn rename_instance(&self, id: &str, new_name: &str) -> Result<InstanceMetadata> {
        let mut metadata = self.find_by_id(id).await?;
        metadata.rename(new_name)?;
        let metadata_path = self.paths.instance_metadata_file(&metadata.dir_name);
        write_json_atomic(&metadata_path, &metadata).await?;

        info!("Renamed instance {} to '{}'", id, metadata.name);
        Ok(metadata)
    }

    /// Registra o horário da última execução confirmada da instância.
    pub async fn record_last_played(&self, id: &str) -> Result<InstanceMetadata> {
        let mut metadata = self.find_by_id(id).await?;
        metadata.last_played = Some(chrono::Utc::now());
        metadata.updated_at = chrono::Utc::now();
        let metadata_path = self.paths.instance_metadata_file(&metadata.dir_name);
        write_json_atomic(&metadata_path, &metadata).await?;
        Ok(metadata)
    }

    /// Registra uma sessão concluída de jogo (somente após o processo ter realmente iniciado).
    pub async fn record_playtime_session(
        &self,
        id: &str,
        elapsed_secs: u64,
    ) -> Result<InstanceMetadata> {
        let mut metadata = self.find_by_id(id).await?;
        metadata.last_played = Some(chrono::Utc::now());
        metadata.playtime.total_playtime_secs = metadata
            .playtime
            .total_playtime_secs
            .saturating_add(elapsed_secs);
        metadata.playtime.last_session_secs = elapsed_secs;
        metadata.playtime.launch_count = metadata.playtime.launch_count.saturating_add(1);
        metadata.updated_at = chrono::Utc::now();
        let metadata_path = self.paths.instance_metadata_file(&metadata.dir_name);
        write_json_atomic(&metadata_path, &metadata).await?;
        Ok(metadata)
    }

    /// Define ou remove (`None`) o ícone personalizado de uma instância sem afetar outras instâncias.
    pub async fn set_instance_icon_key(
        &self,
        id: &str,
        icon_key: Option<String>,
    ) -> Result<InstanceMetadata> {
        let mut metadata = self.find_by_id(id).await?;
        metadata.icon_key = icon_key;
        metadata.updated_at = chrono::Utc::now();
        let metadata_path = self.paths.instance_metadata_file(&metadata.dir_name);
        write_json_atomic(&metadata_path, &metadata).await?;
        Ok(metadata)
    }

    /// Lista todos os ícones personalizados válidos armazenados em `<data_dir>/icons/`.
    pub fn list_managed_icons(&self) -> Vec<(String, PathBuf)> {
        self.list_managed_icons_with_names()
            .into_iter()
            .map(|(k, _, p)| (k, p))
            .collect()
    }

    /// Lista ícones personalizados com nome legível persistido em `icons-metadata.json`
    /// (evitando exibir UUIDs na interface, mantendo identificadores internos estáveis).
    pub fn list_managed_icons_with_names(&self) -> Vec<(String, String, PathBuf)> {
        let dir = self.paths.icons_dir();
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return Vec::new();
        };
        let names = self.load_icon_names();
        let mut out = Vec::new();
        for entry in entries.flatten() {
            let p = entry.path();
            if !p.is_file() {
                continue;
            }
            let ext_ok = p
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| {
                    matches!(
                        e.to_ascii_lowercase().as_str(),
                        "png" | "jpg" | "jpeg" | "webp"
                    )
                })
                .unwrap_or(false);
            if ext_ok {
                if let Some(fname) = p.file_name().and_then(|n| n.to_str()) {
                    let display = names.get(fname).cloned().unwrap_or_else(|| {
                        p.file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or(fname)
                            .to_string()
                    });
                    out.push((fname.to_string(), display, p));
                }
            }
        }
        out.sort_by_key(|(_, display, _)| display.to_lowercase());
        out
    }

    /// Renomeia o nome de exibição de um ícone personalizado sem alterar seu identificador interno.
    pub fn rename_managed_icon(&self, key: &str, new_name: &str) -> Result<()> {
        validate_single_dir_component(key)?;
        let clean = new_name.trim();
        if clean.is_empty() {
            return Err(LauncherError::InvalidInstanceName(
                "Icon name cannot be empty.".to_string(),
            ));
        }
        let icon_path = self.paths.icons_dir().join(key);
        if !icon_path.is_file() {
            return Err(LauncherError::UnsafePath(format!(
                "Custom icon '{key}' not found."
            )));
        }
        let mut names = self.load_icon_names();
        names.insert(key.to_string(), clean.to_string());
        self.save_icon_names(&names)
    }

    /// Conta quantas instâncias estão utilizando um determinado ícone personalizado.
    pub async fn count_instances_using_icon(&self, key: &str) -> Result<usize> {
        let instances = self.list_instances().await?;
        Ok(instances
            .iter()
            .filter(|i| i.icon_key.as_deref() == Some(key))
            .count())
    }

    /// Exclui um ícone personalizado do diretório gerenciado pelo Crab, substituindo-o pelo ícone padrão
    /// em todas as instâncias que o utilizam. Nunca remove arquivos externos originais.
    pub async fn delete_managed_icon(&self, key: &str) -> Result<usize> {
        validate_single_dir_component(key)?;
        let icons_dir = self.paths.icons_dir();
        let target = icons_dir.join(key);
        if target.parent() != Some(icons_dir.as_path()) {
            return Err(LauncherError::UnsafePath(
                "Invalid icon path outside managed icons directory.".to_string(),
            ));
        }

        let mut updated_count = 0usize;
        let instances = self.list_instances().await?;
        for mut inst in instances {
            if inst.icon_key.as_deref() == Some(key) {
                inst.icon_key = None;
                inst.updated_at = chrono::Utc::now();
                let metadata_path = self.paths.instance_metadata_file(&inst.dir_name);
                write_json_atomic(&metadata_path, &inst).await?;
                updated_count += 1;
            }
        }

        if target.is_file() {
            fs::remove_file(&target)
                .await
                .map_err(|source| LauncherError::Io {
                    path: target,
                    source,
                })?;
        }

        let mut names = self.load_icon_names();
        if names.remove(key).is_some() {
            let _ = self.save_icon_names(&names);
        }

        Ok(updated_count)
    }

    fn load_icon_names(&self) -> std::collections::HashMap<String, String> {
        let meta_path = self.paths.icons_dir().join("icons-metadata.json");
        std::fs::read(&meta_path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    fn save_icon_names(&self, map: &std::collections::HashMap<String, String>) -> Result<()> {
        let dir = self.paths.icons_dir();
        std::fs::create_dir_all(&dir).map_err(|source| LauncherError::Io {
            path: dir.clone(),
            source,
        })?;
        let meta_path = dir.join("icons-metadata.json");
        let tmp_path = dir.join("icons-metadata.json.tmp");
        let bytes = serde_json::to_vec_pretty(map)?;
        std::fs::write(&tmp_path, bytes).map_err(|source| LauncherError::Io {
            path: tmp_path.clone(),
            source,
        })?;
        std::fs::rename(&tmp_path, &meta_path).map_err(|source| LauncherError::Io {
            path: meta_path,
            source,
        })?;
        Ok(())
    }

    /// Atualiza o modloader instalado na instância de forma atômica.
    pub async fn set_modloader(
        &self,
        id: &str,
        spec: Option<crate::core::components::model::ModloaderSpec>,
    ) -> Result<InstanceMetadata> {
        let mut metadata = self.find_by_id(id).await?;
        metadata.modloader = spec
            .as_ref()
            .map(|s| format!("{}:{}", s.kind.short_name(), s.version));
        metadata.modloader_spec = spec;
        metadata.updated_at = chrono::Utc::now();
        let metadata_path = self.paths.instance_metadata_file(&metadata.dir_name);
        write_json_atomic(&metadata_path, &metadata).await?;
        Ok(metadata)
    }

    /// Atualiza o estado de instalação da instância de forma atômica.
    pub async fn update_installation_status(
        &self,
        id: &str,
        status: InstallationStatus,
        last_error: Option<String>,
    ) -> Result<InstanceMetadata> {
        let mut metadata = self.find_by_id(id).await?;
        metadata.set_installation_status(status, last_error);
        let metadata_path = self.paths.instance_metadata_file(&metadata.dir_name);
        write_json_atomic(&metadata_path, &metadata).await?;
        Ok(metadata)
    }

    /// Associa ou remove a instância de um grupo (sem mover arquivos físicos).
    pub async fn set_instance_group(
        &self,
        id: &str,
        group_id: Option<String>,
    ) -> Result<InstanceMetadata> {
        let mut metadata = self.find_by_id(id).await?;
        metadata.group_id = group_id;
        metadata.updated_at = chrono::Utc::now();
        let metadata_path = self.paths.instance_metadata_file(&metadata.dir_name);
        write_json_atomic(&metadata_path, &metadata).await?;
        Ok(metadata)
    }

    /// Remove a referência de um grupo excluído de todas as instâncias associadas.
    pub async fn clear_group_from_instances(&self, group_id: &str) -> Result<()> {
        let instances = self.list_instances().await?;
        for mut inst in instances {
            if inst.group_id.as_deref() == Some(group_id) {
                inst.group_id = None;
                inst.updated_at = chrono::Utc::now();
                let metadata_path = self.paths.instance_metadata_file(&inst.dir_name);
                write_json_atomic(&metadata_path, &inst).await?;
            }
        }
        Ok(())
    }

    /// Atualiza nome, grupo e sobrescritas individuais da instância.
    pub async fn update_instance_settings(
        &self,
        id: &str,
        new_name: &str,
        group_id: Option<String>,
        overrides: crate::core::instance::InstanceOverrides,
    ) -> Result<InstanceMetadata> {
        let mut metadata = self.find_by_id(id).await?;
        metadata.rename(new_name)?;
        metadata.group_id = group_id;
        metadata.overrides = overrides;
        metadata.updated_at = chrono::Utc::now();
        let metadata_path = self.paths.instance_metadata_file(&metadata.dir_name);
        write_json_atomic(&metadata_path, &metadata).await?;
        Ok(metadata)
    }

    /// Importa um ícone personalizado (PNG, JPEG, WebP, até 4 MiB) para `icons/` e associa à instância.
    pub async fn import_instance_icon(
        &self,
        id: &str,
        source_path: &Path,
    ) -> Result<InstanceMetadata> {
        const MAX_ICON_BYTES: u64 = 4 * 1024 * 1024;
        let ext = source_path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .filter(|e| matches!(e.as_str(), "png" | "jpg" | "jpeg" | "webp"))
            .ok_or_else(|| {
                LauncherError::UnsafePath(
                    "Unsupported icon format. Use PNG, JPEG, or WebP.".to_string(),
                )
            })?;

        let meta = fs::metadata(source_path)
            .await
            .map_err(|source| LauncherError::Io {
                path: source_path.to_path_buf(),
                source,
            })?;
        if meta.len() > MAX_ICON_BYTES {
            return Err(LauncherError::UnsafePath(
                "Icon file exceeds maximum allowed size (4 MiB).".to_string(),
            ));
        }

        let icons_dir = self.paths.icons_dir();
        fs::create_dir_all(&icons_dir)
            .await
            .map_err(|source| LauncherError::Io {
                path: icons_dir.clone(),
                source,
            })?;

        let icon_filename = format!("{}.{ext}", uuid::Uuid::new_v4());
        let dest_path = icons_dir.join(&icon_filename);
        fs::copy(source_path, &dest_path)
            .await
            .map_err(|source| LauncherError::Io {
                path: dest_path,
                source,
            })?;

        // Registra o nome legível original em icons-metadata.json para evitar exibir UUID na UI
        if let Some(stem) = source_path.file_stem().and_then(|s| s.to_str()) {
            let clean = stem.trim();
            if !clean.is_empty() {
                let mut names = self.load_icon_names();
                names.insert(icon_filename.clone(), clean.to_string());
                let _ = self.save_icon_names(&names);
            }
        }

        let mut inst = self.find_by_id(id).await?;
        inst.icon_key = Some(icon_filename);
        inst.updated_at = chrono::Utc::now();
        let metadata_path = self.paths.instance_metadata_file(&inst.dir_name);
        write_json_atomic(&metadata_path, &inst).await?;
        Ok(inst)
    }

    /// Duplica uma instância existente com novo UUID e diretório isolado,
    /// copiando seletivamente os diretórios de jogo solicitados sem duplicar recursos compartilhados.
    #[allow(clippy::too_many_arguments)]
    pub async fn copy_instance(
        &self,
        source_id: &str,
        new_name: &str,
        group_id: Option<String>,
        copy_saves: bool,
        copy_config: bool,
        copy_resourcepacks: bool,
        copy_screenshots: bool,
    ) -> Result<InstanceMetadata> {
        let source = self.find_by_id(source_id).await?;
        let mut created = self
            .create_instance(new_name, &source.minecraft_version, source.version_type)
            .await?;

        created.group_id = group_id;
        created.icon_key = source.icon_key.clone();
        created.overrides = source.overrides.clone();
        created.installation = source.installation.clone();
        created.updated_at = chrono::Utc::now();

        let src_game = self.paths.instance_game_dir(&source.dir_name);
        let dst_game = self.paths.instance_game_dir(&created.dir_name);

        let pairs = [
            (copy_saves, "saves"),
            (copy_config, "config"),
            (copy_resourcepacks, "resourcepacks"),
            (copy_screenshots, "screenshots"),
        ];

        for (enabled, subdir) in pairs {
            if enabled {
                let s = src_game.join(subdir);
                let d = dst_game.join(subdir);
                if s.is_dir() {
                    copy_dir_recursive(&s, &d)?;
                }
            }
        }

        let metadata_path = self.paths.instance_metadata_file(&created.dir_name);
        write_json_atomic(&metadata_path, &created).await?;
        info!(
            "Copied instance '{}' -> '{}' (id={})",
            source.name, created.name, created.id
        );
        Ok(created)
    }

    /// Remove exclusivamente o diretório da instância, preservando recursos compartilhados.
    pub async fn delete_instance(&self, id: &str) -> Result<()> {
        let metadata = self.find_by_id(id).await?;
        let instance_dir = self.safe_instance_dir(&metadata.dir_name)?;

        if !instance_dir.exists() {
            return Err(LauncherError::InstanceNotFound(id.to_string()));
        }

        fs::remove_dir_all(&instance_dir)
            .await
            .map_err(|source| LauncherError::Io {
                path: instance_dir.clone(),
                source,
            })?;

        info!("Deleted instance {} at {}", id, instance_dir.display());
        Ok(())
    }

    /// Retorna o diretório físico da instância pelo seu UUID.
    pub async fn instance_folder(&self, id: &str) -> Result<PathBuf> {
        let metadata = self.find_by_id(id).await?;
        let dir = self.safe_instance_dir(&metadata.dir_name)?;
        if !dir.exists() {
            return Err(LauncherError::InstanceNotFound(id.to_string()));
        }
        Ok(dir)
    }

    /// Localiza uma instância pelo UUID.
    pub async fn find_by_id(&self, id: &str) -> Result<InstanceMetadata> {
        let instances = self.list_instances().await?;
        instances
            .into_iter()
            .find(|i| i.id == id)
            .ok_or_else(|| LauncherError::InstanceNotFound(id.to_string()))
    }

    /// Gera um nome de diretório único a partir do nome sanitizado, evitando colisões.
    fn allocate_unique_dir_name(&self, display_name: &str) -> Result<String> {
        let base = sanitize_dir_name(display_name);
        let instances_dir = self.paths.instances_dir();

        if !instances_dir.join(&base).exists() {
            return Ok(base);
        }

        for suffix in 2u32..10_000 {
            let candidate = format!("{base}-{suffix}");
            if !instances_dir.join(&candidate).exists() {
                return Ok(candidate);
            }
        }

        Err(LauncherError::UnsafePath(
            "Unable to allocate a unique directory name for instance".to_string(),
        ))
    }

    /// Garante que o diretório resolvido seja estritamente filho direto de `instances/`.
    fn safe_instance_dir(&self, dir_name: &str) -> Result<PathBuf> {
        validate_single_dir_component(dir_name)?;
        let instances_root = self.paths.instances_dir();
        let target = instances_root.join(dir_name);
        if target.parent() != Some(instances_root.as_path()) {
            return Err(LauncherError::UnsafePath(format!(
                "Resolved path escapes instances root: {}",
                target.display()
            )));
        }
        Ok(target)
    }
}

fn validate_single_dir_component(name: &str) -> Result<()> {
    let mut components = Path::new(name).components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(_)), None) => Ok(()),
        _ => Err(LauncherError::UnsafePath(format!(
            "Invalid directory component: '{name}'"
        ))),
    }
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst).map_err(|source| LauncherError::Io {
        path: dst.to_path_buf(),
        source,
    })?;
    for entry_res in std::fs::read_dir(src).map_err(|source| LauncherError::Io {
        path: src.to_path_buf(),
        source,
    })? {
        let entry = entry_res.map_err(|source| LauncherError::Io {
            path: src.to_path_buf(),
            source,
        })?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else if src_path.is_file() {
            std::fs::copy(&src_path, &dst_path).map_err(|source| LauncherError::Io {
                path: dst_path,
                source,
            })?;
        }
    }
    Ok(())
}
