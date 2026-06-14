//! 身份缓存同步 —— 每条收到的消息把发送者的**账号昵称**与**群名片**刷进核心缓存,供「列出别人」
//! 的场景(全局 / 群内排行榜、网页聊天记录等)显示真名,而非只剩 QQ 号。
//!
//! nagisa 解码时已把发送者名合成进事件:群消息 `m.member`(账号昵称 `nickname` + 群名片 `card`)、
//! 私聊 `m.friend`(账号昵称 `nickname`)。本模块据此:
//! - **账号昵称** → upsert 进 [`identity`] 表(按 `uin` 一人一条),**给所有发送者建行**——这是
//!   全局事实,与是否注册、是否动过游戏币无关,故单放一张表、**不**碰核心 `user` 热行(那是游戏币
//!   原子增量的行,只为用过 bot 的人建);
//! - **群名片** → upsert 进 [`member_card`] 表,按 `(uin, gid)` 一格一条
//!   (同一个人在不同群名片可能不同)。
//!
//! 两者都按主键 upsert(`INSERT … ON CONFLICT DO UPDATE … WHERE 值有变`):行不存在则建、已存在则
//! **仅当值变化时**才写(`action_and_where`)。名字极少变,绝大多数消息撞键后零写入——不产生死元组、
//! 不压 autovacuum,十万级消息量下这点很关键;`updated_at` 随之 = 「最近一次变更」(全局榜取最近名片正
//! 用得上)。由 `chatlog` 的每条消息钩子调用(与记录并行);一切失败只记日志,绝不
//! 影响消息处理。

use nagisa::prelude::*;
use sea_orm::ActiveValue::{NotSet, Set};
use sea_orm::sea_query::{Expr, OnConflict};
use sea_orm::{DatabaseConnection, EntityTrait};

use crate::data::entity::{identity, member_card};

/// 同步一条消息发送者的身份缓存(账号昵称 + 群名片)。见模块文档。
pub async fn sync_identity(db: &DatabaseConnection, m: &MessageEvent) {
    let uin = m.sender.0;

    // 账号昵称:群取 member.nickname、私聊取 friend.nickname(**不是**群名片 card)。
    let account = m
        .member
        .as_ref()
        .map(|mi| mi.nickname.as_str())
        .or_else(|| m.friend.as_ref().map(|f| f.nickname.as_str()))
        .map(str::trim)
        .filter(|s| !s.is_empty());
    if let Some(nick) = account {
        upsert_nickname(db, uin, nick).await;
    }

    // 群名片:群消息才有(私聊无群上下文)。
    if m.peer.is_group()
        && let Some(card) = m.member.as_ref().map(|mi| mi.card.trim()).filter(|c| !c.is_empty())
    {
        upsert_card(db, uin, m.peer.id.0, card).await;
    }
}

/// upsert 账号昵称进 `identity[uin]`：行不存在则建(给所有发送者建行——潜水者也叫得出名),已存在则
/// **仅当昵称变了才更新**(`DO UPDATE … WHERE identity.nickname IS DISTINCT FROM excluded.nickname`)。
/// 改名罕见,绝大多数消息撞键后命中 `WHERE` 假、零写入,不产生死元组、不压 autovacuum——十万级消息量
/// 下的关键。全程不碰游戏币热行。
async fn upsert_nickname(db: &DatabaseConnection, uin: i64, nick: &str) {
    let row = identity::ActiveModel {
        uin: Set(uin),
        nickname: Set(nick.to_string()),
        updated_at: NotSet, // 库侧 now();变更时经 excluded 取同一缺省刷新
    };
    let r = identity::Entity::insert(row)
        .on_conflict(
            OnConflict::column(identity::Column::Uin)
                .update_columns([identity::Column::Nickname, identity::Column::UpdatedAt])
                .action_and_where(Expr::cust("identity.nickname IS DISTINCT FROM excluded.nickname"))
                .to_owned(),
        )
        .exec_without_returning(db)
        .await;
    if let Err(e) = r {
        tracing::warn!(uin, error = %e, "同步账号昵称失败");
    }
}

/// upsert 群名片进 `member_card[(uin, gid)]`：行不存在则建,已存在则**仅当名片变了才更新**
/// (`DO UPDATE … WHERE member_card.card IS DISTINCT FROM excluded.card`)。名片极少变,绝大多数群消息
/// 撞键后零写入(同 [`upsert_nickname`] 的写放大考量)。`updated_at` 随变更刷新,故 = 该群名片「最近一次
/// 变更」时刻,全局榜兜底取 `ORDER BY updated_at DESC` 的最近名片正好用得上。
async fn upsert_card(db: &DatabaseConnection, uin: i64, gid: i64, card: &str) {
    let row = member_card::ActiveModel {
        uin: Set(uin),
        gid: Set(gid),
        card: Set(card.to_string()),
        updated_at: NotSet, // 库侧 now();变更时经 excluded 取同一缺省刷新
    };
    let r = member_card::Entity::insert(row)
        .on_conflict(
            OnConflict::columns([member_card::Column::Uin, member_card::Column::Gid])
                .update_columns([member_card::Column::Card, member_card::Column::UpdatedAt])
                .action_and_where(Expr::cust("member_card.card IS DISTINCT FROM excluded.card"))
                .to_owned(),
        )
        .exec_without_returning(db)
        .await;
    if let Err(e) = r {
        tracing::warn!(uin, gid, error = %e, "同步群名片失败");
    }
}
