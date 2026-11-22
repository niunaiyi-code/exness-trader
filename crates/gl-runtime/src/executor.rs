//! 交易执行器：提交市价单并处理异步操作结果。

use crate::{error::RuntimeResult, RuntimeError};
use exness_api::{ClosePositionParams, OpenPositionRequest, PositionSide, RestClient};
use rust_decimal::Decimal;
use std::sync::Arc;

pub struct Executor {
    client: Arc<RestClient>,
    symbol: String,
}

impl Executor {
    pub fn new(client: Arc<RestClient>, symbol: String) -> Self {
        Self { client, symbol }
    }

    /// 按指定方向、手数、偏差开市价仓。
    pub async fn open_position(
        &self,
        idempotency_key: &str,
        side: PositionSide,
        volume: Decimal,
        _deviation: u32,
    ) -> RuntimeResult<String> {
        let request = OpenPositionRequest {
            instrument: self.symbol.clone(),
            side: side.to_string_value().to_string(),
            volume: volume.to_string(),
            price: None,
            stop_loss_price: None,
            take_profit_price: None,
            deviation: None, // 市价执行：不适用偏差
            comment: Some(format!("gl.{}", idempotency_key)),
        };

        let response = self
            .client
            .open_position(&request, idempotency_key)
            .await
            .map_err(|e| RuntimeError::Execution(format!("open_position failed: {}", e)))?;

        // 返回 operation_id 用于跟踪
        Ok(response.operation_id)
    }

    /// 按 ID 平掉某个持仓。
    pub async fn close_position(
        &self,
        idempotency_key: &str,
        position_id: &str,
        _deviation: u32,
    ) -> RuntimeResult<String> {
        let params = ClosePositionParams {
            position_id: position_id.to_string(),
            volume: None,
            price: None,
            deviation: None, // 市价执行：不适用偏差
            close_by_position_id: None,
        };

        let response = self
            .client
            .close_position(position_id, &params, idempotency_key)
            .await
            .map_err(|e| RuntimeError::Execution(format!("close_position failed: {}", e)))?;

        Ok(response.operation_id)
    }
}
