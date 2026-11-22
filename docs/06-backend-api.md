# 06 · 后端对外 API 规范（Web / Android 共用）

Base path：`/api/v1`。鉴权：`Authorization: Bearer <access_token>`。时间：ISO8601 UTC。金额 / 手数 / 价格：十进制字符串。

统一错误体：
```json
{"error":{"code":"ACCOUNT_NOT_FOUND","message":"…","details":{}}}
```
HTTP 状态：400 参数、401 未登录、403 无权限、404 不存在、409 状态冲突（如 Runner 未运行）、422 校验失败、500 内部。

---

## 1. 认证

| 方法 | 路径 | 说明 |
| --- | --- | --- |
| POST | `/auth/login` | `{username,password[,totp]}` → `{access_token, refresh_token, expires_in, user}` |
| POST | `/auth/refresh` | `{refresh_token}` → 新令牌对 |
| POST | `/auth/logout` | 吊销刷新令牌 |
| GET | `/auth/me` | 当前用户、角色、授权账户列表 |

已决策：手动动作不做二次密码验证，仅校验 JWT 与 `operate` 权限；前端用 `Modal.confirm` 做误触保护。

---

## 2. 账户

| 方法 | 路径 | 权限 | 说明 |
| --- | --- | --- | --- |
| GET | `/accounts` | view | 列表（含实时摘要：status, balance, equity, floating_pnl, buy/sell count, flags） |
| POST | `/accounts` | admin | 新增 `{exness_account_id, label, api_key, private_key_hex, params?}`；同步执行预检（含黄金符号自动匹配），返回账户与预检报告 |
| GET | `/accounts/{id}` | view | 账户详情（不含凭证） |
| PATCH | `/accounts/{id}` | admin | `label, enabled` |
| PUT | `/accounts/{id}/credentials` | admin | 替换凭证（要求已停止） |
| DELETE | `/accounts/{id}` | admin | 删除（要求已停止且无 open 台账） |
| POST | `/accounts/{id}/start` | admin | 启动 Runner |
| POST | `/accounts/{id}/stop` | admin | 停止 Runner |
| GET | `/accounts/{id}/preflight` | admin | 重新预检（不改状态） |
| POST | `/accounts/{id}/reconcile` | admin | 立即触发一次快照 ∩ 台账对账，返回差异报告 |
| GET | `/accounts/{id}/users` / PUT | admin | 授权用户与权限 |

预检报告：
```json
{"ok":true,"checks":[{"name":"signature","ok":true},{"name":"currency","ok":true,"value":"USC"},
 {"name":"instrument","ok":true,"value":"XAUUSDc"},{"name":"exec_mode","ok":true,"value":"market"},
 {"name":"trade_mode","ok":true},{"name":"derived","ok":true,"value":{"pip":"0.01","profit_per_dollar_per_lot":"10000","volume_min":"0.01"}}]}
```

---

## 3. 策略参数

| 方法 | 路径 | 权限 | 说明 |
| --- | --- | --- | --- |
| GET | `/accounts/{id}/params` | view | 当前参数 + 推导值 `{max_grid_orders, max_defensive_usd, lot_sequence[], theo_loss, rem_buffer, cushion_pts}` |
| PUT | `/accounts/{id}/params` | operate | 全量更新；校验失败 422 返回逐字段错误；Runner 运行中热更新（仅影响后续决策） |
| POST | `/accounts/{id}/params/preview` | view | 不保存，只返回推导值（参数编辑页实时预览） |
| POST | `/accounts/{id}/params/lock` | admin | `{locked: bool}` 禁止 trader 修改 |

---

## 4. 实时状态与持仓

| 方法 | 路径 | 说明 |
| --- | --- | --- |
| GET | `/accounts/{id}/state` | 最近一帧 `DashboardFrame`（§5 结构） |
| GET | `/accounts/{id}/positions` | 台账 open 持仓 + 外部持仓 `[{position_id, role, side, layer, volume, open_price, open_time, unrealized, swap, commission}]` |
| POST | `/accounts/{id}/positions/{pid}/adopt` | admin：收编外部持仓 `{role:"grid"|"lock", side, layer?}` |
| GET | `/accounts/{id}/candles?tf=M1&from&to` | 本地缓存的 M1 蜡烛（供图表） |

---

## 5. DashboardFrame（WS 与 REST 共用）

```json
{
 "account_id": 1, "ts": "2026-11-19T10:00:00Z",
 "runner": {"status":"ready","ws_ticks":"up","ws_events":"up","quote_age_ms":120},
 "quote": {"bid":"4123.45","ask":"4123.65","spread":"0.20","digits":3},
 "account": {"balance":"10000.00","equity":"9876.50","used_margin":"120.00","floating_pnl":"-123.50","net_lots":"0.03","currency":"USD"},
 "flags": {"locked":false,"lock_state":"idle","locked_since":null,"paused":false,
           "news_avoid":false,"news_event":null,"news_feed_ready":true,"session_avoid_desc":""},
 "limits": {"max_grid_orders":9,"max_defensive_usd":"58.3"},
 "buy":  {"count":3,"lots":"0.05","avg":"4130.12","profit":"-45.20","req_profit":"3.12","cushion_points":"62.4",
          "trailing_active":false,"peak_exit_price":null,"floor_price":null,"closing":false,"pending_op":null,"next_step_points":450},
 "sell": {"count":0,"lots":"0","avg":null,"profit":"0","req_profit":null,"cushion_points":null,
          "trailing_active":false,"peak_exit_price":null,"floor_price":null,"closing":false,"pending_op":null,"next_step_points":null}
}
```

