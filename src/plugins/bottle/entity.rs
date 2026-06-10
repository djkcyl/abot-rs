//! 漂流瓶插件**自有**的四张表实体 —— `bottle`（瓶子）/ `bottle_score`（评分）/
//! `bottle_discuss`（评论）/ `bottle_sent`（发出的转发消息 → 瓶子映射）。按 `uin`、
//! `bottle_id` 软关联（不建 FK），建表迁移见 [`migration`](super::migration)
//! （经 `PluginMigration` 自注册接入核心 `Migrator`）。
//!
//! 一表一子模块，各自 `Model` / `Relation` / `ActiveModel`，引用走
//! `entity::bottle::Model` 等。

/// `bottle` 行模型 —— 一只瓶子的全部状态。
pub mod bottle {
    use sea_orm::entity::prelude::*;

    /// `bottle` 行模型。`images` / `moderation` 是 `Json`（`serde_json::Value`），含浮点
    /// 不满足 `Eq`，故只派生 `PartialEq`（与 chatlog 同款）。
    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "bottle")]
    pub struct Model {
        /// 自增主键（BIGSERIAL），即用户看到的「编号」。
        #[sea_orm(primary_key)]
        pub id: i64,
        /// 投放者 QQ 号。
        pub uin: i64,
        /// 投放者显示名（投放时从事件抓，捞取/审核展示用）；缺失为 `None`。
        pub nickname: Option<String>,
        /// 来源会话群号；私聊为 `None`。
        pub group_id: Option<i64>,
        /// 文本内容；可空（纯图片瓶子为 `None`）。
        pub text: Option<String>,
        /// 已落盘的图片文件名数组（JSONB，库侧默认 `[]`）。
        pub images: Json,
        /// 是否匿名投放。库侧默认 `false`。
        pub anonymous: bool,
        /// 累计被捞次数。库侧默认 `0`。
        pub total_pickups: i32,
        /// 剩余可捞次数（-1 不限，0 用尽不再出现）。库侧默认 `-1`。
        pub remaining_pickups: i32,
        /// 审核状态：`pending`/`ai_approved`/`approved`/`rejected`。
        pub status: String,
        /// 审核命中详情（label/sub_label/source，JSONB），给审核员看；未命中为 `None`。
        pub moderation: Option<Json>,
        /// 软删标记。库侧默认 `false`。
        pub isdelete: bool,
        /// 投放时间（库侧 `now()`）。
        pub created_at: DateTimeWithTimeZone,
    }

    /// 无外联关系（经 `uin` 软关联核心 `user`，不建 FK）。
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

/// `bottle_score` 行模型 —— 一条评分（一人一瓶一分，改分走 upsert）。
pub mod score {
    use sea_orm::entity::prelude::*;

    /// `bottle_score` 行模型。
    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
    #[sea_orm(table_name = "bottle_score")]
    pub struct Model {
        /// 自增主键（BIGSERIAL）。
        #[sea_orm(primary_key)]
        pub id: i64,
        /// 所评瓶子的编号（软关联 `bottle.id`）。
        pub bottle_id: i64,
        /// 评分者 QQ 号。
        pub uin: i64,
        /// 分值（1..=5，SMALLINT）。
        pub score: i16,
        /// 评分时间（库侧 `now()`）。
        pub created_at: DateTimeWithTimeZone,
    }

    /// 无外联关系（经 `bottle_id` / `uin` 软关联，不建 FK）。
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

/// `bottle_sent` 行模型 —— 一条「发出的瓶子转发消息 → 瓶子」映射（「取原文」按回复目标反查）。
pub mod sent {
    use sea_orm::entity::prelude::*;

    /// `bottle_sent` 行模型。
    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
    #[sea_orm(table_name = "bottle_sent")]
    pub struct Model {
        /// 协议规整后的消息键（OneBot 取 onebot_id、Milky 取 seq，均含会话寻址），见命令层 `msg_key`。
        #[sea_orm(primary_key, auto_increment = false)]
        pub msg_key: String,
        /// 对应瓶子编号（软关联 `bottle.id`）。
        pub bottle_id: i64,
        /// 记录时间（库侧 `now()`），懒清理的界。
        pub created_at: DateTimeWithTimeZone,
    }

    /// 无外联关系（经 `bottle_id` 软关联，不建 FK）。
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

/// `bottle_discuss` 行模型 —— 一条评论。
pub mod discuss {
    use sea_orm::entity::prelude::*;

    /// `bottle_discuss` 行模型。
    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
    #[sea_orm(table_name = "bottle_discuss")]
    pub struct Model {
        /// 自增主键（BIGSERIAL）。
        #[sea_orm(primary_key)]
        pub id: i64,
        /// 所评瓶子的编号（软关联 `bottle.id`）。
        pub bottle_id: i64,
        /// 评论者 QQ 号。
        pub uin: i64,
        /// 评论者显示名；缺失为 `None`。
        pub nickname: Option<String>,
        /// 评论内容。
        pub text: String,
        /// 评论时间（库侧 `now()`）。
        pub created_at: DateTimeWithTimeZone,
    }

    /// 无外联关系（经 `bottle_id` / `uin` 软关联，不建 FK）。
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}
