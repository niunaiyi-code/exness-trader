//! 手动操作（EA OnChartEvent 菜单操作移植）。

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// 切换暂停（阻止首单与加网格）。
    TogglePause,
    /// 一键锁仓（仅 Arming 时可用，强制 → Locked）。
    LockNow,
    /// 以市价平掉所有网格持仓。
    CloseAll,
    /// 紧急平仓：放大偏差并最多重试 5 次。
    EmergencyClose,
    /// 反向（先全平，再在相反方向开首单）。
    Reverse,
    /// 解锁（释放 Locked → Idle，平掉锁仓持仓）。
    Unlock,
    /// 停止 EA（优雅关闭）。
    Stop,
}

impl Action {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "toggle_pause" => Some(Self::TogglePause),
            "lock_now" => Some(Self::LockNow),
            "close_all" => Some(Self::CloseAll),
            "emergency_close" => Some(Self::EmergencyClose),
            "reverse" => Some(Self::Reverse),
            "unlock" => Some(Self::Unlock),
            "stop" => Some(Self::Stop),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::TogglePause => "toggle_pause",
            Self::LockNow => "lock_now",
            Self::CloseAll => "close_all",
            Self::EmergencyClose => "emergency_close",
            Self::Reverse => "reverse",
            Self::Unlock => "unlock",
            Self::Stop => "stop",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_roundtrip() {
        let actions = vec![
            Action::TogglePause,
            Action::LockNow,
            Action::CloseAll,
            Action::EmergencyClose,
            Action::Reverse,
            Action::Unlock,
            Action::Stop,
        ];

        for action in actions {
            let s = action.as_str();
            let parsed = Action::from_str(s);
            assert_eq!(parsed, Some(action));
        }
    }

    #[test]
    fn unknown_action() {
        assert_eq!(Action::from_str("unknown"), None);
    }
}