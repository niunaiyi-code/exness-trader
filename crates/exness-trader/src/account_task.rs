//! 每账户任务：独立执行单元（Tokio Task）
//!
//! 每个任务：
//! - 持有自身的账户状态
//! - 独立运行 M1 tick 探测
//! - 自行管理定时器与重连
//! - 不阻塞其他账户

use anyhow::Result;
use exness_api::{
    Client, ClientConfig, Credentials,
    ws::TickStreamClient,
    models::CandleQuery,
};
use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinHandle;
use tracing::{info, warn, error};

use crate::config::AccountConfig;

pub struct AccountTask;

impl AccountTask {
    /// 为该账户启动一个独立任务
    pub fn spawn(cfg: AccountConfig) -> JoinHandle<Result<()>> {
        tokio::spawn(async move {
            Self::run(cfg).await
        })
    }

    async fn run(cfg: AccountConfig) -> Result<()> {
        info!("[{}] Starting account task (ID: {})", cfg.name, cfg.account_id);

        // 构建凭据与客户端
        let creds = Credentials::new(&cfg.api_key, &cfg.private_key)?;
        let client = Arc::new(Client::new(creds, &cfg.account_id, ClientConfig::default())?);

        // 启动 M1 探测定时器（独立于 WebSocket）
        let probe_client = Arc::clone(&client);
        let probe_symbols = cfg.symbols.clone();
        let probe_interval = Duration::from_secs(cfg.probe_interval_secs);
        let probe_task = tokio::spawn(async move {
            Self::run_m1_probe(probe_client, probe_symbols, probe_interval).await
        });

        // 启动 WebSocket tick 流
        let ws_client = TickStreamClient::new(Arc::clone(&client), cfg.symbols.clone());
        let ws_task = tokio::spawn(async move {
            ws_client.run(|tick| {
                info!("[WS] {} bid={} ask={}", tick.symbol, tick.bid, tick.ask);
            }).await
        });

        // 等待任一任务失败（两者本应一直运行）
        tokio::select! {
            res = probe_task => {
                warn!("[{}] M1 probe task exited: {:?}", cfg.name, res);
            }
            res = ws_task => {
                warn!("[{}] WebSocket task exited: {:?}", cfg.name, res);
            }
        }

        Ok(())
    }

    async fn run_m1_probe(
        client: Arc<Client>,
        symbols: Vec<String>,
        interval: Duration,
    ) -> Result<()> {
        let mut ticker = tokio::time::interval(interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            ticker.tick().await;

            for symbol in &symbols {
                // 请求最近 1 根 M1 K 线
                let query = CandleQuery {
                    instrument: symbol.clone(),
                    price_type: "BID".to_string(),
                    timeframe: "M1".to_string(),
                    from: chrono::Utc::now() - chrono::Duration::minutes(5),
                    to: None,
                    count: Some(1),
                };

                match client.candles(&query).await {
                    Ok(candles) => {
                        if let Some(last) = candles.last() {
                            info!(
                                "[M1] {} time={} o={} h={} l={} c={} vol={}",
                                symbol, last.time, last.open, last.high, last.low, last.close, last.volume
                            );
                        }
                    }
                    Err(e) => {
                        error!("[M1] Failed to fetch {} candles: {:?}", symbol, e);
                    }
                }
            }
        }
    }
}
