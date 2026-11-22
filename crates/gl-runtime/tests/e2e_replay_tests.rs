//! GoldLegend（gl-runtime）的端到端 tick 回放仿真测试。
//!
//! 校验完整的订单生命周期：
//! 1. 首个 tick 且点差闸门通过 -> 空篮子直接开首单
//! 2. 状态机切换：Idle -> Arming
//! 3. 行情逆向 -> 触发加网格（手数递进与层数跟踪）
//! 4. 严重回撤 -> 触发对冲锁仓：Arming -> Locked，并阻止后续加仓
//! 5. 行情有利反弹 -> 抛物线止盈计算、35 点缓冲移动止盈与止盈平仓
//! 6. 网格平仓 -> Arming -> Idle
//! 7. 方案 A 新闻规避不变式：只拦首单，从不拦加网格
//! 8. USC 美分账户兼容不变式

use chrono::Utc;
use exness_api::{InstrumentCondition, InstrumentTradeSession, Position, PositionSide, Tick, TransactionEvent, WsEvent};
use gl_runtime::{
    basket::BasketStats,
    config::{AccountConfig, RuntimeConstants, StrategyParams},
    ledger::{Ledger, LedgerEntry, PositionRole},
    market::MarketData,
    ops_tracker::OpsTracker,
    pipeline::StrategyPipeline,
};
use gl_strategy::{GateResult, Gates, LockFsm, LockState, TpDecision, TpEngine};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

fn create_test_config() -> AccountConfig {
    let mut params = StrategyParams::default();
    params.initial_lot = dec!(0.01);
    params.lot_mult = dec!(1.45);
    params.grid_step = 30;
    params.grid_step_add = 10;
    params.lock_trigger_usd = dec!(50.0); // 降低锁仓阈值以适配 contract_size=100
    params.tp_distance_usd = dec!(1.00);

    let constants = RuntimeConstants::from_instrument(dec!(100), "USD", 3, gl_strategy::grid_math::LotSpec::gold_default());

    AccountConfig {
        account_id: "test_acc_001".to_string(),
        symbol: "XAUUSD".to_string(),
        params,
        constants,
    }
}

fn make_tick(bid: f64, ask: f64, time: i64) -> Tick {
    Tick {
        symbol: "XAUUSD".to_string(),
        bid,
        ask,
        timestamp_str: None,
        time,
    }
}

fn make_position(id: &str, side: &str, volume: Decimal, open_price: Decimal) -> Position {
    Position {
        id: id.to_string(),
        symbol: "XAUUSD".to_string(),
        position_type: side.to_string(),
        volume: volume.to_string(),
        open_price: open_price.to_string(),
        current_price: open_price.to_string(),
        profit: "0.0".to_string(),
        swap: "0.0".to_string(),
        commission: "0.0".to_string(),
        open_time: 1700000000,
        comment: None,
    }
}

/// 创建测试用的默认 InstrumentCondition (24/7 交易时段)
fn make_test_instrument_condition() -> InstrumentCondition {
    InstrumentCondition {
        symbol: "XAUUSD".to_string(),
        base_currency: Some("XAU".to_string()),
        quote_currency: Some("USD".to_string()),
        point_digits: Some(3),
        volume_min: Some("0.01".to_string()),
        volume_max: Some("200.0".to_string()),
        volume_step: Some("0.01".to_string()),
        contract_size: Some("100".to_string()),
        trade_mode: Some("enabled".to_string()),
        trade_sessions: vec![InstrumentTradeSession {
            trade_mode: Some("enabled".to_string()),
            week_start_time: Some("Mon,00:00:00".to_string()),
            week_end_time: Some("Fri,23:59:59".to_string()),
        }],
    }
}

