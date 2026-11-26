use crate::core::auth::account::{
    offline_player_uuid, validate_offline_username, AccountKind, AccountStatus, AccountsFile,
    LaunchCredentials, MinecraftAccount,
};
use crate::core::auth::microsoft::refresh_microsoft_token;
use crate::core::auth::token_store::{StoredTokens, TokenStore};
use crate::core::auth::xbox::{authenticate_xbox_and_xsts, XstsToken};
use crate::error::{LauncherError, Result};
use crate::platform::paths::AppPaths;
use crate::services::storage::{read_json, write_json_atomic};
use chrono::{Duration, Utc};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{info, warn};

const MC_LOGIN_URL: &str = "https://api.minecraftservices.com/authentication/login_with_xbox";
const MC_ENTITLEMENTS_URL: &str = "https://api.minecraftservices.com/entitlements/mcstore";
const MC_PROFILE_URL: &str = "https://api.minecraftservices.com/minecraft/profile";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct McLoginRequest {
    identity_token: String,
}

#[derive(Deserialize)]
struct McLoginResponse {
    access_token: String,
    expires_in: i64,
}

#[derive(Deserialize)]
struct EntitlementsResponse {
    #[serde(default)]
    items: Vec<EntitlementItem>,
}

#[derive(Deserialize)]
struct EntitlementItem {
    name: String,
}

#[derive(Deserialize)]
struct ProfileResponse {
    id: String,
    name: String,
    #[serde(default)]
    skins: Vec<ProfileSkin>,
}

#[derive(Deserialize)]
struct ProfileSkin {
    url: String,
    #[serde(default)]
    state: String,
}

/// Gerencia contas Microsoft/Minecraft, verificação de licença e renovação de sessão.
#[derive(Clone)]
pub struct AccountManager {
    client: Client,
    paths: AppPaths,
    token_store: Arc<dyn TokenStore>,
}

impl AccountManager {
    pub fn new(client: Client, paths: AppPaths, token_store: Arc<dyn TokenStore>) -> Self {
        Self {
            client,
            paths,
            token_store,
        }
    }

    pub fn is_token_store_persistent(&self) -> bool {
        self.token_store.is_persistent()
    }

    pub async fn load_accounts(&self) -> Vec<MinecraftAccount> {
        let path = self.paths.accounts_file();
        if !path.exists() {
            return Vec::new();
        }
        match read_json::<AccountsFile>(&path).await {
            Ok(file) => file.accounts,
            Err(err) => {
                warn!("Invalid accounts.json at {}: {err}", path.display());
                Vec::new()
            }
        }
    }

    pub async fn save_accounts(&self, accounts: &[MinecraftAccount]) -> Result<()> {
        let file = AccountsFile {
            accounts: accounts.to_vec(),
        };
        write_json_atomic(&self.paths.accounts_file(), &file).await
    }

    /// Completa a cadeia Xbox Live -> XSTS -> Minecraft Services -> Entitlement -> Profile
    /// a partir de um token OAuth Microsoft válido.
    pub async fn complete_microsoft_login(
        &self,
        ms_access_token: &str,
        ms_refresh_token: &str,
    ) -> Result<MinecraftAccount> {
        let xsts = authenticate_xbox_and_xsts(&self.client, ms_access_token).await?;
        let mc_login = self.login_minecraft_with_xbox(&xsts).await?;
        self.verify_minecraft_entitlement(&mc_login.access_token)
            .await?;
        let profile = self.fetch_minecraft_profile(&mc_login.access_token).await?;

        let skin_url = profile
            .skins
            .iter()
            .find(|s| s.state.eq_ignore_ascii_case("ACTIVE"))
            .or_else(|| profile.skins.first())
            .map(|s| s.url.clone());

        let expires_at = Utc::now() + Duration::seconds(mc_login.expires_in.max(60));
        let account_id = profile.id.clone();

        self.token_store.save_tokens(
            &account_id,
            &StoredTokens {
                ms_refresh_token: ms_refresh_token.to_string(),
                mc_access_token: mc_login.access_token,
            },
        )?;

        let mut accounts = self.load_accounts().await;
        let was_empty = accounts.is_empty();
        let existing_default = accounts
            .iter()
            .find(|a| a.id == account_id)
            .map(|a| a.is_default)
            .unwrap_or(was_empty);

        accounts.retain(|a| a.id != account_id);

        let account = MinecraftAccount {
            id: account_id,
            username: profile.name,
            uuid: profile.id,
            kind: AccountKind::Microsoft,
            xuid: xsts.xuid,
            skin_url,
            is_default: existing_default,
            status: AccountStatus::Authenticated,
            created_at: Some(Utc::now()),
            token_expires_at: Some(expires_at),
        };

        accounts.push(account.clone());
        self.save_accounts(&accounts).await?;
        info!("Authenticated Microsoft account '{}'", account.username);
        Ok(account)
    }

    /// Cria um perfil offline local com UUID determinístico do Minecraft,
    /// sem gerar tokens Microsoft fictícios.
    pub async fn create_offline_account(&self, raw_username: &str) -> Result<MinecraftAccount> {
        let username = validate_offline_username(raw_username)?;
        let mut accounts = self.load_accounts().await;

        if accounts
            .iter()
            .any(|a| a.username.eq_ignore_ascii_case(&username))
        {
            return Err(LauncherError::Auth(format!(
                "An account named '{username}' already exists."
            )));
        }

        let is_default = accounts.is_empty();
        let uuid = offline_player_uuid(&username);
        let account = MinecraftAccount {
            id: uuid::Uuid::new_v4().to_string(),
            username,
            uuid,
            kind: AccountKind::Offline,
            xuid: None,
            skin_url: None,
            is_default,
            status: AccountStatus::Authenticated,
            created_at: Some(Utc::now()),
            token_expires_at: None,
        };

        accounts.push(account.clone());
        self.save_accounts(&accounts).await?;
        info!("Created offline account '{}'", account.username);
        Ok(account)
    }

