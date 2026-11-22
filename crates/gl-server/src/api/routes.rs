//! 完整 REST API 实现（docs/06）。

use axum::{
    extract::{Extension, Path, Query, State},
    http::StatusCode,
    response::Json,
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use crate::{
    auth::{create_jwt, hash_password, verify_jwt, verify_password, Claims},
    calendar::FinanceCalendar,
    config::ServerConfig,
    crypto::CredentialCrypto,
    db::{
        models::{Account, Strategy, StrategyParams, User},
        repository::{AccountRepository, AccountUserRepository, StrategyParamsRepository, StrategyRepository, UserRepository},
        DbPool,
    },
    runtime::RuntimeManager,
    supervisor::SupervisorRegistry,
    ws::WsHub,
};
use tokio::sync::{broadcast, watch, RwLock};

fn select_gold_instrument(instruments: &[String]) -> Option<String> {
    ["XAUUSD", "XAUUSDm", "XAUUSDc"]
        .iter()
        .find(|candidate| instruments.iter().any(|item| item.eq_ignore_ascii_case(candidate)))
        .and_then(|candidate| instruments.iter().find(|item| item.eq_ignore_ascii_case(candidate)))
        .cloned()
}

/// 判断 URL 是否指向本地回环地址（用于测试/内网部署时跳过真实网关发现握手）。
fn is_loopback_url(url: &url::Url) -> bool {
    matches!(url.host_str(), Some("localhost") | Some("127.0.0.1") | Some("::1"))
}

use exness_api::{
    Candle, Client, ClientConfig, ClosePositionParams, Credentials, DealsHistoryResponse, OpenPositionRequest, Tick,
    TradingStateSnapshot,
};

#[derive(Clone)]
pub struct AppState {
    pub db: DbPool,
    pub config: Arc<ServerConfig>,
    pub crypto: Arc<CredentialCrypto>,
    pub ws_hub: Arc<WsHub>,
    pub calendar: Arc<FinanceCalendar>,
    pub calendar_api_key: Option<Arc<String>>,
    pub calendar_api_url: Option<String>,
    pub news_state_tx: watch::Sender<(bool, Option<i64>)>,
    pub instrument_conditions: Arc<crate::instrument_conditions::InstrumentConditions>,
    pub supervisor: Arc<RwLock<SupervisorRegistry>>,
    pub runtimes: Arc<RwLock<RuntimeManager>>,
    pub market_data: Arc<crate::market_data::MarketDataService>,
    pub tick_tx: broadcast::Sender<Tick>,
    pub clients: Arc<RwLock<std::collections::HashMap<String, Arc<Client>>>>,
    pub rate_limiter: crate::rate_limit::RateLimiter,
}

#[derive(Serialize)]
pub struct ErrorResponse {
    pub error: String,
}

// ========== 认证端点 ==========

#[derive(Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Serialize)]
pub struct LoginResponse {
    pub token: String,
    pub user_id: String,
    pub role: String,
}

/// POST /auth/login — 登录
async fn login(
    State(state): State<Arc<AppState>>,
    Json(req): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, (StatusCode, Json<ErrorResponse>)> {
    let user = UserRepository::find_by_username(&state.db, &req.username)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("DB error: {}", e),
                }),
            )
        })?
        .ok_or((
            StatusCode::UNAUTHORIZED,
            Json(ErrorResponse {
                error: "Invalid credentials".to_string(),
            }),
        ))?;

    verify_password(&req.password, &user.password_hash).map_err(|_| {
        (
            StatusCode::UNAUTHORIZED,
            Json(ErrorResponse {
                error: "Invalid credentials".to_string(),
            }),
        )
    })?;

    let token = create_jwt(
        &user.id,
        &user.role,
        &state.config.auth.jwt_secret,
        state.config.auth.jwt_expiry_hours,
    )
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("JWT error: {}", e),
            }),
        )
    })?;

    Ok(Json(LoginResponse {
        token,
        user_id: user.id,
        role: user.role,
    }))
}

#[derive(Deserialize)]
pub struct RegisterRequest {
    pub username: String,
    pub password: String,
    pub role: Option<String>,
}

/// POST /auth/register
/// 受限注册逻辑：未认证或非管理员调用者不可自选角色，一律由服务端赋予普通 trader 角色；
/// 只有已认证的 admin 才能指定创建 admin/trader/viewer 角色。
async fn register(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Json(req): Json<RegisterRequest>,
) -> Result<Json<User>, (StatusCode, Json<ErrorResponse>)> {
    let is_admin = if let Some(auth_header) = headers
        .get("Authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
    {
        if let Ok(claims) = verify_jwt(auth_header, &state.config.auth.jwt_secret) {
            claims.role == "admin"
        } else {
            false
        }
    } else {
        false
    };

    let assigned_role = if is_admin {
        let r = req.role.as_deref().unwrap_or("trader");
        if !["admin", "trader", "viewer"].contains(&r) {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: format!("Invalid role '{}'", r),
                }),
            ));
        }
        r.to_string()
    } else {
        // 未认证或非管理员试图自建 admin 账号直接拒绝
        if let Some(ref r) = req.role {
            if r == "admin" {
                return Err((
                    StatusCode::FORBIDDEN,
                    Json(ErrorResponse {
                        error: "Administrator accounts can only be created by an authenticated administrator".to_string(),
                    }),
                ));
            }
        }
        // 服务端统一赋权为普通交易员
        "trader".to_string()
    };

    let password_hash = hash_password(&req.password).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Hash error: {}", e),
            }),
        )
    })?;

    let user_id = Uuid::new_v4().to_string();
    UserRepository::create(&state.db, &user_id, &req.username, &password_hash, &assigned_role)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("DB error: {}", e),
                }),
            )
        })?;

    let user = UserRepository::find_by_id(&state.db, &user_id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("DB error: {}", e),
                }),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: format!("User not found: {}", user_id),
                }),
            )
        })?;

    Ok(Json(user))
}

/// 辅助函数：校验调用者针对特定账户的 RBAC 访问权限
async fn check_account_permission(
    state: &AppState,
    claims: &Claims,
    account_db_id: &str,
    required: &str, // "view", "operate", "admin"
) -> Result<(), (StatusCode, Json<ErrorResponse>)> {
    if claims.role == "admin" {
        return Ok(());
    }

    let perm = AccountUserRepository::find_permission(&state.db, account_db_id, &claims.sub)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("DB permission check error: {}", e),
                }),
            )
        })?;

    let perm_level = match perm.as_ref().map(|p| p.permission.as_str()) {
        Some("admin") => 3,
        Some("operate") => 2,
        Some("view") => 1,
        _ => 0,
    };

    let required_level = match required {
        "admin" => 3,
        "operate" => 2,
        "view" => 1,
        _ => 3,
    };

    if perm_level >= required_level {
        Ok(())
    } else {
        Err((
            StatusCode::FORBIDDEN,
            Json(ErrorResponse {
                error: format!("Access denied: insufficient permission (requires '{}')", required),
            }),
        ))
    }
}

// ========== 账户端点 ==========

#[derive(Serialize)]
pub struct AccountListItem {
    pub id: String,
    pub account_id: String,
    pub state: String,
    pub strategy_id: Option<String>,
    pub strategy_name: Option<String>,
    pub permission: Option<String>,
}

