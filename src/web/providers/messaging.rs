//! MessagingProvider —— 网页消息发送台。一个 RPC 监听器 `message/send`(仅主人):
//! 以机器人身份往群或好友发一条文本消息。发送即审计。

use nagisa::async_trait;
use nagisa::{Bot, Peer, Segment};
use serde_json::{json, Value};
use std::sync::Arc;

use crate::web::registry::{
    AuthUser, ConsoleContext, ConsolePlugin, ConsolePluginCtor, ConsoleRegistry, WebListener,
};

pub struct MessagingProvider {
    bot: Bot,
}

impl ConsolePlugin for MessagingProvider {
    fn register(self: Arc<Self>, reg: &mut ConsoleRegistry) {
        reg.add_listener(Box::new(MessageSend(self.bot.clone())));
    }
}

struct MessageSend(Bot);

#[async_trait]
impl WebListener for MessageSend {
    fn event(&self) -> &'static str {
        "message/send"
    }
    fn authority(&self) -> u8 {
        // 以机器人身份发消息权力大,仅限主人。
        5
    }
    async fn handle(&self, args: Value, _who: AuthUser) -> Result<Value, String> {
        let tt = args.get("target_type").and_then(|v| v.as_str()).ok_or("缺少 target_type")?;
        let tid = args.get("target_id").and_then(|v| v.as_i64()).ok_or("缺少 target_id")?;
        let peer = match tt {
            "group" => Peer::group(tid),
            "private" | "friend" => Peer::friend(tid),
            _ => return Err("未知目标类型".to_string()),
        };
        let text = args.get("text").and_then(|v| v.as_str()).unwrap_or("").trim();
        if text.is_empty() {
            return Err("消息内容为空".to_string());
        }

        let id = self.0.send(&peer, &[Segment::text(text)]).await.map_err(|e| e.to_string())?;
        tracing::warn!(target: "abot::web::audit", target_type = %tt, target_id = tid, "网页控制台发送消息");
        // onebot_id 优先(整数好回显),没有就退回 seq。
        let message_id = id.onebot_id.map(|n| n as i64).unwrap_or(id.seq);
        Ok(json!({ "ok": true, "message_id": message_id }))
    }
}

nagisa::inventory::submit! {
    ConsolePluginCtor(|cx: &ConsoleContext| -> Arc<dyn ConsolePlugin> {
        Arc::new(MessagingProvider { bot: cx.bot.clone() })
    })
}
