pub mod adoptium;
pub mod azul;
pub mod mojang;
pub mod semeru;

use crate::error::Result;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Provedor oficial de runtimes Java suportado pelo Crab.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JavaProviderKind {
    #[default]
    Mojang,
    Adoptium,
    Azul,
    Semeru,
}

impl JavaProviderKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Mojang => "Mojang",
            Self::Adoptium => "Adoptium",
            Self::Azul => "Azul Zulu",
            Self::Semeru => "IBM Semeru Open",
        }
    }

    pub fn dir_name(self) -> &'static str {
        match self {
            Self::Mojang => "mojang",
            Self::Adoptium => "adoptium",
            Self::Azul => "azul",
            Self::Semeru => "semeru",
        }
    }

    pub fn from_index(idx: i32) -> Self {
        match idx {
            0 => Self::Mojang,
            1 => Self::Adoptium,
            2 => Self::Azul,
            3 => Self::Semeru,
            _ => Self::Mojang,
        }
    }

    pub fn as_index(self) -> i32 {
        match self {
            Self::Mojang => 0,
            Self::Adoptium => 1,
            Self::Azul => 2,
            Self::Semeru => 3,
        }
    }
}

impl fmt::Display for JavaProviderKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JavaPackageType {
    #[default]
    Jre,
    Jdk,
}

impl JavaPackageType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Jre => "JRE",
            Self::Jdk => "JDK",
        }
    }
}

/// Modelo normalizado de uma build Java disponível para download em qualquer provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AvailableJavaBuild {
    pub provider: JavaProviderKind,
    pub distribution: String,
    pub component: Option<String>,
    pub major_version: u32,
    pub full_version: String,
    pub build_number: Option<String>,
    pub release_date: Option<String>,
    pub arch: String,
    pub os: String,
    pub package_type: JavaPackageType,
    pub jvm_impl: Option<String>,
    pub download_url: String,
    pub download_size: Option<u64>,
    pub checksum: Option<String>,
    pub is_recommended: bool,
    pub is_experimental: bool,
}

/// Consulta builds disponíveis para um provider e plataforma atual.
pub async fn fetch_provider_builds(
    client: &Client,
    provider: JavaProviderKind,
) -> Result<Vec<AvailableJavaBuild>> {
    match provider {
        JavaProviderKind::Mojang => mojang::fetch_mojang_builds(client).await,
        JavaProviderKind::Adoptium => adoptium::fetch_adoptium_builds(client).await,
        JavaProviderKind::Azul => azul::fetch_azul_builds(client).await,
        JavaProviderKind::Semeru => semeru::fetch_semeru_builds(client).await,
    }
}
