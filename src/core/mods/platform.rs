use crate::core::components::model::ModloaderKind;
use crate::error::{LauncherError, Result};
use crate::services::download::{DownloadManager, DownloadRequest};
use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use tokio_util::sync::CancellationToken;

pub const MODRINTH_API_BASE: &str = "https://api.modrinth.com/v2";
pub const CURSEFORGE_API_BASE: &str = "https://api.curseforge.com/v1";
pub const CRAB_USER_AGENT: &str = concat!(
    "CrabLauncher/",
    env!("CARGO_PKG_VERSION"),
    " (https://crablauncher.org)"
);

/// Tipo de conteúdo gerenciado pelo downloader unificado (Mods, Resource Packs, Shaders).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContentKind {
    Mod,
    ResourcePack,
    ShaderPack,
}

impl ContentKind {
    pub fn from_id(id: &str) -> Self {
        match id {
            "resourcepacks" | "resourcepack" => Self::ResourcePack,
            "shaderpacks" | "shader" => Self::ShaderPack,
            _ => Self::Mod,
        }
    }

    pub fn as_id(self) -> &'static str {
        match self {
            Self::Mod => "mods",
            Self::ResourcePack => "resourcepacks",
            Self::ShaderPack => "shaderpacks",
        }
    }

    pub fn window_title(self) -> &'static str {
        match self {
            Self::Mod => "Download Mods",
            Self::ResourcePack => "Download Resource Packs",
            Self::ShaderPack => "Download Shaders",
        }
    }

    pub fn modrinth_project_type(self) -> &'static str {
        match self {
            Self::Mod => "mod",
            Self::ResourcePack => "resourcepack",
            Self::ShaderPack => "shader",
        }
    }

    pub fn modrinth_url_segment(self) -> &'static str {
        match self {
            Self::Mod => "mod",
            Self::ResourcePack => "resourcepack",
            Self::ShaderPack => "shader",
        }
    }

    /// CurseForge classId: 6=Mods, 12=Resource Packs, 6552=Shaders.
    pub fn curseforge_class_id(self) -> &'static str {
        match self {
            Self::Mod => "6",
            Self::ResourcePack => "12",
            Self::ShaderPack => "6552",
        }
    }

    pub fn curseforge_url_segment(self) -> &'static str {
        match self {
            Self::Mod => "mc-mods",
            Self::ResourcePack => "texture-packs",
            Self::ShaderPack => "shaders",
        }
    }

    /// Filtros de modloader (Fabric/Forge/NeoForge) só se aplicam a mods.
    pub fn uses_modloader_filter(self) -> bool {
        matches!(self, Self::Mod)
    }
}

/// Filtros de pesquisa do Content Downloader (Categories, Modloaders, Versions, Environments, Hide Installed).
#[derive(Debug, Clone, Default)]
pub struct SearchFilters {
    pub categories: Vec<String>,
    pub loaders: Vec<String>,
    pub game_version: Option<String>,
    pub client_side: bool,
    pub server_side: bool,
    pub hide_installed: bool,
}

/// Provedor de origem de um mod.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModProvider {
    Modrinth,
    CurseForge,
    Local,
}

impl ModProvider {
    pub fn label(self) -> &'static str {
        match self {
            Self::Modrinth => "Modrinth",
            Self::CurseForge => "CurseForge",
            Self::Local => "Local",
        }
    }
}

/// Ordenação suportada na busca de mods.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModSortBy {
    Relevance,
    Downloads,
    Updated,
    Newest,
}

impl ModSortBy {
    pub fn from_id(id: &str) -> Self {
        match id {
            "downloads" => Self::Downloads,
            "updated" => Self::Updated,
            "newest" => Self::Newest,
            _ => Self::Relevance,
        }
    }

    pub fn modrinth_index(self) -> &'static str {
        match self {
            Self::Relevance => "relevance",
            Self::Downloads => "downloads",
            Self::Updated => "updated",
            Self::Newest => "newest",
        }
    }

    /// CurseForge sortField: 1=Featured, 2=Popularity, 3=LastUpdated, 6=TotalDownloads, 11=ReleasedDate
    pub fn curseforge_sort_field(self) -> u32 {
        match self {
            Self::Relevance => 2,
            Self::Downloads => 6,
            Self::Updated => 3,
            Self::Newest => 11,
        }
    }
}

