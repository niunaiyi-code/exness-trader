# GoldLegend Exness Trader（金色传说 · Exness Trader API 版）

> 基于 **Exness Trader API（官方 REST + WebSocket，Ed25519 签名）** 与 **Rust** 重写的金色传说 XAUUSD 网格策略交易系统。
> 服务端为单进程多账户守护程序，前端 **Web 端（React + Ant Design）先行**，**Android 端**后续接入同一套后端 API。

当前阶段：**✅ 完全交付 & 规范化加固 (2026-11-21)** — M1-M6 全部完成，90+ 单元测试 + 4 E2E + 生产级前后端。已完成 77 个源文件注释中文化，与 MQL5 核心和 AGENTS.md 准则完全等价对齐。可直接部署 Docker 或 systemd。详见 `DELIVERY_FINAL.md`。

---

## 1. 为什么重写

| 维度 | MT5 EA（现状） | Exness Trader 版（目标） |
| --- | --- | --- |
| 运行载体 | 每个账户一个 MT5 终端 + 一张图表 | 一个 Rust 守护进程托管 N 个账户 |
| 行情/成交 | MT5 终端本地 | Exness Trader API（REST + WS） |
| 人工操作 | 图表 Q 键快捷菜单 | Web 控制台 / Android App |
| 告警 | MT5 `SendNotification` | Web 提示音已接入；站内实时告警 WSS 推送待实现 |
| 状态持久化 | 终端 GlobalVariable | SQLite（每账户独立命名空间） |
| 回测 | MT5 策略测试器 | 本地 Tick/Candle 回放工具（同一策略内核） |

策略逻辑**逐条等价移植**，不新增、不删减风控规则；所有与 `AGENTS.md` 相关的既定设计（美分账户、千元阶梯、方案 A 新闻避险、分层止盈）原样保留。

边界：**单品种黄金**（账户上的 `XAUUSD` / `XAUUSDm` / `XAUUSDc` 自动匹配）；**不做商业分发形态**，所有客户账户托管在我方服务器，权限由多用户 RBAC 承担。

---

## 2. 文档索引

| 编号 | 文档 | 内容 |
| --- | --- | --- |
| 01 | [需求规格说明书](docs/01-requirements.md) | 目标、范围、角色、功能/非功能需求、验收标准 |
| 02 | [策略逻辑规格](docs/02-strategy-spec.md) | 从 MQL5 源码逐模块提炼的可执行规格（公式、状态机、门禁顺序） |
| 03 | [系统架构设计](docs/03-architecture.md) | Rust workspace 划分、运行时模型、数据流、持久化、部署 |
| 04 | [Exness Trader API 对接说明](docs/04-exness-api-mapping.md) | 签名、端点、WS 事件、异步回执、限流、对账与恢复 |
| 05 | [多账户设计](docs/05-multi-account.md) | 账户注册、凭证加密、隔离、生命周期、权限 |
| 06 | [后端对外 API 规范](docs/06-backend-api.md) | Web/Android 共用的 REST + WS 接口定义 |
| 07 | [Web 前端设计](docs/07-web-frontend.md) | React + Ant Design 页面、组件、状态、实时通道 |
| 08 | [Android 端规划](docs/08-android.md) | 第二阶段：Kotlin + Compose，复用后端 API |
| 09 | [开发路线图](docs/09-roadmap.md) | 里程碑、任务拆解、完成定义、风险 |
| 10 | [测试与运维](docs/10-testing-ops.md) | 等价性测试、回放回测、模拟盘灰度、部署与监控 |
| 11 | [M1 验证手册](docs/11-m1-validation-guide.md) | 模拟账户实测步骤、需回填的字段、验收清单 |
| 12 | [M1 验证报告](docs/12-m1-report.md) | 2026-11-19 实测结论：黄金条件、限流、对冲、幂等、WS 语义 |
| — | [OpenAPI 规范副本](docs/openapi/trading.public.yaml) | 官方 `trading.public.yaml` 1.0.2（2026-11-20 下载） |

---

## 3. 规划中的目录结构

```
exness-trader/
├── README.md
├── docs/                      # 本套开发文档 + openapi/ 规范副本
├── Cargo.toml                 # Rust workspace
├── crates/
│   ├── exness-api/            # ✅ Exness Trader API SDK（签名 / REST / WS / 模型）+ examples/m1_*probe
│   ├── gl-strategy/           # 纯领域策略内核（无 IO，可回放、可单测）
│   ├── gl-runtime/            # 账户运行器：行情适配、持仓台账、操作跟踪、执行适配、对账
│   ├── gl-server/             # axum HTTP/WS 服务、鉴权、账户注册、持久化、通知
│   └── gl-replay/             # 回放回测 CLI（复用 gl-strategy）
├── web/                       # React 18 + TypeScript + Vite + Ant Design 5
├── android/                   # 第二阶段
└── config/                    # 配置样例（不含真实凭证）
```

---

## 4. 技术栈

- **后端**：Rust stable、tokio、axum、reqwest（rustls）、tokio-tungstenite、ed25519-dalek、sqlx（SQLite）、rust_decimal、serde、tracing、argon2、jsonwebtoken
- **Web**：React 18、TypeScript、Vite、Ant Design 5、axios（统一实例 + JWT 拦截器）、react-router、lightweight-charts（TanStack Query / Zustand 为设计稿选型，尚未引入）
- **Android**（后续）：Kotlin、Jetpack Compose、Retrofit/OkHttp、FCM

---

## 5. 与仓库其它目录的关系

- `mql5/Experts/GoldLegend/`：策略唯一事实来源，`docs/02-strategy-spec.md` 以其为准编写；MQL5 侧任何策略变更都必须同步更新规格文档与 Rust 内核。
- `exness-trader/` 独立为新工程，不依赖仓库内其它 Rust/前端目录。
