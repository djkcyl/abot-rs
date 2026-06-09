//! `chat_log` 表实体 —— **消息记录插件自有**的逐条消息记录（与核心表分离，按 `uin` 软关联）。
//!
//! 每条收到的消息落一行：发送者 / 群（私聊为 `None`）/ 消息 id（OneBot `onebot_id` + 会话内
//! `seq`，供防撤回/回查锚点）/ 原始内容段（`content`，JSONB，**不**在此刻渲染成展示文本，保留
//! 结构、之后再渲）/ 时间。图片另由 `media` 模块下载落本地盘，不进本表。

use sea_orm::entity::prelude::*;

/// `chat_log` 行模型。`content` 是 `Json`（`serde_json::Value`），故**不**派生 `Eq`（浮点不满足）。
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "chat_log")]
pub struct Model {
    /// 自增主键（BIGSERIAL）。
    #[sea_orm(primary_key)]
    pub id: i64,
    /// 发送者 QQ 号。
    pub uin: i64,
    /// 群号；私聊为 `None`。
    pub group_id: Option<i64>,
    /// OneBot `message_id`（`onebot_id`，撤回/get_msg 的锚点）；缺失为 `None`。
    pub onebot_id: Option<i64>,
    /// 会话内序号（Milky 的 `seq` / OneBot 的 `message_seq`）；缺省 0。
    pub seq: i64,
    /// 原始消息内容（wire 段数组，JSONB，**未**渲染成展示文本——保留结构、之后再渲）。
    pub content: Json,
    /// 这条是不是 bot 自己发出去的（出站）。收到的为 false，bot 发的为 true。
    pub from_self: bool,
    /// 私聊对端 QQ 号（构成双向会话的「另一方」）：入站为发送者、出站为目标；群消息为 `None`。
    pub private_peer: Option<i64>,
    /// 入库时间（库侧 `now()`）。
    pub time: DateTimeWithTimeZone,
}

/// `chat_log` 表无外联关系（经 `uin` 软关联核心 `user`，不建 FK 以免拖慢写入）。
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
