//! 数据层 —— sea-orm 之上的薄句柄，**不是**仓储/DAO/Service 抽象。
//!
//! 这里只放一个提取器 [`Db`]：把 `main` 经 `App::data(db)` 注入的
//! [`sea_orm::DatabaseConnection`] 从每事件上下文里克隆出来交给 handler。
//! `DatabaseConnection` 内部即 `Arc`（克隆廉价），故 handler 取 `Db` 等于拿到一份
//! 共享连接句柄，可直接 `entity::find()` / 起事务。
//!
//! 真正的「数据 API」是 `AUser` / `AGroup` 句柄 + 其上的**共享**方法
//! （`get`/`coin`/`add_coin` …），它们**直接**包住 `Model + 连接`，没有任何中间层包装。
//! 插件私有的状态与逻辑归各插件自己（见 `crate::plugins`），只经 `AUser::add_coin`
//! 触碰共享经济。下面的 `mod` 声明把这些句柄挂在 `crate::data::` 下。

// 实体定义（三张表的行模型）与建表迁移：
pub mod entity;
pub mod migration;

// 数据 API：用户/群句柄。句柄本身即 API，无仓储/DAO 中间层。
pub mod group;
pub mod user;

// 与具体表无关的通用助手（如 get-or-create 的 `get_or_insert`）。
pub mod util;

// 经验/等级的共享数学（纯函数 + 值对象）：经验是跨插件共享的用户属性，故等级公式归核心。
pub mod level;

// 「个人数据」的插件自注册贡献槽（与 PluginMigration 同款 inventory 机制）。
pub mod profile;

// 常用句柄直接在 `crate::data::` 下可达（`use crate::data::{AUser, AGroup, Db, ..}`）。
pub use group::AGroup;
pub use level::{LevelChange, LevelInfo, level_info, level_of};
pub use profile::{GroupedProfile, ProfileGroup, ProfileProvider, ProfileSection, collect_grouped};
pub use user::AUser;

use nagisa::prelude::*;
use sea_orm::DatabaseConnection;

/// 数据库连接句柄提取器：克隆出 `main` 注入的共享 [`DatabaseConnection`]。
///
/// `App::data(db)` 把连接以 `Arc<DatabaseConnection>` 存进 router 状态表；本提取器
/// 经 `State<DatabaseConnection>` 取出后**克隆内层连接**（其内部是 `Arc`，廉价）交还
/// handler——故 handler 拿到的是 `DatabaseConnection` 值本身（可移动进事务/查询），
/// 而非又一层 `Arc`。缺失注入时沿用 `State` 的 `Reject::Error` 语义（记日志、不触发）。
pub struct Db(pub DatabaseConnection);

#[async_trait]
impl FromContext for Db {
    async fn from_context(ctx: &Ctx) -> Extracted<Self> {
        // State<DatabaseConnection> derefs 到连接；`(*st).clone()` 取出共享句柄。
        let st = State::<DatabaseConnection>::from_context(ctx).await?;
        Ok(Db((*st).clone()))
    }
}
