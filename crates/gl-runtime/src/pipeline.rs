//! 策略流水线：主 tick 处理逻辑。

use crate::{
    basket::BasketStats,
    config::AccountConfig,
    ledger::{Ledger, PositionRole},
    market::MarketData,
    strategy::{can_open_first_order, GateContext, GateDecision},
};
use exness_api::{Position, PositionSide};
use gl_strategy::{GateResult, Gates, LockFsm, LockState, TpDecision, TpEngine};
use rust_decimal::Decimal;
use tracing::{debug, info};

pub struct StrategyPipeline;

impl StrategyPipeline {
    /// 处理 tick：检查闸门、信号、止盈、锁仓状态机。
    pub fn on_tick(
        config: &AccountConfig,
        ledger: &Ledger,
        market: &MarketData,
        lock_state: LockState,
        positions: &[Position],
        paused: bool,
    ) -> PipelineResult {
        let mut result = PipelineResult::default();

        let Some(bid) = market.bid() else {
            return result;
        };
        let Some(ask) = market.ask() else {
            return result;
        };

        let buy_stats = BasketStats::calculate_side(
            ledger,
            PositionRole::Grid,
            PositionSide::Buy,
            positions,
            bid,
            ask,
            config.constants.profit_per_dollar_per_lot,
            config.constants.pip,
        );

        let sell_stats = BasketStats::calculate_side(
            ledger,
            PositionRole::Grid,
            PositionSide::Sell,
            positions,
            bid,
            ask,
            config.constants.profit_per_dollar_per_lot,
            config.constants.pip,
        );

        // 1a. 检查买入篮子止盈
        if buy_stats.count > 0 {
            let required_profit = TpEngine::parabolic_required_profit(
                buy_stats.count,
                config.params.max_orders,
                config.params.initial_lot,
                config.constants.profit_per_dollar_per_lot,
                config.params.tp_distance_usd,
                config.constants.parabola_k,
                config.constants.min_tp,
            );

            let tp_decision = TpEngine::decide(
                buy_stats.profit,
                required_profit,
                buy_stats.total_lots,
                config.constants.profit_per_dollar_per_lot,
                config.constants.pip,
                config.constants.trailing_cushion_points,
                config.constants.trailing_step_points,
            );

            match tp_decision {
                TpDecision::CloseNow => {
                    info!("TP BUY: Close buy basket now (reached 100% target: {} >= {})", buy_stats.profit, required_profit);
                    result.should_close_buy = true;
                    result.should_close_grid = true;
                }
                TpDecision::Trail { trigger_profit } => {
                    debug!("TP BUY: Trailing activated, trigger={}", trigger_profit);
                    result.trailing_trigger_buy = Some(trigger_profit);
                }
                TpDecision::Hold => {}
            }
        }

        // 1b. 检查卖出篮子止盈
        if sell_stats.count > 0 {
            let required_profit = TpEngine::parabolic_required_profit(
                sell_stats.count,
                config.params.max_orders,
                config.params.initial_lot,
                config.constants.profit_per_dollar_per_lot,
                config.params.tp_distance_usd,
                config.constants.parabola_k,
                config.constants.min_tp,
            );

            let tp_decision = TpEngine::decide(
                sell_stats.profit,
                required_profit,
                sell_stats.total_lots,
                config.constants.profit_per_dollar_per_lot,
                config.constants.pip,
                config.constants.trailing_cushion_points,
                config.constants.trailing_step_points,
            );

            match tp_decision {
                TpDecision::CloseNow => {
                    info!("TP SELL: Close sell basket now (reached 100% target: {} >= {})", sell_stats.profit, required_profit);
                    result.should_close_sell = true;
                    result.should_close_grid = true;
                }
                TpDecision::Trail { trigger_profit } => {
                    debug!("TP SELL: Trailing activated, trigger={}", trigger_profit);
                    result.trailing_trigger_sell = Some(trigger_profit);
                }
                TpDecision::Hold => {}
            }
        }

        // 2. 锁仓状态机 tick
        let total_grid_profit = buy_stats.profit + sell_stats.profit;
        let total_grid_count = buy_stats.count + sell_stats.count;
        
        let new_lock_state = LockFsm::tick(
            lock_state,
            total_grid_profit,
            -config.params.lock_trigger_usd,
            total_grid_count,
        );

        if new_lock_state != lock_state {
            info!("Lock FSM: {:?} → {:?}", lock_state, new_lock_state);
            result.new_lock_state = Some(new_lock_state);

            if matches!(new_lock_state, LockState::Locked) {
                result.should_open_lock = true;
            }
        }

        // 3. 检查是否可加网格并开首单（闸门通过且未锁仓）
        if !paused && !matches!(lock_state, LockState::Locked) && !result.should_open_lock {
            // 检查买入篮子加网格
            if !result.should_close_buy && buy_stats.count > 0 {
                let can_add = Self::check_gates_for_add(config, bid, ask, lock_state, buy_stats.count);
                if can_add {
                    let count = buy_stats.count;
                    let last_lot = ledger.grid_positions()
                        .filter(|entry| entry.side == PositionSide::Buy)
                        .map(|entry| entry.volume)
                        .max()
                        .unwrap_or(config.params.initial_lot);
                    let next_lot = gl_strategy::grid_math::calc_next_lot(
                        config.params.initial_lot,
                        config.params.lot_mult,
                        count,
                        last_lot,
                        &config.constants.lot_spec,
                    );
                    if !Self::risk_budget_allows_add(config, PositionSide::Buy, &buy_stats, &sell_stats, next_lot) {
                        debug!(side = "BUY", "Grid add blocked by all-direction floating-loss budget");
                    } else {
                    result.can_add_grid = true;

                    // Calculate step in points (递增步长)
                    let step_points = Decimal::from(config.params.grid_step)
                        + Decimal::from(config.params.grid_step_add) * Decimal::from(count.saturating_sub(1));

                    // 获取最低买入价
                    let lowest_buy = ledger
                        .grid_positions()
                        .filter(|e| e.side == PositionSide::Buy)
                        .map(|e| e.open_price)
                        .min();

                    if let Some(lowest) = lowest_buy {
                        // BUY: 距离 = (最低价 - Ask) / pip, 下跌才加仓
                        let distance_points = (lowest - ask) / config.constants.pip;
                        if distance_points >= step_points {
                            result.should_add_grid = Some((PositionSide::Buy, next_lot));
                        }
                    }
                    }
                }
            }

            // 检查卖出篮子加网格
            if !result.should_close_sell && sell_stats.count > 0 {
                let can_add = Self::check_gates_for_add(config, bid, ask, lock_state, sell_stats.count);
                if can_add {
                    let count = sell_stats.count;
                    let last_lot = ledger.grid_positions()
                        .filter(|entry| entry.side == PositionSide::Sell)
                        .map(|entry| entry.volume)
                        .max()
                        .unwrap_or(config.params.initial_lot);
                    let next_lot = gl_strategy::grid_math::calc_next_lot(
                        config.params.initial_lot,
                        config.params.lot_mult,
                        count,
                        last_lot,
                        &config.constants.lot_spec,
                    );
                    if !Self::risk_budget_allows_add(config, PositionSide::Sell, &buy_stats, &sell_stats, next_lot) {
                        debug!(side = "SELL", "Grid add blocked by all-direction floating-loss budget");
                    } else {
                    result.can_add_grid = true;

                    let step_points = Decimal::from(config.params.grid_step)
                        + Decimal::from(config.params.grid_step_add) * Decimal::from(count.saturating_sub(1));

                    let highest_sell = ledger
                        .grid_positions()
                        .filter(|e| e.side == PositionSide::Sell)
                        .map(|e| e.open_price)
                        .max();

                    if let Some(highest) = highest_sell {
                        // SELL: 距离 = (Bid - 最高价) / pip, 上涨才加仓
                        let distance_points = (bid - highest) / config.constants.pip;
                        if distance_points >= step_points && result.should_add_grid.is_none() {
                            result.should_add_grid = Some((PositionSide::Sell, next_lot));
                        }
                    }
                    }
                }
            }

            // 检查首单
            let can_open_first = Self::check_gates_for_first(config, market, bid, ask, lock_state);
            if can_open_first {
                if config.params.enable_two_way_hedging {
                    // 双向独立对冲模式：任一篮子为空就开首单
                    if buy_stats.count == 0 {
                        result.should_open_first_buy = true;
                    }
                    if sell_stats.count == 0 {
                        result.should_open_first_sell = true;
                    }
                } else {
                    // 单向网格模式：只在完全空仓时开买入首单
                    if buy_stats.count == 0 && sell_stats.count == 0 {
                        result.should_open_first_buy = true;
                    }
                }
            }
        }

        result
    }

