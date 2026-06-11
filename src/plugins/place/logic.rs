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
use super::entity::{history, pixel, replay_cache, snapshot};
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

/// 回放取数的单批行数。
const REPLAY_BATCH: u64 = 10_000;

/// 画布快照间隔:每这么多笔落格存一份 `place_snapshot`。窗口回放从最近快照起步,
/// 最多再重演这么多笔零头;快照本体 36KB(bytea,库侧自动压缩),存储可忽略。
pub const SNAPSHOT_EVERY: i64 = 5_000;

/// 取一段历史(按落格次序升序),只要重演用的 `(x, y, new_color)`。
/// `(after, before)` 是 history id 的开区间界:`after`=0 从头,`before=None` 到尾。
///
/// 按 `id` 游标分批拉、`select_only` 三列直接成元组——历史表是追加审计,行数无上界,
/// 一次 `.all()` 物化完整 Model 在千万行量级是 GB 级内存;分批 + 元组后内存只随结果集
/// (每行 9 字节)走。`id` 自增,次序即落格次序。
async fn fetch_strokes(
    db: &DatabaseConnection,
    after: i64,
    before: Option<i64>,
) -> Result<Vec<(i32, i32, u8)>> {
    let mut out = Vec::new();
    let mut cursor = after;
    loop {
        let mut q = history::Entity::find()
            .select_only()
            .column(history::Column::Id)
            .column(history::Column::X)
            .column(history::Column::Y)
            .column(history::Column::NewColor)
            .filter(history::Column::Id.gt(cursor))
            .order_by_asc(history::Column::Id)
            .limit(REPLAY_BATCH);
        if let Some(b) = before {
            q = q.filter(history::Column::Id.lt(b));
        }
        let rows: Vec<(i64, i32, i32, i32)> =
            q.into_tuple().all(db).await.context("查回放历史失败")?;
        let Some(&(last_id, ..)) = rows.last() else { break };
        cursor = last_id;
        out.extend(rows.iter().map(|&(_, x, y, c)| (x, y, c.clamp(1, 32) as u8)));
        if (rows.len() as u64) < REPLAY_BATCH {
            break;
        }
    }
    Ok(out)
}

/// 把一串笔应用到画布缓冲(越界跳过)。
fn apply_strokes(buf: &mut [u8], rows: &[(i32, i32, u8)]) {
    for &(x, y, c) in rows {
        if (0..render::W as i32).contains(&x) && (0..render::H as i32).contains(&y) {
            buf[(y as u32 * render::W + x as u32) as usize] = c;
        }
    }
}

/// 回放窗口数据:起点画布(窗口开始时刻的真实画布;全量回放为全空)+ 窗内笔序列。
pub struct ReplayWindow {
    /// W×H 索引缓冲,回放首帧。
    pub base: Vec<u8>,
    /// 窗内的笔,落格次序。
    pub rows: Vec<(i32, i32, u8)>,
}

/// 窗口起点:第一笔 `at >= since` 的 history id;`None` = 窗内没有落格。
pub async fn window_start_id(
    db: &DatabaseConnection,
    since: DateTimeWithTimeZone,
) -> Result<Option<i64>> {
    history::Entity::find()
        .select_only()
        .column(history::Column::Id)
        .filter(history::Column::At.gte(since))
        .order_by_asc(history::Column::At)
        .limit(1)
        .into_tuple()
        .one(db)
        .await
        .context("查窗口起点失败")
}

/// 窗内笔数(id ≥ start 的行数,PK 范围 COUNT)——回放计费的「重量」。
pub async fn strokes_from(db: &DatabaseConnection, start: i64) -> Result<i64> {
    let n = history::Entity::find()
        .filter(history::Column::Id.gte(start))
        .count(db)
        .await
        .context("查窗内笔数失败")?;
    Ok(n as i64)
}

/// 回放计费:重量 = 窗内笔数,每 1000 笔 1 币(向上取整),封顶 88。
pub fn replay_cost(strokes: i64) -> i64 {
    ((strokes + 999) / 1_000).clamp(1, 88)
}

