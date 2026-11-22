//! 运行时错误类型。

use thiserror::Error;

#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("Exness API error: {0}")]
    ExnessApi(#[from] exness_api::ApiError),

    #[error("Ledger error: {0}")]
    Ledger(String),

    #[error("Reconciliation failed: {0}")]
    Reconcile(String),

    #[error("Execution error: {0}")]
    Execution(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("WebSocket error: {0}")]
    WebSocket(String),

    #[error("State error: {0}")]
    State(String),
}

pub type RuntimeResult<T> = Result<T, RuntimeError>;
