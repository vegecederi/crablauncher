use crate::core::config::{DownloadSettings, ProxyMode, ProxySettings, ProxyType};
use crate::error::{LauncherError, Result};
use reqwest::{Client, Proxy};
use std::time::Duration;

const USER_AGENT: &str = concat!("Crab/", env!("CARGO_PKG_VERSION"));

/// Cria o cliente HTTP padrão com timeouts adequados e User-Agent próprio.
pub fn build_http_client() -> Result<Client> {
    build_http_client_with_settings(&DownloadSettings::default(), &ProxySettings::default())
}

/// Cria um cliente HTTP respeitando timeout de conexão e configurações de proxy.
pub fn build_http_client_with_settings(
    downloads: &DownloadSettings,
    proxy: &ProxySettings,
) -> Result<Client> {
    let mut builder = Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(Duration::from_secs(downloads.connect_timeout_secs.max(3)))
        .timeout(Duration::from_secs(60));

    match proxy.mode {
        ProxyMode::None => {
            builder = builder.no_proxy();
        }
        ProxyMode::System => {}
        ProxyMode::Manual => {
            if !proxy.host.trim().is_empty() && proxy.port > 0 {
                let scheme = match proxy.proxy_type {
                    ProxyType::Http => "http",
                    ProxyType::Socks5 => "socks5",
                };
                let url = format!("{scheme}://{}:{}", proxy.host.trim(), proxy.port);
                let mut p = Proxy::all(&url).map_err(|err| LauncherError::DownloadFailed {
                    url,
                    reason: format!("Invalid proxy configuration: {err}"),
                })?;
                if let Some(user) = proxy.username.as_deref().filter(|u| !u.is_empty()) {
                    p = p.basic_auth(user, "");
                }
                builder = builder.proxy(p);
            }
        }
    }

    Ok(builder.build()?)
}
