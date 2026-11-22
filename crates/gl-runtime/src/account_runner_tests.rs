//! AccountRunner 批量操作测试

#[cfg(test)]
mod tests {
    use crate::ledger::{LedgerEntry, PositionRole};
    use crate::ops_tracker::OpsTracker;
    use exness_api::PositionSide;
    use rust_decimal::Decimal;

    #[test]
    fn test_ledger_entry_creation() {
        let entry = LedgerEntry {
            position_id: "pos_123".to_string(),
            role: PositionRole::Grid,
            open_time: chrono::Utc::now(),
            volume: Decimal::new(1, 1), // 0.1
            open_price: Decimal::new(200000, 2), // 2000.00
            side: PositionSide::Buy,
        };

        assert_eq!(entry.position_id, "pos_123");
        assert_eq!(entry.role, PositionRole::Grid);
        assert_eq!(entry.volume, Decimal::new(1, 1));
        assert_eq!(entry.side, PositionSide::Buy);
    }

    #[test]
    fn test_position_roles() {
        let grid = PositionRole::Grid;
        let lock = PositionRole::Lock;
        let foreign = PositionRole::Foreign;

        assert!(matches!(grid, PositionRole::Grid));
        assert!(matches!(lock, PositionRole::Lock));
        assert!(matches!(foreign, PositionRole::Foreign));
    }

    #[test]
    fn test_ops_tracker_integration() {
        let mut tracker = OpsTracker::new("test_acc".to_string());

        // 模拟批量开仓
        for i in 0..5 {
            tracker.track_open(
                format!("op_{}", i),
                PositionRole::Grid,
                PositionSide::Buy,
                Decimal::new(1, 1),
            );
        }

        assert_eq!(tracker.pending_count(), 5);
        assert!(tracker.has_pending_side(PositionSide::Buy));

        // 模拟完成其中 3 个
        for i in 0..3 {
            let completed = tracker.mark_completed(&format!("op_{}", i));
            assert!(completed.is_some());
        }

        assert_eq!(tracker.pending_count(), 2);
    }

    #[test]
    fn test_batch_close_simulation() {
        let mut tracker = OpsTracker::new("test_acc".to_string());

        // 模拟批量平仓
        let positions = vec![
            ("pos_1", PositionRole::Grid),
            ("pos_2", PositionRole::Grid),
            ("pos_3", PositionRole::Lock),
        ];

        for (pos_id, role) in positions {
            tracker.track_close(
                format!("close_{}", pos_id),
                pos_id.to_string(),
                role,
            );
        }

        assert_eq!(tracker.pending_count(), 3);

        // Foreign 持仓不应该被跟踪
        tracker.track_close(
            "close_foreign".to_string(),
            "pos_foreign".to_string(),
            PositionRole::Foreign,
        );

        assert_eq!(tracker.pending_count(), 3); // 仍然是 3,Foreign 不计入
    }
}
