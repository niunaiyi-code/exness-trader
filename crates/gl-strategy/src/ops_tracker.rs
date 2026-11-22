//! 操作跟踪器 — 基于 OpsTracker 的幂等下单（docs/02 §5、04 §5）。

use crate::types::Side;
use rust_decimal::Decimal;
use std::collections::HashMap;

/// 跟踪器中的操作状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpStatus {
    /// 已在本地登记，等待 API 响应。
    Pending,
    /// API 返回 202 并携带 operation_id，等待 WS 事件或超时。
    Acked,
    /// 终态：成功。
    Confirmed,
    /// 终态：已拒绝 / 失败。
    Rejected,
    /// 终态：超时，等待对账。
    Timeout,
}

/// 操作结果（供策略决策）。
#[derive(Debug, Clone)]
pub enum OpResult {
    /// 成功：已创建新持仓。
    Confirmed { position_id: String, volume: Decimal, open_price: Decimal },
    /// 已拒绝：附带错误码/错误信息。
    Rejected { code: String, msg: String },
    /// 仍在处理中：策略此时不应重试。
    Pending,
    /// 超时：需要对账；策略按未知状态处理。
    Timeout,
}

/// 被跟踪的操作（按 client_request_id）。
#[derive(Debug, Clone)]
pub struct TrackedOp {
    pub client_request_id: String,
    pub operation_id: Option<String>,
    pub side: Side,
    pub volume: Decimal,
    pub layer: usize,
    pub status: OpStatus,
    pub created_at_ms: u64,
    pub acked_at_ms: Option<u64>,
    pub completed_at_ms: Option<u64>,
    pub result: Option<OpResult>,
}

/// 操作跟踪器 — 以幂等方式管理在途订单。
pub struct OpsTracker {
    /// 所有被跟踪的操作（client_request_id -> TrackedOp）。
    pub operations: HashMap<String, TrackedOp>,
    /// 超时阈值：3 秒后标记为超时（docs/04 §5）。
    pub timeout_ms: u64,
    /// 每个操作的最大重试次数（docs/04 §5）。
    pub max_retries: usize,
}

impl OpsTracker {
    pub fn new() -> Self {
        Self {
            operations: HashMap::new(),
            timeout_ms: 3000,
            max_retries: 3,
        }
    }

    /// 登记一个操作（发往 API 之前）。
    pub fn register(&mut self, client_request_id: String, side: Side, volume: Decimal, layer: usize, now_ms: u64) {
        let op = TrackedOp {
            client_request_id: client_request_id.clone(),
            operation_id: None,
            side,
            volume,
            layer,
            status: OpStatus::Pending,
            created_at_ms: now_ms,
            acked_at_ms: None,
            completed_at_ms: None,
            result: None,
        };
        self.operations.insert(client_request_id, op);
    }

    /// 标记为已 ACK（API 返回 202 后）。
    pub fn ack(&mut self, client_request_id: &str, operation_id: String, now_ms: u64) {
        if let Some(op) = self.operations.get_mut(client_request_id) {
            op.operation_id = Some(operation_id);
            op.status = OpStatus::Acked;
            op.acked_at_ms = Some(now_ms);
        }
    }

    /// 确认成功（来自 WS 事件或轮询）。
    pub fn confirm(
        &mut self,
        client_request_id: &str,
        position_id: String,
        open_price: Decimal,
        now_ms: u64,
    ) {
        if let Some(op) = self.operations.get_mut(client_request_id) {
            op.status = OpStatus::Confirmed;
            op.completed_at_ms = Some(now_ms);
            op.result = Some(OpResult::Confirmed { position_id, volume: op.volume, open_price });
        }
    }

    /// 标记为已拒绝（来自 WS 事件或 API 错误）。
    pub fn reject(&mut self, client_request_id: &str, code: String, msg: String, now_ms: u64) {
        if let Some(op) = self.operations.get_mut(client_request_id) {
            op.status = OpStatus::Rejected;
            op.completed_at_ms = Some(now_ms);
            op.result = Some(OpResult::Rejected { code, msg });
        }
    }

    /// 标记为超时（ACK 后 3 秒仍无 WS 事件）。
    pub fn timeout(&mut self, client_request_id: &str, now_ms: u64) {
        if let Some(op) = self.operations.get_mut(client_request_id) {
            op.status = OpStatus::Timeout;
            op.completed_at_ms = Some(now_ms);
            op.result = Some(OpResult::Timeout);
        }
    }

    /// 检查超时操作（ACK 已超过 3 秒仍无结果）。
    pub fn find_expired(&self, now_ms: u64) -> Vec<String> {
        self.operations
            .values()
            .filter(|op| {
                if op.status != OpStatus::Acked {
                    return false;
                }
                if let Some(acked_ms) = op.acked_at_ms {
                    now_ms.saturating_sub(acked_ms) >= self.timeout_ms
                } else {
                    false
                }
            })
            .map(|op| op.client_request_id.clone())
            .collect()
    }

    /// 获取某个 client_request_id 的结果。
    pub fn get_result(&self, client_request_id: &str) -> Option<OpResult> {
        self.operations.get(client_request_id).and_then(|op| op.result.clone())
    }

