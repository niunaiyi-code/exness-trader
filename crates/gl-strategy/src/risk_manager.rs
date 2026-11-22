//! 风险闸门与时段边界逻辑 — 开仓/加仓过滤器（docs/02 §7）。
//!
//! **闸门层级（固定顺序）：**
//! - `can_add_grid`：锁仓 / 暂停 / 长假收盘风险 / 点差
//! - `can_open_first_order`：锁仓 / 暂停 / 时段边界 / 点差 / 日历就绪 / 新闻 / 冷却（2 秒）
//!
//! **交易时段模型（docs/02 §7.3）：**
//! Exness `trade_sessions` 给出每周窗口 → 展开为每日 `[from_sec, to_sec)` 时段。
//! XAUUSD 实测：22 个时段（周日 23:01:30 起，每日维护 21:58–23:00:29，周五 21:58 收盘）。
//! 长假阈值：3 小时（10800 秒）；长假前 4 小时为收盘风险窗口。

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// 传给风险检查的闸门上下文（由调用方组装，避免参数膨胀）。
#[derive(Debug, Clone)]
pub struct GateContext {
    pub is_locked: bool,
    pub is_paused: bool,
    pub cur_spread_usd: Decimal,
    pub now_time: i64,
    pub last_close_time: i64,
    pub last_entry_time: i64,
}

/// 时段片段（某一天内一段连续的交易窗口）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionSegment {
    /// 星期几（0=周日，6=周六）。
    pub dow: u8,
    /// 开始时间（当日 00:00 起的秒数）。
    pub from_sec: i32,
    /// 结束时间（当日 00:00 起的秒数；86400 = 24:00）。
    pub to_sec: i32,
}

/// 风险管理器 — 闸门 + 时段模型（docs/02 §7）。
#[derive(Debug, Clone)]
pub struct RiskManager {
    pub max_spread_usd: Decimal,
    pub sessions: Vec<SessionSegment>,
    /// 财经日历就绪标志（外部提供；由调用方设置）。
    pub calendar_ready: bool,
    /// 财经日历新闻风险缓存结果（外部提供；由调用方设置）。
    pub news_risk_cached: bool,
}

impl RiskManager {
    pub fn new(max_spread_usd: Decimal) -> Self {
        Self {
            max_spread_usd,
            sessions: Vec::new(),
            calendar_ready: false,
            news_risk_cached: false,
        }
    }

    /// 从 Exness `trade_sessions` 加载时段（每周窗口 → 每日时段）。
    /// 示例：`[{week_start_time: "Mon, 00:00", week_end_time: "Fri, 21:57:59"}]`
    /// 若存在间隔（维护窗口），则展开为 5 天 × 每天多个时段。
    /// 真实 XAUUSD 有 22 个时段（docs/02 §7.3）。
    pub fn load_sessions(&mut self, _trade_sessions: &[TradeSession]) {
        // 占位：week_start_time / week_end_time 字符串的解析由调用方实现
        // 以及展开逻辑。目前先用简单的周一至周五 24/5 模型。
        self.sessions.clear();
        for dow in 1..=5 {
            self.sessions.push(SessionSegment { dow, from_sec: 0, to_sec: 86400 });
        }
    }

    /// 检查当前是否处于长假收盘风险窗口（docs/02 §7.3）。
    pub fn is_long_close_risk_time(&self, now: i64) -> bool {
        let (dow, sec) = day_and_sec(now);
        if let Some(idx) = self.find_session_index(dow, sec) {
            let seg = self.sessions[idx];
            let lead_close = seg.to_sec - sec;
            // 检查该时段之后的间隔是否 ≥ 3 小时。
            let gap_after = self.next_session_gap(dow, seg.to_sec);
            if gap_after >= 10800 && lead_close <= 14400 {
                return true;
            }
        }
        false
    }

    /// 检查时段边界风险（docs/02 §7.3）。
    pub fn is_session_boundary_risk(&self, now: i64) -> (bool, &'static str) {
        if self.sessions.is_empty() {
            return (true, "no_trade_sessions");
        }
        let (dow, sec) = day_and_sec(now);
        let idx_opt = self.find_session_index(dow, sec);
        if idx_opt.is_none() {
            return (true, "market_closed");
        }
        let idx = idx_opt.unwrap();
        let seg = self.sessions[idx];
        let lead_close = seg.to_sec - sec;
        let lead_open = sec - seg.from_sec;

        // 远离边界则无风险。
        if lead_close > 14400 && lead_open >= 1800 {
            return (false, "");
        }

        // 长假前收盘（4 小时窗口）。
        if self.is_long_close_risk_time(now) {
            return (true, "long_close_4h");
        }

        // 短休市前收盘（30 分钟）。
        if lead_close <= 1800 {
            let gap = self.next_session_gap(dow, seg.to_sec);
            if gap > 0 && gap < 10800 {
                return (true, "close_30min");
            }
        }

        // 开盘窗口（30 分钟）。
        if lead_open < 1800 {
            let gap_before = self.prev_session_gap(dow, seg.from_sec);
            if gap_before > 0 {
                return (true, "open_30min");
            }
        }

        (false, "")
    }

    /// 查找包含 (dow, sec) 的时段索引。
    fn find_session_index(&self, dow: u8, sec: i32) -> Option<usize> {
        self.sessions.iter().position(|s| s.dow == dow && sec >= s.from_sec && sec < s.to_sec)
    }

