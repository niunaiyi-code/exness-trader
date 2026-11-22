//! 带接入点发现的签名 REST 客户端。
//!
//! 一个 [`Client`] 恰好服务一个交易账户（Trading Full-access 密钥绑定于单个
//! 账户）。所有账户调用的 base URL 都是该账户的**接入点**，通过
//! `GET {gateway}/v1/trading/access-point?account_id=…`，结果在客户端生命周期内缓存。

use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;
use std::time::Duration;

use chrono::SecondsFormat;
use reqwest::header::{CONTENT_TYPE, DATE, RETRY_AFTER};
use reqwest::{Method, StatusCode};
use serde::de::DeserializeOwned;
use url::Url;

use crate::auth::{now_ms, Credentials, SignRequest, SignedHeaders};
use crate::error::{Error, Result};
use crate::models::*;

/// 要打开哪个 WebSocket 流。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WsStream {
    Ticks,
    Events,
}

impl WsStream {
    pub fn path(self, account_id: &str) -> String {
        match self {
            WsStream::Ticks => format!("/v1/server-events/accounts/{account_id}/ws/ticks"),
            WsStream::Events => format!("/v1/server-events/accounts/{account_id}/ws/events"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ClientConfig {
    /// 来自个人专区 → 密钥管理（DNS 组件）的网关地址。发现接入点所必需。
    pub gateway_url: Option<Url>,
    /// 仅测试用覆盖：跳过发现，直接把它当接入点。
    pub rest_base_url: Option<Url>,
    pub timeout: Duration,
    pub connect_timeout: Duration,
    pub user_agent: String,
    /// 需要请求的 `Sec-WebSocket-Protocol`；服务端应答 `exness-ws-protocol`
    /// 否则拒绝/吞掉该连接。
    pub ws_subprotocol: Option<String>,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            gateway_url: None,
            rest_base_url: None,
            timeout: Duration::from_secs(10),
            connect_timeout: Duration::from_secs(5),
            user_agent: format!("gl-exness-trader/{}", env!("CARGO_PKG_VERSION")),
            ws_subprotocol: Some(DEFAULT_WS_SUBPROTOCOL.to_owned()),
        }
    }
}

/// 在实盘接入点上观测到（2026-11-19）：服务端总是返回该
/// `101 Switching Protocols` 上的子协议。
pub const DEFAULT_WS_SUBPROTOCOL: &str = "exness-ws-protocol";

#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    creds: Arc<Credentials>,
    account_id: String,
    gateway: Option<Url>,
    access_point: Arc<RwLock<Option<Url>>>,
    /// server_time − local_time，来自 `Date` 响应头（秒级精度）。
    clock_skew_ms: Arc<AtomicI64>,
    ws_subprotocol: Option<String>,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client")
            .field("account_id", &self.account_id)
            .field("api_key", &self.creds.api_key())
            .field("access_point", &"<async>")
            .finish_non_exhaustive()
    }
}

