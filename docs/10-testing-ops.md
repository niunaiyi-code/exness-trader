# 10 · 测试与运维

---

## 1. 测试分层

| 层 | 对象 | 工具 | 目标 |
| --- | --- | --- | --- |
| L1 单元 | `gl-strategy` 各模块、`exness-api::auth` | `cargo test`、`proptest` | 公式与状态机等价；签名与规范示例一致 |
| L2 黄金向量 | 数学与目标利润 | 从 MT5 日志 / 手算表导出 JSON | 与 MQL5 输出误差 < 1e-6 |
| L3 回放 | `gl-replay` 驱动内核 | MT5 导出 Tick CSV、Exness 历史 M1 | 事件序列与 MT5 可视化回测对齐 |
| L4 集成 | `gl-runtime` 对接 Exness **模拟账户** | 真实 API | 台账 / 操作 / 对账正确；断线恢复 |
| L5 端到端 | `gl-server` + Web | Playwright | 登录 → 新增账户 → 启动 → 手动动作 → 回执可视 |
| L6 灰度 | 模拟盘 → 小资金实盘 | 监控 + 人工核对 | 7 交易日无不一致 |

---

## 2. 黄金向量采集方法

1. 在 MT5 研发版 EA 上用若干参数组合启动（可在策略测试器中仅 `OnInit`），从日志抓取：
   - `[LOT SEQUENCE]` → `calc_next_lot` 序列
   - `[AUTO-GRID NOTICE]` → `max_orders, theo_loss, rem_buffer, cushion_pts, max_defensive_usd`
2. 对 `CalculateRequiredProfit`，在 EA 中临时打印 n=1..max 的 `req_profit`（研发版，不入库）。
3. 存为 `crates/gl-strategy/tests/vectors/*.json`：
```json
{"params":{"initial_lot":"0.01","grid_step":300,"grid_step_add":75,"lot_mult":"1.45","lock_trigger_usd":"1000","tp_distance_usd":"1.00"},
 "runtime":{"pip":"0.01","ppd":"100","lots_min":"0.01","lots_max":"100","lots_step":"0.01"},
 "expect":{"max_orders":9,"lots":["0.01","0.01","0.02","0.03","0.04","0.06","0.09","0.13","0.19"],"theo_loss":"…","req_profit":["1.00","…"]}}
```
同时覆盖 USC 账户（`ppd=10000`）。

---

## 3. 回放对齐流程

1. MT5：对同一时间段做「每笔 Tick 基于真实 Tick」可视化回测，导出日志中的事件：`NEW CYCLE`、`GRID BUY/SELL`、`BASKET TP CLOSE`、`TRAILING ACTIVATE/TP CLOSE`、`LOCK ROUND START/SUCCESS`。
2. 导出同段 Tick（MT5 `CopyTicks` 脚本 → CSV：`time_msc,bid,ask`）。
3. `gl-replay --ticks ticks.csv --params p.json --sessions sessions.json --calendar cal.csv` 输出事件 JSONL。
4. 对齐脚本按 `(kind, side, layer)` 配对，允许时间差 ≤ 1 Tick、价格差 ≤ 滑点；输出差异报告。
5. 已知可接受差异：Exness Tick 节流（回放用 MT5 Tick 时无此差异）、成交价（回放中假定成交于请求价）。

---

## 4. 集成测试要点（模拟账户）

| 场景 | 验证 |
| --- | --- |
| 冷启动 | 快照为空 → 首单 → 台账 1 条 → 事件 `confirmed` |
| 重启恢复 | 杀进程 → 启动 → 对账：台账与快照一致，无 foreign 告警 |
| 外部持仓 | 在 Exness 终端手动开 0.01 → 系统标记 foreign、不参与统计 → 收编后参与 |
| 断线 | 阻断 WS 60s → Degraded、无开仓 → 恢复、重订阅、对账 |
| 拒单 | 用超过 `volume_max` 的手数触发 rejected → 篮子 `pending_op` 清除、事件记录 |
| 锁仓 | 临时把 `lock_trigger_usd` 调极小 → ARMING → LOCKED → 2h（测试时缩短常量，feature flag）自动全平 → 30s 复位 |
| 限流 | 并发请求触发 429 → 令牌桶收紧、无崩溃 |

