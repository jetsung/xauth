use crate::provider::{AuthProvider, TokenResponse, UserInfo};
use crate::oauth2::{OAuth2Client, OAuth2Config};
use async_trait::async_trait;
use anyhow::Result;

pub struct GoogleProvider {
    client: OAuth2Client,
}

impl GoogleProvider {
    pub fn new(client_id: String, client_secret: String) -> Self {
        let config = OAuth2Config {
            client_id,
            client_secret,
            auth_url: "https://accounts.google.com/o/oauth2/v2/auth".to_string(),
            token_url: "https://oauth2.googleapis.com/token".to_string(),
            user_info_url: "https://www.googleapis.com/oauth2/v2/userinfo".to_string(),
            scopes: "https://www.googleapis.com/auth/userinfo.email https://www.googleapis.com/auth/userinfo.profile".to_string(),
            callback_path: Some("/google".to_string()),
            use_pkce: false,
        };
        Self {
            client: OAuth2Client::new(config),
        }
    }
}

#[async_trait]
impl AuthProvider for GoogleProvider {
    fn is_available(&self) -> bool {
        self.client.is_available()
    }

    fn get_origin(&self) -> String {
        "accounts.google.com".to_string()
    }

    async fn get_redirect_url(&self, server_url: &str, redirect: Option<&str>, state: Option<&str>) -> String {
        // Google 把 redirect+state 放在 state 参数里，redirect_uri 只是 server_url/google
        let redirect_uri = format!("{}/google", server_url.trim_end_matches('/'));
        let state_value = format!(
            "redirect={}&state={}",
            urlencoding::encode(redirect.unwrap_or("")),
            urlencoding::encode(state.unwrap_or(""))
        );

        format!(
            "{}?client_id={}&redirect_uri={}&scope={}&response_type=code&access_type=offline&prompt=consent&state={}",
            self.client.config.auth_url,
            urlencoding::encode(&self.client.config.client_id),
            urlencoding::encode(&redirect_uri),
            urlencoding::encode(&self.client.config.scopes),
            urlencoding::encode(&state_value),
        )
    }

    async fn get_access_token(&self, code: &str, server_url: &str, _redirect: Option<&str>, _state: Option<&str>) -> Result<TokenResponse> {
        self.client.get_access_token(code, server_url, _redirect, _state).await
    }

    async fn get_user_info(&self, token: &TokenResponse) -> Result<UserInfo> {
        self.client.get_user_info(token).await
    }
}
