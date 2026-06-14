//! 签到插件**自有**的实体 —— 流水 [`log`] 是每日签到一行、签到数据的**单一真相**;
//! [`stat`] 是去规范化的累计签到天数,签到榜用。
//!
//! `sign_log` 一天一行,复合主键 `(uin, day)` 天然去重;`day` 取业务日口径(凌晨 4 点边界);
//! `gold`/`exp` 记当日发放的奖励。只追加、不更新——去重 / 连签 / 累计 / 日历全部由本表**派生**
//! (见 [`logic`](super::logic))。`sign_stat` 是为签到榜在十万级用户量下免去每次聚合 `sign_log` 而维护的
//! 每人累计天数(签到时随之置为当前累计,权威值)。两表都按 `uin` 软关联核心 `user`(不建 FK);建表见
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
        /// 当日发放的游戏币总数。
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

/// `sign_stat` 行模型(每人一行的去规范化累计签到天数)。
pub mod stat {
    use sea_orm::entity::prelude::*;

    /// `sign_stat` 行模型(每个签到过的 uin 一行)。`day_count` = 该用户 `sign_log` 的行数(累计签到天数),
    /// 签到时随之置为当前累计(权威值、不漂移),供签到榜 O(1) 读、免每次按 `uin` 聚合 `sign_log`。签到必经
    /// `AUser`,故本表的人天然都建过号。
    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
    #[sea_orm(table_name = "sign_stat")]
    pub struct Model {
        /// QQ 号(主键,**非**自增,软关联核心 `user`)。
        #[sea_orm(primary_key, auto_increment = false)]
        pub uin: i64,
        /// 累计签到天数(`sign_log` 行数;签到时置为当前累计,权威维护)。
        pub day_count: i64,
    }

    /// `sign_stat` 表无外联关系。
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}
