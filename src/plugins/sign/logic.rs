//! 签到结算逻辑 —— **全部从 `sign_log` 流水派生**,不设汇总行(单一真相,与 chatlog
//! 发言数 `COUNT(*)` 派生同款口径):
//!
//! - 去重:今天(业务日)已有行 → [`SignOutcome::Already`];并发兜底靠 `(uin, day)`
//!   主键,撞键回读确认后同样归 `Already`。
//! - 连签:从今天往回数 `sign_log` 里连续的天数([`streak_ending_at`])。
//! - 累计:行数;首签:此前无行。
//!
//! 入口 [`do_sign`]:读该用户全部签到日(降序)→ 结算 → 插当日流水行 → 经
//! [`AUser::add_coin`] / [`AUser::add_exp`] 原子发奖。触碰共享经济只走 `AUser` 句柄,
//! 本逻辑不直接动 `user` 表的任何列。
//!
//! 「当日」= 全 bot 统一的业务日口径(凌晨 4 点刷新,见
//! [`business_day`](crate::data::util::business_day)),去重与连签都按此口径。

use nagisa::prelude::*;
use rand::RngExt as _;
use sea_orm::{
    ActiveModelTrait, ActiveValue::NotSet, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder,
    QuerySelect, Set,
};

use crate::data::AUser;
use crate::data::level::{LevelChange, LevelInfo};
use crate::plugins::sign::entity::log;

/// 签到结果:今天已签到(`Already`)或本次签到完成(`Done`)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignOutcome {
    /// 今天已经签过到了(当日已有流水行),未发放奖励。
    Already,
    /// 本次签到完成:携带呈现所需的全部结算数据(金币各分项 + 经验 + 等级)。
    Done {
        /// 本次发放的金币总数(基础 + 连签加成 + 手气 + 里程碑 + 首签 + 大奖)。
        gold_add: i64,
        /// 金币分项:基础随机额。
        base: i64,
        /// 金币分项:连签加成(`min(连签, 30) * 2`)。
        streak_bonus: i64,
        /// 金币分项:手气随机额。
        luck: i64,
        /// 包含本次的连续签到天数。
        continue_sign: i32,
        /// 包含本次的累计签到次数。
        total_sign: i32,
        /// 里程碑奖励:恰好在第 7 / 30 / 100 天命中(一次性,否则 0)。
        milestone: i64,
        /// 是否为该账号有史以来的首次签到(此前无流水行,礼金 [`FIRST_GIFT`])。
        first_sign: bool,
        /// 是否抽中「大奖」(额外 +[`JACKPOT_GOLD`])。
        jackpot: bool,
        /// 本次获得的经验值。
        exp_gain: i64,
        /// 加经验前后的等级对照(用于判断是否升级)。
        level_change: LevelChange,
        /// 加经验后的级内进度快照(当前级 / 级内进度 / 台阶宽度)。
        level_info: LevelInfo,
    },
}

/// 落账原因(写入 `coin_log.reason`)。
const SIGN_REASON: &str = "签到";

/// 「大奖」中奖概率。中奖额外 +[`JACKPOT_GOLD`] 金币。稀有度只调这一个常量。
const JACKPOT_PROB: f64 = 0.003;
/// 「大奖」奖金。
pub const JACKPOT_GOLD: i64 = 666;
/// 首次签到礼金。
pub const FIRST_GIFT: i64 = 66;

/// 查一个用户的全部签到日,**降序**(连签 / 累计 / 日历共用的一把数据)。
/// 一行只取 `day` 一列;十年天天签也就三千余行,在 `(uin, day)` 主键上顺序扫。
pub async fn days_desc(db: &DatabaseConnection, uin: i64) -> Result<Vec<chrono::NaiveDate>> {
    log::Entity::find()
        .select_only()
        .column(log::Column::Day)
        .filter(log::Column::Uin.eq(uin))
        .order_by_desc(log::Column::Day)
        .into_tuple::<chrono::NaiveDate>()
        .all(db)
        .await
        .context("查签到流水失败")
}

/// 以 `from` 为末日往回数连续天数:`from` 本身没签返回 0。`days_desc` 须降序。
pub fn streak_ending_at(days_desc: &[chrono::NaiveDate], from: chrono::NaiveDate) -> i32 {
    let mut expect = from;
    let mut n = 0;
    for &d in days_desc {
        if d > expect {
            continue; // 比目标新的日子(如已含今天而 from 是昨天)跳过
        }
        if d == expect {
            n += 1;
            expect -= chrono::Duration::days(1);
        } else {
            break; // 出现空洞,连续段到此为止
        }
    }
    n
}

