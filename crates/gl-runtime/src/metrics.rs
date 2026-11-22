//! Prometheus metrics for trading operations

use lazy_static::lazy_static;
use prometheus::{
    register_counter_vec, register_gauge_vec, register_histogram_vec, CounterVec, GaugeVec,
    HistogramVec,
};

lazy_static! {
    /// 开仓操作计数
    pub static ref POSITION_OPEN_TOTAL: CounterVec = register_counter_vec!(
        "trading_position_open_total",
        "Total number of position open operations",
        &["account_id", "side", "role", "status"]
    )
    .unwrap();

    /// 平仓操作计数
    pub static ref POSITION_CLOSE_TOTAL: CounterVec = register_counter_vec!(
        "trading_position_close_total",
        "Total number of position close operations",
        &["account_id", "role", "status"]
    )
    .unwrap();

    /// 对账操作计数
    pub static ref RECONCILE_TOTAL: CounterVec = register_counter_vec!(
        "trading_reconcile_total",
        "Total number of reconciliation operations",
        &["account_id", "status"]
    )
    .unwrap();

    /// 对账变更数量
    pub static ref RECONCILE_CHANGES: GaugeVec = register_gauge_vec!(
        "trading_reconcile_changes",
        "Number of changes in last reconciliation",
        &["account_id", "change_type"]
    )
    .unwrap();

    /// 持仓数量
    pub static ref POSITIONS_OPEN: GaugeVec = register_gauge_vec!(
        "trading_positions_open",
        "Number of open positions",
        &["account_id", "role", "side"]
    )
    .unwrap();

    /// 账户余额
    pub static ref ACCOUNT_BALANCE: GaugeVec = register_gauge_vec!(
        "trading_account_balance",
        "Account balance in USD",
        &["account_id"]
    )
    .unwrap();

    /// 账户权益
    pub static ref ACCOUNT_EQUITY: GaugeVec = register_gauge_vec!(
        "trading_account_equity",
        "Account equity in USD",
        &["account_id"]
    )
    .unwrap();

    /// 浮动盈亏
    pub static ref FLOATING_PL: GaugeVec = register_gauge_vec!(
        "trading_floating_pl",
        "Floating profit/loss in USD",
        &["account_id", "role"]
    )
    .unwrap();

    /// API 请求延迟
    pub static ref API_REQUEST_DURATION: HistogramVec = register_histogram_vec!(
        "trading_api_request_duration_seconds",
        "API request duration in seconds",
        &["account_id", "endpoint", "status"],
        vec![0.01, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0]
    )
    .unwrap();

    /// OpsTracker 待处理操作数
    pub static ref OPS_PENDING: GaugeVec = register_gauge_vec!(
        "trading_ops_pending",
        "Number of pending operations in OpsTracker",
        &["account_id"]
    )
    .unwrap();

    /// WebSocket 重连次数
    pub static ref WS_RECONNECT_TOTAL: CounterVec = register_counter_vec!(
        "trading_ws_reconnect_total",
        "Total number of WebSocket reconnections",
        &["account_id"]
    )
    .unwrap();

    /// 策略决策计数
    pub static ref STRATEGY_DECISION_TOTAL: CounterVec = register_counter_vec!(
        "trading_strategy_decision_total",
        "Total number of strategy decisions",
        &["account_id", "decision_type"]
    )
    .unwrap();
}

/// 记录开仓操作
pub fn record_position_open(account_id: &str, side: &str, role: &str, success: bool) {
    let status = if success { "success" } else { "failed" };
    POSITION_OPEN_TOTAL
        .with_label_values(&[account_id, side, role, status])
        .inc();
}

/// 记录平仓操作
pub fn record_position_close(account_id: &str, role: &str, success: bool) {
    let status = if success { "success" } else { "failed" };
    POSITION_CLOSE_TOTAL
        .with_label_values(&[account_id, role, status])
        .inc();
}

/// 记录对账操作
pub fn record_reconcile(account_id: &str, success: bool, added: usize, removed: usize) {
    let status = if success { "success" } else { "failed" };
    RECONCILE_TOTAL
        .with_label_values(&[account_id, status])
        .inc();

    if success {
        RECONCILE_CHANGES
            .with_label_values(&[account_id, "added"])
            .set(added as f64);
        RECONCILE_CHANGES
            .with_label_values(&[account_id, "removed"])
            .set(removed as f64);
    }
}

/// 更新持仓统计
pub fn update_positions(account_id: &str, role: &str, side: &str, count: usize) {
    POSITIONS_OPEN
        .with_label_values(&[account_id, role, side])
        .set(count as f64);
}

/// 更新账户状态
pub fn update_account_state(account_id: &str, balance: f64, equity: f64) {
    ACCOUNT_BALANCE
        .with_label_values(&[account_id])
        .set(balance);
    ACCOUNT_EQUITY.with_label_values(&[account_id]).set(equity);
}

/// 更新浮动盈亏
pub fn update_floating_pl(account_id: &str, role: &str, pl: f64) {
    FLOATING_PL.with_label_values(&[account_id, role]).set(pl);
}

/// 记录 API 请求
pub fn record_api_request(account_id: &str, endpoint: &str, duration_secs: f64, success: bool) {
    let status = if success { "success" } else { "failed" };
    API_REQUEST_DURATION
        .with_label_values(&[account_id, endpoint, status])
        .observe(duration_secs);
}

/// 更新 OpsTracker 待处理数
pub fn update_ops_pending(account_id: &str, count: usize) {
    OPS_PENDING.with_label_values(&[account_id]).set(count as f64);
}

/// 记录 WebSocket 重连
pub fn record_ws_reconnect(account_id: &str) {
    WS_RECONNECT_TOTAL.with_label_values(&[account_id]).inc();
}

/// 记录策略决策
pub fn record_strategy_decision(account_id: &str, decision_type: &str) {
    STRATEGY_DECISION_TOTAL
        .with_label_values(&[account_id, decision_type])
        .inc();
}
