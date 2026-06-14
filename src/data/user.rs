//! `AUser` —— 一个用户的「数据 API」句柄：包住一行 [`user::Model`] + 一份连接，方法直接在其上
//! 跑。**不是**仓储/DAO/Service——句柄本身就是 API，没有任何中间层包装。
//!
//! 暴露的就这几样跨插件共享的经济 API：读 `coin/exp/level`、`add_coin`（奖励/无条件加减）、
//! `add_exp`、`pay`（带闸花费）、`transfer_to`（原子转账）。插件私有的状态与逻辑（如签到连签）
//! 归各插件自己，只经这些方法触碰共享经济。
//!
//! 设计要点：
//! - 游戏币改动一律走**原子增量** `col_expr(Coin, Expr::col(Coin) ± delta)`，绝不 read-modify-write。
//! - **花费**一律走带闸 `WHERE coin >= amount`（[`pay`](AUser::pay)/[`transfer_to`](AUser::transfer_to)），
//!   从根上杜绝 check-then-act 超支。
//! - 方法返回 `nagisa::Result`（内部用 nagisa 的 [`Context::context`] 转错），handler 直接 `?`。

use nagisa::prelude::*;
use sea_orm::sea_query::Expr;
use sea_orm::{
    ActiveModelTrait, ActiveValue::NotSet, ColumnTrait, ConnectionTrait, DatabaseConnection, EntityTrait, QueryFilter,
    QuerySelect, Set, TransactionTrait,
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

    /// 自设昵称（`user.alias`，空串 = 未设）。呈现时优先级最高，经「改名」命令改。
    pub fn alias(&self) -> &str {
        &self.model.alias
    }

    /// 改自设昵称（调用方先校验长度，这里原样落库并同步 `self.model`；传空串即清除）。
    pub async fn set_alias(&mut self, alias: &str) -> Result<()> {
        user::Entity::update_many()
            .col_expr(user::Column::Alias, Expr::value(alias))
            .filter(user::Column::Uin.eq(self.model.uin))
            .exec(&self.db)
            .await
            .context("写自设昵称失败")?;
        self.model.alias = alias.to_string();
        Ok(())
    }

    /// 自设昵称颜色（`user.alias_color`，`#rrggbb` 原始色相，空串 = 不上色）。出图时经
    /// [`imaging::readable_hex`](crate::imaging::readable_hex) 收对比，只在显示名取自
    /// [`alias`](Self::alias) 时生效；经「昵称颜色」命令改。
    pub fn alias_color(&self) -> &str {
        &self.model.alias_color
    }

    /// 改自设昵称颜色（调用方先归一成 `#rrggbb` 或空串，这里原样落库并同步 `self.model`；
    /// 传空串即清除上色）。
    pub async fn set_alias_color(&mut self, color: &str) -> Result<()> {
        user::Entity::update_many()
            .col_expr(user::Column::AliasColor, Expr::value(color))
            .filter(user::Column::Uin.eq(self.model.uin))
            .exec(&self.db)
            .await
            .context("写自设昵称颜色失败")?;
        self.model.alias_color = color.to_string();
        Ok(())
    }

    /// 当前游戏币余额（`self.model` 侧的值，经各写方法与库保持同步）。
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

    /// 按 `uin` 取用户但**绝不建档**：命中包成句柄，缺失返回 `None`。专给「只对已有用户动手」的
    /// 场景——如管理员调账，不能给一个素未谋面的 QQ 号凭空建号。寻常路径仍用取或建的 [`get`](Self::get)。
    pub async fn find(db: &DatabaseConnection, uin: i64) -> Result<Option<Self>> {
        let model = user::Entity::find_by_id(uin).one(db).await.context("查询用户")?;
        Ok(model.map(|model| Self { model, db: db.clone() }))
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

    /// 原子增减游戏币（`UPDATE coin = coin + delta`，**绝不**读改写）+ 追一行 [`coin_log`]，最后同步
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

    /// **设额 / 带地板扣减的底座（锁行读-改-写）**：在一个事务里对本行排他锁定（`SELECT … FOR
    /// UPDATE`）、读到不会被并发盖掉的旧值，用 `new_from_old(旧)` 算新余额（夹到 ≥ 0），仅当与旧值
    /// 有差（`delta ≠ 0`）才落库 + 追一行 [`coin_log`]（`delta` 带符号、`balance` = 新值），最后同步
    /// `self.model.coin`。返回 `(旧, 新)` 余额。
    ///
    /// 与纯原子增量的 [`add_coin`](Self::add_coin) 不同：**设成某值 / 扣到地板**都得先知道旧值才能
    /// 算新值，故必须锁行（否则并发下旧值会漂、`delta` 与 `balance` 失配）。专供管理员低频调整，普通
    /// 经济路径仍走原子增量 / 带闸扣款。
    async fn modify_coin_locked<F>(&mut self, reason: String, new_from_old: F) -> Result<(i64, i64)>
    where
        F: FnOnce(i64) -> i64 + Send,
    {
        let txn = self.db.begin().await.context("开启改额事务")?;
        // 排他锁定目标行，读到准确旧值（FOR UPDATE）。
        let row = user::Entity::find_by_id(self.model.uin)
            .lock_exclusive()
            .one(&txn)
            .await
            .context("锁定用户行")?
            .ok_or_else(|| Error::action(format!("改额目标用户 {} 不存在", self.model.uin)))?;
        let old = row.coin;
        let new = new_from_old(old).max(0); // 余额永不为负
        let delta = new - old;
        if delta != 0 {
            user::Entity::update_many()
                .col_expr(user::Column::Coin, Expr::value(new))
                .filter(user::Column::Uin.eq(self.model.uin))
                .exec(&txn)
                .await
                .context("写新余额")?;
            coin_log::ActiveModel {
                id: NotSet,
                uin: Set(self.model.uin),
                delta: Set(delta),
                balance: Set(new),
                reason: Set(reason),
                at: NotSet,
            }
            .insert(&txn)
            .await
            .context("写改额流水")?;
        }
        txn.commit().await.context("提交改额")?;
        self.model.coin = new; // 镜像内存侧
        Ok((old, new))
    }

    /// **设额**（管理员）：把余额原子设成 `target`（负数夹到 0），追一行 [`coin_log`]（`reason` 由调用方
    /// 给）。返回应用前后的 `(旧, 新)`；设成原值（`delta = 0`）则不写流水。寻常加减**不要**用它——那是
    /// 原子增量 [`add_coin`](Self::add_coin) / 带闸扣款 [`pay`](Self::pay) 的活。
    pub async fn set_coin(&mut self, target: i64, reason: impl Into<String>) -> Result<(i64, i64)> {
        self.modify_coin_locked(reason.into(), move |_old| target).await
    }

    /// **带地板扣减**（管理员）：扣 `min(amount, 余额)`——够则扣满 `amount`、不够则扣到 0 为止，**绝不**
    /// 让余额变负。`amount` 应 ≥ 0。追一行 [`coin_log`]（`reason` 由调用方给）。返回**实际**扣减额
    /// （≥ 0；对方已是 0 则为 0、不写流水）。带闸花费仍走 [`pay`](Self::pay)（不够则一动不动），本方法是
    /// 管理员「能扣多少扣多少」的钝刀。
    pub async fn deduct_floor(&mut self, amount: i64, reason: impl Into<String>) -> Result<i64> {
        let (old, new) = self.modify_coin_locked(reason.into(), move |old| old - amount).await?;
        Ok(old - new)
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
    .context("写游戏币流水")?;
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
        alias: NotSet,
        alias_color: NotSet,
        exp: NotSet,
        banned: NotSet,
        theme: NotSet,
        theme_color: NotSet,
        join_time: NotSet,
    }
}

/// 提取器：把**消息发送者**取或建成 `AUser`。非消息事件 → `Skip`；连接缺失或建号出错 → `Reject::Error`。
/// 发送者的昵称 / 群名片缓存由 `chatlog` 的每条消息钩子统一同步（见 [`crate::data::identity`]），
/// 不在此处写，故本提取器只负责取或建。
#[async_trait]
impl FromContext for AUser {
    async fn from_context(ctx: &Ctx) -> Extracted<Self> {
        let sender = ctx.message().map(|m| m.sender).ok_or(Reject::Skip)?;
        let db = State::<DatabaseConnection>::from_context(ctx).await?;
        AUser::get(&db, sender.0).await.map_err(Reject::Error)
    }
}
