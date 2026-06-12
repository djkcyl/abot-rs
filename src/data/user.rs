//! `AUser` —— 一个用户的「数据 API」句柄：包住一行 [`user::Model`] + 一份连接，方法直接在其上
//! 跑。**不是**仓储/DAO/Service——句柄本身就是 API，没有任何中间层包装。
//!
//! 暴露的就这几样跨插件共享的经济 API：读 `coin/exp/level`、`add_coin`（奖励/无条件加减）、
//! `add_exp`、`pay`（带闸花费）、`transfer_to`（原子转账）。插件私有的状态与逻辑（如签到连签）
//! 归各插件自己，只经这些方法触碰共享经济。
//!
//! 设计要点：
//! - 金币改动一律走**原子增量** `col_expr(Coin, Expr::col(Coin) ± delta)`，绝不 read-modify-write。
//! - **花费**一律走带闸 `WHERE coin >= amount`（[`pay`](AUser::pay)/[`transfer_to`](AUser::transfer_to)），
//!   从根上杜绝 check-then-act 超支。
//! - 方法返回 `nagisa::Result`（内部用 nagisa 的 [`Context::context`] 转错），handler 直接 `?`。

use nagisa::prelude::*;
use sea_orm::sea_query::Expr;
use sea_orm::{
    ActiveModelTrait, ActiveValue::NotSet, ColumnTrait, ConnectionTrait, DatabaseConnection, EntityTrait, QueryFilter,
    Set, TransactionTrait,
};

use crate::data::entity::{coin_log, user};
use crate::data::level::{LevelChange, LevelInfo, level_info, level_of};

/// 一个用户的可变状态句柄：一行 [`user::Model`] + 一份共享连接。
///
/// 取得方式：[`AUser::get`]（按 uin 取或建）/ [`AUser::get_many`]（批量取或建）/ 或作为提取器
/// （消息发送者，自动建号）。写方法（`add_coin`/`pay`/…）改库后就地同步 `self.model`，故连续
/// 调用读到的是最新值。
#[derive(Clone, Debug)]
pub struct AUser {
    /// 当前行模型。写方法会就地同步它，使句柄读到的值与库一致。
    pub model: user::Model,
    /// 共享连接句柄（内部 `Arc`，克隆廉价）。
    pub db: DatabaseConnection,
}

impl AUser {
    /// 该用户的 QQ 号。
    pub fn uin(&self) -> i64 {
        self.model.uin
    }

    /// 站内 UID（自增注册序号，唯一）。呈现用；寻人一律仍按 `uin`。
    pub fn id(&self) -> i64 {
        self.model.id
    }

    /// 出图亮暗偏好（`auto` / `light` / `dark`）。出图点经
    /// [`imaging::pick_dark`](crate::imaging::pick_dark) 解析成本次亮暗。
    pub fn theme(&self) -> &str {
        &self.model.theme
    }

    /// 改出图亮暗偏好（调用方先校验取值，这里原样落库并同步 `self.model`）。
    pub async fn set_theme(&mut self, theme: &str) -> Result<()> {
        user::Entity::update_many()
            .col_expr(user::Column::Theme, Expr::value(theme))
            .filter(user::Column::Uin.eq(self.model.uin))
            .exec(&self.db)
            .await
            .context("写主题偏好失败")?;
        self.model.theme = theme.to_string();
        Ok(())
    }

    /// 出图主题色偏好（五套预设之一的键，空串 = 缺省远黛蓝）。
    pub fn theme_color(&self) -> &str {
        &self.model.theme_color
    }

    /// 改出图主题色偏好（调用方先归一成主题键或空串，这里原样落库并同步 `self.model`）。
    pub async fn set_theme_color(&mut self, color: &str) -> Result<()> {
        user::Entity::update_many()
            .col_expr(user::Column::ThemeColor, Expr::value(color))
            .filter(user::Column::Uin.eq(self.model.uin))
            .exec(&self.db)
            .await
            .context("写主题色偏好失败")?;
        self.model.theme_color = color.to_string();
        Ok(())
    }

