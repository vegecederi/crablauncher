use std::{
    collections::{BTreeMap, HashMap},
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, AtomicUsize, Ordering},
        Arc,
    },
};

use chrono::Utc;
use futures_util::{stream, StreamExt, TryStreamExt};
use reqwest::Client;
use serde::Deserialize;
use sha1::{Digest, Sha1};
use zip::ZipArchive;

use crate::{
    core::{
        components::model::ModloaderKind,
        mods::platform::{
            fetch_and_cache_mod_icon, InstalledModRecord, ModIndex, ModProvider, ModSortBy,
        },
    },
    error::{LauncherError, Result},
};

const ARCHIVE_LIMIT: u64 = 1024 * 1024 * 1024;
const ICON_LIMIT: u64 = 4 * 1024 * 1024;
const MODRINTH_API_BASE: &str = "https://api.modrinth.com/v2";

fn error(message: impl ToString) -> LauncherError {
    LauncherError::Launch(message.to_string())
}

#[derive(Clone, Deserialize)]
pub struct Project {
    pub project_id: String,
    #[serde(default)]
    pub slug: String,
    pub title: String,
    pub description: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub downloads: u64,
    pub icon_url: Option<String>,
}

impl Project {
    pub fn homepage_url(&self) -> String {
        if self.slug.is_empty() {
            format!("https://modrinth.com/modpack/{}", self.project_id)
        } else {
            format!("https://modrinth.com/modpack/{}", self.slug)
        }
    }
}

#[derive(Clone, Default)]
pub struct PackSearchFilters {
    pub game_version: Option<String>,
    pub loaders: Vec<String>,
    pub categories: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct StageProgress {
    pub stage: String,
    pub completed_files: usize,
    pub total_files: usize,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub current_file: String,
    pub indeterminate: bool,
}

#[derive(Clone, Deserialize)]
pub struct Release {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub version_number: String,
    #[serde(default)]
    pub version_type: String,
    #[serde(default)]
    pub changelog: Option<String>,
    #[serde(default)]
    pub game_versions: Vec<String>,
    #[serde(default)]
    pub loaders: Vec<String>,
    files: Vec<ReleaseFile>,
}

#[derive(Clone, Deserialize)]
struct ReleaseFile {
    url: String,
    filename: String,
    primary: bool,
    hashes: Hashes,
}

impl Release {
    pub fn archive(&self) -> Option<(&str, &str)> {
        let mut files = self
            .files
            .iter()
            .filter(|f| f.filename.ends_with(".mrpack"));
        let first = files.next()?;
        let file = if first.primary {
            first
        } else {
            files.find(|f| f.primary).unwrap_or(first)
        };
        Some((&file.url, &file.hashes.sha1))
    }

    pub fn display_label(&self) -> String {
        let base = if !self.name.trim().is_empty() {
            self.name.trim()
        } else {
            self.version_number.trim()
        };
        let suffix = self.game_versions.first().map(|v| v.as_str()).unwrap_or("");
        if suffix.is_empty() || base.contains(suffix) {
            base.to_string()
        } else {
            format!("{base} ({suffix})")
        }
    }

    pub fn update_dropdown_label(&self, is_current: bool) -> String {
        let base = self.display_label();
        let kind = if self.version_type.trim().is_empty() {
            "release"
        } else {
            self.version_type.trim()
        };
        if is_current {
            format!("{base} — [{kind}] (Current)")
        } else {
            format!("{base} — [{kind}]")
        }
    }
}

pub async fn search_filtered_paged(
    client: &Client,
    query: &str,
    filters: &PackSearchFilters,
    sort: ModSortBy,
    offset: usize,
) -> Result<(Vec<Project>, usize)> {
    #[derive(Deserialize)]
    struct Results {
        hits: Vec<Project>,
        #[serde(default)]
        total_hits: usize,
    }
    let mut facets: Vec<Vec<String>> = vec![vec!["project_type:modpack".to_string()]];
    if let Some(mc) = filters
        .game_version
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        facets.push(vec![format!("versions:{mc}")]);
    }
    if !filters.loaders.is_empty() {
        facets.push(
            filters
                .loaders
                .iter()
                .map(|l| format!("categories:{l}"))
                .collect(),
        );
    }
    for cat in &filters.categories {
        facets.push(vec![format!("categories:{cat}")]);
    }
    let facets_json = serde_json::to_string(&facets).map_err(error)?;
    let results = client
        .get(format!("{MODRINTH_API_BASE}/search"))
        .query(&[
            ("query", query),
            ("index", sort.modrinth_index()),
            ("limit", "25"),
            ("offset", &offset.to_string()),
            ("facets", &facets_json),
        ])
        .send()
        .await
        .map_err(|err| error(err.without_url()))?
        .error_for_status()
        .map_err(|err| error(err.without_url()))?
        .json::<Results>()
        .await
        .map_err(error)?;
    Ok((results.hits, results.total_hits))
}

