//! LogsProvider —— 把最近日志的环形缓冲做成 DataService(key `logs`,authority 4)。
//!
//! 缓冲由 `ConsoleService::run` 里的日志泵后台填充,二者共享 `ConsoleContext::log_buf` 的同一个 `Arc`。
//! `get()` 返回当前整段缓冲,用于连接即推的回填;之后的实时行由日志泵经 `Hub::broadcast_patch` 增量送达。

use nagisa::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;

use crate::web::registry::{
    ConsoleContext, ConsolePlugin, ConsolePluginCtor, ConsoleRegistry, WebDataService,
};
use crate::web::LogBuf;

/// 日志缓冲 Provider。持有与日志泵共享的环形缓冲。
pub struct LogsProvider {
    pub buf: LogBuf,
}

impl ConsolePlugin for LogsProvider {
    fn register(self: Arc<Self>, reg: &mut ConsoleRegistry) {
        reg.add_data_service(Box::new(LogsData { buf: self.buf.clone() }));
    }
}

struct LogsData {
    buf: LogBuf,
}

#[async_trait]
impl WebDataService for LogsData {
    fn key(&self) -> &'static str {
        "logs"
    }
    fn authority(&self) -> u8 {
        4
    }
    async fn get(&self) -> Value {
        json!(self.buf.lock().await.iter().cloned().collect::<Vec<_>>())
    }
}

// 自注册:控制台 prepare 时实例化,从 ctx 取与日志泵共享的缓冲。
nagisa::inventory::submit! {
    ConsolePluginCtor(|cx: &ConsoleContext| -> Arc<dyn ConsolePlugin> {
        Arc::new(LogsProvider { buf: cx.log_buf.clone() })
    })
}
