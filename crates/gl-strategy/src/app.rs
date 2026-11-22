//! 策略应用编排器 — 每 tick 的主事件循环。
//! (docs/02 全部章节综合编排).

use crate::basket::{Basket, MarketData, PositionView};
use crate::config::{Config, RuntimeConstants, StrategyConstants};
use crate::lock_manager::{AccountSnapshot, LockManager, LockDecision};
use crate::risk_manager::{GateContext, RiskManager};
use crate::types::Side;
use rust_decimal::Decimal;

/// 主策略应用。
pub struct GoldLegendApp {
    pub config: Config,
    pub constants: StrategyConstants,
    pub runtime: RuntimeConstants,
    pub buy_basket: Basket,
    pub sell_basket: Basket,
    pub lock_manager: LockManager,
    pub risk_manager: RiskManager,
    /// 各方向上次入场时间（用于冷却）。
    pub last_entry_time: [i64; 2], // [买入=0，卖出=1]
    /// 各方向上次平仓时间（用于冷却）。
    pub last_close_time: [i64; 2],
}

impl GoldLegendApp {
    pub fn new(
        config: Config,
        constants: StrategyConstants,
        runtime: RuntimeConstants,
        magic: u64,
        max_spread_usd: Decimal,
    ) -> Self {
        Self {
            config,
            constants,
            runtime,
            buy_basket: Basket::new(Side::Buy),
            sell_basket: Basket::new(Side::Sell),
            lock_manager: LockManager::new(magic),
            risk_manager: RiskManager::new(max_spread_usd),
            last_entry_time: [0, 0],
            last_close_time: [0, 0],
        }
    }

    /// 每 tick 主决策：汇总 → 止盈 → 加网格 → 入场 → 执行。
    pub fn on_tick(
        &mut self,
        now: i64,
        mkt: &MarketData,
        positions: &[PositionView],
        snap: &AccountSnapshot,
    ) -> TickDecision {
        let mut decision = TickDecision::default();

        // 步骤 1：将持仓汇总进篮子。
        self.buy_basket.reset_stats();
        self.sell_basket.reset_stats();
        for pos in positions {
            self.buy_basket.accumulate(pos);
            self.sell_basket.accumulate(pos);
        }
        self.buy_basket.finalize();
        self.sell_basket.finalize();

        // 步骤 2：检查锁仓状态机。
        let lock_decision = self.lock_manager.check_and_execute(now, snap, self.config.lock_trigger_usd);
        decision.is_locked = self.lock_manager.is_locked();
        let is_frozen = lock_decision == LockDecision::Frozen;
        if lock_decision == LockDecision::AutoClose {
            decision.should_close_all = true;
            return decision;
        }
        if let LockDecision::PlaceLock { lock_lot, is_net_long } = lock_decision {
            decision.lock_order = Some((lock_lot, is_net_long));
        }

        // 步骤 3：检查止盈（双向）。
        let max_orders = 9; // TODO：由 max_loss / 杠杆预算推导
        if let Some(tp_action) = self.buy_basket.check_take_profit(
            mkt,
            &self.config,
            &self.constants,
            &self.runtime,
            max_orders,
        ) {
            decision.buy_tp = Some(tp_action);
        }
        if let Some(tp_action) = self.sell_basket.check_take_profit(
            mkt,
            &self.config,
            &self.constants,
            &self.runtime,
            max_orders,
        ) {
            decision.sell_tp = Some(tp_action);
        }

        if is_frozen {
            return decision; // 锁仓期间禁止网格操作。
        }

        // 步骤 4：检查加网格（双向）。
        let step_points = Decimal::from(self.config.grid_step)
            + Decimal::from(self.config.grid_step_add) * Decimal::from(self.buy_basket.stats().count);
        let gate_ctx = GateContext {
            is_locked: decision.is_locked,
            is_paused: false, // TODO：来自 UI
            cur_spread_usd: mkt.spread_usd,
            now_time: now,
            last_close_time: self.last_close_time[0],
            last_entry_time: self.last_entry_time[0],
        };
        if self.risk_manager.can_add_grid(&gate_ctx) {
            if let Some(add_decision) = self.buy_basket.check_add_grid(
                mkt,
                &self.config,
                &self.runtime,
                max_orders,
                step_points,
                0, // _now_ms 未使用
                now,
            ) {
                decision.buy_add_grid = Some(add_decision);
            }
        }
        let step_points = Decimal::from(self.config.grid_step)
            + Decimal::from(self.config.grid_step_add) * Decimal::from(self.sell_basket.stats().count);
        let gate_ctx = GateContext {
            is_locked: decision.is_locked,
            is_paused: false,
            cur_spread_usd: mkt.spread_usd,
            now_time: now,
            last_close_time: self.last_close_time[1],
            last_entry_time: self.last_entry_time[1],
        };
        if self.risk_manager.can_add_grid(&gate_ctx) {
            if let Some(add_decision) = self.sell_basket.check_add_grid(
                mkt,
                &self.config,
                &self.runtime,
                max_orders,
                step_points,
                0,
                now,
            ) {
                decision.sell_add_grid = Some(add_decision);
            }
        }

        // 步骤 5：检查首单入场（双向）。
        for (side_idx, side) in [Side::Buy, Side::Sell].iter().enumerate() {
            let basket = if *side == Side::Buy { &self.buy_basket } else { &self.sell_basket };
            // 仅当篮子为空时才考虑入场。
            if basket.stats().count > 0 {
                continue;
            }
            let gate_ctx = GateContext {
                is_locked: decision.is_locked,
                is_paused: false,
                cur_spread_usd: mkt.spread_usd,
                now_time: now,
                last_close_time: self.last_close_time[side_idx],
                last_entry_time: self.last_entry_time[side_idx],
            };
            if self.risk_manager.can_open_first_order(&gate_ctx) {
                let entry_lot = self.config.initial_lot;
                if *side == Side::Buy {
                    decision.buy_first_order = Some(entry_lot);
                } else {
                    decision.sell_first_order = Some(entry_lot);
                }
            }
        }

        decision
    }
}

