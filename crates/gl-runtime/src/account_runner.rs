//! AccountRunner：每账户主事件循环。

use crate::{
    config::AccountConfig,
    error::{RuntimeError, RuntimeResult},
    executor::Executor,
    ledger::{Ledger, LedgerEntry, PositionRole},
    market::MarketData,
    ops_tracker::OpsTracker,
    pipeline::StrategyPipeline,
};
use exness_api::{Position, PositionSide, RestClient, Tick, WsEvent};
use gl_strategy::LockState;
use rust_decimal::Decimal;
use std::sync::{atomic::{AtomicBool, Ordering}, Arc, Mutex};
use std::time::Duration;
use tokio::{
    select,
    sync::mpsc,
    time::{interval, MissedTickBehavior},
};
use tracing::{error, info, warn};

/// 运行器的生命周期状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunnerState {
    Init,
    Warmup,
    Running,
    Degraded,
    Stopping,
    Failed,
}

/// Trailing stop state (移动锁利状态机)
#[derive(Debug, Clone)]
struct TrailingState {
    active: bool,
    peak_exit_price: Decimal,
    tracked_count: usize,
}

impl Default for TrailingState {
    fn default() -> Self {
        Self {
            active: false,
            peak_exit_price: Decimal::ZERO,
            tracked_count: 0,
        }
    }
}

pub struct AccountRunner {
    config: AccountConfig,
    rest: Arc<RestClient>,
    executor: Executor,
    ledger: Ledger,
    ops_tracker: OpsTracker,
    market: MarketData,
    lock_state: LockState,
    trailing_buy: TrailingState,
    trailing_sell: TrailingState,
    state: RunnerState,
    paused: Arc<AtomicBool>,
    current_positions: Vec<Position>,
    last_buy_grid_time: Option<chrono::DateTime<chrono::Utc>>,
    last_sell_grid_time: Option<chrono::DateTime<chrono::Utc>>,
    last_snapshot_time: Option<std::time::Instant>,
    trade_notifier: Option<tokio::sync::mpsc::UnboundedSender<crate::events::TradeNotification>>,
}

impl AccountRunner {
    pub fn new(
        config: AccountConfig,
        rest: Arc<RestClient>,
        paused: Arc<AtomicBool>,
    ) -> Self {
        let executor = Executor::new(rest.clone(), config.symbol.clone());
        let ops_tracker = OpsTracker::new(config.account_id.clone());
        let ledger = Ledger::new();
        let market = MarketData::new();

        Self {
            config,
            rest,
            executor,
            ledger,
            ops_tracker,
            market,
            lock_state: LockState::Idle, // 待处理(AGENTS.md): 极端重启锁仓状态持久化延后处理
            trailing_buy: TrailingState::default(),
            trailing_sell: TrailingState::default(),
            state: RunnerState::Init,
            paused,
            current_positions: Vec::new(),
            last_buy_grid_time: None,
            last_sell_grid_time: None,
            last_snapshot_time: None,
            trade_notifier: None,
        }
    }

    /// 设置交易成交事件通知发送器
    pub fn set_trade_notifier(
        &mut self,
        tx: tokio::sync::mpsc::UnboundedSender<crate::events::TradeNotification>,
    ) {
        self.trade_notifier = Some(tx);
    }

    fn notify_trade(
        &self,
        event_type: &str,
        position_id: Option<String>,
        side: &str,
        volume: &str,
        price: Option<String>,
        profit: Option<String>,
    ) {
        if let Some(ref tx) = self.trade_notifier {
            let notif = crate::events::TradeNotification {
                account_id: self.config.account_id.clone(),
                event_id: uuid::Uuid::new_v4().to_string(),
                event_type: event_type.to_string(),
                position_id,
                symbol: self.config.symbol.clone(),
                side: side.to_string(),
                volume: volume.to_string(),
                price,
                profit,
                timestamp: chrono::Utc::now().timestamp_millis(),
            };
            let _ = tx.send(notif);
        }
    }

