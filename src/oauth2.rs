use crate::provider::{AuthProvider, UserInfo, TokenResponse};
use async_trait::async_trait;
use serde_json::Value;
use anyhow::{Result, anyhow};
use reqwest::Client;
use std::collections::HashMap;
use crate::utils::generate_pkce_verifier;
use crate::utils::generate_pkce_challenge;

/// OAuth2 客户端配置
pub struct OAuth2Config {
    pub client_id: String,
    pub client_secret: String,
    pub auth_url: String,
    pub token_url: String,
    pub user_info_url: String,
    pub scopes: String,
    /// OAuth 回调路径，如 "/github"（OAuth 提供商会将 code 回调到此地址）
    pub callback_path: Option<String>,
    pub use_pkce: bool,
}

pub struct OAuth2Client {
    pub config: OAuth2Config,
    http_client: Client,
    pub pkce_verifier: Option<String>,
}

impl OAuth2Client {
    pub fn new(config: OAuth2Config) -> Self {
        let mut client = Self {
            config,
            http_client: Client::new(),
            pkce_verifier: None,
        };
        // 如果使用 PKCE，预生成 verifier
        if client.config.use_pkce {
            client.pkce_verifier = Some(generate_pkce_verifier());
        }
        client
    }

    pub fn http_client(&self) -> &Client {
        &self.http_client
    }

    /// 解析 token 响应
    pub fn parse_token_response(&self, text: &str) -> Result<TokenResponse> {
        // 先尝试 JSON
        if let Ok(json) = serde_json::from_str::<Value>(text) {
            return Ok(TokenResponse {
                access_token: json["access_token"].as_str()
                    .map(|s| s.to_string())
                    .ok_or_else(|| anyhow!("Missing access_token in: {}", text))?,
                token_type: json["token_type"].as_str().map(|s| s.to_string()),
                expires_in: json["expires_in"].as_i64(),
                refresh_token: json["refresh_token"].as_str().map(|s| s.to_string()),
                id_token: json["id_token"].as_str().map(|s| s.to_string()),
            });
        }

        // 尝试 form-urlencoded
        let parsed: HashMap<String, String> = serde_urlencoded::from_str(text)
            .map_err(|e| anyhow!("Failed to parse token response: {}", e))?;

        Ok(TokenResponse {
            access_token: parsed.get("access_token")
                .cloned()
                .ok_or_else(|| anyhow!("Missing access_token in: {}", text))?,
            token_type: parsed.get("token_type").cloned(),
            expires_in: parsed.get("expires_in").and_then(|s| s.parse().ok()),
            refresh_token: parsed.get("refresh_token").cloned(),
            id_token: parsed.get("id_token").cloned(),
        })
    }

    /// 标准化用户信息
    pub fn normalize_user_info(&self, raw: Value) -> UserInfo {
        // 支持 X(Twitter) 的嵌套 data 结构
        let user = if raw.get("data").is_some() {
            &raw["data"]
        } else {
            &raw
        };
        
        let id = user["id"].as_str()
            .or_else(|| user["sub"].as_str())
            .or_else(|| user["login"].as_str()) // GitHub 使用 login 字段
            .or_else(|| user["username"].as_str())
            .unwrap_or("")
            .to_string();

        let name = user["name"].as_str()
            .or_else(|| user["preferred_username"].as_str())
            .or_else(|| user["screen_name"].as_str())
            .or_else(|| user["nickname"].as_str())
            .or_else(|| user["username"].as_str())
            .unwrap_or("")
            .to_string();

        let email = user["email"].as_str()
            .or_else(|| user["confirmed_email"].as_str()) // X(Twitter) 使用 confirmed_email
            .map(|s| s.to_string());

        // 对于 Twitter，如果没有 url 字段，使用 username 构建个人主页链接
        let url = user["url"].as_str()
            .or_else(|| user["profile"].as_str())
            .or_else(|| user["website"].as_str())
            .or_else(|| user["link"].as_str())
            .or_else(|| user["html_url"].as_str())
            .map(|s| s.to_string())
            .or_else(|| {
                user["username"].as_str().map(|u| format!("https://x.com/{}", u))
            });

        let avatar = user["avatar_url"].as_str()
            .or_else(|| user["avatar_large"].as_str())
            .or_else(|| user["profile_image_url"].as_str())
            .or_else(|| user["picture"].as_str())
            .or_else(|| user["avatar"].as_str())
            .map(|s| {
                // 清理可能存在的引号 (匹配 Node.js 处理逻辑)
                s.trim().trim_matches('`').trim_matches('"').to_string()
            });

        UserInfo { id, name, email, url, avatar }
    }
}

#[async_trait]
impl AuthProvider for OAuth2Client {
    fn is_available(&self) -> bool {
        !self.config.client_id.is_empty() && !self.config.client_secret.is_empty()
    }

    fn get_origin(&self) -> String {
        // 从 auth_url 提取 host
        self.config.auth_url
            .split("://")
            .nth(1)
            .and_then(|s| s.split('/').next())
            .unwrap_or("unknown")
            .to_string()
    }

