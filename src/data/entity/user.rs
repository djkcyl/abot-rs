//! `user` 表实体 —— 一个 QQ 用户的**跨插件共享**持久状态（金币、经验、封禁等）。
//!
//! 主键是 QQ 号 `uin`（`i64`，OneBot 的 `Uin` 即 `i64` 内核）。这里**只**放真正
//! 跨插件共享的字段（`coin` / `exp` / `nickname` / `banned` / `join_time`）；任何
//! 插件私有的状态（如签到的 `last_sign`/`continue_sign`/`total_sign`）一律归各插件
//! **自己**的表（见 `plugins::sign`），不得泄进这张核心表。可变计数都带库侧缺省，
//! 新用户 `insert` 时无需逐个填。
//!
//! 这是「数据 API」的底座：`AUser` 句柄就包住一行 [`Model`] + 一份连接，方法
//! （`coin`/`add_coin` …）直接在其上跑——没有仓储/DAO 中间层。
//! 金币改动走 `col_expr(Column::Coin, Expr::col(Column::Coin).add(delta))` 原子增量，
//! 绝不 read-modify-write 写绝对值。

use sea_orm::entity::prelude::*;

/// `user` 行模型。字段顺序与列定义一致；缺省值见 [`migration`](crate::data::migration)。
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "user")]
pub struct Model {
    /// QQ 号（主键，**非**自增——由上游事件给定）。
    #[sea_orm(primary_key, auto_increment = false)]
    pub uin: i64,
    /// 金币余额。库侧缺省 `10`；改动一律走原子增量表达式，不读改写。
    pub coin: i64,
    /// 昵称缓存（可空——未必每个用户都记过名）。
    pub nickname: Option<String>,
    /// 经验值。库侧缺省 `0`。
    pub exp: i64,
    /// 是否封禁。库侧缺省 `false`。
    pub banned: bool,
    /// 首次入库时间（带时区）。库侧缺省 `now()`。
    pub join_time: DateTimeWithTimeZone,
}

/// `user` 表无外联关系（金币流水经 `uin` 软关联，不建 FK 以免约束拖慢写入）。
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