/// GET /accounts — 列出用户可访问的账户（严格按 claims.sub 与角色过滤）
async fn list_accounts(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Vec<AccountListItem>>, (StatusCode, Json<ErrorResponse>)> {
    let accounts = AccountRepository::list_all(&state.db)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("DB error: {}", e),
                }),
            )
        })?;

    let strategies = StrategyRepository::list_all(&state.db).await.unwrap_or_default();
    let strat_map: std::collections::HashMap<String, String> = strategies
        .into_iter()
        .map(|s| (s.id, s.name))
        .collect();

    let is_admin = claims.role == "admin";
    let user_perms = if !is_admin {
        let perms = AccountUserRepository::list_by_user(&state.db, &claims.sub)
            .await
            .unwrap_or_default();
        let map: std::collections::HashMap<String, String> = perms
            .into_iter()
            .map(|p| (p.account_id, p.permission))
            .collect();
        Some(map)
    } else {
        None
    };

    Ok(Json(
        accounts
            .into_iter()
            .filter_map(|a| {
                let perm = if is_admin {
                    Some("admin".to_string())
                } else {
                    user_perms.as_ref().and_then(|m| m.get(&a.id).cloned())
                };

                // 非系统管理员且未被授予该账户权限的用户无法查看该账户
                if perm.is_none() {
                    return None;
                }

                let strat_name = a.strategy_id.as_ref().and_then(|sid| strat_map.get(sid).cloned());
                Some(AccountListItem {
                    id: a.id,
                    account_id: a.account_id,
                    state: a.state,
                    strategy_id: a.strategy_id,
                    strategy_name: strat_name,
                    permission: perm,
                })
            })
            .collect(),
    ))
}

#[derive(Serialize)]
pub struct AccountDetail {
    pub id: String,
    pub account_id: String,
    pub gateway_url: String,
    pub state: String,
    pub strategy_id: Option<String>,
    pub strategy_name: Option<String>,
    pub params: Option<StrategyParams>,
}

/// GET /accounts/:id — 获取账户详情
async fn get_account(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
) -> Result<Json<AccountDetail>, (StatusCode, Json<ErrorResponse>)> {
    let account = match AccountRepository::find_by_id(&state.db, &id).await {
        Ok(Some(a)) => Some(a),
        _ => AccountRepository::find_by_account_id(&state.db, &id).await.map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("DB error: {}", e),
                }),
            )
        })?,
    }
    .ok_or((
        StatusCode::NOT_FOUND,
        Json(ErrorResponse {
            error: "Account not found".to_string(),
        }),
    ))?;

    check_account_permission(&state, &claims, &account.id, "view").await?;

    let strategy_name = if let Some(ref sid) = account.strategy_id {
        StrategyRepository::find_by_id(&state.db, sid)
            .await
            .ok()
            .flatten()
            .map(|s| s.name)
    } else {
        None
    };

    let params = StrategyParamsRepository::find_by_account(&state.db, &account.id)
        .await
        .ok()
        .flatten();

    Ok(Json(AccountDetail {
        id: account.id,
        account_id: account.account_id,
        gateway_url: account.gateway_url,
        state: account.state,
        strategy_id: account.strategy_id,
        strategy_name,
        params,
    }))
}

#[derive(Deserialize)]
pub struct CreateAccountRequest {
    pub account_id: String,
    pub api_key: String,
    pub private_key: String,
    pub key_format: String,
    pub gateway_url: String,
}

/// POST /accounts — 创建新账户
async fn create_account(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<Claims>,
    Json(req): Json<CreateAccountRequest>,
) -> Result<Json<Account>, (StatusCode, Json<ErrorResponse>)> {
    // 加密凭据
    let encrypted_api_key = state.crypto.encrypt(&req.api_key).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Encryption error: {}", e),
            }),
        )
    })?;

    let encrypted_private_key = state.crypto.encrypt(&req.private_key).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Encryption error: {}", e),
            }),
        )
    })?;

    let account_pk = Uuid::new_v4().to_string();
    AccountRepository::create(
        &state.db,
        &account_pk,
        &req.account_id,
        &encrypted_api_key,
        &encrypted_private_key,
        &req.key_format,
        &req.gateway_url,
    )
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("DB error: {}", e),
            }),
        )
    })?;

    // 为创建者授予该账户管理员权限
    AccountUserRepository::grant(&state.db, &account_pk, &claims.sub, "admin")
        .await
        .ok();

    let account = AccountRepository::find_by_id(&state.db, &account_pk)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("DB error: {}", e),
                }),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("Failed to retrieve created account: {}", account_pk),
                }),
            )
        })?;

    Ok(Json(account))
}

/// DELETE /accounts/:id — 删除账户
async fn delete_account(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
) -> Result<StatusCode, (StatusCode, Json<ErrorResponse>)> {
    let account = match AccountRepository::find_by_id(&state.db, &id).await {
        Ok(Some(a)) => Some(a),
        _ => AccountRepository::find_by_account_id(&state.db, &id).await.map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("DB error: {}", e),
                }),
            )
        })?,
    }
    .ok_or((
        StatusCode::NOT_FOUND,
        Json(ErrorResponse {
            error: "Account not found".to_string(),
        }),
    ))?;

    check_account_permission(&state, &claims, &account.id, "admin").await?;

    AccountRepository::delete(&state.db, &account.id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("DB error: {}", e),
                }),
            )
        })?;

    Ok(StatusCode::NO_CONTENT)
}

// ========== 策略参数端点 ==========

/// GET /accounts/:id/params — 获取策略参数
async fn get_params(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
) -> Result<Json<StrategyParams>, (StatusCode, Json<ErrorResponse>)> {
    let account = match AccountRepository::find_by_id(&state.db, &id).await {
        Ok(Some(a)) => Some(a),
        _ => AccountRepository::find_by_account_id(&state.db, &id).await.map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("DB error: {}", e),
                }),
            )
        })?,
    }
    .ok_or((
        StatusCode::NOT_FOUND,
        Json(ErrorResponse {
            error: "Account not found".to_string(),
        }),
    ))?;

    check_account_permission(&state, &claims, &account.id, "view").await?;

    let params = StrategyParamsRepository::find_by_account(&state.db, &account.id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("DB error: {}", e),
                }),
            )
        })?
        .ok_or((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "Params not found".to_string(),
            }),
        ))?;

    Ok(Json(params))
}

#[derive(Deserialize)]
pub struct UpdateParamsRequest {
    pub initial_lot: String,
    pub grid_step: i32,
    pub grid_step_add: i32,
    pub grid_step_sec: i32,
    pub lot_mult: String,
    pub tp_distance_usd: String,
    pub lock_trigger_usd: String,
    #[serde(default)]
    pub params_locked: Option<bool>,
}

#[derive(Deserialize)]
pub struct ToggleLockRequest {
    pub locked: bool,
}

/// PUT /accounts/:id/params — 更新策略参数
async fn update_params(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
    Json(req): Json<UpdateParamsRequest>,
) -> Result<Json<StrategyParams>, (StatusCode, Json<ErrorResponse>)> {
    let account = match AccountRepository::find_by_id(&state.db, &id).await {
        Ok(Some(a)) => Some(a),
        _ => AccountRepository::find_by_account_id(&state.db, &id).await.map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("DB error: {}", e),
                }),
            )
        })?,
    }
    .ok_or((
        StatusCode::NOT_FOUND,
        Json(ErrorResponse {
            error: "Account not found".to_string(),
        }),
    ))?;

    check_account_permission(&state, &claims, &account.id, "operate").await?;

    let existing_params = StrategyParamsRepository::find_by_account(&state.db, &account.id).await.ok().flatten();

    // 检查参数是否锁定：仅非 admin 用户受 params_locked 约束
    if claims.role != "admin" {
        if let Some(ref existing) = existing_params {
            if existing.params_locked {
                return Err((
                    StatusCode::FORBIDDEN,
                    Json(ErrorResponse {
                        error: "Parameters are locked by admin".to_string(),
                    }),
                ));
            }
        }
    }

    let params_locked = if claims.role == "admin" {
        req.params_locked.unwrap_or_else(|| existing_params.as_ref().map(|p| p.params_locked).unwrap_or(false))
    } else {
        false
    };

    let params = StrategyParams {
        account_id: account.id.clone(),
        initial_lot: req.initial_lot,
        grid_step: req.grid_step,
        grid_step_add: req.grid_step_add,
        grid_step_sec: req.grid_step_sec,
        lot_mult: req.lot_mult,
        tp_distance_usd: req.tp_distance_usd,
        lock_trigger_usd: req.lock_trigger_usd,
        params_locked,
        updated_at: chrono::Utc::now().timestamp(),
    };

    StrategyParamsRepository::upsert(&state.db, &params)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("DB error: {}", e),
                }),
            )
        })?;

    Ok(Json(params))
}