---

## 6. 手动动作

`POST /accounts/{id}/actions`，权限 `operate`，无需额外密码验证（已决策）；每次调用写 `audit_log`。

```json
{"type":"lock_now" | "close_all" | "close_loss" | "close_profit" | "close_buy" | "close_sell" | "pause" | "resume",
 "confirm": true}
```
响应：`{"accepted":true,"action_id":"…"}`；执行进度通过 WS `action_progress` 事件推送（提交数 / 已确认 / 失败）。Runner 非 `ready/degraded` 返回 409。

---

## 7. 事件、历史、审计

| 方法 | 路径 | 说明 |
| --- | --- | --- |
| GET | `/accounts/{id}/events?cursor&limit&level&kind` | 策略事件流（决策、门禁、告警、操作结果） |
| GET | `/accounts/{id}/operations?cursor&status` | 操作记录 |
| GET | `/accounts/{id}/history/deals?from&to&cursor` | 透传 Exness deals（带缓存） |
| GET | `/accounts/{id}/history/orders?from&to&cursor` | 透传 Exness orders |
| GET | `/alerts?account_id&since` | 告警列表；`POST /alerts/{id}/ack` |
| GET | `/audit?user_id&account_id&from&to` | admin |

---

## 8. 系统

| 方法 | 路径 | 说明 |
| --- | --- | --- |
| GET | `/system/health` | 进程、DB、日历、各 Runner 状态汇总 |
| GET | `/system/calendar` | 已加载事件列表、最后刷新时间、下次刷新 |
| POST | `/system/calendar/refresh` | admin 强制刷新 |
| GET | `/system/version` | 版本、构建信息 |
| GET/PUT | `/system/notify` | admin：通知通道配置（敏感字段写入后不回显） |
| GET/POST/PATCH/DELETE | `/users` | admin：用户管理 |

---

## 9. WebSocket / WSS

### 当前实现状态

- 服务端入口为根路径 `/ws`；开发环境由 Vite 代理到 `ws://localhost:8080/ws`。生产环境页面使用 HTTPS 时，浏览器将连接升级为 WSS；TLS 由部署入口（如反向代理）终止。
- 当前帧包括 `tick`、`runner_status` 和 `pong`。`event`、`alert` 虽有帧类型定义，但服务端尚无实际广播调用，WEB 也尚未处理这两类帧。
- WEB 端当前消费行情 tick 与运行状态；账户持仓仍通过 `GET /accounts/{id}/snapshot` 每 5 秒轮询，平仓历史通过 REST 轮询。后台 Exness `transactions` 事件送入策略运行器更新内部台账，但尚未转发到前端 WSS Hub。
- `/ws` 当前没有 JWT 身份校验、账户订阅或按用户权限过滤。不可通过该入口广播账户私有的成交、持仓或告警数据；接入前必须先完成认证与账户范围授权。

### 目标协议（尚未实现）

客户端 → 服务端：
```json
{"op":"subscribe","accounts":[1,2],"channels":["frame","tick","event","alert"]}
{"op":"unsubscribe","accounts":[2]}
{"op":"ping"}
```
服务端 → 客户端（`type` 区分）：
| type | 频率 | 内容 |
| --- | --- | --- |
| `frame` | 状态变化或 ≥ 1s | `DashboardFrame`（服务端对每账户 250ms 节流合并，等价 EA `DASHBOARD_RENDER_MSC`） |
| `tick` | 每 Tick | `{account_id, bid, ask, ts}`（图表用；可不订阅） |
| `event` | 实时 | `{account_id, id, ts, level, kind, message, data}` |
| `alert` | 实时 | `{account_id, id, ts, severity, tag, message}`（千元阶梯、锁仓、系统告警） |
| `action_progress` | 实时 | `{account_id, action_id, submitted, confirmed, failed, done}` |
| `runner_status` | 变化时 | `{account_id, status}` |
| `pong` | — | — |

目标协议要求无权限账户的订阅被拒绝并返回 `{"type":"error","code":"FORBIDDEN","account_id":…}`。令牌过期时关闭连接，客户端刷新令牌后重连。实现成交推送时，以 Exness 成交确认事件驱动账户级 `event`，WEB 与 Android 收到后更新界面和音效；重连后仍通过账户快照对账，因为 WSS 不是可回放的持久事件日志。

---

## 10. 版本与兼容

- 路径带 `v1`；破坏性变更升 `v2` 并行。
- 服务端用 `utoipa` 生成 OpenAPI 文档，`/api/docs` 提供 Swagger UI（仅开发 / admin）。
- Android 端与 Web 端共用此规范，DTO 由 OpenAPI 生成。
