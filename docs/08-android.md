# 08 · Android 端规划（阶段二）

> Web 端交付并稳定后启动。Android 端不含任何策略逻辑，只是后端 API 的另一个客户端。

---

## 1. 目标与范围

| 做 | 不做 |
| --- | --- |
| 多账户总览、账户详情看板、七个手动动作、告警推送、参数查看 | 策略计算、本地下单、账户新增 / 凭证录入（仅 Web admin） |
| 离线缓存最近一帧与告警 | 图表高级交互 |

---

## 2. 技术栈

| 项 | 选择 |
| --- | --- |
| 语言 / UI | Kotlin + Jetpack Compose（Material 3，深色主题） |
| 网络 | Retrofit + OkHttp（REST、WS）、kotlinx.serialization |
| DTO | 由后端 OpenAPI 生成（openapi-generator kotlin） |
| 架构 | MVVM + Hilt + Coroutines/Flow |
| 推送 | Firebase Cloud Messaging；后端 `notify` 新增 `fcm` 通道，设备注册 `POST /devices` |
| 安全 | EncryptedSharedPreferences 存刷新令牌；生物识别解锁敏感操作 |
| 最低版本 | Android 8.0（API 26） |

---

## 3. 页面

| 页面 | 内容 |
| --- | --- |
| 登录 | 用户名 / 密码 /（TOTP），记住刷新令牌 |
| 总览 | 账户卡片列表（状态、净值、浮盈亏、多空层数、徽标），下拉刷新 + WS 实时 |
| 账户详情 | HUD 状态条、账户数字、多 / 空篮子卡、持仓列表、快捷动作底部栏 |
| 动作确认 | 底部弹层：动作说明 + 生物识别 / 密码 → 调用 `/actions` → 进度 |
| 告警 | 列表、确认 |
| 参数 | 只读（含推导值）；编辑能力视权限后续开放 |
| 设置 | 服务器地址、通知偏好、主题 |

---

## 4. 实时与后台

- 目标：前台通过 WSS 订阅已授权账户的状态帧、成交事件与告警，与 Web 共用协议（06 §9）。
- 当前：Android 客户端尚未实现；服务端 `/ws` 只有行情与运行状态帧，账户级事件订阅、身份认证和授权过滤尚未完成。接入私有账户数据前须先补齐这些能力。
- 后台：不保持 WS；依赖 FCM 推送千元阶梯、锁仓、自动全平、系统告警（对应 EA `SendNotification` 的场景）。
- 推送点击深链到账户详情。

---

## 5. 后端配套改动

| 项 | 说明 |
| --- | --- |
| `POST /devices` / `DELETE /devices/{id}` | 注册 FCM token，绑定用户与可见账户 |
| 通知通道 `fcm` | 按用户授权账户路由；告警去重与节流沿用服务端规则 |
| 刷新令牌有效期 | 移动端可配置更长（30 天）并支持设备级吊销 |

---

## 6. 里程碑（挂在 09 的 M6）

1. OpenAPI 生成 DTO、登录 / 总览 / 详情只读。
2. 手动动作 + 确认流。
3. FCM 推送。
4. 内测（Firebase App Distribution）。