impl Client {
    pub fn new(creds: Credentials, account_id: impl Into<String>, cfg: ClientConfig) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(cfg.timeout)
            .connect_timeout(cfg.connect_timeout)
            .user_agent(cfg.user_agent)
            .use_rustls_tls()
            .pool_max_idle_per_host(10)  // 每个主机保持 10 个空闲连接
            .pool_idle_timeout(Duration::from_secs(90))  // 空闲连接 90 秒超时
            .tcp_keepalive(Some(Duration::from_secs(60)))  // TCP keepalive
            .http2_keep_alive_interval(Some(Duration::from_secs(30)))  // HTTP/2 ping
            .http2_keep_alive_timeout(Duration::from_secs(10))
            .build()?;
        let access_point = cfg.rest_base_url.as_ref().map(origin_only).transpose()?;
        Ok(Self {
            http,
            creds: Arc::new(creds),
            account_id: account_id.into(),
            gateway: cfg.gateway_url.as_ref().map(origin_only).transpose()?,
            access_point: Arc::new(RwLock::new(access_point)),
            clock_skew_ms: Arc::new(AtomicI64::new(0)),
            ws_subprotocol: cfg.ws_subprotocol,
        })
    }

    pub fn ws_subprotocol(&self) -> Option<&str> {
        self.ws_subprotocol.as_deref()
    }

    pub fn account_id(&self) -> &str {
        &self.account_id
    }

    pub fn credentials(&self) -> &Credentials {
        &self.creds
    }

    pub async fn access_point(&self) -> Option<Url> {
        self.access_point.read().await.clone()
    }

    pub async fn set_access_point(&self, url: Url) -> Result<()> {
        *self.access_point.write().await = Some(origin_only(&url)?);
        Ok(())
    }

    /// 上次观测到的服务端−本地时钟差（毫秒，首次响应前为 0）。
    pub fn clock_skew_ms(&self) -> i64 {
        self.clock_skew_ms.load(Ordering::Relaxed)
    }

    /// 签名时间戳：按观测到的时钟偏差修正后的本地时间。
    fn signing_timestamp(&self) -> i64 {
        now_ms() + self.clock_skew_ms()
    }

    // ----------------------------------------------------------------- 接入点发现

    /// `GET {gateway}/v1/trading/access-point?account_id=…`；将结果存为 base URL。
    pub async fn discover_access_point(&self) -> Result<AccessPointResponse> {
        let gw = self
            .gateway
            .clone()
            .ok_or_else(|| Error::Internal("gateway_url is not configured; copy it from Key Management".into()))?;
        let path = format!("/v1/trading/access-point?account_id={}", self.account_id);
        let resp: AccessPointResponse = self.send_json(&gw, Method::GET, &path, None, None, "access-point").await?;
        let url = Url::parse(&resp.access_point)?;
        self.set_access_point(url).await?;
        tracing::info!(account_id = %self.account_id, access_point = %resp.access_point, "access point discovered");
        Ok(resp)
    }

    /// 与 [`discover_access_point`](Self::discover_access_point) 类似，但返回
    /// 网关拒绝凭据时返回 [`Error::Auth`]（*"ok"*|"error"）。用于
    /// 账户注册流程，使无效密钥被报告为凭据问题而非传输问题。
    pub async fn verify_credentials(&self) -> Result<AccessPointResponse> {
        match self.discover_access_point().await {
            Err(Error::Api { status, code, message, raw, .. })
                if status == 401 || status == 403 || matches!(code, Some(1000..=1002)) =>
            {
                let msg = message.clone();
                Err(Error::Api {
                    status,
                    code,
                    message: format!("credentials rejected: {msg}"),
                    retry_after_ms: None,
                    raw,
                })
            }
            other => other,
        }
    }

    /// 返回缓存的接入点，首次使用时执行发现。
    pub async fn ensure_access_point(&self) -> Result<Url> {
        if let Some(u) = self.access_point().await {
            return Ok(u);
        }
        self.discover_access_point().await?;
        self.access_point().await.ok_or(Error::NoAccessPoint)
    }

    /// 丢弃缓存的接入点，使下次调用重新发现。
    pub async fn invalidate_access_point(&self) {
        *self.access_point.write().await = None;
    }

    // ----------------------------------------------------------------- 配置

    pub async fn account_details(&self) -> Result<AccountDetails> {
        self.get(&format!("/v1/configuration/accounts/{}/account", self.account_id), "account").await
    }

    pub async fn instruments(&self) -> Result<Vec<String>> {
        let r: InstrumentNameList =
            self.get(&format!("/v1/configuration/accounts/{}/instruments", self.account_id), "instruments").await?;
        Ok(r.instruments)
    }

    pub async fn instrument_conditions(&self, instrument: &str) -> Result<InstrumentCondition> {
        self.get(
            &format!("/v1/configuration/accounts/{}/instruments/{instrument}/conditions", self.account_id),
            "conditions",
        )
        .await
    }

    pub async fn limits(&self) -> Result<ApiLimits> {
        let r: LimitsResponse =
            self.get(&format!("/v1/configuration/accounts/{}/limits", self.account_id), "limits").await?;
        Ok(r.limits)
    }

    // ----------------------------------------------------------------- 行情数据

    pub async fn candles(&self, q: &CandleQuery) -> Result<Vec<Candle>> {
        let mut query_string = String::new();
        append_query_pair(&mut query_string, "instrument", &q.instrument);
        append_query_pair(&mut query_string, "price_type", q.price_type.as_str());
        append_query_pair(&mut query_string, "timeframe", q.timeframe.as_str());
        append_query_pair(&mut query_string, "from", &fmt_time(&q.from));
        if let Some(to) = &q.to {
            append_query_pair(&mut query_string, "to", &fmt_time(to));
        }
        if let Some(c) = q.count {
            append_query_pair(&mut query_string, "count", &c.to_string());
        }
        let path = format!("/v1/market-data/accounts/{}/candles?{}", self.account_id, query_string);
        let r: CandleHistoryResponse = self.get(&path, "candles").await?;
        Ok(r.candles)
    }

    // ----------------------------------------------------------------- 交易

    pub async fn snapshot(&self) -> Result<TradingStateSnapshot> {
        self.get(&format!("/v1/trading/accounts/{}/snapshot", self.account_id), "snapshot").await
    }

    /// `GET /v1/history/accounts/{id}/deals` → 获取历史成交（含平仓盈亏）。
    pub async fn deals_history(
        &self,
        from: Option<&str>,
        to: Option<&str>,
        cursor: Option<&str>,
        limit: Option<u32>,
        deal_type: Option<&str>,
    ) -> Result<DealsHistoryResponse> {
        let mut query = String::new();
        if let Some(c) = cursor {
            append_query_pair(&mut query, "cursor", c);
        } else {
            let default_from;
            let f = match from {
                Some(f) => f,
                None => {
                    default_from = (chrono::Utc::now() - chrono::Duration::days(7))
                        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
                    &default_from
                }
            };
            append_query_pair(&mut query, "from", f);
            if let Some(t) = to {
                append_query_pair(&mut query, "to", t);
            }
            if let Some(dt) = deal_type {
                append_query_pair(&mut query, "deal_type", dt);
            }
        }
        if let Some(l) = limit {
            append_query_pair(&mut query, "limit", &l.to_string());
        }
        let path = format!("/v1/history/accounts/{}/deals?{}", self.account_id, query);
        self.get(&path, "deals_history").await
    }

    /// `POST /positions` → 202 ACK。`idempotency_key` 必须满足 [`crate::auth::validate_idempotency_key`
    pub async fn open_position(&self, req: &OpenPositionRequest, idempotency_key: &str) -> Result<AckResponse> {
        let base = self.ensure_access_point().await?;
        let body = serde_json::to_vec(req).map_err(|e| Error::Internal(format!("serialize open request: {e}")))?;
        let path = format!("/v1/trading/accounts/{}/positions", self.account_id);
        self.send_json(&base, Method::POST, &path, Some(body), Some(idempotency_key), "open_position").await
    }

    /// `DELETE /positions/{id}[?volume&price&deviation&close_by_position_id]` → 返回 202 ACK。
    pub async fn close_position(
        &self,
        position_id: &str,
        params: &ClosePositionParams,
        idempotency_key: &str,
    ) -> Result<AckResponse> {
        let base = self.ensure_access_point().await?;
        let mut query = String::new();
        if let Some(v) = &params.volume {
            append_query_pair(&mut query, "volume", &v.to_string());
        }
        if let Some(p) = &params.price {
            append_query_pair(&mut query, "price", &p.to_string());
        }
        if let Some(d) = &params.deviation {
            append_query_pair(&mut query, "deviation", &d.to_string());
        }
        if let Some(id) = &params.close_by_position_id {
            append_query_pair(&mut query, "close_by_position_id", id);
        }
        let mut path = format!("/v1/trading/accounts/{}/positions/{position_id}", self.account_id);
        if !query.is_empty() {
            path.push('?');
            path.push_str(&query);
        }
        self.send_json(&base, Method::DELETE, &path, None, Some(idempotency_key), "close_position").await
    }

    pub async fn operation_status(&self, operation_id: &str) -> Result<OperationStatusResponse> {
        self.get(&format!("/v1/trading/accounts/{}/operations/{operation_id}", self.account_id), "operation").await
    }

    /// 诊断：对任意账户相对路径发起签名 GET，返回原始 JSON。
    pub async fn get_raw(&self, path_and_query: &str) -> Result<serde_json::Value> {
        self.get(path_and_query, "raw").await
    }

    // ----------------------------------------------------------------- WebSocket 辅助

    /// `wss://{access_point_host}{stream path}`（WebSocket 地址）。
    pub async fn ws_url(&self, stream: WsStream) -> Result<Url> {
        let base = self.ensure_access_point().await?;
        let scheme = match base.scheme() {
            "https" => "wss",
            "http" => "ws",
            other => return Err(Error::Internal(format!("unsupported access point scheme {other}"))),
        };
        let mut url = base;
        url.set_scheme(scheme).map_err(|_| Error::Internal("cannot set ws scheme".into()))?;
        url.set_path(&stream.path(&self.account_id));
        url.set_query(None);
        Ok(url)
    }

    /// 为 WebSocket 握手签名（对流路径发起 `GET`，空 body，空幂等键）。
    pub fn sign_ws_handshake(&self, stream: WsStream) -> Result<SignedHeaders> {
        Ok(self.creds.sign(&SignRequest {
            method: "GET",
            path_and_query: &stream.path(&self.account_id),
            body: b"",
            idempotency_key: None,
            timestamp_ms: self.signing_timestamp(),
        })?)
    }

    // ----------------------------------------------------------------- 基础设施

    async fn get<T: DeserializeOwned>(&self, path_and_query: &str, ctx: &'static str) -> Result<T> {
        let base = self.ensure_access_point().await?;
        self.send_json(&base, Method::GET, path_and_query, None, None, ctx).await
    }

    async fn send_json<T: DeserializeOwned>(
        &self,
        base: &Url,
        method: Method,
        path_and_query: &str,
        body: Option<Vec<u8>>,
        idempotency_key: Option<&str>,
        ctx: &'static str,
    ) -> Result<T> {
        let bytes = self.send_bytes(base, method, path_and_query, body, idempotency_key).await?;
        serde_json::from_slice(&bytes).map_err(|e| Error::decode(ctx, &bytes, e))
    }

    async fn send_bytes(
        &self,
        base: &Url,
        method: Method,
        path_and_query: &str,
        body: Option<Vec<u8>>,
        idempotency_key: Option<&str>,
    ) -> Result<Vec<u8>> {
        let url = join_exact(base, path_and_query)?;
        let body_bytes: &[u8] = body.as_deref().unwrap_or(b"");
        let headers = self.creds.sign(&SignRequest {
            method: method.as_str(),
            path_and_query,
            body: body_bytes,
            idempotency_key,
            timestamp_ms: self.signing_timestamp(),
        })?;

        let mut rb = self.http.request(method.clone(), url);
        for (name, value) in headers.iter() {
            rb = rb.header(name, value);
        }
        if let Some(b) = body {
            rb = rb.header(CONTENT_TYPE, "application/json").body(b);
        }

        tracing::debug!(account_id = %self.account_id, %method, path = path_and_query, "exness request");
        let resp = rb.send().await?;
        self.observe_server_date(resp.headers());
        let status = resp.status();
        let retry_after_ms = parse_retry_after(resp.headers());
        let bytes = resp.bytes().await?.to_vec();

        if status.is_success() {
            Ok(bytes)
        } else {
            let err = api_error(status, retry_after_ms, &bytes);
            tracing::warn!(account_id = %self.account_id, %method, path = path_and_query, %err, "exness error");
            Err(err)
        }
    }

    fn observe_server_date(&self, headers: &reqwest::header::HeaderMap) {
        if let Some(d) = headers.get(DATE).and_then(|v| v.to_str().ok()) {
            if let Ok(server) = chrono::DateTime::parse_from_rfc2822(d) {
                let new_skew = server.timestamp_millis() - now_ms();
                let current_skew = self.clock_skew_ms.load(Ordering::Relaxed);
                
                // ⚠️ 防御性检查：偏差突变超过 10 秒则拒绝更新（可能是中间人攻击或服务器时钟跳变）
                let skew_delta = (new_skew - current_skew).abs();
                if skew_delta > 10_000 && current_skew != 0 {
                    tracing::warn!(
                        "拒绝时钟偏差更新：当前 {}ms，新值 {}ms（变化 {}ms），可能是异常",
                        current_skew, new_skew, skew_delta
                    );
                    return;
                }
                
                // 渐进式校正：偏差超过 1 秒才更新，避免频繁抖动
                if skew_delta > 1000 || current_skew == 0 {
                    self.clock_skew_ms.store(new_skew, Ordering::Relaxed);
                    tracing::debug!("时钟偏差已更新: {}ms → {}ms", current_skew, new_skew);
                }
            }
        }
    }
}

