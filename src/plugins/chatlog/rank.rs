//! 消息记录插件对「排行榜」的贡献 —— 发言榜(全局发言数),经 [`RankSection`] 自注册。
//!
//! 发言数读**去规范化计数** `chat_stat.msg_count`(每条入站消息增量自加,见 [`super::record`]),
//! 而非每次全表聚合 `chat_log`——十万级用户量下榜查询才不至于每次扫几亿行日志,`top`/`rank_of` 都落在
//! `chat_stat` 的索引上、O(log n)。跨群累计;本群榜只把人员限制为本群成员,数值仍是全局发言数。
//!
//! 与游戏币/等级榜口径一致,榜上**只列建过号(有 `user` 行)的人**——`chat_stat` 含所有发言者(潜水者也
//! 计数),故按核心 `user` 表半连接过滤(见 [`registered_uins`])。排行榜插件**不**直接读本插件表,经本
//! 贡献者拿数据(与签到 / 个人数据同款墙)。

use std::collections::HashSet;

use nagisa::async_trait;
use sea_orm::sea_query::SelectStatement;
use sea_orm::{
    ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect, QueryTrait,
};

use crate::data::entity::user;
use crate::data::rank::{RankBoard, RankRow, RankSection};
use crate::plugins::chatlog::entity::chat_stat;

/// 发言榜贡献者。
struct ChatBoard;

/// 「只列建号用户」的半连接子查询:`SELECT uin FROM "user"`。`chat_stat` 含所有发言者(含未建号的纯
/// 潜水者),故榜上按核心 `user` 表过滤,只留建过号的人——与游戏币/等级榜口径一致(见
/// [`rank`](crate::plugins::rank) 模块文档)。
fn registered_uins() -> SelectStatement {
    user::Entity::find().select_only().column(user::Column::Uin).into_query()
}

#[async_trait]
impl RankBoard for ChatBoard {
    fn key(&self) -> &'static str {
        "chat"
    }
    fn title(&self) -> &'static str {
        "发言榜"
    }
    fn format_value(&self, value: i64) -> String {
        format!("{value} 条")
    }

    async fn top(&self, db: &DatabaseConnection, members: Option<&HashSet<i64>>, n: usize) -> Vec<RankRow> {
        let mut q = chat_stat::Entity::find()
            .select_only()
            .column(chat_stat::Column::Uin)
            .column(chat_stat::Column::MsgCount)
            .filter(chat_stat::Column::Uin.in_subquery(registered_uins()))
            .order_by_desc(chat_stat::Column::MsgCount)
            .limit(n as u64);
        if let Some(set) = members {
            q = q.filter(chat_stat::Column::Uin.is_in(set.iter().copied()));
        }
        match q.into_tuple::<(i64, i64)>().all(db).await {
            Ok(rows) => rows.into_iter().map(|(uin, value)| RankRow { uin, value }).collect(),
            Err(e) => {
                tracing::warn!(error = %e, "查询发言榜 top 失败");
                Vec::new()
            }
        }
    }

    async fn rank_of(&self, db: &DatabaseConnection, members: Option<&HashSet<i64>>, uin: i64) -> Option<(u32, i64)> {
        // 本群口径:不在本群成员里(罕见——发命令者通常是成员)就不上榜。
        if let Some(set) = members
            && !set.contains(&uin)
        {
            return None;
        }
        // 我的全局发言数(计数行直读)。没发过言 / 无计数行 → 不上榜。
        let mine = chat_stat::Entity::find_by_id(uin).one(db).await.ok()??.msg_count;
        if mine == 0 {
            return None;
        }
        // 严格比我多的建号用户人数 + 1 = 名次(本群口径再加群成员限制)。索引范围 COUNT,不拉行回应用层。
        let mut q = chat_stat::Entity::find()
            .filter(chat_stat::Column::MsgCount.gt(mine))
            .filter(chat_stat::Column::Uin.in_subquery(registered_uins()));
        if let Some(set) = members {
            q = q.filter(chat_stat::Column::Uin.is_in(set.iter().copied()));
        }
        let ahead = q.count(db).await.ok()?;
        Some((ahead as u32 + 1, mine))
    }
}

// 自注册:把发言榜贡献者登记进进程级 inventory 集合。
nagisa::inventory::submit! {
    RankSection(|| Box::new(ChatBoard))
}
