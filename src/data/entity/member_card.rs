//! `member_card` 表实体 —— 一个用户在各群的**群名片缓存**(按 `(uin, gid)` 的「字典」)。
//!
//! 同一个人在不同群的群名片可能都不一样,故按 `(uin, gid)` 复合主键一格一条,而**不**塞进核心
//! `user` 表(那是游戏币原子增量的热行,不该被每条群消息的名片写入搅动)。每条群消息经
//! [`sync_identity`](crate::data::identity::sync_identity) upsert 一格(**名片变了才更新**,
//! `updated_at` = 最近一次变更)。排行榜等
//! 「列出别人」的场景按当前群取名片显示;也兜全局榜没有账号昵称时的名字。按 `uin`/`gid` 软关联
//! 核心 `user`/`group`(不建 FK)。

use sea_orm::entity::prelude::*;

/// `member_card` 行模型(一个用户在一个群的群名片)。
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "member_card")]
pub struct Model {
    /// QQ 号(复合主键之一,软关联核心 `user`)。
    #[sea_orm(primary_key, auto_increment = false)]
    pub uin: i64,
    /// 群号(复合主键之一,软关联核心 `group`)。
    #[sea_orm(primary_key, auto_increment = false)]
    pub gid: i64,
    /// 该用户在该群的群名片。
    pub card: String,
    /// 最近一次同步时间(带时区)。库侧缺省 `now()`,每次 upsert 刷新。
    pub updated_at: DateTimeWithTimeZone,
}

/// `member_card` 表无外联关系(经 `uin`/`gid` 软关联,不建 FK 以免拖慢写入)。
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
