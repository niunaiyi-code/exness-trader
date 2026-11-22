//! 账户配置（策略参数 + 运行时常量）。

use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use gl_strategy::grid_math::LotSpec;
use serde::{Deserialize, Serialize};

/// 策略参数（用户可配置）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategyParams {
    pub initial_lot: Decimal,
    pub max_orders: usize,
    pub grid_step: u32,
    pub grid_step_add: u32,
    pub grid_step_sec: u32,
    pub lot_mult: Decimal,
    pub tp_distance_usd: Decimal,
    pub lock_trigger_usd: Decimal,
    /// 是否启用双向独立对冲（买卖两侧解耦，任一篮子为空就开首单）
    pub enable_two_way_hedging: bool,
}

impl Default for StrategyParams {
    fn default() -> Self {
        Self {
            initial_lot: dec!(0.01),
            max_orders: 8,
            grid_step: 300,
            grid_step_add: 75,
            grid_step_sec: 15,
            lot_mult: dec!(1.45),
            tp_distance_usd: dec!(1.00),
            lock_trigger_usd: dec!(1000),
            enable_two_way_hedging: false,
        }
    }
}

/// 运行时常量（由品种 contract_size 与账户币种推导）。
#[derive(Debug, Clone)]
pub struct RuntimeConstants {
    pub lot_spec: LotSpec,
    pub contract_size: Decimal,
    pub profit_per_dollar_per_lot: Decimal,
    pub pip: Decimal,
    pub deviation: u32,
    pub emergency_deviation: u32,
    pub max_spread_usd: Decimal,
    pub news_avoid_before_sec: u32,
    pub news_avoid_after_sec: u32,
    pub parabola_k: Decimal,
    pub min_tp: Decimal,
    pub trailing_cushion_points: Decimal,
    pub trailing_step_points: Decimal,
}

impl RuntimeConstants {
    pub fn from_instrument(contract_size: Decimal, account_currency: &str, point_digits: u32, lot_spec: LotSpec) -> Self {
        let currency_factor = if account_currency == "USC" { Decimal::from(100) } else { Decimal::ONE };
        let profit_per_dollar_per_lot = contract_size * currency_factor;
        let point = Decimal::new(1, point_digits);
        let pip = if matches!(point_digits, 3 | 5) { point * Decimal::from(10) } else { point };

        Self {
            lot_spec,
            contract_size,
            profit_per_dollar_per_lot,
            pip,
            deviation: 20,
            emergency_deviation: 300,
            max_spread_usd: dec!(0.50),
            news_avoid_before_sec: 60 * 60,
            news_avoid_after_sec: 3 * 60 * 60,
            parabola_k: dec!(9.0),
            min_tp: dec!(1.00),
            trailing_cushion_points: dec!(35.0),
            trailing_step_points: dec!(25.0),
        }
    }
}

/// 完整账户配置（参数 + 常量 + 品种代码）。
#[derive(Debug, Clone)]
pub struct AccountConfig {
    pub account_id: String,
    pub symbol: String,
    pub params: StrategyParams,
    pub constants: RuntimeConstants,
}
