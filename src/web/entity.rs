//! `web_token` 表实体 —— 网页控制台的登录 token。
//! token 是主键(随机串),`authority` 是签发时按 master/superuser 解析的权限级,
//! `expires_at` 之后视为失效(查询时过滤)。

use sea_orm::entity::prelude::*;

/// `web_token` 行模型。列与 `src/web/migration.rs` 一致。
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "web_token")]
pub struct Model {
    /// 随机 token(主键,非自增)。
    #[sea_orm(primary_key, auto_increment = false)]
    pub token: String,
    /// 绑定的 QQ 号。
    pub uin: i64,
    /// 权限级(master=5 / superuser=4 / 其余登录用户=1)。
    pub authority: i16,
    /// 签发时间(带时区)。
    pub created_at: DateTimeWithTimeZone,
    /// 失效时刻(带时区)。
    pub expires_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

/// `setting` 表实体 —— DB 可写配置层。复合主键 (plugin_key, key)。
pub mod setting {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
    #[sea_orm(table_name = "setting")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub plugin_key: String,
        #[sea_orm(primary_key, auto_increment = false)]
        pub key: String,
        pub value: Json,
        pub updated_at: DateTimeWithTimeZone,
    }
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}
    impl ActiveModelBehavior for ActiveModel {}
}