    /// 清理已完成的操作。
    pub fn remove(&mut self, client_request_id: &str) {
        self.operations.remove(client_request_id);
    }

    /// 清理 24 小时前已完成的操作（保留操作记录用于审计）。
    pub fn cleanup_old(&mut self, now_ms: u64, retention_ms: u64) {
        self.operations.retain(|_, op| {
            if let Some(completed_ms) = op.completed_at_ms {
                now_ms.saturating_sub(completed_ms) < retention_ms
            } else {
                true // 保留在途操作
            }
        });
    }
    
    /// 清理超时的在途操作（防止内存泄漏）。返回清理数量。
    pub fn expire_stale(&mut self, max_age_secs: i64) -> usize {
        let max_age_ms = (max_age_secs * 1000) as u64;
        let mut expired_count = 0;
        
        self.operations.retain(|_key, op| {
            let age_ms = if let Some(completed_ms) = op.completed_at_ms {
                // 已完成的操作，按完成时间计算
                op.created_at_ms.saturating_sub(completed_ms)
            } else {
                // 在途操作，按创建时间计算（使用当前时间作为参考）
                // 注意：这里需要传入当前时间，暂用 created_at_ms 作为基准
                0 // 占位：实际需要 now_ms 参数
            };
            
            if op.completed_at_ms.is_none() && age_ms > max_age_ms {
                expired_count += 1;
                false // 移除
            } else {
                true // 保留
            }
        });
        
        expired_count
    }
    
    /// 返回在途操作数量（用于防御性检查）。
    pub fn pending_count(&self) -> usize {
        self.operations
            .values()
            .filter(|op| matches!(op.status, OpStatus::Pending | OpStatus::Acked))
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> Decimal {
        s.parse().unwrap()
    }

    #[test]
    fn ops_tracker_new() {
        let tracker = OpsTracker::new();
        assert_eq!(tracker.timeout_ms, 3000);
        assert!(tracker.operations.is_empty());
    }

    #[test]
    fn register_and_ack() {
        let mut tracker = OpsTracker::new();
        tracker.register("op1".to_string(), Side::Buy, d("0.01"), 0, 1000);
        assert!(tracker.operations.contains_key("op1"));
        assert_eq!(tracker.operations["op1"].status, OpStatus::Pending);

        tracker.ack("op1", "exn-op-123".to_string(), 1010);
        assert_eq!(tracker.operations["op1"].status, OpStatus::Acked);
        assert_eq!(tracker.operations["op1"].operation_id, Some("exn-op-123".to_string()));
    }

    #[test]
    fn confirm_operation() {
        let mut tracker = OpsTracker::new();
        tracker.register("op1".to_string(), Side::Buy, d("0.01"), 0, 1000);
        tracker.ack("op1", "exn-op-123".to_string(), 1010);
        tracker.confirm("op1", "pos-456".to_string(), d("3700.50"), 1500);

        assert_eq!(tracker.operations["op1"].status, OpStatus::Confirmed);
        if let Some(OpResult::Confirmed { position_id, volume, open_price }) = &tracker.operations["op1"].result {
            assert_eq!(position_id, "pos-456");
            assert_eq!(volume, &d("0.01"));
            assert_eq!(open_price, &d("3700.50"));
        } else {
            panic!("Expected Confirmed result");
        }
    }

    #[test]
    fn reject_operation() {
        let mut tracker = OpsTracker::new();
        tracker.register("op1".to_string(), Side::Buy, d("0.01"), 0, 1000);
        tracker.ack("op1", "exn-op-123".to_string(), 1010);
        tracker.reject("op1", "INSUFFICIENT_BALANCE".to_string(), "Not enough margin".to_string(), 1500);

        assert_eq!(tracker.operations["op1"].status, OpStatus::Rejected);
        if let Some(OpResult::Rejected { code, msg }) = &tracker.operations["op1"].result {
            assert_eq!(code, "INSUFFICIENT_BALANCE");
            assert_eq!(msg, "Not enough margin");
        } else {
            panic!("Expected Rejected result");
        }
    }

    #[test]
    fn find_expired() {
        let mut tracker = OpsTracker::new();
        tracker.register("op1".to_string(), Side::Buy, d("0.01"), 0, 1000);
        tracker.ack("op1", "exn-op-123".to_string(), 1010);

        // 尚未超期（经过 1.99 秒）。
        let expired = tracker.find_expired(3000);
        assert!(expired.is_empty());

        // 已超期（经过 3.01 秒）。
        let expired = tracker.find_expired(4020);
        assert_eq!(expired, vec!["op1"]);
    }

    #[test]
    fn cleanup_old() {
        let mut tracker = OpsTracker::new();
        tracker.register("op1".to_string(), Side::Buy, d("0.01"), 0, 1000);
        tracker.confirm("op1", "pos-456".to_string(), d("3700.50"), 2000);

        // 保留期 = 1 天。
        let now = 2000 + 86400 * 1000 + 1000; // 完成后 1 天 + 1 秒
        tracker.cleanup_old(now, 86400 * 1000);
        assert!(tracker.operations.is_empty());
    }
}
