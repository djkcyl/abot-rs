//! SessionsProvider —— 会话/Token 管理(仅主人)。两个 RPC 监听器:
//! `tokens/list`(列出已签发的登录 token)与 `token/revoke`(吊销某个 token)。
//! 列表回完整 token(前端需据它吊销,展示时自行打码);吊销即审计。

use nagisa::async_trait;
use sea_orm::{DatabaseConnection, EntityTrait, QueryOrder};
use serde_json::{json, Value};
use std::sync::Arc;

use crate::web::entity;
use crate::web::registry::{
    AuthUser, ConsoleContext, ConsolePlugin, ConsolePluginCtor, ConsoleRegistry, WebListener,
};

pub struct SessionsProvider {
    db: DatabaseConnection,
}

impl ConsolePlugin for SessionsProvider {
    fn register(self: Arc<Self>, reg: &mut ConsoleRegistry) {
        reg.add_listener(Box::new(TokensList(self.db.clone())));
        reg.add_listener(Box::new(TokenRevoke(self.db.clone())));
    }
}

struct TokensList(DatabaseConnection);

#[async_trait]
impl WebListener for TokensList {
    fn event(&self) -> &'static str {
        "tokens/list"
    }
    fn authority(&self) -> u8 {
        5
    }
    async fn handle(&self, _args: Value, _who: AuthUser) -> Result<Value, String> {
        let now = chrono::Utc::now().fixed_offset();
        let rows = entity::Entity::find()
            .order_by_desc(entity::Column::CreatedAt)
            .all(&self.0)
            .await
            .map_err(|e| e.to_string())?;
        let tokens: Vec<Value> = rows
            .iter()
            .map(|m| {
                json!({
                    "token": m.token,
                    "uin": m.uin,
                    "authority": m.authority,
                    "created_at": m.created_at.to_rfc3339(),
                    "expires_at": m.expires_at.to_rfc3339(),
                    "valid": m.expires_at > now,
                })
            })
            .collect();
        Ok(json!({ "tokens": tokens }))
    }
}

struct TokenRevoke(DatabaseConnection);

#[async_trait]
impl WebListener for TokenRevoke {
    fn event(&self) -> &'static str {
        "token/revoke"
    }
    fn authority(&self) -> u8 {
        5
    }
    async fn handle(&self, args: Value, _who: AuthUser) -> Result<Value, String> {
        let token = args.get("token").and_then(|v| v.as_str()).ok_or("缺少 token")?;
        let res = entity::Entity::delete_by_id(token.to_string())
            .exec(&self.0)
            .await
            .map_err(|e| e.to_string())?;
        tracing::warn!(target: "abot::web::audit", "网页控制台吊销会话");
        Ok(json!({ "ok": true, "affected": res.rows_affected }))
    }
}

nagisa::inventory::submit! {
    ConsolePluginCtor(|cx: &ConsoleContext| -> Arc<dyn ConsolePlugin> {
        Arc::new(SessionsProvider { db: cx.db.clone() })
    })
}