pub async fn search(client: &Client, query: &str) -> Result<Vec<Project>> {
    search_filtered_paged(
        client,
        query,
        &PackSearchFilters::default(),
        ModSortBy::Relevance,
        0,
    )
    .await
    .map(|(hits, _)| hits)
}

pub async fn releases(client: &Client, id: &str) -> Result<Vec<Release>> {
    if !id.bytes().all(|c| c.is_ascii_alphanumeric()) {
        return Err(error("Invalid Modrinth project ID."));
    }
    let releases = client
        .get(format!("{MODRINTH_API_BASE}/project/{id}/version"))
        .send()
        .await
        .map_err(|err| error(err.without_url()))?
        .error_for_status()
        .map_err(|err| error(err.without_url()))?
        .json::<Vec<Release>>()
        .await
        .map_err(error)?;
    Ok(releases
        .into_iter()
        .filter(|r| r.archive().is_some())
        .collect())
}

#[derive(Clone, Deserialize)]
struct Hashes {
    sha1: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Index {
    name: String,
    game: String,
    format_version: u32,
    dependencies: BTreeMap<String, String>,
    files: Vec<PackFile>,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PackFile {
    path: String,
    downloads: Vec<String>,
    hashes: Hashes,
    file_size: u64,
    env: Option<Environment>,
}

#[derive(Clone, Deserialize)]
struct Environment {
    client: String,
}

pub struct Pack {
    pub name: String,
    pub minecraft: String,
    pub loader: Option<(ModloaderKind, String)>,
    pub icon: Option<Vec<u8>>,
    files: Vec<PackFile>,
    prism_prefix: Option<String>,
}

fn safe_path(raw: &str) -> Result<PathBuf> {
    if raw.is_empty()
        || raw.contains(['\\', ':'])
        || raw
            .split('/')
            .any(|part| part.is_empty() || matches!(part, "." | ".."))
    {
        return Err(error("Unsafe path in modpack."));
    }
    Ok(raw.into())
}

fn safe_version(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && !matches!(value, "." | "..")
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._+-".contains(&c))
}

fn archive(path: &Path) -> Result<ZipArchive<File>> {
    let metadata = std::fs::metadata(path).map_err(error)?;
    if !metadata.is_file() {
        return Err(error("Select a modpack archive file."));
    }
    if metadata.len() > ARCHIVE_LIMIT {
        return Err(error("Modpack archive exceeds 1 GiB."));
    }
    let file = File::open(path).map_err(error)?;
    ZipArchive::new(file).map_err(error)
}

fn entry(zip: &mut ZipArchive<File>, name: &str, limit: u64) -> Result<Option<Vec<u8>>> {
    match zip.by_name(name) {
        Ok(file) => {
            if file.size() > limit {
                return Err(error("Modpack metadata or icon is too large."));
            }
            let mut data = Vec::new();
            file.take(limit + 1).read_to_end(&mut data).map_err(error)?;
            if data.len() as u64 > limit {
                return Err(error("Modpack entry is too large."));
            }
            Ok(Some(data))
        }
        Err(zip::result::ZipError::FileNotFound) => Ok(None),
        Err(err) => Err(error(err)),
    }
}

pub fn inspect(path: &Path) -> Result<Pack> {
    let mut zip = archive(path)?;
    if let Some(raw) = entry(&mut zip, "modrinth.index.json", 8 * 1024 * 1024)? {
        let mut index: Index = serde_json::from_slice(&raw).map_err(error)?;
        if index.game != "minecraft" || index.format_version != 1 {
            return Err(error("Unsupported Modrinth pack format."));
        }
        let minecraft = index.dependencies.remove("minecraft").unwrap_or_default();
        let mut loader = None;
        for (key, version) in index.dependencies {
            let kind = match key.as_str() {
                "fabric-loader" => ModloaderKind::Fabric,
                "quilt-loader" => ModloaderKind::Quilt,
                "forge" => ModloaderKind::Forge,
                "neoforge" => ModloaderKind::NeoForge,
                _ => return Err(error(format!("Unsupported dependency: {key}"))),
            };
            if loader.replace((kind, version)).is_some() {
                return Err(error("Multiple modloaders are not supported."));
            }
        }
        let mut icon = None;
        for name in [
            "icon.png",
            "client-overrides/icon.png",
            "overrides/icon.png",
        ] {
            if let Some(data) = entry(&mut zip, name, ICON_LIMIT)? {
                icon = Some(data);
                break;
            }
        }
        return validate(Pack {
            name: index.name,
            minecraft,
            loader,
            icon,
            files: index.files,
            prism_prefix: None,
        });
    }

    #[derive(Deserialize)]
    struct Components {
        components: Vec<Component>,
    }
    #[derive(Deserialize)]
    struct Component {
        uid: String,
        version: Option<String>,
    }
    let config = zip
        .file_names()
        .find(|name| *name == "instance.cfg" || name.ends_with("/instance.cfg"))
        .map(str::to_owned)
        .ok_or_else(|| {
            error("Unsupported archive. Use a Modrinth pack or a Prism/MultiMC instance export.")
        })?;
    let prefix = config
        .strip_suffix("instance.cfg")
        .unwrap_or_default()
        .to_owned();
    let raw =
        entry(&mut zip, &config, 1024 * 1024)?.ok_or_else(|| error("Missing instance.cfg."))?;
    let config_text = String::from_utf8_lossy(&raw);
    let setting = |key: &str| {
        config_text
            .lines()
            .filter_map(|line| line.split_once('='))
            .find(|(k, _)| *k == key)
            .map(|(_, v)| v.trim())
    };
    let name = setting("name")
        .filter(|s| !s.is_empty())
        .unwrap_or("Imported Instance")
        .to_owned();
    let raw = entry(&mut zip, &format!("{prefix}mmc-pack.json"), 1024 * 1024)?
        .ok_or_else(|| error("Missing mmc-pack.json."))?;
    let components: Components = serde_json::from_slice(&raw).map_err(error)?;
    let mut minecraft = String::new();
    let mut loader = None;
    for component in components.components {
        if component.uid == "net.minecraft" {
            minecraft = component.version.unwrap_or_default();
        } else if let Some(kind) = [
            ModloaderKind::Fabric,
            ModloaderKind::Forge,
            ModloaderKind::NeoForge,
            ModloaderKind::Quilt,
            ModloaderKind::LiteLoader,
        ]
        .into_iter()
        .find(|kind| kind.uid() == component.uid)
        {
            if loader
                .replace((kind, component.version.unwrap_or_default()))
                .is_some()
            {
                return Err(error("Multiple modloaders are not supported."));
            }
        } else if !matches!(
            component.uid.as_str(),
            "org.lwjgl"
                | "org.lwjgl3"
                | "net.minecraft.launchwrapper"
                | "net.fabricmc.intermediary"
        ) {
            return Err(error(format!(
                "Unsupported instance component: {}",
                component.uid
            )));
        }
    }
    let icon = if let Some(key) = setting("iconKey").filter(|k| safe_version(k)) {
        entry(&mut zip, &format!("{prefix}{key}.png"), ICON_LIMIT)?
    } else {
        None
    };
    let icon = match icon {
        Some(data) => Some(data),
        None => entry(&mut zip, &format!("{prefix}icon.png"), ICON_LIMIT)?,
    };
    validate(Pack {
        name,
        minecraft,
        loader,
        icon,
        files: Vec::new(),
        prism_prefix: Some(prefix),
    })
}

fn validate(pack: Pack) -> Result<Pack> {
    if !safe_version(&pack.minecraft) || pack.loader.as_ref().is_some_and(|(_, v)| !safe_version(v))
    {
        return Err(error("Missing or invalid Minecraft/modloader version."));
    }
    Ok(pack)
}

pub async fn download(
    client: &Client,
    url: &str,
    limit: u64,
    hash: Option<&str>,
    dest: &Path,
) -> Result<()> {
    let url = reqwest::Url::parse(url).map_err(error)?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || !matches!(
            url.host_str(),
            Some("cdn.modrinth.com" | "github.com" | "raw.githubusercontent.com" | "gitlab.com")
        )
    {
        return Err(error("Unsupported modpack download host."));
    }
    let mut response = client
        .get(url)
        .send()
        .await
        .map_err(|err| error(err.without_url()))?
        .error_for_status()
        .map_err(|err| error(err.without_url()))?;
    let mut file = tokio::fs::File::create(dest).await.map_err(error)?;
    let mut digest = Sha1::new();
    let mut length = 0;
    use tokio::io::AsyncWriteExt;
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|err| error(err.without_url()))?
    {
        length += chunk.len() as u64;
        if length > limit {
            return Err(error("Modpack download exceeds its size limit."));
        }
        if hash.is_some() {
            digest.update(&chunk);
        }
        file.write_all(&chunk).await.map_err(error)?;
    }
    file.flush().await.map_err(error)?;
    if hash
        .is_some_and(|expected| !format!("{:x}", digest.finalize()).eq_ignore_ascii_case(expected))
    {
        return Err(error("Modpack download checksum mismatch."));
    }
    Ok(())
}

pub async fn stage(
    client: &Client,
    pack: &Pack,
    source: PathBuf,
    game: PathBuf,
) -> Result<Vec<String>> {
    stage_with_progress(client, pack, source, game, |_| {}).await
}

pub async fn stage_with_progress<F>(
    client: &Client,
    pack: &Pack,
    source: PathBuf,
    game: PathBuf,
    on_progress: F,
) -> Result<Vec<String>>
where
    F: Fn(StageProgress) + Send + Sync + 'static,
{
    tokio::fs::create_dir_all(&game).await.map_err(error)?;
    let mut total_bytes = 0u64;
    let mut paths = std::collections::HashSet::new();
    let mut files = Vec::new();
    let mut tracked_files = Vec::new();
    for file in &pack.files {
        if file.env.as_ref().is_some_and(|e| e.client != "required") {
            continue;
        }
        let rel = safe_path(&file.path)?;
        let target = game.join(&rel);
        if !paths.insert(target.clone()) {
            return Err(error(format!("Duplicate modpack file: {}", file.path)));
        }
        total_bytes = total_bytes.saturating_add(file.file_size);
        if total_bytes > 4 * ARCHIVE_LIMIT {
            return Err(error("Modpack downloads exceed 4 GiB."));
        }
        tracked_files.push(file.path.clone());
        files.push((file.clone(), target));
    }
    let total_files = files.len();
    let completed = Arc::new(AtomicUsize::new(0));
    let downloaded = Arc::new(AtomicU64::new(0));
    let on_progress = Arc::new(on_progress);

    if total_files > 0 {
        on_progress(StageProgress {
            stage: format!("Downloading modpack components (0/{total_files})..."),
            completed_files: 0,
            total_files,
            downloaded_bytes: 0,
            total_bytes,
            current_file: String::new(),
            indeterminate: false,
        });
    }

    let progress_cb = Arc::clone(&on_progress);
    let completed_ref = Arc::clone(&completed);
    let downloaded_ref = Arc::clone(&downloaded);
    stream::iter(files.into_iter().map(move |(file, target)| {
        let completed_c = Arc::clone(&completed_ref);
        let downloaded_c = Arc::clone(&downloaded_ref);
        let progress_c = Arc::clone(&progress_cb);
        async move {
            tokio::fs::create_dir_all(
                target
                    .parent()
                    .ok_or_else(|| error("Invalid modpack path."))?,
            )
            .await
            .map_err(error)?;
            let mut last = error("Modpack file does not have any download mirrors.");
            for url in &file.downloads {
                match download(
                    client,
                    url,
                    file.file_size,
                    Some(&file.hashes.sha1),
                    &target,
                )
                .await
                {
                    Ok(()) => {
                        let done = completed_c.fetch_add(1, Ordering::Relaxed) + 1;
                        let bytes = downloaded_c.fetch_add(file.file_size, Ordering::Relaxed)
                            + file.file_size;
                        let current = Path::new(&file.path)
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or(&file.path)
                            .to_string();
                        progress_c(StageProgress {
                            stage: format!(
                                "Downloading modpack components ({done}/{total_files})..."
                            ),
                            completed_files: done,
                            total_files,
                            downloaded_bytes: bytes,
                            total_bytes,
                            current_file: current,
                            indeterminate: false,
                        });
                        return Ok(());
                    }
                    Err(err) => last = err,
                }
            }
            Err(error(format!("{}: {last}", file.path)))
        }
    }))
    .buffer_unordered(crate::services::download::DEFAULT_MAX_CONCURRENT_DOWNLOADS)
    .try_collect::<Vec<()>>()
    .await?;

    on_progress(StageProgress {
        stage: "Extracting modpack overrides...".to_string(),
        completed_files: total_files,
        total_files,
        downloaded_bytes: total_bytes,
        total_bytes,
        current_file: String::new(),
        indeterminate: true,
    });

    let prefixes = if let Some(prefix) = &pack.prism_prefix {
        vec![
            format!("{prefix}.minecraft/"),
            format!("{prefix}minecraft/"),
        ]
    } else {
        vec!["overrides/".into(), "client-overrides/".into()]
    };
    let override_files = tokio::task::spawn_blocking(move || -> Result<Vec<String>> {
        let mut zip = archive(&source)?;
        let mut total = 0u64;
        let mut extracted = Vec::new();
        for prefix in prefixes {
            for i in 0..zip.len() {
                let file = zip.by_index(i).map_err(error)?;
                let Some(relative) = file.name().strip_prefix(&prefix).map(str::to_owned) else {
                    continue;
                };
                if file.is_dir() {
                    continue;
                }
                let dest = game.join(safe_path(&relative)?);
                if file.is_symlink() {
                    return Err(error("Symbolic links are not supported in modpacks."));
                }
                let size = file.size();
                total = total.saturating_add(size);
                if total > 4 * ARCHIVE_LIMIT {
                    return Err(error("Extracted modpack exceeds 4 GiB."));
                }
                std::fs::create_dir_all(
                    dest.parent()
                        .ok_or_else(|| error("Invalid modpack path."))?,
                )
                .map_err(error)?;
                let mut output = File::create(&dest).map_err(error)?;
                if std::io::copy(&mut file.take(size + 1), &mut output).map_err(error)? != size {
                    return Err(error("Invalid modpack entry size."));
                }
                extracted.push(relative);
            }
        }
        Ok(extracted)
    })
    .await
    .map_err(error)??;
    tracked_files.extend(override_files);
    tracked_files.sort();
    tracked_files.dedup();
    Ok(tracked_files)
}

fn is_protected_user_path(relative: &str) -> bool {
    relative == "servers.dat"
        || relative == "options.txt"
        || relative.starts_with("saves/")
        || relative.starts_with("screenshots/")
        || relative.starts_with("logs/")
}

/// Aplica os arquivos preparados de uma atualização de modpack na pasta `.minecraft` da instância,
/// removendo arquivos obsoletos do modpack anterior sem apagar mundos, screenshots ou servidores do usuário.
pub async fn sync_staged_update(
    staged_game: PathBuf,
    target_game: PathBuf,
    previous_managed: Vec<String>,
    new_managed: Vec<String>,
) -> Result<()> {
    tokio::task::spawn_blocking(move || -> Result<()> {
        let new_set: std::collections::HashSet<&str> =
            new_managed.iter().map(String::as_str).collect();

        for old_rel in &previous_managed {
            if new_set.contains(old_rel.as_str()) || is_protected_user_path(old_rel) {
                continue;
            }
            let Ok(rel) = safe_path(old_rel) else {
                continue;
            };
            let candidate = target_game.join(rel);
            if candidate.is_file() {
                let _ = std::fs::remove_file(&candidate);
            }
        }

        for rel_str in &new_managed {
            let rel = safe_path(rel_str)?;
            let src = staged_game.join(&rel);
            if !src.is_file() {
                continue;
            }
            let dst = target_game.join(&rel);
            // Preserva options.txt e saves já existentes do usuário durante atualização.
            if is_protected_user_path(rel_str) && dst.exists() {
                continue;
            }
            if let Some(parent) = dst.parent() {
                std::fs::create_dir_all(parent).map_err(error)?;
            }
            std::fs::copy(&src, &dst).map_err(error)?;
        }

        // Remove do ModIndex entradas cujos arquivos não existem mais após a atualização.
        let mods_dir = target_game.join("mods");
        if mods_dir.is_dir() {
            let mut index = ModIndex::load(&mods_dir);
            let before = index.mods.len();
            index.mods.retain(|rec| {
                let base = rec.file_name.trim_end_matches(".disabled");
                mods_dir.join(base).exists() || mods_dir.join(format!("{base}.disabled")).exists()
            });
            if index.mods.len() != before {
                let _ = index.save(&mods_dir);
            }
        }

        Ok(())
    })
    .await
    .map_err(error)?
}

/// Enriquece `.minecraft/mods/.crab-mods-index.json` e o cache de ícones consultando
/// os hashes SHA-1 dos arquivos `.jar` instalados na API em lote do Modrinth.
/// Funciona tanto para modpacks do Modrinth quanto para arquivos importados localmente.
pub async fn enrich_installed_mods_metadata(
    client: &Client,
    mods_dir: &Path,
    cache_dir: &Path,
) -> usize {
    if !mods_dir.is_dir() {
        return 0;
    }

    let mods_dir_owned = mods_dir.to_path_buf();
    let hash_to_files: HashMap<String, Vec<String>> = tokio::task::spawn_blocking(move || {
        let mut map: HashMap<String, Vec<String>> = HashMap::new();
        let Ok(entries) = std::fs::read_dir(&mods_dir_owned) else {
            return map;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if !name.ends_with(".jar") && !name.ends_with(".jar.disabled") {
                continue;
            }
            if let Ok(mut f) = File::open(&path) {
                let mut hasher = Sha1::new();
                let mut buf = [0u8; 16384];
                let mut ok = true;
                loop {
                    match f.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => hasher.update(&buf[..n]),
                        Err(_) => {
                            ok = false;
                            break;
                        }
                    }
                }
                if ok {
                    let digest = format!("{:x}", hasher.finalize());
                    map.entry(digest).or_default().push(name.to_string());
                }
            }
        }
        map
    })
    .await
    .unwrap_or_default();

