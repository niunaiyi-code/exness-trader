# 02 · 策略逻辑规格（从 MQL5 源码逐模块提炼）

> 本文是 Rust 内核 `gl-strategy` 的实现规范。每一节都标注对应的 MQL5 源文件，公式与判定顺序必须逐条等价。
> 「点」= 策略点 = 0.01 美元报价距离；金额单位 = 账户币种单位（USD 或 USC，不换算）。

---

## 1. 参数与常量

### 1.1 可配置参数（对应 `GoldLegendEA.mq5` input 与 `SGoldLegendConfig`）

| 参数 | 类型 | 默认 | 校验（`CGoldLegendApp::ValidateConfig`） | 说明 |
| --- | --- | --- | --- | --- |
| `initial_lot` | Decimal | 0.01 | > 0 | 首单手数 |
| `grid_step` | i32 点 | 300 | ≥ 50 | 网格基础步长（$3.00） |
| `grid_step_add` | i32 点 | 75 | ≥ 0 | 每层步长递增 |
| `grid_step_sec` | i32 秒 | 15 | ≥ 0 | 加仓时间防抖 |
| `lot_mult` | Decimal | 1.45 | ≥ 1.0 | 手数倍率 |
| `tp_distance_usd` | Decimal | 1.00 | > 0 | 首单基准止盈空间（$1.00 = 100 点） |
| `lock_trigger_usd` | Decimal | 1000.0 | > 0 | 账户级总浮亏锁仓触发金额，同时是层数推导的风险预算 |

### 1.2 内部固定值（EA 中写死，不暴露给用户）

| 名称 | 值 | 来源 |
| --- | --- | --- |
| `comment` 前缀 | `GoldLegend` | 订单注释 `GoldLegend_Buy_N` / `GoldLegend_Sell_N` |
| `deviation` | 20 点（$0.20） | 常规滑点档 |
| `emergency_deviation` | 300 点（$3.00） | 紧急平仓滑点档 |
| `max_spread_usd` | 0.50 | 开仓/加仓点差上限 |
| `news_pre_min` / `news_post_min` | 60 / 180 | 新闻避险窗口 |
| `MIN_ENTRY_COOLDOWN_SEC` | 2 | 首单冷却 |
| `SESSION_OPEN_BLOCK_SEC` / `SESSION_CLOSE_BLOCK_SEC` | 1800 / 1800 | 开市后 / 休市前禁首单 |
| `LONG_CLOSE_BLOCK_SEC` | 14400 | 长休市前 4h 禁开仓与加仓 |
| `SESSION_LONG_BREAK_MIN_SEC` | 10800 | 判定长休市的最小空档 |
| `LOCK_RETRY_SEC` | 5 | 锁仓单失败重试 |
| `LOCK_DEV_ESCALATED_UNITS` | 50 点 | 锁仓单第二档滑点 |
| `LOCK_STALE_WARN_SEC` | 3600 | 冻结超时告警 |
| `FLAT_CONFIRM_SEC` | 30 | 持仓归零确认 |
| `AUTO_CLOSE_AFTER_SEC` | 7200 | 冻结 2h 自动全平 |
| `LOCK_PUSH_THROTTLE_SEC` | 600 | 锁仓类推送节流 |
| `NEWS_CACHE_SEC` | 10 | 新闻判定缓存 |
| `GATE_LOG_THROTTLE_MSC` | 60000 | 门禁日志节流 |
| `LOCK_LOG_THROTTLE_SEC` | 600 | 锁仓日志节流 |
| `GRID_ADD_REENTRY_MSC` | 1000 | 同层加仓防重入 |
| `GRID_CLOSE_RETRY_MSC` / `GRID_CLOSE_RETRY_MAX` / `GRID_CLOSE_LOG_THROTTLE_MSC` | 200 / 5 / 10000 | 平仓重试 |
| `GRID_TP_MIN_PROFIT_USD` | 1.00 | 最后一层保本底线 |
| `GRID_TP_PARABOLA_K` | 9.0 | 抛物线曲率 |
| `GRID_TP_TRAILING_POINTS` | 25 | 移动锁利固定回撤 |
| `MIN_TRAILING_CUSHION_POINTS` | 35.0 | 允许放飞的最小安全垫 |
| `AUTO_CLOSE_MAX_SPREAD_USD` | 0.80 | 自动全平点差门禁 |
| `DEFAULT_MAX_ORDERS` | 8 | 兜底层数 |
| `USD_PER_POINT` | 0.01 | 1 点的报价距离 |
| `EPS_PROFIT_USD` / `EPS_PRICE_POINTS` / `EPS_LOT_VOLUME` | 1e-4 / 1e-6 / 1e-6 | 浮点容差 |

