//! M1 交易探测：在黄金品种上开 0.01 手 BUY 和 0.01 手 SELL 以验证对冲，随后
//! 平掉两侧。**会在配置的账户下真实下单** — 请使用模拟账户。
//!
//! 安全联锁：需要 `EXNESS_ALLOW_TRADING=1` 以及命令行参数 `--i-know-this-trades`。
//! 环境变量同 `m1_probe`。

use std::collections::HashMap;
use std::time::{Duration, Instant};

use anyhow::{bail, Context};
use exness_api::auth::generate_idempotency_key;
use exness_api::models::*;
use exness_api::{Client, ClientConfig, Credentials, WsConnection, WsStream};
use rust_decimal::Decimal;
use url::Url;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env().add_directive("exness_api=debug".parse()?))
        .init();

    if std::env::var("EXNESS_ALLOW_TRADING").as_deref() != Ok("1")
        || !std::env::args().any(|a| a == "--i-know-this-trades")
    {
        bail!("refusing to trade: set EXNESS_ALLOW_TRADING=1 and pass --i-know-this-trades (demo account only)");
    }

    let account_id = env("EXNESS_ACCOUNT_ID")?;
    let private_key = match std::env::var("EXNESS_PRIVATE_KEY_FILE") {
        Ok(p) => std::fs::read_to_string(&p).with_context(|| format!("read {p}"))?,
        Err(_) => env("EXNESS_PRIVATE_KEY")?,
    };
    let creds = Credentials::new(env("EXNESS_API_KEY")?, &private_key)?;
    let cfg = ClientConfig {
        gateway_url: std::env::var("EXNESS_GATEWAY_URL").ok().map(|s| Url::parse(&s)).transpose()?,
        rest_base_url: std::env::var("EXNESS_REST_BASE_URL").ok().map(|s| Url::parse(&s)).transpose()?,
        ..Default::default()
    };
    let client = Client::new(creds, account_id.clone(), cfg)?;
    client.ensure_access_point().await?;

    let acc = client.account_details().await?;
    println!(
        "account {} currency={} exec_mode(account)={:?}",
        acc.account_id, acc.settings.currency, acc.settings.execution_mode
    );
    if acc.account_name.as_deref().map(|n| n.to_lowercase().contains("real")).unwrap_or(false) {
        bail!("account_name looks like a REAL account; aborting");
    }

    let instruments = client.instruments().await?;
    let gold = ["XAUUSD", "XAUUSDm", "XAUUSDc"]
        .iter()
        .find(|c| instruments.iter().any(|i| i == *c))
        .map(|s| s.to_string())
        .context("no gold symbol")?;
    let cond = client.instrument_conditions(&gold).await?;
    let exec_mode = cond.exec_mode.unwrap_or(ExecutionMode::Unknown);
    let digits = cond.point_digits.unwrap_or(3);
    println!("{gold}: exec_mode={exec_mode:?} point_digits={digits} volume_min={:?}", cond.volume_min);
    let vol: Decimal = cond.volume_min.unwrap_or_else(|| "0.01".parse().unwrap());

    // 先订阅 events 流，以便看到自己的 transaction_events
    let mut ws = WsConnection::connect(&client, WsStream::Events).await?;
    ws.subscribe(EventsSubscription::Transactions).await?;
    wait_snapshot(&mut ws).await?;

    // 即时模式用实时价格
    let mut ticks = WsConnection::connect(&client, WsStream::Ticks).await?;
    ticks.subscribe_ticks(&[&gold]).await?;
    let tick = loop {
        match tokio::time::timeout(Duration::from_secs(20), ticks.next()).await {
            Ok(Some(Ok(WsInbound::Tick(t)))) => break t,
            Ok(Some(Ok(_))) => continue,
            Ok(Some(Err(e))) => bail!("ticks: {e}"),
            Ok(None) => bail!("ticks ended"),
            Err(_) => bail!("no tick in 20s"),
        }
    };
    println!("tick bid={} ask={}", tick.bid, tick.ask);

    let mk = |side: Side| {
        let mut r = OpenPositionRequest::market(&gold, side, vol).with_comment(format!("GL_M1_{}", side.as_str()));
        if exec_mode == ExecutionMode::Instant {
            let px = match side {
                Side::Buy => tick.ask,
                Side::Sell => tick.bid,
            };
            let px = Decimal::from_f64_retain(px).unwrap().round_dp(digits);
            r = r.with_instant_price(px, Decimal::from(200));
        }
        r
    };

    // 1) 买入
    let k1 = generate_idempotency_key(&format!("gl.{account_id}.m1.buy"))?;
    let ack1 = client.open_position(&mk(Side::Buy), &k1).await.context("open buy")?;
    println!("ACK buy: {ack1:?}");
    let buy_pos = wait_terminal(&mut ws, &client, &ack1, &k1).await?;
    println!("BUY confirmed position_id={buy_pos}");

    // 2) 买入持仓未平仓时卖出 → 对冲检查
    let k2 = generate_idempotency_key(&format!("gl.{account_id}.m1.sell"))?;
    let ack2 = client.open_position(&mk(Side::Sell), &k2).await.context("open sell")?;
    println!("ACK sell: {ack2:?}");
    let sell_pos = wait_terminal(&mut ws, &client, &ack2, &k2).await?;
    println!("SELL confirmed position_id={sell_pos}");

    let snap = client.snapshot().await?;
    let open: Vec<_> = snap.positions.iter().filter(|p| p.instrument == gold).collect();
    println!(
        "snapshot now has {} open gold positions: {:?}",
        open.len(),
        open.iter().map(|p| (p.position_id.as_str(), p.direction, p.volume)).collect::<Vec<_>>()
    );
    let hedging_ok = open.iter().any(|p| p.direction == Side::Buy) && open.iter().any(|p| p.direction == Side::Sell);
    println!("HEDGING (buy & sell coexist) = {hedging_ok}");

    // 3) 幂等重放：相同 key + 相同 payload 必须返回相同 operation_id，且不产生新持仓
    let replay = client.open_position(&mk(Side::Sell), &k2).await;
    println!("idempotent replay of sell: {replay:?} (expected same operation_id {:?})", ack2.operation_id);

    // 4) 平掉两侧
    for (pid, label) in [(&buy_pos, "buy"), (&sell_pos, "sell")] {
        let k = generate_idempotency_key(&format!("gl.{account_id}.m1.close"))?;
        let mut params = ClosePositionParams::default();
        if exec_mode == ExecutionMode::Instant {
            let px = if label == "buy" { tick.bid } else { tick.ask };
            params.price = Some(Decimal::from_f64_retain(px).unwrap().round_dp(digits));
            params.deviation = Some(Decimal::from(300));
        }
        let ack = client.close_position(pid, &params, &k).await.with_context(|| format!("close {label}"))?;
        println!("ACK close {label}: {ack:?}");
        let _ = wait_terminal(&mut ws, &client, &ack, &k).await?;
        println!("close {label} confirmed");
    }

    let snap = client.snapshot().await?;
    println!("remaining gold positions: {}", snap.positions.iter().filter(|p| p.instrument == gold).count());
    Ok(())
}

