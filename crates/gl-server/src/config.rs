//! 服务端配置（docs/06 §1）。

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ServerConfig {
    pub server: HttpConfig,
    pub database: DatabaseConfig,
    pub exness: ExnessConfig,
    pub auth: AuthConfig,
    pub crypto: CryptoConfig,
    #[serde(default)]
    pub market_feed: MarketFeedConfig,
    #[serde(default)]
    pub calendar: CalendarConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct CalendarConfig {
    /// FinanceCalendar API key. Leave empty to disable calendar refresh.
    pub api_key: Option<String>,
    /// 日历 API 基地址；留空时使用默认生产地址（测试/内网部署可覆盖为本地 mock）。
    pub api_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct MarketFeedConfig {
    pub usd_account_id: Option<String>,
    pub usd_api_key: Option<String>,
    pub usd_private_key: Option<String>,
    pub usc_account_id: Option<String>,
    pub usc_api_key: Option<String>,
    pub usc_private_key: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HttpConfig {
    pub host: String,
    pub port: u16,
    pub static_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DatabaseConfig {
    pub url: String,
    pub max_connections: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ExnessConfig {
    /// 来自密钥管理的网关 URL（必填）。
    pub gateway_url: String,
    /// 测试用可选覆盖（生产环境留空）。
    pub rest_base_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AuthConfig {
    pub jwt_secret: String,
    pub jwt_expiry_hours: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CryptoConfig {
    /// 主加密密钥（32 字节 base64）。
    pub master_key: String,
}

impl ServerConfig {
    pub fn from_file(path: &str) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: ServerConfig = toml::from_str(&content)?;
        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_roundtrip() {
        let config = ServerConfig {
            server: HttpConfig {
                host: "127.0.0.1".to_string(),
                port: 8080,
                static_dir: Some(PathBuf::from("web/dist")),
            },
            database: DatabaseConfig {
                url: "sqlite:gl.db".to_string(),
                max_connections: 10,
            },
            exness: ExnessConfig {
                gateway_url: "https://api.exness.com".to_string(),
                rest_base_url: None,
            },
            auth: AuthConfig {
                jwt_secret: "test_secret".to_string(),
                jwt_expiry_hours: 24,
            },
            crypto: CryptoConfig {
                master_key: "dGVzdF9rZXlfdGVzdF9rZXlfdGVzdF9rZXlfdGVzdF9rZXk=".to_string(),
            },
            market_feed: MarketFeedConfig::default(),
            calendar: CalendarConfig::default(),
        };
        let toml_str = toml::to_string(&config).unwrap();
        let parsed: ServerConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.server.port, 8080);
    }
}
