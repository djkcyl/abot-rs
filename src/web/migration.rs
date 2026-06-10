//! WebUI 自有建表迁移 —— `web_token`(登录 token)与 `setting`(DB 配置层),
//! 经 [`crate::data::migration::PluginMigration`] 自注册接入核心 `Migrator`
//! (与插件迁移同款机制)。

use crate::data::migration::PluginMigration;
use sea_orm_migration::prelude::*;

// 自注册:把本迁移登记进进程级 inventory 集合,供核心 Migrator 收集。
nagisa::inventory::submit! {
    PluginMigration(|| Box::new(Migration))
}

/// `web_token` 表列标识。
#[derive(DeriveIden)]
enum WebToken {
    Table,
    Token,
    Uin,
    Authority,
    CreatedAt,
    ExpiresAt,
}

/// `setting` 表列标识(DB 配置层:(plugin_key, key) → value jsonb)。
#[derive(DeriveIden)]
enum Setting {
    Table,
    PluginKey,
    Key,
    Value,
    UpdatedAt,
}

/// 这支迁移:建 `web_token` 与 `setting` 两张表。名取统一序列 `m20260610_0000NN`。
pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260610_000005_create_web"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // web_token:token 主键(随机串),uin i64,authority 小整数,created_at/expires_at 带时区。
        manager
            .create_table(
                Table::create()
                    .table(WebToken::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(WebToken::Token).string().not_null().primary_key())
                    .col(ColumnDef::new(WebToken::Uin).big_integer().not_null())
                    .col(ColumnDef::new(WebToken::Authority).small_integer().not_null())
                    .col(
                        ColumnDef::new(WebToken::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(ColumnDef::new(WebToken::ExpiresAt).timestamp_with_time_zone().not_null())
                    .to_owned(),
            )
            .await?;

        // setting:复合主键 (plugin_key, key),value 为 jsonb,updated_at 带时区。
        manager
            .create_table(
                Table::create()
                    .table(Setting::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Setting::PluginKey).string().not_null())
                    .col(ColumnDef::new(Setting::Key).string().not_null())
                    .col(ColumnDef::new(Setting::Value).json_binary().not_null())
                    .col(
                        ColumnDef::new(Setting::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .primary_key(Index::create().col(Setting::PluginKey).col(Setting::Key))
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Setting::Table).if_exists().to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(WebToken::Table).if_exists().to_owned())
            .await?;
        Ok(())
    }
}