    /// 把主题偏好（亮暗 + 主题色）一次解析成本次出图主题（标准色卡），渲染端直接用。
    pub fn render_theme(&self) -> crate::imaging::UserTheme {
        crate::imaging::UserTheme::resolve(self.theme(), self.theme_color())
    }

    /// 当前金币余额（`self.model` 侧的值，经各写方法与库保持同步）。
    pub fn coin(&self) -> i64 {
        self.model.coin
    }

    /// 当前经验值（`self.model` 侧的值，经 [`add_exp`](Self::add_exp) 与库保持同步）。
    pub fn exp(&self) -> i64 {
        self.model.exp
    }

    /// 当前等级（由 [`exp`](Self::exp) 经 [`level_of`] 换算)。
    pub fn level(&self) -> i64 {
        level_of(self.exp())
    }

    /// 当前等级 + 级内进度快照（见 [`LevelInfo`]）。
    pub fn level_info(&self) -> LevelInfo {
        level_info(self.exp())
    }

    /// 该句柄持有的共享连接（内部 `Arc`，克隆廉价）。已持 `AUser` 的 handler 不必再单取
    /// [`Db`](crate::data::Db)——直接 `user.db()` 拿这份同一连接。
    pub fn db(&self) -> &DatabaseConnection {
        &self.db
    }

    /// 按 `uin` 取用户：命中即包成句柄；缺失则插一行默认值（库侧缺省填 coin=10 等）再返回。
    ///
    /// 并发下两个 `get` 可能同时见 `None` 都去 `insert`——主键冲突时回退一次 `find_by_id`
    /// 取对方刚插的行，故仍稳定返回一行（不向调用方抛主键冲突）。取或建走共用的
    /// [`get_or_insert`](crate::data::util::get_or_insert)；只在**真·首次**插入成功时记
    /// 「新用户注册」（竞态败者走回读分支、由抢先者那侧记过，不重复记）。
    pub async fn get(db: &DatabaseConnection, uin: i64) -> Result<Self> {
        let (model, fresh) = crate::data::util::get_or_insert::<user::Entity, _>(
            db,
            uin,
            // 缺失时：插一行只给主键的默认用户（其余字段由库侧缺省填充）。
            || user::ActiveModel { uin: Set(uin), ..default_active() },
            "用户",
        )
        .await?;
        if fresh {
            // 新用户是低频且值得知道的事 → info。
            tracing::info!(uin, coin = model.coin, "新用户注册");
        }
        Ok(Self { model, db: db.clone() })
    }

    /// 批量按 `uin` 取用户：一条 `WHERE uin IN (..)` 查出已存在的，缺失的批量插默认行，返回的
    /// 句柄顺序与 `uins` 一致（去重后每个 uin 一个句柄）。空输入返回空 vec。
    pub async fn get_many(db: &DatabaseConnection, uins: &[i64]) -> Result<Vec<Self>> {
        if uins.is_empty() {
            return Ok(Vec::new());
        }

        // 去重并记住首现顺序（HashSet 判重，避免 `order.contains` 的 O(n²)）。
        let mut seen = std::collections::HashSet::with_capacity(uins.len());
        let mut order: Vec<i64> = Vec::with_capacity(uins.len());
        for &u in uins {
            if seen.insert(u) {
                order.push(u);
            }
        }

        // 一条 IN 查询取出所有已存在的行。
        let existing = user::Entity::find()
            .filter(user::Column::Uin.is_in(order.iter().copied()))
            .all(db)
            .await
            .context("批量查询用户")?;

        use std::collections::HashMap;
        let mut by_uin: HashMap<i64, user::Model> = existing.into_iter().map(|m| (m.uin, m)).collect();

        // 缺失的批量插（一条 INSERT .. VALUES (..),(..)），拿回带库侧缺省的行。
        let missing: Vec<i64> = order.iter().copied().filter(|u| !by_uin.contains_key(u)).collect();
        if !missing.is_empty() {
            let ams = missing.iter().map(|&u| user::ActiveModel { uin: Set(u), ..default_active() });
            // 并发下可能与别处插入撞主键；用 on_conflict do nothing 容忍，再统一回读缺失项。
            use sea_orm::sea_query::OnConflict;
            user::Entity::insert_many(ams)
                .on_conflict(OnConflict::column(user::Column::Uin).do_nothing().to_owned())
                .do_nothing()
                .exec(db)
                .await
                .context("批量插入用户")?;

            tracing::info!(count = missing.len(), uins = ?missing, "新用户注册(批量)");

            let inserted = user::Entity::find()
                .filter(user::Column::Uin.is_in(missing.iter().copied()))
                .all(db)
                .await
                .context("批量插入后回读用户")?;
            for m in inserted {
                by_uin.insert(m.uin, m);
            }
        }

        // 按首现顺序组装句柄。任何 uin 此时都应已在 map 里。
        let mut out = Vec::with_capacity(order.len());
        for u in order {
            let model = by_uin.remove(&u).with_context(|| format!("用户 {u} 取或建后仍缺失"))?;
            out.push(Self { model, db: db.clone() });
        }
        Ok(out)
    }