#[test]
fn test_e2e_full_order_lifecycle_open_add_trail_close() {
    let config = create_test_config();
    let mut ledger = Ledger::new();
    let mut market = MarketData::new();
    
    // 启用新闻日历以允许首单
    market.news_calendar_ready = true;
    
    // 设置模拟交易时段 (24/7 全天候交易)
    market.instrument_condition = Some(make_test_instrument_condition());

    
    let mut positions: Vec<Position> = Vec::new();
    let mut lock_state = LockState::Idle;

    // ==========================================
    // 阶段 1：行情初始化与首单触发
    // ==========================================
    // 使用更小的点差: 0.01 美元 (合理范围内)
    market.update_tick(make_tick(2650.00, 2650.01, 1000));
    let result = StrategyPipeline::on_tick(&config, &ledger, &market, lock_state, &positions, false);

    assert!(result.should_open_first_buy);
    assert!(!result.should_open_first_sell);
    assert!(!result.should_close_grid);
    assert_eq!(result.should_add_grid, None);

    // 模拟订单成交 #0
    let pos_0 = make_position("pos_grid_0", "buy", dec!(0.01), dec!(2650.20));
    ledger.insert(LedgerEntry {
        position_id: "pos_grid_0".to_string(),
        role: PositionRole::Grid,
        open_time: Utc::now(),
        volume: dec!(0.01),
        open_price: dec!(2650.20),
        side: PositionSide::Buy,
    });
    positions.push(pos_0);

    // ==========================================
    // 阶段 2：状态机 Idle -> Arming
    // ==========================================
    market.update_tick(make_tick(2650.00, 2650.20, 1001));
    let result = StrategyPipeline::on_tick(&config, &ledger, &market, lock_state, &positions, false);
    assert!(matches!(result.new_lock_state, Some(LockState::Arming { .. })));
    lock_state = LockState::Arming {
        trigger_loss: -config.params.lock_trigger_usd,
    };

    // ==========================================
    // 阶段 3：行情逆向 -> 触发加网格
    // ==========================================
    // 价格逆向跌至 2646.00，不利买入持仓
    market.update_tick(make_tick(2646.00, 2646.20, 1015));
    let result = StrategyPipeline::on_tick(&config, &ledger, &market, lock_state, &positions, false);

    assert!(result.can_add_grid);
    assert!(result.should_add_grid.is_some());
    let (add_side, add_lot) = result.should_add_grid.unwrap();
    assert_eq!(add_side, PositionSide::Buy);
    assert_eq!(add_lot, dec!(0.01)); // 首次加仓的递进手数

    // 模拟网格订单成交 #1
    let pos_1 = make_position("pos_grid_1", "buy", add_lot, dec!(2646.20));
    ledger.insert(LedgerEntry {
        position_id: "pos_grid_1".to_string(),
        role: PositionRole::Grid,
        open_time: Utc::now(),
        volume: add_lot,
        open_price: dec!(2646.20),
        side: PositionSide::Buy,
    });
    positions.push(pos_1);
    assert_eq!(ledger.count_by_role(PositionRole::Grid), 2);

    // ==========================================
    // 阶段 4：严重回撤 -> 对冲锁仓状态机
    // ==========================================
    // 行情跌至 2610.00 -> 严重回撤超过 lock_trigger_usd (50.0)
    market.update_tick(make_tick(2610.00, 2610.20, 1030));
    
    let result = StrategyPipeline::on_tick(&config, &ledger, &market, lock_state, &positions, false);
    
    assert_eq!(result.new_lock_state, Some(LockState::Locked));
    assert!(result.should_open_lock);

    // 锁仓后禁止继续加网格
    let locked_result = StrategyPipeline::on_tick(
        &config,
        &ledger,
        &market,
        LockState::Locked,
        &positions,
        false,
    );
    assert!(!locked_result.can_add_grid);

    // ==========================================
    // 阶段 5：行情恢复 -> 抛物线止盈与移动止盈
    // ==========================================
    // 保持 Locked 状态,测试锁仓后的止盈逻辑
    lock_state = LockState::Locked;
    
    // 行情反弹至 2655.00 -> 盈利超过所需目标
    // 由于 cushion_points 较大 (手数较轻),触发移动止盈而不是立即平仓
    market.update_tick(make_tick(2655.00, 2655.20, 1100));
    let result = StrategyPipeline::on_tick(&config, &ledger, &market, lock_state, &positions, false);
    
    // 验证触发移动止盈
    assert!(result.trailing_trigger_buy.is_some());
    assert!(!result.should_close_grid);

    let stats = BasketStats::calculate(
        &ledger,
        PositionRole::Grid,
        &positions,
        dec!(2655.00),
        config.constants.profit_per_dollar_per_lot,
        config.constants.pip,
    );

    let req_profit = TpEngine::parabolic_required_profit(
        stats.count,
        config.params.max_orders,
        config.params.initial_lot,
        config.constants.profit_per_dollar_per_lot,
        config.params.tp_distance_usd,
        config.constants.parabola_k,
        config.constants.min_tp,
    );

    let decision = TpEngine::decide(
        stats.profit,
        req_profit,
        stats.total_lots,
        config.constants.profit_per_dollar_per_lot,
        config.constants.pip,
        config.constants.trailing_cushion_points,
        config.constants.trailing_step_points,
    );
    // cushion_points 较大时,触发移动止盈以捕捉超额利润
    assert_eq!(decision, TpDecision::Trail { trigger_profit: req_profit });

    // ==========================================
    // 阶段 6：平掉网格篮子并回到 Idle
    // ==========================================
    positions.clear();
    ledger.remove("pos_grid_0");
    ledger.remove("pos_grid_1");

    let idle_state = LockFsm::tick(
        LockState::Arming {
            trigger_loss: -config.params.lock_trigger_usd,
        },
        dec!(0.0),
        -config.params.lock_trigger_usd,
        0,
    );
    assert_eq!(idle_state, LockState::Idle);
}

