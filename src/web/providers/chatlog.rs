//! ChatlogProvider —— 聊天记录查看(RPC,authority 4)。
//! `chatlog/conversations` 列出会话:bot 所在的全部群、以及所有好友与历史私聊对端的并集,各带消息数;
//! `chatlog/query` 按群或私聊对端拉取最近若干条消息,带发送者昵称、是否自身发出、原始 OneBot 内容段数组,
//! 按时间正序返回供页面渲染。

use nagisa::async_trait;
use nagisa::Bot;
use sea_orm::{ConnectionTrait, DatabaseConnection, Statement, Value as SqlValue};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;

const DEFAULT_LIMIT: u64 = 60;
const MAX_LIMIT: u64 = 200;

use crate::web::registry::{
    AuthUser, ConsoleContext, ConsolePlugin, ConsolePluginCtor, ConsoleRegistry, WebListener,
};

pub struct ChatlogProvider {
    db: DatabaseConnection,
    bot: Bot,
}

impl ConsolePlugin for ChatlogProvider {
    fn register(self: Arc<Self>, reg: &mut ConsoleRegistry) {
        reg.add_listener(Box::new(ChatlogConversations(Arc::clone(&self))));
        reg.add_listener(Box::new(ChatlogQuery(self)));
    }
}

// ───────────────────────── chatlog/conversations:会话清单 ─────────────────────────

struct ChatlogConversations(Arc<ChatlogProvider>);
#[async_trait]
impl WebListener for ChatlogConversations {
    fn event(&self) -> &'static str {
        "chatlog/conversations"
    }
    fn authority(&self) -> u8 {
        4
    }
    async fn handle(&self, _args: Value, _who: AuthUser) -> Result<Value, String> {
        let db = &self.0.db;
        let bot = &self.0.bot;

        // 各群消息数(一次聚合)。
        let group_counts: HashMap<i64, i64> = {
            let stmt = Statement::from_string(
                db.get_database_backend(),
                "SELECT group_id, count(*) AS n FROM chat_log \
                 WHERE group_id IS NOT NULL GROUP BY group_id"
                    .to_string(),
            );
            db.query_all(stmt)
                .await
                .map_err(|e| e.to_string())?
                .iter()
                .filter_map(|r| {
                    let g = r.try_get::<i64>("", "group_id").ok()?;
                    let n = r.try_get::<i64>("", "n").unwrap_or(0);
                    Some((g, n))
                })
                .collect()
        };

        // 各私聊对端消息数(一次聚合,group_id 为 NULL)。
        let priv_counts: HashMap<i64, i64> = {
            let stmt = Statement::from_string(
                db.get_database_backend(),
                "SELECT private_peer, count(*) AS n FROM chat_log \
                 WHERE group_id IS NULL AND private_peer IS NOT NULL GROUP BY private_peer"
                    .to_string(),
            );
            db.query_all(stmt)
                .await
                .map_err(|e| e.to_string())?
                .iter()
                .filter_map(|r| {
                    let p = r.try_get::<i64>("", "private_peer").ok()?;
                    let n = r.try_get::<i64>("", "n").unwrap_or(0);
                    Some((p, n))
                })
                .collect()
        };

        let mut conversations: Vec<Value> = Vec::new();

        // 群:bot 所在的全部群(含 0 条),按消息数降序。
        let groups = bot.get_group_list(true).await.map_err(|e| e.to_string())?;
        let mut group_items: Vec<(i64, String, i64)> = groups
            .into_iter()
            .map(|g| {
                let count = group_counts.get(&g.group.0).copied().unwrap_or(0);
                (g.group.0, g.name, count)
            })
            .collect();
        group_items.sort_by_key(|g| std::cmp::Reverse(g.2));
        for (id, name, count) in group_items {
            conversations.push(json!({ "kind": "group", "id": id, "name": name, "count": count }));
        }

        // 私聊:好友 ∪ 历史私聊对端。好友名取备注/昵称,其余只有 uin 的用 uin 字符串。
        let friends = bot.get_friend_list(true).await.map_err(|e| e.to_string())?;
        let mut names: HashMap<i64, String> = HashMap::new();
        for f in &friends {
            let name = f.display_name();
            let name = if name.is_empty() { f.user.0.to_string() } else { name.to_string() };
            names.insert(f.user.0, name);
        }
        // 对端全集:好友 + 有过私聊记录的对端。
        let mut peers: std::collections::HashSet<i64> = priv_counts.keys().copied().collect();
        peers.extend(friends.iter().map(|f| f.user.0));

        let mut priv_items: Vec<(i64, String, i64)> = peers
            .into_iter()
            .map(|p| {
                let name = names.get(&p).cloned().unwrap_or_else(|| p.to_string());
                let count = priv_counts.get(&p).copied().unwrap_or(0);
                (p, name, count)
            })
            .collect();
        priv_items.sort_by_key(|p| std::cmp::Reverse(p.2));
        for (id, name, count) in priv_items {
            conversations
                .push(json!({ "kind": "private", "id": id, "name": name, "count": count }));
        }

        Ok(json!({ "conversations": conversations }))
    }
}

