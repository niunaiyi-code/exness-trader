//! 受保护路由的 JWT 中间件。

use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
    Json,
};
use std::sync::Arc;

use crate::{
    api::ErrorResponse,
    auth::verify_jwt,
    config::ServerConfig,
};

/// 从 Authorization 头提取并校验 JWT。
pub async fn auth_middleware(
    State(config): State<Arc<ServerConfig>>,
    mut request: Request,
    next: Next,
) -> Result<Response, (StatusCode, Json<ErrorResponse>)> {
    let auth_header = request
        .headers()
        .get("Authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "));

    let token = auth_header.ok_or((
        StatusCode::UNAUTHORIZED,
        Json(ErrorResponse {
            error: "Missing or invalid Authorization header".to_string(),
        }),
    ))?;

    let claims = verify_jwt(token, &config.auth.jwt_secret).map_err(|e| {
        (
            StatusCode::UNAUTHORIZED,
            Json(ErrorResponse {
                error: format!("Invalid token: {}", e),
            }),
        )
    })?;

    // 将 claims 注入请求扩展，供下游处理器使用。
    request.extensions_mut().insert(claims);

    Ok(next.run(request).await)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::create_jwt;

    #[test]
    fn jwt_roundtrip() {
        let secret = "test_secret";
        let token = create_jwt("user_123", "admin", secret, 24).unwrap();
        let claims = verify_jwt(&token, secret).unwrap();
        assert_eq!(claims.sub, "user_123");
        assert_eq!(claims.role, "admin");
    }
}
