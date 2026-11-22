//! 从环境变量加载配置

use anyhow::{Context, Result};

#[derive(Debug, Clone)]
pub struct Config {
    pub accounts: Vec<AccountConfig>,
}

#[derive(Debug, Clone)]
pub struct AccountConfig {
    pub name: String,
    pub account_id: String,
    pub api_key: String,
    pub private_key: String,
    pub symbols: Vec<String>,
    pub probe_interval_secs: u64,
}

impl Config {
    /// 从环境变量加载：
    /// - EXNESS_ACCOUNTS=acc1,acc2,acc3
    /// - EXNESS_ACC1_ID、EXNESS_ACC1_API_KEY、EXNESS_ACC1_PRIVATE_KEY、EXNESS_ACC1_SYMBOLS（逗号分隔）
    /// - EXNESS_ACC1_PROBE_INTERVAL（可选，默认 1 秒）
    pub fn from_env() -> Result<Self> {
        let accounts_str = std::env::var("EXNESS_ACCOUNTS")
            .context("EXNESS_ACCOUNTS not set (expected comma-separated list like 'demo,prod')")?;

        let account_names: Vec<String> = accounts_str
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        anyhow::ensure!(!account_names.is_empty(), "EXNESS_ACCOUNTS is empty");

        let mut accounts = Vec::new();
        for name in account_names {
            let prefix = format!("EXNESS_{}_", name.to_uppercase());
            let account_id = std::env::var(format!("{}ID", prefix))
                .with_context(|| format!("{}ID not set", prefix))?;
            let api_key = std::env::var(format!("{}API_KEY", prefix))
                .with_context(|| format!("{}API_KEY not set", prefix))?;
            let private_key = std::env::var(format!("{}PRIVATE_KEY", prefix))
                .with_context(|| format!("{}PRIVATE_KEY not set", prefix))?;
            let symbols_str = std::env::var(format!("{}SYMBOLS", prefix))
                .with_context(|| format!("{}SYMBOLS not set", prefix))?;
            let symbols: Vec<String> = symbols_str
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();

            let probe_interval_secs = std::env::var(format!("{}PROBE_INTERVAL", prefix))
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(1);

            accounts.push(AccountConfig {
                name,
                account_id,
                api_key,
                private_key,
                symbols,
                probe_interval_secs,
            });
        }

        Ok(Config { accounts })
    }
}
