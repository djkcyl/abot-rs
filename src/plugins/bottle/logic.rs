//! 漂流瓶业务逻辑 —— 建瓶、加权捞取、原子计数、去极值评分、评分/评论 upsert、查/软删、
//! 发出消息 → 瓶子的映射（「取原文」反查）。
//!
//! 只管数据层：传 `&DatabaseConnection`，不碰消息/经济/审核（那些在命令层串）。计数改动一律走
//! **原子 UPDATE**（`col_expr` / 裸 SQL），绝不读改写；评分/评论的唯一约束冲突走 `on_conflict`。

use sea_orm::sea_query::{Expr, OnConflict};
use sea_orm::{
    ActiveValue::NotSet, ColumnTrait, DatabaseConnection, EntityTrait, Order, PaginatorTrait, QueryFilter, QueryOrder,
    QuerySelect, Set,
};

use super::entity::{bottle, discuss, score, sent};

/// 可捞状态：人工通过 / AI 直接通过。其余（`pending` / `rejected`）不出现在大海里。
const PICKABLE_STATUS: [&str; 2] = ["approved", "ai_approved"];

/// 建瓶参数 —— 命令层把审核/经济算完后,把成形的一只瓶子交给 [`create_bottle`]。
pub struct NewBottle {
    /// 投放者 QQ 号。
    pub uin: i64,
    /// 投放者显示名；缺失为 `None`。
    pub nickname: Option<String>,
    /// 来源会话群号；私聊为 `None`。
    pub group_id: Option<i64>,
    /// 文本内容；纯图片瓶子为 `None`。
    pub text: Option<String>,
    /// 已落盘的图片文件名数组。
    pub images: Vec<String>,
    /// 是否匿名。
    pub anonymous: bool,
    /// 剩余可捞次数（-1 不限）。
    pub remaining: i32,
    /// 审核状态：`pending`（待人工）/ `ai_approved`（AI 直接通过）。
    pub status: String,
    /// 审核命中详情（JSONB）；未命中为 `None`。
    pub moderation: Option<serde_json::Value>,
}

/// 建瓶，返回新瓶编号（`id`）。`images` 存成 JSONB 数组。
pub async fn create_bottle(db: &DatabaseConnection, b: NewBottle) -> anyhow::Result<i64> {
    let row = bottle::ActiveModel {
        id: NotSet,
        uin: Set(b.uin),
        nickname: Set(b.nickname),
        group_id: Set(b.group_id),
        text: Set(b.text),
        images: Set(serde_json::Value::Array(b.images.into_iter().map(serde_json::Value::String).collect())),
        anonymous: Set(b.anonymous),
        total_pickups: NotSet,
        remaining_pickups: Set(b.remaining),
        status: Set(b.status),
        moderation: Set(b.moderation),
        isdelete: NotSet,
        created_at: NotSet,
    };
    let res = bottle::Entity::insert(row).exec(db).await?;
    Ok(res.last_insert_id)
}

/// 加权随机选一个可捞的瓶子（不改库）。可捞 = `isdelete=false` AND `status IN (可捞状态)` AND
/// `remaining_pickups <> 0`。先 SQL 随机采样 3 个，再按各自评分权重（去极值均值四舍五入,至少 1）
/// 加权选一个；没有候选返 `None`。
pub async fn select_candidate(db: &DatabaseConnection) -> anyhow::Result<Option<bottle::Model>> {
    let candidates = bottle::Entity::find()
        .filter(bottle::Column::Isdelete.eq(false))
        .filter(bottle::Column::Status.is_in(PICKABLE_STATUS))
        .filter(bottle::Column::RemainingPickups.ne(0))
        .order_by(Expr::cust("RANDOM()"), Order::Asc)
        .limit(3)
        .all(db)
        .await?;
    if candidates.is_empty() {
        return Ok(None);
    }

    // 权重 = round(去极值均值)，无评分按 3.0；下限 1（保证人人有机会被捞）。
    let mut weights = Vec::with_capacity(candidates.len());
    for c in &candidates {
        let avg = score_avg(db, c.id).await?.unwrap_or(3.0);
        weights.push((avg.round() as u32).max(1));
    }

    use rand::distr::Distribution;
    use rand::distr::weighted::WeightedIndex;
    let dist = WeightedIndex::new(&weights).expect("权重至少为 1,非空且总和为正");
    let idx = dist.sample(&mut rand::rng());
    Ok(candidates.into_iter().nth(idx))
}

