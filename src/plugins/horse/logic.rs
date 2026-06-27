//! 赛马数据层 API + 纯函数。共享经济(报名费/奖励)由 [`mod`](super) 经 `AUser` 处理,本模块不碰金币。
//! 训练增量(点)= `档值 × growth × exp(-当前值/reach) × 全局调教衰减 × 寿命效率`,floor-0:当前值接近 reach
//! 衰减到约 0(单维软墙),总调教次数越多再整体打折(见 [`train_total_decay`])。五维落库为厘点
//! (×[`STAT_SCALE`](consts::STAT_SCALE)),亚点增量靠它累积不丢;[`stats_of`] 读出折回点数。

use std::collections::HashSet;

use chrono::NaiveDate;
use nagisa::prelude::*;
use rand::RngExt as _;
use rand::rngs::StdRng;
use sea_orm::prelude::DateTimeWithTimeZone;
use sea_orm::sea_query::{Expr, OnConflict};
use sea_orm::{
    ActiveModelTrait, ActiveValue::NotSet, ColumnTrait, ConnectionTrait, DatabaseConnection, EntityTrait, QueryFilter,
    QueryOrder, QuerySelect, Set, Statement,
};

use super::consts::{self, Achievement, Item, Stat, Trait};
use super::entity::{achievement, gacha, horse};

/// 近似高斯抽样(三均匀和,免引 rand_distr):均值 `mean`、标准差约 `sigma`。
fn gauss(mean: f32, sigma: f32, rng: &mut StdRng) -> f32 {
    let a = (rng.random_range(0.0..1.0) + rng.random_range(0.0..1.0) + rng.random_range(0.0..1.0)) / 3.0;
    // (avg3-0.5) 标准差约 0.16667,除之归一再乘目标 sigma。
    mean + (a - 0.5) / 0.16667 * sigma
}

/// 按权重抽一个下标(返回 `0..weights.len()`)。空或全零时返 0。
pub(super) fn weighted_pick(weights: &[u32], rng: &mut StdRng) -> usize {
    let total: u32 = weights.iter().sum();
    if total == 0 {
        return 0;
    }
    let mut r = rng.random_range(0..total);
    for (i, &w) in weights.iter().enumerate() {
        if r < w {
            return i;
        }
        r -= w;
    }
    weights.len() - 1
}

/// 抽一个星级(1..=4)。`weights` 下标 = rarity-1。
pub fn roll_rarity(weights: &[u32; 4], rng: &mut StdRng) -> i16 {
    weighted_pick(weights, rng) as i16 + 1
}

/// 抽某维出生 reach,钳到 [`REACH_MIN`](consts::REACH_MIN)..本星基线+[`BIRTH_REACH_MARGIN`](consts::BIRTH_REACH_MARGIN)(出生不超基线太多,留繁殖填充空间)。
fn roll_reach_one(rarity: i16, sigma: f32, rng: &mut StdRng) -> i32 {
    let r = (rarity.clamp(consts::RARITY_MIN, consts::RARITY_MAX) - 1) as usize;
    let hi = (consts::REACH_BASELINE[r] + consts::BIRTH_REACH_MARGIN) as f32;
    gauss(consts::REACH_MEAN[r], sigma, rng).round().clamp(consts::REACH_MIN as f32, hi) as i32
}

/// 抽五维出生 reach,各维独立:小概率出「惊喜苗」(该维落在本星基线附近,罕见好苗),否则按星级带宽。
pub fn roll_reach(rarity: i16, rng: &mut StdRng) -> [i32; consts::STAT_COUNT] {
    let r = (rarity.clamp(consts::RARITY_MIN, consts::RARITY_MAX) - 1) as usize;
    std::array::from_fn(|_| {
        if rng.random_bool(consts::REACH_JACKPOT_PROB) {
            consts::REACH_BASELINE[r] + rng.random_range(consts::REACH_JACKPOT_BONUS.0..=consts::REACH_JACKPOT_BONUS.1)
        } else {
            roll_reach_one(rarity, consts::REACH_SIGMA[r], rng)
        }
    })
}

/// 抽每匹马的成长系数 `growth`(存整数 = growth×100)。
pub fn roll_growth(rarity: i16, rng: &mut StdRng) -> i32 {
    let r = (rarity.clamp(consts::RARITY_MIN, consts::RARITY_MAX) - 1) as usize;
    gauss(consts::GROWTH_MEAN[r], consts::GROWTH_SIGMA, rng)
        .round()
        .clamp(consts::GROWTH_MIN as f32, consts::GROWTH_MAX as f32) as i32
}

/// 出生当前值 = reach × [`BIRTH_REACH_RATIO`](consts::BIRTH_REACH_RATIO)。
fn birth_cur(reach: &[i32; consts::STAT_COUNT]) -> [i32; consts::STAT_COUNT] {
    std::array::from_fn(|i| (reach[i] as f32 * consts::BIRTH_REACH_RATIO).round() as i32)
}

/// 一匹新马的出生属性。`reach` 软上限落库进 `pot_*` 列。
pub struct Birth {
    pub rarity: i16,
    pub cur: [i32; consts::STAT_COUNT],
    pub reach: [i32; consts::STAT_COUNT],
    pub growth: i32,
    pub traits: i32,
}

/// 抽出生特性掩码:[`TRAIT_MAX`](consts::TRAIT_MAX) 个槽各按 [`TRAIT_BIRTH_PROB`](consts::TRAIT_BIRTH_PROB) 命中一条尚未拥有的随机特性。
pub fn roll_traits(rarity: i16, rng: &mut StdRng) -> i32 {
    let r = (rarity.clamp(consts::RARITY_MIN, consts::RARITY_MAX) - 1) as usize;
    let p = consts::TRAIT_BIRTH_PROB[r];
    let mut mask = 0i32;
    for _ in 0..consts::TRAIT_MAX {
        if !rng.random_bool(p) {
            continue;
        }
        let avail: Vec<Trait> = Trait::ALL.into_iter().filter(|t| !t.in_mask(mask)).collect();
        if !avail.is_empty() {
            mask |= avail[rng.random_range(0..avail.len())].bit();
        }
    }
    mask
}

/// 给定星级摇一份出生属性。
pub fn birth_for_rarity(rarity: i16, rng: &mut StdRng) -> Birth {
    let reach = roll_reach(rarity, rng);
    Birth { rarity, cur: birth_cur(&reach), reach, growth: roll_growth(rarity, rng), traits: roll_traits(rarity, rng) }
}

/// 抽一匹马的出生属性(星级用给定权重)。
pub fn roll_birth(rarity_weights: &[u32; 4], rng: &mut StdRng) -> Birth {
    birth_for_rarity(roll_rarity(rarity_weights, rng), rng)
}

/// 领养首马的出生属性:**固定 ★2**,reach 用窄带宽 [`STARTER_REACH_SIGMA`](consts::STARTER_REACH_SIGMA)。
pub fn roll_starter(rng: &mut StdRng) -> Birth {
    let rarity = consts::STARTER_RARITY;
    let reach: [i32; consts::STAT_COUNT] =
        std::array::from_fn(|_| roll_reach_one(rarity, consts::STARTER_REACH_SIGMA, rng));
    Birth { rarity, cur: birth_cur(&reach), reach, growth: roll_growth(rarity, rng), traits: roll_traits(rarity, rng) }
}

/// 建新马的入参(外观 + 属性 + 血统)。
pub struct NewHorse<'a> {
    pub owner_uin: i64,
    pub name: &'a str,
    pub birth: &'a Birth,
    pub color: i16,
    /// 0 公 / 1 母。
    pub sex: i16,
    pub generation: i32,
    /// 初代为 `(None, None)`。
    pub parents: (Option<i64>, Option<i64>),
    /// 生涯累计投入初值(领养/抽卡马 0;繁殖把繁殖费 + 星辉石回收价记到子代)。
    pub invested: i64,
}

/// 建一匹马并落库,返回行模型。
pub async fn create_horse(db: &DatabaseConnection, spec: NewHorse<'_>) -> Result<horse::Model> {
    let NewHorse { owner_uin, name, birth, color, sex, generation, parents, invested } = spec;
    let today = crate::data::util::business_day();
    // 寿命上限由幸运出生潜力(pot_luk)定;三寿命列出生即满。
    let lm = lifespan_max_for(birth.reach[Stat::Luk.idx()]);
    let am = horse::ActiveModel {
        id: NotSet,
        owner_uin: Set(owner_uin),
        name: Set(name.to_string()),
        color: Set(color),
        sex: Set(sex),
        generation: Set(generation),
        rarity: Set(birth.rarity),
        traits: Set(birth.traits),
        // birth.cur 是点数;落库存厘点(× STAT_SCALE)。
        spd: Set(birth.cur[0] * consts::STAT_SCALE),
        sta: Set(birth.cur[1] * consts::STAT_SCALE),
        brs: Set(birth.cur[2] * consts::STAT_SCALE),
        agi: Set(birth.cur[3] * consts::STAT_SCALE),
        luk: Set(birth.cur[4] * consts::STAT_SCALE),
        pot_spd: Set(birth.reach[0]),
        pot_sta: Set(birth.reach[1]),
        pot_brs: Set(birth.reach[2]),
        pot_agi: Set(birth.reach[3]),
        pot_luk: Set(birth.reach[4]),
        growth: Set(birth.growth),
        vitality: Set(consts::VIT_MAX),
        satiety: Set(100),
        state_at: NotSet,
        lifespan: Set(lm),
        lifespan_cap: Set(lm),
        lifespan_max: Set(lm),
        injury: Set(0),
        injury_until: Set(None),
        scar: Set(0),
        scar_until: Set(None),
        breed_cd_until: Set(None),
        breed_count: Set(0),
        status: Set(0),
        wins: Set(0),
        races: Set(0),
        train_day: Set(today),
        train_today: Set(0),
        race_day: Set(today),
        race_today: Set(0),
        bonus_day: Set(NaiveDate::from_ymd_opt(1970, 1, 1).unwrap()),
        season_key: Set(String::new()),
        season_wins: Set(0),
        invested: Set(invested),
        train_total: Set(0),
        father_id: Set(parents.0),
        mother_id: Set(parents.1),
        created_at: NotSet,
    };
    am.insert(db).await.context("建马落库")
}