    if hash_to_files.is_empty() {
        return 0;
    }

    #[derive(Deserialize)]
    struct MrVersionMatch {
        id: String,
        project_id: String,
        #[serde(default)]
        version_number: String,
    }
    #[derive(Deserialize)]
    struct MrProjectSummary {
        id: String,
        #[serde(default)]
        slug: String,
        #[serde(default)]
        title: String,
        #[serde(default)]
        description: String,
        #[serde(default)]
        icon_url: Option<String>,
    }

    let hashes: Vec<String> = hash_to_files.keys().cloned().collect();
    let mut matched_versions: HashMap<String, MrVersionMatch> = HashMap::new();
    for chunk in hashes.chunks(100) {
        let body = serde_json::json!({
            "hashes": chunk,
            "algorithm": "sha1",
        });
        if let Ok(resp) = client
            .post(format!("{MODRINTH_API_BASE}/version_files"))
            .json(&body)
            .send()
            .await
        {
            if let Ok(ok_resp) = resp.error_for_status() {
                if let Ok(parsed) = ok_resp.json::<HashMap<String, MrVersionMatch>>().await {
                    matched_versions.extend(parsed);
                }
            }
        }
    }

    if matched_versions.is_empty() {
        return 0;
    }

    let mut project_ids: Vec<String> = matched_versions
        .values()
        .map(|v| v.project_id.clone())
        .collect();
    project_ids.sort();
    project_ids.dedup();