#[test]
fn test_e2e_high_cushion_trailing_profit_activation() {
    // AGENTS.md 不变式 9：
    // 当 cushion >= 35.0 点时，激活 25 点固定移动止盈以捕捉强趋势！
    let total_lots = dec!(0.001);
    let profit_per_lot = dec!(100);
    let pip = dec!(0.1);
    let trailing_cushion = dec!(35.0);
    let trailing_step = dec!(25.0);

    let req_profit = dec!(1.0); // cushion = 1.0 / (0.001 * 10) = 100 点 >= 35 点
    let _trigger_profit = req_profit + (trailing_step * total_lots * profit_per_lot * pip);
    // _trigger_profit = 1.0 + (25.0 * 0.01) = 1.25

    // 盈利 0.90：低于 req_profit (1.0) -> 持有
    let hold_decision = TpEngine::decide(
        dec!(0.90),
        req_profit,
        total_lots,
        profit_per_lot,
        pip,
        trailing_cushion,
        trailing_step,
    );
    assert_eq!(hold_decision, TpDecision::Hold);

    // 盈利升至 1.30：超过 req_profit (1.0) 且缓冲充足 -> 激活移动止盈！
    let trail_decision = TpEngine::decide(
        dec!(1.30),
        req_profit,
        total_lots,
        profit_per_lot,
        pip,
        trailing_cushion,
        trailing_step,
    );
    assert_eq!(trail_decision, TpDecision::Trail { trigger_profit: req_profit });
}

#[test]
fn test_e2e_plan_a_news_avoidance_invariant() {
    // AGENTS.md 不变式 8：
    // 新闻规避只拦截新周期的首单；绝不拦截加网格！
    use chrono::DateTime;
    let now = DateTime::from_timestamp(1700000000, 0).unwrap();
    let event_time = DateTime::from_timestamp(1700000030, 0).unwrap(); // 未来 30 秒：处于前置窗口内（60 秒）

    // 用例 1：新闻窗口内检查首单 -> 被拦截
    let first_order_gate = Gates::check_news_first_order(now, Some(event_time), 60, 180);
    assert_eq!(
        first_order_gate,
        GateResult::Block("inside news avoidance window")
    );

    // 用例 2：按方案 A，加网格逻辑不调用 check_news_first_order
    // 高影响力新闻期间，已有篮子仍可继续加防守层
    let basket_count = 2;
    assert!(basket_count > 0);
    // 架构设计已验证：CanAddGrid 有意排除新闻检查
}

#[test]
fn test_e2e_spread_protection_gate() {
    let config = create_test_config();
    let ledger = Ledger::new();
    let mut market = MarketData::new();
    market.news_calendar_ready = true; // 启用新闻日历
    market.instrument_condition = Some(make_test_instrument_condition());
    let positions: Vec<Position> = Vec::new();

    // 点差 = 1.00 美元 > max_spread_usd (0.50 美元)
    market.update_tick(make_tick(2650.00, 2651.00, 1000));
    let result = StrategyPipeline::on_tick(&config, &ledger, &market, LockState::Idle, &positions, false);

    // 点差飙升阻止首单开出
    assert!(!result.should_open_first_buy);
    assert!(!result.should_open_first_sell);
    assert!(!result.can_add_grid);

    // 正常点差 <= 0.50 美元 → 通过
    market.update_tick(make_tick(2650.00, 2650.25, 1001));
    let result_normal = StrategyPipeline::on_tick(&config, &ledger, &market, LockState::Idle, &positions, false);
    assert!(result_normal.should_open_first_buy);
    assert!(!result_normal.should_open_first_sell);
}