    /// 主事件循环。
    pub async fn run(
        &mut self,
        mut tick_rx: mpsc::Receiver<Tick>,
        mut event_rx: mpsc::Receiver<WsEvent>,
        mut shutdown_rx: mpsc::Receiver<()>,
        mut news_rx: tokio::sync::watch::Receiver<(bool, Option<i64>)>,
        mut instrument_conditions_rx: tokio::sync::watch::Receiver<Option<exness_api::InstrumentCondition>>,
        tick_busy: Arc<AtomicBool>,
        tick_gate: Arc<Mutex<()>>,
    ) -> RuntimeResult<()> {
        info!(account = %self.config.account_id, "AccountRunner starting");
        self.state = RunnerState::Warmup;
        {
            let (ready, timestamp) = *news_rx.borrow_and_update();
            self.market.news_calendar_ready = ready;
            self.market.next_news_event_time = timestamp.and_then(|value| chrono::DateTime::from_timestamp(value, 0));
        }
        self.market.instrument_condition = instrument_conditions_rx.borrow_and_update().clone();

        // 加载初始快照并对账
        self.warmup().await?;

        self.state = RunnerState::Running;
        info!(account = %self.config.account_id, "AccountRunner entered Running state");

        // 用于对账与心跳的 1 秒定时器
        let mut timer = interval(Duration::from_secs(1));
        timer.set_missed_tick_behavior(MissedTickBehavior::Skip);

        loop {
            select! {
                Some(mut tick) = tick_rx.recv() => {
                    {
                        let _guard = tick_gate.lock().unwrap_or_else(|e| e.into_inner());
                        tick_busy.store(true, Ordering::Release);
                        while let Ok(newer) = tick_rx.try_recv() {
                            tick = newer;
                        }
                    }
                    if let Err(e) = self.on_tick(tick).await {
                        error!(account = %self.config.account_id, error = %e, "Tick handler failed");
                        self.state = RunnerState::Degraded;
                    }
                    {
                        let _guard = tick_gate.lock().unwrap_or_else(|e| e.into_inner());
                        tick_busy.store(false, Ordering::Release);
                    }
                }

                Some(event) = event_rx.recv() => {
                    if let Err(e) = self.on_ws_event(event).await {
                        error!(account = %self.config.account_id, error = %e, "Event handler failed");
                    }
                }

                _ = news_rx.changed() => {
                    let (ready, timestamp) = *news_rx.borrow_and_update();
                    self.market.news_calendar_ready = ready;
                    self.market.next_news_event_time = timestamp.and_then(|value| chrono::DateTime::from_timestamp(value, 0));
                }

                _ = instrument_conditions_rx.changed() => {
                    self.market.instrument_condition = instrument_conditions_rx.borrow_and_update().clone();
                }

                _ = timer.tick() => {
                    if let Err(e) = self.on_timer().await {
                        error!(account = %self.config.account_id, error = %e, "Timer handler failed");
                    }
                }

                Some(_) = shutdown_rx.recv() => {
                    info!(account = %self.config.account_id, "Shutdown signal received");
                    self.shutdown();
                    break;
                }
            }

            if self.state == RunnerState::Stopping {
                break;
            }
        }

        info!(account = %self.config.account_id, "AccountRunner stopped");
        Ok(())
    }

    /// 预热阶段：加载快照、对账、拉取 K 线。
    async fn warmup(&mut self) -> RuntimeResult<()> {
        info!(account = %self.config.account_id, "Warmup: loading snapshot");

        let snapshot = self
            .rest
            .snapshot()
            .await
            .map_err(RuntimeError::ExnessApi)?;

        self.current_positions = snapshot.positions.clone();

        // 区分本策略持仓与外部/手动持仓：只有明确带有本策略标识的持仓才纳入网格管理，其余保留为 Foreign
        for pos in &snapshot.positions {
            let side = match PositionSide::from_string_value(&pos.position_type) {
                Some(s) => s,
                None => {
                    warn!(
                        position_id = %pos.id,
                        position_type = %pos.position_type,
                        "Unknown position_type in warmup snapshot, skipping ledger adoption"
                    );
                    continue;
                }
            };
            let volume = pos.volume.parse().unwrap_or_default();
            let open_price = pos.open_price.parse().unwrap_or_default();
            let is_symbol_match = pos.symbol == self.config.symbol;
            let is_strategy_order = pos.comment.as_ref().map(|c| c.starts_with("gl.") || c.starts_with("GL_")).unwrap_or(false);

            if is_symbol_match && is_strategy_order {
                self.ledger.insert(LedgerEntry {
                    position_id: pos.id.clone(),
                    role: PositionRole::Grid,
                    open_time: chrono::DateTime::from_timestamp(pos.open_time, 0).unwrap_or_else(chrono::Utc::now),
                    volume,
                    open_price,
                    side,
                });
                info!(position_id = %pos.id, side = ?side, volume = %volume, "Adopted verified strategy position into Grid ledger during warmup");
            } else {
                self.ledger.insert(LedgerEntry {
                    position_id: pos.id.clone(),
                    role: PositionRole::Foreign,
                    open_time: chrono::DateTime::from_timestamp(pos.open_time, 0).unwrap_or_else(chrono::Utc::now),
                    volume,
                    open_price,
                    side,
                });
                info!(position_id = %pos.id, symbol = %pos.symbol, "Preserved external/manual position as Foreign during warmup");
            }
        }

        // 拉取最近 100 根 M1 K 线用于预热
        let candle_query = exness_api::CandleQuery {
            instrument: self.config.symbol.clone(),
            price_type: "bid".to_string(),
            timeframe: "M1".to_string(),
            from: chrono::Utc::now(),
            to: None,
            count: Some(-100),
        };

        let candles = self
            .rest
            .candles(&candle_query)
            .await
            .map_err(RuntimeError::ExnessApi)?;

        for candle in candles {
            self.market.update_candle(candle);
        }

        info!(
            account = %self.config.account_id,
            candles = self.market.candle_count(),
            "Warmup complete"
        );

        // 更新持仓统计 metrics
        self.update_position_metrics();

        Ok(())
    }