/// Resultado resumido de busca de mod no Modrinth ou CurseForge.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModSearchResult {
    pub provider: ModProvider,
    pub project_id: String,
    pub slug: String,
    pub title: String,
    pub description: String,
    pub author: String,
    pub downloads: u64,
    pub icon_url: Option<String>,
    pub homepage_url: String,
    pub installed: bool,
}

/// Dependência obrigatória de uma versão de mod.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModDependency {
    pub provider: ModProvider,
    pub project_id: String,
    pub version_id: Option<String>,
}

/// Versão remota baixável de um mod.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModRemoteVersion {
    pub provider: ModProvider,
    pub project_id: String,
    pub version_id: String,
    pub version_number: String,
    pub name: String,
    pub file_name: String,
    pub download_url: String,
    pub sha1: Option<String>,
    pub file_size: Option<u64>,
    pub game_versions: Vec<String>,
    pub loaders: Vec<String>,
    pub required_dependencies: Vec<ModDependency>,
}

/// Metadados persistidos de um mod instalado para permitir Check for Updates, Change Version e View Homepage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledModRecord {
    pub provider: ModProvider,
    pub project_id: String,
    pub version_id: String,
    pub version_number: String,
    pub file_name: String,
    pub title: String,
    pub homepage_url: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub description: String,
    pub sha1: Option<String>,
    pub installed_at: DateTime<Utc>,
}

/// Índice local de metadados de mods em `.minecraft/mods/.crab-mods-index.json`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModIndex {
    pub mods: Vec<InstalledModRecord>,
}

impl ModIndex {
    pub fn path_for(mods_dir: &Path) -> PathBuf {
        mods_dir.join(".crab-mods-index.json")
    }

    pub fn load(mods_dir: &Path) -> Self {
        let path = Self::path_for(mods_dir);
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, mods_dir: &Path) -> Result<()> {
        std::fs::create_dir_all(mods_dir).map_err(|source| LauncherError::Io {
            path: mods_dir.to_path_buf(),
            source,
        })?;
        let path = Self::path_for(mods_dir);
        let tmp = mods_dir.join(".crab-mods-index.json.tmp");
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(&tmp, json).map_err(|source| LauncherError::Io {
            path: tmp.clone(),
            source,
        })?;
        std::fs::rename(&tmp, &path).map_err(|source| LauncherError::Io { path, source })
    }

    pub fn upsert(&mut self, record: InstalledModRecord) {
        if let Some(existing) = self.mods.iter_mut().find(|m| {
            m.file_name == record.file_name
                || (m.provider == record.provider && m.project_id == record.project_id)
        }) {
            *existing = record;
        } else {
            self.mods.push(record);
        }
    }

    pub fn remove_by_file(&mut self, file_name: &str) {
        let base = file_name.trim_end_matches(".disabled");
        self.mods
            .retain(|m| m.file_name.trim_end_matches(".disabled") != base);
    }

    pub fn find_by_file(&self, file_name: &str) -> Option<&InstalledModRecord> {
        let base = file_name.trim_end_matches(".disabled");
        self.mods
            .iter()
            .find(|m| m.file_name.trim_end_matches(".disabled") == base)
    }

    pub fn installed_project_ids(&self, provider: ModProvider) -> HashSet<String> {
        self.mods
            .iter()
            .filter(|m| m.provider == provider)
            .map(|m| m.project_id.clone())
            .collect()
    }

    /// Retorna apenas os `project_id`s cujos arquivos `.jar` (ou `.jar.disabled`) existem de fato
    /// no diretório `mods/` da instância atual, eliminando falsos positivos de `[installed]`.
    pub fn installed_project_ids_verified(
        &self,
        mods_dir: &Path,
        provider: ModProvider,
    ) -> HashSet<String> {
        self.mods
            .iter()
            .filter(|m| m.provider == provider)
            .filter(|m| {
                let base = m.file_name.trim_end_matches(".disabled");
                mods_dir.join(base).exists() || mods_dir.join(format!("{base}.disabled")).exists()
            })
            .map(|m| m.project_id.clone())
            .collect()
    }
}

