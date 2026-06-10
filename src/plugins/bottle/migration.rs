//! 漂流瓶插件**自有**的建表迁移 —— 一次建三表（`bottle` / `bottle_score` /
//! `bottle_discuss`）+ 索引，经 [`PluginMigration`] 自注册接入核心
//! [`Migrator`](crate::data::migration::Migrator)（与 `#[command]` 同款 inventory 机制，
//! 核心不感知本插件）。迁移名取统一序列 `m20260610_0000NN`(见核心迁移说明)。

use sea_orm_migration::prelude::*;

use crate::data::migration::PluginMigration;

// 自注册：把本插件的建表迁移登记进进程级 `inventory` 集合，供核心 `Migrator` 收集。
nagisa::inventory::submit! {
    PluginMigration(|| Box::new(Migration))
}

/// `bottle` 表的列标识。`DeriveIden` 取枚举名 `Bottle` 的 snake_case 作表名 `bottle`。
#[derive(DeriveIden)]
enum Bottle {
    Table,
    Id,
    Uin,
    Nickname,
    GroupId,
    Text,
    Images,
    Anonymous,
    TotalPickups,
    RemainingPickups,
    Status,
    Moderation,
    Isdelete,
    CreatedAt,
}

/// `bottle_score` 表的列标识。枚举名须为 `BottleScore` 方映射到表名 `bottle_score`；
/// 列名天然以 `Bottle` 起头，故就地 `allow` 掉 `enum_variant_names`（重命名反会改了列名）。
#[derive(DeriveIden)]
#[allow(clippy::enum_variant_names)]
enum BottleScore {
    Table,
    Id,
    BottleId,
    Uin,
    Score,
    CreatedAt,
}

/// `bottle_discuss` 表的列标识。枚举名须为 `BottleDiscuss` 方映射到表名 `bottle_discuss`。
#[derive(DeriveIden)]
#[allow(clippy::enum_variant_names)]
enum BottleDiscuss {
    Table,
    Id,
    BottleId,
    Uin,
    Nickname,
    Text,
    CreatedAt,
}

/// 这支迁移：建漂流瓶插件自有的三张表 + 索引。
pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260610_000006_create_bottle"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // bottle：自增主键 id（BIGSERIAL，即用户看到的编号），uin 必填，nickname/group_id/
        // text 可空，images 为 jsonb 默认 [],anonymous/isdelete 默认 false,total_pickups
        // 默认 0,remaining_pickups 默认 -1（不限）,status 必填,moderation 可空 jsonb,
        // created_at 默认 now()。无 FK（经 uin 软关联核心 user）。
        manager
            .create_table(
                Table::create()
                    .table(Bottle::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Bottle::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Bottle::Uin).big_integer().not_null())
                    .col(ColumnDef::new(Bottle::Nickname).text().null())
                    .col(ColumnDef::new(Bottle::GroupId).big_integer().null())
                    .col(ColumnDef::new(Bottle::Text).text().null())
                    .col(
                        ColumnDef::new(Bottle::Images)
                            .json_binary()
                            .not_null()
                            .default(Expr::cust("'[]'::jsonb")),
                    )
                    .col(
                        ColumnDef::new(Bottle::Anonymous)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(
                        ColumnDef::new(Bottle::TotalPickups)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(Bottle::RemainingPickups)
                            .integer()
                            .not_null()
                            .default(-1),
                    )
                    .col(ColumnDef::new(Bottle::Status).text().not_null())
                    .col(ColumnDef::new(Bottle::Moderation).json_binary().null())
                    .col(
                        ColumnDef::new(Bottle::Isdelete)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(
                        ColumnDef::new(Bottle::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await?;

        // 索引：按状态筛可捞/待审、按投放者查自己的瓶子。
        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_bottle_status")
                    .table(Bottle::Table)
                    .col(Bottle::Status)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_bottle_uin")
                    .table(Bottle::Table)
                    .col(Bottle::Uin)
                    .to_owned(),
            )
            .await?;

        // bottle_score：自增主键 id，bottle_id/uin 必填,score SMALLINT 必填,
        // created_at 默认 now()。无 FK（经 bottle_id/uin 软关联）。
        manager
            .create_table(
                Table::create()
                    .table(BottleScore::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(BottleScore::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(BottleScore::BottleId).big_integer().not_null())
                    .col(ColumnDef::new(BottleScore::Uin).big_integer().not_null())
                    .col(ColumnDef::new(BottleScore::Score).small_integer().not_null())
                    .col(
                        ColumnDef::new(BottleScore::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await?;

        // 唯一约束 (bottle_id, uin)：一人一瓶一分，改分走 upsert。
        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .unique()
                    .name("uq_bottle_score_bottle_uin")
                    .table(BottleScore::Table)
                    .col(BottleScore::BottleId)
                    .col(BottleScore::Uin)
                    .to_owned(),
            )
            .await?;
        // 按瓶子查全部评分（算去极值均值）。
        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_bottle_score_bottle")
                    .table(BottleScore::Table)
                    .col(BottleScore::BottleId)
                    .to_owned(),
            )
            .await?;

        // bottle_discuss：自增主键 id，bottle_id/uin 必填,nickname 可空,text 必填,
        // created_at 默认 now()。无 FK。
        manager
            .create_table(
                Table::create()
                    .table(BottleDiscuss::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(BottleDiscuss::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(BottleDiscuss::BottleId).big_integer().not_null())
                    .col(ColumnDef::new(BottleDiscuss::Uin).big_integer().not_null())
                    .col(ColumnDef::new(BottleDiscuss::Nickname).text().null())
                    .col(ColumnDef::new(BottleDiscuss::Text).text().not_null())
                    .col(
                        ColumnDef::new(BottleDiscuss::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await?;

        // 按瓶子查全部评论（呈现/详情）。
        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_bottle_discuss_bottle")
                    .table(BottleDiscuss::Table)
                    .col(BottleDiscuss::BottleId)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // 逆序删表（无 FK，顺序仅为对称）。
        manager
            .drop_table(Table::drop().table(BottleDiscuss::Table).if_exists().to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(BottleScore::Table).if_exists().to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Bottle::Table).if_exists().to_owned())
            .await?;
        Ok(())
    }
}