    /// 处理 WS transaction 事件。
    async fn on_ws_event(&mut self, event: WsEvent) -> RuntimeResult<()> {
        let WsEvent::Transaction(tx) = event else {
            // snapshot / account_state / instrument / hmr 事件此处不使用
            return Ok(());
        };

        info!(
            operation_id = %tx.operation_id,
            status = %tx.status,
            positions = tx.payload.positions.len(),
            "transaction event received"
        );

        if tx.status == "rejected" {
            warn!(
                operation_id = %tx.operation_id,
                "Order rejected by broker. Enforcing 15-second debounce and retaining ledger state"
            );
            if let Some(pending_op) = self.ops_tracker.mark_completed(&tx.operation_id) {
                match pending_op.side {
                    Some(PositionSide::Buy) => self.last_buy_grid_time = Some(chrono::Utc::now()),
                    Some(PositionSide::Sell) => self.last_sell_grid_time = Some(chrono::Utc::now()),
                    None => {
                        self.last_buy_grid_time = Some(chrono::Utc::now());
                        self.last_sell_grid_time = Some(chrono::Utc::now());
                    }
                }
            } else {
                self.last_buy_grid_time = Some(chrono::Utc::now());
                self.last_sell_grid_time = Some(chrono::Utc::now());
            }
            return Ok(());
        }

        let Some(pending_op) = self.ops_tracker.mark_completed(&tx.operation_id) else {
            return Ok(());
        };

        match pending_op.kind {
            crate::ops_tracker::OpKind::OpenGrid | crate::ops_tracker::OpKind::OpenLock => {
                let (Some(role), Some(side), Some(volume)) =
                    (pending_op.role, pending_op.side, pending_op.volume)
                else {
                    return Ok(());
                };
                let event_type = if pending_op.kind == crate::ops_tracker::OpKind::OpenLock {
                    "open_lock"
                } else {
                    "open_grid"
                };
                for pos in &tx.payload.positions {
                    self.ledger.insert(LedgerEntry {
                        position_id: pos.id.clone(),
                        role,
                        side,
                        volume,
                        open_time: chrono::Utc::now(),
                        open_price: pos.open_price.parse().unwrap_or_default(),
                    });
                    // 同步写入本地持仓缓存，避免新仓在下一轮 REST 快照前对策略层不可见
                    // （否则 BasketStats::calculate_side 取交集时视为空篮子，导致 15 秒盲区重复开单）
                    self.current_positions.retain(|p| p.id != pos.id);
                    self.current_positions.push(pos.clone());
                    info!(
                        position_id = %pos.id,
                        role = ?role,
                        side = ?side,
                        volume = %volume,
                        "Position added to ledger and current_positions"
                    );
                    self.notify_trade(
                        event_type,
                        Some(pos.id.clone()),
                        side.to_string_value(),
                        &volume.to_string(),
                        Some(pos.open_price.clone()),
                        None,
                    );
                }
            }
            crate::ops_tracker::OpKind::CloseGrid
            | crate::ops_tracker::OpKind::CloseLock
            | crate::ops_tracker::OpKind::EmergencyClose => {
                if let Some(position_id) = pending_op.position_id {
                    self.ledger.remove(&position_id);
                    self.current_positions.retain(|p| p.id != position_id);
                    info!(position_id = %position_id, "Position removed from ledger and local tracking");
                    let event_type = match pending_op.kind {
                        crate::ops_tracker::OpKind::CloseGrid => "close_tp",
                        crate::ops_tracker::OpKind::CloseLock => "close_lock",
                        _ => "close_sl",
                    };
                    self.notify_trade(
                        event_type,
                        Some(position_id),
                        "",
                        "",
                        None,
                        None,
                    );
                }
            }
        }

        Ok(())
    }