/// 记一次打捞：`total_pickups += 1`；`remaining_pickups > 0` 时 `-= 1`（-1 不限的不动）。
/// 原子 UPDATE，绝不读改写。
pub async fn record_pickup(db: &DatabaseConnection, bottle_id: i64) -> anyhow::Result<()> {
    bottle::Entity::update_many()
        .col_expr(bottle::Column::TotalPickups, Expr::col(bottle::Column::TotalPickups).add(1))
        .col_expr(
            bottle::Column::RemainingPickups,
            Expr::cust("CASE WHEN remaining_pickups > 0 THEN remaining_pickups - 1 ELSE remaining_pickups END"),
        )
        .filter(bottle::Column::Id.eq(bottle_id))
        .exec(db)
        .await?;
    Ok(())
}

/// 去极值均值：`scores` 须**按值升序**。<3 条返 `None`；否则掐掉最低/最高各 5%
/// （索引区间 `floor(n*0.05)..floor(n*0.95)`），其余求均值，保留 1 位小数。
fn trimmed_mean(scores: &[i16]) -> Option<f64> {
    let n = scores.len();
    if n < 3 {
        return None;
    }
    let lo = (n as f64 * 0.05).floor() as usize;
    let hi = (n as f64 * 0.95).floor() as usize;
    let trimmed = &scores[lo..hi];
    if trimmed.is_empty() {
        return None;
    }
    let sum: i64 = trimmed.iter().map(|&s| s as i64).sum();
    let avg = sum as f64 / trimmed.len() as f64;
    Some((avg * 10.0).round() / 10.0)
}

/// 去极值均值评分：取该瓶全部评分按值升序后走 `trimmed_mean`。
pub async fn score_avg(db: &DatabaseConnection, bottle_id: i64) -> anyhow::Result<Option<f64>> {
    let scores: Vec<i16> = score::Entity::find()
        .filter(score::Column::BottleId.eq(bottle_id))
        .order_by_asc(score::Column::Score)
        .select_only()
        .column(score::Column::Score)
        .into_tuple()
        .all(db)
        .await?;
    Ok(trimmed_mean(&scores))
}

/// 批量取一组瓶子的去极值均值评分（列表用，一次查询，免 N+1）。只返回有评分（≥3 条）的瓶子。
pub async fn score_avgs(db: &DatabaseConnection, ids: &[i64]) -> anyhow::Result<std::collections::HashMap<i64, f64>> {
    use std::collections::HashMap;
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    // 一次取这批瓶子的全部 (bottle_id, score)，按瓶子、再按分数升序，便于分组后直接走 trimmed_mean。
    let rows: Vec<(i64, i16)> = score::Entity::find()
        .filter(score::Column::BottleId.is_in(ids.iter().copied()))
        .order_by_asc(score::Column::BottleId)
        .order_by_asc(score::Column::Score)
        .select_only()
        .column(score::Column::BottleId)
        .column(score::Column::Score)
        .into_tuple()
        .all(db)
        .await?;
    let mut by: HashMap<i64, Vec<i16>> = HashMap::new();
    for (bid, s) in rows {
        by.entry(bid).or_default().push(s);
    }
    let mut out = HashMap::new();
    for (bid, scores) in by {
        if let Some(avg) = trimmed_mean(&scores) {
            out.insert(bid, avg);
        }
    }
    Ok(out)
}

/// 评分 upsert：按 `(bottle_id, uin)` 唯一键，存在即改分。`score` 已校验 `1..=5`（校验在上层）。
pub async fn set_score(db: &DatabaseConnection, bottle_id: i64, uin: i64, score: i16) -> anyhow::Result<()> {
    let row = score::ActiveModel {
        id: NotSet,
        bottle_id: Set(bottle_id),
        uin: Set(uin),
        score: Set(score),
        created_at: NotSet,
    };
    score::Entity::insert(row)
        .on_conflict(
            OnConflict::columns([score::Column::BottleId, score::Column::Uin])
                .update_column(score::Column::Score)
                .to_owned(),
        )
        .exec(db)
        .await?;
    Ok(())
}

/// 评论结果：成功记入 / 该用户对该瓶评论已达上限。
pub enum DiscussOutcome {
    /// 评论已插入。
    Added,
    /// 该用户对该瓶已 3 条，未再插入。
    LimitReached,
}

/// 评论：该用户对该瓶已 >=3 条则 `LimitReached`，否则插入并 `Added`。文本长度校验在上层。
pub async fn add_discuss(
    db: &DatabaseConnection,
    bottle_id: i64,
    uin: i64,
    nickname: Option<String>,
    text: &str,
) -> anyhow::Result<DiscussOutcome> {
    let count = discuss::Entity::find()
        .filter(discuss::Column::BottleId.eq(bottle_id))
        .filter(discuss::Column::Uin.eq(uin))
        .count(db)
        .await?;
    if count >= 3 {
        return Ok(DiscussOutcome::LimitReached);
    }
    let row = discuss::ActiveModel {
        id: NotSet,
        bottle_id: Set(bottle_id),
        uin: Set(uin),
        nickname: Set(nickname),
        text: Set(text.to_owned()),
        created_at: NotSet,
    };
    discuss::Entity::insert(row).exec(db).await?;
    Ok(DiscussOutcome::Added)
}