pub async fn get_horse(db: &DatabaseConnection, id: i64) -> Result<Option<horse::Model>> {
    horse::Entity::find_by_id(id).one(db).await.context("查马")
}

/// 取某人的马厩(全部,在厩/赛中/退役都含,按 id 升序)。
pub async fn stable(db: &DatabaseConnection, uin: i64) -> Result<Vec<horse::Model>> {
    horse::Entity::find()
        .filter(horse::Column::OwnerUin.eq(uin))
        .order_by_asc(horse::Column::Id)
        .all(db)
        .await
        .context("查马厩")
}

/// 马厩马匹数(含退役占位;判断「是否已有马」用)。
pub async fn stable_count(db: &DatabaseConnection, uin: i64) -> Result<usize> {
    Ok(stable(db, uin).await?.len())
}

/// **在厩**马匹数(不含退役;容量上限 [`STABLE_CAP`](consts::STABLE_CAP) 按它判,退役不占格)。
pub async fn stable_active_count(db: &DatabaseConnection, uin: i64) -> Result<usize> {
    Ok(stable(db, uin).await?.iter().filter(|h| h.status != 2).count())
}

/// 五维当前值取数组(**点数**:列存厘点 = 点 × [`STAT_SCALE`](consts::STAT_SCALE),这里四舍五入折回点数;
/// reach / 比赛 / 出图都用点数口径)。
pub fn stats_of(m: &horse::Model) -> [i32; consts::STAT_COUNT] {
    let pts = |c: i32| (c + consts::STAT_SCALE / 2) / consts::STAT_SCALE;
    [pts(m.spd), pts(m.sta), pts(m.brs), pts(m.agi), pts(m.luk)]
}

// 寿命:不可逆生涯耗材

/// 出生寿命上限:由幸运出生潜力(pot_luk)定,钳到 [LIFESPAN_MIN, LIFESPAN_CAP_MAX]。
pub fn lifespan_max_for(pot_luk: i32) -> i32 {
    (consts::LIFESPAN_BASE + consts::LIFESPAN_LUK_COEF * pot_luk).clamp(consts::LIFESPAN_MIN, consts::LIFESPAN_CAP_MAX)
}

/// 寿命比 = lifespan / lifespan_max(分母至少 1)。
pub fn life_ratio(m: &horse::Model) -> f32 {
    m.lifespan as f32 / m.lifespan_max.max(1) as f32
}

/// 训练效率系数:life_ratio ≥ PRIME 时 1.0,往下线性掉到 FLOOR(life_ratio=0)。
pub fn train_eff(ratio: f32) -> f32 {
    1.0 - (1.0 - consts::LIFESPAN_TRAIN_EFF_FLOOR)
        * ((consts::LIFESPAN_PRIME_RATIO - ratio) / consts::LIFESPAN_PRIME_RATIO).clamp(0.0, 1.0)
}

/// 赛中耐力折扣系数:life_ratio ≥ LATE_RACE_RATIO 时 1.0,往下线性削最多 STA_PENALTY_MAX。
pub fn stamina_life_mult(ratio: f32) -> f32 {
    1.0 - consts::LIFESPAN_STA_PENALTY_MAX
        * ((consts::LIFESPAN_LATE_RACE_RATIO - ratio) / consts::LIFESPAN_LATE_RACE_RATIO).clamp(0.0, 1.0)
}

/// 全局调教衰减:总调教次数 `n` 越多,每次训练(任意维)增量越小——与单维天赋线衰减 `exp(-cur/reach)`
/// **叠乘**,让久经调教的马边际收益递减(配合寿命推动换代)。头 [`TRAIN_GLOBAL_FREE`](consts::TRAIN_GLOBAL_FREE)
/// 次满额,其后按 `1/(1+(n-FREE)/K)` 平滑衰减(渐近 0、不致死)。
pub fn train_total_decay(n: i32) -> f32 {
    let over = (n - consts::TRAIN_GLOBAL_FREE).max(0) as f32;
    1.0 / (1.0 + over / consts::TRAIN_GLOBAL_K)
}

/// **纯计算、不写库**:把资源恢复 + 伤病到期应用到内存副本,供只读展示出当前态。
/// 按 [块](consts::STATE_BLOCK_MIN)整块结算资源,余数留到下次;伤病到期自动转伤痕、伤痕到期自动清。
pub fn project(m: &horse::Model) -> horse::Model {
    let now = chrono::Local::now().fixed_offset();
    let mut out = m.clone();
    // 伤病到期:转成同等级伤痕(留复发隐患 + stat 惩罚,按 SCAR_HOURS 计时),再清伤病。
    if out.injury > 0 && out.injury_until.is_none_or(|u| u <= now) {
        out.scar = out.injury;
        out.scar_until = Some(now + chrono::Duration::hours(consts::SCAR_HOURS[(out.injury.clamp(1, 3) - 1) as usize]));
        out.injury = 0;
        out.injury_until = None;
    }
    // 伤痕到期:自动消。
    if out.scar > 0 && out.scar_until.is_none_or(|u| u <= now) {
        out.scar = 0;
        out.scar_until = None;
    }
    let blocks = ((now - out.state_at).num_minutes() / consts::STATE_BLOCK_MIN).max(0);
    if blocks > 0 {
        let b = blocks as i32;
        out.vitality = (out.vitality + consts::VIT_PER_BLOCK * b).clamp(0, consts::VIT_MAX);
        out.satiety = (out.satiety - consts::SATIETY_PER_BLOCK * b).clamp(0, consts::VIT_MAX);
        out.state_at += chrono::Duration::minutes(blocks * consts::STATE_BLOCK_MIN);
    }
    // 跨业务日则体力回满。用**原始** state_at 判跨界,触发时把 state_at 推到 now 消费掉余量,
    // 避免后续读改写反复重置(否则当日训练扣的体力会被退回)。
    if crate::data::util::business_day_of(m.state_at) < crate::data::util::business_day_of(now) {
        out.vitality = consts::VIT_MAX;
        out.state_at = now;
    }
    out
}

/// 结算并**落库**:[`project`] 算当前态,有差才写。改资源的命令调用前须先结算;结算后 `m` 与库一致。
pub async fn settle_state(db: &DatabaseConnection, m: &mut horse::Model) -> Result<()> {
    let p = project(m);
    if p.vitality == m.vitality
        && p.satiety == m.satiety
        && p.state_at == m.state_at
        && p.injury == m.injury
        && p.scar == m.scar
    {
        return Ok(()); // 无变动,免写
    }
    horse::Entity::update_many()
        .col_expr(horse::Column::Vitality, Expr::value(p.vitality))
        .col_expr(horse::Column::Satiety, Expr::value(p.satiety))
        .col_expr(horse::Column::StateAt, Expr::value(p.state_at))
        .col_expr(horse::Column::Injury, Expr::value(p.injury))
        .col_expr(horse::Column::InjuryUntil, Expr::value(p.injury_until))
        .col_expr(horse::Column::Scar, Expr::value(p.scar))
        .col_expr(horse::Column::ScarUntil, Expr::value(p.scar_until))
        .filter(horse::Column::Id.eq(m.id))
        .exec(db)
        .await
        .context("写资源结算")?;
    *m = p;
    Ok(())
}

/// 当前是否带伤(须先 [`settle_state`] 或 [`project`] 结算过到期)。
pub fn is_injured(m: &horse::Model) -> bool {
    m.injury > 0
}

/// 带伤剩余分钟(无伤返 `None`)。
pub fn injury_remaining(m: &horse::Model) -> Option<i64> {
    if m.injury <= 0 {
        return None;
    }
    let now = chrono::Local::now().fixed_offset();
    Some(m.injury_until.map(|u| (u - now).num_minutes().max(0)).unwrap_or(0))
}

/// 伤痕(后遗症)剩余分钟(无伤痕返 `None`)。
pub fn scar_remaining(m: &horse::Model) -> Option<i64> {
    if m.scar <= 0 {
        return None;
    }
    let now = chrono::Local::now().fixed_offset();
    Some(m.scar_until.map(|u| (u - now).num_minutes().max(0)).unwrap_or(0))
}

pub fn injury_name(severity: i16) -> &'static str {
    match severity {
        1 => "轻伤",
        2 => "中伤",
        _ => "重伤",
    }
}

/// 今日训练某维度的费用:基础 + 日内递增 + 随该维度**当前值**上浮。`today` 为业务日。
pub fn train_cost(m: &horse::Model, focus: Stat, today: NaiveDate) -> i64 {
    let count = if m.train_day == today { m.train_today as i64 } else { 0 };
    let cur = stats_of(m)[focus.idx()];
    consts::TRAIN_BASE_COST
        + consts::TRAIN_COST_STEP * count
        + (cur as f32 * consts::TRAIN_COST_PER_POINT).round() as i64
}

