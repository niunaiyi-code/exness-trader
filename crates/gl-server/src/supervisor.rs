//! 账户监管器 — 生命周期管理与重启策略（docs/03 §3.4）。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;
use tokio::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SupervisorState {
    Running,
    Backoff,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
pub struct AccountSupervisor {
    pub account_id: String,
    pub state: SupervisorState,
    pub failure_count: u32,
    #[serde(skip)]
    pub last_failure: Option<Instant>,
    #[serde(skip)]
    pub backoff_until: Option<Instant>,
}

impl AccountSupervisor {
    pub fn new(account_id: String) -> Self {
        Self {
            account_id,
            state: SupervisorState::Running,
            failure_count: 0,
            last_failure: None,
            backoff_until: None,
        }
    }

    /// 记录一次失败并计算退避。
    pub fn record_failure(&mut self) {
        self.failure_count += 1;
        self.last_failure = Some(Instant::now());

        if self.failure_count >= 10 {
            self.state = SupervisorState::Failed;
        } else {
            self.state = SupervisorState::Backoff;
            let backoff_secs = (1u64 << (self.failure_count.min(6) - 1)).min(60);
            self.backoff_until = Some(Instant::now() + Duration::from_secs(backoff_secs));
        }
    }

    /// 检查是否可重启。
    pub fn can_restart(&self) -> bool {
        match self.state {
            SupervisorState::Running => false,
            SupervisorState::Failed => false,
            SupervisorState::Backoff => self
                .backoff_until
                .map(|t| Instant::now() >= t)
                .unwrap_or(true),
        }
    }

    /// 重启成功后清零失败计数。
    pub fn reset(&mut self) {
        self.state = SupervisorState::Running;
        self.failure_count = 0;
        self.last_failure = None;
        self.backoff_until = None;
    }
}

pub struct SupervisorRegistry {
    supervisors: HashMap<String, AccountSupervisor>,
}

impl SupervisorRegistry {
    pub fn new() -> Self {
        Self {
            supervisors: HashMap::new(),
        }
    }

    pub fn register(&mut self, account_id: String) {
        self.supervisors
            .insert(account_id.clone(), AccountSupervisor::new(account_id));
    }

    pub fn get_mut(&mut self, account_id: &str) -> Option<&mut AccountSupervisor> {
        self.supervisors.get_mut(account_id)
    }

    pub fn get(&self, account_id: &str) -> Option<&AccountSupervisor> {
        self.supervisors.get(account_id)
    }

    pub fn list(&self) -> Vec<AccountSupervisor> {
        self.supervisors.values().cloned().collect()
    }

    pub fn remove(&mut self, account_id: &str) {
        self.supervisors.remove(account_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supervisor_backoff() {
        let mut sup = AccountSupervisor::new("acc_123".to_string());
        assert_eq!(sup.state, SupervisorState::Running);

        sup.record_failure();
        assert_eq!(sup.state, SupervisorState::Backoff);
        assert_eq!(sup.failure_count, 1);

        for _ in 0..9 {
            sup.record_failure();
        }
        assert_eq!(sup.state, SupervisorState::Failed);
    }

    #[test]
    fn supervisor_reset() {
        let mut sup = AccountSupervisor::new("acc_123".to_string());
        sup.record_failure();
        sup.reset();
        assert_eq!(sup.state, SupervisorState::Running);
        assert_eq!(sup.failure_count, 0);
    }
}
