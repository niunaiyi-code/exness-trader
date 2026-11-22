//! 财经日历服务 — 共享全局状态（docs/03 §3.2）。

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewsEvent {
    pub id: String,
    pub title: String,
    pub country: String,
    pub impact: String, // 高/中/低
    pub forecast: Option<String>,
    pub previous: Option<String>,
    pub actual: Option<String>,
    pub timestamp: i64, // Unix 秒
}

/// 带 10 秒缓存的全局财经日历。
pub struct FinanceCalendar {
    events: Arc<RwLock<Vec<NewsEvent>>>,
    last_update: Arc<RwLock<i64>>,
    cache_ttl_sec: i64,
    http_client: reqwest::Client,  // 复用 HTTP 客户端
}

impl FinanceCalendar {
    pub fn new() -> Self {
        // 创建优化的 HTTP 客户端，支持连接池
        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .connect_timeout(std::time::Duration::from_secs(5))
            .pool_max_idle_per_host(5)
            .pool_idle_timeout(std::time::Duration::from_secs(90))
            .tcp_keepalive(Some(std::time::Duration::from_secs(60)))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        
        Self {
            events: Arc::new(RwLock::new(Vec::new())),
            last_update: Arc::new(RwLock::new(-999999)), // 强制首次刷新
            cache_ttl_sec: 10,
            http_client,
        }
    }

    /// 检查日历是否需要刷新（缓存未命中）。
    pub async fn should_refresh(&self, now_sec: i64) -> bool {
        let last_update = *self.last_update.read().await;
        (now_sec - last_update) >= self.cache_ttl_sec
    }

    /// 用新事件更新日历（由后台拉取任务调用）。
    pub async fn update(&self, events: Vec<NewsEvent>, now_sec: i64) {
        let mut events_lock = self.events.write().await;
        *events_lock = events;

        let mut last_update = self.last_update.write().await;
        *last_update = now_sec;
    }

    /// 获取未来 N 秒内的所有高影响力事件（用于新闻规避检查）。
    pub async fn high_impact_events(&self, from_sec: i64, to_sec: i64) -> Vec<NewsEvent> {
        let events = self.events.read().await;
        events
            .iter()
            .filter(|e| e.impact == "high" && e.timestamp >= from_sec && e.timestamp <= to_sec)
            .cloned()
            .collect()
    }

    /// 检查当前是否处于新闻规避窗口（高影响力事件前后 30 分钟）。
    pub async fn is_in_news_avoid_window(&self, now_sec: i64) -> bool {
        let window_sec = 30 * 60; // 30 分钟
        let events = self
            .high_impact_events(now_sec - window_sec, now_sec + window_sec)
            .await;
        !events.is_empty()
    }

    /// 获取当前缓存中的所有事件。
    pub async fn all_events(&self) -> Vec<NewsEvent> {
        self.events.read().await.clone()
    }

    /// 缓存事件数量（仅计数时无需克隆整个列表）。
    pub async fn len(&self) -> usize {
        self.events.read().await.len()
    }

    /// 只重打最后更新时间戳，不重写事件列表。
    pub async fn touch(&self, now_sec: i64) {
        *self.last_update.write().await = now_sec;
    }

