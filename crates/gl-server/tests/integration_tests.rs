/// gl-server 集成测试（docs/06）。

#[cfg(test)]
mod integration_tests {
    use axum_test::TestServer;
    use gl_server::{
        api::{router as api_router, AppState},
        config::{AuthConfig, CalendarConfig, CryptoConfig, DatabaseConfig, ExnessConfig, HttpConfig, ServerConfig},
        crypto::CredentialCrypto,
        db,
    };
    use std::sync::Arc;

    async fn test_app() -> (TestServer, Arc<AppState>) {
        test_app_with_calendar(None, None).await
    }

    async fn test_app_with_calendar(
        calendar_api_key: Option<String>,
        calendar_api_url: Option<String>,
    ) -> (TestServer, Arc<AppState>) {
        let config = ServerConfig {
            server: HttpConfig {
                host: "127.0.0.1".to_string(),
                port: 8080,
                static_dir: None,
            },
            database: DatabaseConfig {
                url: ":memory:".to_string(),
                max_connections: 5,
            },
            exness: ExnessConfig {
                gateway_url: "https://api.exness.com".to_string(),
                rest_base_url: None,
            },
            auth: AuthConfig {
                jwt_secret: "test_secret".to_string(),
                jwt_expiry_hours: 24,
            },
            crypto: CryptoConfig {
                master_key: "MTIzNDU2Nzg5MDEyMzQ1Njc4OTAxMjM0NTY3ODkwMTI=".to_string(),
            },
            market_feed: Default::default(),
            calendar: CalendarConfig {
                api_key: calendar_api_key.clone(),
                api_url: calendar_api_url.clone(),
            },
        };

        let db_pool = db::init_pool(&config.database.url, config.database.max_connections)
            .await
            .unwrap();

        let crypto = Arc::new(CredentialCrypto::new(&config.crypto.master_key).unwrap());
        let ws_hub = Arc::new(gl_server::ws::WsHub::new());
        let calendar = Arc::new(gl_server::calendar::FinanceCalendar::new());
        let supervisor = Arc::new(tokio::sync::RwLock::new(
            gl_server::supervisor::SupervisorRegistry::new(),
        ));
        let runtimes = Arc::new(tokio::sync::RwLock::new(
            gl_server::runtime::RuntimeManager::new(),
        ));
        let (news_state_tx, _) = tokio::sync::watch::channel((false, None));
        let instrument_conditions = Arc::new(gl_server::instrument_conditions::InstrumentConditions::default());

        let state = Arc::new(AppState {
            db: db_pool,
            config: Arc::new(config),
            crypto,
            ws_hub,
            calendar,
            calendar_api_key: calendar_api_key.map(Arc::new),
            calendar_api_url,
            news_state_tx,
            instrument_conditions,
            supervisor,
            runtimes,
            market_data: Arc::new(gl_server::market_data::MarketDataService::new(
                None,
                None,
                None,
                "https://api.exness.com".to_string(),
            )),
            tick_tx: tokio::sync::broadcast::channel::<exness_api::Tick>(1).0,
            clients: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
            rate_limiter: gl_server::rate_limit::RateLimiter::new(1000, std::time::Duration::from_secs(60)),
        });

        let app = api_router(state.clone());
        let client = TestServer::new(app).unwrap();

        (client, state)
    }

    #[tokio::test]
    async fn test_login_success() {
        let (client, state) = test_app().await;

        // 创建测试用户
        gl_server::db::repository::UserRepository::create(
            &state.db,
            "u1",
            "alice",
            &gl_server::auth::hash_password("password123").unwrap(),
            "admin",
        )
        .await
        .unwrap();

        // 尝试登录
        let response = client
            .post("/auth/login")
            .json(&serde_json::json!({
                "username": "alice",
                "password": "password123"
            }))
            .await;

        assert_eq!(response.status_code(), 200);
        let body: serde_json::Value = response.json();
        assert!(body["token"].is_string());
        assert_eq!(body["user_id"].as_str(), Some("u1"));
    }

