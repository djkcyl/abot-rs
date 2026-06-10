//! 签到结算逻辑 —— **签到插件自有**的连签/去重/发奖，全在这里（不在核心 `AUser` 上）。
//!
//! 入口 [`do_sign`]：按 `uin` 取或建签到行 → 当日去重（`last_sign == 当日` → `Already`）→
//! 否则按「连签」结算（恰好前一天签过则 +1，否则归 1）→ 更新 `sign` 行 → 经
//! [`AUser::add_coin`] 原子发金币 + 经 [`AUser::add_exp`] 原子发经验 → 返回 [`SignOutcome`]。
//!
//! 触碰共享经济只走 `AUser` 句柄方法（金币 `add_coin` 原子自加 + 写 `coin_log`、经验
//! `add_exp` 原子自加），本逻辑不直接动 `user` 表的任何列；签到的私有状态全落在 `sign` 表。
//! 经验/等级是**跨插件共享**属性，落在核心 `user` 上（非签到私有），故经 `AUser` 触碰。
//!
//! 「当日」= 全 bot 统一的业务日口径(凌晨 4 点刷新,见
//! [`business_day`](crate::data::util::business_day)),去重与连签都按此口径。

use nagisa::prelude::*;
use rand::RngExt as _;
use sea_orm::{ActiveModelTrait, ActiveValue::NotSet, DatabaseConnection, Set};

use crate::data::level::{LevelChange, LevelInfo};
use crate::data::AUser;
use crate::plugins::sign::entity as sign;

/// 签到结果：今天已签到（`Already`）或本次签到完成（`Done`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignOutcome {
    /// 今天已经签过到了（`last_sign == 当日`），未发放奖励。
    Already,
    /// 本次签到完成：携带呈现所需的全部结算数据（金币各分项 + 经验 + 等级）。
    Done {
        /// 本次发放的金币总数（基础 + 连签加成 + 手气 + 里程碑 + 首签 + 大奖）。
        gold_add: i64,
        /// 金币分项：基础随机额。
        base: i64,
        /// 金币分项：连签加成（`min(连签, 30) * 2`）。
        streak_bonus: i64,
        /// 金币分项：手气随机额。
        luck: i64,
        /// 包含本次的连续签到天数。
        continue_sign: i32,
        /// 包含本次的累计签到次数。
        total_sign: i32,
        /// 里程碑奖励：恰好在第 7 / 30 / 100 天命中（一次性，否则 0）。
        milestone: i64,
        /// 是否为该账号有史以来的首次签到（`total_sign == 1`，礼金 [`FIRST_GIFT`]）。
        first_sign: bool,
        /// 是否抽中「大奖」（额外 +[`JACKPOT_GOLD`]）。
        jackpot: bool,
        /// 本次获得的经验值。
        exp_gain: i64,
        /// 加经验前后的等级对照（用于判断是否升级）。
        level_change: LevelChange,
        /// 加经验后的级内进度快照（当前级 / 级内进度 / 台阶宽度）。
        level_info: LevelInfo,
    },
}

/// 落账原因（写入 `coin_log.reason`）。
const SIGN_REASON: &str = "签到";

/// 「大奖」中奖概率。中奖额外 +[`JACKPOT_GOLD`] 金币。稀有度只调这一个常量。
const JACKPOT_PROB: f64 = 0.003;
/// 「大奖」奖金。
pub const JACKPOT_GOLD: i64 = 666;
/// 首次签到礼金。
pub const FIRST_GIFT: i64 = 66;

/// 按 `uin` 取或建签到行：命中即返回；缺失则插一行默认值（计数取库侧缺省 0）再返回。
///
/// 并发下与 [`AUser::get`] 同策略（共用 [`get_or_insert`](crate::data::util::get_or_insert)）：
/// 插入撞主键时回读对方刚插的行，故稳定返回一行。本表无「首建」副作用，故忽略 `fresh` 标志。
async fn get_or_create(db: &DatabaseConnection, uin: i64) -> Result<sign::Model> {
    let (model, _fresh) = crate::data::util::get_or_insert::<sign::Entity, _>(
        db,
        uin,
        || sign::ActiveModel {
            uin: Set(uin),
            last_sign: NotSet,
            continue_sign: NotSet,
            total_sign: NotSet,
        },
        "签到行",
    )
    .await?;
    Ok(model)
}

