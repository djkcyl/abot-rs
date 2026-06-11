//! 签到插件对「个人数据」的贡献 —— 经 [`ProfileSection`] 自注册一行连签/累计。
//!
//! 这样「个人数据」插件**不**直接读签到表(不破插件自有数据的墙):签到自己 `submit!` 一个
//! [`ProfileProvider`],个人数据经 [`collect_grouped`](crate::data::collect_grouped) 统一收集
//! (与 `PluginMigration` 同款机制)。连签 / 累计与签到结算同源——都从 `sign_log` 流水派生。

use nagisa::async_trait;
use sea_orm::DatabaseConnection;

use crate::data::profile::{ProfileProvider, ProfileSection};
use crate::plugins::sign::logic;

/// 签到的个人数据贡献者。
struct SignProfile;

#[async_trait]
impl ProfileProvider for SignProfile {
    async fn line(&self, db: &DatabaseConnection, uin: i64) -> Option<String> {
        let days = logic::days_desc(db, uin).await.ok()?;
        // 从未签到 → 不占行。
        if days.is_empty() {
            return None;
        }
        let streak = logic::live_streak(&days, crate::data::util::business_day());
        Some(format!("连签 {} 天，累计 {} 天", streak, days.len()))
    }
}

// 自注册:把签到的贡献者登记进进程级 inventory 集合。
nagisa::inventory::submit! {
    ProfileSection(|| Box::new(SignProfile))
}
