//! 消息记录插件**自有**的建表迁移 —— 创建 `chat_log` 表 + 索引，经 [`PluginMigration`] 自注册
//! 接入核心 [`Migrator`](crate::data::migration::Migrator)（与 `#[command]` 同款 inventory 机制，
//! 核心不感知本插件）。迁移名取统一序列 `m20260610_0000NN`(见核心迁移说明)。

use sea_orm_migration::prelude::*;

use crate::data::migration::PluginMigration;

// 自注册：把本插件的建表迁移登记进进程级 `inventory` 集合。
nagisa::inventory::submit! {
    PluginMigration(|| Box::new(Migration))
}

/// `chat_log` 表的列标识。
#[derive(DeriveIden)]
enum ChatLog {
    Table,
    Id,
    Uin,
    GroupId,
    OnebotId,
    Seq,
    Content,
    Time,
    FromSelf,
    PrivatePeer,
}

/// `chat_stat` 表的列标识(去规范化的每人发言计数)。
#[derive(DeriveIden)]
enum ChatStat {
    Table,
    Uin,
    MsgCount,
}

/// 这支迁移：建消息记录插件自有的 `chat_log` 表 + 常用索引。
pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260610_000003_create_chat_log"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // chat_log：自增主键 id（BIGSERIAL），uin 必填，group_id/onebot_id 可空，seq 默认 0，
        // content 为 jsonb 默认 {}，time 默认 now()。双向会话历史：from_self 标记 bot 出站,
        // private_peer 为私聊对端（入站=发送者、出站=目标;群消息 NULL）。无 FK（经 uin 软关联
        // 核心 user）。
        manager
            .create_table(
                Table::create()
                    .table(ChatLog::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(ChatLog::Id).big_integer().not_null().auto_increment().primary_key())
                    .col(ColumnDef::new(ChatLog::Uin).big_integer().not_null())
                    .col(ColumnDef::new(ChatLog::GroupId).big_integer().null())
                    .col(ColumnDef::new(ChatLog::OnebotId).big_integer().null())
                    .col(ColumnDef::new(ChatLog::Seq).big_integer().not_null().default(0))
                    .col(ColumnDef::new(ChatLog::Content).json_binary().not_null().default(Expr::cust("'{}'::jsonb")))
                    .col(ColumnDef::new(ChatLog::FromSelf).boolean().not_null().default(false))
                    .col(ColumnDef::new(ChatLog::PrivatePeer).big_integer().null())
                    .col(
                        ColumnDef::new(ChatLog::Time)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await?;

        // 索引：按发言人查（个人数据/排行）、按 onebot_id 查（防撤回回查）、按群+时间翻历史、
        // 按私聊对端拉会话。
        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_chat_log_uin")
                    .table(ChatLog::Table)
                    .col(ChatLog::Uin)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_chat_log_onebot_id")
                    .table(ChatLog::Table)
                    .col(ChatLog::OnebotId)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_chat_log_group_time")
                    .table(ChatLog::Table)
                    .col(ChatLog::GroupId)
                    .col(ChatLog::Time)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_chat_log_private_peer")
                    .table(ChatLog::Table)
                    .col(ChatLog::PrivatePeer)
                    .to_owned(),
            )
            .await?;

        // chat_stat:每人一行的去规范化发言计数(msg_count,主键 uin,库侧缺省 0)。发言榜读它取代每次
        // 全表聚合 chat_log;按 msg_count 建索引,供「ORDER BY msg_count DESC LIMIT」取顶与「比我多的人」
        // 范围计数。计数由 `record` 钩子增量自加。
        manager
            .create_table(
                Table::create()
                    .table(ChatStat::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(ChatStat::Uin).big_integer().not_null().primary_key())
                    .col(ColumnDef::new(ChatStat::MsgCount).big_integer().not_null().default(0))
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_chat_stat_count")
                    .table(ChatStat::Table)
                    .col(ChatStat::MsgCount)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table(Table::drop().table(ChatStat::Table).if_exists().to_owned()).await?;
        manager.drop_table(Table::drop().table(ChatLog::Table).if_exists().to_owned()).await?;
        Ok(())
    }
}
