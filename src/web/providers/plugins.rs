//! PluginProvider —— 把 `registered_plugins()` 的插件清单做成 `plugins` DataService(key `plugins`),
//! 并附插件总开关的当前状态;`plugin/toggle` 翻转开关(即时作用于分发)+ 持久化。
//! 清单内容编译期登记、`get()` 现算;authority 1(任意登录用户可见),toggle authority 4。

use nagisa::async_trait;
use nagisa::{registered_plugins, registered_triggers, EnabledSet, PluginMeta, TriggerKind, TriggerMeta};
use sea_orm::DatabaseConnection;
use serde_json::{json, Value};
use std::sync::Arc;

use crate::web::registry::{
    AuthUser, ConsoleContext, ConsolePlugin, ConsolePluginCtor, ConsoleRegistry, WebDataService,
    WebListener,
};
use crate::web::switches::store_overrides;

/// 插件清单 + 开关 Provider。
pub struct PluginProvider {
    db: DatabaseConnection,
    enabled: Arc<EnabledSet>,
}

impl PluginProvider {
    pub fn new(cx: &ConsoleContext) -> Arc<Self> {
        Arc::new(Self { db: cx.db.clone(), enabled: cx.enabled.clone() })
    }
}

impl ConsolePlugin for PluginProvider {
    fn register(self: Arc<Self>, reg: &mut ConsoleRegistry) {
        reg.add_data_service(Box::new(PluginData(Arc::clone(&self))));
        reg.add_listener(Box::new(PluginToggle(Arc::clone(&self))));
        reg.add_listener(Box::new(CommandToggle(self)));
    }
}

struct PluginData(Arc<PluginProvider>);

#[async_trait]
impl WebDataService for PluginData {
    fn key(&self) -> &'static str {
        "plugins"
    }
    fn authority(&self) -> u8 {
        1
    }
    async fn get(&self) -> Value {
        let enabled = &self.0.enabled;
        // 触发器清单一次取出，按插件有效 key 给每个插件挑出它名下的命令。
        let triggers = registered_triggers();
        let plugins: Vec<Value> = registered_plugins()
            .iter()
            .map(|p| plugin_json(p, &triggers, enabled))
            .collect();
        json!({ "plugins": plugins })
    }
}

/// 插件的「有效 key」:显式写了 `key=` 就用它,否则退回 `module_path` 的最后一段。
///
/// 与触发器侧的 `plugin_key` 口径一致,这样命令才能对回所属插件。
fn effective_key(p: &PluginMeta) -> &str {
    if p.key.is_empty() {
        p.module_path.rsplit("::").next().unwrap_or(p.module_path)
    } else {
        p.key
    }
}

/// 把一个 `PluginMeta` 序列化成前端要的字段(含开关当前状态、是否可停用,以及名下命令清单)。
fn plugin_json(p: &PluginMeta, triggers: &[TriggerMeta], enabled: &EnabledSet) -> Value {
    json!({
        "key": p.key,
        "name": p.name,
        "category": format!("{:?}", p.category),
        "description": plugin_desc(p, triggers),
        "hidden": p.hidden,
        "enabled": enabled.is_enabled(p.key, p.default_enable, None),
        "can_disable": p.can_disable,
        "commands": commands_json(effective_key(p), triggers, enabled),
    })
}

/// 插件简介:有 `plugin.description` 用之;没有(单命令插件不写插件级描述)则取那唯一一条
/// 非隐藏命令的描述兜底——和 help 同口径,元数据只写在命令上、不和插件级重复。
fn plugin_desc<'a>(p: &'a PluginMeta, triggers: &'a [TriggerMeta]) -> &'a str {
    if !p.description.is_empty() {
        return p.description;
    }
    let key = effective_key(p);
    let mut it = triggers
        .iter()
        .filter(|t| matches!(t.kind, TriggerKind::Command) && !t.hidden && t.plugin_key == key);
    match (it.next(), it.next()) {
        (Some(only), None) => only.description,
        _ => "",
    }
}