### 1.3 从品种 / 账户推导的运行时常量（对应 `CTradeExecutor::Init`）

MT5 读 `SYMBOL_TRADE_TICK_VALUE / TICK_SIZE / DIGITS / POINT`。Exness 侧用 `GET .../instruments/{instrument}/conditions` 与账户详情替代：

| 量 | MT5 口径 | Exness 口径 |
| --- | --- | --- |
| `point` | `SYMBOL_POINT` | `10^-point_digits` |
| `pip`（策略点） | digits ∈ {3,5} ? point×10 : point | `point_digits` ∈ {3,5} ? point×10 : point；黄金预期 0.01，不等于时记录 `PIP NOTICE` 警告 |
| `profit_per_dollar_per_lot` | `tick_value / tick_size` | `contract_size × ccy_factor`，`ccy_factor` = 账户币种 USC 时 100，USD 时 1（要求 `quote_currency == USD`，否则预检失败） |
| `lots_min / lots_max / lots_step` | `SYMBOL_VOLUME_*` | `volume_min / volume_max / volume_step` |
| `deviation_points`（提交给 API 的 deviation） | units × (digits∈{3,5} ? 10 : 1) | 同左；仅 `exec_mode=instant` 时有意义 |

黄金标准账户示例：`contract_size=100`，USD 账户 → `profit_per_dollar_per_lot = 100`（1 手波动 $1 = $100）；USC 账户 → 10000（单位为美分）。

实测（2026-11-19，XAUUSD @ 模拟账户 18000069657）：`point_digits=3` → `point=0.001`、`pip=0.01`（符合预期，无 `PIP NOTICE`）；`contract_size=100`；`volume_min/step/max = 0.01/0.01/200`；`calc_mode=forex`、`margin_percent=1`、无佣金；`swap_long=-598.3`、`swap_short=0`（绑定账户为 Swap-Free，按 AGENTS.md 不计 swap 成本）。`exec_mode=market`。

### 1.4 账户与品种前置校验（对应 `OnInit`、`ValidateConfig`）
1. 从账户可用品种列表自动匹配黄金符号：`XAUUSD` → `XAUUSDm` → `XAUUSDc`（其它含 `XAU` / `GOLD` 的名称兜底）；无匹配则预检失败。
2. 账户币种 ∈ {USD, USC}。
3. 账户须支持对冲（多空并存）。**已实测确认**（2026-11-19：同品种 0.01 多 + 0.01 空并存于快照）。运行期若快照出现同方向净额合并迹象则告警停止。
4. §1.1 参数校验全部通过。
5. `trade_mode == enabled`，`account_status == active`（`close_only` 时允许启动但进入「只平不开」等价于暂停）。

---

## 2. 网格数学（`GridMath.mqh`、`TradeExecutor.mqh`）

### 2.1 手数归一化 `normalize_lots(lots)`
```
lots = round(lots / step) * step
lots = clamp(lots, lots_min, lots_max)
按 step 的小数位数四舍五入
```

### 2.2 下一层手数 `calc_next_lot(initial, mult, count, last_lot)`
```
if mult <= 1.0: return normalize_lots(initial)
target = initial * mult ^ count          // count = 当前已有层数
next   = normalize_lots(target)
if next < last_lot: next = last_lot      // 防小手数截断倒退
return next
```
层序列：L1 = initial；L(k+1) = calc_next_lot(initial, mult, k, L(k))。

### 2.3 理论浮亏 `theoretical_loss(orders) -> (loss, total_lots)`
```
cum_dist[0] = 0
for j in 1..orders: cum_dist[j] = cum_dist[j-1] + (base_step + (j-1)*step_add)
cur = cum_dist[orders-1]
loss = Σ_k lot_k * ppd * (cur - cum_dist[k]) * pip
total_lots = Σ_k lot_k
```

### 2.4 自动层数上限 `auto_max_orders(budget)`
```
safe = 1
for n in 1..=100:
    (loss, lots) = theoretical_loss(n)
    min_cushion_points = base_step + n*step_add
    cushion_loss = lots * ppd * min_cushion_points * pip
    if loss + cushion_loss <= budget + EPS_PROFIT_USD: safe = n else break
return safe          // base_step <= 0 时直接返回 100；safe < 1 视为初始化失败
```