/// Retorna a chave da API do CurseForge a partir da variável de ambiente `CRAB_CURSEFORGE_API_KEY`.
/// Nenhuma chave privada é embutida no repositório.
pub fn curseforge_api_key() -> Option<String> {
    std::env::var("CRAB_CURSEFORGE_API_KEY")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn modrinth_loader_slug(loader: Option<ModloaderKind>) -> Option<&'static str> {
    match loader {
        Some(ModloaderKind::Fabric) => Some("fabric"),
        Some(ModloaderKind::Forge) => Some("forge"),
        Some(ModloaderKind::NeoForge) => Some("neoforge"),
        Some(ModloaderKind::Quilt) => Some("quilt"),
        Some(ModloaderKind::LiteLoader) => Some("liteloader"),
        None => None,
    }
}

fn curseforge_loader_type(loader: Option<ModloaderKind>) -> Option<u32> {
    match loader {
        Some(ModloaderKind::Forge) => Some(1),
        Some(ModloaderKind::LiteLoader) => Some(3),
        Some(ModloaderKind::Fabric) => Some(4),
        Some(ModloaderKind::Quilt) => Some(5),
        Some(ModloaderKind::NeoForge) => Some(6),
        None => None,
    }
}

/// Busca conteúdo na API oficial Modrinth v2 com filtros avançados (`SearchFilters`) e paginação (`offset`).
#[allow(clippy::too_many_arguments)]
pub async fn search_modrinth_filtered_paged(
    client: &Client,
    kind: ContentKind,
    query: &str,
    filters: &SearchFilters,
    sort: ModSortBy,
    offset: usize,
    installed_ids: &HashSet<String>,
) -> Result<(Vec<ModSearchResult>, usize)> {
    let mut facets: Vec<Vec<String>> = vec![vec![format!(
        "project_type:{}",
        kind.modrinth_project_type()
    )]];
    if let Some(mc_version) = filters.game_version.as_deref() {
        if !mc_version.is_empty() {
            facets.push(vec![format!("versions:{mc_version}")]);
        }
    }
    if kind.uses_modloader_filter() && !filters.loaders.is_empty() {
        facets.push(
            filters
                .loaders
                .iter()
                .map(|l| format!("categories:{}", l.to_lowercase()))
                .collect(),
        );
    }
    if !filters.categories.is_empty() {
        facets.push(
            filters
                .categories
                .iter()
                .map(|c| format!("categories:{}", c.to_lowercase()))
                .collect(),
        );
    }
    if filters.client_side {
        facets.push(vec![
            "client_side:required".to_string(),
            "client_side:optional".to_string(),
        ]);
    }
    if filters.server_side {
        facets.push(vec![
            "server_side:required".to_string(),
            "server_side:optional".to_string(),
        ]);
    }
    let facets_json = serde_json::to_string(&facets)?;
    let offset_str = offset.to_string();

    let resp = client
        .get(format!("{MODRINTH_API_BASE}/search"))
        .header(reqwest::header::USER_AGENT, CRAB_USER_AGENT)
        .query(&[
            ("query", query),
            ("facets", facets_json.as_str()),
            ("index", sort.modrinth_index()),
            ("offset", offset_str.as_str()),
            ("limit", "25"),
        ])
        .send()
        .await?
        .error_for_status()?;

    #[derive(Deserialize)]
    struct MrSearchResp {
        hits: Vec<MrHit>,
        #[serde(default)]
        total_hits: usize,
    }
    #[derive(Deserialize)]
    struct MrHit {
        project_id: String,
        slug: String,
        title: String,
        description: String,
        author: String,
        downloads: u64,
        icon_url: Option<String>,
    }

    let parsed: MrSearchResp = resp.json().await?;
    let total = parsed.total_hits;
    let url_seg = kind.modrinth_url_segment();
    let mut items: Vec<ModSearchResult> = parsed
        .hits
        .into_iter()
        .map(|h| {
            let installed = installed_ids.contains(&h.project_id);
            ModSearchResult {
                provider: ModProvider::Modrinth,
                homepage_url: format!("https://modrinth.com/{url_seg}/{}", h.slug),
                project_id: h.project_id,
                slug: h.slug,
                title: h.title,
                description: h.description,
                author: h.author,
                downloads: h.downloads,
                icon_url: h.icon_url,
                installed,
            }
        })
        .collect();
    if filters.hide_installed {
        items.retain(|item| !item.installed);
    }
    Ok((items, total))
}

