//! 消息记录插件 —— 每条收到的消息都：记发言数 + 落库 + 归档图片。**自带数据 + 逻辑**。
//!
//! 「插件自有数据」约定的样板(与签到一致)：
//! - [`entity`] 定义本插件私有的 `chat_log` 表；[`migration`] 建表 + 索引，经 `PluginMigration`
//!   + `nagisa::inventory` **自注册**接入核心 `Migrator`(核心不感知本插件)；
//! - 图片归档走顶层媒体服务([`crate::media`]):本记录器是它的入口——每条消息里的图
//!   登记 + 排队下载,detached 跑、绝不阻塞;别的插件经 `media::wait` 等图就绪。
//!
//! 发言数**不**进核心 `user` 表：它只有一个写者(本记录器)、别处只读，与签到连签同形,故归本插件
//! ——直接 `COUNT(*)` 自 `chat_log` 派生(单一真相,无冗余计数、无每条双写,连没有 `user` 行的潜水者
//! 也算得出)，经 [`ProfileSection`](crate::data::profile::ProfileSection) 提供给「个人数据」
//! (见 `profile`)。记录用 `#[event(Message, top)]` —— 与命令并行、对每条消息都跑、最早触发。
//!
//! 文本渲染先不做：`content` 存**原始 wire 段数组**(JSONB、未渲染)，之后要展示再从结构渲。

pub mod entity;
pub mod migration;
mod profile;

use nagisa::prelude::*;
use sea_orm::{ActiveModelTrait, ActiveValue::NotSet, DatabaseConnection, Set};
use serde_json::Value;

use crate::data::Db;
use crate::plugins::chatlog::entity as chat_log;

// 消息记录是地基(排行/词云/防撤回都靠它)，故 can_disable=false：不应被群管关掉。
plugin! {
    key = "chatlog",
    name = "消息记录",
    category = Tool,
    can_disable = false,
    // 后台基础插件,不进用户菜单(对齐原 ABot-NT chat_log 的 hidden=True)。
    hidden = true,
    description = "记录聊天消息",
}

/// 每条收到的消息：落一行 `chat_log` + detached 归档图片(发言数由 `COUNT` 派生,见 [`profile`])。
///
/// `top` 使其与命令并行、对每条消息都跑、最早触发。bot 自己发的不记(只记收到的，与老 abot 一致)。
/// 落库失败只记日志、不影响其它处理；图片下载在独立任务里跑，绝不阻塞。
#[event(Message, top, id = "record")]
async fn record(m: MessageEvent, Db(db): Db) -> HandlerResult {
    if m.is_self {
        return Ok(()); // 只记收到的消息,不记 bot 自己发的
    }

    // 落一行消息记录。content = 原始 wire 段数组(未渲染)；群号私聊为 None；
    // 私聊对端 = 发送者（入站方向）；from_self 恒 false（上面已挡掉 bot 自己发的）。
    let content = m.raw.get("message").cloned().unwrap_or_else(|| Value::Array(Vec::new()));
    let group_id = m.peer.is_group().then_some(m.peer.id.0);
    let private_peer = (!m.peer.is_group()).then_some(m.sender.0);
    let row = chat_log::ActiveModel {
        id: NotSet,
        uin: Set(m.sender.0),
        group_id: Set(group_id),
        onebot_id: Set(m.id.onebot_id.map(|v| v as i64)),
        seq: Set(m.id.seq),
        content: Set(content),
        from_self: Set(false),
        private_peer: Set(private_peer),
        time: NotSet,
    };
    if let Err(e) = row.insert(&db).await {
        tracing::warn!(error = %e, "写消息记录失败");
    }

    // 图片归档：登记 + 排队交给顶层媒体服务,detached、绝不阻塞消息处理。
    let refs = crate::media::scan(&m.content);
    if !refs.is_empty() {
        tokio::spawn(crate::media::ingest(refs));
    }
    Ok(())
}

/// bot 自己发出的一条消息落 `chat_log`（出站方向，凑成双向会话历史）。由 `main` 装的出站
/// 日志器在 `bot.send` 成功后调用。`content` 用 OneBot wire 段数组（与入站同形,前端同款渲染）；
/// 私聊对端 = 发送目标；`from_self = true`、`uin = self_id`。落库失败只 warn。
pub async fn record_outgoing(
    db: &DatabaseConnection,
    peer: &Peer,
    segments: &[Segment],
    self_id: Uin,
    msg_id: &MessageId,
) {
    // 段 → OneBot wire 段数组（{type,data} 列表）：复用框架的 OneBot 编码器,与入站存的同形。
    let wire = nagisa::nagisa_onebot::encode_segments(segments);
    let content = serde_json::to_value(&wire).unwrap_or_else(|_| Value::Array(Vec::new()));
    let group_id = peer.is_group().then_some(peer.id.0);
    let private_peer = (!peer.is_group()).then_some(peer.id.0);
    let row = chat_log::ActiveModel {
        id: NotSet,
        uin: Set(self_id.0),
        group_id: Set(group_id),
        onebot_id: Set(msg_id.onebot_id.map(|v| v as i64)),
        seq: Set(msg_id.seq),
        content: Set(content),
        from_self: Set(true),
        private_peer: Set(private_peer),
        time: NotSet,
    };
    if let Err(e) = row.insert(db).await {
        tracing::warn!(error = %e, "写出站消息记录失败");
    }
}