    /// 从财经日历服务刷新事件；只保留未取消的美国高影响力定时事件。
    pub async fn refresh_from_api(&self, api_url: &str, api_key: &str, now_sec: i64) -> anyhow::Result<()> {
        let from = chrono::DateTime::from_timestamp(now_sec, 0)
            .ok_or_else(|| anyhow::anyhow!("invalid calendar refresh timestamp"))?
            .date_naive() - chrono::Days::new(1);
        let to = from + chrono::Days::new(7);
        let response: Value = self.http_client
            .get(api_url)
            .query(&[("from", from.to_string()), ("to", to.to_string()), ("country", "US".into()), ("impact", "high".into()), ("limit", "200".into()), ("offset", "0".into())])
            .header("X-API-Key", api_key)
            .send().await?.error_for_status()?.json().await?;

        let list = response.as_array()
            .or_else(|| response.get("events").and_then(Value::as_array))
            .or_else(|| response.get("data").and_then(Value::as_array))
            .ok_or_else(|| anyhow::anyhow!("calendar response does not contain an event list"))?;

        let events = list.iter().filter_map(|item| {
            let country = item.get("country")?.as_str()?;
            let impact = item.get("impact")?.as_str()?;
            let status = item.get("status").and_then(Value::as_str).unwrap_or_default();
            if country != "US" || !impact.eq_ignore_ascii_case("high") || status.eq_ignore_ascii_case("cancelled")
                || item.get("all_day").and_then(Value::as_bool).unwrap_or(false) {
                return None;
            }
            let timestamp_text = item.get("release_at_utc").or_else(|| item.get("time_utc"))?.as_str()?;
            let timestamp = chrono::DateTime::parse_from_rfc3339(timestamp_text).ok()?.timestamp();
            Some(NewsEvent {
                id: item.get("id").and_then(Value::as_str).unwrap_or(timestamp_text).to_string(),
                title: item.get("title").and_then(Value::as_str).unwrap_or("High impact event").to_string(),
                country: country.to_string(),
                impact: impact.to_string(),
                forecast: item.get("forecast").and_then(Value::as_str).map(str::to_string),
                previous: item.get("previous").and_then(Value::as_str).map(str::to_string),
                actual: item.get("actual").and_then(Value::as_str).map(str::to_string),
                timestamp,
            })
        }).collect();
        self.update(events, now_sec).await;
        Ok(())
    }

    pub async fn next_high_impact_event(&self, now_sec: i64) -> Option<i64> {
        self.events.read().await.iter()
            .filter(|event| event.impact.eq_ignore_ascii_case("high") && event.timestamp >= now_sec - 3 * 60 * 60)
            .map(|event| event.timestamp)
            .min()
    }

    /// 获取最后更新时间戳（秒）。
    pub async fn last_update(&self) -> i64 {
        *self.last_update.read().await
    }

    /// 缓存 TTL（秒）。
    pub fn cache_ttl(&self) -> i64 {
        self.cache_ttl_sec
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn calendar_cache_ttl() {
        let cal = FinanceCalendar::new();
        assert!(cal.should_refresh(0).await);
        assert!(cal.should_refresh(9).await);
        assert!(cal.should_refresh(10).await); // 缓存 TTL = 10 秒，last_update = 0

        cal.update(vec![], 100).await;
        assert!(!cal.should_refresh(105).await); // 仍缓存于 105
        assert!(cal.should_refresh(111).await); // 在 111 处超期
    }

    #[tokio::test]
    async fn high_impact_filter() {
        let cal = FinanceCalendar::new();
        let events = vec![
            NewsEvent {
                id: "ev1".to_string(),
                title: "FOMC".to_string(),
                country: "US".to_string(),
                impact: "high".to_string(),
                forecast: None,
                previous: None,
                actual: None,
                timestamp: 1000,
            },
            NewsEvent {
                id: "ev2".to_string(),
                title: "Jobs".to_string(),
                country: "US".to_string(),
                impact: "medium".to_string(),
                forecast: None,
                previous: None,
                actual: None,
                timestamp: 1100,
            },
        ];
        cal.update(events, 900).await;

        let high = cal.high_impact_events(0, 2000).await;
        assert_eq!(high.len(), 1);
        assert_eq!(high[0].title, "FOMC");
    }

    #[tokio::test]
    async fn news_avoid_window() {
        let cal = FinanceCalendar::new();
        cal.update(
            vec![NewsEvent {
                id: "ev1".to_string(),
                title: "FOMC".to_string(),
                country: "US".to_string(),
                impact: "high".to_string(),
                forecast: None,
                previous: None,
                actual: None,
                timestamp: 5000,
            }],
            4000,
        )
        .await;

        // 窗口之外
        assert!(!cal.is_in_news_avoid_window(1000).await);

        // 处于 30 分钟窗口内
        assert!(cal.is_in_news_avoid_window(4500).await);
        assert!(cal.is_in_news_avoid_window(5000).await);
        assert!(cal.is_in_news_avoid_window(5500).await);
    }
}
