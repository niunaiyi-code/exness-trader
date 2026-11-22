//! 交易执行适配器 — 对接 Exness API 开仓/平仓。
//! （docs/02 §5、04 §4-5）。

use gl_strategy::Side;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 执行模式（市价 / 带偏差的即时）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecutionMode {
    Market,
    Instant,
}

/// 交易执行器 — 按 Exness API 语义开仓/平仓。
pub struct TradeExecutor {
    /// 来自行情数据的当前 bid/ask。
    pub bid: Decimal,
    pub ask: Decimal,
    /// pip = 策略点值（XAUUSD 为 0.01）。
    pub pip: Decimal,
    /// profit_per_dollar_per_lot（XAUUSD/USD 为 100）。
    pub profit_per_dollar_per_lot: Decimal,
    /// 手数规格。
    pub min_lot: Decimal,
    pub max_lot: Decimal,
    pub lot_step: Decimal,
    /// 执行模式与偏差。
    pub exec_mode: ExecutionMode,
    pub deviation_points: Decimal,
    /// 在途操作（operation_id -> 待成交订单）。
    pub pending_ops: HashMap<String, PendingOp>,
    /// 上次拒绝信息（用于重试决策）。
    pub last_rejection: Option<OrderRejection>,
}

#[derive(Debug, Clone)]
pub struct PendingOp {
    pub client_request_id: String,
    pub operation_id: String,
    pub intent: OpIntent,
    pub timestamp_ms: u64,
}

#[derive(Debug, Clone)]
pub enum OpIntent {
    Open { side: Side, volume: Decimal, layer: usize },
    Close { position_id: String, volume: Option<Decimal> },
}

#[derive(Debug, Clone)]
pub struct OrderRejection {
    pub error_code: String,
    pub error_message: String,
    pub timestamp: i64,
}

impl TradeExecutor {
    pub fn new(
        bid: Decimal,
        ask: Decimal,
        pip: Decimal,
        ppd: Decimal,
        min_lot: Decimal,
        max_lot: Decimal,
        lot_step: Decimal,
        exec_mode: ExecutionMode,
        deviation_points: Decimal,
    ) -> Self {
        Self {
            bid,
            ask,
            pip,
            profit_per_dollar_per_lot: ppd,
            min_lot,
            max_lot,
            lot_step,
            exec_mode,
            deviation_points,
            pending_ops: HashMap::new(),
            last_rejection: None,
        }
    }

    /// 更新市场价格。
    pub fn update_market(&mut self, bid: Decimal, ask: Decimal) {
        self.bid = bid;
        self.ask = ask;
    }

    /// 获取当前点差（美元）。
    pub fn spread_usd(&self) -> Decimal {
        self.ask - self.bid
    }

    /// 按规格归一化手数（min/max/step）。
    pub fn normalize_lot(&self, lot: Decimal) -> Decimal {
        if self.lot_step.is_zero() {
            return lot.max(self.min_lot).min(self.max_lot);
        }
        let snapped = (lot / self.lot_step).round() * self.lot_step;
        let normalized = snapped.max(self.min_lot).min(self.max_lot);
        
        // ⚠️ 防御：若归一化后仍低于最小值，强制返回最小值
        if normalized < self.min_lot {
            tracing::warn!(
                "手数 {} 归一化后 {} 仍低于最小值 {}，强制使用最小值",
                lot, normalized, self.min_lot
            );
            return self.min_lot;
        }
        
        normalized
    }

