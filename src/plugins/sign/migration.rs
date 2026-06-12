//! 签到插件**自有**的建表迁移 —— 创建 `sign_log`(每日签到一行,签到数据的单一真相),
//! 并经 [`PluginMigration`] 自注册接入核心 [`Migrator`](crate::data::migration::Migrator)。
//!
//! 核心 `Migrator` 经 `nagisa::inventory` 收集所有 `PluginMigration`(与 `#[command]` 同
//! 款机制),故核心代码**不**引用本插件即可应用这支迁移。迁移名取统一序列
//! `m20260610_0000NN`(见核心迁移说明),序号晚于核心:核心先建共享表、插件后建自有表。

use sea_orm_migration::prelude::*;

use crate::data::migration::PluginMigration;

// 自注册:把本插件的建表迁移登记进进程级 `inventory` 集合,供核心 `Migrator` 收集。
nagisa::inventory::submit! {
    PluginMigration(|| Box::new(Migration))
}

/// `sign_log` 表的列标识。
#[derive(DeriveIden)]
enum SignLog {
    Table,
    Uin,
    Day,
    Gold,
    Exp,
    At,
}

/// 这支迁移:建签到插件自有的 `sign_log` 表。迁移名带日期序号前缀(晚于核心序号),记进
/// `seaql_migrations` 作为已应用标记。
pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260610_000002_create_sign"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // sign_log:每日签到一行,签到的**唯一**持久状态——去重 / 连签 / 累计 / 日历
        // 全由它派生,不设汇总行。复合主键 (uin, day) 天然去重 + 按人查询走主键索引;
        // gold/exp 记当日奖励,at 默认 now()。day 取业务日口径(凌晨 4 点边界)。
        manager
            .create_table(
                Table::create()
                    .table(SignLog::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(SignLog::Uin).big_integer().not_null())
                    .col(ColumnDef::new(SignLog::Day).date().not_null())
                    .col(ColumnDef::new(SignLog::Gold).big_integer().not_null())
                    .col(ColumnDef::new(SignLog::Exp).big_integer().not_null())
                    .col(
                        ColumnDef::new(SignLog::At)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .primary_key(Index::create().col(SignLog::Uin).col(SignLog::Day))
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table(Table::drop().table(SignLog::Table).if_exists().to_owned()).await?;
        Ok(())
    }
}
