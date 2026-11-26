use crate::error::{LauncherError, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};

const XBL_AUTH_URL: &str = "https://user.auth.xboxlive.com/user/authenticate";
const XSTS_AUTH_URL: &str = "https://xsts.auth.xboxlive.com/xsts/authorize";

#[derive(Debug, Clone)]
pub struct XstsToken {
    pub token: String,
    pub user_hash: String,
    pub xuid: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct XblRequest<'a> {
    properties: XblProperties<'a>,
    relying_party: &'a str,
    token_type: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct XblProperties<'a> {
    auth_method: &'a str,
    site_name: &'a str,
    rps_ticket: String,
}

#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct XstsRequest<'a> {
    properties: XstsProperties<'a>,
    relying_party: &'a str,
    token_type: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct XstsProperties<'a> {
    sandbox_id: &'a str,
    user_tokens: Vec<&'a str>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct XboxAuthResponse {
    token: String,
    display_claims: DisplayClaims,
}

#[derive(Deserialize)]
struct DisplayClaims {
    xui: Vec<XuiClaim>,
}

#[derive(Deserialize)]
struct XuiClaim {
    uhs: String,
    #[serde(default)]
    xid: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct XstsErrorResponse {
    #[serde(default)]
    x_err: u64,
}

/// Autentica no Xbox Live e obtém o token XSTS para os serviços do Minecraft.
pub async fn authenticate_xbox_and_xsts(
    client: &Client,
    ms_access_token: &str,
) -> Result<XstsToken> {
    let xbl_req = XblRequest {
        properties: XblProperties {
            auth_method: "RPS",
            site_name: "user.auth.xboxlive.com",
            rps_ticket: format!("d={ms_access_token}"),
        },
        relying_party: "http://auth.xboxlive.com",
        token_type: "JWT",
    };

    let xbl_resp = client
        .post(XBL_AUTH_URL)
        .header("Accept", "application/json")
        .json(&xbl_req)
        .send()
        .await?;

    if !xbl_resp.status().is_success() {
        return Err(LauncherError::Auth(format!(
            "Xbox Live authentication failed (HTTP {})",
            xbl_resp.status()
        )));
    }

    let xbl: XboxAuthResponse = xbl_resp.json().await?;

    let xsts_req = XstsRequest {
        properties: XstsProperties {
            sandbox_id: "RETAIL",
            user_tokens: vec![&xbl.token],
        },
        relying_party: "rp://api.minecraftservices.com/",
        token_type: "JWT",
    };

    let xsts_resp = client
        .post(XSTS_AUTH_URL)
        .header("Accept", "application/json")
        .json(&xsts_req)
        .send()
        .await?;

    if !xsts_resp.status().is_success() {
        let err_code = xsts_resp
            .json::<XstsErrorResponse>()
            .await
            .map(|e| e.x_err)
            .unwrap_or(0);
        let msg = match err_code {
            2148916233 => "This Microsoft account does not have an Xbox profile.",
            2148916235 => "Xbox Live is not available in your country/region.",
            2148916236 | 2148916237 => "This account requires adult verification on Xbox Live.",
            2148916238 => "Child accounts must be added to a Microsoft Family before signing in.",
            _ => "Xbox XSTS authorization failed.",
        };
        return Err(LauncherError::Auth(msg.to_string()));
    }

    let xsts: XboxAuthResponse = xsts_resp.json().await?;
    let claim =
        xsts.display_claims.xui.into_iter().next().ok_or_else(|| {
            LauncherError::Auth("Missing user hash in XSTS response.".to_string())
        })?;

    Ok(XstsToken {
        token: xsts.token,
        user_hash: claim.uhs,
        xuid: claim.xid,
    })
}
