use crate::error::{LauncherError, Result};
use qrcode::{Color, QrCode};
use reqwest::Client;
use serde::Deserialize;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

const DEVICE_CODE_URL: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/devicecode";
const TOKEN_URL: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/token";
const OAUTH_SCOPE: &str = "XboxLive.signin offline_access";

/// Resolve o Client ID Microsoft OAuth no nível da distribuição (compile-time)
/// ou de ambiente de desenvolvimento, sem expor configuração manual ao jogador.
pub fn distribution_client_id() -> Option<String> {
    if let Some(compiled) = option_env!("CRAB_MS_CLIENT_ID") {
        let trimmed = compiled.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    if let Ok(env_id) = std::env::var("CRAB_MS_CLIENT_ID") {
        let trimmed = env_id.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    None
}

/// Gera a matriz booleana de um QR Code localmente (sem serviços externos).
/// Retorna `(largura, módulos)` onde `true` representa módulo escuro.
pub fn generate_qr_matrix(content: &str) -> Result<(usize, Vec<bool>)> {
    let code = QrCode::new(content.as_bytes())
        .map_err(|err| LauncherError::Auth(format!("Failed to generate QR code: {err}")))?;
    let width = code.width();
    let modules: Vec<bool> = code
        .into_colors()
        .into_iter()
        .map(|c| c == Color::Dark)
        .collect();
    Ok((width, modules))
}

/// Resposta inicial do fluxo OAuth 2.0 Device Code da Microsoft.
#[derive(Debug, Clone, Deserialize)]
pub struct DeviceCodeResponse {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    #[serde(default)]
    pub verification_uri_complete: Option<String>,
    pub expires_in: u64,
    #[serde(default = "default_interval")]
    pub interval: u64,
    #[serde(default)]
    pub message: String,
}

impl DeviceCodeResponse {
    /// URL preferencial para QR Code e abertura no navegador.
    pub fn qr_target_url(&self) -> &str {
        self.verification_uri_complete
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or(&self.verification_uri)
    }
}

fn default_interval() -> u64 {
    5
}

/// Tokens retornados pelo endpoint OAuth 2.0 da Microsoft.
#[derive(Debug, Clone, Deserialize)]
pub struct MicrosoftTokenResponse {
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    pub expires_in: u64,
}

#[derive(Debug, Deserialize)]
struct TokenErrorResponse {
    error: String,
    #[serde(default)]
    error_description: Option<String>,
}

/// Inicia o fluxo OAuth 2.0 Device Code com o Client ID da distribuição.
pub async fn start_device_code_flow(
    client: &Client,
    client_id: &str,
) -> Result<DeviceCodeResponse> {
    if client_id.trim().is_empty() {
        return Err(LauncherError::Auth(
            "Microsoft authentication is not available in this build.".to_string(),
        ));
    }

    let resp = client
        .post(DEVICE_CODE_URL)
        .form(&[("client_id", client_id), ("scope", OAUTH_SCOPE)])
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        return Err(LauncherError::Auth(format!(
            "Could not start Microsoft sign-in (HTTP {status}). Please try again later."
        )));
    }

    Ok(resp.json::<DeviceCodeResponse>().await?)
}

/// Aguarda a conclusão do login no navegador pelo usuário (Device Code polling).
pub async fn poll_device_code_token(
    client: &Client,
    client_id: &str,
    device: &DeviceCodeResponse,
    cancel: &CancellationToken,
) -> Result<MicrosoftTokenResponse> {
    let mut interval_secs = device.interval.max(2);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(device.expires_in);

    loop {
        if cancel.is_cancelled() {
            return Err(LauncherError::Cancelled);
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(LauncherError::Auth(
                "Sign-in code expired. Please try again.".to_string(),
            ));
        }

        tokio::select! {
            _ = cancel.cancelled() => return Err(LauncherError::Cancelled),
            _ = tokio::time::sleep(Duration::from_secs(interval_secs)) => {}
        }

        let resp = client
            .post(TOKEN_URL)
            .form(&[
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                ("client_id", client_id),
                ("device_code", &device.device_code),
            ])
            .send()
            .await?;

        if resp.status().is_success() {
            return Ok(resp.json::<MicrosoftTokenResponse>().await?);
        }

        let err_body = resp.json::<TokenErrorResponse>().await.ok();
        match err_body.as_ref().map(|e| e.error.as_str()) {
            Some("authorization_pending") => continue,
            Some("slow_down") => {
                interval_secs += 5;
                continue;
            }
            Some("authorization_declined") => {
                return Err(LauncherError::Auth(
                    "Microsoft sign-in was declined.".to_string(),
                ));
            }
            Some("expired_token") => {
                return Err(LauncherError::Auth(
                    "Sign-in code expired. Please try again.".to_string(),
                ));
            }
            _ => {
                let desc = err_body
                    .and_then(|e| e.error_description)
                    .unwrap_or_else(|| "Microsoft authentication failed.".to_string());
                return Err(LauncherError::Auth(desc));
            }
        }
    }
}

/// Renova o access token Microsoft usando um refresh token previamente salvo.
pub async fn refresh_microsoft_token(
    client: &Client,
    client_id: &str,
    refresh_token: &str,
) -> Result<MicrosoftTokenResponse> {
    if client_id.trim().is_empty() {
        return Err(LauncherError::Auth(
            "Microsoft authentication is not available in this build.".to_string(),
        ));
    }

    let resp = client
        .post(TOKEN_URL)
        .form(&[
            ("grant_type", "refresh_token"),
            ("client_id", client_id),
            ("refresh_token", refresh_token),
            ("scope", OAUTH_SCOPE),
        ])
        .send()
        .await?;

    if !resp.status().is_success() {
        return Err(LauncherError::Auth(
            "Microsoft session expired or revoked. Please sign in again.".to_string(),
        ));
    }

    Ok(resp.json::<MicrosoftTokenResponse>().await?)
}
