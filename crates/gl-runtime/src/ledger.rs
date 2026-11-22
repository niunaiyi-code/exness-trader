//! 持仓账本：跟踪本地持仓的角色归属（网格/锁仓/外部）。

use exness_api::PositionSide;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PositionRole {
    Grid,
    Lock,
    Foreign,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerEntry {
    pub position_id: String,
    pub role: PositionRole,
    pub open_time: chrono::DateTime<chrono::Utc>,
    pub volume: Decimal,
    pub open_price: Decimal,
    pub side: PositionSide,
}

/// 本运行器所开持仓的本地账本。
#[derive(Debug, Clone, Default)]
pub struct Ledger {
    entries: HashMap<String, LedgerEntry>,
}

impl Ledger {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, entry: LedgerEntry) {
        self.entries.insert(entry.position_id.clone(), entry);
    }

    pub fn remove(&mut self, position_id: &str) -> Option<LedgerEntry> {
        self.entries.remove(position_id)
    }

    pub fn get(&self, position_id: &str) -> Option<&LedgerEntry> {
        self.entries.get(position_id)
    }

    pub fn grid_positions(&self) -> impl Iterator<Item = &LedgerEntry> {
        self.entries
            .values()
            .filter(|e| e.role == PositionRole::Grid)
    }

    pub fn all_positions(&self) -> impl Iterator<Item = &LedgerEntry> {
        self.entries.values()
    }

    pub fn lock_positions(&self) -> impl Iterator<Item = &LedgerEntry> {
        self.entries
            .values()
            .filter(|e| e.role == PositionRole::Lock)
    }

    pub fn count_by_role(&self, role: PositionRole) -> usize {
        self.entries.values().filter(|e| e.role == role).count()
    }

    pub fn total_lots_by_role(&self, role: PositionRole) -> Decimal {
        self.entries
            .values()
            .filter(|e| e.role == role)
            .map(|e| e.volume)
            .sum()
    }

    pub fn total_volume_by_role(&self, role: PositionRole) -> Option<Decimal> {
        let vol = self.total_lots_by_role(role);
        if vol > Decimal::ZERO {
            Some(vol)
        } else {
            None
        }
    }

    /// 计算指定角色仓位的多空净敞口：Buy 手数减去 Sell 手数（>0 为净多，<0 为净空）
    pub fn net_exposure_by_role(&self, role: PositionRole) -> Decimal {
        let mut net = Decimal::ZERO;
        for entry in self.entries.values().filter(|e| e.role == role) {
            match entry.side {
                PositionSide::Buy => net += entry.volume,
                PositionSide::Sell => net -= entry.volume,
            }
        }
        net
    }

    pub fn dominant_side(&self, role: PositionRole) -> Option<PositionSide> {
        let mut buy_vol = Decimal::ZERO;
        let mut sell_vol = Decimal::ZERO;

        for entry in self.entries.values().filter(|e| e.role == role) {
            match entry.side {
                PositionSide::Buy => buy_vol += entry.volume,
                PositionSide::Sell => sell_vol += entry.volume,
            }
        }

        if buy_vol > sell_vol {
            Some(PositionSide::Buy)
        } else if sell_vol > buy_vol {
            Some(PositionSide::Sell)
        } else {
            None
        }
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }
}