### 2.5 极限抗波幅 `defensive_survival_usd(max_orders, budget)`
```
(theo_loss, total_lots) = theoretical_loss(max_orders)
span_points = Σ_{s=1}^{max_orders-1} (base_step + (s-1)*step_add)
rem_buffer  = budget - theo_loss
cushion_pts = rem_buffer / (total_lots * ppd * pip)
return span_points*pip + cushion_pts*pip      // 看板「极限抗波幅 $X (N层)」
```

---

## 3. 篮子统计（`GridBasket.mqh` AccumulatePosition / FinalizeStats）

每个 Tick 对**台账内**本账户持仓做一次扫描，分别喂给多篮子、空篮子、账户快照：

| 字段 | 计算 |
| --- | --- |
| `count` | 持仓笔数 |
| `lots` | Σ volume |
| `profit` | Σ (unrealized + swap)；**不含 commission**（与 MT5 `POSITION_PROFIT` 一致） |
| `notional_sum` → `avg_price` | Σ open_price×volume / lots |
| `last_price` / `last_lot` | 对本方向「最不利」的开仓价：BUY 取最低，SELL 取最高，及其手数 |
| `last_order_time` | max(create_time) |

单笔未实现盈亏（API 不返回，本地计算）：
```
exit_price = BUY ? bid : ask
unrealized = (exit_price - open_price) * sign * volume * profit_per_dollar_per_lot   // sign: BUY +1, SELL -1
```
账户快照：`net_lots = Σ buy_vol − Σ sell_vol`，`total_floating_pnl = Σ(unrealized + swap)`，`position_count`。

`count == 0` 时 `FinalizeStats` 复位：`closing_in_progress`、平仓重试计数、移动锁利三元组。

---

## 4. 整组止盈引擎（`CGridBasket::CalculateRequiredProfit` / `CheckBasketTP`）

### 4.1 目标利润
```
n = clamp(count, 1, max_orders)
t_start = initial_lot * ppd * tp_distance_usd
t_min   = GRID_TP_MIN_PROFIT_USD
x = max_orders > 1 ? (n-1)/(max_orders-1) : 0      // 0..1
k = GRID_TP_PARABOLA_K * t_start
req = t_start*(1-x) + t_min*x + k*x*(1-x)
req = max(req, t_min)
```

### 4.2 判定流程（每 Tick，count > 0）
```
if closing_in_progress: close_basket(); return Exit
pip <= 0 → return None
req = required_profit(); req <= EPS → return None

profit_per_pt  = lots * ppd * pip
cushion_points = req / profit_per_pt

if cushion_points < 35 && trailing_active: reset_trailing()
if trailing_active && count != tracked_count: reset_trailing()

if cushion_points < 35:                      // 低安全垫：达标秒平
    if profit >= req - EPS: closing_in_progress = true; close_basket(); return Exit
    return None

exit_price = BUY ? bid : ask                 // 高安全垫：25 点固定回撤移动锁利
if !trailing_active:
    if profit >= req - EPS:
        trailing_active = true; peak_exit_price = exit_price; tracked_count = count
        floor_price = peak - sign*25*pip     // 仅日志/看板
else:
    if sign*(exit_price - peak) > 0: peak = exit_price
    pullback_points = (peak - exit_price)*sign/pip
    if pullback_points + EPS_PRICE_POINTS >= 25:
        closing_in_progress = true; close_basket(); reset_trailing(); return Exit
return None
```
看板需暴露：`req_profit`、`cushion_points`、`trailing_active`、`peak_exit_price`、`floor_price`。

---

## 5. 逆势加仓（`CGridBasket::CheckAddGrid`）

调用方传入 `step = grid_step + (count-1)*grid_step_add`（count ≥ 1，`grid_step_add > 0` 时）。
```
if count == 0 || closing_in_progress || trailing_active || count >= max_orders: return false
if grid_step_sec > 0 && last_order_time > 0 && now - last_order_time < grid_step_sec: return false
if count == count_at_add && now_ms - last_add_ms < 1000: return false
ref = BUY ? ask : bid
distance = (last_price - ref)*sign/pip
if distance + EPS_PRICE_POINTS < step: return false
next_lot = calc_next_lot(initial_lot, lot_mult, count, last_lot)
last_add_ms = now_ms; count_at_add = count
open(side, next_lot, comment = "GoldLegend_{Buy|Sell}_{count}")
成功: last_order_time = now
```
Exness 版补充：开仓为异步 ACK。在该篮子存在**在途开仓操作**时，视同防重入命中（不再提交）。在途操作终态后再由下一 Tick 重新评估。