/// Busca conteúdo (Mods, Resource Packs ou Shaders) na API oficial Modrinth v2 com paginação (`offset`).
#[allow(clippy::too_many_arguments)]
pub async fn search_modrinth_content_paged(
    client: &Client,
    kind: ContentKind,
    query: &str,
    mc_version: &str,
    loader: Option<ModloaderKind>,
    sort: ModSortBy,
    offset: usize,
    installed_ids: &HashSet<String>,
) -> Result<(Vec<ModSearchResult>, usize)> {
    let filters = SearchFilters {
        categories: Vec::new(),
        loaders: modrinth_loader_slug(loader)
            .map(|s| vec![s.to_string()])
            .unwrap_or_default(),
        game_version: if mc_version.is_empty() {
            None
        } else {
            Some(mc_version.to_string())
        },
        client_side: false,
        server_side: false,
        hide_installed: false,
    };
    search_modrinth_filtered_paged(client, kind, query, &filters, sort, offset, installed_ids).await
}

/// Busca mods na API oficial Modrinth v2 com suporte a paginação (`offset`), retornando `(hits, total_hits)`.
pub async fn search_modrinth_paged(
    client: &Client,
    query: &str,
    mc_version: &str,
    loader: Option<ModloaderKind>,
    sort: ModSortBy,
    offset: usize,
    installed_ids: &HashSet<String>,
) -> Result<(Vec<ModSearchResult>, usize)> {
    search_modrinth_content_paged(
        client,
        ContentKind::Mod,
        query,
        mc_version,
        loader,
        sort,
        offset,
        installed_ids,
    )
    .await
}

/// Busca mods na API oficial Modrinth v2 filtrando automaticamente por versão do Minecraft e modloader.
pub async fn search_modrinth(
    client: &Client,
    query: &str,
    mc_version: &str,
    loader: Option<ModloaderKind>,
    sort: ModSortBy,
    installed_ids: &HashSet<String>,
) -> Result<Vec<ModSearchResult>> {
    search_modrinth_paged(client, query, mc_version, loader, sort, 0, installed_ids)
        .await
        .map(|(hits, _)| hits)
}

/// Busca versões compatíveis de um projeto no Modrinth v2.
pub async fn fetch_modrinth_versions(
    client: &Client,
    project_id: &str,
    mc_version: &str,
    loader: Option<ModloaderKind>,
) -> Result<Vec<ModRemoteVersion>> {
    let mut params: Vec<(&str, String)> = Vec::new();
    if !mc_version.is_empty() {
        params.push(("game_versions", serde_json::to_string(&[mc_version])?));
    }
    if let Some(l) = modrinth_loader_slug(loader) {
        params.push(("loaders", serde_json::to_string(&[l])?));
    }

    let resp = client
        .get(format!("{MODRINTH_API_BASE}/project/{project_id}/version"))
        .header(reqwest::header::USER_AGENT, CRAB_USER_AGENT)
        .query(&params)
        .send()
        .await?
        .error_for_status()?;

    #[derive(Deserialize)]
    struct MrVersion {
        id: String,
        project_id: String,
        name: String,
        version_number: String,
        game_versions: Vec<String>,
        loaders: Vec<String>,
        files: Vec<MrFile>,
        #[serde(default)]
        dependencies: Vec<MrDep>,
    }
    #[derive(Deserialize)]
    struct MrFile {
        url: String,
        filename: String,
        primary: bool,
        size: u64,
        #[serde(default)]
        hashes: HashMap<String, String>,
    }
    #[derive(Deserialize)]
    struct MrDep {
        project_id: Option<String>,
        version_id: Option<String>,
        dependency_type: String,
    }

    let raw: Vec<MrVersion> = resp.json().await?;
    let mut out = Vec::new();
    for v in raw {
        let Some(file) = v
            .files
            .iter()
            .find(|f| f.primary)
            .or_else(|| v.files.first())
        else {
            continue;
        };
        let required_dependencies = v
            .dependencies
            .into_iter()
            .filter(|d| d.dependency_type == "required")
            .filter_map(|d| {
                d.project_id.map(|pid| ModDependency {
                    provider: ModProvider::Modrinth,
                    project_id: pid,
                    version_id: d.version_id,
                })
            })
            .collect();
        out.push(ModRemoteVersion {
            provider: ModProvider::Modrinth,
            project_id: v.project_id,
            version_id: v.id,
            version_number: v.version_number,
            name: v.name,
            file_name: file.filename.clone(),
            download_url: file.url.clone(),
            sha1: file.hashes.get("sha1").cloned(),
            file_size: Some(file.size),
            game_versions: v.game_versions,
            loaders: v.loaders,
            required_dependencies,
        });
    }
    Ok(out)
}