/// 每日签到。同一「签到日」（业务日口径，凌晨 4 点边界）重复调用返回
/// [`SignOutcome::Already`]、不重复发奖；否则结算并经 `AUser::add_coin`/`add_exp`
/// 原子发奖：
///
/// - 连签：恰好前一天签过 → +1；否则（首签或断签）归 1。
/// - 金币：`base(8..=18)` + `连签加成(min(连签,30)*2)` + `手气(0..=15)` + `里程碑` +
///   `首签礼(+66)` + `大奖(1% → +666)`。里程碑只在恰好第 7/30/100 天命中一次
///   （7→20、30→88、100→200）。
/// - 经验：`10 + min(连签,30) + (0..=5)`，经 `add_exp` 原子自加并取回等级变化。
///
/// `user` 是发签到奖励的对象句柄（金币经 `add_coin` 原子自加 + 写 `coin_log`，经验经
/// `add_exp` 原子自加）；按其 `uin` 取或建对应的 `sign` 行。
pub async fn do_sign(db: &DatabaseConnection, user: &mut AUser) -> Result<SignOutcome> {
    let uin = user.uin();
    let row = get_or_create(db, uin).await?;
    let today = crate::data::util::business_day();

    if row.last_sign == Some(today) {
        return Ok(SignOutcome::Already);
    }

    // 连签：恰好前一天签过 → +1；否则（首签或断签）重置为 1。
    let continue_sign = match row.last_sign {
        Some(prev) if prev == today - chrono::Duration::days(1) => row.continue_sign + 1,
        _ => 1,
    };
    let total_sign = row.total_sign + 1;
    let first_sign = total_sign == 1;

    // 里程碑：只在恰好达到第 7 / 30 / 100 天那一次发（一次性，非每 7/30 的倍数）。
    let milestone: i64 = match continue_sign {
        100 => 200,
        30 => 88,
        7 => 20,
        _ => 0,
    };

    // 把随机抽样全收进一个块里——`ThreadRng` 非 `Send`，绝不能跨 `.await` 持有（否则
    // handler future 非 `Send`），故在任何 `.await` 之前就把它取样成纯 `i64`/`bool` 并随
    // 块结束 drop 掉。
    let (gold_add, base, streak_bonus, luck, jackpot, exp_gain) = {
        let mut rng = rand::rng();
        let base: i64 = rng.random_range(8..=18);
        let streak_bonus: i64 = (continue_sign as i64).min(30) * 2;
        let luck: i64 = rng.random_range(0..=15);
        let first_gift: i64 = if first_sign { FIRST_GIFT } else { 0 };
        let jackpot_hit = rng.random_bool(JACKPOT_PROB);
        let jackpot_gold: i64 = if jackpot_hit { JACKPOT_GOLD } else { 0 };
        let gold_add = base + streak_bonus + luck + milestone + first_gift + jackpot_gold;
        let exp_gain = 10 + (continue_sign as i64).min(30) + rng.random_range(0..=5);
        (gold_add, base, streak_bonus, luck, jackpot_hit, exp_gain)
    };

    // 落签到字段（last_sign/continue/total）到本插件自有的 sign 表。
    let mut am: sign::ActiveModel = row.into();
    am.last_sign = Set(Some(today));
    am.continue_sign = Set(continue_sign);
    am.total_sign = Set(total_sign);
    am.update(db).await.context("更新签到行失败")?;

    // 经共享经济 API 原子发奖 + 记账（同步 user.model.coin）。
    user.add_coin(gold_add, SIGN_REASON).await?;
    // 经验也是跨插件共享属性，经 `add_exp` 原子自加并取回前后等级对照。
    let level_change = user.add_exp(exp_gain).await?;
    let level_info = user.level_info();

    Ok(SignOutcome::Done {
        gold_add,
        base,
        streak_bonus,
        luck,
        continue_sign,
        total_sign,
        milestone,
        first_sign,
        jackpot,
        exp_gain,
        level_change,
        level_info,
    })
}