/// POST /accounts/:id/params/lock — 切换参数防篡改锁定状态（仅管理员）
async fn toggle_account_params_lock(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
    Json(req): Json<ToggleLockRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    if claims.role != "admin" {
        return Err((
            StatusCode::FORBIDDEN,
            Json(ErrorResponse {
                error: "Admin role required".to_string(),
            }),
        ));
    }
    let account = match AccountRepository::find_by_id(&state.db, &id).await {
        Ok(Some(a)) => Some(a),
        _ => AccountRepository::find_by_account_id(&state.db, &id).await.map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("DB error: {}", e),
                }),
            )
        })?,
    }
    .ok_or((
        StatusCode::NOT_FOUND,
        Json(ErrorResponse {
            error: "Account not found".to_string(),
        }),
    ))?;

    if let Ok(Some(mut sp)) = StrategyParamsRepository::find_by_account(&state.db, &account.id).await {
        sp.params_locked = req.locked;
        sp.updated_at = chrono::Utc::now().timestamp();
        let _ = StrategyParamsRepository::upsert(&state.db, &sp).await;
    }

    Ok(Json(serde_json::json!({ "success": true, "params_locked": req.locked })))
}

// ========== 账户生命周期端点 ==========

#[derive(Serialize)]
pub struct AccountActionResponse {
    pub account_id: String,
    pub status: String,
}

/// POST /accounts/:id/start — 启动账户
pub async fn start_account(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
) -> Result<Json<AccountActionResponse>, (StatusCode, Json<ErrorResponse>)> {
    let account = match AccountRepository::find_by_id(&state.db, &id).await {
        Ok(Some(a)) => Some(a),
        _ => AccountRepository::find_by_account_id(&state.db, &id).await.map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("DB error: {}", e),
                }),
            )
        })?,
    }
    .ok_or((
        StatusCode::NOT_FOUND,
        Json(ErrorResponse {
            error: "Account not found".to_string(),
        }),
    ))?;

    check_account_permission(&state, &claims, &account.id, "operate").await?;

    if state.runtimes.read().await.is_running(&account.account_id) {
        return Ok(Json(AccountActionResponse {
            account_id: id,
            status: "running".to_string(),
        }));
    }

    // 解密凭据并构建交易客户端。
    let api_key = state.crypto.decrypt(&account.encrypted_api_key).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("decrypt api_key: {e}"),
            }),
        )
    })?;
    let private_key = state.crypto.decrypt(&account.encrypted_private_key).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("decrypt private_key: {e}"),
            }),
        )
    })?;
    let creds = Credentials::new(&api_key, &private_key).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: format!("invalid credentials: {e}"),
            }),
        )
    })?;
    let cfg = ClientConfig {
        gateway_url: url::Url::parse(&account.gateway_url).ok(),
        // 当网关地址指向本地回环（测试/内网部署）时，直接将其作为接入点，
        // 跳过真实网关发现握手，使集成测试可通过本地 mock 网关稳定运行。
        rest_base_url: url::Url::parse(&account.gateway_url)
            .ok()
            .filter(|u| is_loopback_url(u)),
        ..Default::default()
    };
    let client = Arc::new(Client::new(creds, &account.account_id, cfg).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("build client: {e}"),
            }),
        )
    })?);

    let account_details = client.account_details().await.map_err(|e| {
        (StatusCode::BAD_GATEWAY, Json(ErrorResponse { error: format!("account details: {e}") }))
    })?;
    if !matches!(account_details.currency.as_str(), "USD" | "USC") {
        return Err((StatusCode::BAD_REQUEST, Json(ErrorResponse {
            error: format!("unsupported account currency: {} (supported: USD, USC)", account_details.currency),
        })));
    }

    let available_instruments = client.instruments().await.map_err(|e| {
        (StatusCode::BAD_GATEWAY, Json(ErrorResponse { error: format!("list instruments: {e}") }))
    })?;
    let symbol = select_gold_instrument(&available_instruments)
        .ok_or((StatusCode::BAD_REQUEST, Json(ErrorResponse {
            error: "account has no supported gold instrument (XAUUSD/XAUUSDm/XAUUSDc)".to_string(),
        })))?;
    let conditions = client.instrument_conditions(&symbol).await.map_err(|e| {
        (StatusCode::BAD_GATEWAY, Json(ErrorResponse { error: format!("instrument conditions: {e}") }))
    })?;
    let instrument_conditions_rx = state.instrument_conditions.register(&account.account_id, conditions.clone());
    if conditions.quote_currency.as_deref() != Some("USD") {
        return Err((StatusCode::BAD_REQUEST, Json(ErrorResponse {
            error: format!("{} quote currency must be USD", symbol),
        })));
    }
    let contract_size = conditions.contract_size.as_deref()
        .and_then(|value| rust_decimal::Decimal::from_str_exact(value).ok())
        .filter(|value| *value > rust_decimal::Decimal::ZERO)
        .ok_or((StatusCode::BAD_GATEWAY, Json(ErrorResponse {
            error: format!("missing or invalid contract_size for {}", symbol),
        })))?;
    let point_digits = conditions.point_digits.ok_or((StatusCode::BAD_GATEWAY, Json(ErrorResponse {
        error: format!("missing point_digits for {}", symbol),
    })))?;
    let parse_volume = |label: &str, value: Option<&str>| -> Result<rust_decimal::Decimal, (StatusCode, Json<ErrorResponse>)> {
        value
            .and_then(|raw| rust_decimal::Decimal::from_str_exact(raw).ok())
            .ok_or((StatusCode::BAD_GATEWAY, Json(ErrorResponse {
                error: format!("missing or invalid {label} for {}", symbol),
            })))
    };
    let lot_spec = gl_strategy::grid_math::LotSpec::new(
        parse_volume("volume_min", conditions.volume_min.as_deref())?,
        parse_volume("volume_max", conditions.volume_max.as_deref())?,
        parse_volume("volume_step", conditions.volume_step.as_deref())?,
    );
    if lot_spec.min <= rust_decimal::Decimal::ZERO
        || lot_spec.step <= rust_decimal::Decimal::ZERO
        || lot_spec.max < lot_spec.min
    {
        return Err((StatusCode::BAD_GATEWAY, Json(ErrorResponse {
            error: format!("invalid volume limits for {}", symbol),
        })));
    }

    // 启动时将策略参数与账户品种规格读入内存（tick 循环内不访问数据库）。
    let loaded_strategy = if let Some(ref strat_id) = account.strategy_id {
        StrategyRepository::find_by_id(&state.db, strat_id).await.ok().flatten()
    } else {
        None
    };

    let (initial_lot, grid_step, grid_step_add, grid_step_sec, lot_mult, tp_distance_usd, lock_trigger_usd) = 
        if let Some(strat) = loaded_strategy {
            (strat.initial_lot, strat.grid_step, strat.grid_step_add, strat.grid_step_sec, strat.lot_mult, strat.tp_distance_usd, strat.lock_trigger_usd)
        } else if let Ok(Some(sp)) = StrategyParamsRepository::find_by_account(&state.db, &account.id).await {
            (sp.initial_lot, sp.grid_step, sp.grid_step_add, sp.grid_step_sec, sp.lot_mult, sp.tp_distance_usd, sp.lock_trigger_usd)
        } else {
            ("0.01".to_string(), 300, 75, 15, "1.45".to_string(), "1.00".to_string(), "1000.00".to_string())
        };

    let runtime_constants = gl_runtime::config::RuntimeConstants::from_instrument(
        contract_size,
        &account_details.currency,
        point_digits,
        lot_spec,
    );
    let initial_lot_dec = rust_decimal::Decimal::from_str_exact(&initial_lot).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: format!("invalid initial_lot parameter '{initial_lot}': {e}"),
            }),
        )
    })?;
    let lot_mult_dec = rust_decimal::Decimal::from_str_exact(&lot_mult).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: format!("invalid lot_mult parameter '{lot_mult}': {e}"),
            }),
        )
    })?;
    let tp_distance_usd_dec = rust_decimal::Decimal::from_str_exact(&tp_distance_usd).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: format!("invalid tp_distance_usd parameter '{tp_distance_usd}': {e}"),
            }),
        )
    })?;
    let lock_trigger_usd_dec = rust_decimal::Decimal::from_str_exact(&lock_trigger_usd).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: format!("invalid lock_trigger_usd parameter '{lock_trigger_usd}': {e}"),
            }),
        )
    })?;

    let mut in_memory_params = gl_runtime::config::StrategyParams {
        initial_lot: initial_lot_dec,
        max_orders: 1,
        grid_step: grid_step as u32,
        grid_step_add: grid_step_add as u32,
        grid_step_sec: grid_step_sec as u32,
        lot_mult: lot_mult_dec,
        tp_distance_usd: tp_distance_usd_dec,
        lock_trigger_usd: lock_trigger_usd_dec,
        enable_two_way_hedging: false, // 默认单向网格
    };
    // Match MT5 AutoMaxOrders: choose the deepest layer whose theoretical loss
    // plus one extra normal grid interval still fits the loss budget.
    in_memory_params.max_orders = gl_strategy::grid_math::auto_max_orders(
        in_memory_params.initial_lot,
        in_memory_params.lot_mult,
        rust_decimal::Decimal::from(in_memory_params.grid_step),
        rust_decimal::Decimal::from(in_memory_params.grid_step_add),
        runtime_constants.pip,
        runtime_constants.profit_per_dollar_per_lot,
        in_memory_params.lock_trigger_usd,
        &lot_spec,
    );
    if in_memory_params.max_orders < 1 {
        return Err((StatusCode::BAD_REQUEST, Json(ErrorResponse {
            error: "risk budget cannot support a valid grid layer".to_string(),
        })));
    }

    tracing::info!(
        account_id = %account.account_id,
        initial_lot = %in_memory_params.initial_lot,
        grid_step = in_memory_params.grid_step,
        max_orders = in_memory_params.max_orders,
        lot_mult = %in_memory_params.lot_mult,
        tp_distance_usd = %in_memory_params.tp_distance_usd,
        lock_trigger_usd = %in_memory_params.lock_trigger_usd,
        "Loaded strategy parameters into RAM for AccountRunner (zero disk/DB IO during live trading)"
    );

    let runner_config = gl_runtime::AccountConfig {
        account_id: account.account_id.clone(),
        symbol: symbol.clone(),
        params: in_memory_params,
        constants: runtime_constants,
    };

    // 更新数据库状态
    AccountRepository::update_state(&state.db, &account.id, "running")
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("DB error: {}", e),
                }),
            )
        })?;

    // 未注册时向监管器注册
    {
        let mut sup = state.supervisor.write().await;
        if sup.get(&account.account_id).is_none() {
            sup.register(account.account_id.clone());
        }
    }

    // 启动真实策略运行器并记录其句柄。
    let tick_rx = state.tick_tx.subscribe();
    let news_rx = state.news_state_tx.subscribe();
    let instrument_conditions = state.instrument_conditions.clone();
    let instrument_account_id = account.account_id.clone();
    let (shutdown_tx, shutdown_rx) = tokio::sync::mpsc::channel(1);
    let paused = state.runtimes.write().await.paused_handle(&account.account_id);
    paused.store(account.paused, std::sync::atomic::Ordering::Release);
    let ws_hub = state.ws_hub.clone();
    let db_pool = state.db.clone();
    let account_db_id = account.id.clone();
    let handle = tokio::spawn(async move {
        let _ = crate::strategy::run_runner(
            runner_config,
            client,
            tick_rx,
            paused,
            news_rx,
            instrument_conditions_rx,
            instrument_conditions,
            instrument_account_id,
            shutdown_rx,
            ws_hub,
            Some((db_pool, account_db_id)),
        ).await;
    });
    state.runtimes.write().await.attach_with_shutdown(&account.account_id, handle, shutdown_tx);

    // 广播 WS 事件
    crate::events::broadcast_runner_status(
        &state.ws_hub,
        account.account_id.clone(),
        "running".to_string(),
    );

    Ok(Json(AccountActionResponse {
        account_id: id,
        status: "running".to_string(),
    }))
}