/// 一次训练的随机产出(审计/呈现用)。增量是**点数**,可含小数(久练时单次只涨零点几)。
pub struct TrainRoll {
    /// 聚焦维度的实际增量(点)。
    pub focus: (Stat, f32),
    /// 溢出维度(若有)的增量(点)。
    pub spill: Option<(Stat, f32)>,
}

/// 按「好值档」抽该维**单维**增量(点,未取整)`档值 × growth × exp(-当前值/reach)`,floor-0:越近 reach 衰减越狠。
/// 不含全局调教衰减 / 寿命效率(那两项由 [`apply_train`] 在外层统一叠乘)。`cur`/`reach` 走点数口径。
/// 档权重受幸运 + 饲料 + `well_fed` 抬向优档,`hungry` 压向低档。
#[allow(clippy::too_many_arguments)]
fn roll_gain(
    cur: i32,
    reach: i32,
    growth: i32,
    luk: i32,
    feed_bump: u32,
    hungry: bool,
    well_fed: bool,
    break_ceiling: bool,
    rng: &mut StdRng,
) -> f32 {
    let good_bump = (luk / 40).clamp(0, 4) as u32 + feed_bump + if well_fed { 1 } else { 0 };
    let mut weights: [u32; 4] = [
        consts::TRAIN_TIERS[0].0.saturating_sub(good_bump * 3),
        consts::TRAIN_TIERS[1].0,
        consts::TRAIN_TIERS[2].0 + good_bump * 2,
        consts::TRAIN_TIERS[3].0 + good_bump,
    ];
    if hungry {
        // 饿着:抬低档、两高档砍半,压低期望增量。
        weights[0] += 15;
        weights[2] /= 2;
        weights[3] /= 2;
    }
    let tier = weighted_pick(&weights, rng);
    let (_, lo, hi) = consts::TRAIN_TIERS[tier];
    let magnitude = rng.random_range(lo..hi);
    // 破限丹:无视天赋线衰减(decay=1),练满也大涨;否则按 floor-0 衰减。
    let decay = if break_ceiling { 1.0 } else { (-(cur.max(0) as f32) / reach.max(1) as f32).exp() };
    (magnitude * (growth as f32 / 100.0) * decay).max(0.0)
}

/// 某维软平台/天赋线估值(出图用):训练平均增量降到 ~1 的当前值。非硬墙——亚点增量累积可蹭过它。
pub fn soft_ceiling(reach: i32, growth: i32) -> i32 {
    // growth 钳在 [GROWTH_MIN, GROWTH_MAX]=[65,155],故 6.7×growth/100 ≥ 4.355 恒 >1,ln 必为正,无需再 max。
    (reach as f32 * (consts::TRAIN_MAG_MEAN * growth as f32 / 100.0).ln()).round() as i32
}

/// 一次训练用的消耗品(传 `None` 即裸练)。决定好值加成 + 专注/集训/破限三种特效。
#[derive(Clone, Copy)]
pub struct TrainAid {
    /// 好值档加成(饲料给;专注/集训/破限为 0)。
    pub feed_bump: u32,
    /// 专注饲料:不溢出、主练维增量 ×1.5。
    pub focus_only: bool,
    /// 集训券:本次训练不耗体力。
    pub no_vit: bool,
    /// 破限丹:本次无视天赋线衰减。
    pub break_ceiling: bool,
}

impl TrainAid {
    /// 从训练道具(`None`=裸练)推出特效;非训练道具按裸练。
    pub fn of(item: Option<Item>) -> TrainAid {
        TrainAid {
            feed_bump: item.map(|i| i.feed_bump()).unwrap_or(0),
            focus_only: item == Some(Item::FocusFeed),
            no_vit: item == Some(Item::DrillPass),
            break_ceiling: item == Some(Item::BreakPill),
        }
    }
}

/// 专注饲料:主练维增量倍率。
const FOCUS_FEED_MULT: f32 = 1.5;

/// 应用一次训练:聚焦维随机增量 + 概率溢出第二维 + 扣体力 + 日内计数,原子写库并同步 `m`。
/// `aid` 训练消耗品特效,`hungry`/`well_fed` 按饱食调好值档。**调用方须先 [`settle_state`] 且确认体力足、金币/道具已扣**。
pub async fn apply_train(
    db: &DatabaseConnection,
    m: &mut horse::Model,
    focus: Stat,
    aid: TrainAid,
    hungry: bool,
    well_fed: bool,
    rng: &mut StdRng,
) -> Result<TrainRoll> {
    let cur_centi = [m.spd, m.sta, m.brs, m.agi, m.luk]; // 厘点(写库基底,保留亚点进度)
    let cur = stats_of(m); // 点数(供衰减/费用/幸运档)
    let reach = [m.pot_spd, m.pot_sta, m.pot_brs, m.pot_agi, m.pot_luk];
    let growth = m.growth;
    let luk = cur[Stat::Luk.idx()];
    let eff = train_eff(life_ratio(m)); // 寿命见底削训练效率
    let global = train_total_decay(m.train_total);
    // 「天才」特性:训练好值档再 +1(并进 feed_bump)。
    let feed_bump = aid.feed_bump + if Trait::Genius.in_mask(m.traits) { 1 } else { 0 };
    let sanity = consts::STAT_SANITY_MAX * consts::STAT_SCALE;
    let to_centi = |g: f32| (g * consts::STAT_SCALE as f32).round().max(0.0) as i32; // 点→厘点

    let fi = focus.idx();
    let mut fg = roll_gain(cur[fi], reach[fi], growth, luk, feed_bump, hungry, well_fed, aid.break_ceiling, rng);
    if aid.focus_only {
        fg *= FOCUS_FEED_MULT;
    }
    let focus_centi = to_centi(fg * global * eff);

    // 溢出:概率受幸运微抬,落到一个非聚焦随机维度,增量减半(为 0 则不计);专注饲料不溢出。
    let spill_prob = (consts::TRAIN_SPILL_PROB + luk as f64 / 600.0).min(0.5);
    let mut spill: Option<(Stat, i32)> = None; // (维, 厘点增量)
    if !aid.focus_only && rng.random_bool(spill_prob) {
        let cands: Vec<Stat> = Stat::ALL.into_iter().filter(|s| s.idx() != fi).collect();
        let s = cands[rng.random_range(0..cands.len())];
        let raw = roll_gain(cur[s.idx()], reach[s.idx()], growth, luk, feed_bump, hungry, well_fed, false, rng) * 0.5;
        let g = to_centi(raw * global * eff);
        if g > 0 {
            spill = Some((s, g));
        }
    }

    // 算新五维(厘点),夹到健壮性上界(非玩法上限,仅防溢出),原子写。
    let mut new = cur_centi;
    new[fi] = (new[fi] + focus_centi).min(sanity);
    if let Some((s, g)) = spill {
        new[s.idx()] = (new[s.idx()] + g).min(sanity);
    }
    let today = crate::data::util::business_day();
    let new_count = if m.train_day == today { m.train_today + 1 } else { 1 };
    let new_total = m.train_total + 1;
    let new_vit = if aid.no_vit { m.vitality } else { (m.vitality - consts::VIT_TRAIN).max(0) };
    // 寿命:每次训练扣(集训券不豁免)。
    let new_life = (m.lifespan - consts::LIFESPAN_TRAIN_COST).max(0);

    horse::Entity::update_many()
        .col_expr(horse::Column::Spd, Expr::value(new[0]))
        .col_expr(horse::Column::Sta, Expr::value(new[1]))
        .col_expr(horse::Column::Brs, Expr::value(new[2]))
        .col_expr(horse::Column::Agi, Expr::value(new[3]))
        .col_expr(horse::Column::Luk, Expr::value(new[4]))
        .col_expr(horse::Column::Vitality, Expr::value(new_vit))
        .col_expr(horse::Column::Lifespan, Expr::value(new_life))
        .col_expr(horse::Column::TrainDay, Expr::value(today))
        .col_expr(horse::Column::TrainToday, Expr::value(new_count))
        .col_expr(horse::Column::TrainTotal, Expr::value(new_total))
        .filter(horse::Column::Id.eq(m.id))
        .exec(db)
        .await
        .context("写训练结果")?;

    m.spd = new[0];
    m.sta = new[1];
    m.brs = new[2];
    m.agi = new[3];
    m.luk = new[4];
    m.vitality = new_vit;
    m.lifespan = new_life;
    m.train_day = today;
    m.train_today = new_count;
    m.train_total = new_total;
    // 厘点增量折回点数(可含小数)上报。
    let to_pts = |c: i32| c as f32 / consts::STAT_SCALE as f32;
    Ok(TrainRoll { focus: (focus, to_pts(focus_centi)), spill: spill.map(|(s, g)| (s, to_pts(g))) })
}

/// 今日该马已比赛次数(业务日切换归零)。
pub fn races_today(m: &horse::Model, today: NaiveDate) -> i64 {
    if m.race_day == today { m.race_today as i64 } else { 0 }
}

/// 当前赛季键(`YYYY-MM`,业务日口径)。
pub fn season_key() -> String {
    crate::data::util::business_day().format("%Y-%m").to_string()
}

/// 原子领取「今日首胜」:今天该玩家还没有任何马夺过冠时,把夺冠马 `bonus_day` 置今天并返 `true`。
/// 用单条带 `NOT EXISTS` 的条件 UPDATE 按受影响行数判定——跨锁域/多进程都只发一次(check-then-act 会双发)。
pub async fn claim_first_win_today(db: &DatabaseConnection, uin: i64, winner_horse_id: i64) -> Result<bool> {
    let today = crate::data::util::business_day();
    let stmt = Statement::from_sql_and_values(
        db.get_database_backend(),
        "UPDATE horse SET bonus_day = $1 WHERE id = $2 AND owner_uin = $3 \
         AND NOT EXISTS (SELECT 1 FROM horse h2 WHERE h2.owner_uin = $3 AND h2.bonus_day = $1)",
        [today.into(), winner_horse_id.into(), uin.into()],
    );
    Ok(db.execute(stmt).await.context("领取每日首胜")?.rows_affected() > 0)
}