    let mut projects: HashMap<String, MrProjectSummary> = HashMap::new();
    for chunk in project_ids.chunks(100) {
        let Ok(ids_json) = serde_json::to_string(chunk) else {
            continue;
        };
        if let Ok(resp) = client
            .get(format!("{MODRINTH_API_BASE}/projects"))
            .query(&[("ids", ids_json)])
            .send()
            .await
        {
            if let Ok(ok_resp) = resp.error_for_status() {
                if let Ok(list) = ok_resp.json::<Vec<MrProjectSummary>>().await {
                    for proj in list {
                        projects.insert(proj.id.clone(), proj);
                    }
                }
            }
        }
    }

    // Baixa e armazena os ícones em cache em paralelo para carregamento instantâneo na lista de Mods.
    let icon_jobs: Vec<(String, String)> = projects
        .values()
        .filter_map(|p| {
            p.icon_url
                .as_deref()
                .filter(|u| !u.trim().is_empty())
                .map(|u| (p.id.clone(), u.to_string()))
        })
        .collect();
    let cache_dir_owned = cache_dir.to_path_buf();
    stream::iter(icon_jobs.into_iter().map(|(pid, url)| {
        let cache = cache_dir_owned.clone();
        async move {
            let _ =
                fetch_and_cache_mod_icon(client, &cache, ModProvider::Modrinth, &pid, &url).await;
        }
    }))
    .buffer_unordered(crate::services::download::DEFAULT_MAX_CONCURRENT_DOWNLOADS)
    .collect::<Vec<()>>()
    .await;

