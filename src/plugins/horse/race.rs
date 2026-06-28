//! 比赛模拟内核:纯计算 + `seed` 播种,整场可复现。`run_race` 被 PvE/PvP 共用,
//! 道具按 owner 播种:增益挂自己、干扰挂对手。

use rand::RngExt as _;
use rand::SeedableRng;
use rand::rngs::StdRng;

use super::consts::{self, Difficulty, Item};

/// 一个生效中的道具效果(挂在某匹马身上)。
#[derive(Clone, Copy)]
struct ActiveEffect {
    item: Item,
    remaining: i32,
    /// 减速类(鸣枪/盯防)的速度乘子;其它道具置 1.0。
    mult: f32,
}

impl ActiveEffect {
    /// 非减速类道具的构造(mult 占位 1.0)。
    fn new(item: Item, remaining: i32) -> ActiveEffect {
        ActiveEffect { item, remaining, mult: 1.0 }
    }
}

/// 一回合里道具对某匹马的合成影响。
struct RoundMod {
    /// 速度乘子(增益 >1,减速 <1)。
    speed_mult: f32,
    /// 本回合定身(绊马索;被防御抵掉则为 false)。
    skip: bool,
    crit_bonus: f64,
    /// 定心丸:加到后程系数上的耐力补偿。
    stamina_bonus: f32,
    /// 稳行:抖动幅度乘数。
    jitter_mult: f32,
    /// 回马枪:本回合挡下了一个负面并要反弹给一名对手(由 [`run_race`] 落实)。
    reflect: bool,
}

/// 某匹马某回合的可视化事件(给关键帧打标 + 结算卡摘要)。
#[derive(Clone, Copy, Default)]
pub struct StepFx {
    pub crit: bool,
    /// 被冻结/滑倒原地(没躲过)。
    pub frozen: bool,
    /// 被下负面但靠敏捷闪避、照常跑。
    pub dodged: bool,
    /// 本回合受伤(局内伤病触发的那一回合)。
    pub injured: bool,
}

/// 一名参赛者的局内受伤上下文(决定受伤概率/严重度/复发)。与 `runners` 同序传入比赛内核。
#[derive(Clone, Copy)]
pub struct InjuryCtx {
    /// 寿命比 lifespan/lifespan_max:越低越易伤、越重。
    pub life_frac: f64,
    /// 伤痕重度(0/1/2/3):>0 抬高再受伤危险且只在中/重间复发。
    pub scar: i16,
    /// 生涯已比赛场数:低于新手保护期只轻伤、不复发。
    pub races: i32,
}

/// 一匹马整场的事件计数(结算卡摘要用)。
#[derive(Clone, Copy, Default)]
pub struct RunnerTally {
    pub crits: u32,
    pub frozen: u32,
    pub dodged: u32,
}

/// 结算某匹马本回合的道具影响并递减时长。护盾/回马枪不计时,留到负面来临才抵掉一块(优先级见下)。
fn process_effects(effects: &mut Vec<ActiveEffect>, progress: f32, sta: i32, pos: f32, leader_pos: f32) -> RoundMod {
    let mut m = RoundMod {
        speed_mult: 1.0,
        skip: false,
        crit_bonus: 0.0,
        stamina_bonus: 0.0,
        jitter_mult: 1.0,
        reflect: false,
    };
    let mut has_shield = false;
    let mut has_reflect = false;
    let leading = pos >= leader_pos; // 自己即领头(回合初快照)
    for e in effects.iter() {
        match e.item {
            Item::Boost => m.speed_mult *= consts::BOOST_MULT,
            Item::LateBoost if progress > consts::LATE_BOOST_PHASE => m.speed_mult *= consts::LATE_BOOST_MULT,
            // 四叶草:领先时暴击加成减半。
            Item::Clover => m.crit_bonus += if leading { consts::CLOVER_CRIT_LEADING } else { consts::CLOVER_CRIT },
            // 定心丸:按 progress 线性补后程;高耐马减半。
            Item::StaminaTonic => {
                let bonus = if sta >= consts::STAMINA_TONIC_STA_GATE {
                    consts::STAMINA_TONIC_BONUS * 0.5
                } else {
                    consts::STAMINA_TONIC_BONUS
                };
                m.stamina_bonus += bonus * progress;
            }
            // 稳行:减抖动 + 小幅速度补偿(否则对劣势方是纯负收益)。
            Item::Steady => {
                m.jitter_mult *= consts::STEADY_JITTER_MULT;
                m.speed_mult *= consts::STEADY_SPEED_MULT;
            }
            Item::Banana => m.skip = true,
            // 鸣枪/盯防:减速倍率播种时已定,存在 e.mult。
            Item::Scare | Item::Mark => m.speed_mult *= e.mult,
            Item::Shield => has_shield = true,
            Item::Reflect => has_reflect = true,
            // 终盘冲刺未到后段、及训练/养成等平时道具:本回合无即时效果。
            _ => {}
        }
    }
    // 抵负面优先级:回马枪(抵掉并反弹)> 护身符;各只用掉一块。
    let mut consume_reflect = false;
    let mut consume_shield = false;
    if m.skip {
        if has_reflect {
            consume_reflect = true;
            m.reflect = true;
            m.skip = false;
        } else if has_shield {
            consume_shield = true;
            m.skip = false;
        }
    }
    // 用掉的护盾/回马枪移除,未用的留到下次负面;其它按回合递减。
    effects.retain_mut(|e| match e.item {
        Item::Shield if consume_shield => {
            consume_shield = false;
            false
        }
        Item::Reflect if consume_reflect => {
            consume_reflect = false;
            false
        }
        Item::Shield | Item::Reflect => true,
        _ => {
            e.remaining -= 1;
            e.remaining > 0
        }
    });
    m
}

/// 一名参赛者的呈现信息(出图用)。
pub struct RunnerInfo {
    pub name: String,
    /// 主人显示名(出图用来区分同名马;NPC 为空串)。
    pub owner: String,
    pub color: i16,
    pub is_npc: bool,
}

/// 一名参赛者的赛前数据。
struct Runner {
    info: RunnerInfo,
    stats: [i32; consts::STAT_COUNT],
    traits: i32,
}

