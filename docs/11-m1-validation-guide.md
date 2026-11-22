# M1 · API 可行性验证手册

> 目标：用 **模拟账户** 打通签名 → 接入点 → 配置 → 快照 → WS → 开/平仓 → 对冲验证，并把实测事实回填到 `04-exness-api-mapping.md`。
> 代码：`crates/exness-api`（SDK）+ 两个 example（`m1_probe` 只读、`m1_trade_probe` 会下单）。

---

## 1. 准备

1. 在 Exness Personal Area → **Key Management** 为模拟账户创建一把 **Trading Full access** Key：
   - 记录 `exnsk-…` Key ID 与 128 hex 私钥（只显示一次）；
   - 从同页 **DNS 组件**复制 Gateway 主机（形如 `https://…`），不要从任何示例复制；
   - 允许 IP 先留空（验证机 IP 不固定），到期日按需。
2. 本机准备环境变量（PowerShell 示例，**不要写进仓库**）：
   ```powershell
   $env:EXNESS_GATEWAY_URL = "https://<key-management 显示的 gateway 主机>"
   $env:EXNESS_ACCOUNT_ID  = "<模拟账号>"
   $env:EXNESS_API_KEY     = "exnsk-..."
   $env:EXNESS_PRIVATE_KEY = "<128 hex>"      # 或 EXNESS_PRIVATE_KEY_FILE=<pem 路径>
   $env:RUST_LOG           = "exness_api=debug"
   ```
3. 确认本机时钟已 NTP 同步（签名时间戳需贴近服务器时间）。

---

## 2. 只读探针（不下单）

```powershell
cargo run -p exness-api --example m1_probe
```

依次执行并打印：接入点发现 → 账户详情（币种 / trade_mode / account_status / execution_mode）→ 品种列表与黄金符号自动匹配（`XAUUSD` → `XAUUSDm` → `XAUUSDc`）→ 黄金条件（`exec_mode`、`point_digits`、`contract_size`、手数规格、交易时段）→ `limits` 全量 → 快照 → 最近 5 根 M1 → ticks WS 收 10 笔（打印间隔与延迟）→ events WS 收 `trading_state_snapshot` 与 `account_state_event`。

需记录到 04 文档的字段：

| 项 | 来源 | 回填位置 |
| --- | --- | --- |
| 接入点主机形态 | 第 1 节输出 | 04 §2.2 |
| `exec_mode` / `point_digits` / `contract_size` / `volume_*` | 第 4 节 | 04 §4、02 §1.3（运行时常量推导） |
| `trade_sessions` 原文 | 第 4 节 | 02 §5（时段模型展开） |
| REST 全局与各 operationId 限额、`max_candles_per_request` | 第 5 节 | 04 §8 |
| WS `max_active_connections`、`connection_rate`、`subscription_operation_rate` | 第 5 节 | 04 §8、03 §3.4 退避参数 |
| Tick 实际间隔（是否 500ms 节流）与 `age` | 第 8 节 | 03 §3.3、04 §11 |
| 时钟偏差 | 第 2 节 `clock skew` | 03 §7.1 阈值复核 |

---

## 3. 交易探针（会在模拟账户下单）

```powershell
$env:EXNESS_ALLOW_TRADING = "1"
cargo run -p exness-api --example m1_trade_probe -- --i-know-this-trades
```

流程：订阅 `transactions` 等到快照 → 取一笔 Tick → **开 BUY 最小手数** → 等 `transaction_event`（按 `client_request_id` 匹配，3s 无事件改为轮询 `operations/{id}`）→ **在 BUY 持有期间开 SELL** → 拉快照确认多空并存（`HEDGING = true/false`）→ 用同一幂等键重放 SELL 请求（期望返回同一 `operation_id`、不新开仓）→ 分别平掉两笔 → 快照确认无剩余。

`exec_mode = instant` 时自动附带 `price`（当前 ask/bid）与 `deviation`（开仓 200 点、平仓 300 点）；`market` 时不附带。

需记录：

| 项 | 回填位置 |
| --- | --- |
| 对冲并存结果 | 01 OQ-03、04 §11、09 风险表 |
| `transaction_event` 实际字段（`client_request_id` 是否回显、`payload.positions[].state`、deal 的 `profit`） | 04 §5、§7 |
| ACK → 事件的典型时延 | 04 §5（轮询兜底阈值 3s 是否合理） |
| 幂等重放行为（同 `operation_id`？HTTP 状态？） | 04 §5 |
| 拒单样例（如有）`error_code/error_message` | 04 §9 |

---

## 4. 验收

- [x] 只读探针全部 9 节无错误完成（2026-11-19，模拟账户 18000069657）。
- [x] 交易探针：两笔开仓 `success`、对冲并存为 `true`、幂等重放未新增持仓、两笔平仓 `success`、末尾快照无剩余黄金持仓。
- [x] 以上表格字段已回填 02 / 03 / 04 文档，并在 09 §1 将 M1 标记完成。

实测结论汇总见 `12-m1-report.md`。

## 5. 已知待确认点

- 订阅成功后服务器是否回一条带 `id` 的确认消息（SDK 已按 `WsInbound::Ack` 兼容，有则记录格式）。
- `access-point` 响应中 `account_id` 的类型（SDK 同时接受字符串与数字）。
- 429 时 `Retry-After` 头是否存在（SDK 已解析秒数与 HTTP-date 两种形式）。
