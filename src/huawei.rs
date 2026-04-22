use crate::provider::{AuthProvider, TokenResponse, UserInfo};
use crate::oauth2::{OAuth2Client, OAuth2Config};
use async_trait::async_trait;
use anyhow::{Result, anyhow};
use serde_json::Value;
use data_encoding::BASE64URL_NOPAD;

pub struct HuaweiProvider {
    client: OAuth2Client,
}

impl HuaweiProvider {
    pub fn new(client_id: String, client_secret: String) -> Self {
        let config = OAuth2Config {
            client_id,
            client_secret,
            auth_url: "https://oauth-login.cloud.huawei.com/oauth2/v3/authorize".to_string(),
            token_url: "https://oauth-login.cloud.huawei.com/oauth2/v3/token".to_string(),
            user_info_url: "".to_string(), // Huawei uses id_token JWT instead of user info endpoint
            scopes: "openid profile email".to_string(),
            callback_path: Some("/huawei".to_string()),
            use_pkce: false,
        };
        Self {
            client: OAuth2Client::new(config),
        }
    }

    /// Decode JWT id_token to extract user claims
    fn decode_id_token(id_token: &str) -> Result<Value> {
        let parts: Vec<&str> = id_token.split('.').collect();
        if parts.len() < 2 {
            return Err(anyhow!("Invalid JWT token: expected at least 2 parts"));
        }

        // Decode payload (second part)
        let payload_bytes = BASE64URL_NOPAD
            .decode(parts[1].as_bytes())
            .map_err(|e| anyhow!("Failed to decode JWT payload: {}", e))?;

        let claims: Value = serde_json::from_slice(&payload_bytes)
            .map_err(|e| anyhow!("Failed to parse JWT payload: {}", e))?;

        Ok(claims)
    }
}

#[async_trait]
impl AuthProvider for HuaweiProvider {
    fn is_available(&self) -> bool {
        self.client.is_available()
    }

    fn get_origin(&self) -> String {
        "oauth-login.cloud.huawei.com".to_string()
    }

    async fn get_redirect_url(&self, server_url: &str, redirect: Option<&str>, state: Option<&str>) -> String {
        self.client.get_redirect_url(server_url, redirect, state).await
    }

    async fn get_access_token(&self, code: &str, server_url: &str, _redirect: Option<&str>, _state: Option<&str>) -> Result<TokenResponse> {
        self.client.get_access_token(code, server_url, _redirect, _state).await
    }

    async fn get_user_info(&self, token: &TokenResponse) -> Result<UserInfo> {
        // Huawei returns user info in the id_token JWT
        let id_token = token.id_token.as_ref()
            .ok_or_else(|| anyhow!("Huawei provider requires id_token for user info"))?;

        let claims = Self::decode_id_token(id_token)?;

        // Extract standard OIDC claims
        let id = claims["sub"].as_str()
            .unwrap_or("")
            .to_string();

        let name = claims["name"].as_str()
            .or_else(|| claims["nickname"].as_str())
            .unwrap_or("")
            .to_string();

        let email = claims["email"].as_str().map(|s| s.to_string());

        // Huawei may include picture in claims
        let avatar = claims["picture"].as_str().map(|s| s.to_string());

        Ok(UserInfo {
            id,
            name,
            email,
            url: None,
            avatar,
        })
    }
}
