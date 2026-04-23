use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::env;
use std::fs;

/// 定义环境变量提供者接口
pub trait EnvProvider {
    fn var(&self, key: &str) -> Option<String>;
}

/// 生产环境实现：读取系统环境变量
pub struct OsEnv;
impl EnvProvider for OsEnv {
    fn var(&self, key: &str) -> Option<String> {
        env::var(key).ok()
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub server: ServerConfig,
    pub providers: HashMap<String, ProviderConfig>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ServerConfig {
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default)]
    pub debug: bool,
    #[serde(default)]
    pub server_url: Option<String>,
}

fn default_port() -> u16 {
    3000
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct ProviderConfig {
    pub client_id: String,
    pub client_secret: String,
    #[serde(default)]
    pub issuer: Option<String>,
    #[serde(default)]
    pub auth_url: Option<String>,
    #[serde(default)]
    pub token_url: Option<String>,
    #[serde(default)]
    pub userinfo_url: Option<String>,
    #[serde(default)]
    pub scopes: Option<String>,
}

/// 提供商名称到环境变量前缀的映射
const PROVIDER_ENV_PREFIXES: &[(&str, &str, &str)] = &[
    ("github", "GITHUB_ID", "GITHUB_SECRET"),
    ("google", "GOOGLE_ID", "GOOGLE_SECRET"),
    ("twitter", "TWITTER_ID", "TWITTER_SECRET"),
    ("twitter", "X_ID", "X_SECRET"),
    ("weibo", "WEIBO_ID", "WEIBO_SECRET"),
    ("qq", "QQ_ID", "QQ_SECRET"),
    ("oidc", "OIDC_ID", "OIDC_SECRET"),
    ("huawei", "HUAWEI_ID", "HUAWEI_SECRET"),
];

pub fn load_config(path: &str) -> Result<Config> {
    load_config_with_env(path, &OsEnv)
}

/// 支持注入 EnvProvider 的配置加载
pub fn load_config_with_env(path: &str, env: &impl EnvProvider) -> Result<Config> {
    let config_path = if path == "config.toml" || path == "config.json" {
        env.var("CONFIG_FILE").unwrap_or_else(|| "config.toml".to_string())
    } else {
        path.to_string()
    };

    let mut config: Config = match fs::read_to_string(&config_path) {
        Ok(content) => toml::from_str(&content)
            .with_context(|| format!("Failed to parse TOML config: {}", config_path))?,
        Err(_) => {
            // 文件不存在时，返回默认配置
            Config {
                server: ServerConfig {
                    host: "0.0.0.0".to_string(),
                    port: 3000,
                    debug: false,
                    server_url: None,
                },
                providers: HashMap::new(),
            }
        }
    };

    apply_env_overrides(&mut config, env)?;

    Ok(config)
}

fn apply_env_overrides(config: &mut Config, env: &impl EnvProvider) -> Result<()> {
    for (provider_name, id_env, secret_env) in PROVIDER_ENV_PREFIXES {
        let id_val = env.var(id_env);
        let secret_val = env.var(secret_env);

        if id_val.is_some() || secret_val.is_some() {
            let provider_cfg = config.providers.entry(provider_name.to_string())
                .or_insert_with(ProviderConfig::default);
            
            if let Some(id) = id_val {
                provider_cfg.client_id = id;
            }
            if let Some(secret) = secret_val {
                provider_cfg.client_secret = secret;
            }
        }
    }

    for (name, provider_cfg) in config.providers.iter_mut() {
        let prefix = name.to_uppercase();
        
        if let Some(scopes) = env.var(&format!("{}_SCOPES", prefix)) {
            provider_cfg.scopes = Some(scopes);
        }
        if let Some(auth_url) = env.var(&format!("{}_AUTH_URL", prefix)) {
            provider_cfg.auth_url = Some(auth_url);
        }
        if let Some(token_url) = env.var(&format!("{}_TOKEN_URL", prefix)) {
            provider_cfg.token_url = Some(token_url);
        }
        if let Some(userinfo_url) = env.var(&format!("{}_USERINFO_URL", prefix)) {
            provider_cfg.userinfo_url = Some(userinfo_url);
        }
        if name == "oidc" {
            if let Some(issuer) = env.var("OIDC_ISSUER") {
                provider_cfg.issuer = Some(issuer);
            }
        }
    }

    if let Some(port_str) = env.var("PORT") {
        if let Ok(port) = port_str.parse::<u16>() {
            config.server.port = port;
        }
    }
    
    if let Some(server_url) = env.var("SERVER_URL") {
        config.server.server_url = Some(server_url);
    }

    if let Some(debug_str) = env.var("DEBUG") {
        config.server.debug = debug_str == "true" || debug_str == "1";
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试用的环境变量模拟器
    struct MapEnv(HashMap<String, String>);
    impl EnvProvider for MapEnv {
        fn var(&self, key: &str) -> Option<String> {
            self.0.get(key).cloned()
        }
    }

    #[test]
    fn test_config_loading() {
        let config_path = "test_config.toml";
        let content = r#"
[server]
port = 8080
host = "127.0.0.1"

[providers.github]
client_id = "test_id"
client_secret = "test_secret"
"#;
        std::fs::write(config_path, content).unwrap();

        let env = MapEnv(HashMap::new());
        let config = load_config_with_env(config_path, &env).unwrap();
        assert_eq!(config.server.port, 8080);
        assert_eq!(config.server.host, "127.0.0.1");
        assert_eq!(config.providers["github"].client_id, "test_id");

        std::fs::remove_file(config_path).unwrap();
    }

    #[test]
    fn test_config_with_oidc_fields() {
        let config_path = "test_oidc_config.toml";
        let content = r#"
[server]
port = 8080
host = "127.0.0.1"

[providers.oidc]
client_id = "oidc_id"
client_secret = "oidc_secret"
issuer = "https://accounts.google.com"
scopes = "openid profile email"
"#;
        std::fs::write(config_path, content).unwrap();

        let env = MapEnv(HashMap::new());
        let config = load_config_with_env(config_path, &env).unwrap();
        let oidc = &config.providers["oidc"];
        assert_eq!(oidc.client_id, "oidc_id");
        assert_eq!(oidc.issuer.as_deref(), Some("https://accounts.google.com"));
        assert_eq!(oidc.scopes.as_deref(), Some("openid profile email"));

        std::fs::remove_file(config_path).unwrap();
    }
}