/// Busca conteúdo (Mods, Resource Packs ou Shaders) na API oficial CurseForge Core v1.
pub async fn search_curseforge_content(
    client: &Client,
    kind: ContentKind,
    query: &str,
    mc_version: &str,
    loader: Option<ModloaderKind>,
    sort: ModSortBy,
    installed_ids: &HashSet<String>,
) -> Result<Vec<ModSearchResult>> {
    let api_key = curseforge_api_key().ok_or_else(|| {
        LauncherError::InvalidMetadata(
            "CurseForge requires an official API key in the CRAB_CURSEFORGE_API_KEY environment variable."
                .into(),
        )
    })?;

    let mut params: Vec<(&str, String)> = vec![
        ("gameId", "432".into()),
        ("classId", kind.curseforge_class_id().into()),
        ("sortField", sort.curseforge_sort_field().to_string()),
        ("sortOrder", "desc".into()),
        ("pageSize", "25".into()),
    ];
    if !query.trim().is_empty() {
        params.push(("searchFilter", query.to_string()));
    }
    if !mc_version.is_empty() {
        params.push(("gameVersion", mc_version.to_string()));
    }
    if kind.uses_modloader_filter() {
        if let Some(lt) = curseforge_loader_type(loader) {
            params.push(("modLoaderType", lt.to_string()));
        }
    }

    let resp = client
        .get(format!("{CURSEFORGE_API_BASE}/mods/search"))
        .header("x-api-key", &api_key)
        .header(reqwest::header::USER_AGENT, CRAB_USER_AGENT)
        .query(&params)
        .send()
        .await?
        .error_for_status()?;

    #[derive(Deserialize)]
    struct CfSearchResp {
        data: Vec<CfMod>,
    }
    #[derive(Deserialize)]
    struct CfMod {
        id: u64,
        slug: String,
        name: String,
        summary: String,
        #[serde(rename = "downloadCount")]
        download_count: u64,
        #[serde(default)]
        authors: Vec<CfAuthor>,
        logo: Option<CfLogo>,
        links: Option<CfLinks>,
    }
    #[derive(Deserialize)]
    struct CfAuthor {
        name: String,
    }
    #[derive(Deserialize)]
    struct CfLogo {
        url: Option<String>,
    }
    #[derive(Deserialize)]
    struct CfLinks {
        #[serde(rename = "websiteUrl")]
        website_url: Option<String>,
    }

    let parsed: CfSearchResp = resp.json().await?;
    let cf_seg = kind.curseforge_url_segment();
    Ok(parsed
        .data
        .into_iter()
        .map(|m| {
            let pid = m.id.to_string();
            let installed = installed_ids.contains(&pid);
            let author = m
                .authors
                .first()
                .map(|a| a.name.clone())
                .unwrap_or_default();
            let homepage_url = m.links.and_then(|l| l.website_url).unwrap_or_else(|| {
                format!("https://www.curseforge.com/minecraft/{cf_seg}/{}", m.slug)
            });
            ModSearchResult {
                provider: ModProvider::CurseForge,
                project_id: pid,
                slug: m.slug,
                title: m.name,
                description: m.summary,
                author,
                downloads: m.download_count,
                icon_url: m.logo.and_then(|l| l.url),
                homepage_url,
                installed,
            }
        })
        .collect())
}

