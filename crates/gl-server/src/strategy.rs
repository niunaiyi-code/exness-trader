//! 策略运行器接线：把共享 tick 流桥接到 AccountRunner 并运行。

use std::sync::{atomic::{AtomicBool, Ordering}, Arc, Mutex};
use exness_api::{Client, EventStreamClient, Tick};
use gl_runtime::{AccountConfig, AccountRunner};
use tokio::sync::{broadcast, mpsc};

/// 为单个交易账户运行一个 `AccountRunner`。订阅共享 tick 流、账本事件，
/// 以及该账户品种条件更新；策略通过 watch 接收去重后的条件快照。
pub async fn run_runner(
    config: AccountConfig,
    client: Arc<Client>,
    mut tick_rx: broadcast::Receiver<Tick>,
    paused: Arc<AtomicBool>,
    news_rx: tokio::sync::watch::Receiver<(bool, Option<i64>)>,
    instrument_conditions_rx: tokio::sync::watch::Receiver<Option<exness_api::InstrumentCondition>>,
    instrument_conditions: Arc<crate::instrument_conditions::InstrumentConditions>,
    instrument_account_id: String,
    shutdown_rx: mpsc::Receiver<()>,
    ws_hub: Arc<crate::ws::WsHub>,
    db: Option<(crate::db::DbPool, String)>,
) -> Result<(), gl_runtime::RuntimeError> {
    let (tick_tx, tick_mpsc_rx) = mpsc::channel(1);
    let (event_tx, event_mpsc_rx) = mpsc::channel(1024);
    let tick_busy = Arc::new(AtomicBool::new(false));
    let tick_gate = Arc::new(Mutex::new(()));

    // 同一账户事件流同时订阅账本事件与该账户品种条件变化。
    let event_client = EventStreamClient::with_instruments(
        client.clone(),
        vec!["transactions".to_string(), "instruments".to_string()],
        vec![config.symbol.clone()],
    );
    let (refresh_tx, mut refresh_rx) = mpsc::channel::<()>(1);
    let refresh_client = client.clone();
    let refresh_service = instrument_conditions.clone();
    let refresh_account_id = instrument_account_id.clone();
    let refresh_symbol = config.symbol.clone();
    let condition_refresh = tokio::spawn(async move {
        while refresh_rx.recv().await.is_some() {
            match refresh_client.instrument_conditions(&refresh_symbol).await {
                Ok(condition) => refresh_service.update(&refresh_account_id, "upd", condition),
                Err(error) => tracing::error!(account_id = %refresh_account_id, error = %error, "failed to refresh instrument conditions after event stream connection"),
            }
        }
    });

    let ws_hub_for_events = ws_hub.clone();
    let feed_account_id = instrument_account_id.clone();
    let event_feed = tokio::spawn(async move {
        let _ = event_client
            .run_with_connected(move |event| {
                match &event {
                    exness_api::WsEvent::Transaction(tx) => {
                        tracing::info!(op = %tx.operation_id, status = %tx.status, "transaction event in feed");
                        ws_hub_for_events.broadcast(crate::ws::WsFrame::Event {
                            account_id: feed_account_id.clone(),
                            event_type: "transaction".to_string(),
                            data: serde_json::json!({
                                "operation_id": tx.operation_id,
                                "status": tx.status,
                                "positions": tx.payload.positions,
                                "orders": tx.payload.orders,
                                "deals": tx.payload.deals,
                                "account_state": tx.payload.account_state,
                            }),
                        });
                    }
                    exness_api::WsEvent::Instrument(update) => {
                        instrument_conditions.update(
                            &instrument_account_id,
                            &update.action,
                            update.payload.clone(),
                        );
                    }
                    exness_api::WsEvent::Other => tracing::info!("other event in feed"),
                }
                if matches!(&event, exness_api::WsEvent::Transaction(_)) {
                    if let Err(e) = event_tx.try_send(event) {
                        tracing::error!(error = %e, "failed to forward transaction event to runner");
                    }
                }
            }, move || {
                let _ = refresh_tx.try_send(());
            })
            .await;
    });

    let (trade_tx, mut trade_rx) = tokio::sync::mpsc::unbounded_channel::<gl_runtime::TradeNotification>();
    let ws_hub_for_trades = ws_hub.clone();
    let trade_forward_task = tokio::spawn(async move {
        while let Some(notif) = trade_rx.recv().await {
            ws_hub_for_trades.broadcast(crate::ws::WsFrame::Event {
                account_id: notif.account_id.clone(),
                event_type: "trade_event".to_string(),
                data: serde_json::to_value(&notif).unwrap_or_default(),
            });
        }
    });

    let forward_busy = tick_busy.clone();
    let forward_gate = tick_gate.clone();
    let tick_fwd = tokio::spawn(async move {
        loop {
            match tick_rx.recv().await {
                Ok(tick) => {
                    let _guard = forward_gate.lock().unwrap_or_else(|e| e.into_inner());
                    if !forward_busy.load(Ordering::Acquire) {
                        // The single slot is only for dispatch while idle. If the runner
                        // cannot accept immediately, discard this quote instead of queuing it.
                        let _ = tick_tx.try_send(tick);
                    }
                }
                Err(broadcast::error::RecvError::Lagged(skipped)) => {
                    tracing::warn!(skipped, "Account tick subscriber lagged; skipped buffered quotes");
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    let run_account_id = config.account_id.clone();
    let mut runner = AccountRunner::new(config, client, paused);
    runner.set_trade_notifier(trade_tx);
    let run_res = runner.run(
        tick_mpsc_rx,
        event_mpsc_rx,
        shutdown_rx,
        news_rx,
        instrument_conditions_rx,
        tick_busy,
        tick_gate,
    ).await;

    let final_status = match &run_res {
        Ok(_) => {
            tracing::info!(account_id = %run_account_id, "AccountRunner exited normally");
            "stopped"
        }
        Err(e) => {
            tracing::error!(account_id = %run_account_id, error = %e, "AccountRunner exited with error");
            "failed"
        }
    };

    if let Some((pool, account_db_id)) = db {
        let _ = crate::db::repository::AccountRepository::update_state(&pool, &account_db_id, final_status).await;
        crate::events::broadcast_runner_status(&ws_hub, run_account_id, final_status.to_string());
    }

    trade_forward_task.abort();
    tick_fwd.abort();
    event_feed.abort();
    condition_refresh.abort();

    run_res
}
