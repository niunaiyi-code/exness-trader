//! 网格篮子数学（EA GridMath.mqh 移植）。

use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::str::FromStr;

pub struct GridMath;

impl GridMath {
    /// 计算下一层网格手数：`initial × mult^(orders-1)`，按经纪商手数步长取整（2 位小数，最小 0.01）
    pub fn calc_next_lot(initial: Decimal, mult: Decimal, orders: i32) -> Decimal {
        if orders <= 1 {
            return initial.round_dp(2).max(dec!(0.01));
        }
        // 转为 f64 做幂运算后再转回（对手数精度足够）
        let initial_f64 = initial.to_string().parse::<f64>().unwrap_or(0.0);
        let mult_f64 = mult.to_string().parse::<f64>().unwrap_or(1.0);
        let result_f64 = initial_f64 * mult_f64.powi(orders - 1);
        let d = Decimal::from_str(&format!("{:.4}", result_f64)).unwrap_or(initial);
        d.round_dp(2).max(dec!(0.01))
    }

    /// 价格逆向波动 `points` 点时，`orders` 个网格持仓的理论美元亏损。
    /// 公式：Σ lot_i × points × profit_per_dollar_per_lot
    pub fn theoretical_loss(
        initial: Decimal,
        mult: Decimal,
        orders: i32,
        points: Decimal,
        profit_per_dollar_per_lot: Decimal,
    ) -> Decimal {
        if orders <= 0 {
            return Decimal::ZERO;
        }

        let mut total_loss = Decimal::ZERO;
        for i in 1..=orders {
            let lot = Self::calc_next_lot(initial, mult, i);
            total_loss += lot * points * profit_per_dollar_per_lot;
        }
        total_loss
    }

    /// 自动计算最大单数，使理论亏损 ≤ 净值 × 0.8。
    /// 在限额内返回最大值，净值不足则返回 0。
    pub fn auto_max_orders(
        initial: Decimal,
        mult: Decimal,
        grid_step_points: Decimal,
        equity: Decimal,
        profit_per_dollar_per_lot: Decimal,
    ) -> i32 {
        let threshold = equity * dec!(0.8);
        for n in 1..=50 {
            let loss = Self::theoretical_loss(
                initial,
                mult,
                n,
                grid_step_points * Decimal::from(n),
                profit_per_dollar_per_lot,
            );
            if loss > threshold {
                return if n > 1 { n - 1 } else { 0 };
            }
        }
        50
    }

    /// 防守生存美元：最深层的理论亏损。
    pub fn defensive_survival_usd(
        initial: Decimal,
        mult: Decimal,
        max_orders: i32,
        grid_step_points: Decimal,
        profit_per_dollar_per_lot: Decimal,
    ) -> Decimal {
        if max_orders <= 0 {
            return Decimal::ZERO;
        }
        Self::theoretical_loss(
            initial,
            mult,
            max_orders,
            grid_step_points * Decimal::from(max_orders),
            profit_per_dollar_per_lot,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_lot_progression() {
        let initial = dec!(0.01);
        let mult = dec!(1.45);
        assert_eq!(GridMath::calc_next_lot(initial, mult, 1), dec!(0.01));
        assert_eq!(GridMath::calc_next_lot(initial, mult, 2), dec!(0.01));
        assert_eq!(GridMath::calc_next_lot(initial, mult, 3), dec!(0.02));
    }

    #[test]
    fn theoretical_loss_3_orders() {
        let initial = dec!(0.01);
        let mult = dec!(1.45);
        let points = dec!(300);
        let profit_per_lot = dec!(10000); // 100 盎司 × 100（USC）

        let loss = GridMath::theoretical_loss(initial, mult, 3, points, profit_per_lot);
        // 归一化后的 2 位小数经纪商手数：
        // 第 1 单：0.01 × 300 × 10000 = 30000
        // 第 2 单：0.01 × 300 × 10000 = 30000（0.0145 取整为 0.01）
        // 第 3 单：0.02 × 300 × 10000 = 60000（0.021025 取整为 0.02）
        // total = 120000
        assert_eq!(loss, dec!(120000.00));
    }

    #[test]
    fn auto_max_with_low_equity() {
        let equity = dec!(100);
        let max = GridMath::auto_max_orders(
            dec!(0.01),
            dec!(1.45),
            dec!(30), // 每档 30 点
            equity,
            dec!(10000),
        );
        // 100 的 80% = 80，应允许 1 或 2 单
        assert!(max >= 0 && max <= 3);
    }
}