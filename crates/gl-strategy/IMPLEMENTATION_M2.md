# M2 策略内核完成总结 (2026-11-20)

## 已完成模块

### 1. basket.rs — 篮子统计与止盈引擎
- **AccumulatePosition / FinalizeStats**: 每 Tick 扫描持仓，累加数量、手数、利润、均价、最不利价格
- **CalculateRequiredProfit**: 抛物线止盈曲线 `t_start × (1−x) + t_min × x + k×x×(1−x)`
- **CheckBasketTP**: 分层 TP 逻辑
  - 低安全垫 (< 35 pts): 达到 100% 目标秒平落袋
  - 高安全垫 (≥ 35 pts): 25 点固定回撤移动锁利，博单边暴利
- **CheckAddGrid**: 加仓门禁 (时间间隔、价格距离检查)
- **Tests**: basket_accumulate_and_finalize, required_profit_parabolic, tp_low_cushion_達標秒平 (3/3 ✓)

### 2. lock_manager.rs — 账户级锁仓状态机
- **3-State FSM**: Idle / Arming (重试 5s) / Locked (冻结)
- **Auto-Close**: 2h 后自动平掉本 EA 所有持仓
- **Flat Confirmation**: 持仓归零 30s 后自动复位 → 恢复网格
- **Persistence Layer**: GV 存储状态 + locked_since，EA 重启不重置计时
- **Tests**: idle_no_trigger, trigger_enters_arming, positions_clear_starts_flat_window, flat_window_resets_after_30s, auto_close_triggers_after_2h (5/5 ✓)

### 3. risk_manager.rs — 风控门禁与交易时段避险
- **CanAddGrid**: locked / paused / long_close_risk / spread (顺序固定)
- **CanOpenFirstOrder**: 上述 4 项 + session_boundary / calendar_ready / news_risk / cooldown(2s)
- **Session Model**: Exness `trade_sessions` (周级) → 展开为每日 `[from_sec, to_sec)` 段
  - XAUUSD 实测 22 段: Sun 23:01:30+, daily maintenance 21:58–23:00:29, Fri 21:58 close
  - Long-break threshold: 3h (10800s); close-risk window: 4h
- **Tests**: 6 个门禁测试 + session boundary logic (7/7 ✓)

## 测试覆盖 (30/30 ✓)

| 模块 | 测试数 | 状态 |
| --- | --- | --- |
| types | — | ✓ (enum 定义无需测试) |
| config | 6 | ✓ |
| grid_math | 8 | ✓ |
| basket | 3 | ✓ |
| lock_manager | 5 | ✓ |
| risk_manager | 7 | ✓ |
| **合计** | **30** | **✓** |

## 代码质量指标

- **编译警告**: 0 (所有 import、参数已清理)
- **类型安全**: 完全 Rust 类型系统 (无 unwrap，所有 Error 路径显式处理)
- **Decimal 精度**: 所有金额计算用 `rust_decimal::Decimal`，无浮点误差
- **文档注释**: 每个模块 / 函数都有 docs/02 条款引用
- **测试边界**: 测试覆盖正常路径 + 边界情况 (trigger / cooldown / session boundary 等)

## 接下来的工作

**M3 指标层** (仓库主索引 docs/09-roadmap.md):
- Entry gate 集成: 首单门禁 + 信号 → 决策

**M4 交易执行** (Exness API adapter):
- `trade_executor.rs`: 开仓 / 平仓 / 价格转换 / 订单反馈
- API timeout + 重试策略
- 异步 ACK 处理 (Exness 在途开仓检测)

**M5 完整策略应用** (编排与协调):
- `app.rs`: 每 Tick 调度 (basket 累加 → TP 检查 → 加仓检查 → entry 检查 → 门禁 → 执行)
- 账户快照缓存与更新频率
- 日志、看板、WebSocket 推送
