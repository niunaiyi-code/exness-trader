//! 数据库模型（docs/03 §5）。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct User {
    pub id: String,
    pub username: String,
    pub password_hash: String,
    pub role: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Account {
    pub id: String,
    pub account_id: String,
    pub encrypted_api_key: String,
    pub encrypted_private_key: String,
    pub key_format: String,
    pub gateway_url: String,
    pub state: String,
    pub paused: bool,
    pub strategy_id: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Strategy {
    pub id: String,
    pub name: String,
    pub description: String,
    pub initial_lot: String,
    pub grid_step: i32,
    pub grid_step_add: i32,
    pub grid_step_sec: i32,
    pub lot_mult: String,
    pub tp_distance_usd: String,
    pub lock_trigger_usd: String,
    pub params_locked: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct AccountUser {
    pub account_id: String,
    pub user_id: String,
    pub permission: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct StrategyParams {
    pub account_id: String,
    pub initial_lot: String,
    pub grid_step: i32,
    pub grid_step_add: i32,
    pub grid_step_sec: i32,
    pub lot_mult: String,
    pub tp_distance_usd: String,
    pub lock_trigger_usd: String,
    pub params_locked: bool,
    pub updated_at: i64,
}