    fn check_gates_for_first(
        config: &AccountConfig,
        market: &MarketData,
        bid: Decimal,
        ask: Decimal,
        lock_state: LockState,
    ) -> bool {
        // 锁仓闸门
        if matches!(lock_state, LockState::Locked) {
            return false;
        }

        // 点差闸门
        let spread_result = Gates::check_spread(
            bid,
            ask,
            config.params.initial_lot,
            config.constants.profit_per_dollar_per_lot,
            config.constants.max_spread_usd,
        );
        if spread_result != GateResult::Pass {
            return false;
        }

        let tick_time = market
            .latest_tick
            .as_ref()
            .and_then(|t| {
                if t.time > 0 {
                    chrono::DateTime::from_timestamp(t.time, 0)
                } else {
                    None
                }
            })
            .unwrap_or_else(chrono::Utc::now);

        // 时段闸门（仅针对首单）
        let ctx = GateContext {
            now: tick_time,
            is_locked: matches!(lock_state, LockState::Locked),
            is_paused: false,
            cur_spread_usd: (ask - bid) * config.params.initial_lot
                * config.constants.profit_per_dollar_per_lot,
            last_close_time: None, // TODO：从 account_runner 跟踪
            last_entry_time: None,
            next_news_event_time: market.next_news_event_time,
            news_calendar_ready: market.news_calendar_ready,
            instrument_condition: market.instrument_condition.clone(),
        };

        let news_pre_min = ((config.constants.news_avoid_before_sec + 59) / 60) as i32;
        let news_post_min = ((config.constants.news_avoid_after_sec + 59) / 60) as i32;

        match can_open_first_order(
            &ctx,
            config.constants.max_spread_usd,
            news_pre_min,
            news_post_min,
        ) {
            GateDecision::Block { reason } => {
                debug!("Gate blocked first order: {}", reason);
                return false;
            }
            GateDecision::Allow => {}
        }

        true
    }

