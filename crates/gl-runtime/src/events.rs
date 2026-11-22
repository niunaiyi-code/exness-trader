use serde::{Deserialize, Serialize};

/// 交易事件通知（发往前端 Web / App 实时驱动界面与音效）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeNotification {
    pub account_id: String,
    pub event_id: String,
    /// 明确的交易事件类型: "open_grid" | "open_lock" | "close_tp" | "close_sl" | "close_lock"
    pub event_type: String,
    pub position_id: Option<String>,
    pub symbol: String,
    pub side: String,
    pub volume: String,
    pub price: Option<String>,
    pub profit: Option<String>,
    pub timestamp: i64,
}
