//! `RankBoard` —— 「排行榜」的插件自注册榜单槽，与 [`ProfileSection`](crate::data::profile::ProfileSection)
//! / [`PluginMigration`](crate::data::migration::PluginMigration) 同款 inventory 机制。
//!
//! 排行榜要排的指标横跨核心(游戏币 / 经验)与各插件私有数据(发言 / 签到 …)。为不破「插件自有
//! 数据」的墙——排行榜插件**不**直接读各插件表——这里给一个自注册槽:每个榜 `submit!` 一个
//! [`RankBoard`],自己产出 top-N 与「某人的名次」;排行榜插件经 [`collect_boards`] 统一收集,
//! 核心 / 排行榜都不引用任何具体插件。核心的游戏币 / 经验榜读核心 `user` 表,就在本文件注册。
//!
//! # 作用域
//!
//! 所有榜一律按**全局数值**排(一人一份的全局游戏币 / 经验 / 跨群总发言 / 累计签到天数)。一个榜
//! 有两种看法:**全局**(全站参与)或**本群**(同一套全局数值,人员集合限制为本群成员)——人员
//! 过滤经 `top` / `rank_of` 的 `members` 参数(`None` = 全站,`Some(set)` = 仅这些 uin)。

use std::collections::HashSet;

use nagisa::async_trait;
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect};

use crate::data::entity::user;
use crate::data::level::level_info;

/// 榜单上的一行:谁(`uin`)+ 排序数值(`value`,越大越靠前)。名字 / 头像由排行榜插件按 uin
/// 另解析,本层只产出原始数值。
#[derive(Clone, Copy, Debug)]
pub struct RankRow {
    /// 上榜者 QQ 号。
    pub uin: i64,
    /// 排序数值(全局口径,降序)。
    pub value: i64,
}

/// 一个榜单的自注册槽位:包一个构造 [`RankBoard`] 的函数指针。
pub struct RankSection(pub fn() -> Box<dyn RankBoard>);
nagisa::inventory::collect!(RankSection);

/// 一个排行榜指标:产出 top-N 与「某人的名次」,可按人员集合限定(本群)。查询失败一律
/// 记日志并降级(空 / `None`),让排行榜照常出图——与 [`ProfileProvider`](crate::data::profile::ProfileProvider)
/// 的宽容口径一致。
#[async_trait]
pub trait RankBoard: Send + Sync {
    /// 榜单键(单榜命令路由 / 总览排序用,如 `"coin"`)。
    fn key(&self) -> &'static str;
    /// 榜单标题(如 `"游戏币榜"` / `"等级榜"`)。
    fn title(&self) -> &'static str;
    /// 把一个排序数值渲成展示串(如 `"1234 游戏币"` / `"Lv.12 · 3456 经验"` / `"789 条"`)。
    fn format_value(&self, value: i64) -> String;
    /// 取前 `n` 名(降序)。`members` 限定参与人员(`None` = 全站)。
    async fn top(&self, db: &DatabaseConnection, members: Option<&HashSet<i64>>, n: usize) -> Vec<RankRow>;
    /// 取某人的名次(1 起)与其数值;从未上榜(无数据 / 不在人员集合)→ `None`。
    async fn rank_of(&self, db: &DatabaseConnection, members: Option<&HashSet<i64>>, uin: i64) -> Option<(u32, i64)>;
}

/// 收集所有已注册的榜单(顺序 = `inventory` 注册顺序;总览的展示顺序由排行榜插件另定)。
pub fn collect_boards() -> Vec<Box<dyn RankBoard>> {
    nagisa::inventory::iter::<RankSection>.into_iter().map(|s| (s.0)()).collect()
}

// ———— 核心榜:游戏币 / 等级,直接读核心 `user` 表 ————

/// 游戏币榜:核心 `user.coin` 降序。
struct CoinBoard;

#[async_trait]
impl RankBoard for CoinBoard {
    fn key(&self) -> &'static str {
        "coin"
    }
    fn title(&self) -> &'static str {
        "游戏币榜"
    }
    fn format_value(&self, value: i64) -> String {
        format!("{value} 游戏币")
    }
    async fn top(&self, db: &DatabaseConnection, members: Option<&HashSet<i64>>, n: usize) -> Vec<RankRow> {
        user_top(db, user::Column::Coin, members, n).await
    }
    async fn rank_of(&self, db: &DatabaseConnection, members: Option<&HashSet<i64>>, uin: i64) -> Option<(u32, i64)> {
        user_rank_of(db, user::Column::Coin, members, uin).await
    }
}

/// 等级榜:核心 `user.exp` 降序(等级由经验单调派生,故经验序即等级序)。
struct LevelBoard;

#[async_trait]
impl RankBoard for LevelBoard {
    fn key(&self) -> &'static str {
        "level"
    }
    fn title(&self) -> &'static str {
        "等级榜"
    }
    fn format_value(&self, value: i64) -> String {
        format!("Lv.{} · {} 经验", level_info(value).level, value)
    }
    async fn top(&self, db: &DatabaseConnection, members: Option<&HashSet<i64>>, n: usize) -> Vec<RankRow> {
        user_top(db, user::Column::Exp, members, n).await
    }
    async fn rank_of(&self, db: &DatabaseConnection, members: Option<&HashSet<i64>>, uin: i64) -> Option<(u32, i64)> {
        user_rank_of(db, user::Column::Exp, members, uin).await
    }
}

/// 核心 `user` 表某列降序的 top-N(可选人员过滤)。查询失败记 warn、退空。
async fn user_top(
    db: &DatabaseConnection,
    col: user::Column,
    members: Option<&HashSet<i64>>,
    n: usize,
) -> Vec<RankRow> {
    let mut q =
        user::Entity::find().select_only().column(user::Column::Uin).column(col).order_by_desc(col).limit(n as u64);
    if let Some(set) = members {
        q = q.filter(user::Column::Uin.is_in(set.iter().copied()));
    }
    match q.into_tuple::<(i64, i64)>().all(db).await {
        Ok(rows) => rows.into_iter().map(|(uin, value)| RankRow { uin, value }).collect(),
        Err(e) => {
            tracing::warn!(error = %e, col = ?col, "查询 user 榜 top 失败");
            Vec::new()
        }
    }
}

/// 核心 `user` 表某列上某人的名次:严格大于其值的人数 + 1。不在 `user` 表(从无记录)→ `None`。
/// 本群口径下 `members` 须含该 uin(调用方查的是本群成员自己,天然满足)。
async fn user_rank_of(
    db: &DatabaseConnection,
    col: user::Column,
    members: Option<&HashSet<i64>>,
    uin: i64,
) -> Option<(u32, i64)> {
    let mine = user::Entity::find()
        .select_only()
        .column(col)
        .filter(user::Column::Uin.eq(uin))
        .into_tuple::<i64>()
        .one(db)
        .await
        .ok()??;
    let mut q = user::Entity::find().filter(col.gt(mine));
    if let Some(set) = members {
        q = q.filter(user::Column::Uin.is_in(set.iter().copied()));
    }
    let ahead = q.count(db).await.ok()?;
    Some((ahead as u32 + 1, mine))
}

// 自注册:把核心两个榜登记进进程级 inventory 集合。
nagisa::inventory::submit! { RankSection(|| Box::new(CoinBoard)) }
nagisa::inventory::submit! { RankSection(|| Box::new(LevelBoard)) }