/// 比赛后结算:扣体力/寿命、累计场次与胜场(含赛季胜场/今日计数)、维护赛季键,原子写并同步 `m`。
/// 每日首胜另由 [`claim_first_win_today`] 领取,本函数不碰 `bonus_day`。
pub async fn finish_race(db: &DatabaseConnection, m: &mut horse::Model, won: bool) -> Result<()> {
    let new_vit = (m.vitality - consts::VIT_RACE).max(0);
    let new_life = (m.lifespan - consts::LIFESPAN_RACE_COST).max(0);
    let win_inc = if won { 1 } else { 0 };
    let today = crate::data::util::business_day();
    let new_race_today = if m.race_day == today { m.race_today + 1 } else { 1 };
    // 赛季胜场:同赛季累加,换月即归零重计。
    let season = today.format("%Y-%m").to_string();
    let new_season_wins = if m.season_key == season { m.season_wins + win_inc } else { win_inc };
    horse::Entity::update_many()
        .col_expr(horse::Column::Vitality, Expr::value(new_vit))
        .col_expr(horse::Column::Lifespan, Expr::value(new_life))
        .col_expr(horse::Column::Races, Expr::col(horse::Column::Races).add(1))
        .col_expr(horse::Column::Wins, Expr::col(horse::Column::Wins).add(win_inc))
        .col_expr(horse::Column::RaceDay, Expr::value(today))
        .col_expr(horse::Column::RaceToday, Expr::value(new_race_today))
        .col_expr(horse::Column::SeasonKey, Expr::value(season.clone()))
        .col_expr(horse::Column::SeasonWins, Expr::value(new_season_wins))
        .filter(horse::Column::Id.eq(m.id))
        .exec(db)
        .await
        .context("写比赛结算")?;
    m.vitality = new_vit;
    m.lifespan = new_life;
    m.races += 1;
    m.wins += win_inc;
    m.race_day = today;
    m.race_today = new_race_today;
    m.season_key = season;
    m.season_wins = new_season_wins;
    Ok(())
}

/// 给马置伤病(伤等 1–3,设恢复到期),原子写并同步 `m`。受伤判定在比赛内核局内逐回合做(见 [`race`](super::race))。
pub async fn set_injury(db: &DatabaseConnection, m: &mut horse::Model, severity: i16) -> Result<()> {
    let hours = consts::INJURY_HOURS[(severity.clamp(1, 3) - 1) as usize];
    let until = (chrono::Local::now() + chrono::Duration::hours(hours)).fixed_offset();
    horse::Entity::update_many()
        .col_expr(horse::Column::Injury, Expr::value(severity))
        .col_expr(horse::Column::InjuryUntil, Expr::value(until))
        .filter(horse::Column::Id.eq(m.id))
        .exec(db)
        .await
        .context("写伤病")?;
    m.injury = severity;
    m.injury_until = Some(until);
    Ok(())
}

pub fn heal_cost(m: &horse::Model) -> i64 {
    consts::HEAL_COST[(m.injury.clamp(1, 3) - 1) as usize]
}

/// 立即治愈(清伤病)并落同等级伤痕(与自然到期一致:治好能上场,但留复发隐患),原子写并同步 `m`。
/// **调用方须先扣费**。
pub async fn heal(db: &DatabaseConnection, m: &mut horse::Model) -> Result<()> {
    let scar = m.injury;
    let scar_until = (scar > 0).then(|| {
        (chrono::Local::now() + chrono::Duration::hours(consts::SCAR_HOURS[(scar.clamp(1, 3) - 1) as usize]))
            .fixed_offset()
    });
    horse::Entity::update_many()
        .col_expr(horse::Column::Injury, Expr::value(0))
        .col_expr(horse::Column::InjuryUntil, Expr::value(Option::<DateTimeWithTimeZone>::None))
        .col_expr(horse::Column::Scar, Expr::value(scar))
        .col_expr(horse::Column::ScarUntil, Expr::value(scar_until))
        .filter(horse::Column::Id.eq(m.id))
        .exec(db)
        .await
        .context("写治疗")?;
    m.injury = 0;
    m.injury_until = None;
    m.scar = scar;
    m.scar_until = scar_until;
    Ok(())
}

// 排行榜

/// 取生涯胜场榜前 `limit` 匹马(出过场的,按胜场降序、同胜场少场优先)。
pub async fn top_horses(db: &DatabaseConnection, limit: u64) -> Result<Vec<horse::Model>> {
    horse::Entity::find()
        .filter(horse::Column::Races.gt(0))
        .order_by_desc(horse::Column::Wins)
        .order_by_asc(horse::Column::Races)
        .order_by_asc(horse::Column::Id)
        .limit(limit)
        .all(db)
        .await
        .context("查赛马榜")
}

/// 取**本赛季**胜场榜前 `limit` 匹马(只算当前赛季有胜的)。
pub async fn top_horses_season(db: &DatabaseConnection, limit: u64) -> Result<Vec<horse::Model>> {
    horse::Entity::find()
        .filter(horse::Column::SeasonKey.eq(season_key()))
        .filter(horse::Column::SeasonWins.gt(0))
        .order_by_desc(horse::Column::SeasonWins)
        .order_by_asc(horse::Column::Id)
        .limit(limit)
        .all(db)
        .await
        .context("查赛季榜")
}

/// 取胜率榜前 `limit` 匹马(出战 ≥ [`RANK_MIN_RACES`](consts::RANK_MIN_RACES) 防小样本刷榜,按胜率降序)。
/// 胜率比较在内存里做,避免 SQL 浮点排序的可移植性问题。
pub async fn top_horses_winrate(db: &DatabaseConnection, limit: usize) -> Result<Vec<horse::Model>> {
    let mut horses = horse::Entity::find()
        .filter(horse::Column::Races.gte(consts::RANK_MIN_RACES))
        .all(db)
        .await
        .context("查胜率榜")?;
    horses.sort_by(|a, b| {
        let ra = a.wins as f64 / a.races as f64;
        let rb = b.wins as f64 / b.races as f64;
        rb.total_cmp(&ra).then(b.races.cmp(&a.races)).then(a.id.cmp(&b.id))
    });
    horses.truncate(limit);
    Ok(horses)
}

/// 批量解析主人显示名:自设昵称 > 账号昵称 > `玩家#UID`。
pub async fn owner_names(db: &DatabaseConnection, uins: &[i64]) -> Result<std::collections::HashMap<i64, String>> {
    use crate::data::entity::{identity, user};
    let mut out = std::collections::HashMap::new();
    if uins.is_empty() {
        return Ok(out);
    }
    let nicks: std::collections::HashMap<i64, String> = identity::Entity::find()
        .filter(identity::Column::Uin.is_in(uins.iter().copied()))
        .all(db)
        .await
        .context("查账号昵称")?
        .into_iter()
        .map(|m| (m.uin, m.nickname))
        .collect();
    let users =
        user::Entity::find().filter(user::Column::Uin.is_in(uins.iter().copied())).all(db).await.context("查用户")?;
    for u in users {
        let name = if !u.alias.is_empty() {
            u.alias.clone()
        } else if let Some(n) = nicks.get(&u.uin).filter(|n| !n.is_empty()) {
            n.clone()
        } else {
            format!("玩家#{}", u.id)
        };
        out.insert(u.uin, name);
    }
    Ok(out)
}

/// 主人显示标签:显示名 + 站内 UID(如「A60 #42」)。取名口径同 [`owner_names`]。
pub async fn owner_label(db: &DatabaseConnection, uin: i64) -> Result<String> {
    use crate::data::entity::{identity, user};
    let Some(u) = user::Entity::find().filter(user::Column::Uin.eq(uin)).one(db).await.context("查用户")? else {
        return Ok(format!("玩家#{uin}"));
    };
    let name = if !u.alias.is_empty() {
        u.alias.clone()
    } else {
        identity::Entity::find()
            .filter(identity::Column::Uin.eq(uin))
            .one(db)
            .await
            .context("查账号昵称")?
            .map(|m| m.nickname)
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| format!("玩家#{}", u.id))
    };
    Ok(format!("{name} #{}", u.id))
}

/// 喂基础草料:回饱食,原子写并同步 `m`。**调用方须先扣费**。
pub async fn feed_basic(db: &DatabaseConnection, m: &mut horse::Model) -> Result<()> {
    let new_sat = (m.satiety + consts::FORAGE_SATIETY).min(consts::VIT_MAX);
    horse::Entity::update_many()
        .col_expr(horse::Column::Satiety, Expr::value(new_sat))
        .filter(horse::Column::Id.eq(m.id))
        .exec(db)
        .await
        .context("写喂养")?;
    m.satiety = new_sat;
    Ok(())
}

/// 护理回寿命:可回复上限永久 −`cap_cost`(不可逆),寿命回 `restore` 但封到新上限,原子写并同步 `m`。
/// **调用方须先扣下护理道具**。
pub async fn apply_restore(db: &DatabaseConnection, m: &mut horse::Model, restore: i32, cap_cost: i32) -> Result<()> {
    let new_cap = (m.lifespan_cap - cap_cost).max(0);
    let new_life = (m.lifespan + restore).min(new_cap);
    horse::Entity::update_many()
        .col_expr(horse::Column::Lifespan, Expr::value(new_life))
        .col_expr(horse::Column::LifespanCap, Expr::value(new_cap))
        .filter(horse::Column::Id.eq(m.id))
        .exec(db)
        .await
        .context("写护理")?;
    m.lifespan = new_life;
    m.lifespan_cap = new_cap;
    Ok(())
}

