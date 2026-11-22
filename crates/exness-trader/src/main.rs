//! exness-trader：多账户交易系统
//!
//! 架构：
//! - 每个账户 = 一个独立 Tokio Task（轻量协程）
//! - 共享 HTTP 客户端连接池（TCP 复用）
//! - 共享 Tokio 运行时（8-16 个 OS 线程）
//! - 账户间状态隔离（无需加锁）

mod account_task;
mod config;

use anyhow::Result;
use tracing::{info, warn};
use config::Config;
use account_task::AccountTask;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"))
        )
        .init();

    let config = Config::from_env()?;
    info!("Loaded {} accounts", config.accounts.len());

    // 每个账户启动一个独立 Task
    let mut tasks = Vec::new();
    for account_cfg in config.accounts {
        let task = AccountTask::spawn(account_cfg);
        tasks.push(task);
    }

    info!("All account tasks spawned, waiting for completion...");

    // 等待所有任务（除非出错或关闭，否则会一直运行）
    for (i, task) in tasks.into_iter().enumerate() {
        match task.await {
            Ok(Ok(())) => info!("Account task {} completed", i),
            Ok(Err(e)) => warn!("Account task {} failed: {:?}", i, e),
            Err(e) => warn!("Account task {} panicked: {:?}", i, e),
        }
    }

    Ok(())
}