/// tick 决策摘要（本 tick 要做什么）。
#[derive(Debug, Clone, Default)]
pub struct TickDecision {
    pub is_locked: bool,
    pub should_close_all: bool,
    pub lock_order: Option<(Decimal, bool)>, // (lock_lot, is_net_long)
    pub buy_tp: Option<crate::basket::TpAction>,
    pub sell_tp: Option<crate::basket::TpAction>,
    pub buy_add_grid: Option<crate::basket::AddGridDecision>,
    pub sell_add_grid: Option<crate::basket::AddGridDecision>,
    pub buy_first_order: Option<Decimal>,
    pub sell_first_order: Option<Decimal>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> Decimal {
        s.parse().unwrap()
    }

    fn mock_cfg() -> Config {
        Config::default()
    }

    fn mock_constants() -> StrategyConstants {
        StrategyConstants::default()
    }

    fn mock_runtime() -> RuntimeConstants {
        RuntimeConstants::gold_usd_standard()
    }

    fn mock_mkt() -> MarketData {
        MarketData { bid: d("3700"), ask: d("3700.5"), spread_usd: d("0.5") }
    }

    fn mock_snap(net_lots: &str, pnl: &str, count: usize) -> AccountSnapshot {
        AccountSnapshot {
            net_lots: d(net_lots),
            floating_pnl: d(pnl),
            position_count: count,
        }
    }

    #[test]
    fn app_new() {
        let app = GoldLegendApp::new(mock_cfg(), mock_constants(), mock_runtime(), 123, d("0.5"));
        assert_eq!(app.lock_manager.magic, 123);
        assert_eq!(app.buy_basket.side(), Side::Buy);
        assert_eq!(app.sell_basket.side(), Side::Sell);
    }

    #[test]
    fn on_tick_empty_positions_no_entry_locked() {
        let mut app =
            GoldLegendApp::new(mock_cfg(), mock_constants(), mock_runtime(), 123, d("0.5"));
        // 锁管理器在高亏损时立即触发，但持仓为 0 时直接进入 Locked（冻结）。
        let decision = app.on_tick(1000, &mock_mkt(), &[], &mock_snap("0", "-2000", 0));
        assert!(decision.is_locked);
        // 自动平仓仅在 2 小时后触发；本次是立即执行，因此 should_close_all=false。
        assert!(!decision.should_close_all);
        // 无需锁仓单（净敞口已中性）。
        assert!(decision.lock_order.is_none());
    }

    #[test]
    fn on_tick_empty_positions_allows_entry() {
        let mut app =
            GoldLegendApp::new(mock_cfg(), mock_constants(), mock_runtime(), 123, d("0.5"));
        // 正常状态，未锁仓。
        let mut rm = RiskManager::new(d("0.5"));
        rm.calendar_ready = true;
        rm.news_risk_cached = false;
        rm.load_sessions(&[]);
        app.risk_manager = rm;

        let decision = app.on_tick(1000, &mock_mkt(), &[], &mock_snap("0", "0", 0));
        assert!(!decision.is_locked);
        // 两侧篮子为空时，应分别生成买入与卖出首单。
        assert!(decision.buy_first_order.is_some());
        assert!(decision.sell_first_order.is_some());
    }
}
