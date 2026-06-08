//! `sign` 表实体 —— **签到插件自有**的每用户签到状态（与核心 `user` 表分离）。
//!
//! 这是「插件自有数据」约定的样板：签到的私有状态（上次签到日 / 连签 / 累计）不再泄进
//! 核心 `user` 表，而是归签到插件**自己**的 `sign` 表，按 `uin` 软关联（不建 FK）。
//! 建表迁移见 [`migration`](super::migration)（经 `PluginMigration` 自注册接入核心
//! `Migrator`）；连签结算逻辑见 [`logic`](super::logic)。
//!
//! 触碰共享经济（发签到奖励）只走 `AUser::add_coin`——本表与 `user` 表互不耦合。

use sea_orm::entity::prelude::*;

/// `sign` 行模型。字段顺序与列定义一致；缺省值见 [`migration`](super::migration)。
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "sign")]
pub struct Model {
    /// QQ 号（主键，**非**自增——与触发签到的用户 `uin` 同值）。
    #[sea_orm(primary_key, auto_increment = false)]
    pub uin: i64,
    /// 上次签到日期（`date`，可空——从未签到即 `None`）。
    pub last_sign: Option<Date>,
    /// 连续签到天数。库侧缺省 `0`。
    pub continue_sign: i32,
    /// 累计签到天数。库侧缺省 `0`。
    pub total_sign: i32,
}

/// `sign` 表无外联关系（经 `uin` 软关联核心 `user`，不建 FK）。
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
