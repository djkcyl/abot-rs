//! 画板核心逻辑 —— 冷却 / 币价 / 落格事务。
//!
//! - **冷却** [`cooldown_secs`]:`max(15min, 2h − 等级×4min)`,从 `place_history` 该人最后落格时间判。
//! - **币价** [`price`]:`min(20, 1 + 累计落格 / (50 + 等级×10))`,涨得很慢、等级放缓涨幅。
//! - **落格** [`try_place`]:校验/冷却/余额过关后,一个事务原子地 upsert 真值 + 追审计 + 扣币 + 加经验。
//!
//! 触碰共享经济仍走核心的 [`add_coin_on`](crate::data::user::add_coin_on)(原子自加 + 写 `coin_log`);
//! 经验在同一事务里对 `user.exp` col_expr 自加。整笔要么全成、要么全回滚。

use nagisa::prelude::*;
use chrono::Local;
use sea_orm::prelude::DateTimeWithTimeZone;
use sea_orm::sea_query::{Expr, OnConflict};
use sea_orm::ActiveValue::{NotSet, Set};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter,
    QueryOrder, QuerySelect, TransactionTrait,
};

use super::colors::EMPTY;
use super::entity::{history, pixel};
use super::render;
use crate::data::entity::user;
use crate::data::user::try_debit_on;
use crate::data::AUser;

/// 落账原因(写入 `coin_log.reason`)。
const PLACE_REASON: &str = "画板";
/// 每次落格奖励的经验。
const EXP_PER_PLACE: i64 = 2;

/// 冷却间隔(秒):`max(15min, 2h − 等级×4min)`。等级越高越短,触底 15 分钟。
pub fn cooldown_secs(level: i64) -> i64 {
    (7200 - level * 240).max(900)
}

/// 单格币价:`min(20, 1 + 累计落格 / (50 + 等级×10))`。涨得很慢,等级放缓涨幅,封顶 20。
pub fn price(placed: i64, level: i64) -> i64 {
    (1 + placed / (50 + level * 10)).min(20)
}

/// 该人累计落格数(`COUNT place_history WHERE uin`)—— 兼作币价的 `n` 与战绩。
pub async fn placed_count(db: &DatabaseConnection, uin: i64) -> Result<i64> {
    let n = history::Entity::find()
        .filter(history::Column::Uin.eq(uin))
        .count(db)
        .await
        .context("查累计落格数失败")?;
    Ok(n as i64)
}

/// 按时间范围取落格历史(按 `at` 升序),只要重演用的 `(x, y, new_color)`。`since=None` 即全量。
pub async fn history_in_range(
    db: &DatabaseConnection,
    since: Option<DateTimeWithTimeZone>,
) -> Result<Vec<(i32, i32, u8)>> {
    let mut q = history::Entity::find().order_by_asc(history::Column::At);
    if let Some(s) = since {
        q = q.filter(history::Column::At.gte(s));
    }
    let rows = q.all(db).await.context("查回放历史失败")?;
    Ok(rows.into_iter().map(|r| (r.x, r.y, r.new_color.clamp(1, 32) as u8)).collect())
}

/// 某格最近若干次落格(按 `at` 降序,最多 `limit` 条)。供 superuser 查「这格谁画的」。
pub async fn cell_history(
    db: &DatabaseConnection,
    x: i32,
    y: i32,
    limit: u64,
) -> Result<Vec<history::Model>> {
    history::Entity::find()
        .filter(history::Column::X.eq(x))
        .filter(history::Column::Y.eq(y))
        .order_by_desc(history::Column::At)
        .limit(limit)
        .all(db)
        .await
        .context("查格子历史失败")
}

/// 全局最近若干次落格(按 `at` 降序,最多 `limit` 条)。供 `画板历史`(无坐标)用。
pub async fn recent_history(db: &DatabaseConnection, limit: u64) -> Result<Vec<history::Model>> {
    history::Entity::find()
        .order_by_desc(history::Column::At)
        .limit(limit)
        .all(db)
        .await
        .context("查最近历史失败")
}

/// 某人最近若干次落格(按 `at` 降序,最多 `limit` 条)。供 `画板历史 @某人` 用。
pub async fn person_history(
    db: &DatabaseConnection,
    uin: i64,
    limit: u64,
) -> Result<Vec<history::Model>> {
    history::Entity::find()
        .filter(history::Column::Uin.eq(uin))
        .order_by_desc(history::Column::At)
        .limit(limit)
        .all(db)
        .await
        .context("查个人历史失败")
}

/// 某格被画过的总次数。
pub async fn cell_count(db: &DatabaseConnection, x: i32, y: i32) -> Result<i64> {
    let n = history::Entity::find()
        .filter(history::Column::X.eq(x))
        .filter(history::Column::Y.eq(y))
        .count(db)
        .await
        .context("查格子次数失败")?;
    Ok(n as i64)
}

/// 该人最后一次落格时间(`MAX(at)`,取最新一行)。
async fn last_place(db: &DatabaseConnection, uin: i64) -> Result<Option<DateTimeWithTimeZone>> {
    let row = history::Entity::find()
        .filter(history::Column::Uin.eq(uin))
        .order_by_desc(history::Column::At)
        .one(db)
        .await
        .context("查最后落格时间失败")?;
    Ok(row.map(|r| r.at))
}