---

## 6. 首单开仓

仅当多篮子或空篮子为空时评估。首单门禁通过后，直接开启所有空篮子对应方向的首单，不读取 K 线或技术指标：
```
for side in [BUY, SELL]:
    if basket(side).count != 0: continue
    if can_open_first_order(side):
        open_first_order(side, initial_lot)     // comment "GoldLegend_{Buy|Sell}_0"
```

开仓流程不计算技术指标。M1 蜡烛可用于行情展示，不参与首单决策。

---

## 7. 开仓门禁（`RiskManager.mqh`）

上下文 `GateContext { is_locked, is_paused, cur_spread = ask - bid, now, last_close_time(side), last_entry_time(side) }`。

### 7.1 加仓门禁 `can_add_grid`（顺序固定）
1. `is_locked` → 拒
2. `is_paused` → 拒
3. `is_long_close_risk_time()` → 拒（长休市收市前 4h）
4. `max_spread_usd > 0 && cur_spread > max_spread_usd` → 拒
5. 通过。**不检查新闻**（AGENTS §8）。

### 7.2 首单门禁 `can_open_first_order`（顺序固定）
1. 锁仓 → 拒
2. 暂停 → 拒
3. 交易时段边界风险（§7.3）→ 拒（日志 60s 节流）
4. 点差超限 → 拒（日志节流）
5. 日历不可用（从未成功加载）→ 拒（日志节流）
6. 处于新闻窗口 → 拒（日志节流）
7. `now - last_close_time < 2s` 或 `now - last_entry_time < 2s` → 拒
8. 通过。

### 7.3 交易时段模型
MT5 按「当日各时段 [from,to)」判断；Exness `trade_sessions[]` 给的是周级窗口 `week_start_time="Mon, 10:00:00"`、`week_end_time="Fri, 23:59:59"`（UTC）。实现：把周级窗口展开为 7 天 × 若干 `[from_sec, to_sec)` 段（跨日的拆成两段，`to=86400` 表示 24:00），之后沿用 EA 算法。

实测 XAUUSD 返回 **22 段**而非单段：周日 00:00–23:01:29 停牌、23:01:30 起可交易；周一至周四 00:00–21:57:59 可交易，**21:58:00–23:00:29 每日维护停牌**，23:00:30–23:01:59 与 23:02:00–23:59:59 可交易；周五 21:58 起停牌；周六全天停牌。因此展开后每天可能多段，`SESSION_MAX_PER_DAY=16` 足够；每日维护空档约 1 小时，小于 `SESSION_LONG_BREAK_MIN_SEC`(3h)，不会被误判为长休市。

- `scan_today_sessions()`：当天各段；无段 → 回测环境放行，实盘视为「品种规格无当日交易时段」拒首单。
- `find_session_index(now)`：不在任一段 → 「当前处于休市时段」拒首单。
- `gap_after` = 下一段 from − 本段 to（当天无下一段则向后找 ≤7 天，找不到视为 7 天）；`gap_before` 同理向前。
- 长休市：`gap_after ≥ 10800`；`is_long_close_risk_time` = 处于长休市前最后一段且 `to − now ≤ 14400`。
- 边界风险顺序：`lead_close > 14400 && lead_open ≥ 1800` → 无风险；长休市前 4h；`lead_close ≤ 1800 && 0 < gap_after < 10800` → 「休市前 30 分钟」；`lead_open < 1800 && gap_before > 0` → 「开市后 30 分钟」。
- `trade_sessions` 为空或字段缺失时：等价「无当日时段」，实盘禁首单并告警。

### 7.4 新闻避险（`FinanceCalendar.mqh`）
- 数据源 `GET https://www.financecalendar.com/wp-json/fc/v1/calendar?from=D-1&to=D+6&country=US&impact=high&limit=200&offset=0`，头 `X-API-Key`。
- 过滤：`country == "US"`、`impact == "high"`、`status != "cancelled"`、非 `all_day`、`release_at_utc`（或 `time_utc`）可解析且带时区。
- 刷新策略：启动预热；检测到开市（上一次判定休市→本次开市）强制刷新；成功后直到下次开市不再刷新；失败 300s 后重试；已加载过则沿用旧数据。
- 判定：存在事件 `t` 使 `t − 60min ≤ now ≤ t + 180min`，返回事件名；结果缓存 10s。
- 服务端全局共享一份日历（所有账户相同），见 03。

---