    /// 从 seg.to_sec 到下一时段 from_sec 的间隔（秒）。
    fn next_session_gap(&self, dow: u8, abs_to: i32) -> i32 {
        // 占位：向前最多扫描 7 天；若找不到时段则返回 7 天。
        for d in 1..=7 {
            let nd = (dow + d) % 7;
            if let Some(s) = self.sessions.iter().find(|seg| seg.dow == nd && seg.to_sec > seg.from_sec) {
                return d as i32 * 86400 + s.from_sec - abs_to;
            }
        }
        7 * 86400
    }

    /// 从上一时段 to_sec 到 seg.from_sec 的间隔（秒）。
    fn prev_session_gap(&self, dow: u8, abs_from: i32) -> i32 {
        for d in 1..=7 {
            let nd = (dow as i32 - d + 7) % 7;
            if let Some(s) = self.sessions.iter().rfind(|seg| seg.dow == nd as u8) {
                return abs_from - (-d * 86400 + s.to_sec);
            }
        }
        7 * 86400
    }

    /// 闸门：能否加网格？（docs/02 §7.1）。
    pub fn can_add_grid(&self, ctx: &GateContext) -> bool {
        if ctx.is_locked {
            return false;
        }
        if ctx.is_paused {
            return false;
        }
        if self.is_long_close_risk_time(ctx.now_time) {
            return false;
        }
        if self.max_spread_usd > Decimal::ZERO && ctx.cur_spread_usd > self.max_spread_usd {
            return false;
        }
        true
    }

    /// 闸门：能否开首单？（docs/02 §7.2）。
    pub fn can_open_first_order(&self, ctx: &GateContext) -> bool {
        if ctx.is_locked {
            return false;
        }
        if ctx.is_paused {
            return false;
        }
        let (boundary_risk, _reason) = self.is_session_boundary_risk(ctx.now_time);
        if boundary_risk {
            return false;
        }
        if self.max_spread_usd > Decimal::ZERO && ctx.cur_spread_usd > self.max_spread_usd {
            return false;
        }
        if !self.calendar_ready {
            return false;
        }
        if self.news_risk_cached {
            return false;
        }
        // 冷却：距上次平仓或入场 2 秒。
        if ctx.last_close_time > 0 && (ctx.now_time - ctx.last_close_time) < 2 {
            return false;
        }
        if ctx.last_entry_time > 0 && (ctx.now_time - ctx.last_entry_time) < 2 {
            return false;
        }
        true
    }
}

/// Exness `trade_sessions` 条目（简化版；真实结构含 week_start_time/week_end_time 字符串）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeSession {
    pub week_start_time: String,
    pub week_end_time: String,
}

/// 从 Unix 时间戳提取 (dow, sec)（UTC）。
fn day_and_sec(ts: i64) -> (u8, i32) {
    let days_since_epoch = ts / 86400;
    let dow = ((days_since_epoch + 4) % 7) as u8; // 1970-01-01 为周四=4
    let sec = (ts % 86400) as i32;
    (dow, sec)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> Decimal {
        s.parse().unwrap()
    }

    fn gate(locked: bool, paused: bool, spread: &str, now: i64) -> GateContext {
        GateContext {
            is_locked: locked,
            is_paused: paused,
            cur_spread_usd: d(spread),
            now_time: now,
            last_close_time: 0,
            last_entry_time: 0,
        }
    }

    #[test]
    fn can_add_grid_rejects_locked() {
        let rm = RiskManager::new(d("0.5"));
        assert!(!rm.can_add_grid(&gate(true, false, "0.3", 1000)));
    }

    #[test]
    fn can_add_grid_rejects_paused() {
        let rm = RiskManager::new(d("0.5"));
        assert!(!rm.can_add_grid(&gate(false, true, "0.3", 1000)));
    }

    #[test]
    fn can_add_grid_rejects_high_spread() {
        let rm = RiskManager::new(d("0.5"));
        assert!(!rm.can_add_grid(&gate(false, false, "0.6", 1000)));
    }

    #[test]
    fn can_add_grid_passes_normal() {
        let rm = RiskManager::new(d("0.5"));
        assert!(rm.can_add_grid(&gate(false, false, "0.3", 1000)));
    }

    #[test]
    fn can_open_first_order_rejects_calendar_not_ready() {
        let mut rm = RiskManager::new(d("0.5"));
        rm.calendar_ready = false;
        assert!(!rm.can_open_first_order(&gate(false, false, "0.3", 1000)));
    }

    #[test]
    fn can_open_first_order_rejects_news_risk() {
        let mut rm = RiskManager::new(d("0.5"));
        rm.calendar_ready = true;
        rm.news_risk_cached = true;
        assert!(!rm.can_open_first_order(&gate(false, false, "0.3", 1000)));
    }

    #[test]
    fn can_open_first_order_passes_normal() {
        let mut rm = RiskManager::new(d("0.5"));
        rm.calendar_ready = true;
        rm.news_risk_cached = false;
        rm.load_sessions(&[]); // 简单周一至周五
        // 选取 UTC 周一 12:00（远离时段边界）。
        let ts = 4 * 86400 + 12 * 3600; // 1970-01-05 12:00（周一）
        assert!(rm.can_open_first_order(&gate(false, false, "0.3", ts)));
    }
}