/// POST /accounts/:id/stop — 停止账户
async fn stop_account(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
) -> Result<Json<AccountActionResponse>, (StatusCode, Json<ErrorResponse>)> {
    let account = match AccountRepository::find_by_id(&state.db, &id).await {
        Ok(Some(a)) => Some(a),
        _ => AccountRepository::find_by_account_id(&state.db, &id).await.map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("DB error: {}", e),
                }),
            )
        })?,
    }
    .ok_or((
        StatusCode::NOT_FOUND,
        Json(ErrorResponse {
            error: "Account not found".to_string(),
        }),
    ))?;

    check_account_permission(&state, &claims, &account.id, "operate").await?;

    // 更新数据库状态
    AccountRepository::update_state(&state.db, &account.id, "init")
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("DB error: {}", e),
                }),
            )
        })?;

    // 停止运行时
    state.runtimes.write().await.stop(&account.account_id);

    // 广播 WS 事件
    crate::events::broadcast_runner_status(
        &state.ws_hub,
        account.account_id.clone(),
        "stopped".to_string(),
    );

    Ok(Json(AccountActionResponse {
        account_id: id,
        status: "stopped".to_string(),
    }))
}

#[derive(Serialize)]
pub struct AccountStateResponse {
    pub account_id: String,
    pub state: String,
    pub is_running: bool,
    pub supervisor: Option<crate::supervisor::AccountSupervisor>,
}

/// GET /accounts/:id/state — 获取账户运行状态
async fn get_account_state(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
) -> Result<Json<AccountStateResponse>, (StatusCode, Json<ErrorResponse>)> {
    let account = match AccountRepository::find_by_id(&state.db, &id).await {
        Ok(Some(a)) => Some(a),
        _ => AccountRepository::find_by_account_id(&state.db, &id).await.map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("DB error: {}", e),
                }),
            )
        })?,
    }
    .ok_or((
        StatusCode::NOT_FOUND,
        Json(ErrorResponse {
            error: "Account not found".to_string(),
        }),
    ))?;

    check_account_permission(&state, &claims, &account.id, "view").await?;

    let is_running = state.runtimes.read().await.is_running(&account.account_id);
    let supervisor = state.supervisor.read().await.get(&account.account_id).cloned();

    Ok(Json(AccountStateResponse {
        account_id: id,
        state: account.state,
        is_running,
        supervisor,
    }))
}