/// 一场比赛的完整结果。
pub struct RaceResult {
    /// 参赛者(顺序即泳道/位置数组下标)。
    pub runners: Vec<RunnerInfo>,
    pub track_len: f32,
    /// 位置时间线:`positions[round][runner]`,已夹到 `track_len`。
    pub positions: Vec<Vec<f32>>,
    /// 事件时间线:`event_marks[round][runner]`,与 `positions` 同形。
    pub event_marks: Vec<Vec<StepFx>>,
    pub tallies: Vec<RunnerTally>,
    /// 名次:runner 下标按第一名→末名排列。
    pub order: Vec<usize>,
    /// 玩家马的 runner 下标。
    pub player_idx: usize,
    /// 玩家马名次(1 起)。
    pub player_place: usize,
    /// 各回合是否为「关键帧」(起跑/首次反超/有马冲线)。
    pub key_rounds: Vec<usize>,
    /// 各参赛者本场最坏伤等(与 `runners` 同序,0=无);赛后由 [`mod`](super) 落库。
    pub injuries: Vec<i16>,
}

/// 最大回合数(防呆上界)。
const ROUND_CAP: usize = 300;

/// 给一名参赛者(`owner` 下标)的道具播种:增益/防护挂自己,干扰挂对手。
/// PvP 下绊马索/盯防锁最强对手,PvE 随机;盯防回合被目标敏捷减时;鸣枪按场上人数分档减速。
fn seed_effects(
    effects: &mut [Vec<ActiveEffect>],
    owner: usize,
    items: &[Item],
    runners: &[Runner],
    is_pvp: bool,
    rng: &mut StdRng,
) {
    let n = runners.len();
    for &it in items {
        match it {
            // 自身增益挂自己;整场型给满回合(终盘冲刺靠 progress 门控,只在后段生效)。
            Item::Boost => effects[owner].push(ActiveEffect::new(it, 3)),
            Item::LateBoost | Item::Clover | Item::StaminaTonic | Item::Steady => {
                effects[owner].push(ActiveEffect::new(it, ROUND_CAP as i32))
            }
            Item::Shield | Item::Reflect => effects[owner].push(ActiveEffect::new(it, 1)),
            // 绊马索:单体定身 [`FREEZE_ROUNDS`](consts::FREEZE_ROUNDS) 回合(PvP 锁最强对手)。
            Item::Banana => {
                let t = if is_pvp { strongest_other(owner, runners) } else { pick_other(owner, n, rng) };
                effects[t].push(ActiveEffect::new(it, consts::FREEZE_ROUNDS));
            }
            // 盯防:单体持续减速,回合数被目标敏捷减时(PvP 锁最强对手)。
            Item::Mark => {
                let t = if is_pvp { strongest_other(owner, runners) } else { pick_other(owner, n, rng) };
                effects[t].push(ActiveEffect {
                    item: it,
                    remaining: mark_rounds(&runners[t]),
                    mult: consts::MARK_SLOW_MULT,
                });
            }
            // 鸣枪惊群:全体对手单回合减速(人多更狠)。
            Item::Scare => {
                let mult = if n.saturating_sub(1) > consts::SCARE_BIG_FIELD {
                    consts::SCARE_SLOW_BIG
                } else {
                    consts::SCARE_SLOW_SMALL
                };
                for (t, slot) in effects.iter_mut().enumerate() {
                    if t != owner {
                        slot.push(ActiveEffect { item: it, remaining: 1, mult });
                    }
                }
            }
            // 非赛中道具,跳过。
            _ => continue,
        }
    }
}

/// 盯防对某目标的有效回合:基础 [`MARK_ROUNDS`](consts::MARK_ROUNDS) 减去目标敏捷「减时」(反射神经特性除数更小),下限 1。
fn mark_rounds(target: &Runner) -> i32 {
    let div = if consts::Trait::Reflex.in_mask(target.traits) {
        consts::AGI_REDUCE_DIV_REFLEX
    } else {
        consts::AGI_REDUCE_DIV
    };
    (consts::MARK_ROUNDS - target.stats[consts::Stat::Agi.idx()] / div).max(1)
}

/// 场上最强(五维和最大)的非 `owner` 对手下标(只剩自己时返自己)。
fn strongest_other(owner: usize, runners: &[Runner]) -> usize {
    (0..runners.len()).filter(|&i| i != owner).max_by_key(|&i| runners[i].stats.iter().sum::<i32>()).unwrap_or(owner)
}

/// 随机挑一个非 `owner` 的下标(只剩自己时返自己)。
fn pick_other(owner: usize, n: usize, rng: &mut StdRng) -> usize {
    if n <= 1 {
        return owner;
    }
    let mut t = rng.random_range(0..n - 1);
    if t >= owner {
        t += 1;
    }
    t
}

