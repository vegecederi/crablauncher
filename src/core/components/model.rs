use serde::{Deserialize, Serialize};
use std::fmt;

/// Tipo de modloader suportado pelo Crab.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModloaderKind {
    Fabric,
    Forge,
    NeoForge,
    Quilt,
    LiteLoader,
}

impl ModloaderKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Fabric => "Fabric Loader",
            Self::Forge => "Forge",
            Self::NeoForge => "NeoForge",
            Self::Quilt => "Quilt Loader",
            Self::LiteLoader => "LiteLoader",
        }
    }

    pub fn short_name(self) -> &'static str {
        match self {
            Self::Fabric => "Fabric",
            Self::Forge => "Forge",
            Self::NeoForge => "NeoForge",
            Self::Quilt => "Quilt",
            Self::LiteLoader => "LiteLoader",
        }
    }

    pub fn uid(self) -> &'static str {
        match self {
            Self::Fabric => "net.fabricmc.fabric-loader",
            Self::Forge => "net.minecraftforge",
            Self::NeoForge => "net.neoforged",
            Self::Quilt => "org.quiltmc.quilt-loader",
            Self::LiteLoader => "com.mumfrey.liteloader",
        }
    }

    pub fn from_index(idx: i32) -> Option<Self> {
        match idx {
            0 => Some(Self::Fabric),
            1 => Some(Self::Forge),
            2 => Some(Self::NeoForge),
            3 => Some(Self::Quilt),
            4 => Some(Self::LiteLoader),
            _ => None,
        }
    }
}

impl fmt::Display for ModloaderKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.short_name())
    }
}

/// Especificação persistida do modloader instalado em uma instância.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModloaderSpec {
    pub kind: ModloaderKind,
    pub version: String,
}

/// Tipo de componente na composição de uma instância.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComponentKind {
    Minecraft,
    Lwjgl,
    Modloader(ModloaderKind),
}

/// Componente real resolvido para exibição na página Version da instância.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstanceComponent {
    pub uid: String,
    pub name: String,
    pub version: String,
    pub kind: ComponentKind,
    pub required: bool,
    pub status: String,
}
