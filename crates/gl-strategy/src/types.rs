//! 核心领域类型 — 枚举与简单结构体（docs/02 §1）。

use serde::{Deserialize, Serialize};

/// 持仓方向（buy = 做多，sell = 做空）。对应 Exness `side` 字段。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Buy,
    Sell,
}

impl Side {
    pub fn opposite(self) -> Self {
        match self {
            Self::Buy => Self::Sell,
            Self::Sell => Self::Buy,
        }
    }
}

/// 账户级对冲锁仓状态机（docs/02 §8）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum LockState {
    /// 网格正常运行，未冻结。
    #[default]
    Idle = 0,
    /// 触发命中，锁仓单尚未成交（5 秒重试）。
    Arming = 1,
    /// 冻结；等待手动清理或 2 小时自动平仓。
    Locked = 2,
}


/// 锁仓状态机 tick 检查结果：继续网格还是冻结？
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockTick {
    /// 未冻结；本 tick 网格继续。
    Run = 0,
    /// 冻结；本 tick 停止网格操作。
    Frozen = 1,
}

/// 由仪表盘或 API 触发的手动操作（docs/02 §9）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManualAction {
    /// 平掉所有持仓（网格 + 锁仓）。
    CloseAll = 2,
    /// 立即触发账户级对冲锁仓。
    LockNow = 8,
    /// 只平亏损持仓。
    CloseLoss = 3,
    /// 只平盈利持仓。
    CloseProfit = 4,
    /// 平掉所有买入持仓。
    CloseBuy = 5,
    /// 平掉所有卖出持仓。
    CloseSell = 6,
    /// 切换/恢复首单开仓。
    TogglePause = 7,
}
