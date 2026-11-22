//! 实时事件 WebSocket hub（docs/06 §2）。

use axum::{
    extract::ws::{Message, WebSocket},
    extract::{Query, State, WebSocketUpgrade},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::broadcast;

use crate::api::routes::AppState;
use crate::db::repository::{AccountRepository, AccountUserRepository};

#[derive(Debug, Deserialize, Default)]
pub struct WsQuery {
    pub account_id: Option<String>,
}

/// 从 `Sec-WebSocket-Protocol` 头提取 Token。
/// 约定子协议格式为 `bearer.<jwt>`，前端连接时通过 `new WebSocket(url, ["bearer.<jwt>"])` 传递，
/// 避免 JWT 明文进入 URL（会被反向代理/访问日志记录）。
fn extract_ws_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(axum::http::header::SEC_WEBSOCKET_PROTOCOL)
        .and_then(|v| v.to_str().ok())
        .and_then(|raw| raw.split(',').map(str::trim).find(|p| p.starts_with("bearer.")))
        .and_then(|p| p.strip_prefix("bearer."))
        .filter(|t| !t.is_empty())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum WsFrame {
    #[serde(rename = "tick")]
    Tick {
        account_id: String,
        #[serde(default)]
        symbol: Option<String>,
        #[serde(default)]
        currency: Option<String>,
        bid: String,
        ask: String,
        timestamp: u64,
    },
    #[serde(rename = "event")]
    Event {
        account_id: String,
        event_type: String,
        data: serde_json::Value,
    },
    #[serde(rename = "alert")]
    Alert {
        account_id: String,
        message: String,
        severity: String,
    },
    #[serde(rename = "runner_status")]
    RunnerStatus {
        account_id: String,
        status: String,
    },
    #[serde(rename = "pong")]
    Pong,
}

pub struct WsHub {
    tx: broadcast::Sender<WsFrame>,
}

impl WsHub {
    pub fn new() -> Self {
        let (tx, _rx) = broadcast::channel(1000);
        Self { tx }
    }

    pub fn broadcast(&self, frame: WsFrame) {
        let _ = self.tx.send(frame);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<WsFrame> {
        self.tx.subscribe()
    }
}

/// 处理来自路由、携带 AppState 的 WebSocket 升级。
pub async fn ws_route_handler(
    ws: WebSocketUpgrade,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
    State(state): State<Arc<AppState>>,
) -> Response {
    // 1. 严格要求身份认证，拒绝匿名连接。
    //    Token 仅通过 Sec-WebSocket-Protocol 子协议传递（避免 JWT 明文进入 URL 被访问日志记录）。
    let token = match extract_ws_token(&headers) {
        Some(t) => t.to_string(),
        None => {
            tracing::warn!("WebSocket connection rejected: missing auth token");
            return (
                StatusCode::UNAUTHORIZED,
                "Unauthorized: missing token",
            ).into_response();
        }
    };

    let claims = match crate::auth::verify_jwt(&token, &state.config.auth.jwt_secret) {
        Ok(claims) => claims,
        Err(e) => {
            tracing::warn!(error = %e, "WebSocket authentication failed: invalid token");
            return (
                StatusCode::UNAUTHORIZED,
                "Unauthorized: invalid token",
            ).into_response();
        }
    };

    let target_account_id = query.account_id;

    // 2. 校验用户对目标账户的访问授权（非管理员访问具体账户时必须在数据库中拥有授权）
    if let Some(ref target) = target_account_id {
        if claims.role != "admin" && target != "admin" {
            let account_res = match AccountRepository::find_by_id(&state.db, target).await {
                Ok(Some(a)) => Ok(Some(a)),
                _ => AccountRepository::find_by_account_id(&state.db, target).await,
            };

            let account = match account_res {
                Ok(Some(a)) => a,
                _ => {
                    tracing::warn!(user_id = %claims.sub, target = %target, "WebSocket forbidden: account not found");
                    return (
                        StatusCode::FORBIDDEN,
                        "Forbidden: account not found",
                    ).into_response();
                }
            };

            match AccountUserRepository::find_permission(&state.db, &account.id, &claims.sub).await {
                Ok(Some(_)) => {}
                _ => {
                    tracing::warn!(user_id = %claims.sub, account_id = %account.account_id, "WebSocket forbidden: insufficient permissions");
                    return (
                        StatusCode::FORBIDDEN,
                        "Forbidden: insufficient permissions for this account",
                    ).into_response();
                }
            }
        }
    }

    let user_role = Some(claims.role);
    let hub = state.ws_hub.clone();
    ws.on_upgrade(move |socket| handle_socket(socket, hub, target_account_id, user_role))
}

async fn handle_socket(
    mut socket: WebSocket,
    hub: Arc<WsHub>,
    target_account_id: Option<String>,
    user_role: Option<String>,
) {
    let mut rx = hub.subscribe();

    loop {
        tokio::select! {
            frame = rx.recv() => {
                match frame {
                    Ok(f) => {
                        let should_send = match &f {
                            // 行情 Tick 是公开市场报价数据，所有客户端均需接收以渲染图表、盘口与实时盈亏
                            WsFrame::Tick { .. } => true,
                            WsFrame::Event { account_id, .. } => {
                                if user_role.is_none() {
                                    false
                                } else if user_role.as_deref() == Some("admin") {
                                    match &target_account_id {
                                        Some(target) => target == "admin" || target == account_id,
                                        None => true,
                                    }
                                } else {
                                    match &target_account_id {
                                        Some(target) => target == account_id,
                                        None => false,
                                    }
                                }
                            }
                            WsFrame::Alert { account_id, .. } => {
                                if user_role.is_none() {
                                    false
                                } else if user_role.as_deref() == Some("admin") {
                                    match &target_account_id {
                                        Some(target) => target == "admin" || target == account_id,
                                        None => true,
                                    }
                                } else {
                                    match &target_account_id {
                                        Some(target) => target == account_id,
                                        None => false,
                                    }
                                }
                            }
                            WsFrame::RunnerStatus { account_id, .. } => {
                                if user_role.as_deref() == Some("admin") {
                                    match &target_account_id {
                                        Some(target) => target == "admin" || target == account_id,
                                        None => true,
                                    }
                                } else {
                                    match &target_account_id {
                                        Some(target) => target == account_id,
                                        None => true,
                                    }
                                }
                            }
                            WsFrame::Pong => true,
                        };

                        if should_send {
                            match serde_json::to_string(&f) {
                                Ok(json) => {
                                    if socket.send(Message::Text(json)).await.is_err() {
                                        break;
                                    }
                                }
                                Err(e) => {
                                    tracing::error!("Failed to serialize WS frame: {}", e);
                                    continue;
                                }
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                        tracing::warn!(target = ?target_account_id, skipped = skipped, "WS broadcast subscriber lagged; skipped messages");
                        continue;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        if text.contains("ping") {
                            if let Ok(pong) = serde_json::to_string(&WsFrame::Pong) {
                                let _ = socket.send(Message::Text(pong)).await;
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ws_hub_broadcast() {
        let hub = WsHub::new();
        let mut rx = hub.subscribe();

        hub.broadcast(WsFrame::Tick {
            account_id: "acc_123".to_string(),
            symbol: None,
            currency: None,
            bid: "3700.50".to_string(),
            ask: "3701.00".to_string(),
            timestamp: 1000,
        });

        let frame = rx.try_recv().unwrap();
        match frame {
            WsFrame::Tick { account_id, .. } => assert_eq!(account_id, "acc_123"),
            _ => panic!("unexpected frame"),
        }
    }
}
