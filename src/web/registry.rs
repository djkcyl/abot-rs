//! Provider 注册表 —— 插件经 `inventory` 自注册数据源/监听器(照搬 `PluginMigration` 路子)。
//!
//! 两条数据原语:[`WebDataService`](被动推:`get()` 全量,内容变了 Hub 广播)与
//! [`WebListener`](主动 RPC:前端 `send(event,args)`)。一个 [`ConsolePlugin`] 可同时贡献
//! 数据源与监听器,并持有共享 `Arc` 状态。`inventory` 项是静态的,故注册的是**构造器**
//! [`ConsolePluginCtor`],`ConsoleService::prepare` 时给 [`ConsoleContext`] 实例化。

use nagisa::Bot;
use nagisa::async_trait;
use sea_orm::DatabaseConnection;
use serde_json::Value;
use std::sync::Arc;

/// 鉴权后的访问者。连接时从 `?token=` 解析;无/失效 token 则 `authority` 为 0。
#[derive(Debug, Clone, Copy)]
pub struct AuthUser {
    pub uin: i64,
    pub authority: u8,
}

/// Provider 实例化时拿到的共享上下文。
#[derive(Clone)]
pub struct ConsoleContext {
    pub db: DatabaseConnection,
    pub bot: Bot,
    pub config: crate::web::config::ConfigStore,
    /// 最近若干条日志的环形缓冲(JSON 行)。日志泵后台往里推、LogsProvider 据此做连接即推的回填;
    /// 二者共享同一个 `Arc`。
    pub log_buf: crate::web::LogBuf,
    /// 进程启动时刻。总览算在线时长用。
    pub boot: chrono::DateTime<chrono::Utc>,
    /// router 据以门控的插件开关表(同一个共享句柄)。插件页据它读/改开关。
    pub enabled: std::sync::Arc<nagisa::EnabledSet>,
}

/// 被动推数据源:key 即前端 store 键;内容变化时 Provider 调 `Hub::refresh(key)` 广播。
#[async_trait]
pub trait WebDataService: Send + Sync {
    /// 前端 store 键(全局唯一)。
    fn key(&self) -> &'static str;
    /// 取该数据所需的最低 authority(默认 1 = 任意登录用户)。
    fn authority(&self) -> u8 {
        1
    }
    /// 全量数据。
    async fn get(&self) -> Value;
}

/// 主动 RPC 监听器。
#[async_trait]
pub trait WebListener: Send + Sync {
    /// 事件名(前端 `send(event, args)` 的 event)。
    fn event(&self) -> &'static str;
    /// 调用所需的最低 authority(默认 4 = 写操作级)。
    fn authority(&self) -> u8 {
        4
    }
    /// 处理一次调用。`Err(String)` 作为 RPC error 回前端。
    async fn handle(&self, args: Value, who: AuthUser) -> Result<Value, String>;
}

/// 注册表:Provider 在 [`ConsolePlugin::register`] 里往这里塞数据源/监听器。
#[derive(Default)]
pub struct ConsoleRegistry {
    pub data_services: Vec<Box<dyn WebDataService>>,
    pub listeners: Vec<Box<dyn WebListener>>,
}

impl ConsoleRegistry {
    /// 登记一个被动推数据源。
    pub fn add_data_service(&mut self, ds: Box<dyn WebDataService>) {
        self.data_services.push(ds);
    }
    /// 登记一个主动 RPC 监听器。
    pub fn add_listener(&mut self, l: Box<dyn WebListener>) {
        self.listeners.push(l);
    }
}

/// 一个 console 插件:把自己的数据源/监听器登记进注册表。实例可持有共享 `Arc` 状态。
pub trait ConsolePlugin: Send + Sync {
    fn register(self: Arc<Self>, reg: &mut ConsoleRegistry);
}

/// `inventory` 自注册槽:给 [`ConsoleContext`]、产出一个 [`ConsolePlugin`] 实例
/// (与 `PluginMigration` 同款机制)。插件在自己模块里
/// `nagisa::inventory::submit!{ ConsolePluginCtor(|cx| Arc::new(MyPlugin::new(cx))) }`。
pub struct ConsolePluginCtor(pub fn(&ConsoleContext) -> Arc<dyn ConsolePlugin>);
nagisa::inventory::collect!(ConsolePluginCtor);
