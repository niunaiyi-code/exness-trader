//! 基于滑动窗口的频率限制中间件。

use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

/// 用户请求记录
#[derive(Debug, Clone)]
struct RequestRecord {
    /// 时间戳列表（滑动窗口）
    timestamps: Vec<Instant>,
}

/// 频率限制器
#[derive(Clone)]
pub struct RateLimiter {
    /// 用户ID -> 请求记录
    records: Arc<Mutex<HashMap<String, RequestRecord>>>,
    /// 时间窗口内允许的最大请求数
    max_requests: usize,
    /// 时间窗口大小
    window: Duration,
}

impl RateLimiter {
    /// 创建新的频率限制器
    pub fn new(max_requests: usize, window: Duration) -> Self {
        Self {
            records: Arc::new(Mutex::new(HashMap::new())),
            max_requests,
            window,
        }
    }

    /// 检查是否允许请求
    pub async fn check(&self, user_id: &str) -> bool {
        let mut records = self.records.lock().await;
        let now = Instant::now();
        let cutoff = now - self.window;

        let record = records.entry(user_id.to_string()).or_insert(RequestRecord {
            timestamps: Vec::new(),
        });

        // 移除过期的时间戳
        record.timestamps.retain(|&t| t > cutoff);

        // 检查是否超过限制
        if record.timestamps.len() >= self.max_requests {
            return false;
        }

        // 记录本次请求
        record.timestamps.push(now);
        true
    }
}

/// 频率限制中间件
pub async fn rate_limit_middleware(
    State(state): State<Arc<crate::api::routes::AppState>>,
    request: Request,
    next: Next,
) -> Response {
    // 从请求扩展中获取用户ID（由 auth_middleware 设置）
    let user_id = request
        .extensions()
        .get::<crate::auth::Claims>()
        .map(|claims| claims.sub.clone())
        .unwrap_or_else(|| "anonymous".to_string());

    // 检查频率限制
    if !state.rate_limiter.check(&user_id).await {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(crate::api::routes::ErrorResponse {
                error: "Rate limit exceeded. Please try again later.".to_string(),
            }),
        )
            .into_response();
    }

    next.run(request).await
}
