//! sea-orm 实体定义 —— 三张表的行模型，**只**描述形状，不含任何业务方法。
//!
//! 业务方法住在 `AUser` / `AGroup` 句柄上（包住这里的 `Model` + 一份连接），不在实体里。
//! 每个子模块按 sea-orm 惯例导出 `Entity` / `Model` / `ActiveModel` / `Column` …，
//! 用法 `use crate::data::entity::user;` 后 `user::Entity::find()…`。
//!
//! 表与列的物理定义（类型/缺省/主键）见 [`migration`](crate::data::migration)——
//! 实体的字段缺省**不**自己建表，建表权属迁移，二者须保持一致。

pub mod coin_log;
pub mod group;
pub mod user;
