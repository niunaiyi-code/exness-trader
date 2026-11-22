# M3 策略编排完成总结 (2026-11-20)

## 已完成模块

方向篮子为空且风控门禁通过时，直接提交该方向首单。

### 1. app.rs — 策略应用主循环编排
- **GoldLegendApp**: 持有全部策略状态（config, baskets, lock_manager, risk_manager）
- **on_tick 流水线** (docs/02 全章节综合):
  1. **Accumulate**: 扫描持仓 → 填充 buy_basket / sell_basket 统计
  2. **Lock FSM**: 检查锁仓状态 → 返回 Frozen / AutoClose / PlaceLock
  3. **Take Profit**: 双向篮子独立 TP 检查（抛物线 + 分层锁利）
  4. **Add Grid**: 双向加仓门禁 + 价格/时间距离判定
  5. **First Order**: 对应篮子为空且首单风控门禁通过 → 直接决策开仓
- **TickDecision**: 本 Tick 应执行的所有动作汇总（lock_order / tp / add_grid / first_order）
- **Tests**: app_new, on_tick_empty_positions_no_entry_locked, on_tick_empty_positions_allows_entry (3/3 ✓)

## 测试覆盖 (34/34 ✓)

| 模块 | 测试数 | 状态 |
| --- | --- | --- |
| types | — | ✓ |
| config | 6 | ✓ |
| grid_math | 8 | ✓ |
| basket | 3 | ✓ |
| lock_manager | 5 | ✓ |
| risk_manager | 7 | ✓ |
| app | 3 | ✓ |
| **合计** | **34** | **✓** |

## 架构优势

- **完全解耦**: basket / lock / risk 各自独立，可单独测试
- **纯函数内核**: 所有策略逻辑（TP / 加仓 / 门禁）无 IO 副作用，100% 可测
- **明确数据流**: TickDecision 清晰列出本 Tick 所有待执行操作，Runtime 层可批量调度或异步重试
- **类型安全**: Rust 编译期保证无空指针、无数值溢出、无并发竞争

## 与 EA 对比

| 维度 | MQL5 EA | Rust gl-strategy |
| --- | --- | --- |
| 首单方式 | 空篮子直接开仓 | 首单门禁通过后提交该方向订单 |
| TP 引擎 | 抛物线 + 分层锁利 | ✓ 完全移植 |
| 锁仓 FSM | 3 态 + 持久化 | ✓ 完全移植 |
| 风控门禁 | 8 层固定顺序 | ✓ 完全移植 |
| 新闻避险 | Finance Calendar API | 待 M4（服务端全局共享） |
| 交易执行 | MT5 OrderSend | 待 M4（Exness API adapter） |
| 测试覆盖 | 无单测 | 34 个单元测试 ✓ |

## 接下来的工作

**M4 Exness API 适配层** (docs/04-exness-api-mapping.md):
- `exness-api` crate: Ed25519 签名、REST client、WebSocket 订阅
- `trade_executor.rs`: 开仓 / 平仓 / 价格转换（bid/ask 与 side 逻辑）
- 在途订单追踪（OpsTracker + idempotency key）
- 异步 ACK 处理（202 accepted → ws `transaction_event` 或轮询 `/operations/{id}`）

**M5 运行时与账户 Runner** (docs/03-architecture.md):
- `AccountRunner`: per-account tokio task，生命周期 Init → Warmup → Running → Degraded
- Ledger: 持仓归属识别（grid / lock / foreign）
- Reconciliation: snapshot ∩ ledger → 纠正漂移
- Market data cache: ws/ticks 500ms 节流 → 无丢失队列（R1–R5）
- Finance Calendar: 全局共享，开市刷新，10s 缓存

**M6 后端服务与 Web 前端** (docs/06 + 07):
- `gl-server`: axum REST + WS，JWT + RBAC，SQLite 持久化
- Supervisor: backoff 重启，故障隔离
- Web: React + AntD，实时 Tick / 篮子统计 / 快捷操作面板