/// Busca mods na API oficial CurseForge Core v1 (exige `CRAB_CURSEFORGE_API_KEY`).
pub async fn search_curseforge(
    client: &Client,
    query: &str,
    mc_version: &str,
    loader: Option<ModloaderKind>,
    sort: ModSortBy,
    installed_ids: &HashSet<String>,
) -> Result<Vec<ModSearchResult>> {
    search_curseforge_content(
        client,
        ContentKind::Mod,
        query,
        mc_version,
        loader,
        sort,
        installed_ids,
    )
    .await
}

/// Busca versões/arquivos compatíveis de um mod no CurseForge Core v1.
pub async fn fetch_curseforge_versions(
    client: &Client,
    project_id: &str,
    mc_version: &str,
    loader: Option<ModloaderKind>,
) -> Result<Vec<ModRemoteVersion>> {
    let api_key = curseforge_api_key().ok_or_else(|| {
        LauncherError::InvalidMetadata(
            "CurseForge requires an official API key in the CRAB_CURSEFORGE_API_KEY environment variable."
                .into(),
        )
    })?;

    let mut params: Vec<(&str, String)> = vec![("pageSize", "25".into())];
    if !mc_version.is_empty() {
        params.push(("gameVersion", mc_version.to_string()));
    }
    if let Some(lt) = curseforge_loader_type(loader) {
        params.push(("modLoaderType", lt.to_string()));
    }

    let resp = client
        .get(format!("{CURSEFORGE_API_BASE}/mods/{project_id}/files"))
        .header("x-api-key", &api_key)
        .header(reqwest::header::USER_AGENT, CRAB_USER_AGENT)
        .query(&params)
        .send()
        .await?
        .error_for_status()?;

    #[derive(Deserialize)]
    struct CfFilesResp {
        data: Vec<CfFile>,
    }
    #[derive(Deserialize)]
    struct CfFile {
        id: u64,
        #[serde(rename = "displayName")]
        display_name: String,
        #[serde(rename = "fileName")]
        file_name: String,
        #[serde(rename = "downloadUrl")]
        download_url: Option<String>,
        #[serde(rename = "fileLength")]
        file_length: u64,
        #[serde(rename = "gameVersions", default)]
        game_versions: Vec<String>,
        #[serde(default)]
        hashes: Vec<CfHash>,
        #[serde(default)]
        dependencies: Vec<CfDep>,
    }
    #[derive(Deserialize)]
    struct CfHash {
        value: String,
        algo: u32, // 1 = SHA1
    }
    #[derive(Deserialize)]
    struct CfDep {
        #[serde(rename = "modId")]
        mod_id: u64,
        #[serde(rename = "relationType")]
        relation_type: u32, // 3 = RequiredDependency
    }

    let parsed: CfFilesResp = resp.json().await?;
    let mut out = Vec::new();
    for f in parsed.data {
        let Some(url) = f.download_url else {
            continue;
        };
        let sha1 = f.hashes.into_iter().find(|h| h.algo == 1).map(|h| h.value);
        let required_dependencies = f
            .dependencies
            .into_iter()
            .filter(|d| d.relation_type == 3)
            .map(|d| ModDependency {
                provider: ModProvider::CurseForge,
                project_id: d.mod_id.to_string(),
                version_id: None,
            })
            .collect();
        out.push(ModRemoteVersion {
            provider: ModProvider::CurseForge,
            project_id: project_id.to_string(),
            version_id: f.id.to_string(),
            version_number: f.display_name.clone(),
            name: f.display_name,
            file_name: f.file_name,
            download_url: url,
            sha1,
            file_size: Some(f.file_length),
            game_versions: f.game_versions,
            loaders: Vec::new(),
            required_dependencies,
        });
    }
    Ok(out)
}

