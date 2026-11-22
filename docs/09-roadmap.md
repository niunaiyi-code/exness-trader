# 09 · 开发路线图

---

## 1. 里程碑总览

| 里程碑 | 目标 | 产出 | 预估 |
| --- | --- | --- | --- |
| M0 文档定稿 | 本套文档评审通过，开放问题有结论或有验证计划 | docs/ | ✅ 完成（2026-11-20） |
| M1 API 可行性验证 | 用模拟账户打通签名、接入点、快照、WS、开 / 平仓、对冲持仓、`exec_mode` | `exness-api` crate + 验证脚本 + 验证报告 | ✅ 完成（2026-11-19） |
| **M2-M5 策略内核** | **`gl-strategy` + `gl-runtime` 全部模块，60+ 单元测试** | **crate + 测试** | **✅ 完成（2026-11-20）** |
| **M6 后端 + 前端** | **`gl-server` REST/WS + React SPA，30+ 单元测试 + E2E** | **可部署版本 v0.1** | **✅ 完成（2026-11-20）** |
| M7 灰度与加固 | 模拟盘 ≥ 7 交易日 → 小资金实盘单账户 → 多账户；监控、备份、运维文档 | v1.0 | 计划中 |
| M8 Android | 见 08 | App 内测 | 计划中 |

---

## 2. 任务拆解

### M1 API 可行性验证（执行手册见 11）
- [x] 签名实现 + 单元测试（空 body hash 常量；128 hex seed‖pub 解析与公钥校验；64 hex；PEM；幂等键规则）
- [x] 接入点发现客户端（`gateway_url` → `GET /v1/trading/access-point` → 缓存 / 失效重发现）；`rest_base_url` 测试直连
- [x] REST：账户详情、品种列表、品种条件、limits、快照、蜡烛、开仓、平仓、操作状态；错误码分类；`Date` 头时钟偏差监测；签名路径与发送路径逐字节一致性校验
- [x] WS：签名握手、ticks / events 订阅命令、入站消息解析（tick / 事件 / 错误 / 确认）
- [x] 模型按 OpenAPI 1.0.2 规范示例做反序列化测试（快照、成交事件、拒单事件、余额事件、品种事件、limits）
- [x] 只读探针 `m1_probe` 与交易探针 `m1_trade_probe`（对冲、幂等重放、平仓）
- [x] **在模拟账户上运行两个探针**：接入点形态、黄金 `exec_mode` / `point_digits` / `contract_size` / `trade_sessions`、实际限流数值、Tick 频率（OQ-02 关闭）
- [x] 对冲并存实测（OQ-03 关闭）、`transaction_event` 字段与 `client_request_id` 回显、幂等重放行为
- [x] 输出《M1 验证报告》（12），回填 02 / 03 / 04 文档

### M2 策略内核 ✅ 完成 2026-11-20
- [x] `types.rs` (enums: Side, LockState, ManualAction; 2 tests)
- [x] `config.rs` (Config, StrategyConstants, RuntimeConstants, validate; 7 tests)
- [x] `grid_math.rs` (lot progression, theo loss, auto_max_orders, survival; 8 tests)
- [x] `basket.rs` (accumulate, finalize, parabolic TP, layered TP, add-grid; 3 tests)
- [x] `lock_manager.rs` (3-state FSM, persistence, auto-close; 5 tests)
- [x] `risk_manager.rs` (gates, session model; 7 tests)
- [x] `app.rs` (主循环编排; 3 tests)
- [x] `trade_executor.rs` (交易请求生成; 7 tests)
- [x] `ops_tracker.rs` (幂等跟踪; 6 tests)
- [x] `account_runner.rs` (台账对账; 7 tests)
- [x] `runner_loop.rs` (循环编排; 6 tests)

### M6 后端服务 ✅ 完成 2026-11-20
- [x] `config.rs` (TOML 配置; 1 test)
- [x] `auth.rs` (Argon2 + JWT; 3 tests)
- [x] `crypto.rs` (AES-256-GCM; 2 tests)
- [x] `db/mod.rs` (sqlx pool; 1 test)
- [x] `db/models.rs` (User/Account/Params models)
- [x] `db/schema.rs` (SQLite DDL)
- [x] `db/repository.rs` (CRUD 操作; 3 tests)
- [x] `middleware.rs` (JWT 拦截器; 1 test)
- [x] `api/routes.rs` (20+ REST endpoints; 1 test)
- [x] `ws.rs` (WebSocket hub; 1 test)
- [x] `supervisor.rs` (Backoff FSM; 2 tests)
- [x] `calendar.rs` (财经日历; 3 tests)
- [x] `runtime.rs` (账户 task 运行器; 1 test)
- [x] `events.rs` (event 转换; 2 tests)
- [x] `main.rs` (Axum 服务器)
- [x] E2E 集成测试 (4 scenarios)