    async fn close_grid_basket_side(&mut self, side: PositionSide) -> RuntimeResult<()> {
        let dev = self.config.constants.deviation;
        let positions: Vec<_> = self
            .current_positions
            .iter()
            .filter(|p| {
                let is_grid = self
                    .ledger
                    .get(&p.id)
                    .map(|e| e.role == PositionRole::Grid)
                    .unwrap_or(false);
                let p_side = PositionSide::from_string_value(&p.position_type);
                is_grid && p_side == Some(side)
            })
            .cloned()
            .collect();

        for pos in positions {
            // 在途去重：该仓位已有未确认的平仓请求时跳过，避免高频 tick 重复下发平仓指令（请求风暴/429 限流）
            if self.ops_tracker.has_pending_close(&pos.id) {
                info!(position_id = %pos.id, "Skip close: position already has in-flight close request");
                continue;
            }
            let key = self.ops_tracker.generate_key("grid_close", 1);
            match self.executor.close_position(&key, &pos.id, dev).await {
                Ok(op_id) => {
                    self.ops_tracker.track_close(op_id.clone(), pos.id.clone(), PositionRole::Grid);
                }
                Err(e) => {
                    error!(position_id = %pos.id, error = %e, "Failed to close grid position");
                }
            }
        }

        Ok(())
    }

    /// 定时器处理器：周期性对账与心跳。
    async fn on_timer(&mut self) -> RuntimeResult<()> {
        let expired = self.ops_tracker.expire_stale(15);
        if !expired.is_empty() {
            warn!(count = expired.len(), "Expired stale in-flight operations");
        }

        let now = std::time::Instant::now();
        let should_snapshot = match self.last_snapshot_time {
            Some(last) => last.elapsed() >= Duration::from_secs(15),
            None => true,
        };

        if should_snapshot {
            self.last_snapshot_time = Some(now);
            match self.rest.snapshot().await {
                Ok(snap) => {
                    self.current_positions = snap.positions.clone();

                    // 对账已消失的持仓（外部平仓或触发止盈/止损）
                    // 宽限期：刚入账不足 10 秒的持仓不参与缺席剔除，避免与 WS 入账时序竞态导致误删
                    let snap_ids: std::collections::HashSet<_> = snap.positions.iter().map(|p| p.id.as_str()).collect();
                    let grace_deadline = chrono::Utc::now() - chrono::Duration::seconds(10);
                    let to_remove: Vec<_> = self.ledger.grid_positions()
                        .filter(|e| !snap_ids.contains(e.position_id.as_str()))
                        .filter(|e| e.open_time < grace_deadline)
                        .map(|e| e.position_id.clone())
                        .collect();
                    
                    let removed_count = to_remove.len();
                    for id in to_remove {
                        self.ledger.remove(&id);
                        info!(position_id = %id, "Reconciled externally closed position out of ledger");
                    }

                    // 将账本中尚未登记的外部新持仓纳入台账（标记为 Foreign，绝不参与网格平仓）
                    let mut added_count = 0;
                    for pos in &snap.positions {
                        if self.ledger.get(&pos.id).is_none() {
                            let side = match PositionSide::from_string_value(&pos.position_type) {
                                Some(s) => s,
                                None => {
                                    warn!(
                                        position_id = %pos.id,
                                        position_type = %pos.position_type,
                                        "Unknown position_type in timer reconcile, skipping foreign adoption"
                                    );
                                    continue;
                                }
                            };
                            let volume = pos.volume.parse().unwrap_or_default();
                            let open_price = pos.open_price.parse().unwrap_or_default();
                            self.ledger.insert(LedgerEntry {
                                position_id: pos.id.clone(),
                                role: PositionRole::Foreign,
                                open_time: chrono::DateTime::from_timestamp(pos.open_time, 0).unwrap_or_else(chrono::Utc::now),
                                volume,
                                open_price,
                                side,
                            });
                            info!(position_id = %pos.id, side = ?side, symbol = %pos.symbol, "Adopted unmanaged position as Foreign during timer reconcile");
                            added_count += 1;
                        }
                    }
                    
                    // 记录对账 metrics
                    crate::metrics::record_reconcile(&self.config.account_id, true, added_count, removed_count);
                }
                Err(e) => {
                    crate::metrics::record_reconcile(&self.config.account_id, false, 0, 0);
                    warn!(
                        account = %self.config.account_id,
                        error = %e,
                        "Failed to reconcile snapshot from broker; retaining current ledger state"
                    );
                }
            }
        }
        Ok(())
    }

