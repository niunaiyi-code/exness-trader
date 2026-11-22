//! Exness Trader API SDK（不含业务语义）。
//!
//! 模块：
//! * [`auth`]   – Ed25519 请求签名（`EXN-*` 请求头）、私钥解析、幂等键。
//! * [`error`]  – 错误类型 + 按 Exness 错误码区间划分的重试分类。
//! * [`models`] – 本项目所用端点的 serde 模型。
//! * [`rest`]   – 带接入点发现的签名 REST 客户端。

pub mod auth;
pub mod error;
pub mod models;
pub mod rest;
pub mod ws;

pub use auth::{Credentials, SignRequest, SignedHeaders};
pub use error::{Error as ApiError, ErrorClass, Result};
pub use models::{
    AccessPointResponse, AccountSnapshot, AckResponse, Candle, CandleQuery, ClosePositionParams,
    Deal, DealHistoryItem, DealsHistoryResponse, InstrumentCondition, InstrumentEvent,
    InstrumentTradeSession, LimitsResponse,
    OpenPositionRequest, OperationResult, Order, Position, ProfitAmount,
    Tick, TradingStateSnapshot, TransactionEvent, TransactionPayload, WsEvent,
};
pub use rest::{Client, Client as RestClient, ClientConfig, WsStream};
pub use ws::{EventStreamClient, TickStreamClient};

// 重新导出常用枚举，方便使用
pub use models::PositionSide;
