//! ConfigProvider —— 把所有 ConfigSpec 的 schema+当前值做成 `config` DataService,`config/set`
//! 校验后写库 + 热生效。

use nagisa::async_trait;
use sea_orm::DatabaseConnection;
use serde_json::{Value, json};
use std::sync::Arc;

use crate::web::config::{ConfigSpec, ConfigStore};
use crate::web::registry::{
    AuthUser, ConsoleContext, ConsolePlugin, ConsolePluginCtor, ConsoleRegistry, WebDataService, WebListener,
};

pub struct ConfigProvider {
    db: DatabaseConnection,
    store: ConfigStore,
}

impl ConfigProvider {
    pub fn new(cx: &ConsoleContext) -> Arc<Self> {
        Arc::new(Self { db: cx.db.clone(), store: cx.config.clone() })
    }
}

impl ConsolePlugin for ConfigProvider {
    fn register(self: Arc<Self>, reg: &mut ConsoleRegistry) {
        reg.add_data_service(Box::new(ConfigData(Arc::clone(&self))));
        reg.add_listener(Box::new(ConfigSet(self)));
    }
}

struct ConfigData(Arc<ConfigProvider>);
#[async_trait]
impl WebDataService for ConfigData {
    fn key(&self) -> &'static str {
        "config"
    }
    fn authority(&self) -> u8 {
        4
    }
    async fn get(&self) -> Value {
        let mut specs = Vec::new();
        let mut values = serde_json::Map::new();
        for spec in nagisa::inventory::iter::<ConfigSpec> {
            specs.push(json!({
                "plugin_key": spec.plugin_key,
                "title": spec.title,
                "schema": (spec.schema)(),
            }));
            let cur = self.0.store.get(spec.plugin_key).map(|v| (*v).clone()).unwrap_or_else(|| (spec.default)());
            values.insert(spec.plugin_key.to_string(), cur);
        }
        json!({ "specs": specs, "values": values })
    }
}

struct ConfigSet(Arc<ConfigProvider>);
#[async_trait]
impl WebListener for ConfigSet {
    fn event(&self) -> &'static str {
        "config/set"
    }
    fn authority(&self) -> u8 {
        4
    }
    async fn handle(&self, args: Value, _who: AuthUser) -> Result<Value, String> {
        let plugin_key = args.get("plugin_key").and_then(|v| v.as_str()).ok_or("缺少 plugin_key")?;
        let value = args.get("value").ok_or("缺少 value")?;
        let mut found = None;
        for s in nagisa::inventory::iter::<ConfigSpec> {
            if s.plugin_key == plugin_key {
                found = Some(s);
                break;
            }
        }
        let spec = found.ok_or("未知 plugin_key")?;
        let normalized = (spec.validate)(value)?;
        self.0.store.set(&self.0.db, plugin_key, normalized).await?;
        Ok(json!({ "ok": true }))
    }
}

nagisa::inventory::submit! {
    ConsolePluginCtor(|cx: &ConsoleContext| -> Arc<dyn ConsolePlugin> { ConfigProvider::new(cx) })
}
