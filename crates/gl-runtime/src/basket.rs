//! 篮子统计计算器（EA BasketStats 逻辑移植）。

use crate::ledger::{Ledger, PositionRole};
use exness_api::{Position, PositionSide};
use rust_decimal::Decimal;
use std::collections::HashSet;

#[derive(Debug, Clone, Default)]
pub struct BasketStats {
    pub total_lots: Decimal,
    pub profit: Decimal,
    pub weighted_avg_price: Decimal,
    pub count: usize,
}

impl BasketStats {
    /// 根据持仓与当前价格计算篮子统计。
    pub fn calculate(
        ledger: &Ledger,
        role: PositionRole,
        positions: &[Position],
        current_price: Decimal,
        profit_per_dollar_per_lot: Decimal,
        _pip: Decimal,
    ) -> Self {
        let ledger_ids: HashSet<&str> = match role {
            PositionRole::Grid => ledger.grid_positions().map(|e| e.position_id.as_str()).collect(),
            PositionRole::Lock => ledger.lock_positions().map(|e| e.position_id.as_str()).collect(),
            PositionRole::Foreign => HashSet::new(),
        };

        let relevant: Vec<&Position> = positions
            .iter()
            .filter(|p| ledger_ids.contains(p.id.as_str()))
            .collect();

        if relevant.is_empty() {
            return Self::default();
        }

        let mut total_lots = Decimal::ZERO;
        let mut total_profit = Decimal::ZERO;
        let mut weighted_sum = Decimal::ZERO;

        for pos in &relevant {
            let side = match PositionSide::from_string_value(&pos.position_type) {
                Some(s) => s,
                None => {
                    tracing::warn!(
                        position_id = %pos.id,
                        position_type = %pos.position_type,
                        "BasketStats: unknown position_type, skipping from basket calculation"
                    );
                    continue;
                }
            };
            let volume = Decimal::from_str_exact(&pos.volume).unwrap_or_default();
            let open_price = Decimal::from_str_exact(&pos.open_price).unwrap_or_default();

            total_lots += volume;

            // profit = (current_price - open_price) × volume × profit_per_dollar_per_lot × side_multiplier
            let price_diff = current_price - open_price;
            let side_mult = if side == PositionSide::Buy {
                Decimal::ONE
            } else {
                -Decimal::ONE
            };
            let pos_profit = price_diff * volume * profit_per_dollar_per_lot * side_mult;
            total_profit += pos_profit;

            weighted_sum += open_price * volume;
        }

        let weighted_avg_price = if total_lots > Decimal::ZERO {
            weighted_sum / total_lots
        } else {
            Decimal::ZERO
        };

        Self {
            total_lots,
            profit: total_profit,
            weighted_avg_price,
            count: relevant.len(),
        }
    }

    /// 计算指定角色与方向（买入或卖出）的篮子统计。
    pub fn calculate_side(
        ledger: &Ledger,
        role: PositionRole,
        side: PositionSide,
        positions: &[Position],
        bid: Decimal,
        ask: Decimal,
        profit_per_dollar_per_lot: Decimal,
        _pip: Decimal,
    ) -> Self {
        let ledger_ids: HashSet<&str> = match role {
            PositionRole::Grid => ledger
                .grid_positions()
                .filter(|e| e.side == side)
                .map(|e| e.position_id.as_str())
                .collect(),
            PositionRole::Lock => ledger
                .lock_positions()
                .filter(|e| e.side == side)
                .map(|e| e.position_id.as_str())
                .collect(),
            PositionRole::Foreign => HashSet::new(),
        };

        let relevant: Vec<&Position> = positions
            .iter()
            .filter(|p| {
                if !ledger_ids.contains(p.id.as_str()) {
                    return false;
                }
                let pos_side = PositionSide::from_string_value(&p.position_type);
                pos_side == Some(side)
            })
            .collect();

        if relevant.is_empty() {
            return Self::default();
        }

        let mut total_lots = Decimal::ZERO;
        let mut total_profit = Decimal::ZERO;
        let mut weighted_sum = Decimal::ZERO;

        // 买入持仓以 Bid 平仓，卖出持仓以 Ask 平仓
        let exit_price = match side {
            PositionSide::Buy => bid,
            PositionSide::Sell => ask,
        };

        for pos in &relevant {
            let volume = Decimal::from_str_exact(&pos.volume).unwrap_or_default();
            let open_price = Decimal::from_str_exact(&pos.open_price).unwrap_or_default();

            total_lots += volume;

            let price_diff = match side {
                PositionSide::Buy => exit_price - open_price,
                PositionSide::Sell => open_price - exit_price,
            };
            let pos_profit = price_diff * volume * profit_per_dollar_per_lot;
            total_profit += pos_profit;

            weighted_sum += open_price * volume;
        }

        let weighted_avg_price = if total_lots > Decimal::ZERO {
            weighted_sum / total_lots
        } else {
            Decimal::ZERO
        };

        Self {
            total_lots,
            profit: total_profit,
            weighted_avg_price,
            count: relevant.len(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ledger::LedgerEntry;
    use chrono::Utc;
    use rust_decimal_macros::dec;

    #[test]
    fn basket_stats_single_position() {
        let mut ledger = Ledger::new();
        ledger.insert(LedgerEntry {
            position_id: "p1".to_string(),
            role: PositionRole::Grid,
            open_time: Utc::now(),
            volume: dec!(0.01),
            open_price: dec!(2000.0),
            side: PositionSide::Buy,
        });

        let positions = vec![Position {
            id: "p1".to_string(),
            symbol: "XAUUSD".to_string(),
            position_type: "buy".to_string(),
            volume: "0.01".to_string(),
            open_price: "2000.0".to_string(),
            current_price: "2001.0".to_string(),
            profit: "10.0".to_string(),
            swap: "0".to_string(),
            commission: "0".to_string(),
            open_time: Utc::now().timestamp(),
            comment: None,
        }];

        let stats = BasketStats::calculate(
            &ledger,
            PositionRole::Grid,
            &positions,
            dec!(2001.0), // 当前价格 +1 点
            dec!(100),    // profit_per_dollar_per_lot
            dec!(0.1),    // pip
        );

        assert_eq!(stats.count, 1);
        assert_eq!(stats.total_lots, dec!(0.01));
        assert_eq!(stats.weighted_avg_price, dec!(2000.0));
        // profit = 1.0 × 0.01 × 100 = 1.0
        assert_eq!(stats.profit, dec!(1.0));
    }
}
