use crate::provider::{AuthProvider, TokenResponse, UserInfo};
use crate::oauth2::{OAuth2Client, OAuth2Config};
use async_trait::async_trait;
use anyhow::Result;

pub struct TwitterProvider {
    client: OAuth2Client,
}

impl TwitterProvider {
    pub fn new(client_id: String, client_secret: String) -> Self {
        let config = OAuth2Config {
            client_id,
            client_secret,
            auth_url: "https://x.com/i/oauth2/authorize".to_string(),
            token_url: "https://api.x.com/2/oauth2/token".to_string(),
            user_info_url: "https://api.x.com/2/users/me".to_string(),
            scopes: "tweet.read users.read offline.access users.email".to_string(),
            callback_path: Some("/twitter".to_string()),
            use_pkce: true,
        };
        Self {
            client: OAuth2Client::new(config),
        }
    }
}

#[async_trait]
impl AuthProvider for TwitterProvider {
    fn is_available(&self) -> bool {
        self.client.is_available()
    }

    fn get_origin(&self) -> String {
        "x.com".to_string()
    }

    async fn get_redirect_url(&self, server_url: &str, redirect: Option<&str>, state: Option<&str>) -> String {
        // Twitter 重写实现，保证参数顺序和要求一致
        use crate::utils::{StateData, generate_pkce_verifier, generate_pkce_challenge};
        use data_encoding::BASE64URL_NOPAD;
        
        // 临时生成 PKCE verifier 并编码到 state 中（无状态模式）
        let verifier = generate_pkce_verifier();
        let challenge = generate_pkce_challenge(&verifier);
        
        let mut url = format!(
            "{}?response_type=code&client_id={}",
            self.client.config.auth_url,
            urlencoding::encode(&self.client.config.client_id)
        );

        // 构建回调地址
        if let Some(ref callback_path) = self.client.config.callback_path {
            let callback_url = format!(
                "{}{}",
                server_url.trim_end_matches('/'),
                callback_path
            );
            
            // 构建 state 数据 - 将 verifier 直接存入 state，不依赖实例存储
            let state_data = StateData {
                redirect: redirect.unwrap_or("").to_string(),
                state: state.unwrap_or("").to_string(),
                verifier: Some(verifier),
                callback_url: Some(callback_url.clone()),
            };
            
            // 编码 state
            let state_json = serde_json::to_string(&state_data).unwrap_or_default();
            let encoded_state = BASE64URL_NOPAD.encode(state_json.as_bytes());
            
            // Twitter 要求的参数顺序
            url.push_str(&format!("&redirect_uri={}", urlencoding::encode(&callback_url)));
            url.push_str(&format!("&scope={}", urlencoding::encode(&self.client.config.scopes)));
            url.push_str(&format!("&state={}", urlencoding::encode(&encoded_state)));
            
            // PKCE 参数
            url.push_str(&format!("&code_challenge={}", challenge));
            url.push_str("&code_challenge_method=S256");
        }

        url
    }

    async fn get_access_token(&self, code: &str, _server_url: &str, _redirect: Option<&str>, state_param: Option<&str>) -> Result<TokenResponse> {
        // Twitter OAuth2 需要使用 Basic Auth，而不是表单参数传递 client_secret
        let mut params = vec![
            ("code", code.to_string()),
            ("grant_type", "authorization_code".to_string()),
        ];

        // 从 state 中提取 verifier 和 callback_url
        if let Some(encoded_state) = state_param {
            if let Ok(state_data) = crate::utils::decode_state(encoded_state) {
                if let Some(verifier) = state_data.verifier {
                    params.push(("code_verifier", verifier));
                }
                if let Some(callback_url) = state_data.callback_url {
                    params.push(("redirect_uri", callback_url));
                }
            }
        }

        // 构建 Basic Auth 凭证
        let auth = base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            format!("{}:{}", self.client.config.client_id, self.client.config.client_secret)
        );

        let res = self.client.http_client().post(&self.client.config.token_url)
            .form(&params)
            .header("Accept", "application/json")
            .header("Authorization", format!("Basic {}", auth))
            .send()
            .await?;

        let text = res.text().await?;
        self.client.parse_token_response(&text)
    }

    async fn get_user_info(&self, token: &TokenResponse) -> Result<UserInfo> {
        // Twitter API v2 需要显式请求额外字段
        let url = format!(
            "{}?user.fields=name,confirmed_email,username,profile_image_url,url",
            self.client.config.user_info_url
        );
        
        let raw = self.client.http_client().get(&url)
            .bearer_auth(&token.access_token)
            .send()
            .await?
            .json::<serde_json::Value>()
            .await?;
            
        Ok(self.client.normalize_user_info(raw))
    }
}
