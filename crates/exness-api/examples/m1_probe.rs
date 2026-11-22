//! 针对（模拟）账户的 M1 只读探测。绝不下单。
//!
//! 环境变量：
//!   EXNESS_GATEWAY_URL      来自密钥管理的网关地址（例如 https://gw-xxxx.exness.com）
//!   EXNESS_ACCOUNT_ID       交易账号
//!   EXNESS_API_KEY          exnsk-…
//!   EXNESS_PRIVATE_KEY      128 位十六进制 / 64 位十六进制 / PEM   （或 EXNESS_PRIVATE_KEY_FILE=路径）
//!   EXNESS_REST_BASE_URL    可选：跳过接入点发现（仅测试用）
//!   PROBE_TICKS             打印多少个 tick（默认 10）
//!
//! Run: cargo run -p exness-api --example m1_probe

use std::time::{Duration, Instant};

use anyhow::{bail, Context};
use chrono::Utc;
use exness_api::models::*;
use exness_api::{Client, ClientConfig, Credentials, WsConnection, WsStream};
use url::Url;

const GOLD_CANDIDATES: [&str; 3] = ["XAUUSD", "XAUUSDm", "XAUUSDc"];

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env().add_directive("exness_api=debug".parse()?))
        .init();

    let account_id = env("EXNESS_ACCOUNT_ID")?;
    let api_key = env("EXNESS_API_KEY")?;
    let private_key = match std::env::var("EXNESS_PRIVATE_KEY_FILE") {
        Ok(p) => std::fs::read_to_string(&p).with_context(|| format!("read {p}"))?,
        Err(_) => env("EXNESS_PRIVATE_KEY")?,
    };
    let creds = Credentials::new(api_key, &private_key)?;
    println!("public key (hex) : {}", creds.public_key_hex());
    println!("public key (SPKI): {}", creds.public_key_spki_base64());

    let cfg = ClientConfig {
        gateway_url: std::env::var("EXNESS_GATEWAY_URL").ok().map(|s| Url::parse(&s)).transpose()?,
        rest_base_url: std::env::var("EXNESS_REST_BASE_URL").ok().map(|s| Url::parse(&s)).transpose()?,
        ..Default::default()
    };
    let client = Client::new(creds, account_id.clone(), cfg)?;

    // 1. 接入点
    section("1. access point");
    if client.access_point().is_none() {
        let ap = client.discover_access_point().await.context("access-point discovery")?;
        println!("{ap:#?}");
    } else {
        println!("using EXNESS_REST_BASE_URL = {}", client.access_point().unwrap());
    }

    // 2. 账户
    section("2. account details");
    let acc = client.account_details().await.context("account details")?;
    println!("{acc:#?}");
    println!("clock skew (server-local) = {} ms", client.clock_skew_ms());
    if acc.settings.trade_mode != Some(TradeMode::Enabled) {
        println!("!! trade_mode is {:?}: trading streams/snapshot will be unavailable", acc.settings.trade_mode);
    }

    // 3. 交易品种 → 匹配黄金
    section("3. instruments");
    let instruments = client.instruments().await.context("instruments")?;
    println!("{} instruments available", instruments.len());
    let gold = GOLD_CANDIDATES.iter().find(|c| instruments.iter().any(|i| i == *c)).map(|s| s.to_string());
    let Some(gold) = gold else {
        let xau: Vec<_> = instruments.iter().filter(|i| i.starts_with("XAU")).collect();
        bail!("no XAUUSD/XAUUSDm/XAUUSDc on this account; XAU* present: {xau:?}");
    };
    println!("gold symbol = {gold}");

    // 4. 交易条件
    section("4. instrument conditions");
    let cond = client.instrument_conditions(&gold).await.context("conditions")?;
    println!(
        "  desc={:?} intl={:?} cat={:?} base/quote/margin={:?}/{:?}/{:?} calc_mode={:?} margin%={:?}",
        cond.description,
        cond.international_name,
        cond.category,
        cond.base_currency,
        cond.quote_currency,
        cond.margin_currency,
        cond.calc_mode,
        cond.margin_percent
    );
    println!(
        "  order_types={:?} filling_modes={:?} commission={:?}",
        cond.order_types, cond.filling_modes, cond.commission_settings
    );
    println!(
        "  swap_long={:?} swap_short={:?} swap_multipliers={:?}",
        cond.swap_long, cond.swap_short, cond.swap_multipliers
    );
    println!("  trade_sessions ({}):", cond.trade_sessions.len());
    for s in &cond.trade_sessions {
        println!(
            "    {:?}  {}  ->  {}",
            s.trade_mode,
            s.week_start_time.as_deref().unwrap_or("-"),
            s.week_end_time.as_deref().unwrap_or("-")
        );
    }
    println!(
        "=> exec_mode={:?} point_digits={:?} contract_size={:?} volume_min/step/max={:?}/{:?}/{:?} trade_mode={:?}",
        cond.exec_mode,
        cond.point_digits,
        cond.contract_size,
        cond.volume_min,
        cond.volume_step,
        cond.volume_max,
        cond.trade_mode
    );

    // 5. 交易限制
    section("5. limits");
    let limits = client.limits().await.context("limits")?;
    println!(
        "global REST: {} req / {}s",
        limits.rest.global_account_rate.limit, limits.rest.global_account_rate.window_seconds
    );
    for m in &limits.rest.methods {
        println!(
            "  {:<28} {:<6} {:>4}/{:<4}s {}",
            m.api_operation_id,
            m.http_method,
            m.rate_limit.limit,
            m.rate_limit.window_seconds,
            m.limits
                .as_ref()
                .and_then(|l| l.max_candles_per_request)
                .map(|c| format!("max_candles={c}"))
                .unwrap_or_default()
        );
    }
    let g = &limits.websocket.global_account_limits;
    println!(
        "WS global: max_conn={} conn_rate={}/{}s burst={} inbound={}/{}s strikes={}/{}ms",
        g.max_active_connections,
        g.connection_rate.limit,
        g.connection_rate.window_seconds,
        g.connection_rate.burst,
        g.inbound_message_rate.limit,
        g.inbound_message_rate.window_seconds,
        g.strikes.threshold,
        g.strikes.period_ms
    );

    // 6. 快照
    section("6. snapshot");
    let snap = client.snapshot().await.context("snapshot")?;
    println!("account_state = {:?}", snap.account_state);
    println!("{} open positions, {} orders", snap.positions.len(), snap.orders.len());
    for p in &snap.positions {
        println!(
            "  {} {:?} {} vol={} open={:?} swap={:?} comm={:?}",
            p.position_id, p.direction, p.instrument, p.volume, p.open_price, p.swap, p.commission
        );
    }

    // 7. K 线（当前时刻前最近 5 根 M1）
    section("7. candles");
    let candles = client
        .candles(&CandleQuery {
            instrument: gold.clone(),
            timeframe: Timeframe::M1,
            price_type: PriceType::Bid,
            from: Utc::now(),
            to: None,
            count: Some(-5),
        })
        .await
        .context("candles")?;
    for c in &candles {
        println!("  {} O={} H={} L={} C={} V={}", c.time, c.open, c.high, c.low, c.close, c.volume);
    }

    // 8. ticks WebSocket 订阅
    section("8. ticks websocket");
    let want: usize = std::env::var("PROBE_TICKS").ok().and_then(|s| s.parse().ok()).unwrap_or(10);
    let mut ws = WsConnection::connect(&client, WsStream::Ticks).await.context("ticks ws connect")?;
    let id = ws.subscribe_ticks(&[&gold]).await?;
    println!("subscribed (id={id}); waiting for {want} ticks (60s max)…");
    let started = Instant::now();
    let mut got = 0usize;
    let mut last: Option<chrono::DateTime<Utc>> = None;
    while got < want && started.elapsed() < Duration::from_secs(60) {
        let msg = tokio::time::timeout(Duration::from_secs(15), ws.next()).await;
        match msg {
            Err(_) => {
                println!("  (no message for 15s)");
                continue;
            }
            Ok(None) => bail!("ticks stream ended"),
            Ok(Some(Err(e))) => bail!("ticks stream error: {e}"),
            Ok(Some(Ok(WsInbound::Tick(t)))) => {
                let gap = last.map(|l| (t.timestamp - l).num_milliseconds()).unwrap_or(0);
                let age = (Utc::now() - t.timestamp).num_milliseconds();
                println!(
                    "  tick {} bid={} ask={} spread={:.3} gap={}ms age={}ms",
                    t.timestamp,
                    t.bid,
                    t.ask,
                    t.ask - t.bid,
                    gap,
                    age
                );
                last = Some(t.timestamp);
                got += 1;
            }
            Ok(Some(Ok(other))) => println!("  {other:?}"),
        }
    }
    ws.close().await.ok();

    // 9. events WebSocket → 快照基线 + 若干 account_state 事件
    section("9. events websocket");
    let mut ws = WsConnection::connect(&client, WsStream::Events).await.context("events ws connect")?;
    ws.subscribe(EventsSubscription::Transactions).await?;
    ws.subscribe(EventsSubscription::AccountState).await?;
    let started = Instant::now();
    let mut seen_snapshot = false;
    let mut acc_events = 0;
    while started.elapsed() < Duration::from_secs(12) {
        match tokio::time::timeout(Duration::from_secs(6), ws.next()).await {
            Err(_) => continue,
            Ok(None) => bail!("events stream ended"),
            Ok(Some(Err(e))) => bail!("events stream error: {e}"),
            Ok(Some(Ok(WsInbound::Event(ev)))) => match *ev {
                ServerEvent::TradingStateSnapshot { payload } => {
                    seen_snapshot = true;
                    println!(
                        "  trading_state_snapshot: {} positions, equity={}",
                        payload.positions.len(),
                        payload.account_state.equity
                    );
                }
                ServerEvent::AccountStateEvent { event_time, payload } => {
                    acc_events += 1;
                    println!(
                        "  account_state_event {:?} equity={} used_margin={}",
                        event_time, payload.account_state.equity, payload.account_state.used_margin
                    );
                }
                other => println!("  {other:?}"),
            },
            Ok(Some(Ok(other))) => println!("  {other:?}"),
        }
    }
    ws.close().await.ok();
    println!("snapshot seen = {seen_snapshot}, account_state events = {acc_events}");

    section("done");
    println!("Record for docs/04 & M1 report: exec_mode, point_digits, contract_size, tick cadence, limits.");
    Ok(())
}

fn env(name: &str) -> anyhow::Result<String> {
    std::env::var(name).with_context(|| format!("missing env {name}"))
}

fn section(title: &str) {
    println!("\n=== {title} ===");
}