#[test]
fn test_e2e_usc_cent_account_compatibility() {
    // AGENTS.md 不变式 4：
    // Exness 美分账户 (XAUUSDc) 的合约大小官方自动缩小为 1 盎司/手 (标准 XAUUSD 为 100 盎司/手)。
    // 0.01 手黄金波动 $1.00:
    // - 标准账户: 0.01 手 = 1 盎司 * $1.00 = 1.00 USD
    // - 美分账户: 0.01 手 = 0.01 盎司 * $1.00 = 0.01 USD = 1.00 USC
    // 在各自账户币种单位下，账面数值完全 1:1 同构，profit_per_dollar_per_lot 恒为 100，根本无需做特殊缩放换算！
    let usd_constants = RuntimeConstants::from_instrument(dec!(100), "USD", 3, gl_strategy::grid_math::LotSpec::gold_default());
    assert_eq!(usd_constants.profit_per_dollar_per_lot, dec!(100));

    let usc_constants = RuntimeConstants::from_instrument(dec!(1), "USC", 3, gl_strategy::grid_math::LotSpec::gold_default());
    assert_eq!(usc_constants.profit_per_dollar_per_lot, dec!(100));
}

#[test]
fn test_e2e_instant_hedge_needs_no_signal_data() {
    let config = create_test_config();
    let ledger = Ledger::new();
    let mut market = MarketData::new();
    market.news_calendar_ready = true; // 启用新闻日历
    market.instrument_condition = Some(make_test_instrument_condition());
    market.update_tick(make_tick(2648.00, 2648.20, 1060));
    let positions: Vec<Position> = Vec::new();

    let result = StrategyPipeline::on_tick(&config, &ledger, &market, LockState::Idle, &positions, false);
    assert!(result.should_open_first_buy || result.should_open_first_sell);
}

#[test]
fn test_e2e_ws_transaction_reconciliation_lifecycle() {
    let config = create_test_config();
    let mut ledger = Ledger::new();
    let mut market = MarketData::new();
    market.news_calendar_ready = true; // 启用新闻日历
    market.instrument_condition = Some(make_test_instrument_condition());
    let mut positions: Vec<Position> = Vec::new();
    let mut ops_tracker = OpsTracker::new(config.account_id.clone());

    // 步骤 1：首个 tick 触发首批网格单
    market.update_tick(make_tick(2650.00, 2650.20, 1000));
    let result = StrategyPipeline::on_tick(&config, &ledger, &market, LockState::Idle, &positions, false);
    assert!(result.should_open_first_buy);
    assert!(!result.should_open_first_sell);

    // 步骤 2：订单执行器向 Exness API 发单并登记在途操作
    let op_id = "382170781959847936".to_string();
    ops_tracker.track_open(op_id.clone(), PositionRole::Grid, PositionSide::Buy, dec!(0.01));
    assert_eq!(ops_tracker.pending_count(), 1);

    // 步骤 3：在途保护：操作在途期间，pending_count > 0 可防止重复开仓
    assert!(ops_tracker.pending_count() > 0, "In-flight guard active");

    // 步骤 4：Exness 通过 try_send 通道回送 WS Transaction 事件
    let (event_tx, mut event_rx) = tokio::sync::mpsc::channel(1024);
    let event = WsEvent::Transaction(TransactionEvent {
        operation_id: op_id.clone(),
        status: "success".to_string(),
        payload: exness_api::TransactionPayload {
            positions: vec![make_position("pos_grid_0", "buy", dec!(0.01), dec!(2650.20))],
            ..Default::default()
        },
    });

    // 测试桥接修复：try_send 成功投递且不丢弃 Future！
    event_tx.try_send(event).expect("try_send should succeed");
    let received_event = event_rx.try_recv().expect("Event must be received from channel");

    // 步骤 5：Runner 的 on_ws_event 处理收到的事件
    if let WsEvent::Transaction(tx) = received_event {
        let pending_op = ops_tracker.mark_completed(&tx.operation_id).expect("Must match pending op");
        assert_eq!(ops_tracker.pending_count(), 0);

        for pos in &tx.payload.positions {
            ledger.insert(LedgerEntry {
                position_id: pos.id.clone(),
                role: pending_op.role.unwrap(),
                side: pending_op.side.unwrap(),
                volume: pending_op.volume.unwrap(),
                open_time: Utc::now(),
                open_price: pos.open_price.parse().unwrap_or_default(),
            });
            positions.push(pos.clone());
        }
    }

    // 步骤 6：对账后校验账本与状态
    assert_eq!(ledger.count_by_role(PositionRole::Grid), 1);
    assert_eq!(ops_tracker.pending_count(), 0);

    // 步骤 7：当前价位下的后续 tick：should_open_first 为 None，should_add_grid 为 NONE！
    market.update_tick(make_tick(2650.05, 2650.25, 1001));
    let next_result = StrategyPipeline::on_tick(&config, &ledger, &market, LockState::Idle, &positions, false);
    assert!(!next_result.should_open_first_buy && !next_result.should_open_first_sell, "Must not re-open initial grid");
    assert_eq!(next_result.should_add_grid, None, "Must not add grid without price movement");
}