    /// 处理收到的 tick（主策略流水线）。
    async fn on_tick(&mut self, tick: Tick) -> RuntimeResult<()> {
        self.ops_tracker.expire_stale(15);
        self.market.update_tick(tick);

        let result = StrategyPipeline::on_tick(
            &self.config,
            &self.ledger,
            &self.market,
            self.lock_state,
            &self.current_positions,
            self.paused.load(Ordering::Acquire),
        );

        let dev = self.config.constants.deviation;

        // 1. 处理买入篮子的移动止损逻辑
        let buy_count = self.ledger.grid_positions().filter(|e| e.side == PositionSide::Buy).count();
        if let Some(trigger_profit) = result.trailing_trigger_buy {
            if !self.trailing_buy.active {
                self.trailing_buy.active = true;
                self.trailing_buy.tracked_count = buy_count;
                if let Some(bid) = self.market.bid() {
                    self.trailing_buy.peak_exit_price = bid;
                    info!(
                        trigger_profit = %trigger_profit,
                        peak_price = %bid,
                        count = buy_count,
                        "Trailing activated for BUY basket (25-pt fixed step)"
                    );
                }
            }
        }

        if self.trailing_buy.active {
            if buy_count != self.trailing_buy.tracked_count {
                info!("Trailing BUY reset: count changed {} -> {}", self.trailing_buy.tracked_count, buy_count);
                self.trailing_buy = TrailingState::default();
            } else if let Some(bid) = self.market.bid() {
                if bid > self.trailing_buy.peak_exit_price {
                    self.trailing_buy.peak_exit_price = bid;
                }

                let pullback_points = (self.trailing_buy.peak_exit_price - bid) / self.config.constants.pip;
                if pullback_points >= self.config.constants.trailing_step_points {
                    info!(
                        peak = %self.trailing_buy.peak_exit_price,
                        current = %bid,
                        pullback_pts = %pullback_points,
                        "Trailing stop hit: 25-pt pullback triggered for BUY basket!"
                    );
                    self.close_grid_basket_side(PositionSide::Buy).await?;
                    self.trailing_buy = TrailingState::default();
                }
            }
        }

        // 2. 处理卖出篮子的移动止损逻辑
        let sell_count = self.ledger.grid_positions().filter(|e| e.side == PositionSide::Sell).count();
        if let Some(trigger_profit) = result.trailing_trigger_sell {
            if !self.trailing_sell.active {
                self.trailing_sell.active = true;
                self.trailing_sell.tracked_count = sell_count;
                if let Some(ask) = self.market.ask() {
                    self.trailing_sell.peak_exit_price = ask;
                    info!(
                        trigger_profit = %trigger_profit,
                        peak_price = %ask,
                        count = sell_count,
                        "Trailing activated for SELL basket (25-pt fixed step)"
                    );
                }
            }
        }

        if self.trailing_sell.active {
            if sell_count != self.trailing_sell.tracked_count {
                info!("Trailing SELL reset: count changed {} -> {}", self.trailing_sell.tracked_count, sell_count);
                self.trailing_sell = TrailingState::default();
            } else if let Some(ask) = self.market.ask() {
                if ask < self.trailing_sell.peak_exit_price || self.trailing_sell.peak_exit_price == Decimal::ZERO {
                    self.trailing_sell.peak_exit_price = ask;
                }

                let pullback_points = (ask - self.trailing_sell.peak_exit_price) / self.config.constants.pip;
                if pullback_points >= self.config.constants.trailing_step_points {
                    info!(
                        peak = %self.trailing_sell.peak_exit_price,
                        current = %ask,
                        pullback_pts = %pullback_points,
                        "Trailing stop hit: 25-pt pullback triggered for SELL basket!"
                    );
                    self.close_grid_basket_side(PositionSide::Sell).await?;
                    self.trailing_sell = TrailingState::default();
                }
            }
        }

        // 3. 执行：达到 100% 目标时立即平掉网格篮子（缓冲不足）
        if result.should_close_buy {
            info!("Executing: close BUY grid basket (100% target, low cushion)");
            self.close_grid_basket_side(PositionSide::Buy).await?;
            self.trailing_buy = TrailingState::default();
        }
        if result.should_close_sell {
            info!("Executing: close SELL grid basket (100% target, low cushion)");
            self.close_grid_basket_side(PositionSide::Sell).await?;
            self.trailing_sell = TrailingState::default();
        }

        // 4. 执行：开对冲锁仓单
        let mut lock_succeeded = false;
        if result.should_open_lock {
            info!("Executing: open lock positions");
            // 计算多空净敞口（net exposure）进行对冲
            let net_exposure = self.ledger.net_exposure_by_role(PositionRole::Grid);

            if net_exposure != rust_decimal::Decimal::ZERO {
                let (lock_side, lock_volume) = if net_exposure > rust_decimal::Decimal::ZERO {
                    // 多头占优，开空单对冲净多头
                    (PositionSide::Sell, net_exposure)
                } else {
                    // 空头占优，开多单对冲净空头
                    (PositionSide::Buy, -net_exposure)
                };

                let key = self.ops_tracker.generate_key("lock", 1);
                match self.executor.open_position(&key, lock_side, lock_volume, dev).await {
                    Ok(op_id) => {
                        self.ops_tracker.track_open(op_id, PositionRole::Lock, lock_side, lock_volume);
                        lock_succeeded = true;
                    }
                    Err(e) => {
                        error!(error = %e, "Failed to open lock position; will retry on subsequent tick");
                    }
                }
            } else {
                info!("Grid already fully balanced (net exposure = 0); no lock order needed");
                lock_succeeded = true;
            }
        }

        // 5. 执行：为空篮子开首单(双向独立对冲)
        let initial_lot = self.config.params.initial_lot;
        
        if result.should_open_first_buy && !self.ops_tracker.has_pending_side(PositionSide::Buy) {
            info!("Executing: open first Buy grid position");
            let key = self.ops_tracker.generate_key("grid_first_buy", 0);
            match self.executor.open_position(&key, PositionSide::Buy, initial_lot, dev).await {
                Ok(op_id) => {
                    self.ops_tracker.track_open(op_id, PositionRole::Grid, PositionSide::Buy, initial_lot);
                }
                Err(e) => {
                    error!(error = %e, "Failed to open first Buy grid position");
                }
            }
        }
        
        if result.should_open_first_sell && !self.ops_tracker.has_pending_side(PositionSide::Sell) {
            info!("Executing: open first Sell grid position");
            let key = self.ops_tracker.generate_key("grid_first_sell", 0);
            match self.executor.open_position(&key, PositionSide::Sell, initial_lot, dev).await {
                Ok(op_id) => {
                    self.ops_tracker.track_open(op_id, PositionRole::Grid, PositionSide::Sell, initial_lot);
                }
                Err(e) => {
                    error!(error = %e, "Failed to open first Sell grid position");
                }
            }
        }

        // 6. 执行：加网格仓（两侧独立冷却与操作）
        let now = chrono::Utc::now();
        
        if let Some((side, volume)) = result.should_add_grid {
            let last_time = match side {
                PositionSide::Buy => self.last_buy_grid_time,
                PositionSide::Sell => self.last_sell_grid_time,
            };
            let debounce = if let Some(last) = last_time {
                (now - last).num_seconds() < self.config.params.grid_step_sec as i64
            } else {
                false
            };

            if !debounce && !self.ops_tracker.has_pending_side(side) {
                let side_str = match side {
                    PositionSide::Buy => "Buy",
                    PositionSide::Sell => "Sell",
                };
                info!("Executing: add grid position {} volume {}", side_str, volume);
                
                match side {
                    PositionSide::Buy => self.last_buy_grid_time = Some(now),
                    PositionSide::Sell => self.last_sell_grid_time = Some(now),
                }
                
                let layer = self.ledger.grid_positions().filter(|e| e.side == side).count() as u32;
                let key = self.ops_tracker.generate_key(&format!("grid_add_{}", side_str.to_lowercase()), layer);

                match self.executor.open_position(&key, side, volume, dev).await {
                    Ok(op_id) => {
                        self.ops_tracker.track_open(op_id, PositionRole::Grid, side, volume);
                    }
                    Err(e) => {
                        error!(error = %e, "Failed to add grid position {}", side_str);
                    }
                }
            }
        }

        // 7. 更新状态机
        if let Some(new_state) = result.new_lock_state {
            if matches!(new_state, LockState::Locked) {
                if lock_succeeded {
                    self.lock_state = new_state;
                } else {
                    warn!("Lock state transition deferred because lock order placement failed");
                }
            } else {
                self.lock_state = new_state;
            }
        }

        Ok(())
    }

