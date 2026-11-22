//! 账户运行时 — 启动每账户运行器并管理生命周期（docs/03 §3）。

use gl_runtime::account_runner::RunnerState;
use std::collections::HashMap;
use std::sync::{atomic::{AtomicBool, Ordering}, Arc};
use tokio::task::JoinHandle;

/// 每账户运行时处理器。
pub struct AccountRuntime {
    pub account_id: String,
    pub runner_state: RunnerState,
    task_handle: Option<JoinHandle<()>>,
    paused: Arc<AtomicBool>,
    shutdown: Option<tokio::sync::mpsc::Sender<()>>,
}

impl AccountRuntime {
    pub fn new(account_id: String) -> Self {
        Self {
            account_id,
            runner_state: RunnerState::Init,
            task_handle: None,
            paused: Arc::new(AtomicBool::new(false)),
            shutdown: None,
        }
    }

    /// 启动账户运行器任务。
    pub fn spawn(&mut self) {
        self.runner_state = RunnerState::Running;
        let _account_id = self.account_id.clone();
        let handle = tokio::spawn(async move {
            // tick 循环（简化版；生产环境由行情数据流驱动）
            loop {
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                // TODO：通过 AccountRunner 调用 on_tick(bid, ask)
            }
        });
        self.task_handle = Some(handle);
    }

    /// 挂载真实运行器任务句柄（替换占位 `spawn`）。
    pub fn attach(&mut self, handle: JoinHandle<()>) {
        self.runner_state = RunnerState::Running;
        self.task_handle = Some(handle);
    }

    pub fn attach_with_shutdown(
        &mut self,
        handle: JoinHandle<()>,
        shutdown: tokio::sync::mpsc::Sender<()>,
    ) {
        self.attach(handle);
        self.shutdown = Some(shutdown);
    }

    /// 中止账户运行器任务。
    pub fn stop(&mut self) {
        self.runner_state = RunnerState::Stopping;
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.try_send(());
        }
        if let Some(mut handle) = self.task_handle.take() {
            tokio::spawn(async move {
                if tokio::time::timeout(tokio::time::Duration::from_secs(5), &mut handle)
                    .await
                    .is_err()
                {
                    handle.abort();
                }
            });
        }
    }

    /// 检查任务是否仍在运行。
    pub fn is_running(&self) -> bool {
        self.task_handle.as_ref().map(|h| !h.is_finished()).unwrap_or(false)
    }

    pub fn set_paused(&self, paused: bool) {
        self.paused.store(paused, Ordering::Release);
    }

    pub fn paused_handle(&self) -> Arc<AtomicBool> {
        self.paused.clone()
    }
}

/// 所有活跃账户运行时的管理器。
pub struct RuntimeManager {
    runtimes: HashMap<String, AccountRuntime>,
}

impl RuntimeManager {
    pub fn new() -> Self {
        Self {
            runtimes: HashMap::new(),
        }
    }

    pub fn start(&mut self, account_id: &str) -> bool {
        if let Some(rt) = self.runtimes.get_mut(account_id) {
            if !rt.is_running() {
                rt.spawn();
                return true;
            }
            false
        } else {
            let mut rt = AccountRuntime::new(account_id.to_string());
            rt.spawn();
            self.runtimes.insert(account_id.to_string(), rt);
            true
        }
    }

    pub fn attach(&mut self, account_id: &str, handle: JoinHandle<()>) {
        if let Some(rt) = self.runtimes.get_mut(account_id) {
            rt.attach(handle);
        } else {
            let mut rt = AccountRuntime::new(account_id.to_string());
            rt.attach(handle);
            self.runtimes.insert(account_id.to_string(), rt);
        }
    }

    pub fn attach_with_shutdown(
        &mut self,
        account_id: &str,
        handle: JoinHandle<()>,
        shutdown: tokio::sync::mpsc::Sender<()>,
    ) {
        let runtime = self
            .runtimes
            .entry(account_id.to_string())
            .or_insert_with(|| AccountRuntime::new(account_id.to_string()));
        runtime.attach_with_shutdown(handle, shutdown);
    }

    pub fn paused_handle(&mut self, account_id: &str) -> Arc<AtomicBool> {
        self.runtimes
            .entry(account_id.to_string())
            .or_insert_with(|| AccountRuntime::new(account_id.to_string()))
            .paused_handle()
    }

    pub fn set_paused(&mut self, account_id: &str, paused: bool) -> bool {
        let Some(runtime) = self.runtimes.get(account_id) else { return false };
        runtime.set_paused(paused);
        true
    }

    pub fn stop(&mut self, account_id: &str) -> bool {
        if let Some(rt) = self.runtimes.get_mut(account_id) {
            if rt.is_running() {
                rt.stop();
                return true;
            }
        }
        false
    }

    pub fn is_running(&self, account_id: &str) -> bool {
        self.runtimes.get(account_id).map(|r| r.is_running()).unwrap_or(false)
    }

    pub fn running_accounts(&self) -> Vec<String> {
        self.runtimes
            .iter()
            .filter(|(_, rt)| rt.is_running())
            .map(|(id, _)| id.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn account_runtime_spawn_stop() {
        let mut rt = AccountRuntime::new("acc_123".to_string());
        rt.spawn();
        assert!(rt.is_running());

        rt.stop();
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        assert!(!rt.is_running());
    }
}
