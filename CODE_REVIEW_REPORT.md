# exness-trader 代码审查报告

> 审查日期：2026-11-22 · 审查范围：全部 5 个 Rust crate + web 前端 + 配置/部署文件（约 11,600 行）
> 结论：**当前不具备实盘上线条件**。金额计算全链路使用 rust_decimal、SQL 全参数化、签名 path 一致性校验等基础质量较高，但存在密钥治理、锁仓安全链路、鉴权缺失三类资金级风险，需优先修复。

---

## 🔴 严重（上线前必须修复）

### A. 密钥治理
| # | 问题 | 位置 | 修复 |
|---|------|------|------|
| A1 | 真实 Exness API Key + Ed25519 私钥、日历 API key、master_key 明文落盘 config.toml | `config.toml:19-31` | **立即轮换全部密钥**；改用环境变量/密钥管理器注入 |
| A2 | `COPY . .` 且无 `.dockerignore`，真实密钥、gl.db、备份库全部打入 Docker 构建层 | `Dockerfile:4` | 添加 .dockerignore，只 COPY 构建必需文件 |
| A3 | 默认管理员硬编码弱口令 `admin/admin123` 且口令写入日志 | `gl-server/src/main.rs:47,55` | 首次启动生成随机口令强制修改，禁止打印 |

### B. 资金安全（策略/运行时）
| # | 问题 | 位置 | 影响 |
|---|------|------|------|
| B1 | 锁仓状态不持久化，重启后锁仓腿被误归类为 Grid，止盈会拆散对冲 | `gl-runtime/src/account_runner.rs:90,253-264` | 净敞口重新暴露 |
| B2 | Arming/Locked 冻结期止盈与移动止损照常执行；同 tick 锁仓与止盈并发时锁仓手数 oversized | `pipeline.rs:58-132`、`account_runner.rs:529-621` | 对冲被系统自身拆解、敞口翻转 |
| B3 | 规格要求的"锁仓 2 小时自动全平"完全缺失 | 全 runtime 无实现 | 进入 Locked 后永久冻结，只能人工干预 |
| B4 | WS 开仓确认只写 ledger 不更新 `current_positions`，15 秒盲窗内重复开仓（幂等键是全新 uuid，服务端不去重） | `account_runner.rs:358-383,456-460` | 同方向真实仓位重复开出 |
| B5 | 平仓无在途去重，止盈期间每 tick（约 2 次/秒）对新幂等键重复下发平仓 | `pipeline.rs:81-122`、`account_runner.rs:411-441` | 请求风暴、重复平仓对账错乱 |
| B6 | `expire_stale(15s)` 直接删除未知状态在途操作；迟到确认被静默丢弃，仓位被当 Foreign 采纳后永不管理 | `ops_tracker.rs:145-164`、`account_runner.rs:342,445,515` | 孤儿实盘仓位 |
| B7 | 定时对账把"刚开仓、尚未出现在 REST 快照"的 Grid 仓移出台账（无宽限期） | `account_runner.rs:462-471` | 真实仓位脱离管理、永久裸露 |
| B8 | USC 账户金额常量未按美分换算：点差门禁恒阻塞、深层止盈目标缩小 100 倍 | `gl-runtime/src/config.rs:68,72` | USC 账户无法开仓或止盈过早亏损 |

### C. 服务端鉴权
| # | 问题 | 位置 | 影响 |
|---|------|------|------|
| C1 | 策略端点（list/get/create/update/delete/bind）完全缺失授权检查 | `gl-server/src/api/routes.rs:1801-1985` | 任意 trader 可把他人账户绑定到任意策略并覆写锁定参数（改变实盘下单行为）、删除全局策略、枚举全部账户 |
| C2 | `start_account` TOCTOU 竞态：并发启动产生双 runner，旧任务不 abort 继续交易 | `routes.rs:745-750,992`、`runtime.rs:126-137` | 同一实盘账户两个策略并发下单 |
| C3 | WS 事件过滤用 `starts_with` 前缀匹配（数字账号可互为前缀）；RunnerStatus 对非管理员无过滤 | `gl-server/src/ws.rs:165-201` | 跨账户持仓/成交/告警泄漏、全账户状态探测 |
| C4 | WS token 走 URL query + TraceLayer 记录完整 URI | `ws.rs:17-20,83`、`main.rs:245` | JWT 泄漏到访问日志，24h 内可接管会话 |
| C5 | 注册响应返回 password_hash；`create_account` 无角色校验（viewer 可创建账户并自封 admin）、响应含加密凭据密文 | `routes.rs:222,423-485`、`db/models.rs:6-13` | 越权 + 哈希暴露 |

### D. API 客户端
| # | 问题 | 位置 | 影响 |
|---|------|------|------|
| D1 | WS 无 Pong 超时/读超时/连接超时，TCP 半开时 events 流静默停摆、重连永不触发 | `exness-api/src/ws.rs:119,140-181` | 订单确认事件全部丢失且无感知 |
| D2 | 错误体按字节切片可能落在多字节 UTF-8 字符中间 → 服务器可控输入触发 panic | `rest.rs:488` | 处理 API 错误时打崩 runner |

---

## 🟠 高（建议尽快修复）

