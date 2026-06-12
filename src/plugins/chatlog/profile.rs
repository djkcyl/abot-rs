//! 消息记录插件对「个人数据」的贡献 —— 一行发言数，经 [`ProfileSection`] 自注册。
//!
//! 发言数直接 `COUNT(*)` 自本插件自有的 `chat_log` 表派生(单一真相,无冗余计数)，与签到经
//! `ProfileSection` 提供连签同形：「个人数据」插件**不**直接读本表、也不感知本插件。

use nagisa::async_trait;
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter};

use crate::data::profile::{ProfileProvider, ProfileSection};
use crate::plugins::chatlog::entity as chat_log;

/// 消息记录的个人数据贡献者：发言条数 = 该用户在 `chat_log` 的行数。
struct ChatProfile;

#[async_trait]
impl ProfileProvider for ChatProfile {
    async fn line(&self, db: &DatabaseConnection, uin: i64) -> Option<String> {
        let n = chat_log::Entity::find().filter(chat_log::Column::Uin.eq(uin)).count(db).await.ok()?;
        // 没记录过 → 不占行。
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