/// 比赛核心循环(PvE/PvP 共用):逐回合推进、记位置时间线、定名次与关键帧。`player_idx` 为主视角马,
/// `player_place` 据它算。
fn run_race(
    runners: Vec<Runner>,
    mut effects: Vec<Vec<ActiveEffect>>,
    track_len: f32,
    player_idx: usize,
    form_sigma: f32,
    ctx: Vec<InjuryCtx>,
    mut rng: StdRng,
) -> RaceResult {
    let n = runners.len();
    // 每匹马一个整场「手感」系数(乘进速度,全程相关、不被回合平均掉);PvE 传 0 不用。
    let forms: Vec<f32> = (0..n).map(|_| race_form(form_sigma, &mut rng)).collect();
    // pos 截顶累计位移(供显示/时间线/leader_pos);raw_pos 不截顶,仅供同回合冲线的名次决胜
    // (越线越多≈越早过线),消除「同回合并列按下标」对低下标(玩家/房主)的偏袒。
    let mut pos = vec![0.0f32; n];
    let mut raw_pos = vec![0.0f32; n];
    let mut finished = vec![false; n];
    let mut finish_round = vec![usize::MAX; n];
    let mut positions: Vec<Vec<f32>> = Vec::new();
    let mut event_marks: Vec<Vec<StepFx>> = Vec::new();
    let mut tallies = vec![RunnerTally::default(); n];
    // 各马本场已受伤等级(0=未伤);受伤后当场跛行(减速)、本场不再掷,赛末作 RaceResult.injuries。
    let mut inj_sev = vec![0i16; n];
    let mut leader: Option<usize> = None;
    let mut key_rounds: Vec<usize> = vec![0]; // 起跑帧

    // 前三(不足三人则全员)冲线即可结算,其余按当前 pos 垫后,不必等全员到线。
    let podium = n.min(3);
    for round in 0..ROUND_CAP {
        let mut any_finished_this_round = false;
        let mut round_fx = vec![StepFx::default(); n];
        // 回合初的领先位置(给落后者「追赶暴击」用,回合内用快照)。
        let leader_pos = pos.iter().copied().fold(0.0f32, f32::max);
        for i in 0..n {
            if finished[i] {
                continue;
            }
            let progress_i = (pos[i] / track_len).clamp(0.0, 1.0);
            let sta_i = runners[i].stats[consts::Stat::Sta.idx()];
            let mut rmod = process_effects(&mut effects[i], progress_i, sta_i, pos[i], leader_pos);
            // 带伤当场跛行:本场剩余回合速度按伤等打折。
            if inj_sev[i] > 0 {
                rmod.speed_mult *= consts::INJURY_LIMP_MULT[(inj_sev[i] - 1) as usize];
            }
            // 回马枪:挡下负面并把一次定身反弹给当前领先者(挂到其效果队列,下次结算生效);领先者即自己则随机挑。
            if rmod.reflect {
                let lead = (0..n).max_by(|&a, &b| pos[a].total_cmp(&pos[b])).unwrap_or(i);
                let t = if lead == i { pick_other(i, n, &mut rng) } else { lead };
                effects[t].push(ActiveEffect::new(Item::Banana, consts::FREEZE_ROUNDS));
            }
            let (dist, fx) =
                step(&runners[i].stats, runners[i].traits, forms[i], pos[i], track_len, leader_pos, &rmod, &mut rng);
            round_fx[i] = fx;
            tallies[i].crits += fx.crit as u32;
            tallies[i].frozen += fx.frozen as u32;
            tallies[i].dodged += fx.dodged as u32;
            raw_pos[i] += dist;
            pos[i] = raw_pos[i].min(track_len);
            // 局内受伤:有位移、本场未伤、且非 NPC(NPC 不掷,保动态难度平衡)才掷。命中→当场起跛行 + 标事件。
            if dist > 0.0 && inj_sev[i] == 0 && !runners[i].info.is_npc {
                let c = &ctx[i];
                let progress = (pos[i] / track_len).clamp(0.0, 1.0) as f64;
                let phase = 1.0 + consts::INJURY_LATE_RAMP * (progress - consts::INJURY_LATE_PHASE).max(0.0);
                let life = 1.0 + consts::INJURY_LIFE_GAIN * (1.0 - c.life_frac).powi(2);
                let resist_stat =
                    (runners[i].stats[consts::Stat::Sta.idx()] + runners[i].stats[consts::Stat::Luk.idx()]) as f64;
                let resist = (1.0 - resist_stat / consts::INJURY_RESIST_DIV).clamp(consts::INJURY_RESIST_FLOOR, 1.0);
                let scar_m = 1.0 + consts::SCAR_HAZARD_GAIN * c.scar as f64;
                let trait_m =
                    if consts::Trait::IronHoof.in_mask(runners[i].traits) { consts::TRAIT_INJURY_MULT } else { 1.0 };
                let p = (consts::INJURY_DIST_HAZARD * dist as f64 * phase * life * resist * scar_m * trait_m)
                    .clamp(0.0, 0.9);
                if rng.random_bool(p) {
                    inj_sev[i] = roll_injury_severity(c, &mut rng);
                    round_fx[i].injured = true;
                }
            }
            if raw_pos[i] >= track_len {
                finished[i] = true;
                finish_round[i] = round;
                any_finished_this_round = true;
            }
        }
        positions.push(pos.clone());
        let froze_this_round = round_fx.iter().any(|f| f.frozen);
        event_marks.push(round_fx);

        // 关键帧:有马冲线、领先者易主或有马被冻。
        let cur_leader = (0..n).max_by(|&a, &b| pos[a].total_cmp(&pos[b]));
        if any_finished_this_round || froze_this_round || (cur_leader != leader && round > 0) {
            key_rounds.push(round);
        }
        leader = cur_leader;

        if finished.iter().filter(|&&f| f).count() >= podium {
            break;
        }
    }

    // 名次:先冲线的在前(回合早优先),同回合按不截顶的越线量决胜(与下标无关,确定可复现);
    // 没冲线的按累计位移垫后(此时 raw_pos == pos)。
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| finish_round[a].cmp(&finish_round[b]).then(raw_pos[b].total_cmp(&raw_pos[a])));
    let player_place = order.iter().position(|&i| i == player_idx).map(|p| p + 1).unwrap_or(n);

    key_rounds.push(positions.len().saturating_sub(1)); // 冲线帧
    key_rounds.sort_unstable();
    key_rounds.dedup();

    RaceResult {
        runners: runners.into_iter().map(|r| r.info).collect(),
        track_len,
        positions,
        event_marks,
        tallies,
        order,
        player_idx,
        player_place,
        key_rounds,
        injuries: inj_sev,
    }
}

/// 局内受伤严重度:带伤痕(复发)只在中/重间取、伤痕越重越偏重伤;否则按基础权重(中/重随寿命见底加权)抽轻/中/重。
/// 新手保护期(生涯 < [`NEWBIE_INJURY_GRACE`](consts::NEWBIE_INJURY_GRACE) 场)钳到轻伤。
fn roll_injury_severity(ctx: &InjuryCtx, rng: &mut StdRng) -> i16 {
    let sev = if ctx.scar > 0 {
        let heavy_pct =
            consts::SCAR_RELAPSE_HEAVY_BASE + consts::SCAR_RELAPSE_HEAVY_STEP * (ctx.scar.clamp(1, 3) as u32 - 1);
        if rng.random_range(0..100) < heavy_pct { 3 } else { 2 }
    } else {
        let frac = ctx.life_frac;
        let weights = [
            consts::INJURY_SEVERITY_BASE[0],
            consts::INJURY_SEVERITY_BASE[1] + (consts::INJURY_SEV_LIFE_MED * (1.0 - frac)).round() as u32,
            consts::INJURY_SEVERITY_BASE[2] + (consts::INJURY_SEV_LIFE_HVY * (1.0 - frac)).round() as u32,
        ];
        super::logic::weighted_pick(&weights, rng) as i16 + 1
    };
    if ctx.races < consts::NEWBIE_INJURY_GRACE { 1 } else { sev }
}

