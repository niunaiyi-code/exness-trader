//! 全局 WS 事件广播器 — 运行器状态帧。

use crate::ws::{WsFrame, WsHub};

/// 向所有订阅者广播一条运行器状态帧。
pub fn broadcast_runner_status(hub: &WsHub, account_id: String, status: String) {
    hub.broadcast(WsFrame::RunnerStatus { account_id, status });
}
