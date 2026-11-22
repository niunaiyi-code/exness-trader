//! 篮子统计计算器（EA GridBasket.mqh 移植）。

use crate::config::{Config, RuntimeConstants, StrategyConstants};
use crate::types::Side;
use rust_decimal::Decimal;

#[derive(Debug, Clone, Default)]
pub struct BasketStats {
    pub count: usize,
    pub total_lots: Decimal,
    pub profit: Decimal,
    pub weighted_avg_price: Decimal,
    pub side: Option<Side>,
}

/// 市场数据快照（用于策略决策）
#[derive(Debug, Clone)]
pub struct MarketData {
    pub bid: Decimal,
    pub ask: Decimal,
    pub spread_usd: Decimal,
}

/// 持仓视图（用于策略计算）
#[derive(Debug, Clone)]
pub struct PositionView {
    pub position_id: String,
    pub volume: Decimal,
    pub open_price: Decimal,
    pub current_price: Decimal,
    pub profit: Decimal,
    pub side: Side,
}

impl BasketStats {
    /// 根据持仓列表计算篮子统计。
    /// 假设所有持仓为同一品种、同一方向（网格同质性）。
    pub fn calculate<P>(positions: &[P]) -> Self
    where
        P: Position,
    {
        if positions.is_empty() {
            return Self::default();
        }

        let mut total_lots = Decimal::ZERO;
        let mut profit = Decimal::ZERO;
        let mut weighted_price_sum = Decimal::ZERO;
        let side = positions[0].side();

        for pos in positions {
            let lot = pos.volume();
            total_lots += lot;
            profit += pos.profit();
            weighted_price_sum += pos.open_price() * lot;
        }

        let weighted_avg_price = if total_lots > Decimal::ZERO {
            weighted_price_sum / total_lots
        } else {
            Decimal::ZERO
        };

        Self {
            count: positions.len(),
            total_lots,
            profit,
            weighted_avg_price,
            side: Some(side),
        }
    }
}

/// 用于从类持仓类型提取网格统计的 trait。
pub trait Position {
    fn volume(&self) -> Decimal;
    fn open_price(&self) -> Decimal;
    fn profit(&self) -> Decimal;
    fn side(&self) -> Side;
}

/// 为 PositionView 实现 Position trait
impl Position for PositionView {
    fn volume(&self) -> Decimal {
        self.volume
    }

    fn open_price(&self) -> Decimal {
        self.open_price
    }

    fn profit(&self) -> Decimal {
        self.profit
    }

    fn side(&self) -> Side {
        self.side
    }
}

/// 止盈动作决策
#[derive(Debug, Clone)]
pub enum TpAction {
    /// 立即平仓，目标已达
    CloseNow,
    /// 等待时机
    Wait,
}

/// 加网格决策
#[derive(Debug, Clone)]
pub struct AddGridDecision {
    pub lot_size: Decimal,
    pub should_add: bool,
}

/// 篮子管理器（包含统计和决策逻辑）
#[derive(Debug, Clone)]
pub struct Basket {
    side: Side,
    stats: BasketStats,
}

impl Basket {
    pub fn new(side: Side) -> Self {
        Self {
            side,
            stats: BasketStats::default(),
        }
    }

    pub fn side(&self) -> Side {
        self.side
    }

    pub fn stats(&self) -> &BasketStats {
        &self.stats
    }

    /// 重置统计（tick 开始时调用）
    pub fn reset_stats(&mut self) {
        self.stats = BasketStats::default();
    }

    /// 累积单个持仓到统计中
    pub fn accumulate<P: Position>(&mut self, pos: &P) {
        // 只累积与当前篮子方向匹配的持仓
        if pos.side() != self.side {
            return;
        }

        self.stats.count += 1;
        self.stats.total_lots += pos.volume();
        self.stats.profit += pos.profit();
        self.stats.weighted_avg_price += pos.open_price() * pos.volume();
    }

    /// 完成统计（计算加权均价）
    pub fn finalize(&mut self) {
        if self.stats.total_lots > Decimal::ZERO {
            self.stats.weighted_avg_price /= self.stats.total_lots;
            self.stats.side = Some(self.side);
        }
    }

    /// 检查止盈条件
    /// TODO: 完整实现止盈逻辑
    pub fn check_take_profit(
        &self,
        _mkt: &MarketData,
        _config: &Config,
        _constants: &StrategyConstants,
        _runtime: &RuntimeConstants,
        _max_orders: usize,
    ) -> Option<TpAction> {
        // TODO: 实现止盈检查逻辑
        // 当前返回 None 表示不执行止盈
        None
    }

    /// 检查是否需要加网格
    /// TODO: 完整实现加网格逻辑
    pub fn check_add_grid(
        &self,
        _mkt: &MarketData,
        _config: &Config,
        _runtime: &RuntimeConstants,
        _max_orders: usize,
        _step_points: Decimal,
        _now_ms: u64,
        _now: i64,
    ) -> Option<AddGridDecision> {
        // TODO: 实现加网格检查逻辑
        // 当前返回 None 表示不添加网格
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    struct TestPosition {
        vol: Decimal,
        open: Decimal,
        pnl: Decimal,
        side: Side,
    }

    impl Position for TestPosition {
        fn volume(&self) -> Decimal { self.vol }
        fn open_price(&self) -> Decimal { self.open }
        fn profit(&self) -> Decimal { self.pnl }
        fn side(&self) -> Side { self.side }
    }

    #[test]
    fn empty_basket() {
        let stats = BasketStats::calculate::<TestPosition>(&[]);
        assert_eq!(stats.count, 0);
        assert_eq!(stats.total_lots, Decimal::ZERO);
    }

    #[test]
    fn basket_3_longs() {
        let positions = vec![
            TestPosition { vol: dec!(0.01), open: dec!(2000), pnl: dec!(-10), side: Side::Buy },
            TestPosition { vol: dec!(0.02), open: dec!(1970), pnl: dec!(-15), side: Side::Buy },
            TestPosition { vol: dec!(0.03), open: dec!(1940), pnl: dec!(-20), side: Side::Buy },
        ];
        let stats = BasketStats::calculate(&positions);
        assert_eq!(stats.count, 3);
        assert_eq!(stats.total_lots, dec!(0.06));
        assert_eq!(stats.profit, dec!(-45));
        // 加权均价 = (0.01×2000 + 0.02×1970 + 0.03×1940) / 0.06 = 117.6 / 0.06 = 1960
        assert_eq!(stats.weighted_avg_price, dec!(1960));
        assert_eq!(stats.side, Some(Side::Buy));
    }
}