## 8. 账户级锁仓状态机（`LockManager.mqh`）

状态：`Idle(0) / Arming(1) / Locked(2)`；持久化 `state` 与 `locked_since`（重启不重置 2h 计时）。

### 8.1 每 Tick / 每秒 `check_and_execute_lock(trigger) -> Run | Frozen`
```
if trigger <= 0: return Run
(net_lots, pnl, total_positions) = snapshot

// 1. 推进状态
if state != Idle:
    if total_positions == 0:
        if flat_since == 0: flat_since = now           // 日志 FLAT PENDING
        if now - flat_since >= 30: reset(); return Run // 日志 LOCK RESET
        return Frozen
    flat_since = 0; (继续)
else:
    if pnl > -trigger: return Run
    state = Arming; locked_since = now; flat_since = 0; persist()   // 日志 LOCK ROUND START
// 2. 归零确认期不下单
if flat_since > 0: return Frozen
// 3. 2h 自动全平
if try_auto_close(): return Frozen
// 4. 1h 超时告警（600s 节流，不动作）
warn_if_stale()
// 5. 已锁定：无论人工怎么平，保持冻结、不补锁
if state == Locked: return Frozen
// 6. Arming：5s 重试节流
if last_lock_attempt > 0 && now - last_lock_attempt < 5: return Frozen
lock_lot = |net_lots|
// 7. 已中性：直接锁定
if lock_lot < lots_min: state = Locked; persist(); return Frozen
// 8. 分级滑点下单
try_place_lock(lock_lot, is_net_long = net_lots > 0)
return Frozen
```

### 8.2 `try_place_lock`
- 手数按 `lots_max` 分笔；每笔先用 `deviation`（20）再升级 `50`；任一笔两档皆失败 → 整体失败，`last_lock_attempt = now`，保持 Arming。
- 成功 → `state = Locked`，持久化，重采快照，推送 `LOCKED` 通知（文案见 §10），作废快照。
- 手动锁仓 `manual_lock()`：Idle 且有持仓时进入 Arming（`locked_since = now`），净敞口 < 最小手数直接 Locked，否则 `try_place_lock`。
- Exness 版：锁仓单为异步操作；Arming 期间存在在途锁仓操作时不重复提交；操作 `confirmed` 后才置 Locked；`rejected/failed` 视为本次失败等待 5s 重试。

### 8.3 `try_auto_close`
```
if state == Idle || locked_since == 0: false
if now - locked_since < 7200: false
if total_positions <= 0: false
if ask - bid > 0.80: 日志 AUTO CLOSE HOLD（600s 节流）; false
failed = close_all(normal); if failed > 0: failed = close_all(emergency)
日志 AUTO CLOSE（600s 节流）
if failed > 0: false
推送 AUTO_CLOSE; true
```

### 8.4 冻结期对主流程的影响
`OnTick` 中 `Frozen` → 立刻返回，不做止盈、加仓、首单；看板与告警由 1 秒定时器维持。

---

## 9. 主循环（`GoldLegendApp.mqh`）

### 9.1 Tick 流水线
```
refresh_quotes(); update_stats()
if run_account_lock() == Frozen: return
exit = buy.check_tp() | sell.check_tp()          // 两侧都评估
if exit || buy.closing || sell.closing: update_stats(); return
if buy.count > 0 && can_add_grid() && buy.check_add_grid(step_buy): update_stats(); return
if sell.count > 0 && can_add_grid() && sell.check_add_grid(step_sell): update_stats(); return
if buy.count == 0 || sell.count == 0: check_new_bar_and_entry()
render()
```

### 9.2 1 秒定时器
```
refresh(); update_stats()
刷新新闻缓存（内部 10s 节流）
if 账户无持仓 || (两篮子为空 && !locked):
    重置统计; if locked: run_account_lock()
    push_alert(); render(); return
if run_account_lock() == Frozen: push_alert(); render(); return
push_alert(); render()
```
Exness 版：Tick 最高 2 次/秒，定时器保留为独立 1s 心跳，承担锁仓推进、告警、看板推送与「无 Tick 时也要推进 2h 计时」。

### 9.3 手动动作（Q 菜单 → `OnChartEvent`）

| 动作 | 实现 |
| --- | --- |
| CloseAll | `close_positions(all, emergency=true)`，两阶段：先网格腿后锁仓腿 |
| LockNow | `manual_lock()` |
| CloseLoss / CloseProfit | 按 `unrealized + swap` 符号筛选平仓 |
| CloseBuy / CloseSell | `basket.close_basket(emergency=true)` |
| TogglePause | 翻转并持久化 `paused` |

