//! 行情数据：为策略决策提供 tick + K 线管理。

use exness_api::{Candle, InstrumentCondition, Tick};
use std::collections::VecDeque;

const MAX_CANDLES: usize = 100;

pub struct MarketData {
    pub latest_tick: Option<Tick>,
    pub candles_m1: VecDeque<Candle>,
    pub next_news_event_time: Option<chrono::DateTime<chrono::Utc>>,
    pub news_calendar_ready: bool,
    pub instrument_condition: Option<InstrumentCondition>,
}

impl MarketData {
    pub fn new() -> Self {
        Self {
            latest_tick: None,
            candles_m1: VecDeque::with_capacity(MAX_CANDLES),
            next_news_event_time: None,
            news_calendar_ready: false,
            instrument_condition: None,
        }
    }

    pub fn update_tick(&mut self, tick: Tick) {
        self.latest_tick = Some(tick);
    }

    pub fn update_candle(&mut self, candle: Candle) {
        self.candles_m1.push_back(candle);
        if self.candles_m1.len() > MAX_CANDLES {
            self.candles_m1.pop_front();
        }
    }

    pub fn push_candle(&mut self, candle: Candle) {
        self.update_candle(candle);
    }

    pub fn candle_count(&self) -> usize {
        self.candles_m1.len()
    }

    pub fn latest_closed_candle(&self) -> Option<&Candle> {
        // 假设 K 线按时间排序
        self.candles_m1.back()
    }

    pub fn bid(&self) -> Option<rust_decimal::Decimal> {
        self.latest_tick
            .as_ref()
            .and_then(|t| rust_decimal::Decimal::try_from(t.bid).ok())
    }

    pub fn ask(&self) -> Option<rust_decimal::Decimal> {
        self.latest_tick
            .as_ref()
            .and_then(|t| rust_decimal::Decimal::try_from(t.ask).ok())
    }
}

impl Default for MarketData {
    fn default() -> Self {
        Self::new()
    }
}