    /// 开仓（买入/卖出）。
    /// 返回：(用于幂等的 client_request_id, requested_price, deviation_units)。
    pub fn open_position(
        &mut self,
        side: Side,
        volume: Decimal,
        layer: usize,
        now_ms: u64,
    ) -> OpenRequest {
        let normalized_vol = self.normalize_lot(volume);
        
        // ⚠️ 二次检查：归一化后的手数必须有效
        if normalized_vol.is_zero() || normalized_vol < self.min_lot {
            tracing::error!(
                "归一化后的手数 {} 无效（原始 {}，最小值 {}），使用最小值",
                normalized_vol, volume, self.min_lot
            );
            // 不返回错误，而是强制使用最小值继续
        }
        
        let price = match side {
            Side::Buy => self.ask,
            Side::Sell => self.bid,
        };
        let deviation_units = match self.exec_mode {
            ExecutionMode::Market => Decimal::ZERO,
            ExecutionMode::Instant => self.deviation_points,
        };
        let client_request_id = format!(
            "gl.{}.grid.{}.{}",
            layer,
            if side == Side::Buy { "buy" } else { "sell" },
            now_ms
        );

        OpenRequest {
            side,
            volume: normalized_vol,
            price,
            deviation_units,
            comment: format!(
                "GoldLegend_{}_{}",
                if side == Side::Buy { "Buy" } else { "Sell" },
                layer
            ),
            client_request_id,
        }
    }

    /// 平仓（全部或部分）。
    /// 返回：(client_request_id, close_price, deviation_units)。
    pub fn close_position(
        &mut self,
        position_id: &str,
        volume: Option<Decimal>,
        emergency: bool,
        now_ms: u64,
    ) -> CloseRequest {
        let price = if self.bid > Decimal::ZERO { self.bid } else { self.ask };
        let deviation_units = match self.exec_mode {
            ExecutionMode::Market => Decimal::ZERO,
            ExecutionMode::Instant => {
                if emergency {
                    Decimal::from(300) // 紧急：300 点
                } else {
                    self.deviation_points
                }
            }
        };
        let client_request_id = format!("gl.close.{}.{}", position_id, now_ms);

        CloseRequest {
            position_id: position_id.to_string(),
            volume,
            price,
            deviation_units,
            client_request_id,
        }
    }

    /// 登记在途操作（API 调用之前）。
    pub fn register_operation(
        &mut self,
        client_request_id: String,
        intent: OpIntent,
        now_ms: u64,
    ) {
        // 占位：operation_id 在 API ACK 后赋值。
        let pending = PendingOp {
            client_request_id: client_request_id.clone(),
            operation_id: String::new(), // 将在 API ACK 后由 OpsTracker 填充
            intent,
            timestamp_ms: now_ms,
        };
        self.pending_ops.insert(client_request_id, pending);
    }

    /// 确认操作（API 返回 202 ACK 并带 operation_id 之后）。
    pub fn confirm_operation(&mut self, client_request_id: &str, operation_id: &str) {
        if let Some(pending) = self.pending_ops.get_mut(client_request_id) {
            pending.operation_id = operation_id.to_string();
        }
    }

    /// 将操作标记为已完成（成功或拒绝）。
    pub fn finish_operation(&mut self, client_request_id: &str) -> Option<PendingOp> {
        self.pending_ops.remove(client_request_id)
    }

    /// 记录拒绝（用于重试决策）。
    pub fn record_rejection(&mut self, code: String, msg: String, now: i64) {
        self.last_rejection = Some(OrderRejection {
            error_code: code,
            error_message: msg,
            timestamp: now,
        });
    }

    /// 上次拒绝是否应重试？（4xx 认证/校验错误不重试）。
    pub fn should_retry_last_rejection(&self) -> bool {
        match &self.last_rejection {
            None => false,
            Some(rej) => {
                // 401、403、400（认证/校验）不重试。
                !rej.error_code.starts_with("4")
            }
        }
    }

    /// 检查某一方向是否有在途开仓操作（防止重复入场）。
    pub fn has_pending_open(&self, side: Side) -> bool {
        self.pending_ops.values().any(|op| {
            if let OpIntent::Open { side: op_side, .. } = &op.intent {
                *op_side == side
            } else {
                false
            }
        })
    }
}

/// 开仓请求。
#[derive(Debug, Clone)]
pub struct OpenRequest {
    pub side: Side,
    pub volume: Decimal,
    pub price: Decimal,
    pub deviation_units: Decimal,
    pub comment: String,
    pub client_request_id: String,
}

