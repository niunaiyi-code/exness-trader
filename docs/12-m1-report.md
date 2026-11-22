# 12 · M1 验证报告（Exness Trader API 可行性）

> 日期：2026-11-19 · 环境：Windows 11 / rustc 1.100.0 / `exness-api` 0.1.0
> 账户：`18000069657`（Standard，USD，余额 10000，leverage 2000，创建于 2026-11-19，模拟账户）
> Gateway：`https://api.exness.com` → 接入点 `https://ap-41f12819.exness.com`（发现耗时约 1.3s）

---

## 1. 结论

M1 全部通过。签名、接入点发现、配置读取、快照、蜡烛、两路 WS、异步开平仓、**同品种多空并存**、幂等重放均按设计工作；OQ-02、OQ-03 关闭。SDK 无需架构级改动，仅补充了 WS 子协议与若干实测语义。

---

## 2. 只读探针结果（`m1_probe`）

| 项 | 实测 |
| --- | --- |
| 接入点 | `https://ap-41f12819.exness.com`；响应 `account_id` 为字符串 |
| 账户 | Standard / USD / leverage 2000 / margin_call 60 / stopout 0 / `trade_mode=enabled` / `account_status=active` / `execution_mode=market` |
| 黄金符号 | 337 个品种中自动匹配到 `XAUUSD` |
| 品种条件 | `exec_mode=market`、`point_digits=3`（point 0.001，pip 0.01）、`contract_size=100`、`volume_min/step/max=0.01/0.01/200`、`calc_mode=forex`、`margin_percent=1`、无佣金、`swap_long=-598.3`、`swap_short=0`（Swap-Free，按 AGENTS.md 不计） |
| 交易时段 | 22 段/周：周日 23:01:30 开市；周一至四 00:00–21:57:59 与 23:00:30 之后可交易，**21:58:00–23:00:29 每日维护停牌**；周五 21:58 起停市；周六全天停市 |
| 快照 | 余额/净值 10000，0 持仓 0 挂单；`leverage` 字段存在 |
| 蜡烛 | 最近 5 根 M1 正常，价格 ~3675–3689 |
| 时钟偏差 | −688ms / −488ms（`Date` 头），低于 1.5s 告警阈值 |
| Tick | 12 笔，间隔 223–809ms，平均 ~2 笔/秒；`age` −140ms ~ +170ms（服务器时钟略快） |
| events WS | 订阅后立即收到 `trading_state_snapshot`；`account_state_event` 严格 2s 周期 |

## 3. 限流实测（`GET /limits`）

| 维度 | 数值 |
| --- | --- |
| REST 全局 | 15 次/秒 |
| `getCandleHistory` | 2/s，单次 ≤1000 根 |
| `openPosition` / `closePosition` | 4/s / 4/s |
| `getTradingStateSnapshot` | 2/s |
| `getOperationStatus` | 10/s |
| `getAccountDetails` / `getInstrumentCondition` | 4/s |
| `getAvailableInstrumentList` / `getLimits` | 2/s |
| WS 连接 | `max_active_connections=5`，`connection_rate=10/1s`（burst 20） |
| WS 入站 | `inbound_message_rate=50/1s`，`strikes.threshold=10 / 600s` |

## 4. 交易探针结果（`m1_trade_probe`）

| 步骤 | 结果 |
| --- | --- |
| 开 BUY 0.01 | ACK `operation_id=381857381572673536` → `transaction_event success`，`position_id=5364020`，成交价 3689.392 |
| 开 SELL 0.01（BUY 持有中） | ACK `operation_id=381857383418167296` → `success`，`position_id=5364021`，成交价 3689.347 |
| **对冲并存** | 快照同时含 Buy 与 Sell 两笔 0.01 → **HEDGING = true** |
| 幂等重放 | 同一幂等键 + 同一载荷 → 返回**同一** `operation_id`，HTTP 202，无新持仓 |
| 平 BUY | `success`，deal 盈利 +0.19 USD |
| 平 SELL | `success`，deal 盈利 −0.58 USD |
| 收尾快照 | 剩余黄金持仓 0 |

ACK → `transaction_event` 时延实测 0.4–0.6s。

## 5. 实测发现与 SDK 调整

1. **WS 子协议**：服务器在 `101` 响应中返回 `Sec-WebSocket-Protocol: exness-ws-protocol`，客户端未声明该子协议时握手被拒。已加入 `ClientConfig::ws_subprotocol`（默认携带）。
2. **订阅确认格式**：`{"id":"…","code":200}`；ticks 流 `code` 为数字、events 流为字符串 `"200"`。SDK 以 `WsInbound::Ack` 原样透传。
3. **事件字段**：`client_request_id` 与 `operation_id` 均回显；平仓事件中 position 为 `closed`、`volume=0`；开仓 deal 的 `profit.currency` 为空串，平仓 deal 带 `USD`。
4. **瞬时错误**：出现一次 `tls handshake eof`（Cloudflare），重试即恢复，归类 Transient。
5. **Tick 时间戳偶尔超前**：负 `age` 属正常，不许按「过期报价」处理。
6. **`exec_mode=market`**：`price` / `deviation` 不参与成交，策略层按「重试替代滑点档」执行（02 §12）。

## 6. 凭证与安全备注

- 本次使用的 API Key 与私钥仅通过进程环境变量传入，未写入仓库与文档。
- 仓库中仍保留 2026-11-19 之前从旧 `exness/` 目录读取到的凭证痕迹风险提示：**不得**把任何真实凭证写入 `exness-trader/` 下的文件或提交；如需复现，按 11 §1 重新录入环境变量。