/// 跑一场 PvE 比赛(玩家 + NPC)。`difficulty` 定赛道/对手强度,`player_ctx` 为玩家受伤上下文(NPC 不受伤),
/// `seed` 决定全程随机(可复现)。
pub fn simulate(
    player: RunnerInfo,
    player_stats: [i32; consts::STAT_COUNT],
    player_traits: i32,
    player_ctx: InjuryCtx,
    difficulty: Difficulty,
    items: &[Item],
    seed: u64,
) -> RaceResult {
    let mut rng = StdRng::seed_from_u64(seed);
    let track_len = difficulty.track_len();
    // 动态难度:NPC = 玩家五维的镜像 × 难度系数;镜像使 min-max 无法钻空。
    let ratio = difficulty.npc_ratio();
    let mut runners: Vec<Runner> = vec![Runner { info: player, stats: player_stats, traits: player_traits }];
    for k in 0..difficulty.npc_count() {
        runners.push(gen_npc(k, &player_stats, ratio, &mut rng));
    }
    let n = runners.len();
    let mut effects: Vec<Vec<ActiveEffect>> = vec![Vec::new(); n];
    seed_effects(&mut effects, 0, items, &runners, false, &mut rng);
    // NPC 使坏:每个 NPC 按难度概率向玩家(下标 0)丢一个绊马索(直接挂玩家槽,不走 seed_effects 的随机选靶)。
    // 用定身(`skip` 是 bool、不叠加)而非减速,避免多个 NPC 的减速相乘把玩家压死。
    let neg_prob = consts::NPC_NEG_ITEM_PROB[difficulty.idx()];
    for _ in 1..n {
        if rng.random_bool(neg_prob) {
            effects[0].push(ActiveEffect::new(Item::Banana, consts::FREEZE_ROUNDS));
        }
    }
    // 受伤上下文:玩家用真 ctx,NPC 占位(life_frac 1、无伤痕、过新手保护;run_race 对 NPC 跳过受伤掷骰)。
    let npc_ctx = InjuryCtx { life_frac: 1.0, scar: 0, races: i32::MAX };
    let ctx: Vec<InjuryCtx> = std::iter::once(player_ctx).chain(std::iter::repeat_n(npc_ctx, n - 1)).collect();
    run_race(runners, effects, track_len, 0, 0.0, ctx, rng) // PvE 不加手感系数
}

/// 一名 PvP 参赛者:呈现信息 + 五维 effective 值 + 带的道具 + 受伤上下文。
pub struct PvpEntrant {
    pub info: RunnerInfo,
    /// 五维 effective 值。
    pub stats: [i32; consts::STAT_COUNT],
    pub traits: i32,
    pub items: Vec<Item>,
    /// 寿命比(局内受伤上下文)。
    pub life_frac: f64,
    /// 伤痕重度(局内受伤上下文)。
    pub scar: i16,
    /// 生涯已比赛场数(局内受伤上下文)。
    pub races: i32,
}

/// 跑一场 PvP 比赛(全真人,无 NPC)。各人道具按 owner 播种;名次用返回的 `order` 映射回各人。各人都掷局内受伤。
pub fn simulate_pvp(entrants: Vec<PvpEntrant>, track_len: f32, seed: u64) -> RaceResult {
    let mut rng = StdRng::seed_from_u64(seed);
    let n = entrants.len();
    let mut runners: Vec<Runner> = Vec::with_capacity(n);
    let mut items_per: Vec<Vec<Item>> = Vec::with_capacity(n);
    let mut ctx: Vec<InjuryCtx> = Vec::with_capacity(n);
    for e in entrants {
        ctx.push(InjuryCtx { life_frac: e.life_frac, scar: e.scar, races: e.races });
        runners.push(Runner { info: e.info, stats: e.stats, traits: e.traits });
        items_per.push(e.items);
    }
    let mut effects: Vec<Vec<ActiveEffect>> = vec![Vec::new(); n];
    for (owner, items) in items_per.iter().enumerate() {
        seed_effects(&mut effects, owner, items, &runners, true, &mut rng);
    }
    // PvP 加整场手感系数:制造冷门。
    run_race(runners, effects, track_len, 0, consts::PVP_FORM_SIGMA, ctx, rng)
}

/// 整场「手感」系数:绕 1.0 的正态扰动(夹到稳定区间);`sigma <= 0`(PvE)返 1.0 不扰动。
fn race_form(sigma: f32, rng: &mut StdRng) -> f32 {
    if sigma <= 0.0 {
        return 1.0;
    }
    let z = (rng.random_range(0.0..1.0) + rng.random_range(0.0..1.0) + rng.random_range(0.0..1.0)) / 3.0;
    (1.0 + (z - 0.5) / 0.16667 * sigma).clamp(0.55, 1.45)
}

