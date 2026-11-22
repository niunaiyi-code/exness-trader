//! GoldLegend 运行时：AccountRunner 基于 Exness API 编排策略执行。

pub mod account_runner;
pub mod basket;
pub mod config;
pub mod error;
pub mod events;
pub mod executor;
pub mod ledger;
pub mod market;
pub mod ops_tracker;
pub mod pipeline;
pub mod reconcile;
pub mod strategy;
pub mod trade_executor;

pub mod metrics;

#[cfg(test)]
mod account_runner_tests;

pub use account_runner::AccountRunner;
pub use basket::BasketStats;
pub use config::AccountConfig;
pub use error::{RuntimeError, RuntimeResult};
pub use events::TradeNotification;
pub use ledger::{Ledger, PositionRole};
pub use ops_tracker::OpsTracker;
pub use pipeline::StrategyPipeline;
pub use strategy::{can_open_first_order, GateContext, GateDecision};
pub use trade_executor::TradeExecutor;
