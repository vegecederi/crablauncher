use crate::error::{LauncherError, Result};
use chrono::{DateTime, Utc};
use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};

/// Tipo de conta suportado pelo Crab.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountKind {
    #[default]
    Microsoft,
    Offline,
}

impl AccountKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Microsoft => "Microsoft",
            Self::Offline => "Offline",
        }
    }
}

/// Estado de autenticação de uma conta.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountStatus {
    #[default]
    Authenticated,
    Expired,
    Error,
}

impl AccountStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Authenticated => "Ready",
            Self::Expired => "Session expired",
            Self::Error => "Auth error",
        }
    }
}

/// Metadados públicos de uma conta persistidos em `accounts.json`.
/// Tokens sensíveis (access_token, refresh_token) NUNCA são armazenados neste arquivo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MinecraftAccount {
    pub id: String,
    pub username: String,
    pub uuid: String,
    #[serde(default)]
    pub kind: AccountKind,
    #[serde(default)]
    pub xuid: Option<String>,
    #[serde(default)]
    pub skin_url: Option<String>,
    #[serde(default)]
    pub is_default: bool,
    #[serde(default)]
    pub status: AccountStatus,
    #[serde(default)]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub token_expires_at: Option<DateTime<Utc>>,
}

impl MinecraftAccount {
    pub fn is_token_expired(&self) -> bool {
        if self.kind == AccountKind::Offline {
            return false;
        }
        match self.token_expires_at {
            Some(exp) => Utc::now() + chrono::Duration::minutes(5) >= exp,
            None => true,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AccountsFile {
    #[serde(default)]
    pub accounts: Vec<MinecraftAccount>,
}

/// Credenciais efêmeras usadas em tempo de execução para lançar o Minecraft.
#[derive(Debug, Clone)]
pub struct LaunchCredentials {
    pub username: String,
    pub uuid: String,
    pub access_token: String,
    pub xuid: String,
    pub user_type: String,
    pub is_offline: bool,
}

/// Valida o nome de usuário conforme as convenções do Minecraft (3–16 chars, A-Z, a-z, 0-9, _).
pub fn validate_offline_username(raw: &str) -> Result<String> {
    let trimmed = raw.trim();
    let len = trimmed.len();
    if !(3..=16).contains(&len) {
        return Err(LauncherError::Auth(
            "Username must be between 3 and 16 characters.".to_string(),
        ));
    }
    if !trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return Err(LauncherError::Auth(
            "Username may only contain letters, numbers, and underscores.".to_string(),
        ));
    }
    Ok(trimmed.to_string())
}

/// Gera o UUID v3 determinístico do Minecraft para perfis offline:
/// `UUID.nameUUIDFromBytes("OfflinePlayer:<username>")`.
pub fn offline_player_uuid(username: &str) -> String {
    let mut hasher = Md5::new();
    hasher.update(format!("OfflinePlayer:{username}").as_bytes());
    let mut bytes: [u8; 16] = hasher.finalize().into();
    // Define versão 3 (MD5) e variante RFC 4122.
    bytes[6] = (bytes[6] & 0x0f) | 0x30;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    hex::encode(bytes)
}