    /// 原子增减金币（`UPDATE coin = coin + delta`，**绝不**读改写）+ 追一行 [`coin_log`]，最后同步
    /// `self.model.coin`。`delta` 可正可负；**花费请用 [`pay`](Self::pay)**（带闸防超支），本方法只
    /// 用于奖励/无条件加减（罚款等已知封顶的扣减也可，但不带余额下限保护）。
    pub async fn add_coin(&mut self, delta: i64, reason: impl Into<String>) -> Result<()> {
        add_coin_on(&self.db, self.model.uin, delta, reason.into()).await?;
        self.model.coin += delta; // 镜像内存侧
        Ok(())
    }

    /// 原子增减经验（`UPDATE exp = exp + delta`，非读改写）+ 同步句柄，返回前后等级对照
    /// [`LevelChange`]。经验不审计敏感，**不**写流水。
    pub async fn add_exp(&mut self, delta: i64) -> Result<LevelChange> {
        let before = level_of(self.model.exp);
        user::Entity::update_many()
            .col_expr(user::Column::Exp, Expr::col(user::Column::Exp).add(delta))
            .filter(user::Column::Uin.eq(self.model.uin))
            .exec(&self.db)
            .await
            .context("原子加经验")?;
        self.model.exp += delta; // 镜像内存侧
        let after = level_of(self.model.exp);
        Ok(LevelChange { before, after })
    }

    /// **带闸花费**：余额够则原子扣 `amount`（带 `coin_log`）、同步句柄、返回 `true`；不够则一动不动、
    /// 返回 `false`。`amount` 应 ≥ 0。SQL 层 `WHERE coin >= amount` 杜绝 check-then-act 超支——这是
    /// 所有花费的唯一口径。需要把扣款与别的表写入放进**同一事务**时，在事务上调 `try_debit_on`。
    pub async fn pay(&mut self, amount: i64, reason: impl Into<String>) -> Result<bool> {
        let ok = try_debit_on(&self.db, self.model.uin, amount, reason.into()).await?;
        if ok {
            self.model.coin -= amount; // 镜像内存侧
        }
        Ok(ok)
    }

    /// **原子转账**：一个事务里带闸扣本人 + 入账对方（各一行 `coin_log`）。够则成、同步本人句柄、
    /// 返回 `true`；不够则整体回滚、返回 `false`。`amount` 应 ≥ 0；对方行不存在会先取或建。
    pub async fn transfer_to(&mut self, target: i64, amount: i64, reason: impl Into<String>) -> Result<bool> {
        let reason = reason.into();
        // 确保对方行存在（否则入账影响 0 行 → add_coin_on 报错）。
        AUser::get(&self.db, target).await?;
        let txn = self.db.begin().await.context("开启转账事务")?;
        if !try_debit_on(&txn, self.model.uin, amount, reason.clone()).await? {
            txn.rollback().await.ok();
            return Ok(false); // 余额不足，整体回滚
        }
        add_coin_on(&txn, target, amount, reason).await?;
        txn.commit().await.context("提交转账")?;
        self.model.coin -= amount; // 镜像内存侧
        Ok(true)
    }
}

