//! 账户运行器 — 生命周期、账本、对账（docs/03、04 §6-7）。

use crate::types::Side;
use rust_decimal::Decimal;
use std::collections::HashMap;

/// 账本中的持仓归属。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PositionRole {
    Grid,    // 策略网格订单。
    Lock,    // 对冲锁仓单。
    Foreign, // 属于其他 EA/用户。
}

/// 账本条目 — 自有持仓。
#[derive(Debug, Clone)]
pub struct LedgerEntry {
    pub position_id: String,
    pub role: PositionRole,
    pub side: Side,
    pub layer: usize,
    pub volume: Decimal,
    pub open_price: Decimal,
    pub open_time: i64,
    pub client_request_id: String,
    pub status: LedgerStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LedgerStatus {
    Open,
    Closing,
    Closed,
}

/// 账户状态机。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountState {
    /// 尚未初始化。
    Init,
    /// 拉取凭据、交易条件、限制。
    Warmup,
    /// 正常运行。
    Running,
    /// 降级：连接异常，等待恢复。
    Degraded,
    /// 被用户停止。
    Stopped,
    /// 致命错误。
    Failed,
}

/// 账户运行器 — 管理单个交易账户的生命周期 + 账本。
pub struct AccountRunner {
    pub account_id: String,
    pub state: AccountState,
    pub ledger: HashMap<String, LedgerEntry>,
    /// 上次快照中的持仓（position_id -> 快照数据）。
    pub snapshot_cache: HashMap<String, SnapshotPosition>,
    /// 上次对账时间。
    pub last_reconcile_ms: u64,
    /// 对账间隔（10 分钟 = 600000 毫秒）。
    pub reconcile_interval_ms: u64,
}

#[derive(Debug, Clone)]
pub struct SnapshotPosition {
    pub position_id: String,
    pub side: Side,
    pub volume: Decimal,
    pub open_price: Decimal,
    pub current_price: Decimal,
    pub open_time: i64,
}

impl AccountRunner {
    pub fn new(account_id: String) -> Self {
        Self {
            account_id,
            state: AccountState::Init,
            ledger: HashMap::new(),
            snapshot_cache: HashMap::new(),
            last_reconcile_ms: 0,
            reconcile_interval_ms: 600000, // 10 分钟
        }
    }

    /// 将已确认持仓登记入账本。
    pub fn register_position(
        &mut self,
        position_id: String,
        role: PositionRole,
        side: Side,
        layer: usize,
        volume: Decimal,
        open_price: Decimal,
        open_time: i64,
        client_request_id: String,
    ) {
        let entry = LedgerEntry {
            position_id: position_id.clone(),
            role,
            side,
            layer,
            volume,
            open_price,
            open_time,
            client_request_id,
            status: LedgerStatus::Open,
        };
        self.ledger.insert(position_id, entry);
    }

    /// 将持仓标记为平仓中。
    pub fn mark_closing(&mut self, position_id: &str) {
        if let Some(entry) = self.ledger.get_mut(position_id) {
            entry.status = LedgerStatus::Closing;
        }
    }

    /// 将持仓标记为已平（对账或显式平仓）。
    pub fn mark_closed(&mut self, position_id: &str) {
        if let Some(entry) = self.ledger.get_mut(position_id) {
            entry.status = LedgerStatus::Closed;
        }
    }

    /// 从 API 更新快照缓存。
    pub fn update_snapshot(&mut self, positions: Vec<SnapshotPosition>) {
        self.snapshot_cache.clear();
        for pos in positions {
            self.snapshot_cache.insert(pos.position_id.clone(), pos);
        }
    }

    /// 账本与快照对账（docs/04 §7）。
    pub fn reconcile(&mut self, now_ms: u64) -> ReconciliationResult {
        self.last_reconcile_ms = now_ms;

        let mut result = ReconciliationResult {
            missing_from_snapshot: Vec::new(),
            foreign_positions: Vec::new(),
            updates: Vec::new(),
        };

        // L \ S：账本中存在但快照缺失的条目 → 标记为已平仓。
        let missing_ids: Vec<String> = self
            .ledger
            .iter()
            .filter(|(pos_id, entry)| {
                !self.snapshot_cache.contains_key(*pos_id) && entry.status == LedgerStatus::Open
            })
            .map(|(pos_id, _)| pos_id.clone())
            .collect();
        for pos_id in missing_ids {
            result.missing_from_snapshot.push(pos_id.clone());
            self.mark_closed(&pos_id);
        }

        // S \ L：快照中有、账本中无 → 外部或未被认领。
        for (pos_id, snap) in self.snapshot_cache.iter() {
            if !self.ledger.contains_key(pos_id) {
                result.foreign_positions.push(ForeignPosition {
                    position_id: pos_id.clone(),
                    side: snap.side,
                    volume: snap.volume,
                });
            }
        }

        // S ∩ L：同步手数、价格（部分平仓等）。
        for (pos_id, snap) in self.snapshot_cache.iter() {
            if let Some(entry) = self.ledger.get_mut(pos_id) {
                if entry.volume != snap.volume {
                    result.updates.push(LedgerUpdate {
                        position_id: pos_id.clone(),
                        old_volume: entry.volume,
                        new_volume: snap.volume,
                    });
                    entry.volume = snap.volume;
                }
            }
        }

        result
    }

    /// 按方向获取所有未平网格持仓。
    pub fn grid_positions_by_side(&self, side: Side) -> Vec<&LedgerEntry> {
        self.ledger
            .values()
            .filter(|e| e.role == PositionRole::Grid && e.side == side && e.status == LedgerStatus::Open)
            .collect()
    }