/// 单回合位移。`rmod` 为道具合成影响,`leader_pos` 为回合初领先位置(给落后者追赶暴击),`form` 为手感系数。
/// 五维分工:速度定基础、敏捷开局抢位兼躲负面、耐力抗后程、爆发主驱暴击(幸运不进赛中公式)。
#[allow(clippy::too_many_arguments)]
fn step(
    stats: &[i32; consts::STAT_COUNT],
    traits: i32,
    form: f32,
    pos: f32,
    track_len: f32,
    leader_pos: f32,
    rmod: &RoundMod,
    rng: &mut StdRng,
) -> (f32, StepFx) {
    use consts::Trait;
    let mut fx = StepFx::default();
    let agi = stats[consts::Stat::Agi.idx()] as f32;
    if rmod.skip {
        // 敏捷闪避:被定身时有概率甩开、照常跑这一回合;「反射神经」特性再加成、封顶更高。
        // agi 恒 ≥ 0,故只需封上界(下界 0 永不触发)。
        let mut dodge = (agi / consts::STAT_EFFECT_REF as f32 * consts::AGI_DODGE_MAX).min(consts::AGI_DODGE_MAX);
        if Trait::Reflex.in_mask(traits) {
            dodge = (dodge * consts::TRAIT_REFLEX_DODGE_MULT).min(consts::AGI_DODGE_CAP_REFLEX);
        }
        if !rng.random_bool(dodge as f64) {
            return (0.0, StepFx { frozen: true, ..Default::default() });
        }
        fx.dodged = true;
    }
    let spd = stats[consts::Stat::Spd.idx()] as f32;
    let sta = stats[consts::Stat::Sta.idx()] as f32;
    let brs = stats[consts::Stat::Brs.idx()] as f32;
    let smax = consts::STAT_MAX as f32;
    // 幸运不进赛中公式(走赛后掉落/奖励,见 mod);速度凹响应见 [`SPEED_BASE_EXP`](consts::SPEED_BASE_EXP)。

    let progress = (pos / track_len).clamp(0.0, 1.0);
    // 速度凹响应:线性会让高速度独大,凹曲线给边际递减。
    let mut base = spd.powf(consts::SPEED_BASE_EXP) * consts::SPEED_BASE_COEF * rmod.speed_mult * form;
    // 敏捷起跑:开局前段起步快;「闪电起步」特性再加成。
    if progress < consts::AGI_START_PHASE {
        let mut boost = agi / smax * consts::AGI_START_BOOST;
        if Trait::QuickStart.in_mask(traits) {
            boost += consts::TRAIT_START_BONUS;
        }
        base *= 1.0 + boost;
    }
    // 「疾风」特性:前半程速度加成(后半不生效,不助长长赛道独大)。
    if Trait::Gale.in_mask(traits) && progress < consts::TRAIT_GALE_PHASE {
        base *= consts::TRAIT_GALE_MULT;
    }
    // 「韧者」特性:在减抖动之外给小幅速度补偿,抵掉「减自身方差」的净负(配合下方 TRAIT_JITTER_MULT)。
    if Trait::Tenacious.in_mask(traits) {
        base *= consts::TRAIT_TENACITY_SPEED_MULT;
    }
    // 爆发直接进位移(不只走暴击概率)。
    base *= 1.0 + brs / consts::STAT_EFFECT_REF as f32 * consts::BURST_BASE_SCALE;
    // 后程系数:progress 越大且耐力越低掉速越狠;「后程之王」特性后半程加成;「定心丸」道具补后程(rmod.stamina_bonus)。
    let late = if Trait::LateSurge.in_mask(traits) && progress > 0.5 { consts::TRAIT_LATE_BONUS } else { 0.0 };
    let stamina_factor = (1.0 - progress * (1.0 - sta / consts::STAMINA_STAT_REF as f32) * consts::STAMINA_COEFF
        + late
        + rmod.stamina_bonus)
        .clamp(consts::STAMINA_FACTOR_MIN, consts::STAMINA_FACTOR_MAX);
    // 暴击:爆发主驱 + 四叶草 + 落后追赶(挂自身爆发,「追击者」特性再放大)+「暴击体质」特性。
    let mut comeback = ((leader_pos - pos) / track_len).clamp(0.0, 1.0) as f64
        * (consts::COMEBACK_BASE + brs as f64 / consts::STAT_EFFECT_REF as f64 * consts::COMEBACK_BRS_SCALE);
    if Trait::Pursuer.in_mask(traits) {
        comeback *= consts::TRAIT_PURSUIT_MULT;
    }
    let trait_crit = if Trait::CritBeast.in_mask(traits) { consts::TRAIT_CRIT_BONUS } else { 0.0 };
    let crit_prob = (brs as f64 / consts::STAT_EFFECT_REF as f64 * consts::BURST_CRIT_SCALE
        + rmod.crit_bonus
        + comeback
        + trait_crit)
        .clamp(0.0, consts::CRIT_PROB_CAP);
    fx.crit = rng.random_bool(crit_prob);
    let crit_mult = if fx.crit { consts::BURST_CRIT_MULT } else { 1.0 };
    // 抖动:「韧者」特性 +「稳行」道具(rmod.jitter_mult)减幅。
    let tenacity = if Trait::Tenacious.in_mask(traits) { consts::TRAIT_JITTER_MULT } else { 1.0 };
    let jitter_amp = base * consts::JITTER_COEFF * tenacity * rmod.jitter_mult;
    let jitter = (rng.random_range(0.0..1.0) - 0.5) * 2.0 * jitter_amp;

    ((base * stamina_factor * crit_mult + jitter).max(0.0), fx)
}

/// NPC 名字池。
const NPC_NAMES: [&str; 10] = ["影疾", "踏雪", "黑旋风", "赤兔", "追风", "乌云", "白蹄乌", "流星", "御风", "霜蹄"];

/// 玩家「实力」= 前四维(速/耐/爆/敏)均值,供按实力缩放名次奖励。刻意排除幸运:它在名次奖已有独立
/// `luck_mult` 通道(见 [`mod`](super) 比赛结算),计入此处会双计。全维相等的马前四维均值 = 五维均值,故
/// [`REWARD_POWER_REF`](consts::REWARD_POWER_REF) 标定不变;NPC 难度按逐维镜像生成、不经此函数。
pub fn player_power(stats: &[i32; consts::STAT_COUNT]) -> f32 {
    let n = consts::Stat::Luk.idx(); // 取幸运之前的四维(速/耐/爆/敏)
    stats[..n].iter().sum::<i32>() as f32 / n as f32
}

/// 即时生成一匹 NPC 马:玩家五维的镜像 × `ratio` + 个体扰动。`ratio` 由难度档定(见
/// [`Difficulty::npc_ratio`](consts::Difficulty::npc_ratio))。
fn gen_npc(k: usize, player_stats: &[i32; consts::STAT_COUNT], ratio: f32, rng: &mut StdRng) -> Runner {
    let stats: [i32; consts::STAT_COUNT] = std::array::from_fn(|i| {
        let target = player_stats[i] as f32 * ratio;
        let wobble = target * rng.random_range(-0.14..0.14);
        // 不夹 STAT_MAX:强玩家的对手也要跟得上,只夹健壮性上界。
        (target + wobble).round().clamp(8.0, consts::STAT_SANITY_MAX as f32) as i32
    });
    Runner {
        info: RunnerInfo {
            name: NPC_NAMES[k % NPC_NAMES.len()].to_string(),
            owner: String::new(),
            color: rng.random_range(0..consts::COLOR_COUNT),
            is_npc: true,
        },
        stats,
        traits: 0,
    }
}

