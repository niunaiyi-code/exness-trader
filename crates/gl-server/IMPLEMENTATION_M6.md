# GL Server M6 Implementation Progress

**Started**: 2026-11-20  
**Status**: Foundation modules complete ✅

---

## Module Completion

### Core Infrastructure (7/7 ✓)

| Module | Lines | Tests | Status |
| --- | --- | --- | --- |
| `config.rs` | ~60 | 1 | ✅ TOML config + validation |
| `auth.rs` | ~100 | 3 | ✅ Argon2 + JWT |
| `crypto.rs` | ~90 | 2 | ✅ AES-256-GCM credential encryption |
| `db/mod.rs` | ~40 | 1 | ✅ SQLite pool + migrations |
| `db/models.rs` | ~50 | — | ✅ User/Account/Params models |
| `db/schema.rs` | ~60 | — | ✅ DDL for 4 core tables |
| `supervisor.rs` | ~80 | 2 | ✅ Backoff policy (1s→60s, 10 failures → Failed) |

**Total**: ~480 lines, 9 tests ✓

### Database Layer (1/1 ✓)

| Module | Lines | Tests | Status |
| --- | --- | --- | --- |
| `db/repository.rs` | ~280 | 3 | ✅ User/Account/Params CRUD |

**Total**: ~280 lines, 3 tests ✓

### API Layer (3/3 ✓)

| Module | Lines | Tests | Status |
| --- | --- | --- | --- |
| `middleware.rs` | ~80 | 1 | ✅ JWT extraction + role checking |
| `api/routes.rs` | ~450 | 1 | ✅ Full REST endpoints (auth/accounts/params) |
| `ws.rs` | ~90 | 1 | ✅ WebSocket hub (broadcast + subscribe) |

**Total**: ~620 lines, 3 tests ✓

### Runtime Integration (3/3 ✓)

| Module | Lines | Tests | Status |
| --- | --- | --- | --- |
| `runtime.rs` | ~60 | 1 | ✅ Per-account runner spawn/stop |
| `calendar.rs` | ~120 | 3 | ✅ Global news calendar (10s cache + high-impact filter) |
| `events.rs` | ~80 | 2 | ✅ Exness event → WS frame conversion |

**Total**: ~260 lines, 6 tests ✓

### Server Entry (1/1 ✓)

| Module | Lines | Tests | Status |
| --- | --- | --- | --- |
| `main.rs` | ~70 | — | ✅ Axum app + CORS + static files |

---

## Test Summary

**Total**: 21 unit tests ✓

### Coverage by Module
- auth: 3 (password hash/verify, JWT create/verify/invalid)
- crypto: 2 (encrypt/decrypt roundtrip, invalid key)
- db/mod: 1 (pool init + table creation)
- db/repository: 3 (user CRUD, account CRUD, permissions)
- supervisor: 2 (backoff escalation, reset)
- middleware: 1 (JWT roundtrip)
- api/routes: 1 (router compiles)
- ws: 1 (hub broadcast)
- runtime: 1 (spawn/stop)
- calendar: 3 (cache TTL, high-impact filter, news window)
- events: 2 (tick conversion, transaction conversion)
- config: 1 (TOML roundtrip)

---

## Next Steps (M6 Remaining)

### Backend (待实现)
- [ ] WS 完整集成（订阅 Exness ticks/events，转发给前端）
- [ ] Calendar 后台刷新任务（每 10s 从 Finance Calendar API 拉取）
- [ ] Supervisor 运行时集成（启动所有账户 runners）
- [ ] 手动操作端点（close all, lock, unlock）
- [ ] 站内 WS 通知推送
- [ ] E2E 集成测试（20+ tests）

### Frontend (待实现)
- [ ] React 18 + TypeScript + Vite 骨架
- [ ] Ant Design 5 主题配置
- [ ] 路由 (登录/总览/账户详情/参数/系统)
- [ ] 账户详情页 (HudBar/BasketCard/QuickActions/Positions/Events)
- [ ] 实时 WebSocket 集成
- [ ] lightweight-charts 价格图表

---

## Architecture Notes

### Multi-Account Model
- One tokio task per account (gl-strategy AccountRunner)
- Shared Exness API client (connection pool)
- Message-isolated (no shared mutable state)
- Supervisor manages lifecycle (backoff + restart)

### Security
- Credentials encrypted at rest (AES-256-GCM)
- JWT for session auth (24h expiry)
- RBAC: admin/trader/viewer × view/operate/admin
- Audit log for all manual actions

### Deployment
- Single binary (`gl-server`)
- SQLite (migrate to PostgreSQL for sharding)
- Optional static file serving (React build)
- systemd / Docker ready

---

**Current State**: Foundation + Runtime Integration complete ✅ — REST API, DB layer, auth, calendar, runtime spawning ready. Next: WS integration + Frontend.
