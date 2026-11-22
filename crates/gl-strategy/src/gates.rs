//! 订单闸门（EA 闸门检查移植）。

use chrono::{DateTime, Datelike, Timelike, Utc};
use rust_decimal::Decimal;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GateResult {
    Pass,
    Block(&'static str),
}

pub struct Gates;

impl Gates {
    /// 检查点差美元成本是否超限。
    pub fn check_spread(
        bid: Decimal,
        ask: Decimal,
        total_lots: Decimal,
        profit_per_dollar_per_lot: Decimal,
        max_spread_usd: Decimal,
    ) -> GateResult {
        let spread_cost = (ask - bid) * total_lots * profit_per_dollar_per_lot;
        if spread_cost > max_spread_usd {
            GateResult::Block("spread exceeds limit")
        } else {
            GateResult::Pass
        }
    }

    /// 检查是否在每周交易时段内（方案 A：已展开为每日）
    pub fn check_session(
        now: DateTime<Utc>,
        sun_open_sec: i32,
        sun_close_sec: i32,
        fri_close_sec: i32,
        mon_open_sec: i32,
    ) -> GateResult {
        let weekday = now.weekday().number_from_sunday(); // 1=周日..7=周六
        let day_sec = now.num_seconds_from_midnight() as i32;

        match weekday {
            1 => {
                // 周日：等待 sun_open_sec
                if day_sec < sun_open_sec {
                    return GateResult::Block("Sunday: before open delay");
                }
                // 周日收盘：86400 - sun_close_sec
                if day_sec > (86400 - sun_close_sec) {
                    return GateResult::Block("Sunday: after close early");
                }
            }
            2 => {
                // 周一：等待 mon_open_sec
                if day_sec < mon_open_sec {
                    return GateResult::Block("Monday: before open delay");
                }
            }
            6 => {
                // 周五：提前 fri_close_sec 收盘
                if day_sec > (86400 - fri_close_sec) {
                    return GateResult::Block("Friday: before close early");
                }
            }
            _ => {} // 周二/周三/周四：无限制
        }
        GateResult::Pass
    }

    /// 检查是否处于新闻窗口（按方案 A 只拦首单，不拦加网格）。
    pub fn check_news_first_order(
        now: DateTime<Utc>,
        next_event_time: Option<DateTime<Utc>>,
        window_before_sec: i32,
        window_after_sec: i32,
    ) -> GateResult {
        if let Some(event_time) = next_event_time {
            let diff_sec = (event_time - now).num_seconds();
            if diff_sec >= -window_after_sec as i64 && diff_sec <= window_before_sec as i64 {
                return GateResult::Block("inside news avoidance window");
            }
        }
        GateResult::Pass
    }

    /// 检查距上次首单是否已过 2 秒冷却。
    pub fn check_cooldown_first_order(
        now: DateTime<Utc>,
        last_first_order_time: Option<DateTime<Utc>>,
        cooldown_sec: i32,
    ) -> GateResult {
        if let Some(last_time) = last_first_order_time {
            let elapsed = (now - last_time).num_seconds();
            if elapsed < cooldown_sec as i64 {
                return GateResult::Block("first-order cooldown not met");
            }
        }
        GateResult::Pass
    }

    /// 检查距上次加网格是否已过 15 秒防抖。
    pub fn check_debounce_grid_add(
        now: DateTime<Utc>,
        last_grid_add_time: Option<DateTime<Utc>>,
        debounce_sec: i32,
    ) -> GateResult {
        if let Some(last_time) = last_grid_add_time {
            let elapsed = (now - last_time).num_seconds();
            if elapsed < debounce_sec as i64 {
                return GateResult::Block("grid add debounce not met");
            }
        }
        GateResult::Pass
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use rust_decimal_macros::dec;

    #[test]
    fn spread_pass() {
        let result = Gates::check_spread(dec!(2000), dec!(2000.05), dec!(0.01), dec!(10000), dec!(10));
        // spread_cost = 0.05 × 0.01 × 10000 = 5 < 10
        assert_eq!(result, GateResult::Pass);
    }

    #[test]
    fn spread_block() {
        let result = Gates::check_spread(dec!(2000), dec!(2001), dec!(0.05), dec!(10000), dec!(10));
        // spread_cost = 1 × 0.05 × 10000 = 500 > 10
        assert_eq!(result, GateResult::Block("spread exceeds limit"));
    }

    #[test]
    fn session_sunday_before_open() {
        let sunday = Utc.with_ymd_and_hms(2026, 3, 22, 0, 10, 0).unwrap(); // 周日 00:10
        let result = Gates::check_session(sunday, 1800, 1800, 14400, 10800);
        assert_eq!(result, GateResult::Block("Sunday: before open delay"));
    }

    #[test]
    fn session_friday_close_early() {
        let friday = Utc.with_ymd_and_hms(2026, 3, 20, 22, 0, 0).unwrap(); // 周五 22:00
        let result = Gates::check_session(friday, 1800, 1800, 14400, 10800);
        // 22:00 = 79200 秒，提前收盘于 86400 - 14400 = 72000 秒
        assert_eq!(result, GateResult::Block("Friday: before close early"));
    }

    #[test]
    fn news_window_block() {
        let now = Utc.with_ymd_and_hms(2026, 3, 16, 12, 59, 30).unwrap();
        let event = Utc.with_ymd_and_hms(2026, 3, 16, 13, 0, 0).unwrap();
        let result = Gates::check_news_first_order(now, Some(event), 60, 180);
        // diff = 30 秒 < 60 秒 → 处于窗口内
        assert_eq!(result, GateResult::Block("inside news avoidance window"));
    }

    #[test]
    fn cooldown_not_met() {
        let now = Utc.with_ymd_and_hms(2026, 3, 16, 12, 0, 1).unwrap();
        let last = Utc.with_ymd_and_hms(2026, 3, 16, 12, 0, 0).unwrap();
        let result = Gates::check_cooldown_first_order(now, Some(last), 2);
        // elapsed = 1 秒 < 2 秒
        assert_eq!(result, GateResult::Block("first-order cooldown not met"));
    }
}