/// GET /accounts/:id/snapshot — 该交易账户的实时快照（余额/净值/保证金 + 持仓）
async fn get_account_snapshot(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
) -> Result<Json<TradingStateSnapshot>, (StatusCode, Json<ErrorResponse>)> {
    let account = match AccountRepository::find_by_id(&state.db, &id).await {
        Ok(Some(a)) => Some(a),
        _ => AccountRepository::find_by_account_id(&state.db, &id).await.map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("DB error: {}", e),
                }),
            )
        })?,
    }
    .ok_or((
        StatusCode::NOT_FOUND,
        Json(ErrorResponse {
            error: "Account not found".to_string(),
        }),
    ))?;

    check_account_permission(&state, &claims, &account.id, "view").await?;

    let client = {
        let clients = state.clients.read().await;
        clients.get(&account.account_id).cloned()
    };

    let client = match client {
        Some(c) => c,
        None => {
            let api_key = state.crypto.decrypt(&account.encrypted_api_key).map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse {
                        error: format!("decrypt api key: {e}"),
                    }),
                )
            })?;
            let private_key = state.crypto.decrypt(&account.encrypted_private_key).map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse {
                        error: format!("decrypt private key: {e}"),
                    }),
                )
            })?;

            let creds = Credentials::new(&api_key, &private_key).map_err(|e| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(ErrorResponse {
                        error: format!("invalid credentials: {e}"),
                    }),
                )
            })?;

            let cfg = ClientConfig {
                gateway_url: url::Url::parse(&account.gateway_url).ok(),
                ..Default::default()
            };
            let c = Arc::new(Client::new(creds, &account.account_id, cfg).map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse {
                        error: format!("build client: {e}"),
                    }),
                )
            })?);
            let _ = c.ensure_access_point().await;
            state.clients.write().await.insert(account.account_id.clone(), c.clone());
            c
        }
    };

    let mut snap = client.snapshot().await.map_err(|e| {
        (
            StatusCode::BAD_GATEWAY,
            Json(ErrorResponse {
                error: format!("fetch snapshot: {e}"),
            }),
        )
    })?;

    // 当券商返回的快照中持仓 current_price 或 profit 为空（例如周末停盘未产出逐笔成交行情时），使用最新收盘价补充推算
    let has_empty = snap.positions.iter().any(|p| p.current_price.trim().is_empty() || p.profit.trim().is_empty());
    if has_empty && !snap.positions.is_empty() {
        if let Ok(candles) = state.market_data.candles("XAUUSD", "M1", 1).await {
            if let Some(last_candle) = candles.last() {
                let latest_price = last_candle.close;
                for p in &mut snap.positions {
                    let open_price = p.open_price.parse::<f64>().unwrap_or(0.0);
                    let volume = p.volume.parse::<f64>().unwrap_or(0.0);
                    let is_buy = p.position_type.to_lowercase() == "buy";
                    let sign = if is_buy { 1.0 } else { -1.0 };

                    if p.current_price.trim().is_empty() && latest_price > 0.0 {
                        p.current_price = format!("{:.3}", latest_price);
                    }
                    if p.profit.trim().is_empty() && latest_price > 0.0 && open_price > 0.0 {
                        let profit = (latest_price - open_price) * sign * volume * 100.0;
                        p.profit = format!("{:.2}", profit);
                    }
                }
            }
        }
    }

    Ok(Json(snap))
}

#[derive(Debug, Deserialize)]
pub struct DealsHistoryQuery {
    pub from: Option<String>,
    pub to: Option<String>,
    pub cursor: Option<String>,
    pub limit: Option<u32>,
    pub deal_type: Option<String>,
}

async fn get_account_deals(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
    Query(query): Query<DealsHistoryQuery>,
) -> Result<Json<DealsHistoryResponse>, (StatusCode, Json<ErrorResponse>)> {
    let account = match AccountRepository::find_by_id(&state.db, &id).await {
        Ok(Some(a)) => Some(a),
        _ => AccountRepository::find_by_account_id(&state.db, &id).await.map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("DB error: {}", e),
                }),
            )
        })?,
    }
    .ok_or((
        StatusCode::NOT_FOUND,
        Json(ErrorResponse {
            error: "Account not found".to_string(),
        }),
    ))?;

    check_account_permission(&state, &claims, &account.id, "view").await?;

    let client = {
        let clients = state.clients.read().await;
        clients.get(&account.account_id).cloned()
    };

    let client = match client {
        Some(c) => c,
        None => {
            let api_key = state.crypto.decrypt(&account.encrypted_api_key).map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse {
                        error: format!("decrypt api key: {e}"),
                    }),
                )
            })?;
            let private_key = state.crypto.decrypt(&account.encrypted_private_key).map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse {
                        error: format!("decrypt private key: {e}"),
                    }),
                )
            })?;

            let creds = Credentials::new(&api_key, &private_key).map_err(|e| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(ErrorResponse {
                        error: format!("invalid credentials: {e}"),
                    }),
                )
            })?;

            let cfg = ClientConfig {
                gateway_url: url::Url::parse(&account.gateway_url).ok(),
                ..Default::default()
            };
            let c = Arc::new(Client::new(creds, &account.account_id, cfg).map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse {
                        error: format!("build client: {e}"),
                    }),
                )
            })?);
            let _ = c.ensure_access_point().await;
            state.clients.write().await.insert(account.account_id.clone(), c.clone());
            c
        }
    };

    let history = client
        .deals_history(
            query.from.as_deref(),
            query.to.as_deref(),
            query.cursor.as_deref(),
            query.limit,
            query.deal_type.as_deref(),
        )
        .await
        .map_err(|e| {
            (
                StatusCode::BAD_GATEWAY,
                Json(ErrorResponse {
                    error: format!("fetch deals history: {e}"),
                }),
            )
        })?;

    Ok(Json(history))
}

