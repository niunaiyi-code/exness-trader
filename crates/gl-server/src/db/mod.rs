//! 基于 sqlx 的数据库层（docs/03 §5）。

pub mod models;
pub mod schema;
pub mod repository;

use sqlx::{
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
    Error as SqlxError,
};
use std::str::FromStr;

pub type DbPool = sqlx::SqlitePool;

/// 初始化数据库连接池并执行迁移。
pub async fn init_pool(database_url: &str, max_connections: u32) -> Result<DbPool, SqlxError> {
    let options = SqliteConnectOptions::from_str(database_url)?
        .create_if_missing(true);

    let pool = SqlitePoolOptions::new()
        .max_connections(max_connections.max(1))
        .connect_with(options)
        .await?;

    // 执行迁移（TODO：生产环境使用 sqlx migrate）。
    sqlx::query(schema::CREATE_USERS_TABLE)
        .execute(&pool)
        .await?;
    sqlx::query(schema::CREATE_ACCOUNTS_TABLE)
        .execute(&pool)
        .await?;
    // 确保已有表中存在 strategy_id 列
    let _ = sqlx::query("ALTER TABLE accounts ADD COLUMN strategy_id TEXT")
        .execute(&pool)
        .await;
    let _ = sqlx::query("ALTER TABLE accounts ADD COLUMN paused BOOLEAN NOT NULL DEFAULT 0")
        .execute(&pool)
        .await;
    sqlx::query(schema::CREATE_ACCOUNT_USERS_TABLE)
        .execute(&pool)
        .await?;
    sqlx::query(schema::CREATE_STRATEGY_PARAMS_TABLE)
        .execute(&pool)
        .await?;
    sqlx::query(schema::CREATE_STRATEGIES_TABLE)
        .execute(&pool)
        .await?;
    // Upgrade existing installations without requiring a database reset.
    // Remove the manually configured max_orders introduced by older builds;
    // it is now derived per account from its risk budget and instrument spec.
    let _ = sqlx::query("ALTER TABLE strategies DROP COLUMN max_orders")
        .execute(&pool)
        .await;
    let _ = sqlx::query("ALTER TABLE strategy_params DROP COLUMN max_orders")
        .execute(&pool)
        .await;

    // 删除旧数据库中不再使用的入场模式列。忽略“列不存在”等兼容性错误。
    let _ = sqlx::query("ALTER TABLE strategies DROP COLUMN entry_mode")
        .execute(&pool)
        .await;
    let _ = sqlx::query("ALTER TABLE strategy_params DROP COLUMN entry_mode")
        .execute(&pool)
        .await;

    Ok(pool)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn init_pool_creates_tables() {
        let pool = init_pool(":memory:", 5).await.unwrap();
        let result: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(result.0, 0);
    }
}
