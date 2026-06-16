//! moderation 模块**自有**的建表迁移 —— 建 `content_moderation`(审核结果记录兼缓存),经
//! [`PluginMigration`] 自注册接入核心 [`Migrator`](crate::data::migration::Migrator)(核心不感知本模块)。

use sea_orm_migration::prelude::*;

use crate::data::migration::PluginMigration;

// 自注册:把建表迁移登记进进程级 `inventory` 集合。
nagisa::inventory::submit! {
    PluginMigration(|| Box::new(Migration))
}

/// `content_moderation` 表的列标识。
#[derive(DeriveIden)]
enum ContentModeration {
    Table,
    Kind,
    ContentKey,
    Content,
    Source,
    Suggestion,
    Label,
    SubLabel,
    Score,
    Details,
    RequestId,
    CreatedAt,
}

/// 这支迁移:建 `content_moderation` 表。复合主键 `(kind, content_key)` —— 图片键为媒体库 md5、
/// 文本键为文本 md5;`content` 仅文本行有(存原文)。
pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260614_000009_create_content_moderation"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(ContentModeration::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(ContentModeration::Kind).string().not_null())
                    .col(ColumnDef::new(ContentModeration::ContentKey).string().not_null())
                    .primary_key(Index::create().col(ContentModeration::Kind).col(ContentModeration::ContentKey))
                    .col(ColumnDef::new(ContentModeration::Content).text().null())
                    .col(ColumnDef::new(ContentModeration::Source).string().not_null())
                    .col(ColumnDef::new(ContentModeration::Suggestion).string().not_null())
                    .col(ColumnDef::new(ContentModeration::Label).string().not_null().default(""))
                    .col(ColumnDef::new(ContentModeration::SubLabel).string().not_null().default(""))
                    .col(ColumnDef::new(ContentModeration::Score).integer().not_null().default(0))
                    .col(
                        ColumnDef::new(ContentModeration::Details)
                            .json_binary()
                            .not_null()
                            .default(Expr::cust("'{}'::jsonb")),
                    )
                    .col(ColumnDef::new(ContentModeration::RequestId).string().not_null().default(""))
                    .col(
                        ColumnDef::new(ContentModeration::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table(Table::drop().table(ContentModeration::Table).if_exists().to_owned()).await?;
        Ok(())
    }
}
