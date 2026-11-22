//! GoldLegend 策略内核：纯函数、无 IO，可同时供实盘运行器与回放工具复用。

pub mod actions;
pub mod app;
pub mod basket;
pub mod config;
pub mod gates;
pub mod grid;
pub mod grid_math;
pub mod lock_fsm;
pub mod lock_manager;
pub mod risk_manager;
pub mod tp_engine;
pub mod types;

pub use actions::Action;
pub use app::{GoldLegendApp, TickDecision};
pub use basket::{Basket, BasketStats, MarketData, Position, PositionView, TpAction, AddGridDecision};
pub use config::{Config, RuntimeConstants, StrategyConstants};
pub use gates::{GateResult, Gates};
pub use grid::GridMath;
pub use lock_fsm::{LockFsm, LockState};
pub use lock_manager::{AccountSnapshot, LockDecision, LockManager};
pub use risk_manager::RiskManager;
pub use tp_engine::{TpDecision, TpEngine};
pub use types::{Side, LockTick, ManualAction};
