use crate::error::{LauncherError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tracing::warn;

const KEYRING_SERVICE: &str = "org.crab.launcher";

/// Conjunto de tokens armazenados de forma segura no cofre do sistema operacional.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredTokens {
    pub ms_refresh_token: String,
    pub mc_access_token: String,
}

/// Abstração para armazenamento seguro de tokens (Secret Service / Keychain / Credential Manager
/// com fallback explícito em memória quando o cofre do SO não estiver disponível).
pub trait TokenStore: Send + Sync {
    fn save_tokens(&self, account_id: &str, tokens: &StoredTokens) -> Result<()>;
    fn load_tokens(&self, account_id: &str) -> Result<Option<StoredTokens>>;
    fn delete_tokens(&self, account_id: &str) -> Result<()>;
    fn is_persistent(&self) -> bool;
}

/// Armazenamento seguro usando o keyring nativo do sistema operacional,
/// com fallback automático em memória se o serviço de segredos estiver indisponível.
pub struct OsTokenStore {
    memory_fallback: Mutex<HashMap<String, StoredTokens>>,
    use_memory_only: bool,
}

impl OsTokenStore {
    pub fn new() -> Arc<Self> {
        // Sonda rápida para verificar se o backend do keyring responde.
        let probe_ok = keyring::Entry::new(KEYRING_SERVICE, "__crab_probe__").is_ok();
        if !probe_ok {
            warn!("OS secret service unavailable; using temporary in-memory token session.");
        }
        Arc::new(Self {
            memory_fallback: Mutex::new(HashMap::new()),
            use_memory_only: !probe_ok,
        })
    }

    pub fn in_memory() -> Arc<Self> {
        Arc::new(Self {
            memory_fallback: Mutex::new(HashMap::new()),
            use_memory_only: true,
        })
    }
}

impl TokenStore for OsTokenStore {
    fn save_tokens(&self, account_id: &str, tokens: &StoredTokens) -> Result<()> {
        if let Ok(mut mem) = self.memory_fallback.lock() {
            mem.insert(account_id.to_string(), tokens.clone());
        }
        if self.use_memory_only {
            return Ok(());
        }
        let payload = serde_json::to_string(tokens)?;
        match keyring::Entry::new(KEYRING_SERVICE, account_id) {
            Ok(entry) => {
                if let Err(err) = entry.set_password(&payload) {
                    warn!(
                        "Failed to persist tokens in OS keyring ({err}); kept in memory session."
                    );
                }
            }
            Err(err) => {
                warn!("OS keyring entry error ({err}); kept in memory session.");
            }
        }
        Ok(())
    }

    fn load_tokens(&self, account_id: &str) -> Result<Option<StoredTokens>> {
        if let Ok(mem) = self.memory_fallback.lock() {
            if let Some(t) = mem.get(account_id) {
                return Ok(Some(t.clone()));
            }
        }
        if self.use_memory_only {
            return Ok(None);
        }
        let entry = keyring::Entry::new(KEYRING_SERVICE, account_id)
            .map_err(|err| LauncherError::Auth(format!("Keyring error: {err}")))?;
        match entry.get_password() {
            Ok(secret) => {
                let parsed: StoredTokens = serde_json::from_str(&secret).map_err(|err| {
                    LauncherError::Auth(format!("Corrupted token entry in keyring: {err}"))
                })?;
                Ok(Some(parsed))
            }
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(err) => {
                warn!("Could not read tokens from OS keyring: {err}");
                Ok(None)
            }
        }
    }

    fn delete_tokens(&self, account_id: &str) -> Result<()> {
        if let Ok(mut mem) = self.memory_fallback.lock() {
            mem.remove(account_id);
        }
        if self.use_memory_only {
            return Ok(());
        }
        if let Ok(entry) = keyring::Entry::new(KEYRING_SERVICE, account_id) {
            let _ = entry.delete_credential();
        }
        Ok(())
    }

    fn is_persistent(&self) -> bool {
        !self.use_memory_only
    }
}
