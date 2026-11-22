//! 数据模型（输入/输出 DTO + 领域类型）。

use serde::{Deserialize, Serialize};

// ========== 枚举 ==========

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PositionSide {
    Buy,
    Sell,
}

impl PositionSide {
    pub fn to_string_value(&self) -> &'static str {
        match self {
            PositionSide::Buy => "buy",
            PositionSide::Sell => "sell",
        }
    }

    pub fn from_string_value(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "buy" => Some(PositionSide::Buy),
            "sell" => Some(PositionSide::Sell),
            _ => None,
        }
    }
}

// ========== 错误响应 ==========

#[derive(Debug, Clone, Deserialize)]
pub struct ErrorResponse {
    pub code: i32,
    pub error_message: String,
}

// ========== 配置 ==========

#[derive(Debug, Clone, Deserialize)]
pub struct AccountSettings {
    #[serde(default)]
    pub currency: String,
    #[serde(default)]
    pub leverage: Option<String>,
    #[serde(default)]
    pub account_status: Option<String>,
    #[serde(default)]
    pub trade_mode: Option<String>,
    #[serde(default)]
    pub execution_mode: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct AccountDetailsRaw {
    pub account_id: String,
    #[serde(default)]
    pub account_name: Option<String>,
    #[serde(default)]
    pub create_date: Option<String>,
    #[serde(default)]
    pub settings: Option<AccountSettings>,
    #[serde(default)]
    pub currency: Option<String>,
    #[serde(default)]
    pub server: Option<String>,
    #[serde(default)]
    pub login: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AccountDetails {
    pub account_id: String,
    pub currency: String,
    pub server: Option<String>,
    pub login: Option<String>,
    pub account_name: Option<String>,
    pub create_date: Option<String>,
    pub settings: Option<AccountSettings>,
}

impl<'de> serde::Deserialize<'de> for AccountDetails {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = AccountDetailsRaw::deserialize(deserializer)?;
        let currency = raw
            .currency
            .filter(|c| !c.is_empty())
            .or_else(|| raw.settings.as_ref().map(|s| s.currency.clone()))
            .unwrap_or_default();
        Ok(AccountDetails {
            account_id: raw.account_id,
            currency,
            server: raw.server,
            login: raw.login,
            account_name: raw.account_name,
            create_date: raw.create_date,
            settings: raw.settings,
        })
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct InstrumentNameList {
    pub instruments: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct InstrumentCondition {
    #[serde(alias = "instrument")]
    pub symbol: String,
    #[serde(default)]
    pub base_currency: Option<String>,
    #[serde(default)]
    pub quote_currency: Option<String>,
    #[serde(default)]
    pub point_digits: Option<u32>,
    #[serde(default)]
    pub volume_min: Option<String>,
    #[serde(default)]
    pub volume_max: Option<String>,
    #[serde(default)]
    pub volume_step: Option<String>,
    #[serde(default)]
    pub contract_size: Option<String>,
    #[serde(default)]
    pub trade_mode: Option<String>,
    #[serde(default)]
    pub trade_sessions: Vec<InstrumentTradeSession>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct InstrumentTradeSession {
    #[serde(default)]
    pub trade_mode: Option<String>,
    #[serde(default)]
    pub week_start_time: Option<String>,
    #[serde(default)]
    pub week_end_time: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LimitsResponse {
    pub limits: ApiLimits,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ApiLimits {
    pub requests_per_second: i32,
    pub orders_per_day: i32,
}

// ========== 接入点发现 ==========

#[derive(Debug, Clone, Deserialize)]
pub struct AccessPointResponse {
    pub access_point: String,
    pub account_id: String,
}

// ========== 账户快照 ==========

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountSnapshot {
    pub balance: String,
    pub equity: String,
    #[serde(rename = "used_margin", alias = "margin")]
    pub used_margin: String,
}

// ========== 持仓 ==========

#[derive(Debug, Clone, Deserialize)]
pub struct PositionsResponse {
    pub data: Vec<Position>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Position {
    #[serde(alias = "position_id")]
    pub id: String,
    #[serde(alias = "instrument")]
    pub symbol: String,
    #[serde(rename = "type", alias = "direction")]
    pub position_type: String, // "buy" | "sell"
    pub volume: String,
    #[serde(default)]
    pub open_price: String,
    #[serde(default)]
    pub current_price: String,
    #[serde(default)]
    pub profit: String,
    #[serde(default)]
    pub swap: String,
    #[serde(default)]
    pub commission: String,
    #[serde(default, alias = "create_time", deserialize_with = "deserialize_epoch_seconds")]
    pub open_time: i64,
    #[serde(default)]
    pub comment: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OpenPositionRequest {
    pub instrument: String,
    pub side: String, // "buy" | "sell"
    pub volume: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_loss_price: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub take_profit_price: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deviation: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}

// ========== 操作记录 ==========

#[derive(Debug, Clone, Deserialize)]
pub struct OperationAck {
    pub operation_id: String,
    pub client_request_id: String,
    pub status: String, // "accepted"
}

#[derive(Debug, Clone, Deserialize)]
pub struct OperationResult {
    pub operation_id: String,
    pub status: String, // "completed" | "failed" | "pending"
    pub position_id: Option<String>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
}

// ========== K 线 ==========

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Candle {
    #[serde(deserialize_with = "deserialize_epoch_seconds")]
    pub time: i64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
}

// ========== WebSocket 事件 ==========

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "event_type")]
pub enum WsEvent {
    #[serde(rename = "transaction_event")]
    Transaction(TransactionEvent),
    #[serde(rename = "instrument_event")]
    Instrument(InstrumentEvent),
    #[serde(other)]
    Other,
}

#[derive(Debug, Clone, Deserialize)]
pub struct InstrumentEvent {
    pub action: String,
    pub payload: InstrumentCondition,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TransactionEvent {
    pub operation_id: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub payload: TransactionPayload,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct TransactionPayload {
    #[serde(default)]
    pub orders: Vec<Order>,
    #[serde(default)]
    pub positions: Vec<Position>,
    #[serde(default)]
    pub deals: Vec<Deal>,
    #[serde(default)]
    pub account_state: Option<AccountSnapshot>,
}

// ========== Tick 数据 ==========

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tick {
    #[serde(alias = "instrument")]
    pub symbol: String,
    pub bid: f64,
    pub ask: f64,
    #[serde(default, alias = "timestamp")]
    pub timestamp_str: Option<String>,
    #[serde(default)]
    pub time: i64,
}

impl Tick {
    pub fn spread(&self) -> f64 {
        self.ask - self.bid
    }

    pub fn time_ms(&self) -> i64 {
        if self.time > 0 {
            return self.time;
        }
        if let Some(ref ts) = self.timestamp_str {
            if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(ts) {
                return dt.timestamp_millis();
            }
        }
        chrono::Utc::now().timestamp_millis()
    }
}

// ========== 订单 ==========

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Order {
    #[serde(default, alias = "order_id")]
    pub id: String,
    #[serde(default, alias = "instrument")]
    pub symbol: String,
    #[serde(default, alias = "order_type")]
    pub order_type: String,
    #[serde(default)]
    pub volume: String,
    #[serde(default, alias = "price")]
    pub open_price: String,
    #[serde(default, alias = "create_time", deserialize_with = "deserialize_epoch_seconds")]
    pub open_time: i64,
}

// ========== REST 专用类型 ==========

#[derive(Debug, Clone, Serialize)]
pub struct CandleQuery {
    pub instrument: String,
    pub price_type: String,
    pub timeframe: String,
    pub from: chrono::DateTime<chrono::Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandleHistoryResponse {
    pub candles: Vec<Candle>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfitAmount {
    #[serde(default)]
    pub amount: String,
    #[serde(default)]
    pub currency: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Deal {
    pub deal_id: String,
    #[serde(default)]
    pub order_id: Option<String>,
    #[serde(default)]
    pub position_id: Option<String>,
    #[serde(rename = "type")]
    pub deal_type: String,
    #[serde(default)]
    pub direction: Option<String>,
    #[serde(default)]
    pub instrument: Option<String>,
    #[serde(default)]
    pub volume: Option<String>,
    #[serde(default)]
    pub price: Option<String>,
    #[serde(default)]
    pub profit: Option<ProfitAmount>,
    #[serde(default)]
    pub create_time: Option<String>,
    #[serde(default)]
    pub commission: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DealHistoryItem {
    pub event_time: String,
    pub deal: Deal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DealsHistoryResponse {
    #[serde(default)]
    pub items: Vec<DealHistoryItem>,
    #[serde(default)]
    pub next_cursor: Option<String>,
    #[serde(default)]
    pub has_more: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradingStateSnapshot {
    #[serde(default)]
    pub account_state: Option<AccountSnapshot>,
    #[serde(default)]
    pub positions: Vec<Position>,
    #[serde(default)]
    pub orders: Vec<Order>,
}

pub type AckResponse = OperationAck;

#[derive(Debug, Clone, Default, Serialize)]
pub struct ClosePositionParams {
    pub position_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deviation: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub close_by_position_id: Option<String>,
}

pub type OperationStatusResponse = OperationResult;

/// 反序列化 API 以 RFC3339 字符串形式发送的 epoch 秒时间戳
/// （例如 "2026-11-20T13:08:00+00:00"），同时兼容原始整数。
fn deserialize_epoch_seconds<'de, D>(deserializer: D) -> Result<i64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(serde::Deserialize)]
    #[serde(untagged)]
    enum TimeRepr {
        Int(i64),
        Str(String),
    }
    match TimeRepr::deserialize(deserializer)? {
        TimeRepr::Int(t) => Ok(t),
        TimeRepr::Str(s) => chrono::DateTime::parse_from_rfc3339(&s)
            .map(|dt| dt.timestamp())
            .map_err(serde::de::Error::custom),
    }
}

