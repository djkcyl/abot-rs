//! 签到插件对「排行榜」的贡献 —— 签到榜(累计签到天数),经 [`RankSection`] 自注册。
//!
//! 累计签到天数读**去规范化计数** `sign_stat.day_count`(签到时置为当前累计,见 [`super::logic::do_sign`]),
//! 而非每次按 `uin` 聚合 `sign_log`——十万级用户量下榜查询落在 `sign_stat` 的索引上、O(log n)。按累计天数排
//! ——稳定;不按连签(连签是当前状态、要逐人派生且会清零,不适合做榜)。本群榜只把人员限制为本群成员,
//! 数值仍是全局累计。`sign_stat` 的人都签过到、签到必经 `AUser`,故天然只含建过号的人,无需像发言榜那样
//! 再按 `user` 表过滤。排行榜插件**不**直接读本表,经本贡献者拿数据(与个人数据同款墙)。

use std::collections::HashSet;

use nagisa::async_trait;
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect};

use crate::data::rank::{RankBoard, RankRow, RankSection};
use crate::plugins::sign::entity::stat;

/// 签到榜贡献者。
struct SignBoard;

#[async_trait]
impl RankBoard for SignBoard {
    fn key(&self) -> &'static str {
        "sign"
    }
    fn title(&self) -> &'static str {
        "签到榜"
    }
    fn format_value(&self, value: i64) -> String {
        format!("{value} 天")
    }

    async fn top(&self, db: &DatabaseConnection, members: Option<&HashSet<i64>>, n: usize) -> Vec<RankRow> {
        let mut q = stat::Entity::find()
            .select_only()
            .column(stat::Column::Uin)
            .column(stat::Column::DayCount)
            .order_by_desc(stat::Column::DayCount)
            .limit(n as u64);
        if let Some(set) = members {
            q = q.filter(stat::Column::Uin.is_in(set.iter().copied()));
        }
        match q.into_tuple::<(i64, i64)>().all(db).await {
            Ok(rows) => rows.into_iter().map(|(uin, value)| RankRow { uin, value }).collect(),
            Err(e) => {
                tracing::warn!(error = %e, "查询签到榜 top 失败");
                Vec::new()
            }
        }
    }

    async fn rank_of(&self, db: &DatabaseConnection, members: Option<&HashSet<i64>>, uin: i64) -> Option<(u32, i64)> {
        if let Some(set) = members
            && !set.contains(&uin)
        {
            return None;
        }
        // 我的累计签到天数(计数行直读)。从没签过 / 无计数行 → 不上榜。
        let mine = stat::Entity::find_by_id(uin).one(db).await.ok()??.day_count;
        if mine == 0 {
            return None;
        }
        // 严格比我多的人数 + 1 = 名次(本群口径再加群成员限制)。索引范围 COUNT,不拉行回应用层。
        let mut q = stat::Entity::find().filter(stat::Column::DayCount.gt(mine));
        if let Some(set) = members {
            q = q.filter(stat::Column::Uin.is_in(set.iter().copied()));
        }
        let ahead = q.count(db).await.ok()?;
        Some((ahead as u32 + 1, mine))
    }
}

// 自注册:把签到榜贡献者登记进进程级 inventory 集合。
nagisa::inventory::submit! {
    RankSection(|| Box::new(SignBoard))
}
