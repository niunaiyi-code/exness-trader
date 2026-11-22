//! 实时 tick 流示例：通过 WebSocket 订阅 XAUUSD bid/ask 报价。
//!
//! 用法：
//! ```sh
//! EXNESS_ACCOUNT_ID=123456 \
//! EXNESS_API_KEY=exnsk-xxx \
//! EXNESS_PRIVATE_KEY=deadbeef... \
//! EXNESS_GATEWAY_URL=https://api-gw-xx.exness.com \
//! cargo run --example tick_stream
//! ```

use std::sync::Arc;

use anyhow::Context;
use exness_api::{Client, ClientConfig, Credentials, TickStreamClient};
use url::Url;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let account_id = std::env::var("EXNESS_ACCOUNT_ID")?;
    let creds = Credentials::new(
        std::env::var("EXNESS_API_KEY")?,
        &std::env::var("EXNESS_PRIVATE_KEY")?,
    )?;
    let cfg = ClientConfig {
        gateway_url: std::env::var("EXNESS_GATEWAY_URL")
            .ok()
            .map(|s| Url::parse(&s))
            .transpose()?,
        ..Default::default()
    };

    let client = Arc::new(Client::new(creds, account_id, cfg)?);

    // 发现接入点
    client.ensure_access_point().await?;

    // 订阅 XAUUSD ticks
    let symbols = vec!["XAUUSD".to_owned()];
    let tick_stream = TickStreamClient::new(client.clone(), symbols);

    println!("Starting tick stream for XAUUSD...");
    println!("Press Ctrl+C to stop\n");

    // 运行 tick 流（断线自动重连）
    tick_stream
        .run(|tick| {
            println!(
                "[{}] {} bid={:.2} ask={:.2} spread={:.2}",
                chrono::Utc::now().format("%H:%M:%S"),
                tick.symbol,
                tick.bid,
                tick.ask,
                tick.spread()
            );
        })
        .await?;

    Ok(())
}
