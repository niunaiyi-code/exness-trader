//! WebSocket tick 流与事件流客户端。

use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use tokio::time::{interval, sleep};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;

use crate::error::{Error, Result};
use crate::models::{Tick, WsEvent};
use crate::rest::{Client, WsStream};

/// ticks 的 WebSocket 订阅消息。
#[derive(Debug, Clone, Serialize)]
struct TickSubscribeMsg {
    id: String,
    subscribe: TickSubscribe,
}

#[derive(Debug, Clone, Serialize)]
struct TickSubscribe {
    event: String, // "ticks"
    instruments: Vec<String>,
}

/// 交易事件的 WebSocket 订阅消息。
#[derive(Debug, Clone, Serialize)]
struct EventSubscribeMsg {
    id: String,
    subscribe: EventSubscribe,
}

#[derive(Debug, Clone, Serialize)]
struct EventSubscribe {
    event: String, // "transactions"、"account_state" 等
    #[serde(skip_serializing_if = "Option::is_none")]
    instruments: Option<Vec<String>>,
}

/// tick 流客户端（实时 bid/ask 报价）。
pub struct TickStreamClient {
    client: Arc<Client>,
    symbols: Vec<String>,
}

impl TickStreamClient {
    pub fn new(client: Arc<Client>, symbols: Vec<String>) -> Self {
        Self { client, symbols }
    }

    /// 连接并订阅 tick 流，每收到一个 tick 调用 `on_tick`。
    /// 断线后按指数退避自动重连。
    pub async fn run<F>(&self, mut on_tick: F) -> Result<()>
    where
        F: FnMut(Tick) + Send + 'static,
    {
        let mut retry_delay = Duration::from_secs(1);
        const MAX_RETRY_DELAY: Duration = Duration::from_secs(60);

        loop {
            match self.connect_and_stream(&mut on_tick).await {
                Ok(_) => {
                    tracing::info!(account_id = %self.client.account_id(), "tick stream ended normally");
                    retry_delay = Duration::from_secs(1); // 正常断开时重置
                }
                Err(e) => {
                    tracing::error!(
                        account_id = %self.client.account_id(),
                        error = %e,
                        retry_in_secs = retry_delay.as_secs(),
                        "tick stream error, reconnecting"
                    );
                    sleep(retry_delay).await;
                    retry_delay = (retry_delay * 2).min(MAX_RETRY_DELAY);
                }
            }
        }
    }
    
    /// 连接并订阅 tick 流，重连后触发对账回调。
    /// 
    /// # 参数
    /// - `on_tick`: 每个 tick 的回调
    /// - `on_reconnect`: 重连成功后的回调（用于触发对账）
    pub async fn run_with_reconcile<F, R, Fut>(&self, mut on_tick: F, mut on_reconnect: R) -> Result<()>
    where
        F: FnMut(Tick) + Send + 'static,
        R: FnMut() -> Fut + Send + 'static,
        Fut: std::future::Future<Output = ()> + Send,
    {
        let mut retry_delay = Duration::from_secs(1);
        const MAX_RETRY_DELAY: Duration = Duration::from_secs(60);
        let mut is_first_connect = true;

        loop {
            match self.connect_and_stream(&mut on_tick).await {
                Ok(_) => {
                    tracing::info!(account_id = %self.client.account_id(), "tick stream ended normally");
                    retry_delay = Duration::from_secs(1);
                }
                Err(e) => {
                    tracing::error!(
                        account_id = %self.client.account_id(),
                        error = %e,
                        retry_in_secs = retry_delay.as_secs(),
                        "tick stream error, reconnecting"
                    );
                    sleep(retry_delay).await;
                    retry_delay = (retry_delay * 2).min(MAX_RETRY_DELAY);
                    
                    // ⚠️ 重连前触发对账（首次连接除外）
                    if !is_first_connect {
                        tracing::warn!(account_id = %self.client.account_id(), "重连成功，触发对账");
                        on_reconnect().await;
                    }
                    is_first_connect = false;
                }
            }
        }
    }

