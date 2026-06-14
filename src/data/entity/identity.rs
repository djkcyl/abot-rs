//! `identity` 表实体 —— 一个 QQ 用户的**账号昵称缓存**(按 `uin` 一人一条),与经济 `user` 行解耦。
//!
//! 账号昵称(QQ 昵称)是「谁是谁」的全局事实,与是否注册、是否动过游戏币无关:凡发过消息的人都该
//! 能被叫出名字。故把它从核心 `user` 表(游戏币原子增量的热行,只为用过 bot 的人建)里拆出来,单放
//! 这张表——**每条消息 upsert、给所有发送者建行**(见 [`sync_identity`](crate::data::identity::sync_identity)),
//! 既不搅动游戏币热行,也让排行榜 / 网页聊天记录等「列出别人」的场景对潜水者也叫得出名。
//!
//! 与 [`member_card`](crate::data::entity::member_card) 分工:本表是**账号昵称**(per-uin、全局),
//! 名片表是**群名片**(per-(uin, gid)、随群不同)。按 `uin` 软关联核心 `user`(不建 FK)。

use sea_orm::entity::prelude::*;

/// `identity` 行模型(一个用户的账号昵称缓存)。
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "identity")]
pub struct Model {
    /// QQ 号(主键,**非**自增,软关联核心 `user`)。
    #[sea_orm(primary_key, auto_increment = false)]
    pub uin: i64,
    /// 账号昵称(QQ 昵称)。每条消息 upsert,**昵称变了才更新**(写放大考量,见 `data::identity`)。
    pub nickname: String,
    /// 最近一次同步时间(带时区)。库侧缺省 `now()`,每次 upsert 刷新。
    pub updated_at: DateTimeWithTimeZone,
}

/// `identity` 表无外联关系(经 `uin` 软关联,不建 FK 以免拖慢写入)。
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
