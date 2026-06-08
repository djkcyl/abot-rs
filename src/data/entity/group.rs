//! `group` 表实体 —— 一个 QQ 群的持久状态（目前只有一个自由格式的 JSON 配置）。
//!
//! 主键是群号 `gid`（`i64`）。`config` 是 `jsonb`，库侧缺省 `{}`，给后续按群可调项
//! （开关、限额、白名单 …）留口子而不必频繁加列。
//!
//! 与 `user` 同理，`AGroup` 句柄会直接包住一行 [`Model`] + 连接，没有中间层。

use sea_orm::entity::prelude::*;

/// `group` 行模型。
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "group")]
pub struct Model {
    /// 群号（主键，非自增——由上游事件给定）。
    #[sea_orm(primary_key, auto_increment = false)]
    pub gid: i64,
    /// 自由格式群配置（`jsonb`，库侧缺省 `{}`）。
    pub config: Json,
    /// 首次入库时间（带时区）。库侧缺省 `now()`。
    pub created_at: DateTimeWithTimeZone,
}

/// `group` 表无外联关系。
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