/// 取回放窗口数据。`since=None` 全量(空白起步、全部笔);带窗则从最近的画布快照
/// 恢复出**窗口起点时刻的真实画布**做首帧(最多补演 [`SNAPSHOT_EVERY`] 笔零头),
/// 窗内笔照常取——回放语义是「画布当时的样子怎么演变到现在」,不是空白上只画窗内笔。
pub async fn replay_window(
    db: &DatabaseConnection,
    since: Option<DateTimeWithTimeZone>,
) -> Result<ReplayWindow> {
    let blank = vec![EMPTY; (render::W * render::H) as usize];
    let Some(s) = since else {
        return Ok(ReplayWindow { base: blank, rows: fetch_strokes(db, 0, None).await? });
    };
    let Some(start) = window_start_id(db, s).await? else {
        return Ok(ReplayWindow { base: blank, rows: Vec::new() });
    };
    // 起点画布 = 最近的水位 < start 的快照 + 补演 (水位, start) 之间的零头。
    // 快照长度不对(理论不该有)按无快照处理,从零补演。
    let snap = snapshot::Entity::find()
        .filter(snapshot::Column::HistoryId.lt(start))
        .order_by_desc(snapshot::Column::HistoryId)
        .one(db)
        .await
        .context("查画布快照失败")?;
    let (mut base, watermark) = match snap {
        Some(s) if s.canvas.len() == blank.len() => (s.canvas, s.history_id),
        Some(s) => {
            tracing::warn!(history_id = s.history_id, len = s.canvas.len(), "画布快照长度异常,从零补演");
            (blank, 0)
        }
        None => (blank, 0),
    };
    apply_strokes(&mut base, &fetch_strokes(db, watermark, Some(start)).await?);
    let rows = fetch_strokes(db, start - 1, None).await?;
    Ok(ReplayWindow { base, rows })
}

/// 取当日(业务日)的全量回放缓存 GIF。
pub async fn replay_cache_get(db: &DatabaseConnection) -> Result<Option<Vec<u8>>> {
    let today = crate::data::util::business_day();
    let row = replay_cache::Entity::find_by_id(today).one(db).await.context("查回放缓存失败")?;
    Ok(row.map(|m| m.gif))
}

/// 落当日全量回放缓存(覆盖),顺手清掉旧日行——表内始终只有当日一行。
pub async fn replay_cache_put(db: &DatabaseConnection, gif: Vec<u8>) -> Result<()> {
    use sea_orm::sea_query::OnConflict;
    let today = crate::data::util::business_day();
    replay_cache::Entity::insert(replay_cache::ActiveModel {
        day: Set(today),
        gif: Set(gif),
        at: Set(Local::now().fixed_offset()),
    })
    .on_conflict(
        OnConflict::column(replay_cache::Column::Day)
            .update_columns([replay_cache::Column::Gif, replay_cache::Column::At])
            .to_owned(),
    )
    .exec(db)
    .await
    .context("写回放缓存失败")?;
    replay_cache::Entity::delete_many()
        .filter(replay_cache::Column::Day.ne(today))
        .exec(db)
        .await
        .context("清旧回放缓存失败")?;
    Ok(())
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

    let hist = history::ActiveModel {
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

    // 周期画布快照:history id 整除间隔时事务内读真值表存一份(原子:快照在则它的
    // 历史行必在)。id 序列有回滚空洞,间隔只是近似;并发下可能漏掉一笔 id 更小但尚未
    // 提交的——影响仅回放底图差一格,且重演按 id 序幂等覆盖,可忽略。
    if hist.id % SNAPSHOT_EVERY == 0 {
        let canvas = render::load_canvas(&txn).await?;
        snapshot::ActiveModel {
            history_id: Set(hist.id),
            canvas: Set(canvas),
            at: Set(now),
        }
        .insert(&txn)
        .await
        .context("写画布快照失败")?;
    }

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
