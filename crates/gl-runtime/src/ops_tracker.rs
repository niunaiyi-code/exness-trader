//! 操作跟踪器：生成幂等键并跟踪在途异步操作。

use crate::ledger::PositionRole;
use exness_api::PositionSide;
use rust_decimal::Decimal;
use std::collections::HashMap;
use uuid::Uuid;

/// 异步交易操作跟踪器。
#[derive(Debug)]
pub struct OpsTracker {
    account_id: String,
    pending: HashMap<String, PendingOp>,
    by_operation_id: HashMap<String, String>, // operation_id -> idempotency_key
}

#[derive(Debug, Clone)]
pub struct PendingOp {
    pub idempotency_key: String,
    pub operation_id: Option<String>,
    pub kind: OpKind,
    pub position_id: Option<String>,
    pub role: Option<PositionRole>,
    pub side: Option<PositionSide>,
    pub volume: Option<Decimal>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpKind {
    OpenGrid,
    OpenLock,
    CloseGrid,
    CloseLock,
    EmergencyClose,
}

impl OpsTracker {
    pub fn new(account_id: String) -> Self {
        Self {
            account_id,
            pending: HashMap::new(),
            by_operation_id: HashMap::new(),
        }
    }

    /// 生成幂等键：`gl.{account}.{role}.{layer}.{unix_ms}.{rand6}`
    pub fn generate_key(&self, role: &str, layer: u32) -> String {
        let now = chrono::Utc::now().timestamp_millis();
        let rand = Uuid::new_v4().to_string().chars().take(6).collect::<String>();
        format!("gl.{}.{}.{}.{}.{}", self.account_id, role, layer, now, rand)
    }

    pub fn track(&mut self, key: String, kind: OpKind) {
        let op = PendingOp {
            idempotency_key: key.clone(),
            operation_id: None,
            kind,
            position_id: None,
            role: None,
            side: None,
            volume: None,
            created_at: chrono::Utc::now(),
        };
        self.pending.insert(key, op);
    }

    pub fn track_open(&mut self, operation_id: String, role: PositionRole, side: PositionSide, volume: Decimal) {
        let kind = match role {
            PositionRole::Grid => OpKind::OpenGrid,
            PositionRole::Lock => OpKind::OpenLock,
            PositionRole::Foreign => return,
        };

        let key = self.generate_key(&format!("{:?}", role), 1);
        let op = PendingOp {
            idempotency_key: key.clone(),
            operation_id: Some(operation_id.clone()),
            kind,
            position_id: None,
            role: Some(role),
            side: Some(side),
            volume: Some(volume),
            created_at: chrono::Utc::now(),
        };

        self.pending.insert(key.clone(), op);
        self.by_operation_id.insert(operation_id, key);
    }

    pub fn track_close(&mut self, operation_id: String, position_id: String, role: PositionRole) {
        let kind = match role {
            PositionRole::Grid => OpKind::CloseGrid,
            PositionRole::Lock => OpKind::CloseLock,
            PositionRole::Foreign => return,
        };

        let key = self.generate_key(&format!("{:?}", role), 1);
        let op = PendingOp {
            idempotency_key: key.clone(),
            operation_id: Some(operation_id.clone()),
            kind,
            position_id: Some(position_id),
            role: Some(role),
            side: None,
            volume: None,
            created_at: chrono::Utc::now(),
        };

        self.pending.insert(key.clone(), op);
        self.by_operation_id.insert(operation_id, key);
    }

    pub fn set_operation_id(&mut self, key: &str, operation_id: String) {
        if let Some(op) = self.pending.get_mut(key) {
            op.operation_id = Some(operation_id.clone());
            self.by_operation_id.insert(operation_id, key.to_string());
        }
    }

    pub fn mark_completed(&mut self, operation_id: &str) -> Option<PendingOp> {
        if let Some(key) = self.by_operation_id.remove(operation_id) {
            self.pending.remove(&key)
        } else {
            None
        }
    }

    pub fn mark_failed(&mut self, operation_id: &str) -> Option<PendingOp> {
        self.mark_completed(operation_id)
    }

    pub fn complete(&mut self, key: &str) -> Option<PendingOp> {
        if let Some(op) = self.pending.remove(key) {
            if let Some(ref operation_id) = op.operation_id {
                self.by_operation_id.remove(operation_id);
            }
            Some(op)
        } else {
            None
        }
    }

    /// 使在途时间超过 `max_age_secs` 的操作过期。
    pub fn expire_stale(&mut self, max_age_secs: i64) -> Vec<PendingOp> {
        let now = chrono::Utc::now();
        let mut expired = Vec::new();
        let stale_keys: Vec<String> = self
            .pending
            .iter()
            .filter(|(_, op)| (now - op.created_at).num_seconds() > max_age_secs)
            .map(|(k, _)| k.clone())
            .collect();

        for key in stale_keys {
            if let Some(op) = self.pending.remove(&key) {
                if let Some(ref operation_id) = op.operation_id {
                    self.by_operation_id.remove(operation_id);
                }
                expired.push(op);
            }
        }
        expired
    }

    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    pub fn has_pending_side(&self, side: PositionSide) -> bool {
        self.pending.values().any(|op| op.side == Some(side))
    }