/// 在任意连接/事务上对一个 uin 做原子加币 + 写流水（[`AUser::add_coin`] / `transfer_to` 入账侧共用）。
///
/// 目标行不存在（加币影响 0 行）→ 直接报错，**绝不**写「幽灵流水」（记了账却没落到任何余额）。
/// 调用方应先经 `get`/`get_many` 建行。`conn` 可为 `&DatabaseConnection` 或 `&DatabaseTransaction`。
pub(crate) async fn add_coin_on<C: ConnectionTrait>(conn: &C, uin: i64, delta: i64, reason: String) -> Result<()> {
    // RETURNING 取回同一条原子更新后的整行——流水的 balance 与 delta 严格对应,
    // 不另查一遍(并发下另查会读到别笔变动后的值)。
    let rows = user::Entity::update_many()
        .col_expr(user::Column::Coin, Expr::col(user::Column::Coin).add(delta))
        .filter(user::Column::Uin.eq(uin))
        .exec_with_returning(conn)
        .await
        .context("原子加币")?;
    let Some(row) = rows.first() else {
        return Err(Error::action(format!("加币目标用户 {uin} 不存在，未落账")));
    };
    coin_log::ActiveModel {
        id: NotSet,
        uin: Set(uin),
        delta: Set(delta),
        balance: Set(row.coin),
        reason: Set(reason),
        at: NotSet,
    }
    .insert(conn)
    .await
    .context("写金币流水")?;
    Ok(())
}

/// **带闸扣款**：`UPDATE coin = coin - amount WHERE uin = ? AND coin >= amount`，SQL 一句保证「够才扣」。
///
/// `amount` 应 ≥ 0。够则扣 + 记一行 `coin_log`（`delta = -amount`）并返回 `true`；不够则一动不动、返回
/// `false`。可跑在连接或事务上（与扣款的其它步骤同事务原子提交）。`AUser::pay`/`transfer_to`/place 落格
/// 都走它——花费防超支的唯一底座。
pub(crate) async fn try_debit_on<C: ConnectionTrait>(conn: &C, uin: i64, amount: i64, reason: String) -> Result<bool> {
    debug_assert!(amount >= 0, "try_debit_on 的 amount 应非负");
    // RETURNING 同 add_coin_on:扣款后余额随同一条原子更新取回,进流水的 balance。
    let rows = user::Entity::update_many()
        .col_expr(user::Column::Coin, Expr::col(user::Column::Coin).sub(amount))
        .filter(user::Column::Uin.eq(uin))
        .filter(user::Column::Coin.gte(amount))
        .exec_with_returning(conn)
        .await
        .context("带闸扣款")?;
    let Some(row) = rows.first() else {
        return Ok(false); // 余额不足（或用户不存在）——未扣、未记账
    };
    coin_log::ActiveModel {
        id: NotSet,
        uin: Set(uin),
        delta: Set(-amount),
        balance: Set(row.coin),
        reason: Set(reason),
        at: NotSet,
    }
    .insert(conn)
    .await
    .context("写扣款流水")?;
    Ok(true)
}

/// 一行「只给主键、其余靠库侧缺省」的用户 `ActiveModel` 骨架（主键由调用方 `Set`）。
fn default_active() -> user::ActiveModel {
    user::ActiveModel {
        uin: NotSet,
        id: NotSet, // 站内 UID 由库侧序列发号
        coin: NotSet,
        nickname: NotSet,
        exp: NotSet,
        banned: NotSet,
        theme: NotSet,
        theme_color: NotSet,
        join_time: NotSet,
    }
}

/// 提取器：把**消息发送者**取或建成 `AUser`。非消息事件 → `Skip`；连接缺失或建号出错 → `Reject::Error`。
#[async_trait]
impl FromContext for AUser {
    async fn from_context(ctx: &Ctx) -> Extracted<Self> {
        let sender = ctx.message().map(|m| m.sender).ok_or(Reject::Skip)?;
        let db = State::<DatabaseConnection>::from_context(ctx).await?;
        AUser::get(&db, sender.0).await.map_err(Reject::Error)
    }
}