/// **当下有效的**连签:今天签了从今天数;今天没签但昨天签了,从昨天数(连签还活着);
/// 否则 0。
pub fn live_streak(days_desc: &[chrono::NaiveDate], today: chrono::NaiveDate) -> i32 {
    let s = streak_ending_at(days_desc, today);
    if s > 0 { s } else { streak_ending_at(days_desc, today - chrono::Duration::days(1)) }
}

/// 每日签到。同一「签到日」(业务日口径,凌晨 4 点边界)重复调用返回
/// [`SignOutcome::Already`]、不重复发奖;否则按流水历史结算并经
/// `AUser::add_coin`/`add_exp` 原子发奖:
///
/// - 连签:昨日为末的连续天数 + 1(断签或首签即 1)。
/// - 金币:`base(8..=18)` + `连签加成(min(连签,30)*2)` + `手气(0..=15)` + `里程碑` +
///   `首签礼(+66)` + `大奖(0.3% → +666)`。里程碑只在恰好第 7/30/100 天命中一次
///   (7→20、30→88、100→200)。
/// - 经验:`10 + min(连签,30) + (0..=5)`,经 `add_exp` 原子自加并取回等级变化。
///
/// 并发下两次同日签到都会试插同一 `(uin, day)` 行:败者撞主键,回读确认行已存在后
/// 归 `Already`,不重复发奖。
pub async fn do_sign(db: &DatabaseConnection, user: &mut AUser) -> Result<SignOutcome> {
    let uin = user.uin();
    let today = crate::data::util::business_day();

    let days = days_desc(db, uin).await?;
    if days.first() == Some(&today) {
        return Ok(SignOutcome::Already);
    }

    // 全部从历史派生:连签 = 昨日为末的连续段 + 今天,累计 = 行数 + 今天,首签 = 此前无行。
    let continue_sign = streak_ending_at(&days, today - chrono::Duration::days(1)) + 1;
    let total_sign = days.len() as i32 + 1;
    let first_sign = days.is_empty();

    // 里程碑:只在恰好达到第 7 / 30 / 100 天那一次发(一次性,非每 7/30 的倍数)。
    let milestone: i64 = match continue_sign {
        100 => 200,
        30 => 88,
        7 => 20,
        _ => 0,
    };

    // 把随机抽样全收进一个块里——`ThreadRng` 非 `Send`,绝不能跨 `.await` 持有(否则
    // handler future 非 `Send`),故在任何 `.await` 之前就把它取样成纯 `i64`/`bool` 并随
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

    // 插当日流水行——这是签到唯一的状态写入,也是并发去重闸:撞 (uin, day) 主键说明
    // 另一次同日签到抢先,回读确认后按 Already 处理(不重复发奖)。
    let insert =
        log::ActiveModel { uin: Set(uin), day: Set(today), gold: Set(gold_add), exp: Set(exp_gain), at: NotSet }
            .insert(db)
            .await;
    if let Err(e) = insert {
        if log::Entity::find_by_id((uin, today)).one(db).await.ok().flatten().is_some() {
            return Ok(SignOutcome::Already);
        }
        return Err(e).context("写签到流水失败");
    }

    // 经共享经济 API 原子发奖 + 记账(同步 user.model.coin)。
    user.add_coin(gold_add, SIGN_REASON).await?;
    // 经验也是跨插件共享属性,经 `add_exp` 原子自加并取回前后等级对照。
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

/// 一个用户某月的签到日历数据:当月签过的「业务日」序列(升序)+ 派生汇总(连签 / 累计)。
pub struct CalendarData {
    /// 当月签过的日期(业务日口径,升序)。
    pub days: Vec<chrono::NaiveDate>,
    /// 当下有效的连续签到天数(见 [`live_streak`])。
    pub continue_sign: i32,
    /// 累计签到次数(流水行数)。
    pub total_sign: i32,
}

/// 查一个用户 `year-month` 月的签到日历数据——一把流水全取,当月落格与汇总同源派生。
/// `today` 是业务日的「今天」,用来算有效连签。没签过也正常返回(空序列 + 0 汇总),
/// 呈现层画空日历。
pub async fn calendar_data(
    db: &DatabaseConnection,
    uin: i64,
    year: i32,
    month: u32,
    today: chrono::NaiveDate,
) -> Result<CalendarData> {
    use chrono::Datelike;

    let all = days_desc(db, uin).await?;
    let total_sign = all.len() as i32;
    let continue_sign = live_streak(&all, today);
    let days = all.into_iter().rev().filter(|d| d.year() == year && d.month() == month).collect();
    Ok(CalendarData { days, continue_sign, total_sign })
}
