//! 签到插件**自有**的建表迁移 —— 创建 `sign` 表，并经 [`PluginMigration`] 自注册
//! 接入核心 [`Migrator`](crate::data::migration::Migrator)。
//!
//! 核心 `Migrator` 经 `nagisa::inventory` 收集所有 `PluginMigration`（与 `#[command]` 同
//! 款机制），故核心代码**不**引用本插件即可应用这支迁移。迁移名取统一序列
//! `m20260610_0000NN`(见核心迁移说明),序号晚于核心:核心先建共享表、插件后建自有表。

use sea_orm_migration::prelude::*;

use crate::data::migration::PluginMigration;

// 自注册：把本插件的建表迁移登记进进程级 `inventory` 集合，供核心 `Migrator` 收集。
nagisa::inventory::submit! {
    PluginMigration(|| Box::new(Migration))
}

/// `sign` 表的列标识。`DeriveIden` 取枚举名 `Sign` 的 snake_case 作表名 `sign`，各变体
/// 作列名。多个列名天然以 `Sign` 结尾（`LastSign`/`ContinueSign`/`TotalSign`），故就地
/// `allow` 掉 `enum_variant_names`——这是 sea-orm iden 的惯用形，重命名反会改了列名。
#[derive(DeriveIden)]
#[allow(clippy::enum_variant_names)]
enum Sign {
    Table,
    Uin,
    LastSign,
    ContinueSign,
    TotalSign,
}

/// 这支迁移：建签到插件自有的 `sign` 表。迁移名带日期序号前缀（晚于核心序号），记进
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
        // sign：主键 uin（非自增，与触发签到的用户 uin 同值），last_sign 可空，
        // continue_sign / total_sign 默认 0。无 FK（经 uin 软关联核心 user）。
        manager
            .create_table(
                Table::create()
                    .table(Sign::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Sign::Uin)
                            .big_integer()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Sign::LastSign).date().null())
                    .col(
                        ColumnDef::new(Sign::ContinueSign)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(Sign::TotalSign)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Sign::Table).if_exists().to_owned())
            .await?;
        Ok(())
    }
}