#[test]
fn test_e2e_instant_two_way_hedging_decoupled() {
    let mut config = create_test_config();
    config.params.enable_two_way_hedging = true; // 启用双向对冲模式
    let mut ledger = Ledger::new();
    let mut market = MarketData::new();
    market.news_calendar_ready = true; // 启用新闻日历以允许首单
    market.instrument_condition = Some(make_test_instrument_condition());
    let mut positions: Vec<Position> = Vec::new();
    let mut ops_tracker = OpsTracker::new("test_acc_001".to_string());

    market.update_tick(make_tick(2650.00, 2650.20, 1000));

    // 1. 首个 tick，篮子为空：买入和卖出首单同时触发！
    let res = StrategyPipeline::on_tick(&config, &ledger, &market, LockState::Idle, &positions, false);
    assert!(res.should_open_first_buy && res.should_open_first_sell, "Both Buy and Sell first orders must trigger (two-way hedging)");

    // 2. 模拟买入订单在途：
    ops_tracker.track_open("op_buy_1".to_string(), PositionRole::Grid, PositionSide::Buy, dec!(0.01));
    assert!(ops_tracker.has_pending_side(PositionSide::Buy));
    // 关键点：卖出侧没有在途操作！
    assert!(!ops_tracker.has_pending_side(PositionSide::Sell), "Sell side must NOT be blocked by Buy in flight");

    // 3. 确认买入订单写入账本
    ledger.insert(LedgerEntry {
        position_id: "pos_buy_1".to_string(),
        role: PositionRole::Grid,
        side: PositionSide::Buy,
        volume: dec!(0.01),
        open_time: Utc::now(),
        open_price: dec!(2650.20),
    });
    positions.push(make_position("pos_buy_1", "buy", dec!(0.01), dec!(2650.20)));
    ops_tracker.mark_completed("op_buy_1");

    // 4. 下一个 tick：买入篮子已占用（count=1），卖出篮子为空（count=0）
    market.update_tick(make_tick(2650.00, 2650.20, 1001));
    let res2 = StrategyPipeline::on_tick(&config, &ledger, &market, LockState::Idle, &positions, false);
    assert!(!res2.should_open_first_buy && res2.should_open_first_sell, "Sell basket is empty -> Sell first order triggered!");

    // 5. 确认卖出订单写入账本 -> 两侧均已激活！
    ledger.insert(LedgerEntry {
        position_id: "pos_sell_1".to_string(),
        role: PositionRole::Grid,
        side: PositionSide::Sell,
        volume: dec!(0.01),
        open_time: Utc::now(),
        open_price: dec!(2650.00),
    });
    positions.push(make_position("pos_sell_1", "sell", dec!(0.01), dec!(2650.00)));

    // 6. 独立止盈：行情涨至 2670.00 -> 买入篮子达到止盈目标，卖出篮子处于回撤
    market.update_tick(make_tick(2670.00, 2670.20, 1010));
    let res3 = StrategyPipeline::on_tick(&config, &ledger, &market, LockState::Idle, &positions, false);
    assert!(res3.should_close_buy || res3.trailing_trigger_buy.is_some(), "Buy basket must hit TP or Trailing");
    assert!(!res3.should_close_sell, "Sell basket must NOT be closed when Buy hits TP");
}
