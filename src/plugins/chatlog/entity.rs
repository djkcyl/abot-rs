//! 消息记录插件**自有**的实体 —— 逐条消息记录 [`chat_log`] + 去规范化的发言计数 [`chat_stat`]
//! (都与核心表分离,按 `uin` 软关联)。
//!
//! `chat_log` 每条收到的消息落一行(单一真相、双向会话历史);`chat_stat` 是为排行榜在十万级用户量下
//! 不必每次全表聚合 `chat_log` 而维护的**每人一行计数器**(随消息增量自加,见
//! [`crate::data::identity`] 同款写放大权衡;建表见 [`migration`](super::migration))。

/// `chat_log` 行模型 —— 逐条消息记录。
pub mod chat_log {
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
}

/// `chat_stat` 行模型 —— 每个发言者一行的去规范化发言计数。
pub mod chat_stat {
    use sea_orm::entity::prelude::*;

    /// `chat_stat` 行模型(每个发言过的 uin 一行)。`msg_count` = 该人的入站发言数,随每条收到的消息
    /// 增量自加(`record` 钩子里 upsert +1),供排行榜 O(1) 读,免去每次全表聚合 `chat_log`。
    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
    #[sea_orm(table_name = "chat_stat")]
    pub struct Model {
        /// QQ 号(主键,**非**自增,软关联核心 `user`)。
        #[sea_orm(primary_key, auto_increment = false)]
        pub uin: i64,
        /// 入站发言数(`from_self = false` 的 `chat_log` 行数,增量维护)。
        pub msg_count: i64,
    }

    /// `chat_stat` 表无外联关系。
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}
