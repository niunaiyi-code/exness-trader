//! 账户级对冲锁仓状态机 — 针对行情逆向的净中性保护
//! （docs/02 §8，`LockManager.mqh`）。
//!
//! **状态机状态：**
//! - `Idle`：网格正常运行，未冻结。
//! - `Arming`: Trigger hit (浮亏 ≤ −lock_trigger_usd), lock order not yet filled (5s retry).
//! - `Locked`：冻结；等待手动清理或 2 小时自动平仓；不再开新网格单。
//!
//! **设计：**
//! - 触发条件：单一 Magic，账户级净盈亏（该 magic 下所有持仓之和）。
//! - 锁仓单：1 张覆盖 net_lots 的反向单，使净敞口归零。
//! - 升级策略：首次偏差尝试 → 5 秒重试 → 放大偏差（50 点 = $0.50）。
//! - 自动平仓：锁仓后 2 小时，按 magic 全部平掉（网格单与锁仓单同时）。
//! - 重置：0 持仓持续 30 秒。
//! - 持久化：状态 + 时间戳存入 GV（EA 重启后仍保留）。

use crate::types::{LockState, LockTick};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// 用于锁仓决策的账户快照（来自 `PositionSnapshot`）。
#[derive(Debug, Clone, Copy)]
pub struct AccountSnapshot {
    pub net_lots: Decimal,
    pub floating_pnl: Decimal,
    pub position_count: usize,
}

/// 锁状态持久化层（GV 模拟：键值存储，例如 Redis / SQLite）。
pub trait LockStateStore {
    fn get_state(&self, magic: u64) -> Option<(LockState, i64)>;
    fn set_state(&mut self, magic: u64, state: LockState, locked_since: i64);
    fn clear_state(&mut self, magic: u64);
}

/// 锁管理器状态机（docs/02 §8）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockManager {
    pub magic: u64,
    pub state: LockState,
    /// 锁仓开始时间（Unix 秒；0 = 未锁仓）。
    pub locked_since: i64,
    /// 空仓确认开始时间（0 = 未处于空仓窗口）。
    pub flat_since: i64,
    /// 上次尝试下锁仓单的时间。
    pub last_lock_attempt: i64,
    /// 上次陈旧告警时间。
    pub last_stale_log: i64,
    /// 上次尝试自动平仓的时间。
    pub last_auto_close_log: i64,
    /// 上次推送通知时间。
    pub last_push_time: i64,
    /// 上次推送标签（用于按事件节流）。
    pub last_push_tag: String,
}

impl Default for LockManager {
    fn default() -> Self {
        Self {
            magic: 0,
            state: LockState::Idle,
            locked_since: 0,
            flat_since: 0,
            last_lock_attempt: 0,
            last_stale_log: 0,
            last_auto_close_log: 0,
            last_push_time: 0,
            last_push_tag: String::new(),
        }
    }
}

impl LockManager {
    pub fn new(magic: u64) -> Self {
        Self { magic, ..Default::default() }
    }

    pub fn is_locked(&self) -> bool {
        self.state != LockState::Idle
    }

    /// 从存储初始化（EA 重启后加载此前状态）。
    pub fn init_from_store(&mut self, store: &impl LockStateStore) {
        if let Some((state, locked_since)) = store.get_state(self.magic) {
            self.state = state;
            self.locked_since = locked_since;
        }
    }

    /// 推进状态机（docs/02 §8.1）。
    /// 返回：网格可继续时为 `Run`；否则为 `Frozen`。
    pub fn advance_lock_state(
        &mut self,
        now: i64,
        snap: &AccountSnapshot,
        lock_trigger_usd: Decimal,
    ) -> LockTick {
        if self.state != LockState::Idle {
            // ARMING 或 LOCKED 状态下：检查持仓是否已清空。
            if snap.position_count == 0 {
                if self.flat_since == 0 {
                    self.flat_since = now;
                    eprintln!(
                        "[FLAT PENDING] Positions cleared; confirmation window {}s",
                        FLAT_CONFIRM_SEC
                    );
                }
                // 是否满足确认窗口？
                if now - self.flat_since >= FLAT_CONFIRM_SEC as i64 {
                    eprintln!("[LOCK RESET] Exiting lock round after {}s flat.", FLAT_CONFIRM_SEC);
                    self.reset();
                    return LockTick::Run;
                }
                return LockTick::Frozen;
            }
            // 持仓重新出现。
            self.flat_since = 0;
            return LockTick::Frozen;
        }

        // IDLE：检查触发条件。
        if snap.floating_pnl > -lock_trigger_usd {
            return LockTick::Run;
        }

        // 触发命中：进入 ARMING。
        self.state = LockState::Arming;
        self.locked_since = now;
        self.flat_since = 0;
        eprintln!(
            "[LOCK ROUND START] Floating P&L ${:.2} <= trigger -${:.2}; entering lock",
            snap.floating_pnl, lock_trigger_usd
        );
        LockTick::Frozen
    }

