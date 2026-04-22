use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use anyhow::Result;

/// 用户信息统一格式
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInfo {
    pub id: String,
    pub name: String,
    pub email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avatar: Option<String>,
}

/// Token 响应统一格式
#[derive(Debug, Clone)]
pub struct TokenResponse {
    pub access_token: String,
    pub token_type: Option<String>,
    pub expires_in: Option<i64>,
    pub refresh_token: Option<String>,
    /// 华为专用：id_token
    pub id_token: Option<String>,
}

/// OAuth 提供商统一接口
#[async_trait]
pub trait AuthProvider: Send + Sync {
    /// 检查配置是否可用
    fn is_available(&self) -> bool;

    /// 获取提供商 origin (如 github.com)
    fn get_origin(&self) -> String;

    /// 生成重定向 URL
    /// 
    /// `server_url` - 认证服务器基础 URL（如 https://oauth.lithub.cc）
    /// `redirect` - 用户最终要跳转的目标 URL
    /// `state` - 可选的 state 参数
    async fn get_redirect_url(&self, server_url: &str, redirect: Option<&str>, state: Option<&str>) -> String;

    /// 获取 access token
    /// `code` - OAuth authorization code
    /// `server_url` - 授权时的 server_url（用于重建 redirect_uri）
    /// `redirect` - 授权时的 redirect 参数
    /// `state` - 授权时的 state 参数
    async fn get_access_token(&self, code: &str, server_url: &str, redirect: Option<&str>, state: Option<&str>) -> Result<TokenResponse>;

    /// 通过 token 获取用户信息
    async fn get_user_info(&self, token: &TokenResponse) -> Result<UserInfo>;
}
