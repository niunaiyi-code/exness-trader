//! 数据库 schema DDL（docs/03 §5）。

pub const CREATE_USERS_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS users (
    id TEXT PRIMARY KEY,
    username TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    role TEXT NOT NULL CHECK(role IN ('admin', 'trader', 'viewer')),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
)
"#;

pub const CREATE_ACCOUNTS_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS accounts (
    id TEXT PRIMARY KEY,
    account_id TEXT NOT NULL UNIQUE,
    encrypted_api_key TEXT NOT NULL,
    encrypted_private_key TEXT NOT NULL,
    key_format TEXT NOT NULL CHECK(key_format IN ('hex128', 'hex64', 'pem')),
    gateway_url TEXT NOT NULL,
    state TEXT NOT NULL CHECK(state IN ('init', 'warmup', 'running', 'degraded', 'stopping', 'failed')),
    paused BOOLEAN NOT NULL DEFAULT 0,
    strategy_id TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
)
"#;

pub const CREATE_STRATEGIES_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS strategies (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    description TEXT NOT NULL DEFAULT '',
    initial_lot TEXT NOT NULL,
    grid_step INTEGER NOT NULL,
    grid_step_add INTEGER NOT NULL,
    grid_step_sec INTEGER NOT NULL,
    lot_mult TEXT NOT NULL,
    tp_distance_usd TEXT NOT NULL,
    lock_trigger_usd TEXT NOT NULL,
    params_locked BOOLEAN NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
)
"#;

pub const CREATE_ACCOUNT_USERS_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS account_users (
    account_id TEXT NOT NULL,
    user_id TEXT NOT NULL,
    permission TEXT NOT NULL CHECK(permission IN ('view', 'operate', 'admin')),
    PRIMARY KEY (account_id, user_id),
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
)
"#;

pub const CREATE_STRATEGY_PARAMS_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS strategy_params (
    account_id TEXT PRIMARY KEY,
    initial_lot TEXT NOT NULL,
    grid_step INTEGER NOT NULL,
    grid_step_add INTEGER NOT NULL,
    grid_step_sec INTEGER NOT NULL,
    lot_mult TEXT NOT NULL,
    tp_distance_usd TEXT NOT NULL,
    lock_trigger_usd TEXT NOT NULL,
    params_locked BOOLEAN NOT NULL DEFAULT 0,
    updated_at INTEGER NOT NULL,
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE
)
"#;