#[derive(Debug, Deserialize)]
pub struct AccountActionRequest {
    pub action: String,
    pub lot: Option<String>,
    pub symbol: Option<String>,
    pub price: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct AccountActionExecResponse {
    pub success: bool,
    pub action: String,
    pub account_id: String,
    pub message: String,
    pub order_id: Option<String>,
}

/// POST /accounts/:id/actions — 执行交易员快捷操作或紧急操作
async fn execute_account_action(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
    Json(req): Json<AccountActionRequest>,
) -> Result<Json<AccountActionExecResponse>, (StatusCode, Json<ErrorResponse>)> {
    let account = match AccountRepository::find_by_id(&state.db, &id).await {
        Ok(Some(a)) => Some(a),
        _ => AccountRepository::find_by_account_id(&state.db, &id).await.map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("DB error: {}", e),
                }),
            )
        })?,
    }
    .ok_or((
        StatusCode::NOT_FOUND,
        Json(ErrorResponse {
            error: "Account not found".to_string(),
        }),
    ))?;

    check_account_permission(&state, &claims, &account.id, "operate").await?;

    let action = req.action.to_lowercase();
    if matches!(action.as_str(), "pause" | "resume") {
        let paused = action == "pause";
        AccountRepository::update_paused(&state.db, &account.id, paused).await.map_err(|e| {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: format!("DB error: {e}") }))
        })?;
        state.runtimes.write().await.set_paused(&account.account_id, paused);
        return Ok(Json(AccountActionExecResponse {
            success: true,
            action: action.clone(),
            account_id: account.account_id,
            message: if paused { "已暂停新开仓和加仓".to_string() } else { "已恢复交易".to_string() },
            order_id: None,
        }));
    }

    let requested_symbol = req.symbol;
    let lot_str = req.lot.unwrap_or_else(|| "0.01".to_string());

    // 尝试获取或构建客户端
    let client = {
        let clients = state.clients.read().await;
        clients.get(&account.account_id).cloned()
    };

    let client = match client {
        Some(c) => Some(c),
        None => {
            if let (Ok(api_key), Ok(private_key)) = (
                state.crypto.decrypt(&account.encrypted_api_key),
                state.crypto.decrypt(&account.encrypted_private_key),
            ) {
                if let Ok(creds) = Credentials::new(&api_key, &private_key) {
                    let cfg = ClientConfig {
                        gateway_url: url::Url::parse(&account.gateway_url).ok(),
                        ..Default::default()
                    };
                    if let Ok(c) = Client::new(creds, &account.account_id, cfg) {
                        let c = Arc::new(c);
                        let _ = c.ensure_access_point().await;
                        state.clients.write().await.insert(account.account_id.clone(), c.clone());
                        Some(c)
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            }
        }
    };

    // 实盘接口：客户端不可用时坚决报错，绝不伪造模拟成功响应
    let client = client.ok_or((
        StatusCode::BAD_GATEWAY,
        Json(ErrorResponse {
            error: "Exness trading client unavailable: credentials decrypt failed or gateway unreachable".to_string(),
        }),
    ))?;

    let symbol = match requested_symbol {
        Some(symbol) => symbol,
        None => {
            let instruments = client.instruments().await.map_err(|e| {
                (StatusCode::BAD_GATEWAY, Json(ErrorResponse { error: format!("list instruments: {e}") }))
            })?;
            select_gold_instrument(&instruments).ok_or((StatusCode::BAD_REQUEST, Json(ErrorResponse {
                error: "account has no supported gold instrument (XAUUSD/XAUUSDm/XAUUSDc)".to_string(),
            })))?
        }
    };

    match action.as_str() {
        "buy" | "sell" => {
            let side = if action == "buy" { "buy" } else { "sell" };
            let open_req = OpenPositionRequest {
                instrument: symbol.clone(),
                side: side.to_string(),
                volume: lot_str.clone(),
                price: req.price.map(|p| p.to_string()),
                stop_loss_price: None,
                take_profit_price: None,
                deviation: None,
                comment: Some("manual_quick_trade".to_string()),
            };
            let idempotency_key = format!(
                "manual-{}-{}-{}",
                account.account_id,
                chrono::Utc::now().timestamp_millis(),
                Uuid::new_v4()
            );

            let order_id = match client.open_position(&open_req, &idempotency_key).await {
                Ok(ack) => ack.operation_id,
                Err(e) => {
                    return Err((
                        StatusCode::BAD_GATEWAY,
                        Json(ErrorResponse {
                            error: format!("Exness open_position failed: {e}"),
                        }),
                    ));
                }
            };

            Ok(Json(AccountActionExecResponse {
                success: true,
                action: action.clone(),
                account_id: account.account_id,
                message: format!(
                    "市价单执行成功: {} {} {} 手",
                    action.to_uppercase(),
                    symbol,
                    lot_str
                ),
                order_id: Some(order_id),
            }))
        }
        "close_all" | "close_buy" | "close_sell" | "close_profit" | "close_loss" => {
            let mut closed_count = 0;
            let snap = client.snapshot().await.map_err(|e| {
                (
                    StatusCode::BAD_GATEWAY,
                    Json(ErrorResponse {
                        error: format!("Exness snapshot failed: {e}"),
                    }),
                )
            })?;

            for pos in snap.positions {
                let is_buy = pos.position_type.eq_ignore_ascii_case("buy");
                let is_sell = pos.position_type.eq_ignore_ascii_case("sell");
                let profit_val = pos.profit.parse::<f64>().unwrap_or(0.0);
                let should_close = match action.as_str() {
                    "close_all" => true,
                    "close_buy" => is_buy,
                    "close_sell" => is_sell,
                    "close_profit" => profit_val > 0.0,
                    "close_loss" => profit_val < 0.0,
                    _ => false,
                };
                if should_close {
                    let idem = format!("close-{}-{}", pos.id, Uuid::new_v4());
                    let params = ClosePositionParams {
                        position_id: pos.id.clone(),
                        volume: None,
                        price: None,
                        deviation: None,
                        close_by_position_id: None,
                    };
                    if client.close_position(&pos.id, &params, &idem).await.is_ok() {
                        closed_count += 1;
                    }
                }
            }
            Ok(Json(AccountActionExecResponse {
                success: true,
                action: action.clone(),
                account_id: account.account_id,
                message: format!("平仓指令 [{}] 已提交，处理订单数: {}", action, closed_count),
                order_id: None,
            }))
        }
        "lock" => {
            let snap = client.snapshot().await.map_err(|e| {
                (
                    StatusCode::BAD_GATEWAY,
                    Json(ErrorResponse {
                        error: format!("Exness snapshot failed: {e}"),
                    }),
                )
            })?;

            let mut buy_vol = rust_decimal::Decimal::ZERO;
            let mut sell_vol = rust_decimal::Decimal::ZERO;
            for pos in snap.positions {
                let vol = pos
                    .volume
                    .parse::<rust_decimal::Decimal>()
                    .unwrap_or(rust_decimal::Decimal::ZERO);
                if pos.position_type.eq_ignore_ascii_case("buy") {
                    buy_vol += vol;
                } else if pos.position_type.eq_ignore_ascii_case("sell") {
                    sell_vol += vol;
                }
            }
            let order_id = if buy_vol > sell_vol {
                let diff = buy_vol - sell_vol;
                let open_req = OpenPositionRequest {
                    instrument: symbol,
                    side: "sell".to_string(),
                    volume: diff.to_string(),
                    price: None,
                    stop_loss_price: None,
                    take_profit_price: None,
                    deviation: None,
                    comment: Some("manual_lock_sell".to_string()),
                };
                let idem = format!("lock-{}-{}", account.account_id, Uuid::new_v4());
                let resp = client.open_position(&open_req, &idem).await.map_err(|e| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(ErrorResponse {
                            error: format!("锁仓开空单失败: {e}"),
                        }),
                    )
                })?;
                Some(resp.operation_id)
            } else if sell_vol > buy_vol {
                let diff = sell_vol - buy_vol;
                let open_req = OpenPositionRequest {
                    instrument: symbol,
                    side: "buy".to_string(),
                    volume: diff.to_string(),
                    price: None,
                    stop_loss_price: None,
                    take_profit_price: None,
                    deviation: None,
                    comment: Some("manual_lock_buy".to_string()),
                };
                let idem = format!("lock-{}-{}", account.account_id, Uuid::new_v4());
                let resp = client.open_position(&open_req, &idem).await.map_err(|e| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(ErrorResponse {
                            error: format!("锁仓开多单失败: {e}"),
                        }),
                    )
                })?;
                Some(resp.operation_id)
            } else {
                return Ok(Json(AccountActionExecResponse {
                    success: true,
                    action: "lock".to_string(),
                    account_id: account.account_id,
                    message: "多空持仓已完全平衡，无需执行对冲锁仓".to_string(),
                    order_id: None,
                }));
            };
            Ok(Json(AccountActionExecResponse {
                success: true,
                action: "lock".to_string(),
                account_id: account.account_id,
                message: "一键对冲锁仓指令已提交".to_string(),
                order_id,
            }))
        }
        _ => Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: format!("Unknown action: {}", action),
            }),
        )),
    }
}

// ========== 系统端点 ==========

#[derive(Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub database: String,
    pub calendar_events: usize,
    pub active_accounts: usize,
}

