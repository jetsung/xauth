mod config;
mod provider;
mod oauth2;
mod github;
mod google;
mod huawei;
mod qq;
mod twitter;
mod weibo;
mod utils;
mod oidc;

use axum::{
    extract::{Query, State, Request},
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use std::{net::SocketAddr, sync::Arc};
use tracing::info;
use dotenvy::dotenv;
use crate::provider::AuthProvider;
use crate::github::GitHubProvider;
use crate::google::GoogleProvider;
use crate::huawei::HuaweiProvider;
use crate::qq::QQProvider;
use crate::twitter::TwitterProvider;
use crate::weibo::WeiboProvider;
use crate::oidc::OidcProvider;
use serde_json::json;

#[derive(Clone)]
pub struct AppState {
    pub providers: Arc<std::collections::HashMap<String, Box<dyn AuthProvider + Send + Sync>>>,
    pub provider_info: Arc<Vec<serde_json::Value>>,
    pub debug: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    use tracing_subscriber::EnvFilter;

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info"))
        )
        .init();

    dotenv().ok();

    info!("Starting Auth Service...");

    let conf = config::load_config("config.toml")?;

    let mut providers: std::collections::HashMap<String, Box<dyn AuthProvider + Send + Sync>> = std::collections::HashMap::new();
    let mut provider_info = Vec::new();

    for (name, cfg) in conf.providers {
        let provider: Box<dyn AuthProvider + Send + Sync> = match name.as_str() {
            "github" => Box::new(GitHubProvider::new(cfg.client_id, cfg.client_secret)),
            "google" => Box::new(GoogleProvider::new(cfg.client_id, cfg.client_secret)),
            "huawei" => Box::new(HuaweiProvider::new(cfg.client_id, cfg.client_secret)),
            "qq" => Box::new(QQProvider::new(cfg.client_id, cfg.client_secret)),
            "twitter" => Box::new(TwitterProvider::new(cfg.client_id, cfg.client_secret)),
            "weibo" => Box::new(WeiboProvider::new(cfg.client_id, cfg.client_secret)),
            "oidc" => {
                let oidc_config = crate::oidc::OidcConfig {
                    issuer: cfg.issuer,
                    auth_url: cfg.auth_url,
                    token_url: cfg.token_url,
                    userinfo_url: cfg.userinfo_url,
                    scopes: cfg.scopes,
                };
                Box::new(OidcProvider::new(cfg.client_id, cfg.client_secret, oidc_config))
            },
            _ => continue,
        };

        if provider.is_available() {
            let origin = provider.get_origin();
            provider_info.push(json!({"name": name.clone(), "origin": origin}));
            providers.insert(name, provider);
        }
    }

    let state = AppState {
        providers: Arc::new(providers),
        provider_info: Arc::new(provider_info),
        debug: conf.server.debug,
    };

    let app = Router::new()
        .route("/", get(handle_index))
        .route("/{provider}", get(handle_provider_request))
        .route("/wb_{*domain}", get(handle_weibo_verification))
        .layer(tower_http::trace::TraceLayer::new_for_http())
        .with_state(state);

    let addr = format!("{}:{}", conf.server.host, conf.server.port)
        .parse::<SocketAddr>()?;

    info!("Server listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

async fn handle_index(
    State(state): State<AppState>,
) -> Json<serde_json::Value> {
    Json(json!({
        "version": "0.1.0",
        "services": *state.provider_info
    }))
}

async fn handle_provider_request(
    State(state): State<AppState>,
    axum::extract::Path(provider_name): axum::extract::Path<String>,
    Query(query): Query<std::collections::HashMap<String, String>>,
    req: Request,
) -> impl IntoResponse {
    // 优先使用 SERVER_URL 环境变量
    let server_url = if let Ok(url) = std::env::var("SERVER_URL") {
        url
    } else {
        let proto = req.headers().get("x-forwarded-proto")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("http");  // 匹配 Node.js: this.ctx.header['x-forwarded-proto'] || 'http'
        let host = req.headers().get("x-forwarded-host")
            .and_then(|v| v.to_str().ok())
            .or_else(|| req.headers().get("host").and_then(|v| v.to_str().ok()))
            .unwrap_or("localhost");
        format!("{}://{}", proto, host)
    };

    let user_agent = req.headers().get("user-agent")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let is_waline = user_agent == "@waline";

    if state.debug {
        println!("[DEBUG] provider: {}", provider_name);
        println!("[DEBUG] query: {:?}", query);
        println!("[DEBUG] server_url: {}", server_url);
        println!("[DEBUG] user_agent: {}", user_agent);
    }

    let provider = match state.providers.get(&provider_name.to_lowercase()) {
        Some(p) => p,
        None => return (axum::http::StatusCode::NOT_FOUND, Json(json!({"errno": 404, "message": "Provider not found"}))).into_response(),
    };

    // 判断是 OAuth 回调（有 code）还是初始重定向（无 code）
    if let Some(code) = query.get("code") {
        // CALLBACK: 有 code → 匹配 Node.js base.js 的逻辑
        // 微博特殊逻辑：先解析 state 中的 redirect 和 type
        let mut redirect = query.get("redirect").cloned();
        let mut state_val = query.get("state").cloned().unwrap_or_default();
        let mut type_val: Option<String> = None;

        if provider_name == "weibo" {
            if let Some(raw_state) = query.get("state") {
                let state_params: std::collections::HashMap<String, String> =
                    serde_urlencoded::from_str(raw_state).unwrap_or_default();

                // 从 state 中提取 redirect, state 和 type
                if let Some(state_redirect) = state_params.get("redirect").filter(|s| !s.is_empty()) {
                    redirect = Some(state_redirect.clone());
                }
                if let Some(s) = state_params.get("state") {
                    state_val = s.clone();
                }
                if let Some(t) = state_params.get("type").filter(|s| !s.is_empty()) {
                    type_val = Some(t.clone());
                }
            }
        }

        // 优先检查 direct redirect 参数（GitHub, Weibo, QQ, Huawei, OIDC）
        if let Some(redirect_url) = redirect {
            if !is_waline {
                let mut final_redirect = redirect_url;

                // 微博添加 type 参数
                if provider_name == "weibo" {
                    if let Some(t) = type_val {
                        let separator = if final_redirect.contains('?') { "&" } else { "?" };
                        final_redirect = format!("{}{}type={}", final_redirect, separator, t);
                    }
                }

                let separator = if final_redirect.contains('?') { "&" } else { "?" };
                let url = format!("{}{}code={}&state={}", final_redirect, separator, code, state_val);
                return (
                    axum::http::StatusCode::FOUND,
                    [(axum::http::header::LOCATION, url)]
                ).into_response();
            }
        }

        // Check state-encoded redirect (Google, X)
        // Google: state format is "redirect=xxx&state=xxx" (URL encoded)
        // X: state is base64-encoded JSON {redirect, state}
        if let Some(raw_state) = query.get("state") {
            if !is_waline {
                // 尝试解析为 redirect=xxx&state=xxx 格式
                let state_params: std::collections::HashMap<String, String> =
                    serde_urlencoded::from_str(raw_state).unwrap_or_default();

                if let Some(redirect) = state_params.get("redirect").filter(|s| !s.is_empty()) {
                    let state_val = state_params.get("state").cloned().unwrap_or_default();
                    let separator = if redirect.contains('?') { "&" } else { "?" };
                    let url = format!("{}{}code={}&state={}", redirect, separator, code, state_val);
                    return (
                        axum::http::StatusCode::FOUND,
                        [(axum::http::header::LOCATION, url)]
                    ).into_response();
                }
                // 尝试解析为 X 的 base64 格式
                if let Ok(state_data) = crate::utils::decode_state(raw_state) {
                    if !state_data.redirect.is_empty() {
                        let separator = if state_data.redirect.contains('?') { "&" } else { "?" };
                        let url = format!("{}{}code={}&state={}", state_data.redirect, separator, code, raw_state);
                        return (
                            axum::http::StatusCode::FOUND,
                            [(axum::http::header::LOCATION, url)]
                        ).into_response();
                    }
                }
            }
        }

        // 无 redirect → 获取用户信息并返回 JSON
        let redirect = query.get("redirect").map(|s| s.as_str());
        let state_query = query.get("state").map(|s| s.as_str());
        let token = match provider.get_access_token(code, &server_url, redirect, state_query).await {
            Ok(t) => t,
            Err(e) => {
                return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"errno": 500, "message": e.to_string()}))).into_response();
            }
        };

        if state.debug {
            println!("[DEBUG] token: {:?}", token);
        }

        let user_info = match provider.get_user_info(&token).await {
            Ok(u) => u,
            Err(e) => {
                return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"errno": 500, "message": e.to_string()}))).into_response();
            }
        };

        if state.debug {
            println!("[DEBUG] user_info: {:?}", user_info);
        }

        Json(user_info).into_response()
    } else {
        // INITIAL REDIRECT: 无 code → 重定向到 OAuth 提供商
        let redirect = query.get("redirect").map(|s| s.as_str());
        let state_param_query = query.get("state").map(|s| s.as_str());
        let url = provider.get_redirect_url(&server_url, redirect, state_param_query).await;
        if state.debug {
            println!("[DEBUG] redirect_url: {}", url);
        }
        (
            axum::http::StatusCode::FOUND,
            [(axum::http::header::LOCATION, url)]
        ).into_response()
    }
}

async fn handle_weibo_verification() -> impl IntoResponse {
    "open.weibo.com".into_response()
}
