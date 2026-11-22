//! 行情数据服务 — 提供高并发安全、防击穿、防截断的 Exness K 线缓存代理。

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use exness_api::{Candle, CandleQuery, Client, ClientConfig, Credentials, TradingStateSnapshot};
use tokio::sync::{Mutex, OnceCell, RwLock};
use url::Url;

/// 常量配置规约：消除魔法数字
const HOT_CACHE_TTL: Duration = Duration::from_secs(3);
const STALE_DEGRADE_TTL: Duration = Duration::from_secs(60);
const DEFAULT_FETCH_COUNT: i32 = 300;
const MAX_CACHE_ENTRIES: usize = 32;

struct CandleCacheEntry {
    fetched_at: Instant,
    candles: Arc<Vec<Candle>>,
}

pub struct MarketDataService {
    account_id: Option<String>,
    api_key: Option<String>,
    private_key: Option<String>,
    gateway_url: String,
    /// 使用 OnceCell 实现无锁、无竞态的单次安全初始化
    client: OnceCell<Arc<Client>>,
    /// 存量行情数据缓存
    candle_cache: RwLock<HashMap<(String, String), CandleCacheEntry>>,
    /// Single-Flight 单飞互斥锁：防止缓存失效瞬间发生惊群击穿
    fetch_gate: Mutex<()>,
}

impl MarketDataService {
    pub fn new(
        account_id: Option<String>,
        api_key: Option<String>,
        private_key: Option<String>,
        gateway_url: String,
    ) -> Self {
        Self {
            account_id,
            api_key,
            private_key,
            gateway_url,
            client: OnceCell::new(),
            candle_cache: RwLock::new(HashMap::new()),
            fetch_gate: Mutex::new(()),
        }
    }

    /// 并发安全的单例客户端初始化（零 DCL 竞态，零重复握手）
    pub async fn client(&self) -> anyhow::Result<Arc<Client>> {
        self.client
            .get_or_try_init(|| async {
                let (account_id, api_key, private_key) = match (
                    self.account_id.as_deref(),
                    self.api_key.as_deref(),
                    self.private_key.as_deref(),
                ) {
                    (Some(a), Some(k), Some(p)) => (a, k, p),
                    _ => anyhow::bail!("market feed probe credentials not configured"),
                };
                let creds = Credentials::new(api_key, private_key)?;
                let cfg = ClientConfig {
                    gateway_url: Url::parse(&self.gateway_url).ok(),
                    ..Default::default()
                };
                let c = Arc::new(Client::new(creds, account_id, cfg)?);
                c.ensure_access_point().await?;
                tracing::info!(account_id, "MarketDataService client initialized successfully");
                Ok(c)
            })
            .await
            .cloned()
    }

    /// 获取 K 线：具备单飞合并、条数足量验证与清晰错误暴露
    pub async fn candles(&self, symbol: &str, timeframe: &str, count: i32) -> anyhow::Result<Vec<Candle>> {
        let requested_count = count.clamp(1, 1000) as usize;
        let key = (symbol.to_string(), timeframe.to_string());

        // 1. 尝试从热缓存读取（必须同时满足：未过期 且 数据条数充足）
        {
            let cache = self.candle_cache.read().await;
            if let Some(entry) = cache.get(&key) {
                if entry.fetched_at.elapsed() < HOT_CACHE_TTL && entry.candles.len() >= requested_count {
                    let skip = entry.candles.len().saturating_sub(requested_count);
                    return Ok(entry.candles[skip..].to_vec());
                }
            }
        }

        // 2. 缓存失效或不足：获取单飞互斥锁，拦截惊群请求
        let _guard = self.fetch_gate.lock().await;

        // 2.1 双重检查：排队期间可能已被前序请求刷新完毕
        {
            let cache = self.candle_cache.read().await;
            if let Some(entry) = cache.get(&key) {
                if entry.fetched_at.elapsed() < HOT_CACHE_TTL && entry.candles.len() >= requested_count {
                    let skip = entry.candles.len().saturating_sub(requested_count);
                    return Ok(entry.candles[skip..].to_vec());
                }
            }
        }

        // 3. 执行单笔上游 HTTP 请求（至少拉取 DEFAULT_FETCH_COUNT 根以满足通用缓存）
        let fetch_limit = requested_count.max(DEFAULT_FETCH_COUNT as usize) as i32;
        let client = self.client().await?;
        let q = CandleQuery {
            instrument: symbol.to_string(),
            price_type: "bid".to_string(),
            timeframe: timeframe.to_string(),
            from: chrono::Utc::now(),
            to: None,
            count: Some(-fetch_limit),
        };

        match client.candles(&q).await {
            Ok(fresh_candles) => {
                let arc_candles = Arc::new(fresh_candles);
                let mut cache = self.candle_cache.write().await;
                if cache.len() >= MAX_CACHE_ENTRIES {
                    cache.clear(); // 容量保护
                }
                cache.insert(
                    key,
                    CandleCacheEntry {
                        fetched_at: Instant::now(),
                        candles: arc_candles.clone(),
                    },
                );
                let skip = arc_candles.len().saturating_sub(requested_count);
                Ok(arc_candles[skip..].to_vec())
            }
            Err(e) => {
                // 4. 熔断降级：仅允许临时限流或网络震荡使用短期存量缓存（带明确告警）
                let cache = self.candle_cache.read().await;
                if let Some(entry) = cache.get(&key) {
                    if entry.fetched_at.elapsed() < STALE_DEGRADE_TTL && entry.candles.len() >= requested_count {
                        tracing::warn!(
                            symbol,
                            timeframe,
                            error = %e,
                            stale_secs = entry.fetched_at.elapsed().as_secs(),
                            "Broker API rate-limited or unavailable; serving stale cached candles"
                        );
                        let skip = entry.candles.len().saturating_sub(requested_count);
                        return Ok(entry.candles[skip..].to_vec());
                    }
                }
                // 严重错误且无可用缓存时，坚决向上暴露异常，拒绝掩盖
                Err(anyhow::anyhow!("Failed to fetch candles for {symbol} ({timeframe}): {e}"))
            }
        }
    }

    /// 实时交易快照查询（由专门探测账户提供）
    pub async fn snapshot(&self) -> anyhow::Result<TradingStateSnapshot> {
        let client = self.client().await?;
        Ok(client.snapshot().await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn unconfigured_market_data_fails_cleanly() {
        let svc = MarketDataService::new(None, None, None, "https://api.exness.com".to_string());
        let res = svc.client().await;
        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("credentials not configured"));
    }

    #[tokio::test]
    async fn cache_entry_slicing() {
        let dummy_candles: Vec<Candle> = (0..300)
            .map(|i| Candle {
                time: 1000 + i,
                open: 2000.0 + i as f64,
                high: 2005.0 + i as f64,
                low: 1995.0 + i as f64,
                close: 2002.0 + i as f64,
                volume: 10.0,
            })
            .collect();

        let arc_candles = Arc::new(dummy_candles);
        let count = 50usize;
        let skip = arc_candles.len().saturating_sub(count);
        let sliced = &arc_candles[skip..];
        assert_eq!(sliced.len(), 50);
        assert_eq!(sliced.last().unwrap().time, 1299);
    }
}