    pub fn has_pending_side_kind(&self, side: PositionSide, kind: OpKind) -> bool {
        self.pending.values().any(|op| op.side == Some(side) && op.kind == kind)
    }

    /// 判断指定持仓是否已有在途平仓操作（用于止盈/止损平仓前去重，避免高频重复下发平仓指令）。
    pub fn has_pending_close(&self, position_id: &str) -> bool {
        self.pending.values().any(|op| {
            op.position_id.as_deref() == Some(position_id)
                && matches!(op.kind, OpKind::CloseGrid | OpKind::CloseLock | OpKind::EmergencyClose)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_track_and_complete() {
        let mut tracker = OpsTracker::new("test_acc".to_string());
        let key = tracker.generate_key("Grid", 1);
        tracker.track(key.clone(), OpKind::OpenGrid);

        assert_eq!(tracker.pending_count(), 1);
        
        let completed = tracker.complete(&key);
        assert!(completed.is_some());
        assert_eq!(tracker.pending_count(), 0);
    }

    #[test]
    fn test_track_open_and_mark_completed() {
        let mut tracker = OpsTracker::new("test_acc".to_string());
        tracker.track_open(
            "op_123".to_string(),
            PositionRole::Grid,
            PositionSide::Buy,
            Decimal::new(1, 1), // 0.1
        );

        assert_eq!(tracker.pending_count(), 1);
        
        let completed = tracker.mark_completed("op_123");
        assert!(completed.is_some());
        assert_eq!(completed.unwrap().kind, OpKind::OpenGrid);
        assert_eq!(tracker.pending_count(), 0);
    }

    #[test]
    fn test_expire_stale() {
        let mut tracker = OpsTracker::new("test_acc".to_string());
        
        // 添加一个旧操作
        let key = tracker.generate_key("Grid", 1);
        let op = PendingOp {
            idempotency_key: key.clone(),
            operation_id: Some("old_op".to_string()),
            kind: OpKind::OpenGrid,
            position_id: None,
            role: Some(PositionRole::Grid),
            side: Some(PositionSide::Buy),
            volume: Some(Decimal::new(1, 1)),
            created_at: chrono::Utc::now() - chrono::Duration::seconds(100),
        };
        tracker.pending.insert(key.clone(), op.clone());
        tracker.by_operation_id.insert("old_op".to_string(), key);

        // 添加一个新操作
        tracker.track_open(
            "new_op".to_string(),
            PositionRole::Grid,
            PositionSide::Sell,
            Decimal::new(2, 1), // 0.2
        );

        assert_eq!(tracker.pending_count(), 2);

        // 过期 50 秒以上的操作
        let expired = tracker.expire_stale(50);
        
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0].operation_id, Some("old_op".to_string()));
        assert_eq!(tracker.pending_count(), 1);
    }

    #[test]
    fn test_has_pending_side() {
        let mut tracker = OpsTracker::new("test_acc".to_string());
        
        assert!(!tracker.has_pending_side(PositionSide::Buy));
        
        tracker.track_open(
            "op_buy".to_string(),
            PositionRole::Grid,
            PositionSide::Buy,
            Decimal::new(1, 1),
        );

        assert!(tracker.has_pending_side(PositionSide::Buy));
        assert!(!tracker.has_pending_side(PositionSide::Sell));
    }

    #[test]
    fn test_has_pending_side_kind() {
        let mut tracker = OpsTracker::new("test_acc".to_string());
        
        tracker.track_open(
            "op_grid".to_string(),
            PositionRole::Grid,
            PositionSide::Buy,
            Decimal::new(1, 1),
        );

        assert!(tracker.has_pending_side_kind(PositionSide::Buy, OpKind::OpenGrid));
        assert!(!tracker.has_pending_side_kind(PositionSide::Buy, OpKind::OpenLock));
        assert!(!tracker.has_pending_side_kind(PositionSide::Sell, OpKind::OpenGrid));
    }

    #[test]
    fn test_track_close() {
        let mut tracker = OpsTracker::new("test_acc".to_string());
        
        tracker.track_close(
            "op_close".to_string(),
            "pos_123".to_string(),
            PositionRole::Grid,
        );

        assert_eq!(tracker.pending_count(), 1);
        
        let completed = tracker.mark_completed("op_close");
        assert!(completed.is_some());
        let op = completed.unwrap();
        assert_eq!(op.kind, OpKind::CloseGrid);
        assert_eq!(op.position_id, Some("pos_123".to_string()));
    }

    #[test]
    fn test_foreign_position_not_tracked() {
        let mut tracker = OpsTracker::new("test_acc".to_string());
        
        // Foreign 持仓不应该被跟踪
        tracker.track_open(
            "op_foreign".to_string(),
            PositionRole::Foreign,
            PositionSide::Buy,
            Decimal::new(1, 1),
        );

        assert_eq!(tracker.pending_count(), 0);
        
        tracker.track_close(
            "op_foreign_close".to_string(),
            "pos_foreign".to_string(),
            PositionRole::Foreign,
        );

        assert_eq!(tracker.pending_count(), 0);
    }
}