动作结束统一：重采统计、刷新看板。

---

## 10. 通知（`NotificationService.mqh`、`LockManager::PushAlert`）

### 10.1 浮亏千元阶梯（每秒评估）
```
if balance <= 0: return
equity = balance + floating_pnl
if buy.count + sell.count == 0: reset(); return
loss = max(0, -floating_pnl)
cur_tier = floor(loss/1000)*1000
if cur_tier < last_tier && loss <= last_tier - 200 && now - last_time >= 60: last_tier = cur_tier   // 静默降档
if cur_tier >= 1000 && cur_tier > last_tier:
    dd_pct = balance > equity ? (balance-equity)/balance*100 : 0
    send("【金色传说·浮亏$%d提醒】账户:%s 净值浮亏已达 -$%.2f (-%.1f%%)! 当前余额=$%.2f, 净值=$%.2f, 多头%d单(%.2f手), 空头%d单(%.2f手)")
    last_tier = cur_tier; last_time = now
```
`balance` 取 Exness `account_state.balance`；USD/USC 统一数值（AGENTS §7）。

### 10.2 锁仓类推送（按 tag 600s 节流）
- `LOCKED`：「【金色传说·锁仓提醒】账户:%s %reason，EA 已锁仓 %SIDE %.2f 手，净敞口 %.2f 手，网格已冻结、不会再补锁。解锁请人工处理；若未处理，锁仓满 2 小时将自动平掉本 EA 全部持仓。」
- `AUTO_CLOSE`：「【金色传说·2小时自动解锁】账户:%s 锁仓已满 2 小时，EA 已自动平掉本 EA 全部持仓 (%d 笔)。平仓前浮亏 $%.2f、净敞口 %.2f 手。持仓归零 30 秒后将自动复位恢复常规网格；若不想继续运行，请手动点击【暂停开仓】。」

---

## 11. 看板数据帧（`SDashboardFrame` + 篮子访问器）

Rust 版每次状态变更推送给前端的结构（详见 06 §5）：
`is_locked, lock_state, locked_since, is_paused, news_avoid, news_event, news_feed_ready, session_avoid_desc, max_grid_orders, max_defensive_usd, digits, bid, ask, spread, utc_now, balance, equity, floating_pnl, net_lots, buy{count,lots,avg,profit,req_profit,cushion,trailing_active,peak,floor,closing}, sell{同}`。

HUD 状态优先级（`CDashboard::Render`）：锁仓（红）> 暂停（橙）> 时段避险 / 新闻避险（黄）；「新闻源不可用」独立追加且在非锁仓非暂停时置红。

---

## 12. MT5 → Exness 语义差异与处理决策

| 差异点 | MT5 | Exness Trader API | 决策 |
| --- | --- | --- | --- |
| 持仓归属 | magic + comment | `Position` 无 magic、无 comment 返回 | 本地台账（04 §6）；仍在开仓请求 `comment` 写 `GoldLegend_*` / `GL_NET_HEDGE` 便于终端肉眼识别 |
| 下单结果 | 同步 retcode | 202 ACK + 异步 `transaction_event` | 操作跟踪器；篮子状态引入 `pending_op` |
| 浮盈亏 | `POSITION_PROFIT` 服务器算 | 不提供 | 本地按对手价计算（§3） |
| 滑点 | `deviation` 总有效 | 仅 `instant` 模式有效；实测黄金为 `market` | 两档/紧急档保留为参数但不生效；以重试策略替代 |
| 时间 | `TimeCurrent()` 服务器时间 | ISO8601 UTC | 全部 UTC；本地时钟与 `EXN-TIMESTAMP` 窗口需 NTP 校准 |
| Tick 频率 | 逐 Tick | 实测约 2 笔/秒，间隔 223–809ms（500ms 节流生效） | 策略判定不依赖 Tick 密度；1s 心跳兜底 |
| 交易时段 | `SymbolInfoSessionTrade` 按日 | `trade_sessions` 按周，实测 XAUUSD 22 段（含每日 21:58–23:00 UTC 维护） | 展开为按日段（§7.3） |
| 状态存储 | GlobalVariable | 无 | SQLite |
| 音效 | `PlaySound` | 无 | Web 端可选提示音 |
| 推送 | `SendNotification` | 无 | WS 站内告警 + 可插拔通道 |
| 回测 | 策略测试器 | 无 | `gl-replay` 回放工具 |