/// 仅保留 `scheme://host[:port]`（接入点只有主机名）。
fn origin_only(url: &Url) -> Result<Url> {
    let host = url.host_str().ok_or_else(|| Error::Internal(format!("url {url} has no host")))?;
    let mut s = format!("{}://{host}", url.scheme());
    if let Some(p) = url.port() {
        s.push(':');
        s.push_str(&p.to_string());
    }
    Ok(Url::parse(&s)?)
}

/// 构造请求 URL 并校验 `path + query` 逐字节保持一致：已签名的
/// `path` 必须与实际上线的内容完全一致。
fn join_exact(base: &Url, path_and_query: &str) -> Result<Url> {
    if !path_and_query.starts_with('/') {
        return Err(Error::Internal(format!("path must start with '/': {path_and_query}")));
    }
    let full = format!("{}{}", base.as_str().trim_end_matches('/'), path_and_query);
    let url = Url::parse(&full)?;
    let mut rebuilt = url.path().to_owned();
    if let Some(q) = url.query() {
        rebuilt.push('?');
        rebuilt.push_str(q);
    }
    if rebuilt != path_and_query {
        return Err(Error::Internal(format!(
            "url normalisation changed the signed path: signed={path_and_query} sent={rebuilt}"
        )));
    }
    Ok(url)
}