/// 冷却剩余:返回 `Some((剩余分钟, 间隔分钟))` 表示仍在冷却中;`None` 表示可落格。
pub async fn cooldown_remaining(
    db: &DatabaseConnection,
    uin: i64,
    level: i64,
) -> Result<Option<(i64, i64)>> {
    let interval = cooldown_secs(level);
    let Some(last) = last_place(db, uin).await? else {
        return Ok(None); // 从未落格
    };
    let elapsed = (Local::now().fixed_offset() - last).num_seconds();
    let remain = interval - elapsed;
    if remain > 0 {
        Ok(Some(((remain + 59) / 60, interval / 60))) // 剩余向上取整到分钟
    } else {
        Ok(None)
    }
}

/// 一次落格的结果。
pub enum PlaceResult {
    /// 成功:本次花费 + 落格后余额。
    Placed { cost: i64, balance: i64 },
    /// 冷却中:剩余分钟 + 当前间隔分钟。
    Cooldown { remain_min: i64, interval_min: i64 },
    /// 余额不足:本格价 + 现有余额。
    Poor { cost: i64, have: i64 },
    /// 该格已是此色(不扣币不计冷却)。
    Same,
    /// 坐标越界。
    OutOfRange,
}

/// 尝试落一格。`color` 已是合法调色板索引(1–32);坐标范围在此校验。
///
/// 过关顺序:范围 → 冷却 → 同色早退 → 余额。最后一个事务原子落账。
pub async fn try_place(
    user: &AUser,
    db: &DatabaseConnection,
    group_id: Option<i64>,
    x: i32,
    y: i32,
    color: u8,
) -> Result<PlaceResult> {
    if !(0..render::W as i32).contains(&x) || !(0..render::H as i32).contains(&y) {
        return Ok(PlaceResult::OutOfRange);
    }
    let uin = user.uin();
    let level = user.level();

    if let Some((remain_min, interval_min)) = cooldown_remaining(db, uin, level).await? {
        return Ok(PlaceResult::Cooldown { remain_min, interval_min });
    }

    // 旧色(无行 = 空 = EMPTY 哨兵 0,与任何可选色都不等,故空格上画白也能正常落)。
    // 同色直接早退,不扣币不计冷却。
    let old = pixel::Entity::find_by_id((x, y))
        .one(db)
        .await
        .context("查旧像素失败")?
        .map(|m| m.color as u8)
        .unwrap_or(EMPTY);
    if old == color {
        return Ok(PlaceResult::Same);
    }

    let n = placed_count(db, uin).await?;
    let cost = price(n, level);
    // 快速预判(友好提示、省得白开事务);真正的闸是事务里的带闸扣款。
    if user.coin() < cost {
        return Ok(PlaceResult::Poor { cost, have: user.coin() });
    }

    // 原子落账:带闸扣币(放最前,不够即回滚)→ upsert 真值 → 追审计 → 加经验。
    let now = Local::now().fixed_offset();
    let txn = db.begin().await.context("开启落格事务失败")?;

    // 带闸扣款:`WHERE coin >= cost`。引导流程的 user.coin() 快照可能拖了几十秒已过期,
    // 这里以库侧真值为准,余额不足则回滚、回 Poor(重读真实余额)。
    if !try_debit_on(&txn, uin, cost, PLACE_REASON.to_string()).await? {
        txn.rollback().await.ok();
        let have = AUser::get(db, uin).await.map(|u| u.coin()).unwrap_or(0);
        return Ok(PlaceResult::Poor { cost, have });
    }

    pixel::Entity::insert(pixel::ActiveModel {
        x: Set(x),
        y: Set(y),
        color: Set(color as i32),
        uin: Set(uin),
        at: Set(now),
    })
    .on_conflict(
        OnConflict::columns([pixel::Column::X, pixel::Column::Y])
            .update_columns([pixel::Column::Color, pixel::Column::Uin, pixel::Column::At])
            .to_owned(),
    )
    .exec(&txn)
    .await
    .context("upsert 像素失败")?;

    history::ActiveModel {
        id: NotSet,
        uin: Set(uin),
        group_id: Set(group_id),
        x: Set(x),
        y: Set(y),
        old_color: Set(old as i32),
        new_color: Set(color as i32),
        at: NotSet,
    }
    .insert(&txn)
    .await
    .context("写落格历史失败")?;

    // 扣币已在事务最前完成(带闸),这里只加经验。
    user::Entity::update_many()
        .col_expr(user::Column::Exp, Expr::col(user::Column::Exp).add(EXP_PER_PLACE))
        .filter(user::Column::Uin.eq(uin))
        .exec(&txn)
        .await
        .context("加经验失败")?;

    txn.commit().await.context("提交落格事务失败")?;

    // 真实余额重读(快照可能已过期)。
    let balance = AUser::get(db, uin).await.map(|u| u.coin()).unwrap_or(user.coin() - cost);
    Ok(PlaceResult::Placed { cost, balance })
}
