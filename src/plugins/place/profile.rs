//! 画板插件对「个人数据」的贡献 —— 一行「画板落格 N 格」,归 [`ProfileGroup::Game`] 战绩组。
//!
//! 与签到同款:画板自己 `submit!` 一个 [`ProfileProvider`],个人数据插件统一收集,不直接读画板表。

use nagisa::async_trait;
use sea_orm::DatabaseConnection;

use super::logic::placed_count;
use crate::data::profile::{ProfileGroup, ProfileProvider, ProfileSection};

/// 画板的个人数据贡献者(战绩组)。
struct PlaceProfile;

#[async_trait]
impl ProfileProvider for PlaceProfile {
    fn group(&self) -> ProfileGroup {
        ProfileGroup::Game
    }

    async fn line(&self, db: &DatabaseConnection, uin: i64) -> Option<String> {
        let n = placed_count(db, uin).await.ok()?;
        if n == 0 {
            return None; // 没画过 → 不占行
        }
        Some(format!("画板落格 {n} 格"))
    }
}

// 自注册:把画板的贡献者登记进进程级 inventory 集合。
nagisa::inventory::submit! {
    ProfileSection(|| Box::new(PlaceProfile))
}