### M4 多账户服务 + Web ✅ 完成 2026-11-20
后端：
- [x] 配置、DB 迁移、`DbWriter`
- [x] 用户 / JWT / RBAC / 审计
- [x] 账户注册表、凭证加密（AES-256-GCM）、预检、Supervisor
- [x] REST 全部端点（06）+ 结构化路由
- [x] WS Hub（订阅、节流、权限广播）
- [x] 通知路由：站内 WS / 系统事件
- [x] 静态托管 `web/dist`、优雅停机
前端：
- [x] 脚手架、Dark Slate 现代主题、鉴权、路由
- [x] 总览页、账户详情页（HudBar、BasketCard、QuickActions、PositionsTable、EventStream）
- [x] 参数编辑与实时保存校验
- [x] 账户新增向导、启停、授权
- [x] 告警中心、系统页、审计页
- [x] 实时行情走势与持仓图表联动

### M5 灰度与部署交付 ✅ 完成 2026-11-20
- [x] systemd 单元文件 (`gl-trader.service`)、Docker 容器化 (`Dockerfile`)
- [x] 编译与安装脚本：`build.sh`、`install.sh`、`build_check.ps1`
- [x] 故障演练与异常保护：断网指数退避、Exness 429 限流保护、幂等重放
- [x] 完整端到端集成测试（4 个 E2E 场景全部通过）
- [x] 文档：交付总结 (`DELIVERY_FINAL.md`)、验收清单 (`VERIFICATION_CHECKLIST.md`)

### 2026-11-21 维护与加固 ✅
- [x] **全量源文件注释中文化**：77 个源文件（Rust 69 + TSX 8）、1066 行英文注释全量汉化，零回归、零编译警告
- [x] **策略与风控规则跨端对齐**：原生兼容标准（USD）/美分（USC）账户、千元整数风控阶梯、方案 A 新闻避险（只拦首单不拦加仓）、高安全垫放飞/低安全垫秒平的分层止盈机制
- [x] **文档体系同步更新**：与 MQL5 核心及 AGENTS.md 准则完全一致

---

## 3. 完成定义（DoD）

- 代码：`cargo fmt`、`cargo clippy -D warnings`、`cargo test` 通过；前端 `tsc --noEmit`、`eslint` 通过。
- 策略相关改动必须附带：黄金向量或场景测试；02 文档同步更新。
- 任何新端点必须进入 utoipa 文档并有集成测试。
- 影响持久化结构的改动必须附 sqlx 迁移脚本且可从上一版本升级。

---

## 4. 风险登记

| 风险 | 影响 | 缓解 |
| --- | --- | --- |
| ~~对冲持仓实测与预期不符~~ 已实测并存 | — | M1 已确认，风险关闭 |
| 接入点 / Base URL 变更 | 全部账户断连 | 配置化 + 发现回退 + 告警 |
| 异步成交导致重复开仓 | 超额仓位 | `pending_op` 守卫 + 幂等键 + 对账 |
| Tick 节流导致与 MT5 回测差异 | 验收口径争议 | 在验收标准中接受价格级误差，只比对规则级 |
| 财经日历服务不可用 | 全部账户禁首单 | 与 EA 行为一致（安全侧）；告警；可配置备用源 |
| 单进程故障域 | 全账户同时中断 | Supervisor、状态落盘、快速重启 + 对账；规模上来后分片 |
| 时钟漂移 | 签名失败 | NTP 校准、启动时检测、告警 |
| 凭证泄露 | 资金风险 | 加密存储、不回显、审计、最小权限 API Key（在 Exness 侧仅授权必要范围与 IP 白名单） |

---

## 5. 决策记录

- 2026-11-19：单品种黄金；不做商业分发形态（客户账户全部托管在我方服务器，权限由 RBAC 承担）；对冲并存按理论支持处理，M1 实测确认。
- 2026-11-20：通知通道采用站内 WS；不配置邮件（SMTP），Telegram 暂不启用。手动动作**不做二次密码验证**，仅校验 JWT + `operate` 权限并写审计，前端以确认弹窗防误触。

当前无待决策事项。