    pub fn state(&self) -> RunnerState {
        self.state
    }

    pub fn pause(&mut self) {
        self.paused.store(true, Ordering::Release);
        info!(account = %self.config.account_id, "Runner paused");
    }

    pub fn resume(&mut self) {
        self.paused.store(false, Ordering::Release);
        info!(account = %self.config.account_id, "Runner resumed");
    }

    pub fn shutdown(&mut self) {
        info!(account = %self.config.account_id, "AccountRunner shutting down");
        self.state = RunnerState::Stopping;
        // TODO：优雅关闭 WS，取消在途操作
    }

    /// 将 LedgerEntry 转换为策略层的 PositionView
    /// 需要当前市场价格来计算浮动盈亏
    fn ledger_entry_to_position_view(
        &self,
        entry: &LedgerEntry,
        current_bid: Decimal,
        current_ask: Decimal,
    ) -> gl_strategy::basket::PositionView {
        use exness_api::PositionSide;
        
        // 根据持仓方向选择平仓价格
        let current_price = match entry.side {
            PositionSide::Buy => current_bid,   // 多头用 bid 平仓
            PositionSide::Sell => current_ask,  // 空头用 ask 平仓
        };

        // 计算浮动盈亏
        let price_diff = match entry.side {
            PositionSide::Buy => current_price - entry.open_price,
            PositionSide::Sell => entry.open_price - current_price,
        };
        
        // 盈亏 = 价格变动 × 手数 × 合约大小
        let profit = price_diff * entry.volume * self.config.constants.contract_size;

        // 转换 Side 类型
        let side = match entry.side {
            PositionSide::Buy => gl_strategy::types::Side::Buy,
            PositionSide::Sell => gl_strategy::types::Side::Sell,
        };

        gl_strategy::basket::PositionView {
            position_id: entry.position_id.clone(),
            volume: entry.volume,
            open_price: entry.open_price,
            current_price,
            profit,
            side,
        }
    }