    /// 核心决策循环（docs/02 §8，CheckAndExecuteLock）。
    pub fn check_and_execute(
        &mut self,
        now: i64,
        snap: &AccountSnapshot,
        lock_trigger_usd: Decimal,
    ) -> LockDecision {
        if lock_trigger_usd <= Decimal::ZERO {
            return LockDecision::Run;
        }

        // 步骤 1：推进状态机。
        if self.advance_lock_state(now, snap, lock_trigger_usd) == LockTick::Run {
            return LockDecision::Run;
        }

        // 步骤 1b：是否处于空仓确认窗口？
        if self.flat_since > 0 {
            return LockDecision::Frozen;
        }

        // 步骤 1c：2 小时后自动平仓？
        if self.should_auto_close(now) {
            return LockDecision::AutoClose;
        }

        // 步骤 2：陈旧告警。
        self.warn_if_stale(now, snap);

        // 步骤 3：是否已锁仓？
        if self.state == LockState::Locked {
            return LockDecision::Frozen;
        }

        // 步骤 4：ARMING 重试节流。
        if self.last_lock_attempt > 0 && (now - self.last_lock_attempt) < LOCK_RETRY_SEC as i64 {
            return LockDecision::Frozen;
        }

        // 步骤 5：净敞口是否已中性？
        let lock_lot = snap.net_lots.abs();
        if lock_lot < Decimal::from_str_exact("0.01").unwrap_or(Decimal::ZERO) {
            self.state = LockState::Locked;
            eprintln!("[LOCK FROZEN] Net lot {:.2} already neutral", lock_lot);
            return LockDecision::Frozen;
        }

        // 步骤 6：发送锁仓单。
        LockDecision::PlaceLock { lock_lot, is_net_long: snap.net_lots > Decimal::ZERO }
    }

    /// 检查是否适用 2 小时自动平仓窗口。
    fn should_auto_close(&self, now: i64) -> bool {
        self.state != LockState::Idle && (now - self.locked_since) >= AUTO_CLOSE_AFTER_SEC as i64
    }

    /// 记录陈旧告警（1 小时阈值）。
    fn warn_if_stale(&mut self, now: i64, snap: &AccountSnapshot) {
        if self.state == LockState::Idle {
            return;
        }
        let stale_sec = 3600;
        if self.last_stale_log == 0 || (now - self.last_stale_log) >= stale_sec {
            let elapsed = now - self.locked_since;
            if elapsed >= stale_sec {
                eprintln!(
                    "[LOCK STALE] Frozen {} sec (1h+); pnl ${:.2}; manual intervention advised.",
                    elapsed, snap.floating_pnl
                );
            }
            self.last_stale_log = now;
        }
    }

    /// 记录锁仓尝试（用于重试节流）。
    pub fn record_lock_attempt(&mut self, now: i64) {
        self.last_lock_attempt = now;
        self.state = LockState::Locked;
    }

    /// 重置为 IDLE（确认 + 自动平仓之后）。
    pub fn reset(&mut self) {
        self.state = LockState::Idle;
        self.locked_since = 0;
        self.flat_since = 0;
        self.last_lock_attempt = 0;
        self.last_stale_log = 0;
        self.last_auto_close_log = 0;
        self.last_push_time = 0;
        self.last_push_tag.clear();
    }
}

/// 锁仓状态机决策结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LockDecision {
    /// 网格正常运行。
    Run,
    /// 冻结所有操作（等待锁仓单、处于空仓窗口等）。
    Frozen,
    /// 执行自动平仓（2 小时超时）。
    AutoClose,
    /// 发送锁仓单。
    PlaceLock { lock_lot: Decimal, is_net_long: bool },
}

/// 常量（docs/02 §8）。
pub const FLAT_CONFIRM_SEC: usize = 30;
pub const LOCK_RETRY_SEC: usize = 5;
pub const AUTO_CLOSE_AFTER_SEC: usize = 7200; // 2 小时
pub const LOCK_LOG_THROTTLE_SEC: usize = 300;

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> Decimal {
        s.parse().unwrap()
    }

    fn snap(net_lots: &str, pnl: &str, count: usize) -> AccountSnapshot {
        AccountSnapshot {
            net_lots: d(net_lots),
            floating_pnl: d(pnl),
            position_count: count,
        }
    }

    #[test]
    fn idle_no_trigger() {
        let mut mgr = LockManager::new(123);
        let dec = mgr.check_and_execute(1000, &snap("0.1", "500", 2), d("1000"));
        assert_eq!(dec, LockDecision::Run);
        assert_eq!(mgr.state, LockState::Idle);
    }

    #[test]
    fn trigger_enters_arming() {
        let mut mgr = LockManager::new(123);
        let decision = mgr.check_and_execute(1000, &snap("0.2", "-1500", 3), d("1000"));
        assert_eq!(decision, LockDecision::PlaceLock { lock_lot: d("0.2"), is_net_long: true });
        // 订单成功后调用 record_lock_attempt 之前，状态仍为 ARMING。
        assert_eq!(mgr.state, LockState::Arming);
        // 模拟下单成功。
        mgr.record_lock_attempt(1000);
        assert_eq!(mgr.state, LockState::Locked);
    }

    #[test]
    fn positions_clear_starts_flat_window() {
        let mut mgr = LockManager::new(123);
        mgr.state = LockState::Arming;
        mgr.locked_since = 1000;
        let decision = mgr.check_and_execute(1005, &snap("0", "0", 0), d("1000"));
        assert_eq!(decision, LockDecision::Frozen);
        assert!(mgr.flat_since > 0);
    }

    #[test]
    fn flat_window_resets_after_30s() {
        let mut mgr = LockManager::new(123);
        mgr.state = LockState::Arming;
        mgr.locked_since = 1000;
        mgr.flat_since = 1000;
        let decision = mgr.check_and_execute(1000 + FLAT_CONFIRM_SEC as i64, &snap("0", "0", 0), d("1000"));
        assert_eq!(decision, LockDecision::Run);
        assert_eq!(mgr.state, LockState::Idle);
    }

    #[test]
    fn auto_close_triggers_after_2h() {
        let mut mgr = LockManager::new(123);
        mgr.state = LockState::Locked;
        mgr.locked_since = 1000;
        let decision = mgr.check_and_execute(
            1000 + AUTO_CLOSE_AFTER_SEC as i64,
            &snap("0.3", "-1500", 5),
            d("1000"),
        );
        assert_eq!(decision, LockDecision::AutoClose);
    }
}
