//! 消息记录插件对「个人数据」的贡献 —— 一行发言数，经 [`ProfileSection`] 自注册。
//!
//! 发言数读去规范化计数 `chat_stat.msg_count`(每条入站消息增量自加,见 [`super::record`])——与发言榜
//! 同源,故「个人数据」与榜上数值一致,且单人直读主键 O(1)。「个人数据」插件**不**直接读本表、也不感知本插件。

use nagisa::async_trait;
use sea_orm::{DatabaseConnection, EntityTrait};

use crate::data::profile::{ProfileProvider, ProfileSection};
use crate::plugins::chatlog::entity::chat_stat;

/// 消息记录的个人数据贡献者：发言条数 = 该用户的 `chat_stat.msg_count`。
struct ChatProfile;

#[async_trait]
impl ProfileProvider for ChatProfile {
    async fn line(&self, db: &DatabaseConnection, uin: i64) -> Option<String> {
        let n = chat_stat::Entity::find_by_id(uin).one(db).await.ok()??.msg_count;
        // 没发过言（无计数行 / 计数 0）→ 不占行。
        if n == 0 {
            return None;
        }
        Some(format!("发言 {n} 条"))
    }
}

// 自注册：把消息记录的贡献者登记进进程级 inventory 集合。
nagisa::inventory::submit! {
    ProfileSection(|| Box::new(ChatProfile))
}
