//! WS 线协议 —— 抄 Koishi console,精简到四类消息。
//!
//! 服务器→前端:`data`(某 DataService 全量)/ `patch`(增量)/ `response`(RPC 回执)/
//! `pong`(保活);前端→服务器:`send`(主动 RPC)/ `ping`。tag 字段名为 `type`。

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 服务器 → 前端。
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ServerMsg {
    /// 某 DataService 的全量数据(前端 `store[key] = value`)。
    Data { key: String, value: Value },
    /// 某 DataService 的增量(前端按约定 append/merge)。
    Patch { key: String, value: Value },
    /// 一次 RPC 的回执。
    Response {
        id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        value: Option<Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    /// 保活。
    Pong,
}

/// 前端 → 服务器。
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ClientMsg {
    /// 主动 RPC:调用某 listener。
    Send {
        id: String,
        event: String,
        #[serde(default)]
        args: Value,
    },
    /// 保活。
    Ping,
}