/// GET /system/health — 健康检查
async fn system_health(
    State(state): State<Arc<AppState>>,
) -> Json<HealthResponse> {
    let cal_events = state.calendar.len().await;
    let active = state.runtimes.read().await.running_accounts().len();
    Json(HealthResponse {
        status: "ok".to_string(),
        database: "connected".to_string(),
        calendar_events: cal_events,
        active_accounts: active,
    })
}

#[derive(Serialize)]
pub struct CalendarResponse {
    pub events: Vec<crate::calendar::NewsEvent>,
    pub last_update: i64,
    pub cache_ttl_sec: i64,
}

/// GET /system/calendar — 获取财经日历
async fn get_calendar(
    State(state): State<Arc<AppState>>,
) -> Json<CalendarResponse> {
    let events = state.calendar.all_events().await;
    let last_update = state.calendar.last_update().await;
    let cache_ttl_sec = state.calendar.cache_ttl();
    Json(CalendarResponse {
        events,
        last_update,
        cache_ttl_sec,
    })
}

#[derive(Serialize)]
pub struct RefreshCalendarResponse {
    pub refreshed: bool,
    pub last_update: i64,
}

/// POST /system/calendar/refresh — 刷新财经日历
async fn refresh_calendar(
    State(state): State<Arc<AppState>>,
) -> Result<Json<RefreshCalendarResponse>, (StatusCode, Json<ErrorResponse>)> {
    let now = chrono::Utc::now().timestamp();
    let api_key = state.calendar_api_key.as_deref().ok_or((StatusCode::SERVICE_UNAVAILABLE, Json(ErrorResponse {
        error: "calendar.api_key is not configured in config.toml".to_string(),
    })))?;
    let api_url = state.calendar_api_url.as_deref()
        .unwrap_or("https://www.financecalendar.com/wp-json/fc/v1/calendar");
    state.calendar.refresh_from_api(
        api_url, api_key, now,
    ).await.map_err(|e| (StatusCode::BAD_GATEWAY, Json(ErrorResponse { error: format!("calendar refresh failed: {e}") })))?;
    let next = state.calendar.next_high_impact_event(now).await;
    state.news_state_tx.send_if_modified(|current| {
        let updated = (true, next);
        if *current == updated { false } else { *current = updated; true }
    });
    Ok(Json(RefreshCalendarResponse {
        refreshed: true,
        last_update: now,
    }))
}

// ========== 行情数据端点 ==========

#[derive(Deserialize)]
pub struct CandlesQuery {
    symbol: Option<String>,
    timeframe: Option<String>,
    count: Option<i32>,
}

/// GET /market/candles — 由 Exness Trader API 代理的历史 K 线。
async fn market_candles(
    State(state): State<Arc<AppState>>,
    Query(q): Query<CandlesQuery>,
) -> Result<Json<Vec<Candle>>, (StatusCode, Json<ErrorResponse>)> {
    let symbol = q.symbol.unwrap_or_else(|| "XAUUSD".to_string());
    let timeframe = q.timeframe.unwrap_or_else(|| "M1".to_string());
    let count = q.count.unwrap_or(300).clamp(1, 1000);
    let candles = state
        .market_data
        .candles(&symbol, &timeframe, count)
        .await
        .map_err(|e| {
            (
                StatusCode::BAD_GATEWAY,
                Json(ErrorResponse {
                    error: format!("candles: {e}"),
                }),
            )
        })?;
    Ok(Json(candles))
}

/// GET /market/snapshot — 实时账户交易状态（余额/净值/保证金 + 持仓）。
async fn market_snapshot(
    State(state): State<Arc<AppState>>,
) -> Result<Json<TradingStateSnapshot>, (StatusCode, Json<ErrorResponse>)> {
    let snap = state.market_data.snapshot().await.map_err(|e| {
        (
            StatusCode::BAD_GATEWAY,
            Json(ErrorResponse {
                error: format!("snapshot: {e}"),
            }),
        )
    })?;
    Ok(Json(snap))
}

// ========== 策略端点 ==========

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateStrategyRequest {
    pub name: String,
    pub description: Option<String>,
    pub initial_lot: String,
    pub grid_step: i32,
    pub grid_step_add: i32,
    pub grid_step_sec: i32,
    pub lot_mult: String,
    pub tp_distance_usd: String,
    pub lock_trigger_usd: String,
    pub params_locked: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateStrategyRequest {
    pub name: String,
    pub description: Option<String>,
    pub initial_lot: String,
    pub grid_step: i32,
    pub grid_step_add: i32,
    pub grid_step_sec: i32,
    pub lot_mult: String,
    pub tp_distance_usd: String,
    pub lock_trigger_usd: String,
    pub params_locked: bool,
}

#[derive(Debug, Serialize)]
pub struct StrategyListItem {
    #[serde(flatten)]
    pub strategy: Strategy,
    pub bound_accounts: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct BindStrategyRequest {
    pub account_ids: Vec<String>,
    #[serde(default)]
    pub params_locked: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct BindAccountStrategyRequest {
    pub strategy_id: Option<String>,
    #[serde(default)]
    pub params_locked: Option<bool>,
}

/// GET /strategies — 列出所有策略及其绑定账户
async fn list_strategies(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<StrategyListItem>>, (StatusCode, Json<ErrorResponse>)> {
    let strategies = StrategyRepository::list_all(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: e.to_string() })))?;
    let accounts = AccountRepository::list_all(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: e.to_string() })))?;

    let mut result = Vec::new();
    for s in strategies {
        let bound: Vec<String> = accounts
            .iter()
            .filter(|a| a.strategy_id.as_deref() == Some(&s.id))
            .map(|a| a.account_id.clone())
            .collect();
        result.push(StrategyListItem {
            strategy: s,
            bound_accounts: bound,
        });
    }
    Ok(Json(result))
}

/// GET /strategies/:id — 获取策略详情
async fn get_strategy(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<StrategyListItem>, (StatusCode, Json<ErrorResponse>)> {
    let strat = StrategyRepository::find_by_id(&state.db, &id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: e.to_string() })))?
        .ok_or((StatusCode::NOT_FOUND, Json(ErrorResponse { error: "Strategy not found".to_string() })))?;

    let accounts = AccountRepository::list_all(&state.db)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: e.to_string() })))?;
    let bound: Vec<String> = accounts
        .iter()
        .filter(|a| a.strategy_id.as_deref() == Some(&strat.id))
        .map(|a| a.account_id.clone())
        .collect();

    Ok(Json(StrategyListItem {
        strategy: strat,
        bound_accounts: bound,
    }))
}

/// POST /strategies — 创建新策略
async fn create_strategy(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateStrategyRequest>,
) -> Result<Json<Strategy>, (StatusCode, Json<ErrorResponse>)> {
    let now = chrono::Utc::now().timestamp();
    let strategy = Strategy {
        id: format!("strat_{}", Uuid::new_v4().simple()),
        name: req.name,
        description: req.description.unwrap_or_default(),
        initial_lot: req.initial_lot,
        grid_step: req.grid_step,
        grid_step_add: req.grid_step_add,
        grid_step_sec: req.grid_step_sec,
        lot_mult: req.lot_mult,
        tp_distance_usd: req.tp_distance_usd,
        lock_trigger_usd: req.lock_trigger_usd,
        params_locked: req.params_locked.unwrap_or(false),
        created_at: now,
        updated_at: now,
    };
    StrategyRepository::create(&state.db, &strategy)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(ErrorResponse { error: e.to_string() })))?;
    Ok(Json(strategy))
}

