//! 对账：重启或出现偏差后，使账本与快照对齐。

use crate::{
    error::RuntimeResult,
    ledger::{Ledger, LedgerEntry, PositionRole},
};
use exness_api::{Position, PositionSide};
use std::collections::HashSet;

/// 用 API 快照对账（不可变版本，供检查）。
pub fn reconcile_positions(ledger: &Ledger, snapshot: &[Position]) -> Vec<Position> {
    let ledger_ids: HashSet<String> = ledger
        .grid_positions()
        .chain(ledger.lock_positions())
        .map(|e| e.position_id.clone())
        .collect();

    // 返回快照中有、账本中无的持仓 → 外部持仓
    snapshot
        .iter()
        .filter(|p| !ledger_ids.contains(&p.id))
        .cloned()
        .collect()
}

/// 用 API 快照对账（可变版本，用于实际对账）。
pub fn reconcile(ledger: &mut Ledger, snapshot: Vec<Position>) -> RuntimeResult<ReconcileReport> {
    let mut report = ReconcileReport::default();

    let ledger_ids: HashSet<String> = ledger
        .grid_positions()
        .chain(ledger.lock_positions())
        .map(|e| e.position_id.clone())
        .collect();

    let snapshot_ids: HashSet<String> = snapshot.iter().map(|p| p.id.clone()).collect();
    
    // ⚠️ 防御性检查 1: 空快照且账本非空 → 拒绝对账
    if snapshot.is_empty() && !ledger_ids.is_empty() {
        return Err(crate::RuntimeError::Reconcile(format!(
            "拒绝对账：快照为空但账本有 {} 个持仓，可能是 API 异常",
            ledger_ids.len()
        )));
    }
    
    // ⚠️ 防御性检查 2: 变更过大（超过 50% 且 > 5 个）→ 警告但继续
    let to_remove: Vec<_> = ledger_ids.difference(&snapshot_ids).cloned().collect();
    let to_add: Vec<_> = snapshot.iter().filter(|p| !ledger_ids.contains(&p.id)).cloned().collect();
    
    let total_changes = to_remove.len() + to_add.len();
    let current_size = ledger_ids.len();
    
    if total_changes > current_size / 2 && current_size > 5 {
        tracing::warn!(
            "对账变更过大：移除 {} 个，新增 {} 个（账本共 {} 个），请人工确认",
            to_remove.len(), to_add.len(), current_size
        );
        // 可选：返回错误或仅记录到审计日志
    }

    // 账本中有、快照中无 → 已在外部平仓
    for id in to_remove {
        if let Some(entry) = ledger.remove(&id) {
            report.removed.push(entry);
        }
    }

    // 快照中有、账本中无 → 外部持仓（外部开的仓）
    for pos in to_add {
        let side = match PositionSide::from_string_value(&pos.position_type) {
            Some(s) => s,
            None => {
                tracing::warn!(
                    position_id = %pos.id,
                    position_type = %pos.position_type,
                    "Unknown position_type in reconcile; skipping ledger adoption"
                );
                continue;
            }
        };
        let entry = LedgerEntry {
            position_id: pos.id.clone(),
            role: PositionRole::Foreign,
            open_time: chrono::DateTime::from_timestamp(pos.open_time, 0)
                .unwrap_or_else(chrono::Utc::now),
            volume: pos.volume.parse().unwrap_or_default(),
            open_price: pos.open_price.parse().unwrap_or_default(),
            side,
        };
        ledger.insert(entry.clone());
        report.adopted.push(entry);
    }
    
    tracing::info!(
        "对账完成：移除 {} 个，新增 {} 个",
        report.removed.len(),
        report.adopted.len()
    );

    Ok(report)
}

#[derive(Debug, Default)]
pub struct ReconcileReport {
    pub removed: Vec<LedgerEntry>,
    pub adopted: Vec<LedgerEntry>,
}