    /// Seleciona a conta padrão para execução do jogo.
    pub async fn set_default_account(&self, account_id: &str) -> Result<Vec<MinecraftAccount>> {
        let mut accounts = self.load_accounts().await;
        for acc in &mut accounts {
            acc.is_default = acc.id == account_id;
        }
        self.save_accounts(&accounts).await?;
        Ok(accounts)
    }

    /// Remove uma conta e apaga seus tokens do armazenamento seguro.
    pub async fn remove_account(&self, account_id: &str) -> Result<Vec<MinecraftAccount>> {
        let mut accounts = self.load_accounts().await;
        accounts.retain(|a| a.id != account_id);
        if !accounts.is_empty() && !accounts.iter().any(|a| a.is_default) {
            accounts[0].is_default = true;
        }
        self.token_store.delete_tokens(account_id)?;
        self.save_accounts(&accounts).await?;
        Ok(accounts)
    }

    /// Garante credenciais válidas para lançamento.
    /// Contas offline retornam sessão local sem acessar serviços da Microsoft.
    pub async fn resolve_launch_credentials(
        &self,
        client_id: Option<&str>,
    ) -> Result<LaunchCredentials> {
        let accounts = self.load_accounts().await;
        let account = accounts
            .iter()
            .find(|a| a.is_default)
            .or_else(|| accounts.first())
            .cloned()
            .ok_or_else(|| {
                LauncherError::Auth(
                    "No account selected. Add an offline or Microsoft account first.".to_string(),
                )
            })?;

        if account.kind == AccountKind::Offline {
            return Ok(LaunchCredentials {
                username: account.username,
                uuid: account.uuid,
                access_token: "0".to_string(),
                xuid: "0".to_string(),
                user_type: "legacy".to_string(),
                is_offline: true,
            });
        }

        let stored = self.token_store.load_tokens(&account.id)?.ok_or_else(|| {
            LauncherError::Auth(format!(
                "Session tokens for '{}' are not available. Please sign in again.",
                account.username
            ))
        })?;

        if !account.is_token_expired() {
            return Ok(LaunchCredentials {
                username: account.username,
                uuid: account.uuid,
                access_token: stored.mc_access_token,
                xuid: account.xuid.unwrap_or_default(),
                user_type: "msa".to_string(),
                is_offline: false,
            });
        }

        let cid = client_id.ok_or_else(|| {
            LauncherError::Auth(
                "Session expired and Microsoft authentication is not available in this build."
                    .to_string(),
            )
        })?;

        let refreshed_ms =
            refresh_microsoft_token(&self.client, cid, &stored.ms_refresh_token).await?;
        let new_refresh = refreshed_ms
            .refresh_token
            .unwrap_or(stored.ms_refresh_token);
        let updated = self
            .complete_microsoft_login(&refreshed_ms.access_token, &new_refresh)
            .await?;
        let new_stored = self.token_store.load_tokens(&updated.id)?.ok_or_else(|| {
            LauncherError::Auth("Failed to read refreshed session tokens.".to_string())
        })?;

        Ok(LaunchCredentials {
            username: updated.username,
            uuid: updated.uuid,
            access_token: new_stored.mc_access_token,
            xuid: updated.xuid.unwrap_or_default(),
            user_type: "msa".to_string(),
            is_offline: false,
        })
    }

    async fn login_minecraft_with_xbox(&self, xsts: &XstsToken) -> Result<McLoginResponse> {
        let req = McLoginRequest {
            identity_token: format!("XBL3.0 x={};{}", xsts.user_hash, xsts.token),
        };
        let resp = self.client.post(MC_LOGIN_URL).json(&req).send().await?;
        if !resp.status().is_success() {
            return Err(LauncherError::Auth(format!(
                "Minecraft Services login failed (HTTP {})",
                resp.status()
            )));
        }
        Ok(resp.json::<McLoginResponse>().await?)
    }

    async fn verify_minecraft_entitlement(&self, mc_access_token: &str) -> Result<()> {
        let resp = self
            .client
            .get(MC_ENTITLEMENTS_URL)
            .bearer_auth(mc_access_token)
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(LauncherError::Auth(format!(
                "Failed to verify Minecraft ownership (HTTP {})",
                resp.status()
            )));
        }
        let body: EntitlementsResponse = resp.json().await?;
        let owns_game = body
            .items
            .iter()
            .any(|item| item.name == "product_minecraft" || item.name == "game_minecraft");
        if !owns_game {
            return Err(LauncherError::Auth(
                "This Microsoft account does not own Minecraft: Java Edition.".to_string(),
            ));
        }
        Ok(())
    }

    async fn fetch_minecraft_profile(&self, mc_access_token: &str) -> Result<ProfileResponse> {
        let resp = self
            .client
            .get(MC_PROFILE_URL)
            .bearer_auth(mc_access_token)
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(LauncherError::Auth(format!(
                "Failed to fetch Minecraft profile (HTTP {})",
                resp.status()
            )));
        }
        Ok(resp.json::<ProfileResponse>().await?)
    }
}
