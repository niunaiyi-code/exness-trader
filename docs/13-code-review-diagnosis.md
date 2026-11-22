# Exness Trader 系统代码审查与缺陷诊断报告（同行评审版）

> **文档编号**：`docs/13-code-review-diagnosis.md`  
> **编写日期**：2026-11-22  
> **适用对象**：AI 架构师 / 核心开发人员同行评审（Peer Review）  
> **审查范围**：全量 5 个 Rust Crate (`exness-api`, `gl-runtime`, `gl-server`, `gl-strategy`, `exness-trader`) + Web 前端  
> **当前基线**：全套单测通过、`gl-runtime` 8/8 E2E 仿真测试通过、前端 Vite 构建 0 TS 错误；但实盘交易与服务层存在待处理逻辑与时序缺陷。

---

## 一、审查概览与执行状态

在 Claude 完成大规模重构与代码优化后，系统的数学计算（`gl-strategy`）与底层协议解析（`exness-api`）基础扎实，所有涉及资金计算的部分全链路使用 `rust_decimal`。然而，在交易状态流转、并发去重及跨账户隔离方面，仍排查出以下关键缺陷。

---

## 二、缺陷明细（按严重等级排序）

### 🔴 维度一：资金安全与交易执行漏洞（实盘核心阻塞项）