    async fn connect_and_stream<F>(&self, on_tick: &mut F) -> Result<()>
    where
        F: FnMut(Tick),
    {
        // 构造 WebSocket URL
        let url = self.client.ws_url(WsStream::Ticks).await?;
        let mut request = url.as_str().into_client_request()?;

        // 握手签名
        let headers = self.client.sign_ws_handshake(WsStream::Ticks)?;
        for (name, value) in headers.iter() {
            use tokio_tungstenite::tungstenite::http;
            let header_name: http::HeaderName = name.parse()
                .map_err(|e| Error::Internal(format!("invalid header name {name}: {e}")))?;
            let header_value: http::HeaderValue = value.parse()
                .map_err(|e| Error::Internal(format!("invalid header value for {name}: {e}")))?;
            request.headers_mut().insert(header_name, header_value);
        }

        // 如已配置则添加 Sec-WebSocket-Protocol
        if let Some(proto) = self.client.ws_subprotocol() {
            use tokio_tungstenite::tungstenite::http;
            let header_value: http::HeaderValue = proto.parse()
                .map_err(|e| Error::Internal(format!("invalid subprotocol: {e}")))?;
            request.headers_mut().insert("Sec-WebSocket-Protocol", header_value);
        }

        tracing::info!(
            account_id = %self.client.account_id(),
            url = %url,
            symbols = ?self.symbols,
            "connecting to tick stream"
        );

        // 连接
        let (ws_stream, _) = connect_async(request).await?;
        let (mut write, mut read) = ws_stream.split();

        // 订阅交易品种
        let subscribe_msg = TickSubscribeMsg {
            id: format!("ticks-{}", chrono::Utc::now().timestamp_millis()),
            subscribe: TickSubscribe {
                event: "ticks".to_owned(),
                instruments: self.symbols.clone(),
            },
        };
        let msg_text = serde_json::to_string(&subscribe_msg)
            .map_err(|e| Error::Internal(format!("serialize subscribe: {e}")))?;
        write.send(Message::Text(msg_text)).await?;

        tracing::info!(
            account_id = %self.client.account_id(),
            "tick stream subscribed"
        );

        // 启动心跳任务（每 30 秒发送 Ping）
        let mut heartbeat = interval(Duration::from_secs(30));

        loop {
            tokio::select! {
                _ = heartbeat.tick() => {
                    if let Err(e) = write.send(Message::Ping(vec![])).await {
                        tracing::warn!(account_id = %self.client.account_id(), error = %e, "heartbeat ping failed");
                        return Err(e.into());
                    }
                }

                msg = read.next() => {
                    match msg {
                        Some(Ok(Message::Text(text))) => {
                            if let Ok(tick) = serde_json::from_str::<Tick>(&text) {
                                on_tick(tick);
                            } else if text.contains("\"code\":200") || text.contains("\"code\":\"200\"") {
                                tracing::info!(account_id = %self.client.account_id(), "tick stream subscription acknowledged");
                            } else {
                                tracing::warn!(
                                    account_id = %self.client.account_id(),
                                    text = %text,
                                    "unrecognized tick stream message"
                                );
                            }
                        }
                        Some(Ok(Message::Close(_))) => {
                            tracing::info!(account_id = %self.client.account_id(), "tick stream closed by server");
                            break;
                        }
                        Some(Ok(Message::Pong(_))) => {}
                        Some(Ok(_)) => {}
                        Some(Err(e)) => {
                            tracing::error!(account_id = %self.client.account_id(), error = %e, "websocket error");
                            return Err(e.into());
                        }
                        None => {
                            tracing::info!(account_id = %self.client.account_id(), "tick stream ended");
                            break;
                        }
                    }
                }
            }
        }

        Ok(())
    }
}

/// 事件流客户端（交易事件、账户状态等）。
pub struct EventStreamClient {
    client: Arc<Client>,
    event_types: Vec<String>,
    instruments: Vec<String>,
}

impl EventStreamClient {
    pub fn new(client: Arc<Client>, event_types: Vec<String>) -> Self {
        Self { client, event_types, instruments: Vec::new() }
    }

    pub fn with_instruments(client: Arc<Client>, event_types: Vec<String>, instruments: Vec<String>) -> Self {
        Self { client, event_types, instruments }
    }

    /// 连接并订阅事件流，每收到一个事件调用 `on_event`。
    pub async fn run<F>(&self, mut on_event: F) -> Result<()>
    where
        F: FnMut(WsEvent) + Send + 'static,
    {
        let mut retry_delay = Duration::from_secs(1);
        const MAX_RETRY_DELAY: Duration = Duration::from_secs(60);

        loop {
            match self.connect_and_stream(&mut on_event, &mut || {}).await {
                Ok(_) => {
                    tracing::info!(account_id = %self.client.account_id(), "event stream ended normally");
                    retry_delay = Duration::from_secs(1);
                }
                Err(e) => {
                    tracing::error!(
                        account_id = %self.client.account_id(),
                        error = %e,
                        retry_in_secs = retry_delay.as_secs(),
                        "event stream error, reconnecting"
                    );
                    sleep(retry_delay).await;
                    retry_delay = (retry_delay * 2).min(MAX_RETRY_DELAY);
                }
            }
        }
    }

