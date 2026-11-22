//! 策略配置参数与运行时常量。

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// 用户可配置的策略参数（由 EA 输入参数移植）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    // 网格初始化
    pub initial_lot: Decimal,          // 0.01
    pub grid_step: i32,                // 300 点（30.0 pip）
    pub grid_step_add: i32,            // 每层 75 点
    pub grid_step_sec: i32,            // 15 秒防抖
    pub lot_mult: Decimal,             // 1.45
    pub tp_distance_usd: Decimal,      // 每轮 1.00 美元
    pub lock_trigger_usd: Decimal,     // 1000 美元浮亏

    // 交易时段与新闻
    pub session_sun_open_sec: i32,     // 1800 秒（延迟 30 分钟）
    pub session_sun_close_sec: i32,    // 1800 秒（提前 30 分钟收盘）
    pub session_fri_close_sec: i32,    // 14400 秒（提前 4 小时收盘）
    pub session_mon_open_sec: i32,     // 10800 秒（晚 3 小时开盘）
    pub news_window_before_sec: i32,   // 60s
    pub news_window_after_sec: i32,    // 180s

    // 风险限额
    pub max_spread_usd: Decimal,       // 0.50 美元（点差美元成本）
    pub max_orders: i32,               // 为 0 时自动计算
}

impl Default for Config {
    fn default() -> Self {
        Self {
            initial_lot: Decimal::from_str_exact("0.01").unwrap(),
            grid_step: 300,
            grid_step_add: 75,
            grid_step_sec: 15,
            lot_mult: Decimal::from_str_exact("1.45").unwrap(),
            tp_distance_usd: Decimal::ONE,
            lock_trigger_usd: Decimal::from(1000),
            session_sun_open_sec: 1800,
            session_sun_close_sec: 1800,
            session_fri_close_sec: 14400,
            session_mon_open_sec: 10800,
            news_window_before_sec: 60,
            news_window_after_sec: 180,
            max_spread_usd: Decimal::from_str_exact("0.50").unwrap(),
            max_orders: 0,
        }
    }
}

/// 初始化时由账户/品种一次性推导的运行时常量。
#[derive(Debug, Clone)]
pub struct RuntimeConstants {
    pub contract_size: Decimal,              // 例如 100 盎司
    pub point: Decimal,                      // 2 位小数为 0.01，3 位小数为 0.001
    pub pip: Decimal,                        // point × 10
    pub profit_per_dollar_per_lot: Decimal,  // contract_size ×（USC 为 100，否则为 1）
    pub digits: u32,                         // 2 或 3
    pub deviation: i32,                      // 20 点
    pub emergency_deviation: i32,            // 300 点
    pub parabola_k: Decimal,                 // 9.0
    pub min_tp_profit: Decimal,              // 1.00 美元
    pub trailing_cushion_points: Decimal,    // 35.0 点
    pub trailing_step_points: Decimal,       // 25.0 点
    pub auto_close_spread_usd: Decimal,      // 0.80 美元
    pub reentry_cooldown_ms: u64,            // 1000ms
    pub close_retry_ms: u64,                 // 200ms
    pub max_close_retry: u32,                // 5
    pub lock_retry_sec: u32,                 // 5s
    pub escalated_deviation: i32,            // 50 点
}

impl RuntimeConstants {
    pub fn new(
        contract_size: Decimal,
        point: Decimal,
        digits: u32,
        currency: &str,
    ) -> Self {
        let pip = point * Decimal::from(10);
        let multiplier = if currency == "USC" { Decimal::from(100) } else { Decimal::ONE };
        let profit_per_dollar_per_lot = contract_size * multiplier;

        Self {
            contract_size,
            point,
            pip,
            profit_per_dollar_per_lot,
            digits,
            deviation: 20,
            emergency_deviation: 300,
            parabola_k: Decimal::from(9),
            min_tp_profit: Decimal::ONE,
            trailing_cushion_points: Decimal::from(35),
            trailing_step_points: Decimal::from(25),
            auto_close_spread_usd: Decimal::from_str_exact("0.80").unwrap(),
            reentry_cooldown_ms: 1000,
            close_retry_ms: 200,
            max_close_retry: 5,
            lock_retry_sec: 5,
            escalated_deviation: 50,
        }
    }

    pub fn format_price(&self, price: Decimal) -> String {
        format!("{:.prec$}", price, prec = self.digits as usize)
    }
}

/// 策略常量（固定不变的策略参数）
#[derive(Debug, Clone)]
pub struct StrategyConstants {
    pub deviation: i32,
    pub trailing_step_points: Decimal,
}

impl Default for StrategyConstants {
    fn default() -> Self {
        Self {
            deviation: 20,
            trailing_step_points: Decimal::from(25),
        }
    }
}

impl RuntimeConstants {
    /// 黄金/美元标准参数
    pub fn gold_usd_standard() -> Self {
        Self::new(
            Decimal::from(100),    // contract_size: 100 盎司
            Decimal::from_str_exact("0.01").unwrap(),  // point: 0.01
            2,                     // digits: 2
            "USD",                 // currency
        )
    }
}