/// 取某瓶全部评论（按时间升序），渲染楼层用。
pub async fn get_discuss(db: &DatabaseConnection, bottle_id: i64) -> anyhow::Result<Vec<discuss::Model>> {
    let rows = discuss::Entity::find()
        .filter(discuss::Column::BottleId.eq(bottle_id))
        .order_by_asc(discuss::Column::CreatedAt)
        .all(db)
        .await?;
    Ok(rows)
}

/// 消息映射保留天数：超期行在下次 [`record_sent`] 时懒清理（不开定时任务）。
const SENT_KEEP_DAYS: i64 = 90;

/// 记一条「发出的瓶子转发消息 → 瓶子」映射（「取原文」按回复目标反查用），顺手懒清理
/// 超期行。同键冲突（理论不至）就地覆盖瓶子编号。
pub async fn record_sent(db: &DatabaseConnection, msg_key: &str, bottle_id: i64) -> anyhow::Result<()> {
    sent::Entity::delete_many()
        .filter(Expr::cust(format!("created_at < now() - interval '{SENT_KEEP_DAYS} days'")))
        .exec(db)
        .await?;
    let row = sent::ActiveModel { msg_key: Set(msg_key.to_owned()), bottle_id: Set(bottle_id), created_at: NotSet };
    sent::Entity::insert(row)
        .on_conflict(OnConflict::column(sent::Column::MsgKey).update_column(sent::Column::BottleId).to_owned())
        .exec(db)
        .await?;
    Ok(())
}

/// 按消息键反查瓶子编号（「取原文」回复路径）；没记过或已被清理返 `None`。
pub async fn sent_bottle_id(db: &DatabaseConnection, msg_key: &str) -> anyhow::Result<Option<i64>> {
    let row = sent::Entity::find_by_id(msg_key.to_owned()).one(db).await?;
    Ok(row.map(|r| r.bottle_id))
}

/// 按编号取瓶（含已删 / 各状态，调用方自行判断可见性）。
pub async fn get_bottle(db: &DatabaseConnection, id: i64) -> anyhow::Result<Option<bottle::Model>> {
    let row = bottle::Entity::find_by_id(id).one(db).await?;
    Ok(row)
}

/// 改瓶子审核状态（approved/rejected 等）。原子 UPDATE。
pub async fn set_status(db: &DatabaseConnection, id: i64, status: &str) -> anyhow::Result<()> {
    bottle::Entity::update_many()
        .col_expr(bottle::Column::Status, Expr::value(status))
        .filter(bottle::Column::Id.eq(id))
        .exec(db)
        .await?;
    Ok(())
}

/// 列出某人投放的瓶子（未删），按时间倒序，至多 `limit` 条。
pub async fn list_user_bottles(db: &DatabaseConnection, uin: i64, limit: u64) -> anyhow::Result<Vec<bottle::Model>> {
    let rows = bottle::Entity::find()
        .filter(bottle::Column::Uin.eq(uin))
        .filter(bottle::Column::Isdelete.eq(false))
        .order_by_desc(bottle::Column::CreatedAt)
        .limit(limit)
        .all(db)
        .await?;
    Ok(rows)
}

/// 软删结果：成功软删 / 找不到 / 非本人（且非主人）。
pub enum DeleteOutcome {
    /// 已置 `isdelete=true`。
    Deleted,
    /// 编号对应的瓶子不存在（或已删）。
    NotFound,
    /// 请求者既非投放者也非主人，无权删。
    NotOwner,
}

/// 软删：仅本人或主人可删。找不到→`NotFound`；非本人且非主人→`NotOwner`；否则置
/// `isdelete=true`→`Deleted`。
pub async fn delete_bottle(
    db: &DatabaseConnection,
    id: i64,
    requester: i64,
    is_master: bool,
) -> anyhow::Result<DeleteOutcome> {
    let Some(b) = bottle::Entity::find_by_id(id).filter(bottle::Column::Isdelete.eq(false)).one(db).await? else {
        return Ok(DeleteOutcome::NotFound);
    };
    if b.uin != requester && !is_master {
        return Ok(DeleteOutcome::NotOwner);
    }
    bottle::Entity::update_many()
        .col_expr(bottle::Column::Isdelete, Expr::value(true))
        .filter(bottle::Column::Id.eq(id))
        .exec(db)
        .await?;
    Ok(DeleteOutcome::Deleted)
}