    /// 获取当前所有网格持仓的视图（用于策略决策）
    pub fn get_positions_view(&self) -> Vec<gl_strategy::basket::PositionView> {
        let bid = self.market.bid().unwrap_or(Decimal::ZERO);
        let ask = self.market.ask().unwrap_or(Decimal::ZERO);

        self.ledger
            .grid_positions()
            .map(|entry| self.ledger_entry_to_position_view(entry, bid, ask))
            .collect()
    }

    /// 获取账户快照（用于策略决策）
    pub fn get_account_snapshot(&self) -> gl_strategy::lock_manager::AccountSnapshot {
        use exness_api::PositionSide;
        
        let bid = self.market.bid().unwrap_or(Decimal::ZERO);
        let ask = self.market.ask().unwrap_or(Decimal::ZERO);
        
        let mut net_lots = Decimal::ZERO;
        let mut floating_pnl = Decimal::ZERO;
        let mut position_count = 0;

        for entry in self.ledger.grid_positions() {
            // 计算净手数（多头为正，空头为负）
            match entry.side {
                PositionSide::Buy => net_lots += entry.volume,
                PositionSide::Sell => net_lots -= entry.volume,
            }

            // 计算浮动盈亏
            let current_price = match entry.side {
                PositionSide::Buy => bid,
                PositionSide::Sell => ask,
            };
            
            let price_diff = match entry.side {
                PositionSide::Buy => current_price - entry.open_price,
                PositionSide::Sell => entry.open_price - current_price,
            };
            
            floating_pnl += price_diff * entry.volume * self.config.constants.contract_size;
            position_count += 1;
        }

        gl_strategy::lock_manager::AccountSnapshot {
            net_lots,
            floating_pnl,
            position_count,
        }
    }