    #[tokio::test]
    async fn test_login_invalid_password() {
        let (client, state) = test_app().await;

        // 创建测试用户
        gl_server::db::repository::UserRepository::create(
            &state.db,
            "u1",
            "alice",
            &gl_server::auth::hash_password("password123").unwrap(),
            "admin",
        )
        .await
        .unwrap();

        // 用错误密码尝试登录
        let response = client
            .post("/auth/login")
            .json(&serde_json::json!({
                "username": "alice",
                "password": "wrongpassword"
            }))
            .await;

        assert_eq!(response.status_code(), 401);
    }

    #[tokio::test]
    async fn test_create_account() {
        let (client, state) = test_app().await;

        // 创建用户并获取 token
        gl_server::db::repository::UserRepository::create(
            &state.db,
            "u1",
            "alice",
            &gl_server::auth::hash_password("password123").unwrap(),
            "admin",
        )
        .await
        .unwrap();

        let token = gl_server::auth::create_jwt("u1", "admin", "test_secret", 24).unwrap();

        // 创建账户
        let response = client
            .post("/accounts")
            .add_header("Authorization", format!("Bearer {}", token))
            .json(&serde_json::json!({
                "account_id": "18000069657",
                "api_key": "test_key",
                "private_key": "test_priv",
                "key_format": "hex128",
                "gateway_url": "https://api.exness.com"
            }))
            .await;

        assert_eq!(response.status_code(), 200);
        let body: serde_json::Value = response.json();
        assert_eq!(body["account_id"].as_str(), Some("18000069657"));
    }

    #[tokio::test]
    async fn test_list_accounts_unauthorized() {
        let (client, _state) = test_app().await;

        let response = client.get("/accounts").await;

        assert_eq!(response.status_code(), 401);
    }

    #[tokio::test]
    async fn test_system_health() {
        let (client, _state) = test_app().await;

        let response = client.get("/system/health").await;
        assert_eq!(response.status_code(), 200);

        let body: serde_json::Value = response.json();
        assert_eq!(body["status"].as_str(), Some("ok"));
        assert_eq!(body["database"].as_str(), Some("connected"));
    }

    #[tokio::test]
    async fn test_system_calendar_and_refresh() {
        // 用 wiremock 模拟日历 API，避免集成测试真实外呼外部服务
        let mock = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/wp-json/fc/v1/calendar"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
            .mount(&mock)
            .await;

        let (client, state) = test_app_with_calendar(
            Some("test_calendar_key".to_string()),
            Some(format!("{}/wp-json/fc/v1/calendar", mock.uri())),
        ).await;

        // 填充一个事件
        state
            .calendar
            .update(
                vec![gl_server::calendar::NewsEvent {
                    id: "ev_test_1".to_string(),
                    title: "FOMC Rate Decision".to_string(),
                    country: "US".to_string(),
                    impact: "high".to_string(),
                    forecast: None,
                    previous: None,
                    actual: None,
                    timestamp: 1700000000,
                }],
                1700000000,
            )
            .await;

        let response = client.get("/system/calendar").await;
        assert_eq!(response.status_code(), 200);
        let body: serde_json::Value = response.json();
        assert_eq!(body["events"].as_array().unwrap().len(), 1);
        assert_eq!(body["events"][0]["id"].as_str(), Some("ev_test_1"));

        // 触发刷新：该端点位于 JWT 认证路由层，须携带管理员 Authorization 头
        let token = gl_server::auth::create_jwt("admin_uid", "admin", "test_secret", 24).unwrap();
        let refresh_resp = client
            .post("/system/calendar/refresh")
            .add_header("Authorization", format!("Bearer {}", token))
            .await;
        assert_eq!(refresh_resp.status_code(), 200);
        let refresh_body: serde_json::Value = refresh_resp.json();
        assert_eq!(refresh_body["refreshed"].as_bool(), Some(true));
    }