    let mut index = ModIndex::load(mods_dir);
    let mut updated = 0usize;
    for (hash, version) in &matched_versions {
        let Some(files) = hash_to_files.get(hash) else {
            continue;
        };
        let proj = projects.get(&version.project_id);
        let title = proj
            .map(|p| p.title.clone())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| version.project_id.clone());
        let description = proj.map(|p| p.description.clone()).unwrap_or_default();
        let homepage_url = proj
            .map(|p| {
                if p.slug.is_empty() {
                    format!("https://modrinth.com/mod/{}", p.id)
                } else {
                    format!("https://modrinth.com/mod/{}", p.slug)
                }
            })
            .unwrap_or_else(|| format!("https://modrinth.com/mod/{}", version.project_id));

        for file_name in files {
            index.upsert(InstalledModRecord {
                provider: ModProvider::Modrinth,
                project_id: version.project_id.clone(),
                version_id: version.id.clone(),
                version_number: version.version_number.clone(),
                file_name: file_name.clone(),
                title: title.clone(),
                homepage_url: homepage_url.clone(),
                author: String::new(),
                description: description.clone(),
                sha1: Some(hash.clone()),
                installed_at: Utc::now(),
            });
            updated += 1;
        }
    }

    if updated > 0 {
        let _ = index.save(mods_dir);
    }
    updated
}

