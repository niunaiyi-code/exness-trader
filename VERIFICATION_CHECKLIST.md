# 策略一致性验证清单
**日期**: 2026-11-21  
**目标**: 确保 Rust 实现与 MQL5 EA 完全一致

---

## ✅ 已完成的修复

### 1. **止盈引擎修复** (tp_engine.rs)
- **问题**: 高安全垫时要求利润超过 `required + 25点buffer` 才激活 trailing
- **修复**: 达到 100% 目标且安全垫 ≥ 35点立即激活 trailing
- **验证**: 测试通过 `cargo test --package gl-runtime test_tp_parabolic`

### 2. **数据库配置匹配 MQL5**
- **问题**: 账户仍绑定较保守的网格参数
- **修复**: 创建 `strat_mql5_default` 并切换账户策略
- **当前配置**:
  ```
  账户: 18000069657
  策略: strat_mql5_default
  Grid Step: 300点
  Grid Step Add: 75点
  Lot Mult: 1.45
  TP Distance: $1.00
  Lock Trigger: $1000
  ```

### 3. **代码逻辑验证**

#### ✅ 空篮子开仓 (pipeline.rs)
风控门禁通过后，对所有空篮子生成首单决策。

#### ✅ 加仓距离计算 (pipeline.rs:154-155, 184-185)
```rust
let step_points = Decimal::from(config.params.grid_step)
    + Decimal::from(config.params.grid_step_add) * Decimal::from(count.saturating_sub(1));
let distance_points = (lowest - ask) / config.constants.pip;
let distance_ok = distance_points >= step_points;
```

#### ✅ 移动锁利状态机 (account_runner.rs:281-388)
- `TrailingState` 完整实现
- 棘轮跟踪 `peak_exit_price`
- 25点回撤触发平仓

---

## 🎯 用户问题解答

### 问题1: "订单到了止盈目标，为什么没有卖出？"
**根本原因**: 旧版代码在高安全垫时不会立即激活 trailing  
**当前状态**: ✅ 已修复，达到 100% 目标立即激活  
**验证方法**: 观察日志中的 `TpDecision::Trail` 触发时机

### 问题2: "加仓也不对"
**检查结果**: ✅ 代码逻辑正确，使用递增步长公式  
**当前配置**: 300点基础 + 75点递增/层  
**可能原因**: 之前数据库配置是 400点+100点 (strat_conservative)

### 问题3: "不是即时对冲开单"
**当前行为**: 不存在入场模式或技术指标判断；空篮子且风控门禁通过时直接生成对应方向的首单决策。  
**验证方法**: 无持仓时应同时生成 Buy 和 Sell 首单决策

---

## 📊 MQL5 vs Rust 参数对照表

| 参数 | MQL5 EA 默认 | Rust strat_mql5_default | 状态 |
|------|-------------|------------------------|------|
| Grid Step | 300 | 300 | ✅ |
| Grid Step Add | 75 | 75 | ✅ |
| Grid Step Sec | 15 | 15 | ✅ |
| Initial Lot | 0.01 | 0.01 | ✅ |
| Lot Multiplier | 1.45 | 1.45 | ✅ |
| TP Distance | $1.00 | $1.00 | ✅ |
| Lock Trigger | $1000 | $1000 | ✅ |
| Trailing Cushion | 35点 | 35.0 | ✅ |
| Trailing Step | 25点 | 25.0 | ✅ |

---

## 🔍 运行时验证步骤

### 1. 检查数据库配置
```bash
sqlite3 gl.db "SELECT a.account_id, s.name, s.grid_step, s.grid_step_add FROM accounts a JOIN strategies s ON a.strategy_id = s.id;"
```
**预期输出**:
```
18000069657|MQL5研发默认参数|300|75
```

### 2. 启动程序并观察日志
```bash
cargo run --release --bin exness-trader
```

### 3. 关键日志检查点

#### ✅ 即时对冲开单
```
[INFO] Opening first order: Buy, lot=0.01
[INFO] Opening first order: Sell, lot=0.01
```

#### ✅ 加仓触发
```
[INFO] Grid add: distance=300.5 points >= step=300 points
[INFO] Adding grid order: Buy, lot=0.0145, count=1
```

#### ✅ 止盈激活
```
[INFO] TP decision: Trail, profit=$1.02 >= required=$1.00, cushion=36.2 points
[INFO] Trailing activated: peak_exit_price=2651.35
```

#### ✅ 移动锁利触发
```
[INFO] Trailing pullback: 26.3 points >= 25.0 points
[INFO] Closing grid: trailing stop triggered
```

---

## 🚨 已知差异（非 Bug）

1. **新闻日历**: MQL5 使用 `FinanceCalendar`，Rust 暂未实现（仅阻止首单）
2. **GL_LOG 宏**: MQL5 商业版会禁用日志，Rust 无此限制
3. **时间同步**: Rust 使用系统时间，MQL5 使用 MT5 服务器时间

---

## 📝 下次运行前检查

- [ ] 确认账户 API 密钥已配置
- [ ] 确认 `gl.db` 中账户策略为 `strat_mql5_default`
- [ ] 确认 MT5 账户无持仓（测试即时对冲）
- [ ] 观察首次 tick 是否同时开多空
- [ ] 观察加仓距离计算是否正确
- [ ] 观察止盈激活和回撤平仓

---

**编译状态**: ✅ Release build successful  
**测试状态**: ✅ TP engine tests passed  
**配置状态**: ✅ Database aligned with MQL5
