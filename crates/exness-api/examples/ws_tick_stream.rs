//! 示例：通过 WebSocket 订阅实时 tick 流
//!
//! 用法：
//!   EXNESS_API_KEY=<key_id> EXNESS_PRIVATE_KEY=<hex> EXNESS_ACCOUNT_ID=<id> \
//!   cargo run --example ws_tick_stream

use anyhow::Result;
use exness_api::{Credentials, Client, ClientConfig, ws::TickStreamClient};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let api_key = std::env::var("EXNESS_API_KEY")?;
    let private_key_hex = std::env::var("EXNESS_PRIVATE_KEY")?;
    let account_id = std::env::var("EXNESS_ACCOUNT_ID")?;

    let creds = Credentials::new(api_key, &private_key_hex)?;
    let gateway_url = std::env::var("EXNESS_GATEWAY_URL")
        .unwrap_or_else(|_| "https://api.exness.com".to_string());
    let cfg = ClientConfig {
        gateway_url: Some(url::Url::parse(&gateway_url)?),
        ..Default::default()
    };
    let client = Arc::new(Client::new(creds, &account_id, cfg)?);
    println!("Discovering access point for account {} ...", account_id);
    let ap = client.ensure_access_point().await?;
    println!("Access point discovered: {}", ap);
    let tick_client = TickStreamClient::new(client, vec!["XAUUSD".to_string()]);

    println!("Connecting to Exness WebSocket tick stream...");
    println!("Listening for XAUUSD ticks (press Ctrl+C to stop)...");

    tick_client.run(|tick| {
        println!(
            "[{}] {} bid={} ask={} spread={:.5}",
            tick.time, tick.symbol, tick.bid, tick.ask,
            (tick.ask - tick.bid).abs()
        );
    }).await?;

    Ok(())
}
