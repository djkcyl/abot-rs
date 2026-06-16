//! mcping 插件**自有**实体 —— `mc_server`:群内保存的 MC 服务器清单(供批量 ping)。

/// `mc_server` —— 一台群内保存的服务器。
pub mod server {
    use sea_orm::entity::prelude::*;

    /// `mc_server` 行模型。
    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
    #[sea_orm(table_name = "mc_server")]
    pub struct Model {
        /// 自增主键(`BIGSERIAL`),`insert` 时由库生成。
        #[sea_orm(primary_key)]
        pub id: i64,
        /// 所属群号(本功能仅群内)。
        pub group_id: i64,
        /// 展示名(列表条目标题)。
        pub name: String,
        /// 连接地址(`host` / `host:port` / IP)。
        pub address: String,
        /// 添加者 QQ 号。
        pub added_by: i64,
        /// 添加时间(带时区)。库侧缺省 `now()`。
        pub at: DateTimeWithTimeZone,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}