/// 名次奖励(冠/亚/季按系数,超出无奖)× 实力系数(power / [`REWARD_POWER_REF`](consts::REWARD_POWER_REF),
/// 钳到 [`REWARD_POWER_CLAMP`](consts::REWARD_POWER_CLAMP)):越强的马同档赢得越多。
pub fn reward_for(place: usize, diff: Difficulty, power: f32) -> i64 {
    let (lo, hi) = consts::REWARD_POWER_CLAMP;
    let power_factor = (power / consts::REWARD_POWER_REF).clamp(lo, hi);
    consts::PLACE_REWARD_FACTOR
        .get(place.saturating_sub(1))
        .map(|f| (diff.reward_base() as f32 * f * power_factor).round() as i64)
        .unwrap_or(0)
}

// —— PvP 段位赔率结算(无匹配·只开房;马段位 ELO + 即时 power 定赔率;马主段位纯荣誉)——

/// 各马隐含赢率:组合分 `C_i = elo_i + POWER_TO_ELO×(power_i − REWARD_POWER_REF)`,`p = softmax(C / S)`,
/// `S = ELO_SCALE / ln(10)`。`powers` 已含按马战意(PvP-only)乘子。返回归一概率(Σ=1)。
pub fn pvp_win_probs(elos: &[i32], powers: &[f32]) -> Vec<f64> {
    let n = elos.len();
    if n == 0 {
        return Vec::new();
    }
    let s = consts::ELO_SCALE / std::f64::consts::LN_10;
    let c: Vec<f64> = elos
        .iter()
        .zip(powers)
        .map(|(&e, &pw)| e as f64 + consts::POWER_TO_ELO * (pw as f64 - consts::REWARD_POWER_REF as f64))
        .collect();
    let mx = c.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let ex: Vec<f64> = c.iter().map(|&ci| ((ci - mx) / s).exp()).collect();
    let sum: f64 = ex.iter().sum();
    if sum <= 0.0 {
        return vec![1.0 / n as f64; n];
    }
    ex.iter().map(|&e| e / sum).collect()
}

/// Harville:由赢率 `p` 递推前三名概率 `(P1, P2, P3)`(N≤8,O(N³) 可接受)。
fn harville_top3(p: &[f64]) -> (Vec<f64>, Vec<f64>, Vec<f64>) {
    let n = p.len();
    let p1 = p.to_vec();
    let mut p2 = vec![0.0; n];
    let mut p3 = vec![0.0; n];
    for i in 0..n {
        for j in 0..n {
            if j == i {
                continue;
            }
            let dj = 1.0 - p[j];
            if dj <= 1e-12 {
                continue;
            }
            p2[i] += p[j] * (p[i] / dj); // j 第一、i 第二
            for k in 0..n {
                if k == i || k == j {
                    continue;
                }
                let djk = 1.0 - p[j] - p[k];
                if djk <= 1e-12 {
                    continue;
                }
                p3[i] += p[j] * (p[k] / dj) * (p[i] / djk); // j 第一、k 第二、i 第三
            }
        }
    }
    (p1, p2, p3)
}

