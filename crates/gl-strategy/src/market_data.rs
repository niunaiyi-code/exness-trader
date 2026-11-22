//! 行情数据适配器 — 带无损保证的 tick 队列（R1-R5）。
//! （docs/03-architecture.md §3.3）。

use crate::types::Side;
use rust_decimal::Decimal;
use std::collections::VecDeque;

/// 来自 Exness WebSocket 的 tick。
#[derive(Debug, Clone)]
pub struct Tick {
    pub instrument: String,
    pub bid: Decimal,
    pub ask: Decimal,
    pub timestamp_ms: u64,
}

/// 带 tick 无损保证的行情数据队列。
/// R1：WS tick 无阻塞入队（有界队列，淘汰时告警）。
/// R2：Runner 每轮出队一个 tick（FIFO）。
/// R3：队列容量 = 500 个 tick（上游节流 500ms → 最多 2 tick/秒）。
/// R4：Exness WS 可能节流到 500ms（每品种最多 2 tick/秒）。
/// R5：队列满时记录严重告警（上游突发超出容量）。
pub struct MarketDataQueue {
    pub queue: VecDeque<Tick>,
    pub capacity: usize,
    pub overflow_count: u64,
}

impl MarketDataQueue {
    pub fn new(capacity: usize) -> Self {
        Self {
            queue: VecDeque::with_capacity(capacity),
            capacity,
            overflow_count: 0,
        }
    }

    /// 将 WS 收到的 tick 入队（R1：非阻塞）。
    pub fn enqueue(&mut self, tick: Tick) {
        if self.queue.len() >= self.capacity {
            // 溢出：淘汰最旧 tick 并计数。
            self.queue.pop_front();
            self.overflow_count += 1;
            // TODO：记录严重告警（上游突发超出容量）。
        }
        self.queue.push_back(tick);
    }

    /// 出队下一个 tick（R2：FIFO，每轮一个）。
    pub fn dequeue(&mut self) -> Option<Tick> {
        self.queue.pop_front()
    }

    /// 检查队列是否为空。
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    /// 当前队列长度。
    pub fn len(&self) -> usize {
        self.queue.len()
    }

    /// 获取溢出计数（因容量超限丢失的 tick 数）。
    pub fn overflow_count(&self) -> u64 {
        self.overflow_count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> Decimal {
        s.parse().unwrap()
    }

    #[test]
    fn queue_new() {
        let queue = MarketDataQueue::new(500);
        assert_eq!(queue.capacity, 500);
        assert!(queue.is_empty());
        assert_eq!(queue.overflow_count(), 0);
    }

    #[test]
    fn enqueue_dequeue() {
        let mut queue = MarketDataQueue::new(500);
        let tick = Tick {
            instrument: "XAUUSD".to_string(),
            bid: d("3700"),
            ask: d("3700.5"),
            timestamp_ms: 1000,
        };
        queue.enqueue(tick.clone());
        assert_eq!(queue.len(), 1);

        let dequeued = queue.dequeue().unwrap();
        assert_eq!(dequeued.bid, d("3700"));
        assert!(queue.is_empty());
    }

    #[test]
    fn overflow_evicts_oldest() {
        let mut queue = MarketDataQueue::new(2); // 测试用小容量
        let tick1 = Tick {
            instrument: "XAUUSD".to_string(),
            bid: d("3700"),
            ask: d("3700.5"),
            timestamp_ms: 1000,
        };
        let tick2 = Tick {
            instrument: "XAUUSD".to_string(),
            bid: d("3701"),
            ask: d("3701.5"),
            timestamp_ms: 2000,
        };
        let tick3 = Tick {
            instrument: "XAUUSD".to_string(),
            bid: d("3702"),
            ask: d("3702.5"),
            timestamp_ms: 3000,
        };

        queue.enqueue(tick1);
        queue.enqueue(tick2);
        queue.enqueue(tick3); // 溢出：tick1 被淘汰

        assert_eq!(queue.len(), 2);
        assert_eq!(queue.overflow_count(), 1);

        let first = queue.dequeue().unwrap();
        assert_eq!(first.bid, d("3701")); // tick2 现位于队首
    }
}