    /// Connect and resubscribe after reconnects, notifying the caller after every successful subscription.
    pub async fn run_with_connected<F, C>(&self, mut on_event: F, mut on_connected: C) -> Result<()>
    where
        F: FnMut(WsEvent) + Send + 'static,
        C: FnMut() + Send + 'static,
    {
        let mut retry_delay = Duration::from_secs(1);
        const MAX_RETRY_DELAY: Duration = Duration::from_secs(60);
        loop {
            match self.connect_and_stream(&mut on_event, &mut on_connected).await {
                Ok(_) => retry_delay = Duration::from_secs(1),
                Err(e) => {
                    tracing::error!(account_id = %self.client.account_id(), error = %e, retry_in_secs = retry_delay.as_secs(), "event stream error, reconnecting");
                    sleep(retry_delay).await;
                    retry_delay = (retry_delay * 2).min(MAX_RETRY_DELAY);
                }
            }
        }
    }

    async fn connect_and_stream<F, C>(&self, on_event: &mut F, on_connected: &mut C) -> Result<()>
    where
        F: FnMut(WsEvent),
        C: FnMut(),
    {
        let url = self.client.ws_url(WsStream::Events).await?;
        let mut request = url.as_str().into_client_request()?;

        let headers = self.client.sign_ws_handshake(WsStream::Events)?;
        for (name, value) in headers.iter() {
            use tokio_tungstenite::tungstenite::http;
            let header_name: http::HeaderName = name.parse()
                .map_err(|e| Error::Internal(format!("invalid header name {name}: {e}")))?;
            let header_value: http::HeaderValue = value.parse()
                .map_err(|e| Error::Internal(format!("invalid header value for {name}: {e}")))?;
            request.headers_mut().insert(header_name, header_value);
        }

        if let Some(proto) = self.client.ws_subprotocol() {
            use tokio_tungstenite::tungstenite::http;
            let header_value: http::HeaderValue = proto.parse()
                .map_err(|e| Error::Internal(format!("invalid subprotocol: {e}")))?;
            request.headers_mut().insert("Sec-WebSocket-Protocol", header_value);
        }

        tracing::info!(
            account_id = %self.client.account_id(),
            url = %url,
            event_types = ?self.event_types,
            "connecting to event stream"
        );

        let (ws_stream, _) = connect_async(request).await?;
        let (mut write, mut read) = ws_stream.split();

        // 订阅各事件类型
        for event_type in &self.event_types {
            let subscribe_msg = EventSubscribeMsg {
                id: format!("{}-{}", event_type, chrono::Utc::now().timestamp_millis()),
                subscribe: EventSubscribe {
                    event: event_type.clone(),
                    instruments: (event_type == "instruments").then(|| self.instruments.clone()),
                },
            };
            let msg_text = serde_json::to_string(&subscribe_msg)
                .map_err(|e| Error::Internal(format!("serialize subscribe: {e}")))?;
            write.send(Message::Text(msg_text)).await?;
        }

        tracing::info!(
            account_id = %self.client.account_id(),
            "event stream subscribed"
        );
        on_connected();

        let mut heartbeat = interval(Duration::from_secs(30));

        loop {
            tokio::select! {
                _ = heartbeat.tick() => {
                    if let Err(e) = write.send(Message::Ping(vec![])).await {
                        tracing::warn!(account_id = %self.client.account_id(), error = %e, "heartbeat ping failed");
                        return Err(e.into());
                    }
                }

                msg = read.next() => {
                    match msg {
                        Some(Ok(Message::Text(text))) => {
                            if text.contains("\"code\":200") || text.contains("\"code\":\"200\"") {
                                // 订阅确认；不是事件
                            } else {
                                match serde_json::from_str::<WsEvent>(&text) {
                                    Ok(event) => on_event(event),
                                    Err(e) => {
                                        tracing::warn!(
                                            account_id = %self.client.account_id(),
                                            error = %e,
                                            text = %text,
                                            "failed to parse event message"
                                        );
                                    }
                                }
                            }
                        }
                        Some(Ok(Message::Close(_))) => {
                            tracing::info!(account_id = %self.client.account_id(), "event stream closed by server");
                            break;
                        }
                        Some(Ok(Message::Pong(_))) => {}
                        Some(Ok(_)) => {}
                        Some(Err(e)) => {
                            tracing::error!(account_id = %self.client.account_id(), error = %e, "websocket error");
                            return Err(e.into());
                        }
                        None => {
                            tracing::info!(account_id = %self.client.account_id(), "event stream ended");
                            break;
                        }
                    }
                }
            }
        }

        Ok(())
    }
}
