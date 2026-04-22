use crate::provider::{AuthProvider, UserInfo, TokenResponse};
use crate::oauth2::{OAuth2Client, OAuth2Config};
use async_trait::async_trait;
use anyhow::Result;

pub struct GitHubProvider {
    client: OAuth2Client,
}

impl GitHubProvider {
    pub fn new(client_id: String, client_secret: String) -> Self {
        let config = OAuth2Config {
            client_id,
            client_secret,
            auth_url: "https://github.com/login/oauth/authorize".to_string(),
            token_url: "https://github.com/login/oauth/access_token".to_string(),
            user_info_url: "https://api.github.com/user".to_string(),
            scopes: "read:user,user:email".to_string(),
            callback_path: Some("/github".to_string()),
            use_pkce: false,
        };
        Self {
            client: OAuth2Client::new(config),
        }
    }
}

#[async_trait]
impl AuthProvider for GitHubProvider {
    fn is_available(&self) -> bool {
        self.client.is_available()
    }

    fn get_origin(&self) -> String {
        "github.com".to_string()
    }

    async fn get_redirect_url(&self, server_url: &str, redirect: Option<&str>, state: Option<&str>) -> String {
        self.client.get_redirect_url(server_url, redirect, state).await
    }

    async fn get_access_token(&self, code: &str, server_url: &str, _redirect: Option<&str>, _state: Option<&str>) -> Result<TokenResponse> {
        self.client.get_access_token(code, server_url, _redirect, _state).await
    }

    async fn get_user_info(&self, token: &TokenResponse) -> Result<UserInfo> {
        let mut user: serde_json::Value = self.client.http_client()
            .get("https://api.github.com/user")
            .bearer_auth(&token.access_token)
            .header("User-Agent", "xauth")
            .header("Accept", "application/json")
            .send()
            .await?
            .json::<serde_json::Value>()
            .await?;

        // 获取邮箱
        let emails = self.client.http_client()
            .get("https://api.github.com/user/emails")
            .bearer_auth(&token.access_token)
            .header("User-Agent", "xauth")
            .header("Accept", "application/json")
            .send()
            .await?
            .json::<serde_json::Value>()
            .await?;

        // 合并邮箱到用户信息
        if let Some(obj) = user.as_object_mut() {
            obj.insert("emails".to_string(), emails.clone());
            // 提取主邮箱
            if let Some(email_array) = emails.as_array() {
                for email_obj in email_array {
                    if email_obj["primary"].as_bool() == Some(true) {
                        if let Some(email) = email_obj["email"].as_str() {
                            obj.insert("email".to_string(), serde_json::Value::String(email.to_string()));
                            break;
                        }
                    }
                }
            }
        }

        // 与 JS 版本完全对齐：优先使用 blog，否则回退到 github 个人主页
        let url = if let Some(blog) = user["blog"].as_str().filter(|s| !s.is_empty()) {
            Some(blog.to_string())
        } else if let Some(login) = user["login"].as_str() {
            Some(format!("https://github.com/{}", login))
        } else {
            None
        };

        let mut user_info = self.client.normalize_user_info(user);
        user_info.url = url;

        Ok(user_info)
    }
}