/// 累加生涯投入(各花币养成点真埋点):原子 `invested += amount`,同步 `m`。
pub async fn add_invested(db: &DatabaseConnection, m: &mut horse::Model, amount: i64) -> Result<()> {
    horse::Entity::update_many()
        .col_expr(horse::Column::Invested, Expr::col(horse::Column::Invested).add(amount))
        .filter(horse::Column::Id.eq(m.id))
        .exec(db)
        .await
        .context("写投入累计")?;
    m.invested += amount;
    Ok(())
}

// 道具:养成/恢复/繁殖/趣味效果。都「调用方须先扣下对应道具」,这层只做单匹马的原子写 + 同步内存。

/// 育骨精料:给某维资质 reach +[`REACH_TONIC_ADD`](consts::REACH_TONIC_ADD) 钳到上界。返回该维新 reach。
pub async fn apply_reach_tonic(db: &DatabaseConnection, m: &mut horse::Model, stat: Stat) -> Result<i32> {
    let (col, cur) = match stat {
        Stat::Spd => (horse::Column::PotSpd, m.pot_spd),
        Stat::Sta => (horse::Column::PotSta, m.pot_sta),
        Stat::Brs => (horse::Column::PotBrs, m.pot_brs),
        Stat::Agi => (horse::Column::PotAgi, m.pot_agi),
        Stat::Luk => (horse::Column::PotLuk, m.pot_luk),
    };
    let nv = (cur + consts::REACH_TONIC_ADD).min(consts::REACH_HARD_MAX);
    horse::Entity::update_many()
        .col_expr(col, Expr::value(nv))
        .filter(horse::Column::Id.eq(m.id))
        .exec(db)
        .await
        .context("写资质")?;
    match stat {
        Stat::Spd => m.pot_spd = nv,
        Stat::Sta => m.pot_sta = nv,
        Stat::Brs => m.pot_brs = nv,
        Stat::Agi => m.pot_agi = nv,
        Stat::Luk => m.pot_luk = nv,
    }
    Ok(nv)
}

/// 洗髓草:按本星级重摇 growth(成长),原子写并同步。返回新 growth。
pub async fn reroll_growth(db: &DatabaseConnection, m: &mut horse::Model, rng: &mut StdRng) -> Result<i32> {
    let g = roll_growth(m.rarity, rng);
    horse::Entity::update_many()
        .col_expr(horse::Column::Growth, Expr::value(g))
        .filter(horse::Column::Id.eq(m.id))
        .exec(db)
        .await
        .context("写成长")?;
    m.growth = g;
    Ok(g)
}

/// 特性秘传:未满 [`TRAIT_MAX`](consts::TRAIT_MAX) 时随机学一条尚未拥有的特性。返回学到的(满了/无可学返 `None`)。
pub async fn add_random_trait(
    db: &DatabaseConnection,
    m: &mut horse::Model,
    rng: &mut StdRng,
) -> Result<Option<Trait>> {
    if Trait::from_mask(m.traits).len() as u32 >= consts::TRAIT_MAX {
        return Ok(None);
    }
    let avail: Vec<Trait> = Trait::ALL.into_iter().filter(|t| !t.in_mask(m.traits)).collect();
    let Some(&t) = avail.get(rng.random_range(0..avail.len().max(1))).filter(|_| !avail.is_empty()) else {
        return Ok(None);
    };
    let nm = m.traits | t.bit();
    horse::Entity::update_many()
        .col_expr(horse::Column::Traits, Expr::value(nm))
        .filter(horse::Column::Id.eq(m.id))
        .exec(db)
        .await
        .context("写特性")?;
    m.traits = nm;
    Ok(Some(t))
}

/// 静心符:按本星级出生特性概率重摇全部特性。返回新掩码。
pub async fn reroll_traits(db: &DatabaseConnection, m: &mut horse::Model, rng: &mut StdRng) -> Result<i32> {
    let nm = roll_traits(m.rarity, rng);
    horse::Entity::update_many()
        .col_expr(horse::Column::Traits, Expr::value(nm))
        .filter(horse::Column::Id.eq(m.id))
        .exec(db)
        .await
        .context("写特性")?;
    m.traits = nm;
    Ok(nm)
}

/// 能量饮:回体力 `amount` 钳到上限,原子写并同步。
pub async fn restore_vitality(db: &DatabaseConnection, m: &mut horse::Model, amount: i32) -> Result<()> {
    let nv = (m.vitality + amount).min(consts::VIT_MAX);
    horse::Entity::update_many()
        .col_expr(horse::Column::Vitality, Expr::value(nv))
        .filter(horse::Column::Id.eq(m.id))
        .exec(db)
        .await
        .context("写体力")?;
    m.vitality = nv;
    Ok(())
}

/// 精草料:回饱食 `amount` 钳到上限,原子写并同步。
pub async fn restore_satiety(db: &DatabaseConnection, m: &mut horse::Model, amount: i32) -> Result<()> {
    let nv = (m.satiety + amount).min(consts::VIT_MAX);
    horse::Entity::update_many()
        .col_expr(horse::Column::Satiety, Expr::value(nv))
        .filter(horse::Column::Id.eq(m.id))
        .exec(db)
        .await
        .context("写饱食")?;
    m.satiety = nv;
    Ok(())
}

/// 红绳:立即清母马繁殖冷却,原子写并同步。
pub async fn clear_breed_cd(db: &DatabaseConnection, m: &mut horse::Model) -> Result<()> {
    horse::Entity::update_many()
        .col_expr(horse::Column::BreedCdUntil, Expr::value(Option::<DateTimeWithTimeZone>::None))
        .filter(horse::Column::Id.eq(m.id))
        .exec(db)
        .await
        .context("写繁殖冷却")?;
    m.breed_cd_until = None;
    Ok(())
}

/// 续种符:作种次数 −1(夹到 0,等于多配一次),原子写并同步。
pub async fn reduce_breed_count(db: &DatabaseConnection, m: &mut horse::Model) -> Result<()> {
    let nc = (m.breed_count - 1).max(0);
    horse::Entity::update_many()
        .col_expr(horse::Column::BreedCount, Expr::value(nc))
        .filter(horse::Column::Id.eq(m.id))
        .exec(db)
        .await
        .context("写作种次数")?;
    m.breed_count = nc;
    Ok(())
}

/// 染色剂:改毛色,原子写并同步。
pub async fn set_color(db: &DatabaseConnection, m: &mut horse::Model, color: i16) -> Result<()> {
    horse::Entity::update_many()
        .col_expr(horse::Column::Color, Expr::value(color))
        .filter(horse::Column::Id.eq(m.id))
        .exec(db)
        .await
        .context("写毛色")?;
    m.color = color;
    Ok(())
}

// 背包:走跨游戏共享的 `game_item`(见 crate::data::inventory),活动在赛马号段
// `HORSE_ITEM_BASE..+SPAN`。这层只做 Item↔全局 id 的翻译 + 堆叠上限。

/// 赛马背包(本号段内 qty>0 的道具,按 id 升序)。
pub async fn backpack(db: &DatabaseConnection, uin: i64) -> Result<Vec<(Item, i32)>> {
    let rows = crate::data::inventory::list_range(
        db,
        uin,
        consts::HORSE_ITEM_BASE,
        consts::HORSE_ITEM_BASE + consts::HORSE_ITEM_SPAN,
    )
    .await?;
    Ok(rows.into_iter().filter_map(|(id, qty)| Item::from_global(id).map(|i| (i, qty))).collect())
}

/// 入袋 `n` 个道具,夹到堆叠上限;返回**溢出数**(没装下的,由调用方折算金币返还)。
pub async fn add_item(db: &DatabaseConnection, uin: i64, it: Item, n: i32) -> Result<i32> {
    crate::data::inventory::add_capped(db, uin, it.global_id(), n, consts::ITEM_STACK_CAP).await
}

/// 带闸扣 `n` 个道具:够则扣、返 `true`,不够一动不动、返 `false`。
pub async fn take_item(db: &DatabaseConnection, uin: i64, it: Item, n: i32) -> Result<bool> {
    crate::data::inventory::take(db, uin, it.global_id(), n).await
}

// 抽卡

/// 抽卡一次的产物。
pub enum Pull {
    Item(Item),
    /// 携带已抽好的出生属性。
    Horse(Birth),
}

/// 赛后掉落(幸运产出维):按幸运掷掉落概率,命中则按品质档抽一件道具。`fortuitous` 幸运儿特性放大概率,
/// `prob_scale` 场景乘子(PvE 1.0、PvP 减半)。未触发返 `None`。**调用方负责入袋**。
pub fn roll_drop(luk: i32, fortuitous: bool, prob_scale: f64, rng: &mut StdRng) -> Option<Item> {
    let mut p = ((luk as f64 - consts::LUCK_DROP_FLOOR) / consts::LUCK_DROP_DIV).clamp(0.0, consts::LUCK_DROP_CAP);
    if fortuitous {
        p = (p * consts::TRAIT_FORTUNE_DROP_MULT).min(consts::LUCK_DROP_CAP);
    }
    let p = (p * prob_scale).clamp(0.0, 1.0);
    if !rng.random_bool(p) {
        return None;
    }
    let item = match weighted_pick(&consts::DROP_QUALITY_WEIGHTS, rng) {
        0 => consts::DROP_COMMON[rng.random_range(0..consts::DROP_COMMON.len())],
        1 => consts::DROP_MID[rng.random_range(0..consts::DROP_MID.len())],
        _ => Item::TREASURE[weighted_pick(&consts::GACHA_TREASURE_WEIGHTS, rng)],
    };
    Some(item)
}

