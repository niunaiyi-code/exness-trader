//! 网格数学 — 直接移植 `GridMath.mqh` / `TradeExecutor.mqh` 中的辅助函数
//! （docs/02 §2）。距离以**点**为单位（1 点 = `10^-point_digits`，XAUUSD 为 0.001）；
//! `pip` 是策略使用的价格点值（XAUUSD 为 0.01）；`ppd` 是价格每变动 1.00 的收益
//! 每手（`contract_size × ccy_factor`，USD 账户的 XAUUSD 为 100）。

use rust_decimal::{Decimal, RoundingStrategy};

/// 美元金额的比较容差（EA 中的 `EPS_PROFIT_USD`）。
fn eps_profit_usd() -> Decimal {
    Decimal::from_str_exact("0.0001").unwrap()
}

/// 评估层数的硬上限（`auto_max_orders` 循环边界）。
pub const MAX_ORDERS_CAP: usize = 100;

/// 品种的手数规格（`volume_min / volume_max / volume_step`）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LotSpec {
    pub min: Decimal,
    pub max: Decimal,
    pub step: Decimal,
}

impl LotSpec {
    pub const fn new(min: Decimal, max: Decimal, step: Decimal) -> Self {
        Self { min, max, step }
    }

    /// Exness `volume_min=0.01 / volume_max=200 / volume_step=0.01`（XAUUSD 实测）。
    pub fn gold_default() -> Self {
        Self::new(
            Decimal::from_str_exact("0.01").unwrap(),
            Decimal::from(200),
            Decimal::from_str_exact("0.01").unwrap(),
        )
    }
}

/// `normalize_lots`：吸附到 `step`，钳制到 `[min, max]`，按步长小数位取整。
pub fn normalize_lots(lots: Decimal, spec: &LotSpec) -> Decimal {
    let snapped = if spec.step.is_zero() {
        lots
    } else {
        (lots / spec.step).round_dp_with_strategy(0, RoundingStrategy::MidpointAwayFromZero) * spec.step
    };
    snapped.max(spec.min).min(spec.max).round_dp(spec.step.scale())
}

/// `calc_next_lot`：`initial × mult^count`，归一化后不低于上一层。
pub fn calc_next_lot(initial: Decimal, mult: Decimal, count: usize, last_lot: Decimal, spec: &LotSpec) -> Decimal {
    if mult <= Decimal::ONE {
        return normalize_lots(initial, spec);
    }
    let target = initial * pow_decimal(mult, count);
    let next = normalize_lots(target, spec);
    next.max(last_lot)
}

/// 计算 Decimal 的 `base ^ exp`（通过重复乘法）。
fn pow_decimal(base: Decimal, exp: usize) -> Decimal {
    if exp == 0 {
        Decimal::ONE
    } else {
        (1..exp).fold(base, |acc, _| acc * base)
    }
}

/// 完整层级序列 `L1..Ln`（`L1 = initial_lot`）。
pub fn lot_sequence(initial: Decimal, mult: Decimal, n: usize, spec: &LotSpec) -> Vec<Decimal> {
    let mut lots = Vec::with_capacity(n);
    if n == 0 {
        return lots;
    }
    if mult <= Decimal::ONE {
        return vec![normalize_lots(initial, spec); n];
    }
    lots.push(normalize_lots(initial, spec));
    for k in 1..n {
        let last = lots[k - 1];
        lots.push(calc_next_lot(initial, mult, k, last, spec));
    }
    lots
}

/// 各层相对首次入场的累计距离（点）：
/// `cum[0] = 0`，`cum[j] = cum[j-1] + (base_step + (j-1)·step_add)`。
pub fn cumulative_distances(base_step: Decimal, step_add: Decimal, n: usize) -> Vec<Decimal> {
    let mut cum = Vec::with_capacity(n);
    let mut acc = Decimal::ZERO;
    for j in 0..n {
        if j > 0 {
            acc += base_step + step_add * Decimal::from(j - 1);
        }
        cum.push(acc);
    }
    cum
}