/// Resolve dependências obrigatórias (1 nível + deduplicação) que ainda não estejam instaladas.
pub async fn resolve_required_dependencies(
    client: &Client,
    selected: &[ModRemoteVersion],
    mc_version: &str,
    loader: Option<ModloaderKind>,
    already_installed: &HashSet<String>,
) -> Vec<ModRemoteVersion> {
    let mut seen: HashSet<String> = already_installed.clone();
    for v in selected {
        seen.insert(v.project_id.clone());
    }
    let mut resolved = Vec::new();
    for v in selected {
        for dep in &v.required_dependencies {
            if seen.contains(&dep.project_id) {
                continue;
            }
            seen.insert(dep.project_id.clone());
            let versions = match dep.provider {
                ModProvider::Modrinth => {
                    fetch_modrinth_versions(client, &dep.project_id, mc_version, loader)
                        .await
                        .ok()
                }
                ModProvider::CurseForge => {
                    fetch_curseforge_versions(client, &dep.project_id, mc_version, loader)
                        .await
                        .ok()
                }
                ModProvider::Local => None,
            };
            if let Some(list) = versions {
                if let Some(best) = dep
                    .version_id
                    .as_deref()
                    .and_then(|vid| list.iter().find(|item| item.version_id == vid))
                    .or_else(|| list.first())
                {
                    resolved.push(best.clone());
                }
            }
        }
    }
    resolved
}

/// Baixa um conteúdo com reporte real de bytes baixados e grava seus metadados no índice local.
#[allow(clippy::too_many_arguments)]
pub async fn download_and_record_content_with_progress<F>(
    download_manager: &DownloadManager,
    target_dir: &Path,
    version: &ModRemoteVersion,
    title: &str,
    homepage_url: &str,
    author: &str,
    description: &str,
    cancel: &CancellationToken,
    on_bytes: F,
) -> Result<PathBuf>
where
    F: Fn(u64) + Send + Sync,
{
    let dest = target_dir.join(&version.file_name);
    let req = DownloadRequest {
        url: version.download_url.clone(),
        destination: dest.clone(),
        expected_sha1: version.sha1.clone(),
        expected_size: version.file_size,
    };
    download_manager
        .download_file(&req, cancel, on_bytes)
        .await?;

    let mut index = ModIndex::load(target_dir);
    index.upsert(InstalledModRecord {
        provider: version.provider,
        project_id: version.project_id.clone(),
        version_id: version.version_id.clone(),
        version_number: version.version_number.clone(),
        file_name: version.file_name.clone(),
        title: title.to_string(),
        homepage_url: homepage_url.to_string(),
        author: author.to_string(),
        description: description.to_string(),
        sha1: version.sha1.clone(),
        installed_at: Utc::now(),
    });
    index.save(target_dir)?;
    Ok(dest)
}

/// Baixa um conteúdo (mod, resource pack ou shader pack) e grava seus metadados no índice local.
#[allow(clippy::too_many_arguments)]
pub async fn download_and_record_content(
    download_manager: &DownloadManager,
    target_dir: &Path,
    version: &ModRemoteVersion,
    title: &str,
    homepage_url: &str,
    author: &str,
    description: &str,
    cancel: &CancellationToken,
) -> Result<PathBuf> {
    download_and_record_content_with_progress(
        download_manager,
        target_dir,
        version,
        title,
        homepage_url,
        author,
        description,
        cancel,
        |_| {},
    )
    .await
}

/// Baixa um mod usando o `DownloadManager` existente e grava seus metadados em `.crab-mods-index.json`.
pub async fn download_and_record_mod(
    download_manager: &DownloadManager,
    mods_dir: &Path,
    version: &ModRemoteVersion,
    title: &str,
    homepage_url: &str,
    cancel: &CancellationToken,
) -> Result<PathBuf> {
    download_and_record_content(
        download_manager,
        mods_dir,
        version,
        title,
        homepage_url,
        "",
        "",
        cancel,
    )
    .await
}

/// Detalhes completos de um projeto (Markdown body + links externos).
#[derive(Debug, Clone, Default)]
pub struct ModProjectDetails {
    pub body_markdown: String,
    pub issues_url: Option<String>,
    pub source_url: Option<String>,
    pub discord_url: Option<String>,
    pub wiki_url: Option<String>,
}

