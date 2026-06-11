//! `coin_log` 表实体 —— 金币变动的追加式流水（审计/回溯用）。
//!
//! 每次 `add_coin` / 批量结算落账都顺手追一行：谁（`uin`）、变了多少（`delta`，
//! 带符号）、变完剩多少（`balance`，与扣加同一条原子 UPDATE 的 `RETURNING` 取回）、
//! 为什么（`reason`）、何时（`at`）。`id` 是 `BIGSERIAL` 自增主键，写入时不填、由库
//! 生成。这张表只追加、不更新——它是金币原子增量的「为什么 + 当时余额」侧记录。

use sea_orm::entity::prelude::*;

/// `coin_log` 行模型。
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "coin_log")]
pub struct Model {
    /// 自增主键（`BIGSERIAL`）。`insert` 时由库生成，应用侧不填。
    #[sea_orm(primary_key)]
    pub id: i64,
    /// 变动归属的 QQ 号（软关联 `user.uin`，不建 FK）。
    pub uin: i64,
    /// 带符号的金币增量（正为加、负为扣）。
    pub delta: i64,
    /// 本笔落账后的余额（与 `delta` 严格对应同一次原子更新）。
    pub balance: i64,
    /// 变动原因（人类可读，如 `"sign"` / `"gift"`）。
    pub reason: String,
    /// 落账时间（带时区）。库侧缺省 `now()`。
    pub at: DateTimeWithTimeZone,
}

/// `coin_log` 表无外联关系（经 `uin` 软关联到 `user`）。
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