async fn wait_snapshot(ws: &mut WsConnection) -> anyhow::Result<()> {
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_secs(10) {
        if let Ok(Some(Ok(WsInbound::Event(ev)))) = tokio::time::timeout(Duration::from_secs(5), ws.next()).await {
            if matches!(*ev, ServerEvent::TradingStateSnapshot { .. }) {
                return Ok(());
            }
        }
    }
    bail!("no trading_state_snapshot after subscribe")
}

/// 等待与 ACK 匹配的 transaction_event（按 client_request_id 或 operation_id），
/// 3 秒后回退到操作状态轮询。返回受影响的 position_id（如有）。
async fn wait_terminal(ws: &mut WsConnection, client: &Client, ack: &AckResponse, key: &str) -> anyhow::Result<String> {
    let t0 = Instant::now();
    let mut polled = HashMap::<String, OperationStatus>::new();
    loop {
        if t0.elapsed() > Duration::from_secs(30) {
            bail!("operation {:?} not terminal after 30s", ack.operation_id);
        }
        match tokio::time::timeout(Duration::from_millis(500), ws.next()).await {
            Ok(Some(Ok(WsInbound::Event(ev)))) => {
                let ServerEvent::TransactionEvent {
                    operation_id,
                    client_request_id,
                    status,
                    error_code,
                    error_message,
                    payload,
                    ..
                } = *ev
                else {
                    continue;
                };
                let mine = client_request_id.as_deref() == Some(key)
                    || (operation_id.is_some() && operation_id == ack.operation_id);
                if !mine {
                    println!("  (other transaction_event op={operation_id:?} status={status:?})");
                    continue;
                }
                println!("  transaction_event op={operation_id:?} crid={client_request_id:?} status={status:?} err={error_code:?} {error_message:?}");
                for p in &payload.positions {
                    println!(
                        "    position {} {:?} vol={} state={:?} open={:?}",
                        p.position_id, p.direction, p.volume, p.state, p.open_price
                    );
                }
                for d in &payload.deals {
                    println!("    deal {} {:?} price={:?} profit={:?}", d.deal_id, d.deal_type, d.price, d.profit);
                }
                match status {
                    Some(TradingEventStatus::Success) => {
                        return Ok(payload.positions.first().map(|p| p.position_id.clone()).unwrap_or_default());
                    }
                    other => bail!("operation ended with {other:?}: {error_code:?} {error_message:?}"),
                }
            }
            Ok(Some(Ok(_))) => continue,
            Ok(Some(Err(e))) => bail!("events ws: {e}"),
            Ok(None) => bail!("events ws ended"),
            Err(_) => {}
        }
        if t0.elapsed() > Duration::from_secs(3) {
            if let Some(op) = &ack.operation_id {
                let st = client.operation_status(op).await?;
                if polled.insert(op.clone(), st.status) != Some(st.status) {
                    println!("  poll operation {op}: {:?} {:?} {:?}", st.status, st.error_code, st.error_message);
                }
                if st.status.is_terminal() && st.status != OperationStatus::Confirmed {
                    bail!("operation {op} {:?}: {:?} {:?}", st.status, st.error_code, st.error_message);
                }
            }
        }
    }
}

fn env(name: &str) -> anyhow::Result<String> {
    std::env::var(name).with_context(|| format!("missing env {name}"))
}