// ───────────────────────── chatlog/query:消息明细 ─────────────────────────

struct ChatlogQuery(Arc<ChatlogProvider>);
#[async_trait]
impl WebListener for ChatlogQuery {
    fn event(&self) -> &'static str {
        "chatlog/query"
    }
    fn authority(&self) -> u8 {
        4
    }
    async fn handle(&self, args: Value, _who: AuthUser) -> Result<Value, String> {
        let db = &self.0.db;
        let backend = db.get_database_backend();

        let kind = args.get("kind").and_then(|v| v.as_str()).ok_or("缺少 kind")?;
        let id = args.get("id").and_then(|v| v.as_i64()).ok_or("缺少 id")?;
        let before_id = args.get("before_id").and_then(|v| v.as_i64());
        let limit = args
            .get("limit")
            .and_then(|v| v.as_u64())
            .unwrap_or(DEFAULT_LIMIT)
            .clamp(1, MAX_LIMIT);

        // 会话条件 + 值参数化:群走 group_id=$1,私聊走 group_id IS NULL AND private_peer=$1。
        let mut values: Vec<SqlValue> = Vec::new();
        let scope_clause = match kind {
            "group" => {
                values.push(SqlValue::from(id));
                format!("cl.group_id = ${}", values.len())
            }
            "private" => {
                values.push(SqlValue::from(id));
                format!("cl.group_id IS NULL AND cl.private_peer = ${}", values.len())
            }
            other => return Err(format!("未知的 kind:{other}")),
        };
        let before_clause = match before_id {
            Some(b) => {
                values.push(SqlValue::from(b));
                format!(" AND cl.id < ${}", values.len())
            }
            None => String::new(),
        };

        // content 经 to_jsonb 原样带回(已是 jsonb 数组)。limit 已夹紧,可内联。
        let sql = format!(
            "SELECT cl.id, cl.uin, u.nickname, cl.from_self, to_jsonb(cl.content) AS content, cl.time \
             FROM chat_log cl LEFT JOIN \"user\" u ON u.uin = cl.uin \
             WHERE {scope_clause}{before_clause} ORDER BY cl.id DESC LIMIT {limit}"
        );
        let stmt = Statement::from_sql_and_values(backend, &sql, values);
        let rows = db.query_all(stmt).await.map_err(|e| e.to_string())?;

        let mut messages: Vec<Value> = rows
            .iter()
            .map(|r| {
                let time = r
                    .try_get::<chrono::DateTime<chrono::FixedOffset>>("", "time")
                    .map(|t| t.to_rfc3339())
                    .unwrap_or_default();
                json!({
                    "id": r.try_get::<i64>("", "id").unwrap_or(0),
                    "uin": r.try_get::<i64>("", "uin").unwrap_or(0),
                    "nickname": r.try_get::<Option<String>>("", "nickname").unwrap_or(None),
                    "from_self": r.try_get::<bool>("", "from_self").unwrap_or(false),
                    "content": r.try_get::<Value>("", "content").unwrap_or(Value::Null),
                    "time": time,
                })
            })
            .collect();
        // 取的是 DESC(最近在前),页面要时间正序,翻转回旧→新。
        messages.reverse();

        Ok(json!({ "messages": messages }))
    }
}

nagisa::inventory::submit! {
    ConsolePluginCtor(|cx: &ConsoleContext| -> Arc<dyn ConsolePlugin> {
        Arc::new(ChatlogProvider { db: cx.db.clone(), bot: cx.bot.clone() })
    })
}