    #[tokio::test]
    async fn test_account_start_and_stop_lifecycle() {
        // 用 wiremock 模拟券商网关，避免集成测试依赖真实 api.exness.com
        let mock = wiremock::MockServer::start().await;

        // 握手 1：账户详情（currency=USD）
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/v1/configuration/accounts/acc_lifecycle_1/account"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "account_id": "acc_lifecycle_1",
                "currency": "USD"
            })))
            .mount(&mock)
            .await;

        // 握手 2：品种列表（含 XAUUSD）
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/v1/configuration/accounts/acc_lifecycle_1/instruments"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "instruments": ["XAUUSD", "EURUSD", "GBPUSD"]
            })))
            .mount(&mock)
            .await;

        // 握手 3：品种交易条件（quote_currency=USD）
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/v1/configuration/accounts/acc_lifecycle_1/instruments/XAUUSD/conditions"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "symbol": "XAUUSD",
                "quote_currency": "USD",
                "point_digits": 2,
                "volume_min": "0.01",
                "volume_max": "100",
                "volume_step": "0.01",
                "contract_size": "100"
            })))
            .mount(&mock)
            .await;

        // warmup：账户快照（空）
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/v1/trading/accounts/acc_lifecycle_1/snapshot"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
            .mount(&mock)
            .await;

        // warmup：K 线（空）
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/v1/market-data/accounts/acc_lifecycle_1/candles"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "candles": []
            })))
            .mount(&mock)
            .await;

        let (client, state) = test_app().await;

        let token = gl_server::auth::create_jwt("u1", "admin", "test_secret", 24).unwrap();

        // 先创建账户（gateway_url 指向本地 mock 网关）
        let create_resp = client
            .post("/accounts")
            .add_header("Authorization", format!("Bearer {}", token))
            .json(&serde_json::json!({
                "account_id": "acc_lifecycle_1",
                "api_key": "exnsk-test1234567890",
                "private_key": "1111111111111111111111111111111111111111111111111111111111111111",
                "key_format": "hex64",
                "gateway_url": mock.uri()
            }))
            .await;
        assert_eq!(create_resp.status_code(), 200);
        let acc: serde_json::Value = create_resp.json();
        let id = acc["id"].as_str().unwrap();

        // 启动前订阅 WS hub
        let mut ws_rx = state.ws_hub.subscribe();

        // 启动账户
        let start_resp = client
            .post(&format!("/accounts/{}/start", id))
            .add_header("Authorization", format!("Bearer {}", token))
            .await;
        assert_eq!(start_resp.status_code(), 200);
        let start_body: serde_json::Value = start_resp.json();
        assert_eq!(start_body["status"].as_str(), Some("running"));

        // 验证收到 WS 帧
        let ws_msg = ws_rx.recv().await.unwrap();
        match ws_msg {
            gl_server::ws::WsFrame::RunnerStatus { account_id, status } => {
                assert_eq!(account_id, "acc_lifecycle_1");
                assert_eq!(status, "running");
            }
            _ => panic!("Expected RunnerStatus WS frame"),
        }

        // 查询状态
        let state_resp = client
            .get(&format!("/accounts/{}/state", id))
            .add_header("Authorization", format!("Bearer {}", token))
            .await;
        assert_eq!(state_resp.status_code(), 200);
        let state_body: serde_json::Value = state_resp.json();
        assert_eq!(state_body["is_running"].as_bool(), Some(true));
        assert_eq!(state_body["state"].as_str(), Some("running"));

        // 停止账户
        let stop_resp = client
            .post(&format!("/accounts/{}/stop", id))
            .add_header("Authorization", format!("Bearer {}", token))
            .await;
        let stop_status = stop_resp.status_code();
        let stop_text = stop_resp.text();
        assert_eq!(stop_status, 200, "stop_account failed: {}", stop_text);
        let stop_body: serde_json::Value = serde_json::from_str(&stop_text).unwrap();
        assert_eq!(stop_body["status"].as_str(), Some("stopped"));

        // 验证收到 WS 帧
        let ws_stop_msg = ws_rx.recv().await.unwrap();
        match ws_stop_msg {
            gl_server::ws::WsFrame::RunnerStatus { account_id, status } => {
                assert_eq!(account_id, "acc_lifecycle_1");
                assert_eq!(status, "stopped");
            }
            _ => panic!("Expected RunnerStatus WS frame"),
        }
    }

    #[tokio::test]
    async fn test_strategy_definition_and_account_binding() {
        let (client, _state) = test_app().await;
        let token = gl_server::auth::create_jwt("u_admin", "admin", "test_secret", 24).unwrap();

        // 1. 创建策略
        let strat_resp = client
            .post("/strategies")
            .add_header("Authorization", format!("Bearer {}", token))
            .json(&serde_json::json!({
                "name": "高空低多突破型",
                "description": "基于K线与点位的大间距网格",
                "initial_lot": "0.02",
                "grid_step": 350,
                "grid_step_add": 80,
                "grid_step_sec": 20,
                "lot_mult": "1.50",
                "tp_distance_usd": "2.50",
                "lock_trigger_usd": "1200.00",
                "params_locked": true
            }))
            .await;
        assert_eq!(strat_resp.status_code(), 200);
        let strat_body: serde_json::Value = strat_resp.json();
        let strat_id = strat_body["id"].as_str().unwrap();

        // 2. 创建账户
        let acc_resp = client
            .post("/accounts")
            .add_header("Authorization", format!("Bearer {}", token))
            .json(&serde_json::json!({
                "account_id": "acc_strat_test_1",
                "api_key": "exnsk-test1234567890",
                "private_key": "1111111111111111111111111111111111111111111111111111111111111111",
                "key_format": "hex64",
                "gateway_url": "https://api.exness.com"
            }))
            .await;
        assert_eq!(acc_resp.status_code(), 200);
        let acc_body: serde_json::Value = acc_resp.json();
        let acc_id = acc_body["id"].as_str().unwrap();

        // 3. 将账户绑定至策略
        let bind_resp = client
            .post(&format!("/strategies/{}/bind", strat_id))
            .add_header("Authorization", format!("Bearer {}", token))
            .json(&serde_json::json!({
                "account_ids": [acc_id]
            }))
            .await;
        assert_eq!(bind_resp.status_code(), 200);

        // 4. 验证账户列表显示已绑定策略的名称与 ID
        let list_resp = client
            .get("/accounts")
            .add_header("Authorization", format!("Bearer {}", token))
            .await;
        assert_eq!(list_resp.status_code(), 200);
        let list_body: Vec<serde_json::Value> = list_resp.json();
        let found = list_body.iter().find(|a| a["account_id"] == "acc_strat_test_1").unwrap();
        assert_eq!(found["strategy_id"].as_str(), Some(strat_id));
        assert_eq!(found["strategy_name"].as_str(), Some("高空低多突破型"));

        // 5. 验证策略列表显示已绑定账户
        let strats_list_resp = client
            .get("/strategies")
            .add_header("Authorization", format!("Bearer {}", token))
            .await;
        assert_eq!(strats_list_resp.status_code(), 200);
        let strats_list: Vec<serde_json::Value> = strats_list_resp.json();
        let strat_item = strats_list.iter().find(|s| s["id"] == strat_id).unwrap();
        let bound_accs: Vec<String> = strat_item["bound_accounts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        assert!(bound_accs.contains(&"acc_strat_test_1".to_string()));
    }

    #[tokio::test]
    async fn test_unauthenticated_registration_defaults_to_trader_and_rejects_admin() {
        let (client, state) = test_app().await;

        // 1. 未登录用户尝试注册管理员 -> 必须被 403 拒绝
        let resp_hack = client
            .post("/auth/register")
            .json(&serde_json::json!({
                "username": "attacker",
                "password": "Password123!",
                "role": "admin"
            }))
            .await;
        assert_eq!(resp_hack.status_code(), 403);

        // 2. 未登录用户普通注册 -> 允许，但服务端强制赋予 trader 角色
        let resp_ok = client
            .post("/auth/register")
            .json(&serde_json::json!({
                "username": "bob",
                "password": "Password123!"
            }))
            .await;
        assert_eq!(resp_ok.status_code(), 200);
        let bob: serde_json::Value = resp_ok.json();
        assert_eq!(bob["role"].as_str(), Some("trader"));

        // 3. 已认证管理员注册用户 -> 允许指定角色
        let admin_token = gl_server::auth::create_jwt("admin_uid", "admin", "test_secret", 24).unwrap();
        let resp_admin_create = client
            .post("/auth/register")
            .add_header("Authorization", format!("Bearer {}", admin_token))
            .json(&serde_json::json!({
                "username": "new_admin",
                "password": "Password123!",
                "role": "admin"
            }))
            .await;
        assert_eq!(resp_admin_create.status_code(), 200);
        let new_admin: serde_json::Value = resp_admin_create.json();
        assert_eq!(new_admin["role"].as_str(), Some("admin"));
    }

    #[tokio::test]
    async fn test_rbac_account_isolation_and_permission_enforcement() {
        let (client, state) = test_app().await;

        // 在数据库中注册用户 A 和用户 B
        gl_server::db::repository::UserRepository::create(
            &state.db,
            "user_a",
            "trader_a",
            &gl_server::auth::hash_password("Password123!").unwrap(),
            "trader",
        )
        .await
        .unwrap();

        gl_server::db::repository::UserRepository::create(
            &state.db,
            "user_b",
            "trader_b",
            &gl_server::auth::hash_password("Password123!").unwrap(),
            "trader",
        )
        .await
        .unwrap();

        // 用户 A (Trader A)
        let token_a = gl_server::auth::create_jwt("user_a", "trader", "test_secret", 24).unwrap();
        // 用户 B (Trader B)
        let token_b = gl_server::auth::create_jwt("user_b", "trader", "test_secret", 24).unwrap();

        // 1. 用户 A 创建账户 acc_a
        let resp_create = client
            .post("/accounts")
            .add_header("Authorization", format!("Bearer {}", token_a))
            .json(&serde_json::json!({
                "account_id": "18000000001",
                "api_key": "key_a",
                "private_key": "priv_a",
                "key_format": "hex128",
                "gateway_url": "https://api.exness.com"
            }))
            .await;
        assert_eq!(resp_create.status_code(), 200);
        let acc_a: serde_json::Value = resp_create.json();
        let acc_a_pk = acc_a["id"].as_str().unwrap();

        // 2. 用户 B 列出账户列表 -> 绝不包含 acc_a
        let resp_list_b = client
            .get("/accounts")
            .add_header("Authorization", format!("Bearer {}", token_b))
            .await;
        assert_eq!(resp_list_b.status_code(), 200);
        let list_b: Vec<serde_json::Value> = resp_list_b.json();
        assert!(list_b.iter().all(|a| a["account_id"] != "18000000001"));

        // 3. 用户 B 尝试直接访问 acc_a 详情 -> 403 越权拒绝
        let resp_get_b = client
            .get(&format!("/accounts/{}", acc_a_pk))
            .add_header("Authorization", format!("Bearer {}", token_b))
            .await;
        assert_eq!(resp_get_b.status_code(), 403);

        // 4. 用户 B 尝试启动或操作 acc_a -> 403 越权拒绝
        let resp_start_b = client
            .post(&format!("/accounts/{}/start", acc_a_pk))
            .add_header("Authorization", format!("Bearer {}", token_b))
            .await;
        assert_eq!(resp_start_b.status_code(), 403);

        // 5. 用户 A 自己查看 -> 200 允许，且 permission 显示 admin
        let resp_get_a = client
            .get(&format!("/accounts/{}", acc_a_pk))
            .add_header("Authorization", format!("Bearer {}", token_a))
            .await;
        assert_eq!(resp_get_a.status_code(), 200);
    }

    #[tokio::test]
    async fn test_manual_order_fails_when_client_unavailable() {
        let (client, state) = test_app().await;

        let token = gl_server::auth::create_jwt("user_1", "admin", "test_secret", 24).unwrap();

        // 创建一个凭据无法连接的测试账户
        let resp_create = client
            .post("/accounts")
            .add_header("Authorization", format!("Bearer {}", token))
            .json(&serde_json::json!({
                "account_id": "18999999999",
                "api_key": "invalid_key",
                "private_key": "invalid_priv",
                "key_format": "hex128",
                "gateway_url": "https://api.exness.com"
            }))
            .await;
        assert_eq!(resp_create.status_code(), 200);
        let acc: serde_json::Value = resp_create.json();
        let acc_id = acc["id"].as_str().unwrap();

        // 执行买入操作 -> 客户端构建/解密不可用时，必须返回 502 BAD_GATEWAY，绝不能返回假成功 sim-order
        let resp_action = client
            .post(&format!("/accounts/{}/actions", acc_id))
            .add_header("Authorization", format!("Bearer {}", token))
            .json(&serde_json::json!({
                "action": "buy",
                "lot": "0.01",
                "symbol": "XAUUSD"
            }))
            .await;
        assert_eq!(resp_action.status_code(), 502);
    }
}
