//! `sign_log` 表实体 —— **签到插件自有**的每日签到流水,签到数据的**单一真相**。
//!
//! 一天一行,复合主键 `(uin, day)` 天然去重;`day` 取业务日口径(凌晨 4 点边界);
//! `gold`/`exp` 记当日发放的奖励。只追加、不更新——去重 / 连签 / 累计 / 日历全部由
//! 本表**派生**(见 [`logic`](super::logic)),不另设汇总行(无冗余计数,与 chatlog
//! 发言数同款口径)。按 `uin` 软关联核心 `user`(不建 FK);建表迁移见
//! [`migration`](super::migration)。
//!
//! 触碰共享经济(发签到奖励)只走 `AUser::add_coin`——本表与 `user` 表互不耦合。

/// `sign_log` 表实体(每日签到一行)。
pub mod log {
    use sea_orm::entity::prelude::*;

    /// `sign_log` 行模型。
    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
    #[sea_orm(table_name = "sign_log")]
    pub struct Model {
        /// QQ 号(复合主键之一,软关联核心 `user`)。
        #[sea_orm(primary_key, auto_increment = false)]
        pub uin: i64,
        /// 签到日(复合主键之一,业务日口径)。
        #[sea_orm(primary_key, auto_increment = false)]
        pub day: Date,
        /// 当日发放的金币总数。
        pub gold: i64,
        /// 当日发放的经验。
        pub exp: i64,
        /// 落账时间(带时区)。库侧缺省 `now()`。
        pub at: DateTimeWithTimeZone,
    }

    /// `sign_log` 表无外联关系。
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}
