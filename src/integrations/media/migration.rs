//! 媒体服务**自有**的建表迁移 —— 创建 `media_file` 登记表,经 [`PluginMigration`] 自注册
//! 接入核心 [`Migrator`](crate::data::migration::Migrator)(核心不感知本模块)。

use sea_orm_migration::prelude::*;

use crate::data::migration::PluginMigration;

// 自注册:把本模块的建表迁移登记进进程级 `inventory` 集合。
nagisa::inventory::submit! {
    PluginMigration(|| Box::new(Migration))
}
nagisa::inventory::submit! {
    PluginMigration(|| Box::new(MigrationFilename))
}

/// `media_file` 表的列标识。
#[derive(DeriveIden)]
enum MediaFile {
    Table,
    Md5,
    Url,
    Status,
    Error,
    Size,
    ClaimedExt,
    Format,
    Animated,
    SeenCount,
    CreatedAt,
    LastSeen,
    LastUsed,
    DoneAt,
    Filename,
}

/// 这支迁移:建顶层媒体服务的 `media_file` 登记表 + 状态索引。
pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260610_000007_create_media_file"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // media_file:内容 md5 当主键(同图一行,即无后缀落盘文件名),url/error 用 text
        // (URL 可超 255)。后缀/格式是元数据;遇见/使用三件套供统计与将来按冷热清理。
        manager
            .create_table(
                Table::create()
                    .table(MediaFile::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(MediaFile::Md5).string().not_null().primary_key())
                    .col(ColumnDef::new(MediaFile::Url).text().not_null())
                    .col(ColumnDef::new(MediaFile::Status).string().not_null().default("pending"))
                    .col(ColumnDef::new(MediaFile::Error).text().null())
                    .col(ColumnDef::new(MediaFile::Size).big_integer().null())
                    .col(ColumnDef::new(MediaFile::ClaimedExt).string().null())
                    .col(ColumnDef::new(MediaFile::Format).string().null())
                    .col(ColumnDef::new(MediaFile::Animated).boolean().null())
                    .col(ColumnDef::new(MediaFile::SeenCount).big_integer().not_null().default(1))
                    .col(
                        ColumnDef::new(MediaFile::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(MediaFile::LastSeen)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(ColumnDef::new(MediaFile::LastUsed).timestamp_with_time_zone().null())
                    .col(ColumnDef::new(MediaFile::DoneAt).timestamp_with_time_zone().null())
                    .to_owned(),
            )
            .await?;

        // 状态索引:启动恢复(捞 pending 重排队)与排错(看 failed)用。
        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_media_file_status")
                    .table(MediaFile::Table)
                    .col(MediaFile::Status)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table(Table::drop().table(MediaFile::Table).if_exists().to_owned()).await?;
        Ok(())
    }
}

/// 这支迁移:给 `media_file` 补 `filename` 列(+ 非唯一索引)——下载时记下上游 wire 文件名的 md5 主体。
/// 多数图它即主键 `md5`;少数被服务器转码的图(动画表情等)wire 名 md5 与内容 md5 不同,这一列就是同名图
/// 认到本行、免重下的线索。无名来源为 `NULL`。
pub struct MigrationFilename;

impl MigrationName for MigrationFilename {
    fn name(&self) -> &str {
        "m20260614_000008_media_file_filename"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for MigrationFilename {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(MediaFile::Table)
                    .add_column_if_not_exists(ColumnDef::new(MediaFile::Filename).string().null())
                    .to_owned(),
            )
            .await?;
        // 按 wire 名 stem 反查真 md5 用(名实不符的转码图);非唯一(同内容偶可多名)。
        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_media_file_filename")
                    .table(MediaFile::Table)
                    .col(MediaFile::Filename)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.alter_table(Table::alter().table(MediaFile::Table).drop_column(MediaFile::Filename).to_owned()).await?;
        Ok(())
    }
}
