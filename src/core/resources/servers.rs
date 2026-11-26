use crate::core::resources::nbt::{read_nbt_file, write_nbt_file, NbtTag};
use crate::error::{LauncherError, Result};
use std::path::{Path, PathBuf};

/// Entrada de servidor multiplayer em `.minecraft/servers.dat`, preservando tags NBT desconhecidas.
#[derive(Debug, Clone)]
pub struct MinecraftServerEntry {
    pub name: String,
    pub ip: String,
    pub icon_base64: Option<String>,
    /// Tag Compound original para preservar campos desconhecidos (ex.: `acceptTextures`, `hidden`).
    raw_compound: NbtTag,
}

impl MinecraftServerEntry {
    pub fn new(name: impl Into<String>, ip: impl Into<String>) -> Self {
        let name = name.into();
        let ip = ip.into();
        let raw_compound = NbtTag::Compound(vec![
            ("name".into(), NbtTag::String(name.clone())),
            ("ip".into(), NbtTag::String(ip.clone())),
        ]);
        Self {
            name,
            ip,
            icon_base64: None,
            raw_compound,
        }
    }

    fn to_nbt(&self) -> NbtTag {
        let mut tag = self.raw_compound.clone();
        tag.set_compound_field("name", NbtTag::String(self.name.clone()));
        tag.set_compound_field("ip", NbtTag::String(self.ip.clone()));
        tag
    }
}

pub fn servers_dat_path(minecraft_dir: &Path) -> PathBuf {
    minecraft_dir.join("servers.dat")
}

/// Lê `.minecraft/servers.dat` (NBT não-comprimido) preservando metadados desconhecidos.
pub fn load_servers(minecraft_dir: &Path) -> Result<Vec<MinecraftServerEntry>> {
    let path = servers_dat_path(minecraft_dir);
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let (_, root) = read_nbt_file(&path)?;
    let Some(NbtTag::List(_, items)) = root.get("servers") else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for item in items {
        if item.as_compound().is_some() {
            let name = item
                .get("name")
                .and_then(|t| t.as_str())
                .unwrap_or("Minecraft Server")
                .to_string();
            let ip = item
                .get("ip")
                .and_then(|t| t.as_str())
                .unwrap_or("")
                .to_string();
            let icon_base64 = item
                .get("icon")
                .and_then(|t| t.as_str())
                .map(|s| s.to_string());
            out.push(MinecraftServerEntry {
                name,
                ip,
                icon_base64,
                raw_compound: item.clone(),
            });
        }
    }
    Ok(out)
}

/// Salva `.minecraft/servers.dat` de forma atômica, preservando campos desconhecidos do arquivo raiz e de cada servidor.
pub fn save_servers(
    minecraft_dir: &Path,
    servers: &[MinecraftServerEntry],
    instance_running: bool,
) -> Result<()> {
    if instance_running {
        return Err(LauncherError::InvalidMetadata(
            "Cannot modify servers.dat while the instance is running.".into(),
        ));
    }
    let path = servers_dat_path(minecraft_dir);
    let (root_name, mut root) = if path.is_file() {
        read_nbt_file(&path).unwrap_or_else(|_| (String::new(), NbtTag::Compound(Vec::new())))
    } else {
        (String::new(), NbtTag::Compound(Vec::new()))
    };

    let list_items: Vec<NbtTag> = servers.iter().map(|s| s.to_nbt()).collect();
    root.set_compound_field("servers", NbtTag::List(10, list_items));
    write_nbt_file(&path, &root_name, &root, false)
}
