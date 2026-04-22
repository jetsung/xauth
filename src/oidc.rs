use crate::provider::{AuthProvider, TokenResponse, UserInfo};
use crate::oauth2::{OAuth2Client, OAuth2Config};
use async_trait::async_trait;
use anyhow::Result;
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::RwLock;

/// OIDC 配置项
pub struct OidcConfig {
    pub issuer: Option<String>,
    pub auth_url: Option<String>,
    pub token_url: Option<String>,
    pub userinfo_url: Option<String>,
    pub scopes: Option<String>,
}

/// OIDC Discovery 响应
#[derive(Debug, Clone, Deserialize)]
struct OidcDiscovery {
    authorization_endpoint: String,
    token_endpoint: String,
    userinfo_endpoint: Option<String>,
}

/// 缓存的 OIDC 端点
#[derive(Clone)]
struct OidcEndpoints {
    auth_url: String,
    token_url: String,
    userinfo_url: String,
}

pub struct OidcProvider {
    client_id: String,
    client_secret: String,
    cfg: OidcConfig,
    origin: String,
    cached_endpoints: Arc<RwLock<Option<OidcEndpoints>>>,
}

impl OidcProvider {
    pub fn new(client_id: String, client_secret: String, cfg: OidcConfig) -> Self {
        let origin = if let Some(ref issuer) = cfg.issuer {
            Self::extract_origin(issuer)
        } else {
            cfg.auth_url.as_deref()
                .map(Self::extract_origin)
                .unwrap_or_else(|| "oidc".to_string())
        };

        Self {
            client_id,
            client_secret,
            cfg,
            origin,
            cached_endpoints: Arc::new(RwLock::new(None)),
        }
    }

    async fn get_endpoints(&self) -> OidcEndpoints {
        // 先检查缓存
        {
            let lock = self.cached_endpoints.read().await;
            if let Some(ref cached) = *lock {
                return cached.clone();
            }
        }

        // 获取端点
        let result = if let Some(ref issuer) = self.cfg.issuer {
            let discovery_url = format!("{}/.well-known/openid-configuration", issuer.trim_end_matches('/'));
            let http_client = reqwest::Client::new();
            match http_client.get(&discovery_url).send().await {
                Ok(resp) => match resp.json::<OidcDiscovery>().await {
                    Ok(d) => Some(OidcEndpoints {
                        auth_url: d.authorization_endpoint,
                        token_url: d.token_endpoint,
                        userinfo_url: d.userinfo_endpoint.unwrap_or_default(),
                    }),
                    Err(_) => None,
                },
                Err(_) => None,
            }
        } else {
            Some(OidcEndpoints {
                auth_url: self.cfg.auth_url.clone().unwrap_or_default(),
                token_url: self.cfg.token_url.clone().unwrap_or_default(),
                userinfo_url: self.cfg.userinfo_url.clone().unwrap_or_default(),
            })
        };

        let endpoints = result.unwrap_or_else(|| OidcEndpoints {
            auth_url: self.cfg.auth_url.clone().unwrap_or_default(),
            token_url: self.cfg.token_url.clone().unwrap_or_default(),
            userinfo_url: self.cfg.userinfo_url.clone().unwrap_or_default(),
        });

        // 缓存
        {
            let mut lock = self.cached_endpoints.write().await;
            *lock = Some(endpoints.clone());
        }

        endpoints
    }

    async fn make_client(&self) -> OAuth2Client {
        let endpoints = self.get_endpoints().await;

        let scopes = self.cfg.scopes.clone().unwrap_or_else(|| "openid profile email".to_string());

        OAuth2Client::new(OAuth2Config {
            client_id: self.client_id.clone(),
            client_secret: self.client_secret.clone(),
            auth_url: endpoints.auth_url,
            token_url: endpoints.token_url,
            user_info_url: endpoints.userinfo_url,
            scopes,
            callback_path: Some("/oidc".to_string()),
            use_pkce: false,
        })
    }

    fn extract_origin(url: &str) -> String {
        url.split("://")
            .nth(1)
            .and_then(|s| s.split('/').next())
            .unwrap_or("oidc")
            .to_string()
    }
}

#[async_trait]
impl AuthProvider for OidcProvider {
    fn is_available(&self) -> bool {
        !self.client_id.is_empty() && !self.client_secret.is_empty()
    }

    fn get_origin(&self) -> String {
        self.origin.clone()
    }

    async fn get_redirect_url(&self, server_url: &str, redirect: Option<&str>, state: Option<&str>) -> String {
        use crate::utils::StateData;
        use data_encoding::BASE64URL_NOPAD;

        let client = self.make_client().await;
        let mut url = format!(
            "{}?response_type=code&client_id={}",
            client.config.auth_url,
            urlencoding::encode(&client.config.client_id)
        );

        if let Some(ref callback_path) = client.config.callback_path {
            let callback_url = format!(
                "{}{}",
                server_url.trim_end_matches('/'),
                callback_path
            );

            // 构建 state 数据
            let state_data = StateData {
                redirect: redirect.unwrap_or("").to_string(),
                state: state.unwrap_or("").to_string(),
                verifier: None,
                callback_url: Some(callback_url.clone()),
            };

            // 编码 state
            let state_json = serde_json::to_string(&state_data).unwrap_or_default();
            let encoded_state = BASE64URL_NOPAD.encode(state_json.as_bytes());

            url.push_str(&format!("&redirect_uri={}", urlencoding::encode(&callback_url)));
            url.push_str(&format!("&scope={}", urlencoding::encode(&client.config.scopes)));
            url.push_str(&format!("&state={}", urlencoding::encode(&encoded_state)));
        }

        url
    }

    async fn get_access_token(&self, code: &str, server_url: &str, redirect: Option<&str>, state: Option<&str>) -> Result<TokenResponse> {
        let client = self.make_client().await;
        client.get_access_token(code, server_url, redirect, state).await
    }

    async fn get_user_info(&self, token: &TokenResponse) -> Result<UserInfo> {
        let client = self.make_client().await;
        client.get_user_info(token).await
    }
}