    /// 平掉所有持仓（用于止盈或紧急平仓）
    pub async fn close_all_positions(&mut self) -> RuntimeResult<Vec<String>> {
        let positions: Vec<_> = self.ledger.grid_positions()
            .map(|entry| (entry.position_id.clone(), entry.role))
            .collect();
        
        if positions.is_empty() {
            return Ok(Vec::new());
        }

        info!("开始批量平仓: {} 个持仓", positions.len());
        let mut operation_ids = Vec::new();

        for (position_id, role) in positions {
            let idempotency_key = format!("close_all_{}_{}",
                position_id,
                chrono::Utc::now().timestamp_millis()
            );
            
            match self.executor.close_position(&idempotency_key, &position_id, 20).await {
                Ok(op_id) => {
                    info!("提交平仓: {} -> {}", position_id, op_id);
                    operation_ids.push(op_id.clone());
                    // 记录在途操作
                    self.ops_tracker.track_close(op_id, position_id.clone(), role);
                    // 从账本中移除
                    self.ledger.remove(&position_id);
                }
                Err(e) => {
                    error!("平仓失败 {}: {}", position_id, e);
                    // 继续处理其他持仓
                }
            }
        }

        Ok(operation_ids)
    }

    /// 平掉指定方向的所有持仓
    pub async fn close_positions_by_side(
        &mut self,
        side: exness_api::PositionSide,
    ) -> RuntimeResult<Vec<String>> {
        let positions: Vec<_> = self.ledger.grid_positions()
            .filter(|entry| entry.side == side)
            .map(|entry| (entry.position_id.clone(), entry.role))
            .collect();
        
        if positions.is_empty() {
            return Ok(Vec::new());
        }

        info!("开始平掉 {:?} 持仓: {} 个", side, positions.len());
        let mut operation_ids = Vec::new();

        for (position_id, role) in positions {
            let idempotency_key = format!("close_{:?}_{}_{}",
                side,
                position_id,
                chrono::Utc::now().timestamp_millis()
            );
            
            match self.executor.close_position(&idempotency_key, &position_id, 20).await {
                Ok(op_id) => {
                    info!("提交平仓: {} -> {}", position_id, op_id);
                    operation_ids.push(op_id.clone());
                    self.ops_tracker.track_close(op_id, position_id.clone(), role);
                    self.ledger.remove(&position_id);
                }
                Err(e) => {
                    error!("平仓失败 {}: {}", position_id, e);
                }
            }
        }

        Ok(operation_ids)
    }

    /// 批量开仓（用于加网格）
    pub async fn open_grid_position(
        &mut self,
        side: exness_api::PositionSide,
        volume: Decimal,
        layer: usize,
    ) -> RuntimeResult<String> {
        let idempotency_key = format!("grid_open_{:?}_layer{}_{}_{}",
            side,
            layer,
            volume,
            chrono::Utc::now().timestamp_millis()
        );

        info!("开网格仓: {:?} {} 手 layer={}", side, volume, layer);
        
        let operation_id = self.executor.open_position(
            &idempotency_key,
            side,
            volume,
            20, // deviation
        ).await?;

        // 记录在途操作
        self.ops_tracker.track_open(
            operation_id.clone(),
            crate::ledger::PositionRole::Grid,
            side,
            volume,
        );

        Ok(operation_id)
    }

    /// 更新持仓统计 metrics
    fn update_position_metrics(&self) {
        use std::collections::HashMap;
        let mut stats: HashMap<(PositionRole, PositionSide), usize> = HashMap::new();

        for entry in self.ledger.all_positions() {
            *stats.entry((entry.role, entry.side)).or_insert(0) += 1;
        }

        for ((role, side), count) in stats {
            let role_str = match role {
                PositionRole::Grid => "grid",
                PositionRole::Lock => "lock",
                PositionRole::Foreign => "foreign",
            };
            let side_str = match side {
                PositionSide::Buy => "buy",
                PositionSide::Sell => "sell",
            };
            crate::metrics::update_positions(&self.config.account_id, role_str, side_str, count);
        }

        // 更新 OpsTracker 待处理数
        crate::metrics::update_ops_pending(&self.config.account_id, self.ops_tracker.pending_count());
    }
}
