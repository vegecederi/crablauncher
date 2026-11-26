use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Tipo de versão do Minecraft no manifesto oficial.
/// Mantém variantes futuras (old_beta, old_alpha) para evolução sem quebra de compatibilidade.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VersionType {
    Release,
    Snapshot,
    OldBeta,
    OldAlpha,
    #[serde(other)]
    Unknown,
}

impl VersionType {
    /// Indica se o tipo é suportado na interface nesta fase.
    pub fn is_supported_phase1(self) -> bool {
        matches!(self, Self::Release | Self::Snapshot)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Release => "Release",
            Self::Snapshot => "Snapshot",
            Self::OldBeta => "Beta",
            Self::OldAlpha => "Alpha",
            Self::Unknown => "Unknown",
        }
    }
}

impl fmt::Display for VersionType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Entrada resumida de uma versão do Minecraft pronta para exibição e filtragem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MinecraftVersion {
    pub id: String,
    pub version_type: VersionType,
    pub release_time: DateTime<Utc>,
    pub url: String,
}

impl MinecraftVersion {
    /// Data de lançamento formatada de forma compacta (YYYY-MM-DD).
    pub fn formatted_release_date(&self) -> String {
        self.release_time.format("%Y-%m-%d").to_string()
    }
}
