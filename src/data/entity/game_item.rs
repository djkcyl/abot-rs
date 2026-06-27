//! `game_item` 表实体 —— **跨游戏共享**的玩家物品背包(一人一物一行)。
//!
//! 这是 abot 游戏生态的公共物品系统:赛马的比赛道具、未来钓鱼/种菜的渔获/作物/养成素材
//! 都堆在这一张表里,各游戏在自己的 `item_id` 号段里产出与消耗(命名空间靠号段隔开,见各插件
//! 的物品基址常量)。增减一律带闸、原子 upsert,与 `coin_log` 同属核心**共享**设施,故住在
//! `crate::data` 而非任何插件下。业务 API 见 [`inventory`](crate::data::inventory)。

use sea_orm::entity::prelude::*;

/// `game_item` 行模型。复合主键 `(uin, item_id)`,`qty` 原子增减、带闸扣。
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "game_item")]
pub struct Model {
    /// 主人 QQ 号(软关联 `user.uin`,不建 FK)。
    #[sea_orm(primary_key, auto_increment = false)]
    pub uin: i64,
    /// 物品全局编号(各游戏按号段划分,跨插件唯一)。
    #[sea_orm(primary_key, auto_increment = false)]
    pub item_id: i32,
    /// 持有数量。
    pub qty: i32,
}

/// `game_item` 表无外联关系(经 `uin` 软关联 `user`)。
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