/// 读抽卡保底计数(无则 0)。
pub async fn gacha_pity(db: &DatabaseConnection, uin: i64) -> Result<i32> {
    let row = gacha::Entity::find_by_id(uin).one(db).await.context("查抽卡保底")?;
    Ok(row.map(|m| m.pity).unwrap_or(0))
}

/// 落库抽卡保底计数(upsert)。
pub async fn set_gacha_pity(db: &DatabaseConnection, uin: i64, pity: i32) -> Result<()> {
    let am = gacha::ActiveModel { uin: Set(uin), pity: Set(pity) };
    gacha::Entity::insert(am)
        .on_conflict(OnConflict::column(gacha::Column::Uin).update_column(gacha::Column::Pity).to_owned())
        .exec(db)
        .await
        .context("写抽卡保底")?;
    Ok(())
}

/// 抽一次(纯函数):按 `class_weights` 大类权重 `(道具,训练,恢复,珍材,马)` 出产物,出马星级用 `horse_rarity_weights`,
/// 更新 `pity`。`pity` = 距上次 ★3+ 的累计抽数:到 [`GACHA_PITY`](consts::GACHA_PITY) 强制出 ★3+ 并清零;自然 ★3+ 也清零,
/// 但自然出的 **★1/★2 马不清零**(否则保底永不触发)。软保底末段出马权重随距保底渐升。
pub fn gacha_pull(pity: &mut i32, class_weights: &[u32; 5], horse_rarity_weights: &[u32; 4], rng: &mut StdRng) -> Pull {
    let next = *pity + 1;
    let force = next >= consts::GACHA_PITY;
    let into_soft = next - (consts::GACHA_PITY - consts::GACHA_SOFT_PITY);
    let [w_race, w_train, w_recovery, w_treasure, w_horse_base] = *class_weights;
    let class = if force {
        4 // 保底:强制出 ★3+ 马
    } else {
        let w_horse = w_horse_base + if into_soft > 0 { into_soft as u32 * 4 } else { 0 };
        weighted_pick(&[w_race, w_train, w_recovery, w_treasure, w_horse], rng)
    };
    match class {
        4 => {
            let weights = if force { &consts::GACHA_PITY_RARITY_WEIGHTS } else { horse_rarity_weights };
            let rarity = roll_rarity(weights, rng);
            // 只有 ★3+ 清空保底计数;低星自然出马仍让 pity 累进。
            *pity = if rarity >= 3 { 0 } else { next };
            Pull::Horse(birth_for_rarity(rarity, rng))
        }
        3 => {
            *pity = next;
            Pull::Item(Item::TREASURE[weighted_pick(&consts::GACHA_TREASURE_WEIGHTS, rng)])
        }
        2 => {
            *pity = next;
            Pull::Item(Item::RECOVERY[weighted_pick(&consts::GACHA_RECOVERY_WEIGHTS, rng)])
        }
        1 => {
            *pity = next;
            Pull::Item(Item::TRAIN[weighted_pick(&consts::GACHA_TRAIN_WEIGHTS, rng)])
        }
        _ => {
            *pity = next;
            Pull::Item(Item::RACE[weighted_pick(&consts::GACHA_RACE_WEIGHTS, rng)])
        }
    }
}

// 繁殖遗传

/// 一匹马上溯 `depth` 代的祖先集合(**含自身**)。
pub async fn ancestor_set(db: &DatabaseConnection, id: i64, depth: u32) -> Result<HashSet<i64>> {
    let mut set = HashSet::new();
    set.insert(id);
    let mut frontier = vec![(id, depth)];
    while let Some((cur, d)) = frontier.pop() {
        if d == 0 {
            continue;
        }
        if let Some(h) = get_horse(db, cur).await? {
            for p in [h.father_id, h.mother_id].into_iter().flatten() {
                if set.insert(p) {
                    frontier.push((p, d - 1));
                }
            }
        }
    }
    Ok(set)
}

/// 两匹马是否近亲(`depth` 代内有共同祖先,或互为祖先/同一匹)。
pub async fn is_incest(db: &DatabaseConnection, a: i64, b: i64, depth: u32) -> Result<bool> {
    let sa = ancestor_set(db, a, depth).await?;
    let sb = ancestor_set(db, b, depth).await?;
    Ok(sa.intersection(&sb).next().is_some())
}

/// 繁殖费用(按较高亲本星级,见 [`BREED_COST_BY_RARITY`](consts::BREED_COST_BY_RARITY))。
pub fn breed_cost(f: &horse::Model, m: &horse::Model) -> i64 {
    let r = (f.rarity.max(m.rarity).clamp(consts::RARITY_MIN, consts::RARITY_MAX) - 1) as usize;
    consts::BREED_COST_BY_RARITY[r]
}

/// 繁殖产出的子代雏形(出生属性 + 代数 + 外观)。
pub struct BredChild {
    pub birth: Birth,
    pub generation: i32,
    pub color: i16,
    pub sex: i16,
}

/// 遗传纯函数(软基线回归模型):先定**星级**(决定子代 reach/growth 回归的基线),再每维取「偏向较优亲本」的中值、
/// 向本星基线回归 + 噪声;`growth` 向**子代星均值**回归 + 噪声后 **.min(本星均值) 硬顶**(繁殖不超均值、不传递,
/// 高 growth 只能靠抽卡/洗髓)。reach/growth 各自钳到合法区间。
pub fn breed_child(f: &horse::Model, m: &horse::Model, incest: bool, star_stone: bool, rng: &mut StdRng) -> BredChild {
    let freach = [f.pot_spd, f.pot_sta, f.pot_brs, f.pot_agi, f.pot_luk];
    let mreach = [m.pot_spd, m.pot_sta, m.pot_brs, m.pot_agi, m.pot_luk];
    let rarity = breed_rarity(f.rarity, m.rarity, incest, star_stone, rng);
    let star_idx = (rarity.clamp(consts::RARITY_MIN, consts::RARITY_MAX) - 1) as usize;
    let baseline = consts::REACH_BASELINE[star_idx] as f32;
    let reach: [i32; consts::STAT_COUNT] = std::array::from_fn(|i| {
        let (lo, hi) = (freach[i].min(mreach[i]) as f32, freach[i].max(mreach[i]) as f32);
        // 偏向较优亲本(可定向选育),留方差 → 也可能落到较差亲本的该维。
        let w = gauss(consts::BREED_REACH_LEAN, consts::BREED_REACH_LEAN_SD, rng).clamp(0.0, 1.0);
        let mid = lo + w * (hi - lo);
        // 向本星基线回归(低于→正向填充、高于→往回拉)+ 噪声(可负 → 有概率更烂)。
        let drift = consts::BREED_REACH_REVERT * (baseline - mid) + gauss(0.0, consts::BREED_REACH_NOISE, rng);
        (mid + drift).round().clamp(consts::REACH_MIN as f32, consts::REACH_HARD_MAX as f32) as i32
    });
    // growth 向子代星均值回归 + 噪声,再硬顶到本星均值(繁殖只填到均值、不超、不传递)。
    let parent_mean = (f.growth + m.growth) as f32 / 2.0;
    let target = consts::GROWTH_MEAN[star_idx];
    let growth = (parent_mean
        + consts::GROWTH_BREED_REVERT * (target - parent_mean)
        + gauss(0.0, consts::GROWTH_BREED_NOISE, rng))
    .min(target)
    .round()
    .clamp(consts::GROWTH_MIN as f32, consts::GROWTH_MAX as f32) as i32;
    let traits = breed_traits(f.traits, m.traits, rng);
    BredChild {
        birth: Birth { rarity, cur: birth_cur(&reach), reach, growth, traits },
        generation: f.generation.max(m.generation) + 1,
        color: if rng.random_bool(0.5) { f.color } else { m.color },
        sex: rng.random_range(0..2) as i16,
    }
}

/// 子代星级:基础 = floor(双亲均值)(向下取整,使低星亲本真把基础星拉低、不能靠廉价陪配稳定复制高星),
/// 再按概率 +1(双亲均 ★3+ 时更高)/ −1(回退,可繁殖出更烂的);近亲只跌不升。钳到 [1, 4]。
/// `star_stone`(星辉石):跳过随机,子代确定 = base+1。
fn breed_rarity(rf: i16, rm: i16, incest: bool, star_stone: bool, rng: &mut StdRng) -> i16 {
    let base = ((rf + rm) as f32 / 2.0).floor() as i16;
    if star_stone {
        return (base + 1).clamp(consts::RARITY_MIN, consts::RARITY_MAX);
    }
    let (up, down) = if incest {
        (0.0, consts::BREED_RARITY_DOWN_PROB_INCEST)
    } else {
        let up = if rf >= 3 && rm >= 3 { consts::BREED_RARITY_UP_PROB_HIGH } else { consts::BREED_RARITY_UP_PROB };
        (up, consts::BREED_RARITY_DOWN_PROB)
    };
    let r = rng.random_range(0.0..1.0);
    let delta = if r < up {
        1
    } else if r < up + down {
        -1
    } else {
        0
    };
    (base + delta).clamp(consts::RARITY_MIN, consts::RARITY_MAX)
}

