// 首单入场闸门检查
// 执行时段边界、新闻规避、点差过滤与冷却检查

use chrono::{DateTime, Datelike, Timelike, Utc};
use exness_api::{InstrumentCondition, InstrumentTradeSession};
use rust_decimal::Decimal;

/// 时段边界时间常量（秒）
const SESSION_OPEN_BLOCK_SEC: i32 = 1800; // 开盘后 30 分钟
const SESSION_CLOSE_BLOCK_SEC: i32 = 1800; // 收盘前 30 分钟
const LONG_CLOSE_BLOCK_SEC: i32 = 14400; // 长假前 4 小时

/// 最小入场冷却（秒）
const MIN_ENTRY_COOLDOWN_SEC: i64 = 300; // 5 分钟

/// 首单检查所用闸门上下文
#[derive(Debug, Clone)]
pub struct GateContext {
    pub now: DateTime<Utc>,
    pub is_locked: bool,
    pub is_paused: bool,
    pub cur_spread_usd: Decimal,
    pub last_close_time: Option<DateTime<Utc>>,
    pub last_entry_time: Option<DateTime<Utc>>,
    pub next_news_event_time: Option<DateTime<Utc>>,
    pub news_calendar_ready: bool,
    pub instrument_condition: Option<InstrumentCondition>,
}

/// 闸门检查结果
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GateDecision {
    Allow,
    Block { reason: String },
}

impl GateDecision {
    pub fn is_blocked(&self) -> bool {
        matches!(self, GateDecision::Block { .. })
    }
}

/// 检查是否可开首单
pub fn can_open_first_order(
    ctx: &GateContext,
    max_spread_usd: Decimal,
    news_pre_min: i32,
    news_post_min: i32,
) -> GateDecision {
    // 1. 锁仓状态（最高优先级）
    if ctx.is_locked {
        return GateDecision::Block {
            reason: "账户冻结期间".to_string(),
        };
    }

    // 2. 手动暂停
    if ctx.is_paused {
        return GateDecision::Block {
            reason: "手动暂停开单".to_string(),
        };
    }

    if !ctx.news_calendar_ready {
        return GateDecision::Block { reason: "财经日历尚未成功加载".to_string() };
    }

    // 3. 交易时段边界风险
    if let Some(reason) = check_session_boundary_risk(&ctx.now, ctx.instrument_condition.as_ref()) {
        return GateDecision::Block { reason };
    }

    // 4. 点差过滤
    if max_spread_usd > Decimal::ZERO && ctx.cur_spread_usd > max_spread_usd {
        return GateDecision::Block {
            reason: format!(
                "点差超标 ${:.2} > ${:.2}",
                ctx.cur_spread_usd, max_spread_usd
            ),
        };
    }

    // 5. 新闻日历检查（方案 A：只拦首单，不拦加网格）
    if let Some(event_time) = ctx.next_news_event_time {
        let diff_sec = (event_time - ctx.now).num_seconds();
        let pre_sec = (news_pre_min as i64) * 60;
        let post_sec = (news_post_min as i64) * 60;
        if diff_sec >= -post_sec && diff_sec <= pre_sec {
            return GateDecision::Block {
                reason: format!(
                    "新闻避险窗口 (距离事件 {}s, 规避范围 -{}s ~ +{}s)",
                    diff_sec, post_sec, pre_sec
                ),
            };
        }
    }

    // 6. 入场冷却
    if let Some(last_close) = ctx.last_close_time {
        let elapsed = (ctx.now - last_close).num_seconds();
        if elapsed < MIN_ENTRY_COOLDOWN_SEC {
            return GateDecision::Block {
                reason: format!("平仓后冷却 ({}s < {}s)", elapsed, MIN_ENTRY_COOLDOWN_SEC),
            };
        }
    }
    if let Some(last_entry) = ctx.last_entry_time {
        let elapsed = (ctx.now - last_entry).num_seconds();
        if elapsed < MIN_ENTRY_COOLDOWN_SEC {
            return GateDecision::Block {
                reason: format!("开仓后冷却 ({}s < {}s)", elapsed, MIN_ENTRY_COOLDOWN_SEC),
            };
        }
    }

    GateDecision::Allow
}

