//! 分层止盈引擎（EA RiskManager.mqh 止盈逻辑移植）。

use rust_decimal::Decimal;

#[derive(Debug, Clone, PartialEq)]
pub enum TpDecision {
    /// 立即以市价平仓（cushion <35 点，已达到 100% 目标）。
    CloseNow,
    /// 以 25 点步长移动止盈（cushion ≥35 点）。
    Trail { trigger_profit: Decimal },
    /// 持有（未达目标或未触发移动止盈）。
    Hold,
}

pub struct TpEngine;

impl TpEngine {
    /// 按 cushion 决定平掉篮子还是移动止盈。
    ///
    /// 当 cushion_points ≥ MIN_TRAILING_CUSHION_POINTS (35.0) 时：
    ///   → Trail with 25-pt fixed step (博取超额暴利).
    /// 当 cushion_points < 35.0 时：
    ///   → Close at 100% target (秒平落袋保本，防止回撤吞噬利润).
    pub fn decide(
        current_profit: Decimal,
        required_profit: Decimal,
        total_lots: Decimal,
        profit_per_dollar_per_lot: Decimal,
        pip: Decimal,
        trailing_cushion_points: Decimal,
        _trailing_step_points: Decimal,
    ) -> TpDecision {
        if total_lots <= Decimal::ZERO || profit_per_dollar_per_lot <= Decimal::ZERO {
            return TpDecision::Hold;
        }

        // cushion_points = req_profit / (m_lots × ProfitPerDollarPerLot × pip)
        let cushion_points = required_profit / (total_lots * profit_per_dollar_per_lot * pip);

        if cushion_points >= trailing_cushion_points {
            // 安全浅层：激活 25 点移动止盈
            // MQL5: 达到100%目标且安全垫≥35点 → 立即激活trailing
            if current_profit >= required_profit {
                TpDecision::Trail { trigger_profit: required_profit }
            } else {
                TpDecision::Hold
            }
        } else {
            // 深层重仓：在 100% 目标位平仓
            if current_profit >= required_profit {
                TpDecision::CloseNow
            } else {
                TpDecision::Hold
            }
        }
    }

