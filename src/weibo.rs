use crate::provider::{AuthProvider, TokenResponse, UserInfo};
use crate::oauth2::{OAuth2Client, OAuth2Config};
use async_trait::async_trait;
use anyhow::Result;
use serde_json::Value;

pub struct WeiboProvider {
    client: OAuth2Client,
}

impl WeiboProvider {
    pub fn new(client_id: String, client_secret: String) -> Self {
        let config = OAuth2Config {
            client_id,
            client_secret,
            auth_url: "https://api.weibo.com/oauth2/authorize".to_string(),
            token_url: "https://api.weibo.com/oauth2/access_token".to_string(),
            user_info_url: "https://api.weibo.com/2/users/show.json".to_string(),
            scopes: "".to_string(),
            callback_path: Some("/weibo".to_string()),
            use_pkce: false,
        };
        Self {
            client: OAuth2Client::new(config),
        }
    }
}

#[async_trait]
impl AuthProvider for WeiboProvider {
    fn is_available(&self) -> bool {
        self.client.is_available()
    }

    fn get_origin(&self) -> String {
        "api.weibo.com".to_string()
    }

    async fn get_redirect_url(&self, server_url: &str, redirect: Option<&str>, state: Option<&str>) -> String {
        let callback_url = format!("{}/weibo", server_url.trim_end_matches('/'));

        // 与 JS 版本一致：使用 urlencoded 格式编码 state = { redirect, state, type }
        let mut state_params = std::collections::HashMap::new();
        state_params.insert("redirect", redirect.unwrap_or(""));
        state_params.insert("state", state.unwrap_or(""));
        state_params.insert("type", "weibo");
        
        let state_value = serde_urlencoded::to_string(&state_params).unwrap_or_default();

        let url = format!(
            "{}?client_id={}&redirect_uri={}&response_type=code&state={}",
            self.client.config.auth_url,
            urlencoding::encode(&self.client.config.client_id),
            urlencoding::encode(&callback_url),
            urlencoding::encode(&state_value)
        );

        url
    }

    async fn get_access_token(&self, code: &str, server_url: &str, _redirect: Option<&str>, _state: Option<&str>) -> Result<TokenResponse> {
        self.client.get_access_token(code, server_url, _redirect, _state).await
    }

    async fn get_user_info(&self, token: &TokenResponse) -> Result<UserInfo> {
        let client = self.client.http_client();
        
        // 需要先通过 token 获取 uid
        let token_info_url = "https://api.weibo.com/oauth2/get_token_info";
        let token_info = client.post(token_info_url)
            .form(&[("access_token", token.access_token.as_str())])
            .send()
            .await?
            .json::<Value>()
            .await?;
        
        let uid = token_info["uid"].as_i64()
            .ok_or_else(|| anyhow::anyhow!("uid not found in token_info"))?;
        
        let url = format!(
            "{}?access_token={}&uid={}",
            self.client.config.user_info_url,
            token.access_token,
            uid
        );
        let raw = client.get(&url)
            .send()
            .await?
            .json::<Value>()
            .await?;

        // Map Weibo fields to UserInfo
        let id = raw["idstr"].as_str()
            .or_else(|| raw["id"].as_str())
            .unwrap_or("")
            .to_string();

        let name = raw["screen_name"].as_str()
            .or_else(|| raw["name"].as_str())
            .unwrap_or("")
            .to_string();

        let url = raw["url"].as_str()
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("https://weibo.com/u/{}", id));

        let avatar = raw["avatar_large"].as_str()
            .or_else(|| raw["profile_image_url"].as_str())
            .map(|s| s.to_string());

        Ok(UserInfo {
            id,
            name,
            email: Some("".to_string()), // 与 JS 版本一致，始终返回空字符串
            url: Some(url),
            avatar,
        })
    }
}
