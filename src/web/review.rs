//! 通用审核框架 —— 消费者(漂流瓶/投稿/入群…)各注册一个 [`ReviewSource`],自定列表/按钮/
//! 详情/功能;[`ReviewProvider`] 聚合各审核来源的待审 + 按来源派发 `detail`/`invoke`。待审真值在各
//! 消费者自己的库里(重启自动重建,无中心队列表、不存闭包)。

use nagisa::async_trait;
use nagisa::Bot;
use sea_orm::DatabaseConnection;
use serde_json::{json, Value};
use std::sync::Arc;

use crate::web::registry::{
    AuthUser, ConsoleContext, ConsolePlugin, ConsolePluginCtor, ConsoleRegistry, WebDataService,
    WebListener,
};

/// 列表列定义。
pub struct Column {
    pub key: &'static str,
    pub label: &'static str,
}
/// 按钮定义。
pub struct Action {
    pub key: &'static str,
    pub label: &'static str,
    pub style: &'static str,
}
/// 一条待审项(`columns` 字段对应该 source 的 `list_columns`)。
pub struct Entry {
    pub id: String,
    pub columns: Value,
}

/// handle / pending / detail 用的上下文。
#[derive(Clone)]
pub struct ReviewContext {
    pub db: DatabaseConnection,
    pub bot: Bot,
}

/// 一个审核来源。消费者实现并经 [`ReviewSourceCtor`] 注册。
#[async_trait]
pub trait ReviewSource: Send + Sync {
    fn source(&self) -> &'static str;
    fn label(&self) -> &'static str;
    fn list_columns(&self) -> Vec<Column>;
    fn actions(&self) -> Vec<Action>;
    async fn pending(&self, ctx: &ReviewContext) -> Vec<Entry>;
    async fn detail(&self, id: &str, ctx: &ReviewContext) -> Value;
    async fn handle(
        &self,
        action: &str,
        id: &str,
        who: AuthUser,
        ctx: &ReviewContext,
    ) -> Result<(), String>;
}

/// `inventory` 自注册槽。
pub struct ReviewSourceCtor(pub fn(&ConsoleContext) -> Arc<dyn ReviewSource>);
nagisa::inventory::collect!(ReviewSourceCtor);

/// 聚合所有 `ReviewSource`:一个 `review` DataService + `review/detail`、`review/invoke` 两 Listener。
pub struct ReviewProvider {
    sources: Vec<Arc<dyn ReviewSource>>,
    ctx: ReviewContext,
}

impl ReviewProvider {
    pub fn collect(cx: &ConsoleContext) -> Arc<Self> {
        let mut sources: Vec<Arc<dyn ReviewSource>> = Vec::new();
        for c in nagisa::inventory::iter::<ReviewSourceCtor> {
            sources.push((c.0)(cx));
        }
        Arc::new(Self {
            sources,
            ctx: ReviewContext { db: cx.db.clone(), bot: cx.bot.clone() },
        })
    }
    fn find(&self, source: &str) -> Option<&Arc<dyn ReviewSource>> {
        self.sources.iter().find(|s| s.source() == source)
    }
}

impl ConsolePlugin for ReviewProvider {
    fn register(self: Arc<Self>, reg: &mut ConsoleRegistry) {
        reg.add_data_service(Box::new(ReviewData(Arc::clone(&self))));
        reg.add_listener(Box::new(ReviewDetail(Arc::clone(&self))));
        reg.add_listener(Box::new(ReviewInvoke(self)));
    }
}

struct ReviewData(Arc<ReviewProvider>);
#[async_trait]
impl WebDataService for ReviewData {
    fn key(&self) -> &'static str {
        "review"
    }
    fn authority(&self) -> u8 {
        4
    }
    async fn get(&self) -> Value {
        let mut sources = Vec::new();
        let mut items = Vec::new();
        for s in &self.0.sources {
            sources.push(json!({
                "source": s.source(),
                "label": s.label(),
                "columns": s.list_columns().iter().map(|c| json!({"key":c.key,"label":c.label})).collect::<Vec<_>>(),
                "actions": s.actions().iter().map(|a| json!({"key":a.key,"label":a.label,"style":a.style})).collect::<Vec<_>>(),
            }));
            for e in s.pending(&self.0.ctx).await {
                items.push(json!({ "source": s.source(), "id": e.id, "columns": e.columns }));
            }
        }
        json!({ "sources": sources, "items": items })
    }
}

struct ReviewDetail(Arc<ReviewProvider>);
#[async_trait]
impl WebListener for ReviewDetail {
    fn event(&self) -> &'static str {
        "review/detail"
    }
    fn authority(&self) -> u8 {
        4
    }
    async fn handle(&self, args: Value, _who: AuthUser) -> Result<Value, String> {
        let source = args.get("source").and_then(|v| v.as_str()).ok_or("缺少 source")?;
        let id = args.get("id").and_then(|v| v.as_str()).ok_or("缺少 id")?;
        let s = self.0.find(source).ok_or("未知 source")?;
        Ok(s.detail(id, &self.0.ctx).await)
    }
}

struct ReviewInvoke(Arc<ReviewProvider>);
#[async_trait]
impl WebListener for ReviewInvoke {
    fn event(&self) -> &'static str {
        "review/invoke"
    }
    fn authority(&self) -> u8 {
        4
    }
    async fn handle(&self, args: Value, who: AuthUser) -> Result<Value, String> {
        let source = args.get("source").and_then(|v| v.as_str()).ok_or("缺少 source")?;
        let id = args.get("id").and_then(|v| v.as_str()).ok_or("缺少 id")?;
        let action = args.get("action").and_then(|v| v.as_str()).ok_or("缺少 action")?;
        let s = self.0.find(source).ok_or("未知 source")?;
        s.handle(action, id, who, &self.0.ctx).await?;
        Ok(json!({ "ok": true }))
    }
}

// ReviewProvider 自身是一个 ConsolePlugin(prepare 时 collect 所有 ReviewSource)。
nagisa::inventory::submit! {
    ConsolePluginCtor(|cx: &ConsoleContext| -> Arc<dyn ConsolePlugin> { ReviewProvider::collect(cx) })
}