/// 子代特性遗传:把父母特性**合并**后,每条按 [`TRAIT_INHERIT_PROB`](consts::TRAIT_INHERIT_PROB) 遗传(双亲
/// 共有的也只掷一次),另有 [`TRAIT_MUTATE_PROB`](consts::TRAIT_MUTATE_PROB) 概率变异出一条全新特性;总数封顶
/// [`TRAIT_MAX`](consts::TRAIT_MAX)。
fn breed_traits(f_traits: i32, m_traits: i32, rng: &mut StdRng) -> i32 {
    let parent_mask = f_traits | m_traits;
    // 合并后**打乱顺序**再逐条遗传,避免按 bit 顺序贪心使低位特性系统性优先。
    let mut cands: Vec<Trait> = Trait::ALL.into_iter().filter(|t| t.in_mask(parent_mask)).collect();
    for i in (1..cands.len()).rev() {
        cands.swap(i, rng.random_range(0..=i));
    }
    let mut mask = 0i32;
    let mut owned = 0u32;
    for t in cands {
        if owned >= consts::TRAIT_MAX {
            break;
        }
        if rng.random_bool(consts::TRAIT_INHERIT_PROB) {
            mask |= t.bit();
            owned += 1;
        }
    }
    if owned < consts::TRAIT_MAX && rng.random_bool(consts::TRAIT_MUTATE_PROB) {
        let avail: Vec<Trait> = Trait::ALL.into_iter().filter(|t| !t.in_mask(mask)).collect();
        if !avail.is_empty() {
            mask |= avail[rng.random_range(0..avail.len())].bit();
        }
    }
    mask
}

// 成就 / 称号

/// 某人马厩派生出的成就判定快照(一次扫描全马算齐)。
struct Dex {
    total_wins: i32,
    max_gen: i32,
    has_star4: bool,
    has_trait: bool,
    colors: HashSet<i16>,
    active: usize,
}

/// 从一个人的全部马算成就判定快照。
fn compute_dex(horses: &[horse::Model]) -> Dex {
    let mut d =
        Dex { total_wins: 0, max_gen: 0, has_star4: false, has_trait: false, colors: HashSet::new(), active: 0 };
    for h in horses {
        d.total_wins += h.wins;
        d.max_gen = d.max_gen.max(h.generation);
        d.has_star4 |= h.rarity >= 4;
        d.has_trait |= h.traits != 0;
        if h.status != 2 {
            d.active += 1;
            d.colors.insert(h.color); // 「集色」按在厩马算
        }
    }
    d
}

/// 某成就当前是否达成(对照判定快照)。
fn qualifies(a: Achievement, d: &Dex) -> bool {
    match a {
        Achievement::FirstWin => d.total_wins >= 1,
        Achievement::HundredWins => d.total_wins >= consts::ACH_HUNDRED_WINS,
        Achievement::FirstBreed => d.max_gen >= 2,
        Achievement::Dynasty => d.max_gen >= consts::ACH_DYNASTY_GEN,
        Achievement::Chosen => d.has_star4,
        Achievement::Gifted => d.has_trait,
        Achievement::Collector => d.colors.len() >= consts::COLOR_COUNT as usize,
        Achievement::Tycoon => d.active >= consts::ACH_TYCOON_HORSES,
    }
}

/// 某人已达成的成就代码集合。
pub async fn earned_achievements(db: &DatabaseConnection, uin: i64) -> Result<HashSet<i32>> {
    Ok(achievement::Entity::find()
        .filter(achievement::Column::Uin.eq(uin))
        .all(db)
        .await
        .context("查成就")?
        .into_iter()
        .map(|m| m.code)
        .collect())
}

/// 评估并发放新达成的成就:扫描全马 → 比对已达成 → 把新达成的入库(幂等)→ 返回新达成列表。
/// **不发金币**(由 [`mod`](super) 经 `AUser` 发)。
pub async fn evaluate_and_grant(db: &DatabaseConnection, uin: i64) -> Result<Vec<Achievement>> {
    let earned = earned_achievements(db, uin).await?;
    let horses = stable(db, uin).await?;
    let d = compute_dex(&horses);
    let mut newly = Vec::new();
    for a in Achievement::ALL {
        if !earned.contains(&a.code()) && qualifies(a, &d) {
            let am = achievement::ActiveModel { uin: Set(uin), code: Set(a.code()), earned_at: NotSet };
            // 幂等且**只在真正插入了新行时**才算新达成:并发/连点下撞主键的那一方 affected=0、不入列、不发币。
            let affected = achievement::Entity::insert(am)
                .on_conflict(
                    OnConflict::columns([achievement::Column::Uin, achievement::Column::Code]).do_nothing().to_owned(),
                )
                .exec_without_returning(db)
                .await
                .context("写成就")?;
            if affected > 0 {
                newly.push(a);
            }
        }
    }
    Ok(newly)
}

/// 某人当前称号:已达成且带称号的成就里取奖励最高的那个的称号(没有则 `None`)。
pub async fn user_title(db: &DatabaseConnection, uin: i64) -> Result<Option<&'static str>> {
    let earned = earned_achievements(db, uin).await?;
    Ok(Achievement::ALL
        .into_iter()
        .filter(|a| earned.contains(&a.code()) && a.title().is_some())
        .max_by_key(|a| a.reward())
        .and_then(|a| a.title()))
}

/// 给母马置繁殖冷却(now + [`BREED_COOLDOWN_HOURS`](consts::BREED_COOLDOWN_HOURS))。
pub async fn set_breed_cd(db: &DatabaseConnection, mother_id: i64) -> Result<()> {
    let until = (chrono::Local::now() + chrono::Duration::hours(consts::BREED_COOLDOWN_HOURS)).fixed_offset();
    horse::Entity::update_many()
        .col_expr(horse::Column::BreedCdUntil, Expr::value(until))
        .filter(horse::Column::Id.eq(mother_id))
        .exec(db)
        .await
        .context("写繁殖冷却")?;
    Ok(())
}

/// 给一匹马的作种次数 +1(原子;父母各调一次)。
pub async fn bump_breed_count(db: &DatabaseConnection, id: i64) -> Result<()> {
    horse::Entity::update_many()
        .col_expr(horse::Column::BreedCount, Expr::col(horse::Column::BreedCount).add(1))
        .filter(horse::Column::Id.eq(id))
        .exec(db)
        .await
        .context("写作种次数")?;
    Ok(())
}

// 退役 / 改名

/// 退役回馈金币:地板 [`RETIRE_REWARD_BASE`](consts::RETIRE_REWARD_BASE) + 生涯累计投入(invested)的
/// [`RETIRE_INVEST_PCT`](consts::RETIRE_INVEST_PCT)。养得多回得多,但比例远低于 1,退役只是腾格 + 部分回血。
pub fn retire_reward(m: &horse::Model) -> i64 {
    consts::RETIRE_REWARD_BASE + (m.invested as f64 * consts::RETIRE_INVEST_PCT).round() as i64
}

/// 改一匹马的状态(0 在厩 / 2 退役),原子写并同步 `m`。
pub async fn set_status(db: &DatabaseConnection, m: &mut horse::Model, status: i16) -> Result<()> {
    horse::Entity::update_many()
        .col_expr(horse::Column::Status, Expr::value(status))
        .filter(horse::Column::Id.eq(m.id))
        .exec(db)
        .await
        .context("写马状态")?;
    m.status = status;
    Ok(())
}