- **WS 订阅 ACK 用 `"code":200` 子串匹配**，订阅失败完全不检测、含该子串的真实事件被吞（`exness-api/src/ws.rs:156,322`）
- **时钟偏差修正混入单向延迟 + Date 头仅秒级**，误差可达 ~1.3s，接近 1.5s 告警阈值，签名可能被判过期（`rest.rs:417-424`）
- **SDK 无限流实现**（文档承诺令牌桶，`parse_retry_after` 无消费方）（`rest.rs:362-415`）
- **WS 流客户端 `run()` 永不返回、无停机通道**，只能 task.abort 强杀，占用连接配额（`ws.rs:57-82,206-251`）
- **锁仓执行链不完整**：无 5s 重试节流（失败后每 tick 重试）、lock_volume 不做 lots 归一/分笔、无保证金预检（`account_runner.rs:624-653`、`executor.rs:19-45`）
- **首单冷却门禁从未生效**（`last_close_time: None` TODO + 冷却常量 300s 与规格 2s 不符）（`pipeline.rs:309`、`gate.rs:14`）
- **加仓门禁缺"长休市前 4 小时"检查**（规格 §7.1 强制项）（`pipeline.rs:335-366`）
- **篮子浮盈不含 swap**，负 swap 方向账面盈利高估、止盈提前（`basket.rs:139-153`）
- **移动锁利缺 cushion < 35 点重置分支**（`account_runner.rs:547-609`）
- **定时对账/事件时序问题**：`parse().unwrap_or_default()` 静默吞错变 0 仓位/0 价格（`account_runner.rs:250,365,487`、`basket.rs:57,140`、`reconcile.rs:64`）
- **入口 crate `exness-trader` 只是日志探针**，不接策略不下单；运维若启动此二进制，系统"看起来在跑、实际从不交易"（`exness-trader/src/main.rs:29-34`）
- **Dockerfile root 运行 + `rust:latest` 未固定版本**（`Dockerfile:1,8-19`）
- **JWT 存 localStorage**（XSS 可窃取）（`web/src/lib/api.ts:16,27-30`）
- **前端 WS 账号匹配同样用前后缀判断**，可能串户（`TraderCockpitPage.tsx:377-381`）
- **snapshot 轮询与 WS 事件竞态**，30 秒前的旧响应可覆盖刚推送的新持仓（`TraderCockpitPage.tsx:346-353,282-297`）
- **市价单携带前端缓存的过期 price** 发给后端（`QuickActions.tsx:71-74`）
- **CORS 全开放**（`main.rs:244`）；**登录/注册无速率限制、无口令强度校验**（`routes.rs:81-223`）；**角色固化于 JWT 无吊销**（`auth.rs:21-66`）

---

## 🟡 中 / 低（摘要）

- `/market/candles` 完全公开无限流（匿名可代理打券商 API 消耗真实账户额度）；`/market/snapshot` 无 RBAC 可读资金快照；`/system/calendar/refresh` 无角色限制
- SQLite 未显式配置 WAL/busy_timeout/foreign_keys；`delete_account` 不停 runner 不清缓存 → 幽灵账户继续交易
- AES-GCM 未用 AAD 绑定账户上下文（密文可互换不被察觉）；错误响应泄漏 DB/解密内部细节；快照利润兜底硬编码乘数 100.0
- `parse_week_second` 强制秒位，`HH:MM` 格式会使首单永久禁用
- `gl-strategy` 近半数文件是未在 lib.rs 声明的死代码（两套锁仓状态机、f64 版 GridMath、假时段 risk_manager），测试覆盖的是死代码
- 大量 `format!("DB error: {e}")` 类错误直出；`create_jwt` 的 `u64→i64` 截断 + unwrap
- 前端：XAUUSD 价格多处 `toFixed(2)` 丢第三位小数（项目已有 `formatMt5Price` 未统一使用）；生产构建生成 sourcemap 且可能由后端托管；历史盈亏浮点直加未取整
- 配置：`.gitignore` 忽略 Cargo.lock（应用应提交）；`gl.db.backup-*` 不匹配 `*.db` 规则；`gl-trader.service` 的 WorkingDirectory `/opt/gl-trader` 未被 install.sh 创建；systemd 缺加固项
- LoginPage 源码硬编码真实账号密码（DEV 门控但源码即泄露）；`scripts/test_ui_playwright.py` 硬编码个人机器路径

---

## ✅ 值得肯定的实现

- `join_exact` 逐字节校验"签名 path == 发送 path"，杜绝签名不一致整类事故（`rest.rs:440-457`）
- 错误分类（Fatal/Transient/限流）与文档完全对齐；幂等键设计正确——SDK 绝不自动重试 POST/DELETE
- 私钥 128-hex 格式校验"后半段公钥 == 派生公钥"，可提前发现录入错误
- SQL 全参数化无注入；AES-GCM 随机 nonce 无复用；Argon2id 默认参数合规；JWT 单算法校验无混淆风险
- 金额计算核心链路全用 rust_decimal；`grid_math.rs` 纯 Decimal 幂运算、`tp_engine.rs`、首单门禁实现质量高且测试充分
- 前端：401 拦截器豁免判断正确、快捷操作 in-flight 守卫 + 600ms 防连击、K 线切换 cancelled 标志防竞态

---

## 修复优先级建议

1. **立即**：轮换 config.toml 中全部真实密钥；加 .dockerignore；修默认 admin 口令
2. **实盘前必须**：B1-B8（锁仓持久化与冻结语义、WS 确认/超时/快照三方时序一致性）、C1-C3（策略端点 RBAC、start 互斥、WS 过滤精确匹配）、D1-D2
3. **紧随其后**：高优先级列表中的门禁接线（冷却、休市、swap、USC 换算）、JWT 传输方式、限流
4. **收尾**：清理 gl-strategy 死代码、统一入口、前端显示精度与竞态
5. 修复完成后做一轮**端到端故障注入回放**（断线、超时、对账不一致、重启恢复）