/// 平仓请求。
#[derive(Debug, Clone)]
pub struct CloseRequest {
    pub position_id: String,
    pub volume: Option<Decimal>,
    pub price: Decimal,
    pub deviation_units: Decimal,
    pub client_request_id: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> Decimal {
        s.parse().unwrap()
    }

    #[test]
    fn executor_new() {
        let exec = TradeExecutor::new(
            d("3700"),
            d("3700.5"),
            d("0.01"),
            d("100"),
            d("0.01"),
            d("200"),
            d("0.01"),
            ExecutionMode::Instant,
            d("20"),
        );
        assert_eq!(exec.bid, d("3700"));
        assert_eq!(exec.ask, d("3700.5"));
        assert_eq!(exec.spread_usd(), d("0.5"));
    }

    #[test]
    fn normalize_lot() {
        let exec = TradeExecutor::new(
            d("3700"),
            d("3700.5"),
            d("0.01"),
            d("100"),
            d("0.01"),
            d("200"),
            d("0.01"),
            ExecutionMode::Instant,
            d("20"),
        );
        assert_eq!(exec.normalize_lot(d("0.015")), d("0.02")); // 向上取整到步长
        assert_eq!(exec.normalize_lot(d("0.009")), d("0.01")); // 最小值
        assert_eq!(exec.normalize_lot(d("250")), d("200")); // 最大值
    }

    #[test]
    fn open_position_buy() {
        let mut exec = TradeExecutor::new(
            d("3700"),
            d("3700.5"),
            d("0.01"),
            d("100"),
            d("0.01"),
            d("200"),
            d("0.01"),
            ExecutionMode::Instant,
            d("20"),
        );
        let req = exec.open_position(Side::Buy, d("0.01"), 0, 1000);
        assert_eq!(req.side, Side::Buy);
        assert_eq!(req.volume, d("0.01"));
        assert_eq!(req.price, d("3700.5")); // 买入用 Ask 价
        assert_eq!(req.deviation_units, d("20"));
    }

    #[test]
    fn open_position_sell() {
        let mut exec = TradeExecutor::new(
            d("3700"),
            d("3700.5"),
            d("0.01"),
            d("100"),
            d("0.01"),
            d("200"),
            d("0.01"),
            ExecutionMode::Instant,
            d("20"),
        );
        let req = exec.open_position(Side::Sell, d("0.02"), 1, 1000);
        assert_eq!(req.side, Side::Sell);
        assert_eq!(req.volume, d("0.02"));
        assert_eq!(req.price, d("3700")); // 卖出用 Bid 价
    }

    #[test]
    fn close_position_normal() {
        let mut exec = TradeExecutor::new(
            d("3700"),
            d("3700.5"),
            d("0.01"),
            d("100"),
            d("0.01"),
            d("200"),
            d("0.01"),
            ExecutionMode::Instant,
            d("20"),
        );
        let req = exec.close_position("pos_123", None, false, 1000);
        assert_eq!(req.position_id, "pos_123");
        assert_eq!(req.volume, None); // 全部平仓
        assert_eq!(req.deviation_units, d("20"));
    }

    #[test]
    fn close_position_emergency() {
        let mut exec = TradeExecutor::new(
            d("3700"),
            d("3700.5"),
            d("0.01"),
            d("100"),
            d("0.01"),
            d("200"),
            d("0.01"),
            ExecutionMode::Instant,
            d("20"),
        );
        let req = exec.close_position("pos_123", Some(d("0.01")), true, 1000);
        assert_eq!(req.deviation_units, d("300")); // 紧急：300 点
    }

    #[test]
    fn has_pending_open() {
        let mut exec = TradeExecutor::new(
            d("3700"),
            d("3700.5"),
            d("0.01"),
            d("100"),
            d("0.01"),
            d("200"),
            d("0.01"),
            ExecutionMode::Instant,
            d("20"),
        );
        exec.register_operation(
            "op1".to_string(),
            OpIntent::Open { side: Side::Buy, volume: d("0.01"), layer: 0 },
            1000,
        );
        assert!(exec.has_pending_open(Side::Buy));
        assert!(!exec.has_pending_open(Side::Sell));
    }
}
