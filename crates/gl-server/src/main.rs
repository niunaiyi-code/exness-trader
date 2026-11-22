//! GL Server — 主入口。

use axum::{
    routing::get,
    Router,
};
use std::{env, net::SocketAddr, sync::Arc};
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use gl_server::{
    api::routes::{router, AppState},
    calendar::FinanceCalendar,
    config::ServerConfig,
    crypto::CredentialCrypto,
    db,
    runtime::RuntimeManager,
    supervisor::SupervisorRegistry,
    ws::{ws_route_handler, WsHub},
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 初始化 tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,gl_server=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // 加载配置
    let config_path = env::var("CONFIG_PATH").unwrap_or_else(|_| "config.toml".to_string());
    let config = ServerConfig::from_file(&config_path)?;
    tracing::info!("Loaded config from {}", config_path);

    // 初始化数据库
    let db_pool = db::init_pool(&config.database.url, config.database.max_connections).await?;
    tracing::info!("Database initialized: {}", config.database.url);


    // 若无默认管理员用户则创建
    if let Ok(count) = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM users").fetch_one(&db_pool).await {
        if count == 0 {
            let admin_id = uuid::Uuid::new_v4().to_string();
            let password_hash = gl_server::auth::hash_password("admin123")
                .expect("Failed to hash admin password during initialization");
            let _ = gl_server::db::repository::UserRepository::create(
                &db_pool,
                &admin_id,
                "admin",
                &password_hash,
                "admin",
            ).await;
            tracing::info!("Seeded default admin user: admin / admin123");

            // 创建默认交易员账户
            let trader_id = uuid::Uuid::new_v4().to_string();
            let trader_password = gl_server::auth::hash_password("Password123!")
                .expect("Failed to hash trader password during initialization");
            let _ = gl_server::db::repository::UserRepository::create(
                &db_pool,
                &trader_id,
                "18000069657",
                &trader_password,
                "trader",
            ).await;
            tracing::info!("Seeded default trader user: 18000069657 / Password123!");
        }
    }

    // 若无预置策略则创建
    if let Ok(count) = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM strategies").fetch_one(&db_pool).await {
        if count == 0 {
            let now = chrono::Utc::now().timestamp();
            let presets = vec![
                gl_server::db::models::Strategy {
                    id: "strat_standard".to_string(),
                    name: "经典稳健型网格".to_string(),
                    description: "标准 300 点间距，1.45 倍率加仓，兼顾平稳收益与回撤控制".to_string(),
                    initial_lot: "0.01".to_string(),
                    grid_step: 300,
                    grid_step_add: 75,
                    grid_step_sec: 15,
                    lot_mult: "1.45".to_string(),
                    tp_distance_usd: "1.00".to_string(),
                    lock_trigger_usd: "1000.00".to_string(),
                    params_locked: false,
                    created_at: now,
                    updated_at: now,
                },
                gl_server::db::models::Strategy {
                    id: "strat_conservative".to_string(),
                    name: "低频防守型网格".to_string(),
                    description: "400 点大间距，1.35 倍率平缓加仓，低风险防守".to_string(),
                    initial_lot: "0.01".to_string(),
                    grid_step: 400,
                    grid_step_add: 100,
                    grid_step_sec: 30,
                    lot_mult: "1.35".to_string(),
                    tp_distance_usd: "1.50".to_string(),
                    lock_trigger_usd: "800.00".to_string(),
                    params_locked: false,
                    created_at: now,
                    updated_at: now,
                },
                gl_server::db::models::Strategy {
                    id: "strat_aggressive".to_string(),
                    name: "激进获利型网格".to_string(),
                    description: "250 点密集间距，1.50 倍率加仓，追求高频止盈与单边快速落袋".to_string(),
                    initial_lot: "0.02".to_string(),
                    grid_step: 250,
                    grid_step_add: 50,
                    grid_step_sec: 15,
                    lot_mult: "1.50".to_string(),
                    tp_distance_usd: "2.00".to_string(),
                    lock_trigger_usd: "1500.00".to_string(),
                    params_locked: false,
                    created_at: now,
                    updated_at: now,
                },
            ];
            for p in presets {
                let _ = gl_server::db::repository::StrategyRepository::create(&db_pool, &p).await;
            }
            tracing::info!("Seeded 3 default strategy profiles");
        }
    }

    // 将未分配账户关联到默认标准策略
    let _ = sqlx::query("UPDATE accounts SET strategy_id = 'strat_standard' WHERE strategy_id IS NULL")
        .execute(&db_pool)
        .await;

    // 构建应用组件
    let crypto = Arc::new(CredentialCrypto::new(&config.crypto.master_key)?);
    let ws_hub = Arc::new(WsHub::new());
    let calendar = Arc::new(FinanceCalendar::new());
    let supervisor = Arc::new(tokio::sync::RwLock::new(SupervisorRegistry::new()));
    let runtimes = Arc::new(tokio::sync::RwLock::new(RuntimeManager::new()));
    let market_feed = Arc::new(gl_server::market_feed::MarketFeedHub::new());
    market_feed.start(
        config.market_feed.clone(),
        ws_hub.clone(),
        config.exness.gateway_url.clone(),
    );

    let market_data = Arc::new(gl_server::market_data::MarketDataService::new(
        config.market_feed.usd_account_id.clone(),
        config.market_feed.usd_api_key.clone(),
        config.market_feed.usd_private_key.clone(),
        config.exness.gateway_url.clone(),
    ));

    let calendar_api_key = config.calendar.api_key.clone()
        .filter(|value| !value.trim().is_empty())
        .map(Arc::new);
    let calendar_api_url = config.calendar.api_url.clone()
        .filter(|value| !value.trim().is_empty());
    let (news_state_tx, _) = tokio::sync::watch::channel((false, None));
    let instrument_conditions = Arc::new(gl_server::instrument_conditions::InstrumentConditions::default());

    // 启动后台日历刷新任务
    let cal_clone = calendar.clone();
    let key_clone = calendar_api_key.clone();
    let news_tx_clone = news_state_tx.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(300));
        loop {
            interval.tick().await;
            let now = chrono::Utc::now().timestamp();
            if let Some(api_key) = key_clone.as_deref() {
                match cal_clone.refresh_from_api("https://www.financecalendar.com/wp-json/fc/v1/calendar", api_key, now).await {
                    Ok(()) => {
                        let next = cal_clone.next_high_impact_event(now).await;
                        news_tx_clone.send_if_modified(|current| {
                            let updated = (true, next);
                            if *current == updated { false } else { *current = updated; true }
                        });
                    }
                    Err(error) => tracing::error!(%error, "Finance calendar refresh failed; first orders remain blocked until a successful refresh"),
                }
            }
        }
    });

    let app_state = Arc::new(AppState {
        db: db_pool,
        config: Arc::new(config.clone()),
        crypto,
        ws_hub: ws_hub.clone(),
        calendar: calendar.clone(),
        calendar_api_key,
        calendar_api_url,
        news_state_tx,
        instrument_conditions,
        supervisor: supervisor.clone(),
        runtimes: runtimes.clone(),
        market_data: market_data.clone(),
        tick_tx: market_feed.tick_sender(),
        clients: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
        rate_limiter: gl_server::rate_limit::RateLimiter::new(60, std::time::Duration::from_secs(60)),
    });

    // Restore accounts that were active before the service process stopped.
    let system_claims = gl_server::auth::Claims {
        sub: "system-recovery".to_string(),
        role: "admin".to_string(),
        exp: u64::MAX,
    };
    match gl_server::db::repository::AccountRepository::list_all(&app_state.db).await {
        Ok(accounts) => {
            for account in accounts.into_iter().filter(|account| account.state == "running") {
                match gl_server::api::routes::start_account(
                    axum::extract::State(app_state.clone()),
                    axum::Extension(system_claims.clone()),
                    axum::extract::Path(account.id.clone()),
                ).await {
                    Ok(_) => tracing::info!(account_id = %account.account_id, "Recovered running account"),
                    Err((status, error)) => tracing::error!(account_id = %account.account_id, %status, error = %error.0.error, "Failed to recover running account"),
                }
            }
        }
        Err(error) => tracing::error!(%error, "Failed to load accounts for runtime recovery"),
    }

    // 构建路由
    let mut app = Router::new()
        .route("/health", get(health_handler))
        .route("/metrics", get(metrics_handler))
        .route("/ws", get(ws_route_handler).with_state(app_state.clone()))
        .nest("/api/v1", router(app_state));

    if let Some(ref static_dir) = config.server.static_dir {
        if static_dir.exists() {
            let index_path = static_dir.join("index.html");
            let serve_dir = tower_http::services::ServeDir::new(static_dir)
                .fallback(tower_http::services::ServeFile::new(index_path));
            app = app.fallback_service(serve_dir);
            tracing::info!("Serving static files from {}", static_dir.display());
        }
    } else if std::path::Path::new("web/dist").exists() {
        let serve_dir = tower_http::services::ServeDir::new("web/dist")
            .fallback(tower_http::services::ServeFile::new("web/dist/index.html"));
        app = app.fallback_service(serve_dir);
        tracing::info!("Serving static files from web/dist");
    }

    let app = app
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http());

    // 启动服务端
    let addr: SocketAddr = format!("{}:{}", config.server.host, config.server.port)
        .parse()
        .map_err(|e| anyhow::anyhow!("Invalid server address {}:{}: {}", config.server.host, config.server.port, e))?;
    tracing::info!("Starting server on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

async fn health_handler() -> &'static str {
    "OK"
}

async fn metrics_handler() -> Result<String, axum::http::StatusCode> {
    use prometheus::Encoder;
    let encoder = prometheus::TextEncoder::new();
    let metric_families = prometheus::gather();
    let mut buffer = vec![];
    encoder.encode(&metric_families, &mut buffer)
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?;
    String::from_utf8(buffer)
        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)
}