#### 1. 【严重】WS 开仓确认未同步更新 `current_positions`（15 秒盲区重复开单）
- **代码位置**：[`crates/gl-runtime/src/account_runner.rs:361-385`](file:///d:/Projects/exness-trader/crates/gl-runtime/src/account_runner.rs#L361-L385)
- **根因分析**：
  当券商通过 WebSocket 推送成交确认 `TransactionEvent`（开仓成功）时，代码执行了 `self.ledger.insert(...)` 并将 `ops_tracker` 中的 pending 操作标记为完成（`mark_completed`），但**遗漏了将新成交订单写入本地持仓缓存 `self.current_positions`**（平仓分支有调用 `current_positions.retain` 移除，开仓分支缺失 push）。
- **连锁风险**：
  后续每个行情 tick 到达时，策略通过 `BasketStats::calculate_side` 计算当前持仓层数，该函数是以 `self.current_positions` 与 `ledger` 取交集进行统计。由于新持仓尚未进入 `current_positions`（直到 15 秒后的 REST 快照轮询才拉取到），系统计算出当前持仓数仍为 `count = 0`；同时因为 `ops_tracker` 已完成，`has_pending_side` 为 `false`，导致系统在随后的数个 tick 内**连续重复下发首单开仓请求**。
- **推荐修复方案**：
  ```rust
  // account_runner.rs on_ws_event 开仓分支中补全：
  for pos in &tx.payload.positions {
      self.ledger.insert(LedgerEntry { ... });
      self.current_positions.retain(|p| p.id != pos.id);
      self.current_positions.push(pos.clone());
  }
  ```

---

#### 2. 【高危】止盈平仓缺乏在途去重（高频重复发送平仓指令）
- **代码位置**：[`crates/gl-runtime/src/account_runner.rs:431-441`](file:///d:/Projects/exness-trader/crates/gl-runtime/src/account_runner.rs#L431-L441)
- **根因分析**：
  当达到止盈条件触发 `close_grid_basket_side` 时，代码遍历当前持仓逐笔调用 `self.executor.close_position(...)`。每次调用生成的幂等键都带有新的时间戳与随机串。在券商返回确认前的网络延迟窗口期内（约数百毫秒至数秒），由于行情高频 tick 不断流入且仍满足止盈条件，系统会**在每个 tick 对相同仓位重复下发平仓指令**。
- **连锁风险**：
  引发向券商的请求风暴，可能触发 API 429 限流封禁，并在对账时因重复平仓产生大量未知拒绝日志。
- **推荐修复方案**：
  在下发平仓前检查 `ops_tracker` 是否已存在该 `position_id` 的在途平仓操作，存在则跳过。

---

#### 3. 【中危】定时快照对账与 WS 事件时序竞态
- **代码位置**：[`crates/gl-runtime/src/account_runner.rs:461-506`](file:///d:/Projects/exness-trader/crates/gl-runtime/src/account_runner.rs#L461-L506)
- **根因分析**：
  运行器每 15 秒通过 REST `snapshot()` 对账。若在 REST 请求发出至响应返回的数百毫秒内，刚好有新开仓完成并先通过 WS 入账，而 REST 快照中尚未包含该单，定时对账逻辑会将“快照中没有的持仓”视作外部已平仓并直接从 `ledger` 中剔除。
- **推荐修复方案**：
  在快照对账剔除仓位时增加“创建时间宽限期”（例如刚入账不足 10 秒的订单不参与缺席剔除）。

---

### 🟡 维度二：数据隔离与接口安全

#### 4. 【高危】WebSocket 广播使用 `starts_with` 前缀匹配（跨账户数据泄漏）
- **代码位置**：
  - 后端：[`crates/gl-server/src/ws.rs:165-201`](file:///d:/Projects/exness-trader/crates/gl-server/src/ws.rs#L165-L201)
  - 前端：[`web/src/pages/TraderCockpitPage.tsx:434-438`](file:///d:/Projects/exness-trader/web/src/pages/TraderCockpitPage.tsx#L434-L438)
- **根因分析**：
  WebSocket 推送过滤使用了：
  `target == account_id || target.starts_with(account_id) || account_id.starts_with(target)`
- **连锁风险**：
  若系统存在纯数字账号（例如 `1800001` 与 `18000012`），两者的成交明细、报警、持仓事件会相互串户，非属主用户可监听到其他账户的实盘动态。
- **推荐修复方案**：
  严格使用精确匹配 `target == account_id`。

#### 5. 【中危】WebSocket Token 通过 URL Query 传递
- **代码位置**：
  - 后端：[`crates/gl-server/src/ws.rs:17-20, 83`](file:///d:/Projects/exness-trader/crates/gl-server/src/ws.rs#L17-L20)
  - 前端：[`web/src/pages/TraderCockpitPage.tsx:447`](file:///d:/Projects/exness-trader/web/src/pages/TraderCockpitPage.tsx#L447)
- **根因分析**：
  JWT token 作为 query 参数附加在 WebSocket 连接 URL 中。Trace 日志和反向代理访问日志通常记录完整 URL，可能导致 JWT 泄漏。
- **推荐修复方案**：
  改用 WebSocket 子协议（Sec-WebSocket-Protocol）或首包 Handshake 帧传递 Token。

---

### ⚪ 维度三：集成测试失败项排查

当前 `crates/gl-server/tests/integration_tests.rs` 存在 2 处断言失败：

1. **`test_system_calendar_and_refresh` 返回 401（期望 200）**
   - **位置**：[`integration_tests.rs:226`](file:///d:/Projects/exness-trader/crates/gl-server/tests/integration_tests.rs#L226)
   - **根因**：后端重构后将 `/system/calendar/refresh` 纳入了 JWT 认证路由层，但该集成测试发起 POST 请求时未携带 `Authorization` 头。
2. **`test_account_start_and_stop_lifecycle` 返回 502（期望 200）**
   - **位置**：[`integration_tests.rs:258-262`](file:///d:/Projects/exness-trader/crates/gl-server/tests/integration_tests.rs#L258-L262)
   - **根因**：`start_account` 端点在启动前增加了对真实券商网关的前置握手（拉取品种规格与账户详情）。测试环境连接外部 `https://api.exness.com` 且为虚构密钥，网关失败直接返回 502 Bad Gateway。

---

### 📌 维度四：项目既定规范与暂缓项（见 AGENTS.md）

在进行评审与重构时，以下两项属于**用户明确规定的既定设计或暂缓项，无需提出作为缺陷修改**：
1. **USC 美分账户常量换算**（`AGENTS.md` 既定设计）：关于点差及金额常量的换算逻辑按现有设计保留，**严禁自行修改或改动该换算逻辑**。
2. **锁仓状态（LockState）持久化**（`AGENTS.md` 待处理项）：系统在锁仓状态下遭遇异常重启属于极端场景，当前阶段暂不考虑与处理该极端情况，保持纯内存状态机即可，优先保障实盘交易与正常风控的主链路稳定。