fn fmt_time(t: &chrono::DateTime<chrono::Utc>) -> String {
    // 显式的 `+00:00` 偏移，与规范示例一致
    t.to_rfc3339_opts(SecondsFormat::Secs, false)
}

/// 向查询串追加 URL 编码的 `key=value` 对。Send 安全的替代方案，
/// `form_urlencoded::Serializer`，其 `&dyn Fn` 编码覆盖不是 `Sync`。
fn append_query_pair(out: &mut String, key: &str, value: &str) {
    if !out.is_empty() {
        out.push('&');
    }
    out.push_str(key);
    out.push('=');
    for s in url::form_urlencoded::byte_serialize(value.as_bytes()) {
        out.push_str(s);
    }
}

fn parse_retry_after(headers: &reqwest::header::HeaderMap) -> Option<u64> {
    let v = headers.get(RETRY_AFTER)?.to_str().ok()?;
    if let Ok(secs) = v.trim().parse::<f64>() {
        return Some((secs * 1000.0) as u64);
    }
    let when = chrono::DateTime::parse_from_rfc2822(v).ok()?;
    Some((when.timestamp_millis() - now_ms()).max(0) as u64)
}

fn api_error(status: StatusCode, retry_after_ms: Option<u64>, body: &[u8]) -> Error {
    let raw = String::from_utf8_lossy(body);
    let raw = if raw.len() > 512 { format!("{}…", &raw[..512]) } else { raw.into_owned() };

    // spec shape: {"code": 1001, "error_message": "AUTH_INVALID_SIGNATURE"}
    if let Ok(e) = serde_json::from_slice::<ErrorResponse>(body) {
        return Error::Api {
            status: status.as_u16(),
            code: Some(e.code),
            message: e.error_message,
            retry_after_ms,
            raw,
        };
    }
    // defensive: {"error": {"code": …, "error_message"|"message": …}}
    if let Ok(v) = serde_json::from_slice::<serde_json::Value>(body) {
        let inner = v.get("error").unwrap_or(&v);
        let code = inner.get("code").and_then(|c| c.as_i64()).map(|c| c as i32);
        let message = inner
            .get("error_message")
            .or_else(|| inner.get("message"))
            .and_then(|m| m.as_str())
            .map(str::to_owned)
            .unwrap_or_else(|| status.canonical_reason().unwrap_or("error").to_owned());
        return Error::Api { status: status.as_u16(), code, message, retry_after_ms, raw };
    }
    Error::Api {
        status: status.as_u16(),
        code: None,
        message: status.canonical_reason().unwrap_or("error").to_owned(),
        retry_after_ms,
        raw,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn join_exact_keeps_signed_path_and_encodes_time_safely() {
        let base = Url::parse("https://ap-1.exness.com").unwrap();
        let from = chrono::Utc.with_ymd_and_hms(2026, 3, 16, 12, 54, 30).unwrap();
        let mut qs = url::form_urlencoded::Serializer::new(String::new());
        qs.append_pair("instrument", "XAUUSD").append_pair("from", &fmt_time(&from)).append_pair("count", "-5");
        let path = format!("/v1/market-data/accounts/1/candles?{}", qs.finish());
        assert!(path.contains("from=2026-03-16T12%3A54%3A30%2B00%3A00"), "{path}");
        let url = join_exact(&base, &path).unwrap();
        assert_eq!(url.as_str(), format!("https://ap-1.exness.com{path}"));
    }

    #[test]
    fn join_exact_rejects_paths_that_normalise_differently() {
        let base = Url::parse("https://ap-1.exness.com").unwrap();
        assert!(join_exact(&base, "/v1/a/../b").is_err());
        assert!(join_exact(&base, "v1/no-leading-slash").is_err());
        assert!(join_exact(&base, "/v1/trading/accounts/1/positions/2?volume=0.01&deviation=200").is_ok());
    }

    #[test]
    fn origin_only_strips_paths() {
        let u = origin_only(&Url::parse("https://ap-1.exness.com/some/path?x=1").unwrap()).unwrap();
        assert_eq!(u.as_str(), "https://ap-1.exness.com/");
        let u = origin_only(&Url::parse("http://localhost:8080/").unwrap()).unwrap();
        assert_eq!(u.as_str(), "http://localhost:8080/");
    }

    #[test]
    fn api_error_parsing() {
        let e = api_error(StatusCode::UNAUTHORIZED, None, br#"{"code":1001,"error_message":"AUTH_INVALID_SIGNATURE"}"#);
        assert_eq!(e.api_code(), Some(1001));
        assert_eq!(e.class(), crate::ErrorClass::Fatal);

        let e = api_error(
            StatusCode::TOO_MANY_REQUESTS,
            Some(2000),
            br#"{"code":3015,"error_message":"REQUEST_RATE_LIMIT"}"#,
        );
        assert!(e.is_rate_limited());
        assert_eq!(e.class(), crate::ErrorClass::Transient);

        let e = api_error(StatusCode::BAD_GATEWAY, None, b"<html>bad gateway</html>");
        assert_eq!(e.api_code(), None);
        assert_eq!(e.class(), crate::ErrorClass::Transient);

        let e = api_error(StatusCode::BAD_REQUEST, None, br#"{"error":{"code":3001,"message":"bad"}}"#);
        assert_eq!(e.api_code(), Some(3001));
    }

    #[tokio::test]
    async fn ws_url_and_handshake_signature() {
        let creds = Credentials::new("exnsk-x", &hex::encode([9u8; 32])).unwrap();
        let cfg =
            ClientConfig { rest_base_url: Some(Url::parse("https://ap-1.exness.com").unwrap()), ..Default::default() };
        let c = Client::new(creds, "123", cfg).unwrap();
        assert_eq!(
            c.ws_url(WsStream::Ticks).await.unwrap().as_str(),
            "wss://ap-1.exness.com/v1/server-events/accounts/123/ws/ticks"
        );
        let h = c.sign_ws_handshake(WsStream::Events).unwrap();
        assert_eq!(h.idempotency_key, "");
        let payload: serde_json::Value = serde_json::from_slice(&h.payload_bytes().unwrap()).unwrap();
        assert_eq!(payload["method"], "GET");
        assert_eq!(payload["path"], "/v1/server-events/accounts/123/ws/events");
    }

    #[test]
    fn retry_after_seconds() {
        let mut h = reqwest::header::HeaderMap::new();
        h.insert(RETRY_AFTER, "3".parse().unwrap());
        assert_eq!(parse_retry_after(&h), Some(3000));
    }
}
