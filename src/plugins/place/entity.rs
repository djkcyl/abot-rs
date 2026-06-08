//! 画板插件自有的两张表实体。
//!
//! - [`pixel`]:`place_pixel` —— 画布**真值**,复合主键 `(x,y)`,每格一行,落格 upsert。渲染数据源。
//! - [`history`]:`place_history` —— **追加审计**,每次落格一行;派生冷却(`MAX(at)`)、币价 &
//!   战绩(`COUNT`)。
//!
//! 两个实体各放一个子模块,避免 `DeriveEntityModel` 生成的 `Entity`/`Model`/`Column` 同名相撞。
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