    async fn get_redirect_url(&self, server_url: &str, redirect: Option<&str>, state: Option<&str>) -> String {
        let mut url = format!(
            "{}?response_type=code&client_id={}",
            self.config.auth_url,
            urlencoding::encode(&self.config.client_id)
        );

        // 构建 redirect_uri: 先编码 redirect 值，再整体 URL 编码
        // 匹配 Node.js: qs.stringify({redirect, state}) 先编码 redirect，再外层 qs.stringify({redirect_uri})
        if let Some(ref callback_path) = self.config.callback_path {
            let callback_url = format!(
                "{}{}",
                server_url.trim_end_matches('/'),
                callback_path
            );
            
            // 构建 state 数据对象
            use crate::utils::StateData;
            use data_encoding::BASE64URL_NOPAD;
            
            let mut state_data = StateData {
                redirect: redirect.unwrap_or("").to_string(),
                state: state.unwrap_or("").to_string(),
                verifier: None,
                callback_url: None,
            };
            
            // 如果使用 PKCE，将 verifier 和 callback_url 加入 state
            if self.config.use_pkce {
                state_data.verifier = self.pkce_verifier.clone();
                state_data.callback_url = Some(callback_url.clone());
            }
            
            // 编码 state 数据为 base64 URL
            let state_json = serde_json::to_string(&state_data).unwrap_or_default();
            let encoded_state = BASE64URL_NOPAD.encode(state_json.as_bytes());
            
            let raw_callback_url = if let Some(redirect_url) = redirect {
                // 二层编码（GitHub 方式）：
                // 1. 编码 redirect 参数值
                let encoded_redirect = urlencoding::encode(redirect_url);
                // 2. 拼 callback path 和参数
                format!(
                    "{}?redirect={}&state={}",
                    callback_url,
                    encoded_redirect,
                    urlencoding::encode(state.unwrap_or(""))
                )
            } else {
                format!(
                    "{}?state={}",
                    callback_url,
                    encoded_state
                )
            };

            url.push_str(&format!("&redirect_uri={}", urlencoding::encode(&raw_callback_url)));
        }

        // scope 参数（只在有值时添加）
        if !self.config.scopes.is_empty() {
            url.push_str(&format!("&scope={}", urlencoding::encode(&self.config.scopes)));
        }

        // PKCE 支持
        if self.config.use_pkce {
            if let Some(ref verifier) = self.pkce_verifier {
                let challenge = generate_pkce_challenge(verifier);
                url.push_str("&code_challenge_method=S256");
                url.push_str(&format!("&code_challenge={}", challenge));
            }
        }

        url
    }

    async fn get_access_token(&self, code: &str, server_url: &str, _redirect: Option<&str>, state_param: Option<&str>) -> Result<TokenResponse> {
        let mut params = vec![
            ("client_id", self.config.client_id.clone()),
            ("client_secret", self.config.client_secret.clone()),
            ("code", code.to_string()),
            ("grant_type", "authorization_code".to_string()),
        ];

        // 如果使用 PKCE 并且有 state 参数，从 state 中提取 verifier 和 callback_url
        if self.config.use_pkce {
            if let Some(encoded_state) = state_param {
                if let Ok(state_data) = crate::utils::decode_state(encoded_state) {
                    if let Some(verifier) = state_data.verifier {
                        params.push(("code_verifier", verifier));
                    }
                    // 使用 state 中存储的 callback_url（确保和授权时一致）
                    if let Some(callback_url) = state_data.callback_url {
                        params.push(("redirect_uri", callback_url));
                    }
                }
            }
        } else {
            // 非 PKCE 提供商使用标准 redirect_uri
            if let Some(ref callback_path) = self.config.callback_path {
                let redirect_uri = format!("{}{}", server_url.trim_end_matches('/'), callback_path);
                params.push(("redirect_uri", redirect_uri));
            }
        }

        let res = self.http_client.post(&self.config.token_url)
            .form(&params)
            .header("Accept", "application/json")
            .send()
            .await?;

        let text = res.text().await?;
        self.parse_token_response(&text)
    }

    async fn get_user_info(&self, token: &TokenResponse) -> Result<UserInfo> {
        let raw = self.http_client.get(&self.config.user_info_url)
            .bearer_auth(&token.access_token)
            .send()
            .await?
            .json::<Value>()
            .await?;
        Ok(self.normalize_user_info(raw))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{MockServer, Mock, ResponseTemplate};
    use wiremock::matchers::{method, path};

    #[tokio::test]
    async fn test_get_access_token() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"access_token": "mock_token"})))
            .mount(&mock_server)
            .await;

        let client = OAuth2Client::new(OAuth2Config {
            client_id: "id".into(),
            client_secret: "secret".into(),
            auth_url: "auth".into(),
            token_url: format!("{}/token", mock_server.uri()),
            user_info_url: "user".into(),
            scopes: "read".into(),
            callback_path: Some("/callback".into()),
            use_pkce: false,
        });

        let token = client.get_access_token("code", "http://localhost", None, None).await.unwrap();
        assert_eq!(token.access_token, "mock_token");
    }
}
