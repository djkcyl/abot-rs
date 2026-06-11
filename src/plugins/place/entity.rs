//! 画板插件自有的四张表实体。
//!
//! - [`pixel`]:`place_pixel` —— 画布**真值**,复合主键 `(x,y)`,每格一行,落格 upsert。渲染数据源。
//! - [`history`]:`place_history` —— **追加审计**,每次落格一行;派生冷却(`MAX(at)`)、币价 &
//!   战绩(`COUNT`)。
//! - [`snapshot`]:`place_snapshot` —— 画布**周期快照**,每若干笔存一份,窗口回放从这儿起步,
//!   不必从零重演。
//! - [`replay_cache`]:`place_replay_cache` —— 全量回放 GIF 的**当日缓存**,按业务日一行,
//!   旧日随写随清。
//!
//! 各实体一个子模块,避免 `DeriveEntityModel` 生成的 `Entity`/`Model`/`Column` 同名相撞。
//! 经 `uin` 软关联核心 `user`,不建 FK。建表见 [`migration`](super::migration)。

/// `place_pixel` —— 画布真值(每格一行,落格 upsert)。
pub mod pixel {
    use sea_orm::entity::prelude::*;

    /// `place_pixel` 行模型。复合主键 `(x,y)`。
    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
    #[sea_orm(table_name = "place_pixel")]
    pub struct Model {
        /// 列坐标 0–255(复合主键之一,非自增)。
        #[sea_orm(primary_key, auto_increment = false)]
        pub x: i32,
        /// 行坐标 0–143(复合主键之一,非自增)。
        #[sea_orm(primary_key, auto_increment = false)]
        pub y: i32,
        /// 调色板索引 1–32。
        pub color: i32,
        /// 最后落格者 QQ 号。
        pub uin: i64,
        /// 最后落格时间(带时区)。
        pub at: DateTimeWithTimeZone,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

/// `place_snapshot` —— 画布周期快照(每 [`SNAPSHOT_EVERY`](super::logic::SNAPSHOT_EVERY) 笔一份)。
pub mod snapshot {
    use sea_orm::entity::prelude::*;

    /// `place_snapshot` 行模型。
    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
    #[sea_orm(table_name = "place_snapshot")]
    pub struct Model {
        /// 水位:本快照包含 `place_history` 中 id ≤ 此值的全部落格(主键,非自增)。
        #[sea_orm(primary_key, auto_increment = false)]
        pub history_id: i64,
        /// 画布索引缓冲(W×H 字节、行优先,0=空;与渲染/回放的内存布局同构)。
        pub canvas: Vec<u8>,
        /// 快照时间(带时区)。
        pub at: DateTimeWithTimeZone,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

/// `place_replay_cache` —— 全量回放 GIF 的当日缓存(按业务日一行,旧日随写随清)。
pub mod replay_cache {
    use sea_orm::entity::prelude::*;

    /// `place_replay_cache` 行模型。
    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
    #[sea_orm(table_name = "place_replay_cache")]
    pub struct Model {
        /// 业务日(凌晨 4 点界,主键)。
        #[sea_orm(primary_key, auto_increment = false)]
        pub day: Date,
        /// 缓存的 GIF 字节。
        pub gif: Vec<u8>,
        /// 生成时间(带时区)。
        pub at: DateTimeWithTimeZone,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

/// `place_history` —— 追加审计(每次落格一行)。
pub mod history {
    use sea_orm::entity::prelude::*;

    /// `place_history` 行模型。
    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
    #[sea_orm(table_name = "place_history")]
    pub struct Model {
        /// 自增主键(`BIGSERIAL`),`insert` 时由库生成。
        #[sea_orm(primary_key)]
        pub id: i64,
        /// 落格者 QQ 号。
        pub uin: i64,
        /// 来源群(私聊为 `None`)。
        pub group_id: Option<i64>,
        /// 列坐标 0–255。
        pub x: i32,
        /// 行坐标 0–143。
        pub y: i32,
        /// 落格前该格颜色(无则 32=白)。
        pub old_color: i32,
        /// 落格后颜色(调色板索引 1–32)。
        pub new_color: i32,
        /// 落格时间(带时区)。库侧缺省 `now()`。
        pub at: DateTimeWithTimeZone,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}