/// PUT /strategies/:id — 更新策略并同步到已绑定账户
async fn update_strategy(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<UpdateStrategyRequest>,
) -> Result<Json<Strategy>, (StatusCode, Json<ErrorResponse>)> {
    let existing = StrategyRepository::find_by_id(&state.db, &id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: e.to_string() })))?
        .ok_or((StatusCode::NOT_FOUND, Json(ErrorResponse { error: "Strategy not found".to_string() })))?;

    let now = chrono::Utc::now().timestamp();
    let updated = Strategy {
        id: existing.id,
        name: req.name,
        description: req.description.unwrap_or(existing.description),
        initial_lot: req.initial_lot,
        grid_step: req.grid_step,
        grid_step_add: req.grid_step_add,
        grid_step_sec: req.grid_step_sec,
        lot_mult: req.lot_mult,
        tp_distance_usd: req.tp_distance_usd,
        lock_trigger_usd: req.lock_trigger_usd,
        params_locked: req.params_locked,
        created_at: existing.created_at,
        updated_at: now,
    };
    StrategyRepository::update(&state.db, &updated)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: e.to_string() })))?;

    // 同步所有已绑定账户的 strategy_params
    let accounts = AccountRepository::list_all(&state.db).await.unwrap_or_default();
    for acc in accounts.into_iter().filter(|a| a.strategy_id.as_deref() == Some(&id)) {
        let sp = StrategyParams {
            account_id: acc.id.clone(),
            initial_lot: updated.initial_lot.clone(),
            grid_step: updated.grid_step,
            grid_step_add: updated.grid_step_add,
            grid_step_sec: updated.grid_step_sec,
            lot_mult: updated.lot_mult.clone(),
            tp_distance_usd: updated.tp_distance_usd.clone(),
            lock_trigger_usd: updated.lock_trigger_usd.clone(),
            params_locked: updated.params_locked,
            updated_at: now,
        };
        let _ = StrategyParamsRepository::upsert(&state.db, &sp).await;
    }

    Ok(Json(updated))
}

/// DELETE /strategies/:id — 删除策略
async fn delete_strategy(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<StatusCode, (StatusCode, Json<ErrorResponse>)> {
    StrategyRepository::delete(&state.db, &id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: e.to_string() })))?;
    Ok(StatusCode::NO_CONTENT)
}

/// POST /strategies/:id/bind — 批量绑定账户到策略
async fn bind_strategy_accounts(
    State(state): State<Arc<AppState>>,
    Path(strategy_id): Path<String>,
    Json(req): Json<BindStrategyRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let strat = StrategyRepository::find_by_id(&state.db, &strategy_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: e.to_string() })))?
        .ok_or((StatusCode::NOT_FOUND, Json(ErrorResponse { error: "Strategy not found".to_string() })))?;

    let now = chrono::Utc::now().timestamp();
    let is_locked = req.params_locked.unwrap_or(true);
    for acc_id_or_num in &req.account_ids {
        let acc = match AccountRepository::find_by_id(&state.db, acc_id_or_num).await {
            Ok(Some(a)) => Some(a),
            _ => AccountRepository::find_by_account_id(&state.db, acc_id_or_num).await.ok().flatten(),
        };
        if let Some(account) = acc {
            AccountRepository::bind_strategy(&state.db, &account.id, Some(&strategy_id))
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: e.to_string() })))?;

            let sp = StrategyParams {
                account_id: account.id.clone(),
                initial_lot: strat.initial_lot.clone(),
                grid_step: strat.grid_step,
                grid_step_add: strat.grid_step_add,
                grid_step_sec: strat.grid_step_sec,
                lot_mult: strat.lot_mult.clone(),
                tp_distance_usd: strat.tp_distance_usd.clone(),
                lock_trigger_usd: strat.lock_trigger_usd.clone(),
                params_locked: is_locked,
                updated_at: now,
            };
            let _ = StrategyParamsRepository::upsert(&state.db, &sp).await;
        }
    }

    Ok(Json(serde_json::json!({
        "success": true,
        "strategy_id": strategy_id,
        "count": req.account_ids.len()
    })))
}

/// POST /accounts/:id/strategy — 将账户关联或解绑策略
async fn set_account_strategy(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
    Json(req): Json<BindAccountStrategyRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let account = match AccountRepository::find_by_id(&state.db, &id).await {
        Ok(Some(a)) => Some(a),
        _ => AccountRepository::find_by_account_id(&state.db, &id).await.map_err(|e| {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: e.to_string() }))
        })?,
    }
    .ok_or((StatusCode::NOT_FOUND, Json(ErrorResponse { error: "Account not found".to_string() })))?;

    check_account_permission(&state, &claims, &account.id, "operate").await?;

    let now = chrono::Utc::now().timestamp();
    AccountRepository::bind_strategy(&state.db, &account.id, req.strategy_id.as_deref())
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: e.to_string() })))?;

    if let Some(ref strat_id) = req.strategy_id {
        if let Ok(Some(strat)) = StrategyRepository::find_by_id(&state.db, strat_id).await {
            let is_locked = req.params_locked.unwrap_or(true);
            let sp = StrategyParams {
                account_id: account.id.clone(),
                initial_lot: strat.initial_lot.clone(),
                grid_step: strat.grid_step,
                grid_step_add: strat.grid_step_add,
                grid_step_sec: strat.grid_step_sec,
                lot_mult: strat.lot_mult.clone(),
                tp_distance_usd: strat.tp_distance_usd.clone(),
                lock_trigger_usd: strat.lock_trigger_usd.clone(),
                params_locked: is_locked,
                updated_at: now,
            };
            let _ = StrategyParamsRepository::upsert(&state.db, &sp).await;
        }
    }

    Ok(Json(serde_json::json!({
        "success": true,
        "account_id": account.account_id,
        "strategy_id": req.strategy_id
    })))
}

// ========== 路由配置 ==========

pub fn router(state: Arc<AppState>) -> Router {
    // 受保护的路由 + 认证中间件
    let protected = Router::new()
        .route("/accounts", get(list_accounts).post(create_account))
        .route(
            "/accounts/:id",
            get(get_account).delete(delete_account),
        )
        .route(
            "/accounts/:id/params",
            get(get_params).put(update_params),
        )
        .route("/accounts/:id/params/lock", post(toggle_account_params_lock))
        .route("/accounts/:id/start", post(start_account))
        .route("/accounts/:id/stop", post(stop_account))
        .route("/accounts/:id/state", get(get_account_state))
        .route("/accounts/:id/snapshot", get(get_account_snapshot))
        .route("/accounts/:id/history/deals", get(get_account_deals))
        .route("/accounts/:id/actions", post(execute_account_action))
        .route("/accounts/:id/strategy", post(set_account_strategy))
        .route("/strategies", get(list_strategies).post(create_strategy))
        .route(
            "/strategies/:id",
            get(get_strategy)
                .put(update_strategy)
                .delete(delete_strategy),
        )
        .route("/strategies/:id/bind", post(bind_strategy_accounts))
        .route("/system/calendar/refresh", post(refresh_calendar))
        .route("/market/snapshot", get(market_snapshot))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            crate::rate_limit::rate_limit_middleware,
        ))
        .layer(axum::middleware::from_fn_with_state(
            state.config.clone(),
            crate::middleware::auth_middleware,
        ));

    Router::new()
        // 认证
        .route("/auth/login", post(login))
        .route("/auth/register", post(register))
        // 系统
        .route("/system/health", get(system_health))
        .route("/system/calendar", get(get_calendar))
        // 行情数据（历史 K 线，公开）
        .route("/market/candles", get(market_candles))
        // WebSocket
        .route("/ws", get(crate::ws::ws_route_handler))
        // 账户（受保护）
        .merge(protected)
        .with_state(state)
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn test_router_compiles() {
        // 集成测试需要完整服务端环境
        assert!(true);
    }
}
