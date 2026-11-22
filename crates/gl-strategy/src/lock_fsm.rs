//! 锁仓状态机（EA LockManager.mqh FSM 移植）。

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LockState {
    Idle,
    Arming { trigger_loss: Decimal },
    Locked,
}

impl Default for LockState {
    fn default() -> Self {
        Self::Idle
    }
}

pub struct LockFsm;

impl LockFsm {
    /// 状态转换逻辑：Idle → Arming（存在网格时）→ Locked（触发命中时）。
    pub fn tick(
        state: LockState,
        current_loss: Decimal,
        trigger_loss: Decimal,
        basket_count: usize,
    ) -> LockState {
        match state {
            LockState::Idle => {
                if basket_count > 0 {
                    LockState::Arming { trigger_loss }
                } else {
                    LockState::Idle
                }
            }
            LockState::Arming { trigger_loss: armed_trigger } => {
                if basket_count == 0 {
                    // 网格已平，重置为 Idle
                    LockState::Idle
                } else if current_loss <= armed_trigger {  // current_loss 为负，trigger 也为负
                    // 触发命中（亏损达到阈值）
                    LockState::Locked
                } else {
                    // 保持 Arming
                    LockState::Arming { trigger_loss: armed_trigger }
                }
            }
            LockState::Locked => {
                if basket_count == 0 {
                    // 锁仓单被手动或外部平掉
                    LockState::Idle
                } else {
                    LockState::Locked
                }
            }
        }
    }

    /// 检查是否允许手动锁仓（仅 Arming 状态）。
    pub fn can_manual_lock(state: LockState) -> bool {
        matches!(state, LockState::Arming { .. })
    }

    /// 立即进入 Locked 状态（手动操作）。
    pub fn force_lock(state: LockState) -> LockState {
        if Self::can_manual_lock(state) {
            LockState::Locked
        } else {
            state
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn idle_to_arming_when_basket_exists() {
        let next = LockFsm::tick(LockState::Idle, dec!(-500), dec!(-1000), 1);
        assert_eq!(next, LockState::Arming { trigger_loss: dec!(-1000) });
    }

    #[test]
    fn arming_to_locked_when_trigger_hit() {
        let next = LockFsm::tick(
            LockState::Arming { trigger_loss: dec!(-1000) },
            dec!(-1001),
            dec!(-1000),
            1,
        );
        assert_eq!(next, LockState::Locked);
    }

    #[test]
    fn arming_back_to_idle_when_basket_closed() {
        let next = LockFsm::tick(
            LockState::Arming { trigger_loss: dec!(-1000) },
            dec!(-500),
            dec!(-1000),
            0,
        );
        assert_eq!(next, LockState::Idle);
    }

    #[test]
    fn manual_lock_only_in_arming() {
        assert!(LockFsm::can_manual_lock(LockState::Arming { trigger_loss: dec!(-1000) }));
        assert!(!LockFsm::can_manual_lock(LockState::Idle));
        assert!(!LockFsm::can_manual_lock(LockState::Locked));
    }

    #[test]
    fn force_lock_advances_state() {
        let locked = LockFsm::force_lock(LockState::Arming { trigger_loss: dec!(-1000) });
        assert_eq!(locked, LockState::Locked);

        let idle = LockFsm::force_lock(LockState::Idle);
        assert_eq!(idle, LockState::Idle);
    }
}