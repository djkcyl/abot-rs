//! `user` 表实体 —— 一个 QQ 用户的**跨插件共享**持久状态（游戏币、经验、封禁等）。
//!
//! 主键是 QQ 号 `uin`（`i64`，OneBot 的 `Uin` 即 `i64` 内核）。这里**只**放真正
//! 跨插件共享的字段（`id` / `coin` / `exp` / `alias` / `alias_color` / `banned` /
//! `theme` / `theme_color` / `join_time`）；任何插件私有的状态（如签到流水）一律归各插件**自己**的表
//! （账号昵称缓存也已拆出去——见 [`identity`](crate::data::entity::identity)，不在本热行上）
//! （见 `plugins::sign`），不得泄进这张核心表。可变计数都带库侧缺省，新用户
//! `insert` 时无需逐个填。
//!
//! 这是「数据 API」的底座：`AUser` 句柄就包住一行 [`Model`] + 一份连接，方法
//! （`coin`/`add_coin` …）直接在其上跑——没有仓储/DAO 中间层。
//! 游戏币改动走 `col_expr(Column::Coin, Expr::col(Column::Coin).add(delta))` 原子增量，
//! 绝不 read-modify-write 写绝对值。

use sea_orm::entity::prelude::*;

/// `user` 行模型。字段顺序与列定义一致；缺省值见 [`migration`](crate::data::migration)。
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "user")]
pub struct Model {
    /// QQ 号（主键，**非**自增——由上游事件给定）。
    #[sea_orm(primary_key, auto_increment = false)]
    pub uin: i64,
    /// 站内 UID（自增注册序号，唯一）。供呈现用，insert 时不填、由库侧序列发号。
    pub id: i64,
    /// 游戏币余额。库侧缺省 `10`；改动一律走原子增量表达式，不读改写。
    pub coin: i64,
    /// 自设昵称（用户经「改名」命令自定，呈现时优先级最高；库侧缺省空串、空串视作未设）。
    pub alias: String,
    /// 自设昵称的颜色（用户经「昵称颜色」命令自定，`#rrggbb` 原始色相；库侧缺省空串、
    /// 空串 = 不上色用缺省文字色）。出图点经 [`imaging::readable_hex`](crate::imaging::readable_hex)
    /// 按本次亮暗收对比后上色，只在显示名取自 `alias` 时生效。
    pub alias_color: String,
    /// 经验值。库侧缺省 `0`。
    pub exp: i64,
    /// 是否封禁。库侧缺省 `false`。
    pub banned: bool,
    /// 出图亮暗偏好：`auto`（按日出日落）/ `light` / `dark`。库侧缺省 `auto`，经「主题」命令改。
    pub theme: String,
    /// 出图主题色偏好：五套预设之一的键（见 [`imaging::THEMES`](crate::imaging::THEMES)），
    /// 空串走缺省远黛蓝。库侧缺省空串，经「主题」命令改，出图点经
    /// [`imaging::UserTheme`](crate::imaging::UserTheme) 解析成标准色卡。
    pub theme_color: String,
    /// 首次入库时间（带时区）。库侧缺省 `now()`。
    pub join_time: DateTimeWithTimeZone,
}

/// `user` 表无外联关系（游戏币流水经 `uin` 软关联，不建 FK 以免约束拖慢写入）。
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
