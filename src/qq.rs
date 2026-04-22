use crate::provider::{AuthProvider, TokenResponse, UserInfo};
use crate::oauth2::{OAuth2Client, OAuth2Config};
use async_trait::async_trait;
use anyhow::Result;
use serde_json::Value;

pub struct QQProvider {
    client: OAuth2Client,
}

impl QQProvider {
    pub fn new(client_id: String, client_secret: String) -> Self {
        let config = OAuth2Config {
            client_id,
            client_secret,
            auth_url: "https://graph.qq.com/oauth2.0/authorize".to_string(),
            token_url: "https://graph.qq.com/oauth2.0/token".to_string(),
            user_info_url: "https://graph.qq.com/user/get_user_info?fmt=json&unionid=1".to_string(),
            scopes: "".to_string(),
            callback_path: Some("/qq".to_string()),
            use_pkce: false,
        };
        Self {
            client: OAuth2Client::new(config),
        }
    }
}

#[async_trait]
impl AuthProvider for QQProvider {
    fn is_available(&self) -> bool {
        self.client.is_available()
    }

    fn get_origin(&self) -> String {
        "graph.qq.com".to_string()
    }

    async fn get_redirect_url(&self, server_url: &str, redirect: Option<&str>, state: Option<&str>) -> String {
        self.client.get_redirect_url(server_url, redirect, state).await
    }

    async fn get_access_token(&self, code: &str, server_url: &str, _redirect: Option<&str>, _state: Option<&str>) -> Result<TokenResponse> {
        self.client.get_access_token(code, server_url, _redirect, _state).await
    }

    async fn get_user_info(&self, token: &TokenResponse) -> Result<UserInfo> {
        // QQ user info endpoint returns form-encoded data, not JSON
        let client = self.client.http_client();
        let url = format!(
            "{}&access_token={}&openid={}",
            self.client.config.user_info_url,
            token.access_token,
            token.access_token, // openid would normally come from token info endpoint
        );
        let text = client.get(&url)
            .send()
            .await?
            .text()
            .await?;

        // QQ may return JSON or form-encoded; try JSON first
        let raw: Value = if text.starts_with('{') {
            serde_json::from_str(&text)?
        } else {
            // Parse form-encoded response
            let parsed: std::collections::HashMap<String, String> = serde_urlencoded::from_str(&text)?;
            serde_json::to_value(parsed)?
        };

        let id = raw["openid"].as_str()
            .or_else(|| raw["client_id"].as_str())
            .unwrap_or("")
            .to_string();

        let name = raw["nickname"].as_str()
            .unwrap_or("")
            .to_string();

        // Avatar priority: figureurl_qq_2 > figureurl_qq_1 > figureurl_qq > figureurl_2 > figureurl_1 > figureurl
        let avatar = raw["figureurl_qq_2"].as_str()
            .or_else(|| raw["figureurl_qq_1"].as_str())
            .or_else(|| raw["figureurl_qq"].as_str())
            .or_else(|| raw["figureurl_2"].as_str())
            .or_else(|| raw["figureurl_1"].as_str())
            .or_else(|| raw["figureurl"].as_str())
            .map(|s| s.to_string());

        Ok(UserInfo {
            id,
            name,
            email: None,
            url: None,
            avatar,
        })
    }
}
