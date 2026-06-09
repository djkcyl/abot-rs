//! 消息记录插件**自有**的建表迁移 —— 创建 `chat_log` 表 + 索引，经 [`PluginMigration`] 自注册
//! 接入核心 [`Migrator`](crate::data::migration::Migrator)（与 `#[command]` 同款 inventory 机制，
//! 核心不感知本插件）。迁移名带日期序号前缀且晚于核心序号，排序稳定。

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

/// 这支迁移：建消息记录插件自有的 `chat_log` 表 + 常用索引。
pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20250103_000001_create_chat_log"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // chat_log：自增主键 id（BIGSERIAL），uin 必填，group_id/onebot_id 可空，seq 默认 0，
        // content 为 jsonb 默认 {}，time 默认 now()。无 FK（经 uin 软关联核心 user）。
        manager
            .create_table(
                Table::create()
                    .table(ChatLog::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(ChatLog::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(ChatLog::Uin).big_integer().not_null())
                    .col(ColumnDef::new(ChatLog::GroupId).big_integer().null())
                    .col(ColumnDef::new(ChatLog::OnebotId).big_integer().null())
                    .col(ColumnDef::new(ChatLog::Seq).big_integer().not_null().default(0))
                    .col(
                        ColumnDef::new(ChatLog::Content)
                            .json_binary()
                            .not_null()
                            .default(Expr::cust("'{}'::jsonb")),
                    )
                    .col(
                        ColumnDef::new(ChatLog::Time)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await?;

        // 索引：按发言人查（个人数据/排行）、按 onebot_id 查（防撤回回查）、按群+时间翻历史。
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
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(ChatLog::Table).if_exists().to_owned())
            .await?;
        Ok(())
    }
}

// 第二支迁移自注册：给 chat_log 加「出站标记 + 私聊对端」两列(双向会话历史)。
nagisa::inventory::submit! {
    PluginMigration(|| Box::new(AddDirection))
}

/// 这支迁移:给已建好的 `chat_log` 补两列 —— `from_self`(是否 bot 自己发的)、
/// `private_peer`(私聊对端 QQ 号,群消息为 NULL),并回填历史私聊行的对端 = 其发送者。
pub struct AddDirection;

impl MigrationName for AddDirection {
    fn name(&self) -> &str {
        "m20260609_000001_chat_log_add_direction"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for AddDirection {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // from_self：是否 bot 自己发出的,默认 false（历史行都是收到的）。
        manager
            .alter_table(
                Table::alter()
                    .table(ChatLog::Table)
                    .add_column(
                        ColumnDef::new(ChatLog::FromSelf)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .to_owned(),
            )
            .await?;
        // private_peer：私聊对端 QQ 号,可空（群消息为 NULL）。
        manager
            .alter_table(
                Table::alter()
                    .table(ChatLog::Table)
                    .add_column(ColumnDef::new(ChatLog::PrivatePeer).big_integer().null())
                    .to_owned(),
            )
            .await?;
        // 回填：历史私聊行（group_id IS NULL）的对端 = 其发送者 uin。
        manager
            .get_connection()
            .execute_unprepared("UPDATE chat_log SET private_peer = uin WHERE group_id IS NULL")
            .await?;
        // 按对端建索引（拉某人私聊历史）。
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
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .if_exists()
                    .name("idx_chat_log_private_peer")
                    .table(ChatLog::Table)
                    .to_owned(),
            )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(ChatLog::Table)
                    .drop_column(ChatLog::PrivatePeer)
                    .to_owned(),
            )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(ChatLog::Table)
                    .drop_column(ChatLog::FromSelf)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }
}
