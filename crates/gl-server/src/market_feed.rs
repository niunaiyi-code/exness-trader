//! 全局行情广播器（Pub/Sub）。
//! 连接 Exness Ticks WebSocket（探测账户）并扇出到 ws_hub（前端）
//! 以及原始 tick 广播（策略运行器）。探测为只读 — 不交易。

use std::sync::Arc;
use std::time::Duration;
use exness_api::{Client, ClientConfig, Credentials, Tick, TickStreamClient};
use tokio::sync::broadcast;
use url::Url;

use crate::config::MarketFeedConfig;
use crate::ws::{WsFrame, WsHub};

pub struct MarketFeedHub {
    tick_tx: broadcast::Sender<Tick>,
}

impl MarketFeedHub {
    pub fn new() -> Self {
        let (tick_tx, _) = broadcast::channel(1024);
        Self { tick_tx }
    }

    /// 供策略运行器使用的原始 tick 流。
    pub fn subscribe_ticks(&self) -> broadcast::Receiver<Tick> {
        self.tick_tx.subscribe()
    }

    /// 原始 tick 流的发送端（用于接入 `AppState`）。
    pub fn tick_sender(&self) -> broadcast::Sender<Tick> {
        self.tick_tx.clone()
    }

    pub fn start(&self, config: MarketFeedConfig, ws_hub: Arc<WsHub>, gateway_url: String) {
        // USD 探测：ticks
        if let (Some(account_id), Some(api_key), Some(private_key)) = (
            config.usd_account_id.clone(),
            config.usd_api_key.clone(),
            config.usd_private_key.clone(),
        ) {
            let ws_hub = ws_hub.clone();
            let tick_tx = self.tick_tx.clone();
            let gw = gateway_url.clone();
            tokio::spawn(async move {
                Self::run_tick_stream(account_id, api_key, private_key, gw, "XAUUSD".to_string(), "USD".to_string(), ws_hub, tick_tx).await;
            });
        }

        // USC 探测：ticks
        if let (Some(account_id), Some(api_key), Some(private_key)) = (
            config.usc_account_id,
            config.usc_api_key,
            config.usc_private_key,
        ) {
            let ws_hub = ws_hub.clone();
            let tick_tx = self.tick_tx.clone();
            let gw = gateway_url.clone();
            tokio::spawn(async move {
                Self::run_tick_stream(account_id, api_key, private_key, gw, "XAUUSD".to_string(), "USC".to_string(), ws_hub, tick_tx).await;
            });
        }
    }

    async fn build_client(account_id: &str, api_key: &str, private_key: &str, gateway_url: &str) -> Option<Arc<Client>> {
        let creds = match Credentials::new(api_key, private_key) {
            Ok(c) => c,
            Err(e) => {
                tracing::error!(account_id, error = %e, "invalid market-feed credentials");
                return None;
            }
        };
        let cfg = ClientConfig {
            gateway_url: Url::parse(gateway_url).ok(),
            ..Default::default()
        };
        let client = match Client::new(creds, account_id, cfg) {
            Ok(c) => Arc::new(c),
            Err(e) => {
                tracing::error!(account_id, error = %e, "failed to build market-feed client");
                return None;
            }
        };
        loop {
            match client.ensure_access_point().await {
                Ok(_) => break,
                Err(e) => {
                    tracing::error!(account_id, error = %e, "market-feed discovery failed, retrying in 5s");
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
        }
        Some(client)
    }

    async fn run_tick_stream(
        account_id: String,
        api_key: String,
        private_key: String,
        gateway_url: String,
        symbol: String,
        currency: String,
        ws_hub: Arc<WsHub>,
        tick_tx: broadcast::Sender<Tick>,
    ) {
        let Some(client) = Self::build_client(&account_id, &api_key, &private_key, &gateway_url).await else {
            return;
        };
        let tick_client = TickStreamClient::new(client, vec![symbol.clone()]);
        let acc_id = account_id;
        let cur = currency.clone();
        // `run` 会永久重连；每个 tick 广播给运行器和前端。
        let _ = tick_client
            .run(move |tick| {
                let _ = tick_tx.send(tick.clone());
                ws_hub.broadcast(WsFrame::Tick {
                    account_id: acc_id.clone(),
                    symbol: Some(symbol.clone()),
                    currency: Some(cur.clone()),
                    bid: format!("{:.3}", tick.bid),
                    ask: format!("{:.3}", tick.ask),
                    timestamp: tick.time_ms() as u64,
                });
            })
            .await;
    }
}