/// 改名,原子写并同步 `m`。
pub async fn rename(db: &DatabaseConnection, m: &mut horse::Model, new: &str) -> Result<()> {
    horse::Entity::update_many()
        .col_expr(horse::Column::Name, Expr::value(new))
        .filter(horse::Column::Id.eq(m.id))
        .exec(db)
        .await
        .context("写马名")?;
    m.name = new.to_string();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;

    /// 领养首马:固定 ★2、growth 与 reach 在合法区间、当前值 = 0.40×reach。
    #[test]
    fn starter_basic() {
        let mut rng = StdRng::seed_from_u64(3);
        for _ in 0..2000 {
            let b = roll_starter(&mut rng);
            assert_eq!(b.rarity, consts::STARTER_RARITY);
            assert!((consts::GROWTH_MIN..=consts::GROWTH_MAX).contains(&b.growth), "growth 越界: {}", b.growth);
            for i in 0..consts::STAT_COUNT {
                assert!(
                    (consts::REACH_MIN..=consts::REACH_HARD_MAX).contains(&b.reach[i]),
                    "reach 越界: {}",
                    b.reach[i]
                );
                let want = (b.reach[i] as f32 * consts::BIRTH_REACH_RATIO).round() as i32;
                assert_eq!(b.cur[i], want, "出生值应 = 0.40×reach");
            }
        }
    }

    /// 同一 seed 出生稳定;高星 reach 均值明显高于低星。
    #[test]
    fn birth_deterministic_and_star_bands() {
        let mut a = StdRng::seed_from_u64(42);
        let mut b = StdRng::seed_from_u64(42);
        assert_eq!(roll_reach(3, &mut a), roll_reach(3, &mut b));
        let mut rng = StdRng::seed_from_u64(7);
        let avg = |r: i16, rng: &mut StdRng| {
            let n = 400;
            let mut sum = 0i64;
            for _ in 0..n {
                sum += roll_reach(r, rng).iter().sum::<i32>() as i64;
            }
            sum as f32 / (n * consts::STAT_COUNT as i32) as f32
        };
        let lo = avg(1, &mut rng);
        let hi = avg(4, &mut rng);
        assert!(hi > lo + 40.0, "★4 reach 均值应远高于 ★1: {hi} vs {lo}");
    }

    /// 训练增量(floor-0):接近 reach 涨得越少、远超 reach 基本练不动(软墙);reach/growth 越高涨得越多。
    #[test]
    fn train_gain_floor0_reach_gated() {
        let avg = |cur: i32, reach: i32, growth: i32, rng: &mut StdRng| {
            let n = 2000;
            (0..n).map(|_| roll_gain(cur, reach, growth, 30, 0, false, false, false, rng)).sum::<f32>() / n as f32
        };
        let mut r = StdRng::seed_from_u64(2);
        assert!(avg(10, 60, 100, &mut r) > avg(110, 60, 100, &mut r), "接近 reach 应涨更少");
        let far = avg(260, 60, 100, &mut r);
        assert!(far < 0.3, "远超 reach 应基本练不动(软墙): {far}");
        assert!(avg(80, 120, 100, &mut r) > avg(80, 40, 100, &mut r), "同位置高 reach 涨更多");
        assert!(avg(40, 80, 140, &mut r) > avg(40, 80, 70, &mut r), "高 growth 涨更多");
    }

    /// 造一匹测试用马(`reach` 进 `pot_*` 列、growth=100,其余占位)。
    fn mk(reach: [i32; consts::STAT_COUNT], rarity: i16, generation: i32) -> horse::Model {
        let t = chrono::DateTime::parse_from_rfc3339("2026-01-01T00:00:00+08:00").unwrap();
        horse::Model {
            id: 1,
            owner_uin: 1,
            name: "测".into(),
            color: 0,
            sex: 0,
            generation,
            rarity,
            traits: 0,
            spd: 0,
            sta: 0,
            brs: 0,
            agi: 0,
            luk: 0,
            pot_spd: reach[0],
            pot_sta: reach[1],
            pot_brs: reach[2],
            pot_agi: reach[3],
            pot_luk: reach[4],
            growth: 100,
            vitality: 100,
            satiety: 100,
            state_at: t,
            lifespan: 800,
            lifespan_cap: 800,
            lifespan_max: 800,
            injury: 0,
            injury_until: None,
            scar: 0,
            scar_until: None,
            breed_cd_until: None,
            breed_count: 0,
            status: 0,
            wins: 0,
            races: 0,
            train_day: t.date_naive(),
            train_today: 0,
            race_day: t.date_naive(),
            race_today: 0,
            bonus_day: t.date_naive(),
            season_key: String::new(),
            season_wins: 0,
            invested: 0,
            train_total: 0,
            father_id: None,
            mother_id: None,
            created_at: t,
        }
    }

    /// 繁殖 reach 向本星基线回归:双亲低于基线→子代被拉高,高于基线→被拉低;代数 = max+1。
    #[test]
    fn breed_reach_reverts_to_baseline() {
        let avg_reach = |pr: i32| {
            let f = mk([pr, pr, pr, pr, pr], 4, 2);
            let m = mk([pr, pr, pr, pr, pr], 4, 3);
            let mut rng = StdRng::seed_from_u64(11);
            let n = 400;
            let mut sum = 0i64;
            for _ in 0..n {
                let c = breed_child(&f, &m, false, false, &mut rng);
                assert_eq!(c.generation, 4, "代数应为 max(父,母)+1");
                sum += c.birth.reach.iter().sum::<i32>() as i64;
            }
            sum as f32 / (n * consts::STAT_COUNT as i32) as f32
        };
        // ★4 基线 150:双亲 80 应被拉高、双亲 190 应被拉低,且更优双亲→更优子代(单调)。
        let low = avg_reach(80);
        let high = avg_reach(190);
        assert!(low > 90.0, "低于基线应被拉高: {low}");
        assert!(high < 185.0, "高于基线应被拉低: {high}");
        assert!(low < high, "更优双亲应得更优子代: {low} vs {high}");
    }

    /// 近亲繁殖星级回退:近亲只跌不升,平均星级明显低于正常。
    #[test]
    fn breed_incest_regresses_rarity() {
        let f = mk([100, 100, 100, 100, 100], 3, 2);
        let m = mk([100, 100, 100, 100, 100], 3, 3);
        let avg_rarity = |incest: bool| {
            let mut rng = StdRng::seed_from_u64(7);
            let n = 600;
            let sum: i64 = (0..n).map(|_| breed_child(&f, &m, incest, false, &mut rng).birth.rarity as i64).sum();
            sum as f32 / n as f32
        };
        let normal = avg_rarity(false);
        let incest = avg_rarity(true);
        assert!(incest < normal - 0.2, "近亲平均星级应明显更低: {incest} vs {normal}");
    }

    /// 寿命纯函数:寿命见底削训练效率(到 FLOOR)与赛中耐力(削 STA_PENALTY_MAX),满寿命不打折;
    /// 寿命上限随幸运出生潜力升,并钳到 [MIN, CAP_MAX]。
    #[test]
    fn lifespan_curves_and_cap() {
        // 满寿命无惩罚,见底到地板。
        assert!((train_eff(1.0) - 1.0).abs() < 1e-6, "满寿命训练不打折");
        assert!((train_eff(0.0) - consts::LIFESPAN_TRAIN_EFF_FLOOR).abs() < 1e-6, "寿命=0 训练效率到地板");
        assert!(train_eff(0.5) < 1.0 && train_eff(0.5) > consts::LIFESPAN_TRAIN_EFF_FLOOR, "PRIME 以下渐降");
        assert!((stamina_life_mult(1.0) - 1.0).abs() < 1e-6, "满寿命耐力不削");
        assert!((stamina_life_mult(0.0) - (1.0 - consts::LIFESPAN_STA_PENALTY_MAX)).abs() < 1e-6, "寿命=0 削满");
        assert!((stamina_life_mult(consts::LIFESPAN_LATE_RACE_RATIO) - 1.0).abs() < 1e-6, "起点以上不削");
        // 寿命上限单调随 pot_luk 升,并钳到上下界。
        assert_eq!(lifespan_max_for(0), consts::LIFESPAN_MIN, "低幸运钳到下界");
        assert_eq!(lifespan_max_for(9999), consts::LIFESPAN_CAP_MAX, "高幸运钳到封顶");
        assert!(lifespan_max_for(150) > lifespan_max_for(60), "上限随幸运潜力单调");
    }

    /// 全局调教衰减:头 FREE 次满额,其后单调降、恒 (0,1]、不致死,FREE+K 处约减半。
    #[test]
    fn train_total_decay_shape() {
        assert!((train_total_decay(0) - 1.0).abs() < 1e-6, "0 次满额");
        assert!((train_total_decay(consts::TRAIN_GLOBAL_FREE) - 1.0).abs() < 1e-6, "FREE 内满额");
        let a = train_total_decay(consts::TRAIN_GLOBAL_FREE + 60);
        let b = train_total_decay(consts::TRAIN_GLOBAL_FREE + 600);
        assert!(a < 1.0 && a > b && b > 0.0, "超出后单调降且恒正: {a} vs {b}");
        let half = train_total_decay(consts::TRAIN_GLOBAL_FREE + consts::TRAIN_GLOBAL_K as i32);
        assert!((half - 0.5).abs() < 0.02, "FREE+K 处约减半: {half}");
    }

    /// 饲料 / 吃饱抬高训练平均增量,饿着压低。
    #[test]
    fn feed_and_hunger_affect_gain() {
        let avg = |feed: u32, hungry: bool, well_fed: bool, rng: &mut StdRng| {
            let n = 3000;
            (0..n).map(|_| roll_gain(50, 80, 100, 30, feed, hungry, well_fed, false, rng)).sum::<f32>() / n as f32
        };
        let mut r = StdRng::seed_from_u64(9);
        let plain = avg(0, false, false, &mut r);
        let fed = avg(9, false, false, &mut r);
        let well_fed = avg(0, false, true, &mut r);
        let hungry = avg(0, true, false, &mut r);
        assert!(fed > plain, "饲料应提高平均增量: {fed} vs {plain}");
        assert!(well_fed > plain, "吃饱应提高平均增量: {well_fed} vs {plain}");
        assert!(hungry < plain, "饿着应降低平均增量: {hungry} vs {plain}");
    }

    /// 赛后掉落随幸运单调:幸运 ≤ 下限几乎不掉,高幸运远多于中幸运,幸运儿再放大。
    #[test]
    fn drop_scales_with_luck() {
        let cnt = |luk: i32, fort: bool, scale: f64, rng: &mut StdRng| {
            (0..4000).filter(|_| roll_drop(luk, fort, scale, rng).is_some()).count()
        };
        let mut r = StdRng::seed_from_u64(4);
        let floor = cnt(20, false, 1.0, &mut r); // < LUCK_DROP_FLOOR(30) → p=0
        let mid = cnt(80, false, 1.0, &mut r);
        let high = cnt(250, false, 1.0, &mut r);
        let fort = cnt(250, true, 1.0, &mut r);
        let pvp = cnt(250, false, consts::PVP_DROP_MULT, &mut r); // PvP 减半乘子
        assert_eq!(floor, 0, "幸运不足下限应不掉: {floor}");
        assert!(high > mid * 2, "高幸运掉落应远多于中幸运: {high} vs {mid}");
        assert!(fort >= high, "幸运儿应不降低掉落: {fort} vs {high}");
        assert!(pvp < high, "PvP 减半乘子应明显降低掉落: {pvp} vs {high}");
        // 掉落物必属三档池之一。
        let mut r2 = StdRng::seed_from_u64(8);
        for _ in 0..2000 {
            if let Some(it) = roll_drop(285, true, 1.0, &mut r2) {
                let ok = consts::DROP_COMMON.contains(&it)
                    || consts::DROP_MID.contains(&it)
                    || consts::Item::TREASURE.contains(&it);
                assert!(ok, "掉落物应属三档池: {:?}", it);
            }
        }
    }
}
