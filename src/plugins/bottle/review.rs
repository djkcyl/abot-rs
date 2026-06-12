//! 漂流瓶接入通用审核框架 —— 超管在网页「审核」页处理待人工的瓶子（投放命中审核转人工的那批）。
//!
//! 不存任何状态：待审真值就是库里 `status='pending'` 的瓶子，`pending` 每次现查；通过/驳回直接
//! 改 `status` 并尽力私聊通知投放者。注册经 [`ReviewSourceCtor`] 自挂到审核框架。

use nagisa::async_trait;
use nagisa::{Peer, Segment, Uin};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder};
use serde_json::{Value, json};

use crate::web::registry::AuthUser;
use crate::web::review::{Action, Column, Entry, ReviewContext, ReviewSource};

use super::entity::bottle;
use super::logic;

/// 漂流瓶审核来源。无状态：db/bot 都从 `ctx` 取。
struct BottleReviewSource;

/// 取 `moderation` JSON 里的 `label`；取不到给空串。
fn hit_label(moderation: &Option<Value>) -> String {
    moderation.as_ref().and_then(|m| m.get("label")).and_then(|l| l.as_str()).unwrap_or("").to_string()
}

/// `images` JSONB 数组的长度（非数组按 0）。
fn images_count(images: &Value) -> usize {
    images.as_array().map(|a| a.len()).unwrap_or(0)
}

/// 内容摘要：取前 30 字符，超出加省略号；无文本给「（无文本）」。
fn text_brief(text: &Option<String>) -> String {
    match text {
        Some(t) if !t.is_empty() => {
            let chars: Vec<char> = t.chars().collect();
            if chars.len() > 30 {
                format!("{}…", chars[..30].iter().collect::<String>())
            } else {
                chars.iter().collect()
            }
        }
        _ => "（无文本）".to_string(),
    }
}

#[async_trait]
impl ReviewSource for BottleReviewSource {
    fn source(&self) -> &'static str {
        "bottle"
    }
    fn label(&self) -> &'static str {
        "漂流瓶"
    }
    fn list_columns(&self) -> Vec<Column> {
        vec![
            Column { key: "id", label: "编号" },
            Column { key: "from", label: "投放者" },
            Column { key: "group", label: "来源群" },
            Column { key: "text", label: "内容" },
            Column { key: "images", label: "图片数" },
            Column { key: "hit", label: "命中" },
            Column { key: "time", label: "投放时间" },
        ]
    }
    fn actions(&self) -> Vec<Action> {
        vec![
            Action { key: "approve", label: "通过", style: "primary" },
            Action { key: "reject", label: "驳回", style: "error" },
        ]
    }

    async fn pending(&self, ctx: &ReviewContext) -> Vec<Entry> {
        let rows = bottle::Entity::find()
            .filter(bottle::Column::Status.eq("pending"))
            .filter(bottle::Column::Isdelete.eq(false))
            .order_by_asc(bottle::Column::CreatedAt)
            .all(&ctx.db)
            .await
            .unwrap_or_default();
        rows.into_iter()
            .map(|b| Entry {
                id: b.id.to_string(),
                columns: json!({
                    "id": b.id,
                    "from": b.nickname.clone().unwrap_or_else(|| b.uin.to_string()),
                    "group": b.group_id,
                    "text": text_brief(&b.text),
                    "images": images_count(&b.images),
                    "hit": hit_label(&b.moderation),
                    "time": b.created_at.format("%Y-%m-%d %H:%M:%S").to_string(),
                }),
            })
            .collect()
    }

    async fn detail(&self, id: &str, ctx: &ReviewContext) -> Value {
        let Ok(id) = id.parse::<i64>() else {
            return json!({ "error": "编号非法" });
        };
        match logic::get_bottle(&ctx.db, id).await {
            Ok(Some(b)) => {
                // 图片给带签名的访问 URL（/api/media/<名>?sig=…），审核页直接 <img>。
                let images: Vec<String> = b
                    .images
                    .as_array()
                    .map(|a| a.iter().filter_map(|v| v.as_str()).map(crate::web::media::signed_path).collect())
                    .unwrap_or_default();
                json!({
                    "id": b.id,
                    "from_uin": b.uin,
                    "from_name": b.nickname,
                    "group": b.group_id,
                    "anonymous": b.anonymous,
                    "text": b.text,
                    "images": images,
                    "moderation": b.moderation,
                    "remaining": b.remaining_pickups,
                    "status": b.status,
                    "time": b.created_at.format("%Y-%m-%d %H:%M:%S").to_string(),
                })
            }
            Ok(None) => json!({ "error": "漂流瓶不存在" }),
            Err(e) => json!({ "error": e.to_string() }),
        }
    }

    async fn handle(&self, action: &str, id: &str, _who: AuthUser, ctx: &ReviewContext) -> Result<(), String> {
        let id: i64 = id.parse().map_err(|_| "编号非法".to_string())?;
        let b = logic::get_bottle(&ctx.db, id)
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "漂流瓶不存在".to_string())?;

        let (new_status, note) = match action {
            "approve" => ("approved", format!("你的漂流瓶 {id} 已通过审核")),
            "reject" => ("rejected", format!("你的漂流瓶 {id} 未通过审核")),
            other => return Err(format!("未知动作：{other}")),
        };

        logic::set_status(&ctx.db, id, new_status).await.map_err(|e| e.to_string())?;

        // 尽力私聊通知投放者；发不出去不影响审核结果。
        let _ = ctx.bot.send(&Peer::friend(Uin(b.uin)), &[Segment::text(note)]).await;

        tracing::warn!(target: "abot::web::audit", action, bottle = id, "漂流瓶审核操作");
        Ok(())
    }
}

nagisa::inventory::submit! {
    crate::web::review::ReviewSourceCtor(|_cx: &crate::web::registry::ConsoleContext| -> std::sync::Arc<dyn crate::web::review::ReviewSource> {
        std::sync::Arc::new(BottleReviewSource)
    })
}
