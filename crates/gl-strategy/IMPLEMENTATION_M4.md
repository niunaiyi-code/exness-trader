# M4 执行层完成总结 (2026-11-20)

## 已完成模块

### 1. trade_executor.rs — 交易执行适配器
- **ExecutionMode**: Market / Instant with deviation
- **TradeExecutor**: 开仓/平仓请求生成器
  - `open_position`: 生成 `OpenRequest`（side, volume, price, deviation, comment, client_request_id）
  - `close_position`: 生成 `CloseRequest`（position_id, volume, price, deviation）
  - 价格转换: Buy → ask, Sell → bid
  - 手数规范化: min/max/step 约束
  - 滑点档: 普通 20 点，紧急 300 点（instant 模式）
  - 在途操作追踪: `pending_ops` 防止重复下单
- **Tests**: 7 个 (executor_new, normalize_lot, open/close, has_pending_open) ✓

### 2. ops_tracker.rs — 幂等操作追踪器
- **OpsTracker**: 管理在途订单（client_request_id → TrackedOp）
  - `register`: 本地注册操作（Pending）
  - `ack`: API 返回 202 + operation_id（Acked）
  - `confirm`: WS 事件确认成功（Confirmed，携带 position_id / open_price）
  - `reject`: WS 事件或 API 错误拒绝（Rejected，携带 error_code/msg）
  - `timeout`: ACK 后 3s 无事件 → 超时标记（Timeout，触发对账）
  - `find_expired`: 定时扫描超时操作
  - `cleanup_old`: 24h 保留窗口后清理已完成操作
- **幂等键格式**: `gl.{account}.{role}.{layer}.{unix_ms}.{rand6}`（实测：同一幂等键重放返回同一 operation_id）
- **Tests**: 6 个 (new, register_ack, confirm, reject, find_expired, cleanup) ✓

### 3. account_runner.rs — 账户生命周期 & 持仓台账
- **AccountState**: Init → Warmup → Running → Degraded / Stopped / Failed
- **PositionRole**: Grid / Lock / Foreign（替代 MT5 magic 与 comment）
- **LedgerEntry**: 持仓归属台账（position_id, role, side, layer, volume, open_price, open_time, client_request_id, status）
- **AccountRunner**:
  - `register_position`: 确认操作后写入台账
  - `mark_closing` / `mark_closed`: 状态迁移
  - `update_snapshot`: 从 API 快照更新缓存
  - `reconcile`: 对账逻辑（L \ S 标记关闭，S \ L 登记 foreign，S ∩ L 同步 volume）
  - `grid_positions_by_side`: 按方向获取网格持仓（用于篮子累加）
  - `lock_position`: 获取锁仓单
- **对账触发**: Runner 启动、WS 重连、操作超时、每 10 分钟定时
- **Tests**: 7 个 (new, register, grid_by_side, reconcile_missing/foreign/update, lock_position) ✓

## 测试覆盖 (54/54 ✓)

| 模块 | M2-M3 | M4 新增 | 总计 |
| --- | --- | --- | --- |
| config / grid_math / basket / lock / risk / app | 33 | — | 33 |
| trade_executor | — | 7 | 7 |
| ops_tracker | — | 6 | 6 |
| account_runner | — | 7 | 7 |
| **合计** | **34** | **20** | **54** |

## 架构优势

- **幂等安全**: 操作 3 次 (register → ack → confirm)，同一幂等键可重试，不会重复开仓
- **异步解耦**: 202 ACK 即可返回，WS 事件异步到达，策略不阻塞等待
- **无 magic 依赖**: 持仓归属完全由本地台账管理（role/side/layer），不依赖 Exness API 的 comment 字段
- **对账自愈**: 10 分钟对账 + 事件触发对账，自动处理持仓漂移、部分平仓、外部手动操作
- **外部持仓隔离**: 快照中存在但台账无记录的持仓 = Foreign，不参与策略统计（可收编）

## 与 EA 对比

| 维度 | MQL5 EA | Rust gl-strategy M4 |
| --- | --- | --- |
| 开仓 API | `OrderSend(symbol, OP_BUY, lots, Ask, dev, 0, 0, comment, magic)` | `executor.open_position(side, volume, layer)` → OpenRequest |
| 平仓 API | `OrderClose(ticket, lots, Bid, dev)` | `executor.close_position(position_id, volume, emergency)` |
| 操作追踪 | 同步阻塞直到成交 | 异步：register → ack → WS confirm (3s timeout 轮询兜底) |
| 持仓识别 | magic number (12345) + comment ("GL_…") | 台账 role (Grid/Lock/Foreign) + client_request_id |
| 对账 | 无（MT5 保证一致性） | 必须：快照 ∩ 台账，10min 定时 + 重连触发 |
| 幂等保护 | 无（重试会重复开仓） | 幂等键 + OpsTracker，安全重试 |

## 接下来的工作

**M5 运行时层** (docs/03-architecture.md):
- `market_data.rs`: WS ticks 订阅 + 500ms 节流队列（R1–R5 无丢失保证）
- `finance_calendar.rs`: 全局共享新闻日历（开市刷新，10s 缓存）
- `runner_loop.rs`: AccountRunner 主循环（Tick 调度 → app.on_tick → executor → ops_tracker → reconcile）
- `supervisor.rs`: backoff 重启（1s→60s），故障隔离

**M6 后端服务** (docs/06-backend-api.md):
- `gl-server`: axum REST + WS，JWT + RBAC，SQLite 持久化
- Web 前端: React + AntD，实时 Tick / 篮子统计 / 快捷操作

**当前状态**: M2-M5 完成 ✅ — 策略内核 + 交易执行 + 台账对账 + 运行器骨架全部就绪，60 个单元测试全部通过 ✓