/// PvP 赔率派彩(广义 waterfill 单次联合投影):零和守恒(Σ=q)、倒扣有上限(floor)。
/// `p` 隐含赢率、`order[r]` = 第 r 名的下标、`stakes` 各注、`q` 派彩总额(= pool − rake)。
/// 返回各马派彩 `gross_i`(按下标;Σ=q、各 ≥ floor_i;奖圈 floor = (1−REVCAP)×stake、圈外 0 → 倒扣 ≤75%/100%)。
/// 严禁两步法(先填到 0 再事后钳 floor 会凭空铸币):此处只用单条 `g_i = max(floor_i, base_i − λ)` 的 λ 二分。
pub fn pvp_payout(p: &[f64], order: &[usize], stakes: &[i64], q: i64) -> Vec<i64> {
    let n = p.len();
    if n == 0 || q <= 0 {
        return vec![0; n];
    }
    let psum: f64 = p.iter().sum();
    let p: Vec<f64> = if psum > 0.0 { p.iter().map(|&x| x / psum).collect() } else { vec![1.0 / n as f64; n] };
    let (p1, p2, p3) = harville_top3(&p);
    let pf = consts::PVP_PAYOUT_FACTOR; // [0.6,0.25,0.15],Σ=1
    let fair_e: Vec<f64> = (0..n).map(|i| p1[i] * pf[0] as f64 + p2[i] * pf[1] as f64 + p3[i] * pf[2] as f64).collect();
    let erank: Vec<f64> = (0..n)
        .map(|i| 1.0 + (0..n).filter(|&j| j != i).map(|j| p[j] / (p[i] + p[j]).max(1e-12)).sum::<f64>())
        .collect();
    let mut rank_of = vec![0usize; n];
    for (r, &idx) in order.iter().enumerate() {
        rank_of[idx] = r + 1;
    }
    let place_value = |r: usize| -> f64 { if r >= 1 && r <= pf.len() { pf[r - 1] as f64 } else { 0.0 } };
    // raw_i = 1/N + S1×(pf_i − E_i) + S2×(Erank_i − rank_i)/N(三项各自 Σ=1/0/0 → Σ raw = 1)。
    let raw: Vec<f64> = (0..n)
        .map(|i| {
            1.0 / n as f64
                + consts::PVP_ODDS_S1 * (place_value(rank_of[i]) - fair_e[i])
                + consts::PVP_ODDS_S2 * (erank[i] - rank_of[i] as f64) / n as f64
        })
        .collect();
    let base: Vec<f64> = raw.iter().map(|&r| r * q as f64).collect();
    let floor: Vec<f64> = (0..n)
        .map(|i| if rank_of[i] <= pf.len() { (1.0 - consts::PVP_REVCAP_INRING) * stakes[i] as f64 } else { 0.0 })
        .collect();
    // λ 二分:g_i = max(floor_i, base_i − λ)。λ=0 时 Σ ≥ q(Σbase=q、max≥base);λ↑ Σ 单减;Σfloor ≤ q 保可行。
    let sum_at = |lam: f64| -> f64 { (0..n).map(|i| floor[i].max(base[i] - lam)).sum::<f64>() };
    let (mut lo, mut hi) = (0.0_f64, base.iter().cloned().fold(0.0_f64, f64::max).max(1.0));
    for _ in 0..100 {
        let mid = (lo + hi) / 2.0;
        if sum_at(mid) > q as f64 {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let _ = lo;
    let lam = hi;
    let g: Vec<f64> = (0..n).map(|i| floor[i].max(base[i] - lam)).collect();
    // 取整,零头给冠军,保证 Σ == q(守恒)。
    let mut gi: Vec<i64> = g.iter().map(|&x| x.round().max(0.0) as i64).collect();
    let diff = q - gi.iter().sum::<i64>();
    if let Some(&w) = order.first() {
        gi[w] = (gi[w] + diff).max(0);
    }
    gi
}

/// 单场 ELO 增量(按下标):`ΔR_i = K_i/(N−1) × Σ_{j≠i}(S_ij − E_ij)`,`S_ij = [rank_i<rank_j]`,
/// `E_ij = 1/(1+10^((R_j−R_i)/ELO_SCALE))`。`ranks[i]` 名次(1-based),`k[i]` 该马本轨 K 值。
pub fn elo_deltas(ranks: &[usize], elos: &[i32], k: &[f64]) -> Vec<i32> {
    let n = elos.len();
    if n < 2 {
        return vec![0; n];
    }
    (0..n)
        .map(|i| {
            let s: f64 = (0..n)
                .filter(|&j| j != i)
                .map(|j| {
                    let sij = if ranks[i] < ranks[j] { 1.0 } else { 0.0 };
                    let eij = 1.0 / (1.0 + 10f64.powf((elos[j] - elos[i]) as f64 / consts::ELO_SCALE));
                    sij - eij
                })
                .sum();
            (k[i] / (n - 1) as f64 * s).round() as i32
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 赔率派彩:零和守恒(Σ=q)、奖圈倒扣不超 REVCAP、冷门夺冠净赚、强马爆冷重亏。
    #[test]
    fn pvp_payout_conserves_and_caps() {
        let p = [0.7, 0.2, 0.1]; // 强(idx0)/中/弱
        let stakes = [100i64, 100, 100];
        let q = 285; // pool 300 − 5% rake
        let g = pvp_payout(&p, &[0, 1, 2], &stakes, q); // 强马夺冠
        assert_eq!(g.iter().sum::<i64>(), q, "守恒");
        assert!(g.iter().all(|&x| x >= 0));
        assert!(g[0] - 100 < 120, "热门夺冠不暴利: {}", g[0] - 100);
        let g2 = pvp_payout(&p, &[2, 1, 0], &stakes, q); // 强马(idx0)垫底(第3名,奖圈内 floor=25)
        assert_eq!(g2.iter().sum::<i64>(), q, "守恒");
        assert!(g2[0] >= 25, "奖圈倒扣不超 75%(≥floor25): {}", g2[0]);
        assert!(g2[0] <= 75, "强马爆冷应重亏: {}", g2[0]);
        assert!(g2[2] > 100, "冷门夺冠应净赚: {}", g2[2]);
    }

    /// 圈外(第4名起)floor=0,可倒扣满注。
    #[test]
    fn pvp_payout_out_of_ring_floor_zero() {
        let p = [0.4, 0.3, 0.2, 0.1];
        let stakes = [50i64; 4];
        let q = (200.0 * 0.95) as i64; // 190
        let g = pvp_payout(&p, &[1, 2, 3, 0], &stakes, q); // 强马 idx0 垫底(第4名,圈外)
        assert_eq!(g.iter().sum::<i64>(), q);
        assert!(g[0] >= 0 && g[0] < 50, "圈外强马爆冷可净亏(gross<stake): {}", g[0]);
    }

    fn info(name: &str) -> RunnerInfo {
        RunnerInfo { name: name.to_string(), owner: "主人".into(), color: 0, is_npc: false }
    }

    /// 健康马的受伤上下文(满寿命、无伤痕、过新手保护):平衡测试统一按健康构造。
    fn hc() -> InjuryCtx {
        InjuryCtx { life_frac: 1.0, scar: 0, races: i32::MAX }
    }

    /// 同 seed 同输入 → 同名次同时间线(可复现)。
    #[test]
    fn reproducible() {
        let stats = [80, 70, 60, 50, 40];
        let a = simulate(info("甲"), stats, 0, hc(), Difficulty::Normal, &[], 12345);
        let b = simulate(info("甲"), stats, 0, hc(), Difficulty::Normal, &[], 12345);
        assert_eq!(a.order, b.order);
        assert_eq!(a.positions.len(), b.positions.len());
        assert_eq!(a.player_place, b.player_place);
    }

    /// 比赛会结束(不撞回合上界),且至少前三(不足三人则全员)摸到终点。
    #[test]
    fn terminates_and_finishes() {
        let r = simulate(info("甲"), [90, 80, 70, 60, 50], 0, hc(), Difficulty::Easy, &[], 999);
        assert!(r.positions.len() < ROUND_CAP);
        let last = r.positions.last().unwrap();
        let finished = last.iter().filter(|&&p| p >= r.track_len - 0.001).count();
        let podium = r.runners.len().min(3);
        assert!(finished >= podium, "应至少 {podium} 匹冲线: {last:?}");
        assert!(r.player_place >= 1 && r.player_place <= r.runners.len());
    }

    /// 带道具(冲刺 + 四叶草)平均名次优于裸跑(统计意义)。
    #[test]
    fn items_help() {
        let stats = [55, 55, 55, 55, 55];
        let avg_place = |items: &[Item]| {
            let mut sum = 0usize;
            for s in 0..60u64 {
                sum += simulate(info("甲"), stats, 0, hc(), Difficulty::Normal, items, s).player_place;
            }
            sum as f32 / 60.0
        };
        let bare = avg_place(&[]);
        let buffed = avg_place(&[Item::Boost, Item::Clover]);
        assert!(buffed < bare - 0.2, "带道具应平均名次更好: {buffed} vs {bare}");
    }

    /// 新自身增益道具方向性:定心丸(抗后程)+ 终盘冲刺(后程提速)在长赛道明显改善名次。
    /// (干扰类/稳行等单场微效道具走同 [`Boost`](Item::Boost) 的 speed_mult 管线,由 [`items_help`] 覆盖,
    /// 单场效应小易被 rng 淹没,不另做断言。)
    #[test]
    fn new_self_buff_items_help() {
        let avg_self = |items: &[Item]| {
            let mut sum = 0usize;
            for s in 0..200u64 {
                sum += simulate(info("甲"), [90, 70, 80, 60, 60], 0, hc(), Difficulty::Hard, items, s).player_place;
            }
            sum as f32 / 200.0
        };
        let bare = avg_self(&[]);
        let buffed = avg_self(&[Item::StaminaTonic, Item::LateBoost]);
        assert!(buffed < bare - 0.1, "定心丸+终盘冲刺应明显改善名次: {buffed} vs {bare}");
    }

    /// 敏捷靠闪避:大师 PvE(NPC 高频投绊马索)下,高敏捷明显比低敏捷夺冠更多。
    /// (单场闪避效应小,清水/2 人局会被手感噪声淹没,故用干扰密集的大师 + 大样本测。)
    #[test]
    fn agility_dodges_negatives() {
        // 大样本:局内受伤掷骰嵌进同一 rng 流会限速玩家、压低 Master 胜率,需大样本才稳住「高敏多赢」信号。
        let n = 3000u64;
        let wins = |agi: i32| {
            (0..n)
                .filter(|&s| {
                    simulate(info("守"), [100, 100, 100, agi, 100], 0, hc(), Difficulty::Master, &[], s).player_place
                        == 1
                })
                .count()
        };
        let low = wins(30);
        let high = wins(190);
        assert!(high > low + 12, "高敏捷在干扰密集场应明显多赢: {high} vs {low}/{n}");
    }

    /// 盯防「减时」:目标敏捷越高、有效回合越少;反射神经更易减;下限 1。
    #[test]
    fn mark_rounds_reduced_by_agility() {
        let mk = |agi: i32, traits: i32| Runner { info: info("x"), stats: [80, 80, 80, agi, 80], traits };
        assert_eq!(mark_rounds(&mk(0, 0)), consts::MARK_ROUNDS, "低敏不减时");
        assert!(mark_rounds(&mk(240, 0)) < mark_rounds(&mk(0, 0)), "高敏应减时");
        let reflex = consts::Trait::Reflex.bit();
        assert!(mark_rounds(&mk(150, reflex)) < mark_rounds(&mk(150, 0)), "反射神经更易减时");
        assert!(mark_rounds(&mk(9999, 0)) >= 1, "减时下限为 1");
    }

    /// 护身符能抵掉一个负面:带护身符的马被惊扰,平均名次明显好于不带的同马(守方敏捷设 0 排除闪避干扰)。
    #[test]
    fn shield_blocks_negative() {
        // 大样本:PvP 双方都掷局内受伤(跛行摆幅远大于 1 回合定身),需大样本压住噪声、稳住「护身符更优」信号。
        let avg_victim_place = |with_shield: bool| {
            let n = 4000u64;
            let mut sum = 0usize;
            for s in 0..n {
                let attacker = PvpEntrant {
                    info: info("攻"),
                    stats: [80, 80, 80, 0, 80],
                    traits: 0,
                    items: vec![Item::Banana],
                    life_frac: 1.0,
                    scar: 0,
                    races: i32::MAX,
                };
                let items = if with_shield { vec![Item::Shield] } else { vec![] };
                let victim = PvpEntrant {
                    info: info("守"),
                    stats: [80, 80, 80, 0, 80],
                    traits: 0,
                    items,
                    life_frac: 1.0,
                    scar: 0,
                    races: i32::MAX,
                };
                let r = simulate_pvp(vec![attacker, victim], 100.0, s);
                sum += r.order.iter().position(|&i| i == 1).unwrap() + 1;
            }
            sum as f32 / n as f32
        };
        let no_shield = avg_victim_place(false);
        let shielded = avg_victim_place(true);
        assert!(shielded < no_shield, "带护身符被定身应少吃亏、平均名次更好: {shielded} vs {no_shield}");
    }

    /// 动态难度:简单稳赢、大师豪赌、普通真较量,梯度跨绝对强度都成立。
    #[test]
    fn dynamic_difficulty_menu() {
        let wr = |stats: [i32; consts::STAT_COUNT], d: Difficulty| {
            (0..150u64).filter(|&s| simulate(info("甲"), stats, 0, hc(), d, &[], s).player_place == 1).count() as f32
                / 150.0
        };
        for p in [60, 180] {
            let s = [p, p, p, p, p];
            // 阈值 0.74:不截顶决胜去掉玩家=idx0 的并列偏袒后,弱档简单赛胜率略降,放宽后仍属「稳赢」。
            assert!(wr(s, Difficulty::Easy) > 0.74, "简单应稳赢(power {p})");
            assert!(wr(s, Difficulty::Master) < 0.30, "大师应是豪赌(power {p})");
            let nm = wr(s, Difficulty::Normal);
            assert!(nm > 0.30 && nm < 0.95, "普通应是真较量(power {p}): {nm}");
        }
    }

    /// 强马在「适合它的难度」(简单)稳赢;普通在镜像动态难度下对强马也是真较量(不断言碾压)。
    #[test]
    fn strong_horse_dominates_easy() {
        let stats = [180, 180, 150, 120, 120];
        let easy = (0..40u64)
            .filter(|&s| simulate(info("强"), stats, 0, hc(), Difficulty::Easy, &[], s).player_place == 1)
            .count();
        assert!(easy > 30, "强马简单赛应稳赢,实得 {easy}/40");
        // 普通(镜像难度)对强马仍是真较量:多数被青睐但远非碾压。
        let normal = (0..60u64)
            .filter(|&s| simulate(info("强"), stats, 0, hc(), Difficulty::Normal, &[], s).player_place == 1)
            .count();
        assert!(normal * 100 / 60 >= 45, "强马普通赛应被青睐(≥45%),实得 {normal}/60");
    }
}