    fn check_gates_for_add(
        config: &AccountConfig,
        bid: Decimal,
        ask: Decimal,
        lock_state: LockState,
        basket_count: usize,
    ) -> bool {
        if basket_count >= config.params.max_orders {
            return false;
        }

        // 锁仓闸门
        if matches!(lock_state, LockState::Locked) {
            return false;
        }

        // 点差闸门
        let spread_result = Gates::check_spread(
            bid,
            ask,
            config.params.initial_lot,
            config.constants.profit_per_dollar_per_lot,
            config.constants.max_spread_usd,
        );
        if spread_result != GateResult::Pass {
            return false;
        }

        // No session gate for add grid (加仓不受时段限制)

        true
    }

    /// Stress both directional baskets before adding. The adverse move is the
    /// MT5 AutoMaxOrders reserve after the candidate layer: base_step + n*step_add.
    fn risk_budget_allows_add(
        config: &AccountConfig,
        side: PositionSide,
        buy: &BasketStats,
        sell: &BasketStats,
        next_lot: Decimal,
    ) -> bool {
        let next_count = match side {
            PositionSide::Buy => buy.count + 1,
            PositionSide::Sell => sell.count + 1,
        };
        let cushion_points = Decimal::from(config.params.grid_step)
            + Decimal::from(config.params.grid_step_add) * Decimal::from(next_count);
        let adverse_move = cushion_points * config.constants.pip;
        let move_profit = config.constants.profit_per_dollar_per_lot * adverse_move;

        let (buy_future, sell_future) = match side {
            PositionSide::Buy => (
                buy.profit - (buy.total_lots + next_lot) * move_profit,
                sell.profit + sell.total_lots * move_profit,
            ),
            PositionSide::Sell => (
                buy.profit + buy.total_lots * move_profit,
                sell.profit - (sell.total_lots + next_lot) * move_profit,
            ),
        };
        let projected_all_direction_loss = buy_future.min(Decimal::ZERO).abs()
            + sell_future.min(Decimal::ZERO).abs();
        projected_all_direction_loss <= config.params.lock_trigger_usd
    }

}

#[derive(Debug, Default)]
pub struct PipelineResult {
    pub should_close_grid: bool,
    pub should_close_buy: bool,
    pub should_close_sell: bool,
    pub should_open_lock: bool,
    pub should_open_first_buy: bool,
    pub should_open_first_sell: bool,
    pub should_add_grid: Option<(PositionSide, Decimal)>,
    pub can_add_grid: bool,
    pub trailing_trigger_buy: Option<Decimal>,
    pub trailing_trigger_sell: Option<Decimal>,
    pub new_lock_state: Option<LockState>,
}
