//! 跨游戏共享背包 API —— 在 [`game_item`](crate::data::entity::game_item::Model) 共享表上做带闸增减,
//! 与 `AUser::add_coin`/`pay` 同属核心**共享**设施。任何游戏插件(赛马、未来钓鱼/种菜)都经这几个
//! 函数往同一个玩家背包里产出与消耗物品,各自在自己的 `item_id` 号段里活动(号段由各插件定常量,
//! 跨插件唯一)。
//!
//! 设计要点同经济层:增量原子、扣减带闸(`WHERE qty >= n`),`item_id` 是全局编号、不在核心侧
//! 解释含义(物品名/效果归各插件)。核心只管「谁有多少个几号物品」。

use nagisa::prelude::*;
use sea_orm::sea_query::Expr;
use sea_orm::{
    ColumnTrait, ConnectionTrait, DatabaseConnection, DbBackend, EntityTrait, QueryFilter, QueryOrder, Statement,
};

use crate::data::entity::game_item;

/// 某玩家某物品的持有数(无则 0)。
pub async fn count(db: &DatabaseConnection, uin: i64, item_id: i32) -> Result<i32> {
    let row = game_item::Entity::find_by_id((uin, item_id)).one(db).await.context("查物品")?;
    Ok(row.map(|m| m.qty).unwrap_or(0))
}

/// 入袋 `n` 个,夹到 `cap`;返回**溢出数**(没装下的,由调用方折算返还)。`n` 应 ≥ 0。
///
/// 写入是**单条原子 upsert**(`qty = LEAST(qty + n, cap)`),并发下不丢更新、不回退;溢出数按写前
/// 读到的当前值估算(仅用于返还,精度不敏感)。
pub async fn add_capped(db: &DatabaseConnection, uin: i64, item_id: i32, n: i32, cap: i32) -> Result<i32> {
    let cur = count(db, uin, item_id).await?;
    let overflow = (cur + n - cap).max(0);
    db.execute(Statement::from_sql_and_values(
        DbBackend::Postgres,
        "INSERT INTO game_item (uin, item_id, qty) VALUES ($1, $2, LEAST($3, $4)) \
         ON CONFLICT (uin, item_id) DO UPDATE SET qty = LEAST(game_item.qty + $3, $4)",
        [uin.into(), item_id.into(), n.into(), cap.into()],
    ))
    .await
    .context("写物品")?;
    Ok(overflow)
}

/// 带闸扣 `n` 个:够则扣、返 `true`,不够一动不动、返 `false`。`n` 应 ≥ 0。
pub async fn take(db: &DatabaseConnection, uin: i64, item_id: i32, n: i32) -> Result<bool> {
    let res = game_item::Entity::update_many()
        .col_expr(game_item::Column::Qty, Expr::col(game_item::Column::Qty).sub(n))
        .filter(game_item::Column::Uin.eq(uin))
        .filter(game_item::Column::ItemId.eq(item_id))
        .filter(game_item::Column::Qty.gte(n))
        .exec(db)
        .await
        .context("扣物品")?;
    Ok(res.rows_affected > 0)
}

/// 列出某玩家在 `[lo, hi)` 号段内持有的物品(`qty>0`,按 `item_id` 升序)。各插件传自己的号段
/// 取自家背包;传全域即取全部。
pub async fn list_range(db: &DatabaseConnection, uin: i64, lo: i32, hi: i32) -> Result<Vec<(i32, i32)>> {
    let rows = game_item::Entity::find()
        .filter(game_item::Column::Uin.eq(uin))
        .filter(game_item::Column::Qty.gt(0))
        .filter(game_item::Column::ItemId.gte(lo))
        .filter(game_item::Column::ItemId.lt(hi))
        .order_by_asc(game_item::Column::ItemId)
        .all(db)
        .await
        .context("查背包")?;
    Ok(rows.into_iter().map(|r| (r.item_id, r.qty)).collect())
}