    /// 获取所有未平持仓（用于止盈检查、加网格检查等）。
    pub fn open_positions(&self) -> Vec<&LedgerEntry> {
        self.ledger.values().filter(|e| e.status == LedgerStatus::Open).collect()
    }

    /// 获取锁仓持仓。
    pub fn lock_position(&self) -> Option<&LedgerEntry> {
        self.ledger.values().find(|e| e.role == PositionRole::Lock && e.status == LedgerStatus::Open)
    }
}

#[derive(Debug, Clone)]
pub struct ReconciliationResult {
    pub missing_from_snapshot: Vec<String>,    // 已消失的持仓 ID。
    pub foreign_positions: Vec<ForeignPosition>, // 不在账本中的持仓。
    pub updates: Vec<LedgerUpdate>,            // 手数变化。
}

#[derive(Debug, Clone)]
pub struct ForeignPosition {
    pub position_id: String,
    pub side: Side,
    pub volume: Decimal,
}

#[derive(Debug, Clone)]
pub struct LedgerUpdate {
    pub position_id: String,
    pub old_volume: Decimal,
    pub new_volume: Decimal,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> Decimal {
        s.parse().unwrap()
    }

    #[test]
    fn runner_new() {
        let runner = AccountRunner::new("acc_123".to_string());
        assert_eq!(runner.account_id, "acc_123");
        assert_eq!(runner.state, AccountState::Init);
        assert!(runner.ledger.is_empty());
    }

    #[test]
    fn register_position() {
        let mut runner = AccountRunner::new("acc_123".to_string());
        runner.register_position(
            "pos_1".to_string(),
            PositionRole::Grid,
            Side::Buy,
            0,
            d("0.01"),
            d("3700.50"),
            1000,
            "gl.op.1".to_string(),
        );
        assert!(runner.ledger.contains_key("pos_1"));
        assert_eq!(runner.ledger["pos_1"].role, PositionRole::Grid);
    }

    #[test]
    fn grid_positions_by_side() {
        let mut runner = AccountRunner::new("acc_123".to_string());
        runner.register_position(
            "pos_1".to_string(),
            PositionRole::Grid,
            Side::Buy,
            0,
            d("0.01"),
            d("3700.50"),
            1000,
            "gl.op.1".to_string(),
        );
        runner.register_position(
            "pos_2".to_string(),
            PositionRole::Grid,
            Side::Buy,
            1,
            d("0.02"),
            d("3699.00"),
            1010,
            "gl.op.2".to_string(),
        );
        runner.register_position(
            "pos_3".to_string(),
            PositionRole::Grid,
            Side::Sell,
            0,
            d("0.01"),
            d("3701.00"),
            1020,
            "gl.op.3".to_string(),
        );

        let buy_pos = runner.grid_positions_by_side(Side::Buy);
        assert_eq!(buy_pos.len(), 2);

        let sell_pos = runner.grid_positions_by_side(Side::Sell);
        assert_eq!(sell_pos.len(), 1);
    }

    #[test]
    fn reconcile_missing() {
        let mut runner = AccountRunner::new("acc_123".to_string());
        runner.register_position(
            "pos_1".to_string(),
            PositionRole::Grid,
            Side::Buy,
            0,
            d("0.01"),
            d("3700.50"),
            1000,
            "gl.op.1".to_string(),
        );
        // 快照为空（pos_1 已消失）。
        runner.update_snapshot(vec![]);

        let result = runner.reconcile(2000);
        assert_eq!(result.missing_from_snapshot.len(), 1);
        assert_eq!(result.missing_from_snapshot[0], "pos_1");
    }

    #[test]
    fn reconcile_foreign() {
        let mut runner = AccountRunner::new("acc_123".to_string());
        runner.update_snapshot(vec![SnapshotPosition {
            position_id: "pos_unknown".to_string(),
            side: Side::Buy,
            volume: d("0.05"),
            open_price: d("3700.00"),
            current_price: d("3705.00"),
            open_time: 1000,
        }]);

        let result = runner.reconcile(2000);
        assert_eq!(result.foreign_positions.len(), 1);
        assert_eq!(result.foreign_positions[0].position_id, "pos_unknown");
    }

    #[test]
    fn reconcile_volume_update() {
        let mut runner = AccountRunner::new("acc_123".to_string());
        runner.register_position(
            "pos_1".to_string(),
            PositionRole::Grid,
            Side::Buy,
            0,
            d("0.01"),
            d("3700.50"),
            1000,
            "gl.op.1".to_string(),
        );
        // 部分平仓：手数减至 0.005。
        runner.update_snapshot(vec![SnapshotPosition {
            position_id: "pos_1".to_string(),
            side: Side::Buy,
            volume: d("0.005"),
            open_price: d("3700.50"),
            current_price: d("3702.00"),
            open_time: 1000,
        }]);

        let result = runner.reconcile(2000);
        assert_eq!(result.updates.len(), 1);
        assert_eq!(result.updates[0].new_volume, d("0.005"));
    }

    #[test]
    fn lock_position() {
        let mut runner = AccountRunner::new("acc_123".to_string());
        runner.register_position(
            "pos_lock".to_string(),
            PositionRole::Lock,
            Side::Buy,
            0,
            d("0.20"),
            d("3700.00"),
            1000,
            "gl.lock.1".to_string(),
        );
        assert!(runner.lock_position().is_some());
        assert_eq!(runner.lock_position().unwrap().role, PositionRole::Lock);
    }
}