    /// 按当前层数计算抛物线所需盈利。
    ///
    /// 公式（MQL5 GridBasket.mqh:303-309）：
    /// ```text
    /// x = (n - 1) / (max_n - 1)    // 归一化层数进度 [0, 1]
    /// k = GRID_TP_PARABOLA_K × t_start
    /// req_profit = t_start × (1 - x) + t_min × x + k × x × (1 - x)
    /// ```
    ///
    /// - `n=1` (首单): `req_profit = t_start`
    /// - `n=max_n` (满层): `req_profit = t_min`
    /// - 中间层: 抛物线插值（浅层高利润，深层快速收窄到保本底线）
    pub fn parabolic_required_profit(
        current_count: usize,
        max_orders: usize,
        initial_lot: Decimal,
        profit_per_dollar_per_lot: Decimal,
        tp_distance: Decimal,
        parabola_k: Decimal,
        min_tp: Decimal,
    ) -> Decimal {
        if current_count == 0 {
            return Decimal::ZERO;
        }

        let n = current_count.min(max_orders);
        let max_n = max_orders.max(1); // 防止除零

        // t_start: 首单初始目标利润 = initial_lot × profit_per_lot × tp_distance
        let t_start = initial_lot * profit_per_dollar_per_lot * tp_distance;

        // x: 归一化进度 [0, 1]
        let x = if max_n > 1 {
            Decimal::from(n.saturating_sub(1)) / Decimal::from(max_n - 1)
        } else {
            Decimal::ZERO
        };

        // 抛物线插值
        let k = parabola_k * t_start;
        let req_profit = t_start * (Decimal::ONE - x) + min_tp * x + k * x * (Decimal::ONE - x);

        // 保证不低于底线
        req_profit.max(min_tp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn low_cushion_close_at_100_percent() {
        let decision = TpEngine::decide(
            dec!(10.0),       // 当前盈利
            dec!(10.0),       // 所需值（100%）
            dec!(0.05),       // 总手数（重仓）
            dec!(10000),      // profit_per_lot
            dec!(0.1),        // pip 0.1
            dec!(35.0),       // 最小缓冲
            dec!(25.0),       // 移动步幅
        );
        // cushion = 10 / (0.05 × 10000 × 0.1) = 10 / 50 = 0.2 点 < 35 → CloseNow
        assert_eq!(decision, TpDecision::CloseNow);
    }

    #[test]
    fn high_cushion_trail() {
        let decision = TpEngine::decide(
            dec!(350.0),      // 当前盈利 (达到100%目标)
            dec!(350.0),      // 所需值
            dec!(0.01),       // 总手数（轻仓）
            dec!(10000),
            dec!(0.1),
            dec!(35.0),
            dec!(25.0),
        );
        // cushion = 350 / (0.01 × 10000 × 0.1) = 350 / 10 = 35 点 ≥ 35 → Trail
        // MQL5: profit >= required → 立即激活trailing
        match decision {
            TpDecision::Trail { trigger_profit } => {
                assert_eq!(trigger_profit, dec!(350.0));
            }
            _ => panic!("Expected Trail, got {:?}", decision),
        }
    }

    #[test]
    fn high_cushion_trail_activated() {
        let decision = TpEngine::decide(
            dec!(600.0),      // 当前盈利 (超过100%目标)
            dec!(350.0),      // 所需值
            dec!(0.01),       // 总手数
            dec!(10000),
            dec!(0.1),
            dec!(35.0),
            dec!(25.0),
        );
        // cushion = 350 / 10 = 35 点 ≥ 35 → Trail
        // profit=600 >= required=350 → Trail
        match decision {
            TpDecision::Trail { trigger_profit } => {
                assert_eq!(trigger_profit, dec!(350));
            }
            _ => panic!("Expected Trail, got {:?}", decision),
        }
    }

    #[test]
    fn parabolic_first_layer() {
        // n=1, initial_lot=0.01, profit_per_lot=10000, tp_distance=100, max=8
        // t_start = 0.01 × 10000 × 100 = 10000
        // x = (1-1)/(8-1) = 0
        // req = 10000 × (1-0) + 1 × 0 + k × 0 × 1 = 10000
        let req = TpEngine::parabolic_required_profit(
            1,           // current_count
            8,           // max_orders
            dec!(0.01),  // initial_lot
            dec!(10000), // profit_per_dollar_per_lot
            dec!(100),   // tp_distance
            dec!(9),     // parabola_k
            dec!(1),     // min_tp
        );
        assert_eq!(req, dec!(10000));
    }

    #[test]
    fn parabolic_last_layer() {
        // n=8, max=8
        // x = (8-1)/(8-1) = 1
        // req = 10000 × 0 + 1 × 1 + k × 1 × 0 = 1
        let req = TpEngine::parabolic_required_profit(
            8,
            8,
            dec!(0.01),
            dec!(10000),
            dec!(100),
            dec!(9),
            dec!(1),
        );
        assert_eq!(req, dec!(1));
    }

    #[test]
    fn parabolic_middle_layer() {
        // n=4, max=8
        // x = (4-1)/(8-1) = 3/7
        // t_start = 10000
        // k = 9 × 10000 = 90000
        // req = 10000×(1-3/7) + 1×(3/7) + 90000×(3/7)×(4/7)
        let req = TpEngine::parabolic_required_profit(
            4,
            8,
            dec!(0.01),
            dec!(10000),
            dec!(100),
            dec!(9),
            dec!(1),
        );
        // 实际计算结果约 27755.53
        let expected = dec!(27755.53);
        let diff = (req - expected).abs();
        assert!(diff < dec!(1), "req={}, expected={}", req, expected);
    }
}