/// 某插件名下的命令清单:只取命令型触发器(过滤掉事件型),每条带自身子开关的当前状态。
fn commands_json(plugin_key: &str, triggers: &[TriggerMeta], enabled: &EnabledSet) -> Vec<Value> {
    let mut cmds: Vec<&TriggerMeta> = triggers
        .iter()
        .filter(|t| matches!(t.kind, TriggerKind::Command) && t.plugin_key == plugin_key)
        .collect();
    cmds.sort_by_key(|t| t.order); // 与 help 同序(order 小在前,稳定)
    cmds.into_iter()
        .map(|t| {
            json!({
                "id": t.id,
                "name": t.name,
                "words": t.words,
                "description": t.description,
                "enabled": enabled.is_enabled(t.key, t.default_enable, None),
                "can_disable": t.can_disable,
                "hidden": t.hidden,
                "key": t.key,
            })
        })
        .collect()
}

/// `plugin/toggle` —— 翻转插件总开关并持久化。
struct PluginToggle(Arc<PluginProvider>);

#[async_trait]
impl WebListener for PluginToggle {
    fn event(&self) -> &'static str {
        "plugin/toggle"
    }
    fn authority(&self) -> u8 {
        4
    }
    async fn handle(&self, args: Value, _who: AuthUser) -> Result<Value, String> {
        let key = args.get("key").and_then(|v| v.as_str()).ok_or("缺少 key")?;
        let want = args.get("enabled").and_then(|v| v.as_bool()).ok_or("缺少 enabled")?;

        // 校验插件存在且允许停用(不可停用的插件不许动开关)。
        let meta = registered_plugins()
            .into_iter()
            .find(|p| p.key == key)
            .ok_or("未知插件")?;
        if !meta.can_disable {
            return Err("该插件不可停用".into());
        }

        // 翻转(即时作用于运行中的分发)。
        self.0.enabled.set(key, None, want);
        // 持久化整份覆盖快照。
        store_overrides(&self.0.db, &self.0.enabled.snapshot()).await?;

        tracing::warn!(target: "abot::web::audit", plugin = %key, %want, "网页控制台插件开关");
        Ok(json!({ "key": key, "enabled": want }))
    }
}

/// `command/toggle` —— 翻转单条命令子开关并持久化(键为 `"<plugin_key>.<id>"` 点分形)。
struct CommandToggle(Arc<PluginProvider>);

#[async_trait]
impl WebListener for CommandToggle {
    fn event(&self) -> &'static str {
        "command/toggle"
    }
    fn authority(&self) -> u8 {
        4
    }
    async fn handle(&self, args: Value, _who: AuthUser) -> Result<Value, String> {
        let key = args.get("key").and_then(|v| v.as_str()).ok_or("缺少 key")?;
        let want = args.get("enabled").and_then(|v| v.as_bool()).ok_or("缺少 enabled")?;

        // 校验:该 key 对应一条命令型触发器,且允许停用(不可停用的命令不许动开关)。
        let meta = registered_triggers()
            .into_iter()
            .find(|t| t.key == key && matches!(t.kind, TriggerKind::Command))
            .ok_or("未知命令")?;
        if !meta.can_disable {
            return Err("该命令不可停用".into());
        }

        // 翻转(即时作用于运行中的分发)。
        self.0.enabled.set(key, None, want);
        // 持久化整份覆盖快照。
        store_overrides(&self.0.db, &self.0.enabled.snapshot()).await?;

        tracing::warn!(target: "abot::web::audit", command = %key, %want, "网页控制台命令开关");
        Ok(json!({ "key": key, "enabled": want }))
    }
}

// 自注册:控制台 prepare 时经 ctx 实例化(需 db + EnabledSet)。
nagisa::inventory::submit! {
    ConsolePluginCtor(|cx: &ConsoleContext| -> Arc<dyn ConsolePlugin> { PluginProvider::new(cx) })
}
