//! 画板插件**自有**的建表迁移 —— 建 `place_pixel`(画布真值)+ `place_history`(追加审计),
//! 经 [`PluginMigration`] 自注册接入核心 [`Migrator`](crate::data::migration::Migrator)。
//!
//! 迁移名取统一序列 `m20260610_0000NN`(见核心迁移说明)。

use sea_orm_migration::prelude::*;

use crate::data::migration::PluginMigration;

// 自注册:登记进进程级 `inventory` 集合,供核心 `Migrator` 收集(无需核心引用本插件)。
nagisa::inventory::submit! {
    PluginMigration(|| Box::new(Migration))
}

/// `place_pixel` 列标识。
#[derive(DeriveIden)]
enum PlacePixel {
    Table,
    X,
    Y,
    Color,
    Uin,
    At,
}

/// `place_history` 列标识。
#[derive(DeriveIden)]
enum PlaceHistory {
    Table,
    Id,
    Uin,
    GroupId,
    X,
    Y,
    OldColor,
    NewColor,
    At,
}

/// `place_snapshot` 列标识。
#[derive(DeriveIden)]
enum PlaceSnapshot {
    Table,
    HistoryId,
    Canvas,
    At,
}

/// `place_replay_cache` 列标识。
#[derive(DeriveIden)]
enum PlaceReplayCache {
    Table,
    Day,
    Gif,
    At,
}

/// 建画板两表的迁移。
pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260610_000004_create_place"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // place_pixel:复合主键 (x,y),color/uin 非空,at 默认 now()。落格 upsert。
        manager
            .create_table(
                Table::create()
                    .table(PlacePixel::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(PlacePixel::X).integer().not_null())
                    .col(ColumnDef::new(PlacePixel::Y).integer().not_null())
                    .col(ColumnDef::new(PlacePixel::Color).integer().not_null())
                    .col(ColumnDef::new(PlacePixel::Uin).big_integer().not_null())
                    .col(
                        ColumnDef::new(PlacePixel::At)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .primary_key(Index::create().col(PlacePixel::X).col(PlacePixel::Y))
                    .to_owned(),
            )
            .await?;

        // place_history:自增主键 id（BIGSERIAL），group_id 可空（私聊 null），at 默认 now()。
        manager
            .create_table(
                Table::create()
                    .table(PlaceHistory::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(PlaceHistory::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(PlaceHistory::Uin).big_integer().not_null())
                    .col(ColumnDef::new(PlaceHistory::GroupId).big_integer().null())
                    .col(ColumnDef::new(PlaceHistory::X).integer().not_null())
                    .col(ColumnDef::new(PlaceHistory::Y).integer().not_null())
                    .col(ColumnDef::new(PlaceHistory::OldColor).integer().not_null())
                    .col(ColumnDef::new(PlaceHistory::NewColor).integer().not_null())
                    .col(
                        ColumnDef::new(PlaceHistory::At)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await?;

        // (uin, at) 索引:供冷却 MAX(at)、币价 & 战绩 COUNT 的按人过滤。
        manager
            .create_index(
                Index::create()
                    .name("idx_place_history_uin_at")
                    .table(PlaceHistory::Table)
                    .col(PlaceHistory::Uin)
                    .col(PlaceHistory::At)
                    .to_owned(),
            )
            .await?;

        // (at) 独立索引:`recent_history` 的 `ORDER BY at DESC` 与回放按天数取窗的
        // `at >= since` 过滤都不带 uin,复合索引服务不了,append-only 表越长越退化成
        // 全表扫 + filesort。
        manager
            .create_index(
                Index::create()
                    .name("idx_place_history_at")
                    .table(PlaceHistory::Table)
                    .col(PlaceHistory::At)
                    .to_owned(),
            )
            .await?;

        // (x, y, at) 索引:`画板历史 x,y` 按格查(filter x,y + ORDER BY at DESC)与
        // 格子次数 COUNT 用;没有它每次都是全表扫,且这是普通命令就能触发的查询。
        manager
            .create_index(
                Index::create()
                    .name("idx_place_history_xy_at")
                    .table(PlaceHistory::Table)
                    .col(PlaceHistory::X)
                    .col(PlaceHistory::Y)
                    .col(PlaceHistory::At)
                    .to_owned(),
            )
            .await?;

        // place_snapshot:画布周期快照,主键 = history 水位 id,canvas 为 W×H 索引字节
        // (bytea,PG TOAST 自动压缩)。窗口回放从最近快照起步,不必从零重演。
        manager
            .create_table(
                Table::create()
                    .table(PlaceSnapshot::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(PlaceSnapshot::HistoryId)
                            .big_integer()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(PlaceSnapshot::Canvas).binary().not_null())
                    .col(
                        ColumnDef::new(PlaceSnapshot::At)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await?;

        // place_replay_cache:全量回放 GIF 的当日缓存,业务日为主键,旧日行随写随清,
        // 表内始终只有当日一行。
        manager
            .create_table(
                Table::create()
                    .table(PlaceReplayCache::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(PlaceReplayCache::Day).date().not_null().primary_key())
                    .col(ColumnDef::new(PlaceReplayCache::Gif).binary().not_null())
                    .col(
                        ColumnDef::new(PlaceReplayCache::At)
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
        manager
            .drop_table(Table::drop().table(PlaceReplayCache::Table).if_exists().to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(PlaceSnapshot::Table).if_exists().to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(PlaceHistory::Table).if_exists().to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(PlacePixel::Table).if_exists().to_owned())
            .await?;
        Ok(())
    }
}

