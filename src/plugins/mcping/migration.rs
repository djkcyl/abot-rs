//! mcping 插件**自有**建表迁移 —— 建 `mc_server`(群内保存的服务器清单),经 [`PluginMigration`]
//! 自注册接入核心 [`Migrator`](crate::data::migration::Migrator)。

use sea_orm_migration::prelude::*;

use crate::data::migration::PluginMigration;

nagisa::inventory::submit! {
    PluginMigration(|| Box::new(Migration))
}

/// `mc_server` 列标识。
#[derive(DeriveIden)]
enum McServer {
    Table,
    Id,
    GroupId,
    Name,
    Address,
    AddedBy,
    At,
}

/// 建 `mc_server` 表的迁移。
pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260614_000010_create_mc_server"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // mc_server:自增主键 id,group_id 非空(仅群内),name/address 非空,at 默认 now()。
        manager
            .create_table(
                Table::create()
                    .table(McServer::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(McServer::Id).big_integer().not_null().auto_increment().primary_key())
                    .col(ColumnDef::new(McServer::GroupId).big_integer().not_null())
                    .col(ColumnDef::new(McServer::Name).string().not_null())
                    .col(ColumnDef::new(McServer::Address).string().not_null())
                    .col(ColumnDef::new(McServer::AddedBy).big_integer().not_null())
                    .col(
                        ColumnDef::new(McServer::At)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await?;

        // (group_id, id) 索引:按群取清单并按添加序排列。
        manager
            .create_index(
                Index::create()
                    .name("idx_mc_server_group_id")
                    .table(McServer::Table)
                    .col(McServer::GroupId)
                    .col(McServer::Id)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table(Table::drop().table(McServer::Table).if_exists().to_owned()).await?;
        Ok(())
    }
}