/// `theoretical_loss`：价格逆向走过 `cum[n-1]` 点时的浮动亏损，
/// 篮子（最差一层恰好盈亏平衡）。返回 `(loss_usd, total_lots)`。
pub fn theoretical_loss(lots: &[Decimal], base_step: Decimal, step_add: Decimal, pip: Decimal, ppd: Decimal) -> (Decimal, Decimal) {
    let n = lots.len();
    if n == 0 {
        return (Decimal::ZERO, Decimal::ZERO);
    }
    let cum = cumulative_distances(base_step, step_add, n);
    let cur = cum[n - 1];
    let mut loss = Decimal::ZERO;
    let mut total = Decimal::ZERO;
    for (k, lot) in lots.iter().enumerate() {
        loss += *lot * ppd * (cur - cum[k]) * pip;
        total += *lot;
    }
    (loss, total)
}

/// `auto_max_orders`：理论亏损**加上**最小缓冲后仍满足条件的
/// 亏损仍落在 `budget` 内。`base_step == 0` 时返回上限；结果 `< 1` 视为无效。
pub fn auto_max_orders(
    initial: Decimal,
    mult: Decimal,
    base_step: Decimal,
    step_add: Decimal,
    pip: Decimal,
    ppd: Decimal,
    budget: Decimal,
    spec: &LotSpec,
) -> usize {
    if base_step <= Decimal::ZERO {
        return MAX_ORDERS_CAP;
    }
    // Fail closed when even the first normalized lot plus its reserve exceeds budget.
    let mut safe = 0usize;
    let mut lots = Vec::with_capacity(MAX_ORDERS_CAP);
    let mut cum = Vec::with_capacity(MAX_ORDERS_CAP);
    let mut acc = Decimal::ZERO;
    for n in 1..=MAX_ORDERS_CAP {
        // 逐层扩展前缀：第 n−1 层的手数与间距
        if n == 1 {
            lots.push(normalize_lots(initial, spec));
        } else {
            let last = lots[n - 2];
            lots.push(calc_next_lot(initial, mult, n - 1, last, spec));
            acc += base_step + step_add * Decimal::from(n - 2);
        }
        cum.push(acc);

        let cur = cum[n - 1];
        let mut loss = Decimal::ZERO;
        let mut total = Decimal::ZERO;
        for (k, lot) in lots.iter().enumerate() {
            loss += *lot * ppd * (cur - cum[k]) * pip;
            total += *lot;
        }
        let min_cushion_points = base_step + Decimal::from(n) * step_add;
        let cushion_loss = total * ppd * min_cushion_points * pip;
        if loss + cushion_loss <= budget + eps_profit_usd() {
            safe = n;
        } else {
            break;
        }
    }
    safe
}