/// Busca a descrição completa em Markdown e links externos de um projeto no Modrinth v2.
pub async fn fetch_modrinth_project_details(
    client: &Client,
    project_id: &str,
) -> Result<ModProjectDetails> {
    let resp = client
        .get(format!("{MODRINTH_API_BASE}/project/{project_id}"))
        .header(reqwest::header::USER_AGENT, CRAB_USER_AGENT)
        .send()
        .await?
        .error_for_status()?;

    #[derive(Deserialize)]
    struct MrProject {
        #[serde(default)]
        body: String,
        issues_url: Option<String>,
        source_url: Option<String>,
        discord_url: Option<String>,
        wiki_url: Option<String>,
    }

    let p: MrProject = resp.json().await?;
    Ok(ModProjectDetails {
        body_markdown: p.body,
        issues_url: p.issues_url.filter(|s| !s.trim().is_empty()),
        source_url: p.source_url.filter(|s| !s.trim().is_empty()),
        discord_url: p.discord_url.filter(|s| !s.trim().is_empty()),
        wiki_url: p.wiki_url.filter(|s| !s.trim().is_empty()),
    })
}

/// Baixa, decodifica (PNG, JPEG, WebP, GIF) e armazena em cache local como `.png` canônico
/// para garantir carregamento 100% confiável no Slint.
pub async fn fetch_and_cache_mod_icon(
    client: &Client,
    cache_dir: &Path,
    provider: ModProvider,
    project_id: &str,
    icon_url: &str,
) -> Option<PathBuf> {
    let safe_id: String = project_id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let key = format!("{}-{}", provider.label().to_lowercase(), safe_id);
    fetch_and_cache_remote_image(client, &cache_dir.join("mod-icons"), &key, icon_url).await
}

/// Baixa qualquer imagem remota (ícone ou banner de descrição), decodifica via `image` crate
/// (suportando WebP/JPEG/PNG/GIF) e salva como `.png` em cache local.
pub async fn fetch_and_cache_remote_image(
    client: &Client,
    dir: &Path,
    cache_key: &str,
    url: &str,
) -> Option<PathBuf> {
    if url.trim().is_empty() {
        return None;
    }
    let _ = tokio::fs::create_dir_all(dir).await;
    let dest = dir.join(format!("{cache_key}.png"));
    if dest.is_file() {
        return Some(dest);
    }
    let resp = client
        .get(url)
        .header(reqwest::header::USER_AGENT, CRAB_USER_AGENT)
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let bytes = resp.bytes().await.ok()?;
    if bytes.is_empty() {
        return None;
    }
    let dest_clone = dest.clone();
    tokio::task::spawn_blocking(move || {
        let img = image::load_from_memory(&bytes).ok()?;
        let tmp = dest_clone.with_extension("png.tmp");
        img.save_with_format(&tmp, image::ImageFormat::Png).ok()?;
        std::fs::rename(&tmp, &dest_clone).ok()?;
        Some(dest_clone)
    })
    .await
    .ok()
    .flatten()
}

/// Converte Markdown de projeto em texto limpo estruturado para exibição no painel de detalhes.
pub fn render_markdown_plain(md: &str) -> String {
    let mut lines = Vec::new();
    for raw in md.lines() {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            lines.push(String::new());
            continue;
        }
        // Ignora tags HTML puras de layout/badges mantendo legibilidade
        if trimmed.starts_with('<') && trimmed.ends_with('>') {
            continue;
        }
        // Remove imagens markdown `![alt](url)`
        let cleaned = if trimmed.starts_with("![") {
            continue;
        } else if let Some(h) = trimmed.strip_prefix("### ") {
            format!("▸ {h}")
        } else if let Some(h) = trimmed.strip_prefix("## ") {
            h.to_uppercase()
        } else if let Some(h) = trimmed.strip_prefix("# ") {
            h.to_uppercase()
        } else if let Some(b) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
        {
            format!("• {b}")
        } else {
            trimmed.to_string()
        };
        let cleaned = cleaned.replace("**", "").replace("__", "").replace('`', "");
        lines.push(cleaned);
    }
    lines.join("\n")
}

/// Exporta a lista de mods instalados em formato Markdown legível.
pub fn export_mods_list_markdown(
    instance_name: &str,
    mods: &[(String, String, String, bool)], // (name, version, provider, enabled)
) -> String {
    let mut out = format!("# Installed Mods — {instance_name}\n\n");
    out.push_str("| Status | Name | Version | Provider |\n| :--- | :--- | :--- | :--- |\n");
    for (name, ver, prov, enabled) in mods {
        let status = if *enabled { "Enabled" } else { "Disabled" };
        out.push_str(&format!("| {status} | {name} | {ver} | {prov} |\n"));
    }
    out
}