pub async fn download_archive(client: &Client, url: &str, hash: &str, dest: &Path) -> Result<()> {
    download(client, url, ARCHIVE_LIMIT, Some(hash), dest).await
}

pub async fn download_icon(client: &Client, url: &str, dest: &Path) -> Result<()> {
    download(client, url, ICON_LIMIT, None, dest).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        core::version::VersionType, platform::paths::AppPaths,
        services::instance_manager::InstanceManager,
    };
    use std::io::Write;
    use zip::{write::SimpleFileOptions, ZipWriter};

    fn make_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let mut zip = ZipWriter::new(File::create(path).unwrap());
        for (name, contents) in entries {
            zip.start_file(*name, SimpleFileOptions::default()).unwrap();
            zip.write_all(contents).unwrap();
        }
        zip.finish().unwrap();
    }

    const INDEX: &[u8] = br#"{
        "name":"Test Pack","game":"minecraft","formatVersion":1,
        "dependencies":{"minecraft":"1.20.1","fabric-loader":"0.16.10"},"files":[]
    }"#;

    #[tokio::test]
    async fn modpack_import_persists_name_icon_and_client_overrides() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_root(tmp.path());
        let manager = InstanceManager::new(paths.clone());
        std::fs::create_dir_all(paths.instances_dir()).unwrap();
        let zip = tmp.path().join("test.mrpack");
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgba8(1, 1)
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        make_zip(
            &zip,
            &[
                ("modrinth.index.json", INDEX),
                ("icon.png", png.get_ref()),
                ("overrides/config/options.txt", b"base"),
                ("client-overrides/config/options.txt", b"client"),
                ("server-overrides/server.properties", b"server"),
            ],
        );
        let pack = inspect(&zip).unwrap();
        assert_eq!(pack.loader, Some((ModloaderKind::Fabric, "0.16.10".into())));
        let staging = tempfile::tempdir_in(paths.instances_dir()).unwrap();
        let source = staging.path().join("instance");
        let managed_files = stage(&Client::new(), &pack, zip, source.join(".minecraft"))
            .await
            .unwrap();
        assert!(managed_files.iter().any(|p| p == "config/options.txt"));
        let instance = manager
            .publish_import(
                &source,
                &pack.name,
                &pack.minecraft,
                VersionType::Release,
                None,
                None,
                None,
                pack.icon.as_deref(),
            )
            .await
            .unwrap();
        let stored = InstanceManager::new(paths.clone())
            .find_by_id(&instance.id)
            .await
            .unwrap();
        assert_eq!(stored.name, "Test Pack");
        let key = stored.icon_key.unwrap();
        assert!(paths.icons_dir().join(&key).is_file());
        let game = paths.instance_game_dir(&stored.dir_name);
        assert_eq!(
            std::fs::read(game.join("config/options.txt")).unwrap(),
            b"client"
        );
        assert!(!game.join("server.properties").exists());
        manager
            .rename_managed_icon(&key, "Renamed Pack Icon")
            .unwrap();
        assert!(manager
            .list_managed_icons_with_names()
            .iter()
            .any(|(id, name, _)| id == &key && name == "Renamed Pack Icon"));
        assert_eq!(manager.delete_managed_icon(&key).await.unwrap(), 1);
        assert!(manager
            .find_by_id(&instance.id)
            .await
            .unwrap()
            .icon_key
            .is_none());
        assert!(!paths.icons_dir().join(key).exists());
    }

    #[tokio::test]
    async fn modpack_rejects_archive_paths_outside_the_game_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let zip = tmp.path().join("bad.zip");
        make_zip(
            &zip,
            &[
                ("modrinth.index.json", INDEX),
                ("overrides/../outside.txt", b"outside"),
            ],
        );
        let pack = inspect(&zip).unwrap();
        assert!(stage(&Client::new(), &pack, zip, tmp.path().join("game"))
            .await
            .is_err());
        assert!(!tmp.path().join("outside.txt").exists());
    }

    #[tokio::test]
    async fn modpack_import_reads_nested_prism_instance() {
        let tmp = tempfile::tempdir().unwrap();
        let zip = tmp.path().join("prism.zip");
        make_zip(
            &zip,
            &[
                ("Export/instance.cfg", b"name=Prism Export"),
                (
                    "Export/mmc-pack.json",
                    br#"{"components":[{"uid":"net.minecraft","version":"1.20.1"}]}"#,
                ),
                ("Export/.minecraft/mods/example.jar", b"mod contents"),
            ],
        );
        let pack = inspect(&zip).unwrap();
        assert_eq!(pack.name, "Prism Export");
        let game = tmp.path().join("game");
        stage(&Client::new(), &pack, zip, game.clone())
            .await
            .unwrap();
        assert_eq!(
            std::fs::read(game.join("mods/example.jar")).unwrap(),
            b"mod contents"
        );
    }

    #[tokio::test]
    async fn sync_staged_update_replaces_obsolete_mods_and_preserves_user_saves() {
        let tmp = tempfile::tempdir().unwrap();
        let staged = tmp.path().join("staged");
        let target = tmp.path().join("target");
        std::fs::create_dir_all(target.join("mods")).unwrap();
        std::fs::create_dir_all(target.join("saves/MyWorld")).unwrap();
        std::fs::create_dir_all(staged.join("mods")).unwrap();

        std::fs::write(target.join("mods/old.jar"), b"old").unwrap();
        std::fs::write(target.join("saves/MyWorld/level.dat"), b"world").unwrap();
        std::fs::write(target.join("options.txt"), b"user-options").unwrap();
        std::fs::write(staged.join("mods/new.jar"), b"new").unwrap();
        std::fs::write(staged.join("options.txt"), b"pack-options").unwrap();

        sync_staged_update(
            staged,
            target.clone(),
            vec!["mods/old.jar".into(), "saves/MyWorld/level.dat".into()],
            vec!["mods/new.jar".into(), "options.txt".into()],
        )
        .await
        .unwrap();

        assert!(!target.join("mods/old.jar").exists());
        assert_eq!(std::fs::read(target.join("mods/new.jar")).unwrap(), b"new");
        assert_eq!(
            std::fs::read(target.join("saves/MyWorld/level.dat")).unwrap(),
            b"world"
        );
        assert_eq!(
            std::fs::read(target.join("options.txt")).unwrap(),
            b"user-options"
        );
    }
}