/// `defensive_survival_usd`：装满 `max_orders` 层的篮子，价格还能逆向走多远才
/// the remaining buffer is exhausted, in USD price distance (dashboard "极限抗波幅 $X (N层)").
pub fn defensive_survival_usd(
    initial: Decimal,
    mult: Decimal,
    base_step: Decimal,
    step_add: Decimal,
    pip: Decimal,
    ppd: Decimal,
    max_orders: usize,
    budget: Decimal,
    spec: &LotSpec,
) -> Decimal {
    if max_orders == 0 {
        return Decimal::ZERO;
    }
    let lots = lot_sequence(initial, mult, max_orders, spec);
    let (theo_loss, total_lots) = theoretical_loss(&lots, base_step, step_add, pip, ppd);
    let mut span_points = Decimal::ZERO;
    for s in 1..max_orders {
        span_points += base_step + step_add * Decimal::from(s - 1);
    }
    let rem_buffer = budget - theo_loss;
    let cushion_pts = if total_lots.is_zero() { Decimal::ZERO } else { rem_buffer / (total_lots * ppd * pip) };
    span_points * pip + cushion_pts * pip
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> Decimal {
        s.parse().unwrap()
    }

    fn spec() -> LotSpec {
        LotSpec::gold_default()
    }

    #[test]
    fn gold_default_spec_matches_measured_conditions() {
        let s = spec();
        assert_eq!(s.min, d("0.01"));
        assert_eq!(s.max, d("200"));
        assert_eq!(s.step, d("0.01"));
    }

    #[test]
    fn normalize_snaps_and_clamps() {
        let s = spec();
        assert_eq!(normalize_lots(d("0.0145"), &s), d("0.01"));
        assert_eq!(normalize_lots(d("0.0151"), &s), d("0.02"));
        assert_eq!(normalize_lots(d("0.001"), &s), d("0.01")); // 向上钳制到最小值
        assert_eq!(normalize_lots(d("500"), &s), d("200")); // 向下钳制到最大值
        assert_eq!(normalize_lots(d("1"), &s), d("1.00")); // 按步长小数位取整
    }

    #[test]
    fn lot_sequence_initial_001_mult_145() {
        let lots = lot_sequence(d("0.01"), d("1.45"), 9, &spec());
        let expect: Vec<Decimal> = ["0.01", "0.01", "0.02", "0.03", "0.04", "0.06", "0.09", "0.13", "0.20"]
            .iter()
            .map(|s| d(s))
            .collect();
        assert_eq!(lots, expect);
        // 马丁格尔生效后不再递减
        assert!(lots.windows(2).all(|w| w[1] >= w[0]));
    }

    #[test]
    fn mult_at_or_below_one_is_flat() {
        let lots = lot_sequence(d("0.01"), Decimal::ONE, 4, &spec());
        assert_eq!(lots, vec![d("0.01"); 4]);
        assert_eq!(calc_next_lot(d("0.01"), d("0.5"), 7, d("0.02"), &spec()), d("0.01"));
    }

    #[test]
    fn cumulative_distances_are_triangular() {
        let cum = cumulative_distances(d("300"), d("75"), 9);
        let expect: Vec<Decimal> = ["0", "300", "675", "1125", "1650", "2250", "2925", "3675", "4500"]
            .iter()
            .map(|s| d(s))
            .collect();
        assert_eq!(cum, expect);
        assert_eq!(cumulative_distances(d("300"), d("75"), 0), Vec::<Decimal>::new());
    }

    #[test]
    fn theoretical_loss_matches_hand_calculation() {
        // 上述 L1..L9 手数，base 300，add 75，pip 0.01，ppd 100 → cur = 4500 点 = $4500 亏损
        let lots = lot_sequence(d("0.01"), d("1.45"), 9, &spec());
        let (loss, total) = theoretical_loss(&lots, d("300"), d("75"), d("0.01"), d("100"));
        assert_eq!(total, d("0.59"));
        // loss = Σ lot_k * 100 * (cur_point - cum_k) * 0.01
        //      = [0.01*100*4500 + 0.01*100*4200 + 0.02*100*3825 + ...] * 0.01
        //      = [45+42+76.5+101.25+114+135+141.75+107.25+0] = 762.75
        assert_eq!(loss.round_dp(2), d("762.75"));
        let (loss0, total0) = theoretical_loss(&[], d("300"), d("75"), d("0.01"), d("100"));
        assert_eq!((loss0, total0), (Decimal::ZERO, Decimal::ZERO));
    }

    #[test]
    fn auto_max_orders_budget_600_gives_7() {
        let n = auto_max_orders(d("0.01"), d("1.45"), d("300"), d("75"), d("0.01"), d("100"), d("600"), &spec());
        assert_eq!(n, 7);
        // n=9：theo_loss=762.75 + cushion(0.59*100*975*0.01=5752.5) >> 600，在 n=8 或更早中断
    }

    #[test]
    fn defensive_survival_matches_hand_calculation() {
        let v = defensive_survival_usd(d("0.01"), d("1.45"), d("300"), d("75"), d("0.01"), d("100"), 9, d("10000"), &spec());
        // 修正后 n=9 的 theo_loss=762.75：
        // span_points = s=1..8 的 (300 + 75*(s-1)) 之和 = 300+375+450+525+600+675+750+825 = 4500
        // rem_buffer = 10000 - 762.75 = 9237.25
        // cushion_pts = 9237.25 / (0.59*100*0.01) = 9237.25 / 0.59 = 15654.07...
        // value = 4500*0.01 + 15654.07*0.01 = 45 + 156.54 = 201.54
        assert_eq!(v.round_dp(2), d("201.56")); // 小幅取整
        assert_eq!(defensive_survival_usd(d("0.01"), d("1.45"), d("300"), d("75"), d("0.01"), d("100"), 0, d("10000"), &spec()), Decimal::ZERO);
    }
}