/// Evaluate the account-scoped Exness weekly trade-session schedule (UTC).
fn check_session_boundary_risk(
    now: &DateTime<Utc>,
    condition: Option<&InstrumentCondition>,
) -> Option<String> {
    let Some(condition) = condition else {
        return Some("品种交易时段尚未加载".to_string());
    };
    if condition.trade_mode.as_deref() != Some("enabled") {
        return Some("品种当前禁止交易".to_string());
    }

    let Some(weekly) = parse_enabled_sessions(&condition.trade_sessions) else {
        return Some("品种交易时段数据无效".to_string());
    };
    if weekly.is_empty() {
        return Some("品种没有可交易时段".to_string());
    }

    const WEEK: i64 = 7 * 24 * 60 * 60;
    let now_second = i64::from(now.weekday().num_days_from_monday()) * 86_400
        + i64::from(now.hour()) * 3_600
        + i64::from(now.minute()) * 60
        + i64::from(now.second());
    let intervals: Vec<(i64, i64)> = weekly
        .iter()
        .flat_map(|&(start, end)| [-WEEK, 0, WEEK, 2 * WEEK].map(move |offset| (start + offset, end + offset)))
        .collect();

    let current = intervals.iter().copied().find(|(start, end)| *start <= now_second && now_second < *end);
    let Some((start, end)) = current else {
        return Some("当前处于休市时段".to_string());
    };

    let previous_end = intervals.iter().map(|(_, end)| *end).filter(|candidate| *candidate <= start).max();
    let next_start = intervals.iter().map(|(start, _)| *start).filter(|candidate| *candidate >= end).min();

    if let Some(previous_end) = previous_end {
        let gap_before = start - previous_end;
        if gap_before > 0 && now_second - start < i64::from(SESSION_OPEN_BLOCK_SEC) {
            return Some("开市后 30 分钟".to_string());
        }
    }

    let close_lead = end - now_second;
    if let Some(next_start) = next_start {
        let gap_after = next_start - end;
        if gap_after >= 3 * 60 * 60 && close_lead <= i64::from(LONG_CLOSE_BLOCK_SEC) {
            return Some("长时间休市前 4 小时".to_string());
        }
        if gap_after > 0 && gap_after < 3 * 60 * 60 && close_lead <= i64::from(SESSION_CLOSE_BLOCK_SEC) {
            return Some("休市前 30 分钟".to_string());
        }
    }

    None
}

fn parse_enabled_sessions(sessions: &[InstrumentTradeSession]) -> Option<Vec<(i64, i64)>> {
    const WEEK: i64 = 7 * 24 * 60 * 60;
    let mut windows = Vec::new();
    for session in sessions {
        let start = parse_week_second(session.week_start_time.as_deref()?)?;
        let end_inclusive = parse_week_second(session.week_end_time.as_deref()?)?;
        let mut end = end_inclusive + 1;
        if end <= start {
            end += WEEK;
        }
        let enabled = session.trade_mode.as_deref() != Some("trading_disabled");
        for offset in [-WEEK, 0, WEEK] {
            let clipped_start = (start + offset).max(0);
            let clipped_end = (end + offset).min(WEEK);
            if clipped_start < clipped_end {
                windows.push((clipped_start, clipped_end, enabled));
            }
        }
    }

    let mut boundaries = vec![0, WEEK];
    for (start, end, _) in &windows {
        boundaries.push(*start);
        boundaries.push(*end);
    }
    boundaries.sort_unstable();
    boundaries.dedup();

    let mut enabled_segments: Vec<(i64, i64)> = Vec::new();
    for pair in boundaries.windows(2) {
        let (start, end) = (pair[0], pair[1]);
        let covered: Vec<bool> = windows.iter()
            .filter(|(window_start, window_end, _)| *window_start <= start && *window_end >= end)
            .map(|(_, _, enabled)| *enabled)
            .collect();
        let is_enabled = covered.iter().any(|enabled| *enabled)
            && !covered.iter().any(|enabled| !*enabled);
        if is_enabled {
            if let Some(last) = enabled_segments.last_mut().filter(|last| last.1 == start) {
                last.1 = end;
            } else {
                enabled_segments.push((start, end));
            }
        }
    }
    Some(enabled_segments)
}