测试用常量缩短（2h→2min、30s→5s）通过 `cfg(feature = "fast-timers")` 编译开关实现，生产构建禁止启用。

---

## 5. 部署

### 5.1 systemd（推荐）
```ini
[Unit]
Description=GoldLegend Exness Trader
After=network-online.target

[Service]
User=gl
WorkingDirectory=/opt/gl
ExecStart=/opt/gl/gl-server --config /etc/gl/config.toml
Restart=always
RestartSec=3
LimitNOFILE=65536
KillSignal=SIGTERM
TimeoutStopSec=45

[Install]
WantedBy=multi-user.target
```
- 公网经 Caddy 反向代理（自动 TLS），后端只监听 `127.0.0.1`。
- 服务器 NTP 必须开启（`timedatectl set-ntp true`）。

### 5.2 Docker
- `gl-server` 多阶段构建（rust:1.8x-slim → debian:bookworm-slim），镜像内含 `web/dist`。
- `docker compose`：`gl-server` + `caddy`；卷：`./data:/opt/gl/data`。

### 5.3 升级
1. `systemctl stop gl`（优雅停机等待在途操作）。
2. 备份 `data/gl.db`。
3. 替换二进制，启动；sqlx 迁移自动执行。
4. 观察 `/system/health` 所有 Runner `ready`，核对无 foreign 告警。

---

## 6. 监控与告警

| 指标 | 阈值 / 动作 |
| --- | --- |
| Runner 状态 | 非 `ready` 持续 > 60s → 告警 |
| 报价年龄 | > 5s → Degraded；> 60s → 告警 |
| Tick 队列深度 / 处理时延 | 深度 > 100 或时延 > 500ms → 告警 |
| 操作超时数 | 任一账户 5 分钟内 > 3 → 告警 |
| foreign 持仓出现 | 立即告警 |
| 平仓连续失败 | ≥ 5 次 → critical |
| 429 次数 | 每小时 > 10 → 告警 |
| 时钟偏差 | > 1.5s → 告警 |
| 进程重启 | 每次 → 通知 |

输出：`/api/v1/system/health`（JSON）、可选 `/metrics`（Prometheus）、站内 WS 系统告警。

---

## 7. 日志与数据保留

- 日志：`tracing` JSON 到 stdout → journald / 文件滚动；保留 30 天。
- `events` 表：按账户保留 30 天（可配置）；`operations` 90 天；`audit_log` 永久。
- 备份：每日 `sqlite3 .backup` 到异地；恢复演练每季度一次。

---

## 8. 安全运维

- `config.toml` 包含 JWT 密钥、主加密密钥和可选财经日历 API 密钥；文件权限应限制为仅服务账号可读（例如 `0600`）。
- Exness API Key：每账户一把 Trading Full access Key；在 Key Management 设置允许 IP 为服务器固定出口 IP，设置到期日并在系统内登记；到期前 7 天告警、到期前完成轮换（新 Key 录入 → 验证 → 切换 → 吊销旧 Key）。
- 服务器更换出口 IP 前必须先在所有账户的 Key 上更新允许 IP。
- 管理员账户启用 TOTP；默认密码强制首次修改。
- 定期 `cargo audit` / `npm audit`。

---

## 9. 故障手册（摘要）

| 现象 | 排查 |
| --- | --- |
| 全部账户 `AUTH_INVALID_SIGNATURE` | 时钟偏差 → NTP；私钥损坏 → 重新录入 |
| 单账户 `Failed` | 查 events：账户禁用 / 品种不可交易 / 对冲不支持 |
| 看板不动 | WS Hub 连接数、Runner 报价年龄、Exness ticks 节流 |
| 台账与终端不一致 | 触发手动对账 `POST /accounts/{id}/reconcile`（admin）；查 operations 超时记录 |
| 锁仓后未自动全平 | 点差 > 0.80 的 `AUTO CLOSE HOLD` 日志；等待或人工处置 |
