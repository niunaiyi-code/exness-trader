//! 数据库仓储层 — CRUD 操作（docs/06）。

use crate::db::{
    models::{Account, AccountUser, Strategy, StrategyParams, User},
    DbPool,
};
use sqlx::Result;

/// 用户仓储。
pub struct UserRepository;

impl UserRepository {
    pub async fn create(
        pool: &DbPool,
        id: &str,
        username: &str,
        password_hash: &str,
        role: &str,
    ) -> Result<()> {
        let now = chrono::Utc::now().timestamp();
        sqlx::query(
            "INSERT INTO users (id, username, password_hash, role, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(id)
        .bind(username)
        .bind(password_hash)
        .bind(role)
        .bind(now)
        .bind(now)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn find_by_username(pool: &DbPool, username: &str) -> Result<Option<User>> {
        sqlx::query_as::<_, User>("SELECT * FROM users WHERE username = ?")
            .bind(username)
            .fetch_optional(pool)
            .await
    }

    pub async fn find_by_id(pool: &DbPool, id: &str) -> Result<Option<User>> {
        sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = ?")
            .bind(id)
            .fetch_optional(pool)
            .await
    }
}

/// 账户仓储。
pub struct AccountRepository;

impl AccountRepository {
    pub async fn create(
        pool: &DbPool,
        id: &str,
        account_id: &str,
        encrypted_api_key: &str,
        encrypted_private_key: &str,
        key_format: &str,
        gateway_url: &str,
    ) -> Result<()> {
        let now = chrono::Utc::now().timestamp();
        sqlx::query(
            "INSERT INTO accounts (id, account_id, encrypted_api_key, encrypted_private_key,
             key_format, gateway_url, state, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, 'init', ?, ?)",
        )
        .bind(id)
        .bind(account_id)
        .bind(encrypted_api_key)
        .bind(encrypted_private_key)
        .bind(key_format)
        .bind(gateway_url)
        .bind(now)
        .bind(now)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn find_by_id(pool: &DbPool, id: &str) -> Result<Option<Account>> {
        sqlx::query_as::<_, Account>("SELECT * FROM accounts WHERE id = ?")
            .bind(id)
            .fetch_optional(pool)
            .await
    }

    pub async fn find_by_account_id(pool: &DbPool, account_id: &str) -> Result<Option<Account>> {
        sqlx::query_as::<_, Account>("SELECT * FROM accounts WHERE account_id = ?")
            .bind(account_id)
            .fetch_optional(pool)
            .await
    }

    pub async fn list_all(pool: &DbPool) -> Result<Vec<Account>> {
        sqlx::query_as::<_, Account>("SELECT * FROM accounts ORDER BY created_at DESC")
            .fetch_all(pool)
            .await
    }

    pub async fn update_state(pool: &DbPool, id: &str, state: &str) -> Result<()> {
        let now = chrono::Utc::now().timestamp();
        sqlx::query("UPDATE accounts SET state = ?, updated_at = ? WHERE id = ?")
            .bind(state)
            .bind(now)
            .bind(id)
            .execute(pool)
            .await?;
        Ok(())
    }

    pub async fn update_paused(pool: &DbPool, id: &str, paused: bool) -> Result<()> {
        sqlx::query("UPDATE accounts SET paused = ?, updated_at = ? WHERE id = ?")
            .bind(paused)
            .bind(chrono::Utc::now().timestamp())
            .bind(id)
            .execute(pool)
            .await?;
        Ok(())
    }

    pub async fn bind_strategy(pool: &DbPool, id: &str, strategy_id: Option<&str>) -> Result<()> {
        let now = chrono::Utc::now().timestamp();
        sqlx::query("UPDATE accounts SET strategy_id = ?, updated_at = ? WHERE id = ? OR account_id = ?")
            .bind(strategy_id)
            .bind(now)
            .bind(id)
            .bind(id)
            .execute(pool)
            .await?;
        Ok(())
    }

    pub async fn delete(pool: &DbPool, id: &str) -> Result<()> {
        sqlx::query("DELETE FROM accounts WHERE id = ?")
            .bind(id)
            .execute(pool)
            .await?;
        Ok(())
    }
}

/// AccountUser 仓储（权限）。
pub struct AccountUserRepository;

impl AccountUserRepository {
    pub async fn grant(
        pool: &DbPool,
        account_id: &str,
        user_id: &str,
        permission: &str,
    ) -> Result<()> {
        sqlx::query(
            "INSERT OR REPLACE INTO account_users (account_id, user_id, permission)
             VALUES (?, ?, ?)",
        )
        .bind(account_id)
        .bind(user_id)
        .bind(permission)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn revoke(pool: &DbPool, account_id: &str, user_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM account_users WHERE account_id = ? AND user_id = ?")
            .bind(account_id)
            .bind(user_id)
            .execute(pool)
            .await?;
        Ok(())
    }

    pub async fn find_permission(
        pool: &DbPool,
        account_id: &str,
        user_id: &str,
    ) -> Result<Option<AccountUser>> {
        sqlx::query_as::<_, AccountUser>(
            "SELECT * FROM account_users WHERE account_id = ? AND user_id = ?",
        )
        .bind(account_id)
        .bind(user_id)
        .fetch_optional(pool)
        .await
    }

    pub async fn list_by_user(pool: &DbPool, user_id: &str) -> Result<Vec<AccountUser>> {
        sqlx::query_as::<_, AccountUser>("SELECT * FROM account_users WHERE user_id = ?")
            .bind(user_id)
            .fetch_all(pool)
            .await
    }
}

/// StrategyParams 仓储。
pub struct StrategyParamsRepository;

impl StrategyParamsRepository {
    pub async fn upsert(pool: &DbPool, params: &StrategyParams) -> Result<()> {
        let now = chrono::Utc::now().timestamp();
        sqlx::query(
            "INSERT OR REPLACE INTO strategy_params
             (account_id, initial_lot, grid_step, grid_step_add, grid_step_sec,
              lot_mult, tp_distance_usd, lock_trigger_usd, params_locked, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&params.account_id)
        .bind(&params.initial_lot)
        .bind(params.grid_step)
        .bind(params.grid_step_add)
        .bind(params.grid_step_sec)
        .bind(&params.lot_mult)
        .bind(&params.tp_distance_usd)
        .bind(&params.lock_trigger_usd)
        .bind(params.params_locked)
        .bind(now)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn find_by_account(
        pool: &DbPool,
        account_id: &str,
    ) -> Result<Option<StrategyParams>> {
        sqlx::query_as::<_, StrategyParams>(
            "SELECT account_id, initial_lot, grid_step, grid_step_add, grid_step_sec,
             lot_mult, tp_distance_usd, lock_trigger_usd, params_locked, updated_at
             FROM strategy_params WHERE account_id = ?",
        )
        .bind(account_id)
        .fetch_optional(pool)
        .await
    }
}

/// 策略仓储。
pub struct StrategyRepository;

impl StrategyRepository {
    pub async fn create(pool: &DbPool, s: &Strategy) -> Result<()> {
        sqlx::query(
            "INSERT INTO strategies (id, name, description, initial_lot, grid_step,
             grid_step_add, grid_step_sec, lot_mult, tp_distance_usd, lock_trigger_usd,
             params_locked, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&s.id)
        .bind(&s.name)
        .bind(&s.description)
        .bind(&s.initial_lot)
        .bind(s.grid_step)
        .bind(s.grid_step_add)
        .bind(s.grid_step_sec)
        .bind(&s.lot_mult)
        .bind(&s.tp_distance_usd)
        .bind(&s.lock_trigger_usd)
        .bind(s.params_locked)
        .bind(s.created_at)
        .bind(s.updated_at)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn update(pool: &DbPool, s: &Strategy) -> Result<()> {
        let now = chrono::Utc::now().timestamp();
        sqlx::query(
            "UPDATE strategies SET name = ?, description = ?, initial_lot = ?,
             grid_step = ?, grid_step_add = ?, grid_step_sec = ?, lot_mult = ?,
             tp_distance_usd = ?, lock_trigger_usd = ?, params_locked = ?, updated_at = ?
             WHERE id = ?",
        )
        .bind(&s.name)
        .bind(&s.description)
        .bind(&s.initial_lot)
        .bind(s.grid_step)
        .bind(s.grid_step_add)
        .bind(s.grid_step_sec)
        .bind(&s.lot_mult)
        .bind(&s.tp_distance_usd)
        .bind(&s.lock_trigger_usd)
        .bind(s.params_locked)
        .bind(now)
        .bind(&s.id)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn delete(pool: &DbPool, id: &str) -> Result<()> {
        let _ = sqlx::query("UPDATE accounts SET strategy_id = NULL WHERE strategy_id = ?")
            .bind(id)
            .execute(pool)
            .await;
        sqlx::query("DELETE FROM strategies WHERE id = ?")
            .bind(id)
            .execute(pool)
            .await?;
        Ok(())
    }

    pub async fn find_by_id(pool: &DbPool, id: &str) -> Result<Option<Strategy>> {
        sqlx::query_as::<_, Strategy>(
            "SELECT id, name, description, initial_lot, grid_step, grid_step_add,
             grid_step_sec, lot_mult, tp_distance_usd, lock_trigger_usd, params_locked,
             created_at, updated_at FROM strategies WHERE id = ?",
        )
            .bind(id)
            .fetch_optional(pool)
            .await
    }

    pub async fn list_all(pool: &DbPool) -> Result<Vec<Strategy>> {
        sqlx::query_as::<_, Strategy>(
            "SELECT id, name, description, initial_lot, grid_step, grid_step_add,
             grid_step_sec, lot_mult, tp_distance_usd, lock_trigger_usd, params_locked,
             created_at, updated_at FROM strategies ORDER BY created_at ASC",
        )
            .fetch_all(pool)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    async fn test_pool() -> DbPool {
        db::init_pool(":memory:", 5).await.unwrap()
    }

    #[tokio::test]
    async fn user_crud() {
        let pool = test_pool().await;
        UserRepository::create(&pool, "u1", "alice", "hash123", "admin")
            .await
            .unwrap();

        let user = UserRepository::find_by_username(&pool, "alice")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(user.username, "alice");
        assert_eq!(user.role, "admin");
    }

    #[tokio::test]
    async fn account_crud() {
        let pool = test_pool().await;
        AccountRepository::create(
            &pool,
            "acc1",
            "18000069657",
            "enc_key",
            "enc_priv",
            "hex128",
            "https://api.exness.com",
        )
        .await
        .unwrap();

        let account = AccountRepository::find_by_account_id(&pool, "18000069657")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(account.account_id, "18000069657");
        assert_eq!(account.state, "init");

        AccountRepository::update_state(&pool, "acc1", "running")
            .await
            .unwrap();
        let updated = AccountRepository::find_by_id(&pool, "acc1")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.state, "running");
    }

    #[tokio::test]
    async fn permissions() {
        let pool = test_pool().await;

        // 先创建用户与账户（外键约束）
        UserRepository::create(&pool, "u1", "alice", "hash", "admin")
            .await
            .unwrap();
        AccountRepository::create(
            &pool,
            "acc1",
            "18000069657",
            "enc_key",
            "enc_priv",
            "hex128",
            "https://api.exness.com",
        )
        .await
        .unwrap();

        AccountUserRepository::grant(&pool, "acc1", "u1", "admin")
            .await
            .unwrap();

        let perm = AccountUserRepository::find_permission(&pool, "acc1", "u1")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(perm.permission, "admin");

        AccountUserRepository::revoke(&pool, "acc1", "u1")
            .await
            .unwrap();
        let gone = AccountUserRepository::find_permission(&pool, "acc1", "u1")
            .await
            .unwrap();
        assert!(gone.is_none());
    }

    #[tokio::test]
    async fn strategy_crud_and_binding() {
        let pool = test_pool().await;
        let strat = Strategy {
            id: "s1".to_string(),
            name: "Test Strategy".to_string(),
            description: "A test".to_string(),
            initial_lot: "0.02".to_string(),
            grid_step: 300,
            grid_step_add: 50,
            grid_step_sec: 15,
            lot_mult: "1.5".to_string(),
            tp_distance_usd: "2.0".to_string(),
            lock_trigger_usd: "1200.0".to_string(),
            params_locked: false,
            created_at: 100,
            updated_at: 100,
        };

        StrategyRepository::create(&pool, &strat).await.unwrap();
        let fetched = StrategyRepository::find_by_id(&pool, "s1").await.unwrap().unwrap();
        assert_eq!(fetched.name, "Test Strategy");

        AccountRepository::create(
            &pool,
            "acc1",
            "18000069657",
            "enc_key",
            "enc_priv",
            "hex128",
            "https://api.exness.com",
        )
        .await
        .unwrap();

        AccountRepository::bind_strategy(&pool, "acc1", Some("s1")).await.unwrap();
        let acc = AccountRepository::find_by_id(&pool, "acc1").await.unwrap().unwrap();
        assert_eq!(acc.strategy_id, Some("s1".to_string()));

        let all = StrategyRepository::list_all(&pool).await.unwrap();
        assert_eq!(all.len(), 1);

        StrategyRepository::delete(&pool, "s1").await.unwrap();
        let acc_after = AccountRepository::find_by_id(&pool, "acc1").await.unwrap().unwrap();
        assert_eq!(acc_after.strategy_id, None);
    }
}