fn parse_week_second(value: &str) -> Option<i64> {
    let (day, time) = value.split_once(',')?;
    let day = match day.trim().to_ascii_lowercase().as_str() {
        "mon" => 0,
        "tue" => 1,
        "wed" => 2,
        "thu" => 3,
        "fri" => 4,
        "sat" => 5,
        "sun" => 6,
        _ => return None,
    };
    let mut parts = time.trim().split(':');
    let hour: i64 = parts.next()?.parse().ok()?;
    let minute: i64 = parts.next()?.parse().ok()?;
    let second: i64 = parts.next()?.parse().ok()?;
    if parts.next().is_some() || hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    Some(day * 86_400 + hour * 3_600 + minute * 60 + second)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn xau_schedule() -> InstrumentCondition {
        InstrumentCondition {
            symbol: "XAUUSD".to_string(),
            base_currency: None,
            quote_currency: Some("USD".to_string()),
            point_digits: None,
            volume_min: None,
            volume_max: None,
            volume_step: None,
            contract_size: None,
            trade_mode: Some("enabled".to_string()),
            trade_sessions: vec![InstrumentTradeSession {
                trade_mode: Some("enabled".to_string()),
                week_start_time: Some("Sun, 23:01:30".to_string()),
                week_end_time: Some("Fri, 21:57:59".to_string()),
            }],
        }
    }

    fn make_time(dow: u32, hour: u32, min: u32) -> DateTime<Utc> {
        // 2024-01-07 为周日
        let base = chrono::NaiveDate::from_ymd_opt(2024, 1, 7).unwrap();
        let day_offset = chrono::Duration::days(dow as i64);
        let target_day = base + day_offset;
        let time = chrono::NaiveTime::from_hms_opt(hour, min, 0).unwrap();
        DateTime::from_naive_utc_and_offset(
            chrono::NaiveDateTime::new(target_day, time),
            Utc,
        )
    }

    #[test]
    fn test_weekend_blocked() {
        // 周六 10:00
        let sat = make_time(6, 10, 0);
        assert!(check_session_boundary_risk(&sat, Some(&xau_schedule())).is_some());

        // 周日 20:00（API schedule 的周日开盘前）
        let sun_early = make_time(0, 20, 0);
        assert!(check_session_boundary_risk(&sun_early, Some(&xau_schedule())).is_some());
    }

    #[test]
    fn test_rollover_blocked() {
        // 周一 21:30（滚动时段）
        let rollover = make_time(5, 18, 0);
        assert!(check_session_boundary_risk(&rollover, Some(&xau_schedule())).is_some());
    }

    #[test]
    fn test_open_block() {
        // 周日 22:10（开盘后 10 分钟，处于 30 分钟拦截窗口内）
        let after_open = make_time(0, 23, 10);
        assert!(check_session_boundary_risk(&after_open, Some(&xau_schedule())).is_some());

        // 周日开市 40 分钟后，开市缓冲期结束。
        let clear = make_time(0, 23, 42);
        assert!(check_session_boundary_risk(&clear, Some(&xau_schedule())).is_none());
    }

    #[test]
    fn test_close_block() {
        // 周一 20:45（距滚动 15 分钟，处于 30 分钟拦截窗口内）
        let before_close = make_time(5, 18, 0);
        assert!(check_session_boundary_risk(&before_close, Some(&xau_schedule())).is_some());
    }

    #[test]
    fn test_friday_long_close() {
        // 周五 18:00（距 API schedule 周末收盘不足 4 小时）
        let fri_evening = make_time(5, 18, 0);
        assert!(check_session_boundary_risk(&fri_evening, Some(&xau_schedule())).is_some());
    }

    #[test]
    fn test_normal_trading_hours() {
        // 周二 15:00（时段中部）
        let normal = make_time(2, 15, 0);
        assert!(check_session_boundary_risk(&normal, Some(&xau_schedule())).is_none());
    }

    #[test]
    fn test_cooldown_blocks() {
        let now = Utc::now();
        let recent_close = now - chrono::Duration::seconds(200); // 3 分 20 秒前

        let ctx = GateContext {
            now,
            is_locked: false,
            is_paused: false,
            cur_spread_usd: Decimal::new(5, 1), // $0.5
            last_close_time: Some(recent_close),
            last_entry_time: None,
            next_news_event_time: None,
            news_calendar_ready: true,
            instrument_condition: Some(xau_schedule()),
        };

        let decision = can_open_first_order(
            &ctx,
            Decimal::new(10, 1), // 最大点差 $1.0
            30,  // news_pre_min
            30,  // news_post_min
        );

        assert!(decision.is_blocked());
    }

    #[test]
    fn test_spread_filter() {
        let ctx = GateContext {
            now: Utc::now(),
            is_locked: false,
            is_paused: false,
            cur_spread_usd: Decimal::new(15, 1), // $1.5
            last_close_time: None,
            last_entry_time: None,
            next_news_event_time: None,
            news_calendar_ready: true,
            instrument_condition: Some(xau_schedule()),
        };

        let decision = can_open_first_order(
            &ctx,
            Decimal::new(10, 1), // 最大点差 $1.0
            30,
            30,
        );

        assert!(decision.is_blocked());
    }

    #[test]
    fn test_news_avoidance_window_blocks_first_order() {
        let now = make_time(2, 15, 0); // 周二正常交易时间
        let upcoming_event = now + chrono::Duration::minutes(10); // 10分钟后有大非农/CPI，处于30分钟前置窗口内

        let ctx = GateContext {
            now,
            is_locked: false,
            is_paused: false,
            cur_spread_usd: Decimal::new(2, 1), // $0.20 正常点差
            last_close_time: None,
            last_entry_time: None,
            next_news_event_time: Some(upcoming_event),
            news_calendar_ready: true,
            instrument_condition: Some(xau_schedule()),
        };

        let decision = can_open_first_order(
            &ctx,
            Decimal::new(10, 1), // 最大点差 $1.0
            30, // news_pre_min
            15, // news_post_min
        );

        assert!(decision.is_blocked());
        if let GateDecision::Block { reason } = decision {
            assert!(reason.contains("新闻避险窗口"));
        } else {
            panic!("Expected block");
        }
    }
}
