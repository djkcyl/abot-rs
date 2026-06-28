//! 赛马的枚举与数值常量。

/// 五维属性的维度数。
pub const STAT_COUNT: usize = 5;

/// 出图/公式基准值,非硬上限:训练可超过它。
pub const STAT_MAX: i32 = 200;

/// 当前值健壮性上界(点数口径,非玩法上限,仅防 i32 溢出;落库按 ×[`STAT_SCALE`] 钳)。
pub const STAT_SANITY_MAX: i32 = 2000;

/// 五维当前值存储精度:落库值 = 点数 × 此值(厘点),训练亚点增量靠它累积不被取整抹掉;
/// [`stats_of`](super::logic::stats_of) 读出折回点数,比赛/出图按点数。改它需迁移既有数据。
pub const STAT_SCALE: i32 = 100;

/// 出图归一参照:进度条/雷达按它满格、超过封顶;纯显示,不进玩法公式。
pub const DISPLAY_REF: i32 = 360;
/// 资质档位 C/B/A/S 的下界(D=不足 C),读 [`soft_ceiling`](super::logic::soft_ceiling)。
pub const APTITUDE_BANDS: [i32; 4] = [75, 120, 180, 260];

/// 一个属性维度。判别式即在 `horse` 行/数组里的下标。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stat {
    /// 速度:每回合基础位移(凹响应)。
    Spd = 0,
    /// 耐力:抗后程掉速,也降伤病。
    Sta = 1,
    /// 爆发:直接位移 + 暴击 + 落后翻盘。
    Brs = 2,
    /// 敏捷:闪避负面、缩短被干扰回合、起步抢位。
    Agi = 3,
    /// 幸运:赛后掉落 + 名次奖加成 + 训练好值(赛中不直接影响胜负)。
    Luk = 4,
}

impl Stat {
    /// 全部五维,顺序即下标。
    pub const ALL: [Stat; STAT_COUNT] = [Stat::Spd, Stat::Sta, Stat::Brs, Stat::Agi, Stat::Luk];

    pub fn idx(self) -> usize {
        self as usize
    }

    pub fn name(self) -> &'static str {
        match self {
            Stat::Spd => "速度",
            Stat::Sta => "耐力",
            Stat::Brs => "爆发",
            Stat::Agi => "敏捷",
            Stat::Luk => "幸运",
        }
    }

    /// 从用户输入解析属性名(全名或单字别名)。
    pub fn parse(s: &str) -> Option<Stat> {
        match s.trim() {
            "速度" | "速" => Some(Stat::Spd),
            "耐力" | "耐" => Some(Stat::Sta),
            "爆发" | "爆" => Some(Stat::Brs),
            "敏捷" | "敏" => Some(Stat::Agi),
            "幸运" | "运" => Some(Stat::Luk),
            _ => None,
        }
    }
}

/// 星级取值范围。
pub const RARITY_MIN: i16 = 1;
pub const RARITY_MAX: i16 = 4;

/// 领养(免费首马)固定星级。
pub const STARTER_RARITY: i16 = 2;

/// 领养随首马一次性发的启动金。
pub const STARTER_GRANT: i64 = 120;

// 出生:每维软上限 reach(复用 pot_* 列)+ 每马成长系数 growth(存 ×100)。
// 训练增量 ∝ m·growth·exp(-cur/reach)(见 logic::roll_gain):reach 越大能练越高,growth 是资质的快慢轴。
// 非硬墙——亚点增量靠厘点累积不丢(STAT_SCALE),海量训练可蹭过软平台,但全局调教衰减自限(train_total_decay)。

/// 领养首马的 reach 抽样标准差(比常规 ★2 窄,更标准化)。
pub const STARTER_REACH_SIGMA: f32 = 7.0;

/// 各维 reach 按星级的出生均值(下标 = rarity-1)。出生落在本星基线下方,留繁殖填充的空间。
pub const REACH_MEAN: [f32; 4] = [36.0, 54.0, 76.0, 100.0];
/// reach 按星级的抽样标准差(下标 = rarity-1)。
pub const REACH_SIGMA: [f32; 4] = [7.0, 10.0, 14.0, 18.0];
/// reach 钳制下界。
pub const REACH_MIN: i32 = 20;
/// reach 防溢出上界(纯健壮性,非玩法上限;玩法用软基线 [`REACH_BASELINE`])。
pub const REACH_HARD_MAX: i32 = 220;
/// 每星级的 reach 软基线(下标 = rarity-1):繁殖把子代 reach 向它回归(软,非死墙)。
pub const REACH_BASELINE: [i32; 4] = [60, 90, 120, 150];
/// 出生 reach 钳到「本星基线 + 此余量」内(出生不超过基线太多)。
pub const BIRTH_REACH_MARGIN: i32 = 8;
/// 出生「惊喜苗」概率:某维天生落在本星基线附近(罕见好苗,仍在本星尺度内)。
pub const REACH_JACKPOT_PROB: f64 = 0.015;
/// 惊喜苗 reach = 本星基线 + 此均匀区间(下界, 上界)。
pub const REACH_JACKPOT_BONUS: (i32, i32) = (0, 12);

/// 成长系数 growth 按星级的均值(存 ×100;下标 = rarity-1),也兼作繁殖子代 growth 的本星硬顶。
pub const GROWTH_MEAN: [f32; 4] = [95.0, 100.0, 105.0, 115.0];
/// growth 抽样标准差(×100,抽卡/洗髓口径)。
pub const GROWTH_SIGMA: f32 = 13.0;
/// growth 钳制下界(×100)。
pub const GROWTH_MIN: i32 = 65;
/// growth 钳制上界(×100)。
pub const GROWTH_MAX: i32 = 155;

/// 出生当前值 = reach × 此比例(出生即可下场,但远未练到平台)。
pub const BIRTH_REACH_RATIO: f32 = 0.40;

/// 毛色种类数(纯外观)。
pub const COLOR_COUNT: i16 = 6;

/// 毛色名(出图与呈现)。下标 = color 列值。
pub const COLOR_NAMES: [&str; COLOR_COUNT as usize] = ["枣红", "栗色", "乌骓", "白龙", "青骢", "金棕"];

/// 取毛色名(越界回第一个)。
pub fn color_name(c: i16) -> &'static str {
    COLOR_NAMES.get(c.clamp(0, COLOR_COUNT - 1) as usize).copied().unwrap_or("枣红")
}

/// 毛色名 → 列值(染色剂用;不识别返 `None`)。
pub fn color_index(s: &str) -> Option<i16> {
    COLOR_NAMES.iter().position(|&n| n == s.trim()).map(|i| i as i16)
}

// —— 体力 ——

/// 体力上限(饱食同样 0..=100)。
pub const VIT_MAX: i32 = 100;
/// 训练消耗体力。
pub const VIT_TRAIN: i32 = 12;
/// 比赛消耗体力。
pub const VIT_RACE: i32 = 15;

// —— 时间型资源统一结算(体力↑ / 饱食↓) ——
// 体力 +1/6min、饱食 -1/15min,两速率 lcm=30min,故按 30min 整块结算、余数留到下次,无漂移
// (见 logic::settle_state)。

/// 统一结算的时间块(分钟,= lcm(6,15))。
pub const STATE_BLOCK_MIN: i64 = 30;
/// 每块体力恢复(+5/30min = +1/6min)。
pub const VIT_PER_BLOCK: i32 = 5;
/// 每块饱食衰减(-2/30min = -1/15min)。
pub const SATIETY_PER_BLOCK: i32 = 2;

// —— 饱食 ——

/// 饱食低位阈值:低于则训练好值概率降、比赛轻微减速。
pub const SATIETY_LOW: i32 = 30;
/// 饱食高位阈值:高于则训练好值概率获正向加成。
pub const SATIETY_HIGH: i32 = 70;
/// 饿着(饱食 < 阈值)时比赛的速度系数。
pub const HUNGRY_SPEED_MULT: f32 = 0.95;

// —— 寿命(不可逆生涯耗材:训练/比赛只降,道具回一部分,可回复上限永久缩)——

/// 寿命上限基底:出生 lifespan_max = BASE + LUK_COEF×pot_luk,钳到 [MIN, CAP_MAX]。
pub const LIFESPAN_BASE: i32 = 400;
/// 寿命上限随幸运出生潜力(pot_luk)的系数(幸运=寿命维)。
pub const LIFESPAN_LUK_COEF: i32 = 4;
/// 寿命上限下界。
pub const LIFESPAN_MIN: i32 = 480;
/// 寿命上限封顶(防高 reach 幸运把寿命堆穿)。
pub const LIFESPAN_CAP_MAX: i32 = 1100;
/// 每场比赛耗的寿命。
pub const LIFESPAN_RACE_COST: i32 = 6;
/// 每次训练耗的寿命(集训券不豁免)。
pub const LIFESPAN_TRAIN_COST: i32 = 1;
/// 训练效率惩罚起点:life_ratio 低于此值起,训练增量按 [`train_eff`](super::logic::train_eff) 线性打折。
pub const LIFESPAN_PRIME_RATIO: f32 = 0.70;
/// 训练效率地板(life_ratio=0 时的最低系数)。
pub const LIFESPAN_TRAIN_EFF_FLOOR: f32 = 0.30;
/// 赛中削耐力起点:life_ratio 低于此值起,比赛 effective 耐力按 [`stamina_life_mult`](super::logic::stamina_life_mult) 线性削。
pub const LIFESPAN_LATE_RACE_RATIO: f32 = 0.40;
/// 赛中削耐力的最大折扣(life_ratio=0 时削掉这么多耐力)。
pub const LIFESPAN_STA_PENALTY_MAX: f32 = 0.20;

// —— 伤病(局内逐回合按距离触发 + 当场跛行 + 伤痕期 + 复发,见 [`race`](super::race))——

/// 命中受伤后,轻/中/重的基础权重(中/重再随寿命见底线性加,见 [`race`](super::race))。
pub const INJURY_SEVERITY_BASE: [u32; 3] = [60, 30, 10];
/// 单回合受伤的距离危险系数(× 本回合位移 × 后段/寿命/抗性/伤痕等乘子)。
pub const INJURY_DIST_HAZARD: f64 = 0.00028;
/// 后段受伤抬升斜率:progress 越过 [`INJURY_LATE_PHASE`] 后线性加危险。
pub const INJURY_LATE_RAMP: f64 = 1.5;
/// 后段受伤抬升起点(progress)。
pub const INJURY_LATE_PHASE: f64 = 0.5;
/// 寿命见底的受伤抬升:`1 + INJURY_LIFE_GAIN×(1−life_ratio)²`。
pub const INJURY_LIFE_GAIN: f64 = 4.5;
/// 耐力+幸运抗伤的分母(越大抗得越少)。
pub const INJURY_RESIST_DIV: f64 = 700.0;
/// 抗伤系数地板(高属性也压不穿这条受伤底)。
pub const INJURY_RESIST_FLOOR: f64 = 0.5;
/// 带伤当场跛行的速度乘子(下标 = 伤等-1):越重越慢。
pub const INJURY_LIMP_MULT: [f32; 3] = [0.90, 0.78, 0.62];
/// 中伤权重随寿命见底的加成(× (1−life_ratio))。
pub const INJURY_SEV_LIFE_MED: f64 = 40.0;
/// 重伤权重随寿命见底的加成(× (1−life_ratio))。
pub const INJURY_SEV_LIFE_HVY: f64 = 30.0;
/// 伤痕抬高再受伤危险:`1 + SCAR_HAZARD_GAIN×scar`。
pub const SCAR_HAZARD_GAIN: f64 = 0.8;
/// 复发取重伤的基础百分比(伤痕等级 1 时)。
pub const SCAR_RELAPSE_HEAVY_BASE: u32 = 25;
/// 复发取重伤百分比随伤痕等级每级的增量(25/45/65)。
pub const SCAR_RELAPSE_HEAVY_STEP: u32 = 20;
/// 伤痕期时长(小时,下标 = 伤痕等级-1):此期内带 stat 惩罚且易复发,到期自动消。
pub const SCAR_HOURS: [i64; 3] = [12, 24, 48];
/// 伤痕期内速/耐/爆/敏各打的折扣(下标 = 伤痕等级-1)。
pub const SCAR_STAT_PENALTY: [f32; 3] = [0.03, 0.06, 0.10];
/// 轻/中/重伤的恢复时长(小时)。下标 = 伤等-1。
pub const INJURY_HOURS: [i64; 3] = [3, 8, 16];
/// 治疗各伤等的费用(金币)。下标 = 伤等-1。
pub const HEAL_COST: [i64; 3] = [30, 40, 100];
/// 新手保护:马生涯前这么多场受伤只可能轻伤(且不复发)。
pub const NEWBIE_INJURY_GRACE: i32 = 10;

// —— 喂养 ——

/// 基础草料(金币购,纯回饱食):费用 + 回多少。
pub const FORAGE_COST: i64 = 6;
/// 基础草料回的饱食。
pub const FORAGE_SATIETY: i32 = 35;

// —— 训练 ——

/// 训练基础费用。
pub const TRAIN_BASE_COST: i64 = 26;
/// 训练费用日内每次递增。
pub const TRAIN_COST_STEP: i64 = 7;
/// 训练费用随被练维度的当前值上浮(每点附加),越到后期越贵。
pub const TRAIN_COST_PER_POINT: f32 = 0.28;

/// 训练单次「好值档」的基础幅度 `m` 区间(再乘 growth × `exp(-cur/reach)`)。每档 `(权重, 下界, 上界)`,
/// 权重受幸运/饲料/吃饱抬高优档(见 [`logic`](super::logic))。
pub const TRAIN_TIERS: [(u32, f32, f32); 4] = [
    (40, 2.0, 5.0),  // 暗淡
    (38, 4.0, 9.0),  // 普通
    (18, 8.0, 15.0), // 优良
    (4, 14.0, 24.0), // 暴击
];

/// [`TRAIN_TIERS`] 的加权期望幅度(= Σ权重·档中点 / 100),出图估软平台用(见
/// [`logic::soft_ceiling`](super::logic::soft_ceiling))。改 TRAIN_TIERS 时同步改这里。
pub const TRAIN_MAG_MEAN: f32 = 6.7;

/// 训练 spillover(溢出到第二维)的基础概率(幸运再抬)。
pub const TRAIN_SPILL_PROB: f64 = 0.18;

// —— 全局调教衰减(总调教次数越多每次涨越少,与单维天赋线衰减叠乘,见
//    [`train_total_decay`](super::logic::train_total_decay))——

/// 头这么多次训练满额、不受全局衰减(给新马一个高效成长窗)。
pub const TRAIN_GLOBAL_FREE: i32 = 40;
/// 全局衰减尺度:超出 [`TRAIN_GLOBAL_FREE`] 后按 `1/(1+(n-FREE)/K)` 平滑衰减(渐近 0、不致死)。
/// K 越大衰得越慢:n=FREE+K 时减半、+2K 时约三分之一。
pub const TRAIN_GLOBAL_K: f32 = 120.0;

// —— 比赛难度 ——

/// 一档比赛难度。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Difficulty {
    Easy,
    Normal,
    Hard,
    /// 大师:Hard 之上的 endgame 难度档。
    Master,
}

impl Difficulty {
    /// 解析难度关键词(仅当是难度词时返回 `Some`,供与道具名区分;无难度词时调用方缺省普通)。
    pub fn try_parse(s: &str) -> Option<Difficulty> {
        match s.trim() {
            "简单" | "易" | "easy" => Some(Difficulty::Easy),
            "普通" | "normal" => Some(Difficulty::Normal),
            "困难" | "难" | "hard" => Some(Difficulty::Hard),
            "大师" | "锦标" | "master" => Some(Difficulty::Master),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Difficulty::Easy => "简单",
            Difficulty::Normal => "普通",
            Difficulty::Hard => "困难",
            Difficulty::Master => "大师",
        }
    }

    /// 难度下标(Easy 0 / Normal 1 / Hard 2 / Master 3),查 [`NPC_NEG_ITEM_PROB`] 等按难度的表。
    pub fn idx(self) -> usize {
        match self {
            Difficulty::Easy => 0,
            Difficulty::Normal => 1,
            Difficulty::Hard => 2,
            Difficulty::Master => 3,
        }
    }

    /// 赛道长度(回合数尺度)。
    pub fn track_len(self) -> f32 {
        match self {
            Difficulty::Easy => 80.0,
            Difficulty::Normal => 95.0,
            Difficulty::Hard => 118.0,
            Difficulty::Master => 138.0,
        }
    }

    /// NPC 马匹数(连玩家自己即总参赛数 = 此值 + 1)。
    pub fn npc_count(self) -> usize {
        match self {
            Difficulty::Easy => 3,
            Difficulty::Normal => 4,
            Difficulty::Hard => 5,
            Difficulty::Master => 6,
        }
    }

    /// 动态难度系数:每匹 NPC = 玩家自己五维的镜像 × 此值(见 [`gen_npc`](super::race))。难度是
    /// 「对手相对你多强」而非绝对内容档;镜像使堆一维 min-max 钻不了空子。简单 = 对手弱于你、大师 = 强于你。
    pub fn npc_ratio(self) -> f32 {
        match self {
            // 速度凹响应压缩了低位差距,简单档压到 0.70 保新手「简单稳赢」的正反馈。
            Difficulty::Easy => 0.70,
            Difficulty::Normal => 0.88,
            Difficulty::Hard => 0.98,
            Difficulty::Master => 1.06,
        }
    }

    /// 报名费基础部分(实际报名费 = 此值 + [`entry_step`](Difficulty::entry_step) × 今日该马已比赛次数,见
    /// [`race_cmd`](super::race) 调用处)。
    pub fn entry_fee(self) -> i64 {
        match self {
            Difficulty::Easy => 5,
            Difficulty::Normal => 25,
            Difficulty::Hard => 45,
            Difficulty::Master => 70,
        }
    }

    /// 冠军基础奖励(名次递减、再 × 实力系数,见 [`reward_for`](super::race::reward_for))。配合
    /// [`entry_step`](Difficulty::entry_step) 让日内边际收益快速趋零、自然封顶。当日首胜另发一次性
    /// [`DAILY_FIRST_WIN_BONUS`](每玩家每天一次,跨难度与 PvP 不叠)。
    pub fn reward_base(self) -> i64 {
        match self {
            Difficulty::Easy => 30,
            Difficulty::Normal => 80,
            Difficulty::Hard => 135,
            Difficulty::Master => 240,
        }
    }

    /// 报名费的「日内递增」步长:实际报名费 = [`entry_fee`](Difficulty::entry_fee) + 此值 × 今日该马已比赛次数。
    /// 高难度步长大,日内边际收益更快趋零、封顶。业务日切换归零。
    pub fn entry_step(self) -> i64 {
        match self {
            Difficulty::Easy => 5,
            Difficulty::Normal => 10,
            Difficulty::Hard => 18,
            Difficulty::Master => 25,
        }
    }
}

/// 名次奖励系数(下标 = 名次-1;超出长度的名次无奖)。
pub const PLACE_REWARD_FACTOR: [f32; 3] = [1.0, 0.6, 0.4];

/// 名次奖励随玩家实力(前四维均值,见 [`player_power`](super::race::player_power))缩放的参照:实力 = 此值时
/// 系数恰 1.0。取 150 使奖励沿星梯线性给、不早早顶死。全维相等的马前四维均值 = 五维均值,故不受口径调整影响。
pub const REWARD_POWER_REF: f32 = 150.0;
/// 奖励实力系数的钳制区间(下界, 上界)。
pub const REWARD_POWER_CLAMP: (f32, f32) = (0.6, 2.0);

/// 敏捷满值时闪避负面道具的最大概率(线性按 `agi/STAT_EFFECT_REF` 缩放)。
pub const AGI_DODGE_MAX: f32 = 0.85;
/// 反射神经特性下的闪避上限(再 ×1.15 后封顶)。
pub const AGI_DODGE_CAP_REFLEX: f32 = 0.90;
/// 敏捷「减时」:多回合负面(盯防)有效回合 = `max(1, n - 敏捷/此值)`。反射神经特性改用更小的 70。
pub const AGI_REDUCE_DIV: i32 = 120;
/// 反射神经特性的减时除数(更易缩短负面)。
pub const AGI_REDUCE_DIV_REFLEX: i32 = 70;

// —— 比赛模拟系数(见 [`race::step`](super::race);集中放这里便于一处校准平衡)——

/// 比赛效果(暴击/闪避)的属性缩放参照:属性达此值时该效果项满格。取 285(≈满级 ★4 平台)让 200→300 段
/// 仍有边际收益,不在 200 处就饱和。
pub const STAT_EFFECT_REF: i32 = 285;
/// 耐力后程项的缩放参照:配合后程系数区间让长赛道吃重但不「独大」。
pub const STAMINA_STAT_REF: i32 = 255;
/// 速度位移凹响应:`base = 速度^EXP × COEF`。镜像下均匀标量改不动边际胜率,唯一合法杠杆是给位移加凹曲线
/// (边际递减);线性会让高速独大。标定 速度100→4.55,不破坏全局时长,偏离 100 越远越凹。
pub const SPEED_BASE_EXP: f32 = 0.55;
/// 凹响应系数(见 [`SPEED_BASE_EXP`],标定 速度100→4.55)。
pub const SPEED_BASE_COEF: f32 = 0.3611;
/// 爆发直接位移项:`base ×= 1 + 爆发/STAT_EFFECT_REF × 此值`。让爆发成为可练的「暴击型二速」而非纯暴击概率
/// (否则近废维)。
pub const BURST_BASE_SCALE: f32 = 0.45;
/// 后程系数斜率:进度越大、耐力越低掉速越狠。
pub const STAMINA_COEFF: f32 = 1.0;
/// 后程系数区间上界:使超高耐力不能「又稳又快」反客为主。
pub const STAMINA_FACTOR_MAX: f32 = 1.18;
/// 后程系数区间下界:免低耐马长赛道彻底崩速。
pub const STAMINA_FACTOR_MIN: f32 = 0.40;
/// 爆发对暴击概率的缩放(`爆发/STAT_EFFECT_REF × 此值`)。
pub const BURST_CRIT_SCALE: f64 = 0.30;
/// 暴击命中时的位移倍率。
pub const BURST_CRIT_MULT: f32 = 1.6;
/// 落后者「追赶暴击」基础项:与领先者的距离差(归一)× (此基数 + 爆发缩放),挂到自身爆发。
pub const COMEBACK_BASE: f64 = 0.15;
/// 追赶暴击的爆发缩放项(`爆发/STAT_EFFECT_REF × 此值`,叠进上面的基数)。
pub const COMEBACK_BRS_SCALE: f64 = 0.25;
/// 暴击概率封顶(纯属性 + 四叶草 + 追赶叠加后的硬上界)。设 0.7 让高 brs/luk 在 200→300 段不被截死。
pub const CRIT_PROB_CAP: f64 = 0.7;
/// 抖动幅度系数(`基础位移 × 此值 × 韧者/稳行乘子`):单回合随机性,逐回合独立、被中心极限平均掉。
/// 幸运不收敛抖动(实测为负收益,别回退);减抖只来自韧者特性与稳行道具。
pub const JITTER_COEFF: f32 = 0.85;
/// 敏捷「抢内道/起跑快」:开局阶段(进度 < [`AGI_START_PHASE`])给基础位移乘 `1 + 敏捷/STAT_MAX × 此值`。
pub const AGI_START_BOOST: f32 = 0.3;
/// 敏捷起跑加成的生效赛道前段比例。
pub const AGI_START_PHASE: f32 = 0.2;
/// 绊马索(定身)持续回合。
pub const FREEZE_ROUNDS: i32 = 1;
/// 冲刺:起跑数回合的速度乘子。
pub const BOOST_MULT: f32 = 1.5;
/// 终盘冲刺生效的赛道后段起点(progress 超过即提速)。终盘冲刺整场挂载、progress 门控:仅赛道后 28%
/// (progress > 0.72)生效、该段速度 ×[`LATE_BOOST_MULT`]。0.72 经标定使其总位移收益在典型赛长
/// (速度100≈28回合)下≈起跑冲刺(×1.5、前 3 回合);非恒等——赛越短/马越快则终盘冲刺相对偏弱,
/// 反之偏强(后段回合数随 track_len 增多)。实测对夺冠率提升约为起跑冲刺的 2~3 倍,故 base_value 定 90 > 冲刺 60。
pub const LATE_BOOST_PHASE: f32 = 0.72;
/// 终盘冲刺:后段的速度乘子。
pub const LATE_BOOST_MULT: f32 = 1.3;
/// 定心丸:按 progress 线性加到后程系数上的耐力补偿(只补后程,非全程平加)。
pub const STAMINA_TONIC_BONUS: f32 = 0.10;
/// 定心丸高耐减半阈值:耐力 ≥ 此值时补偿减半(防与耐力流叠成超模)。
pub const STAMINA_TONIC_STA_GATE: i32 = 150;
/// 稳行:抖动幅度乘数(更稳)。配合 [`STEADY_SPEED_MULT`] 给小幅速度补偿,避免「减自己方差」对劣势方净负。
pub const STEADY_JITTER_MULT: f32 = 0.70;
/// 稳行:小幅速度补偿(免「减自己方差」对劣势方净负)。
pub const STEADY_SPEED_MULT: f32 = 1.05;
/// 四叶草:整场暴击加成。
pub const CLOVER_CRIT: f64 = 0.20;
/// 四叶草:领先时(自己即领头)暴击加成减半,从「无脑翻盘」变「拉锯/守成」。
pub const CLOVER_CRIT_LEADING: f64 = 0.10;
/// 鸣枪惊群:对手 > 4 时的单回合速度乘子(大乱斗更强)。
pub const SCARE_SLOW_BIG: f32 = 0.60;
/// 鸣枪惊群:对手 ≤ 4 时的单回合速度乘子(人少时弱)。
pub const SCARE_SLOW_SMALL: f32 = 0.72;
/// 鸣枪惊群「大乱斗」判定:对手数(不含自己)超过此值用 [`SCARE_SLOW_BIG`]。
pub const SCARE_BIG_FIELD: usize = 4;
/// 盯防:目标对手每回合速度乘数(单体持续软控)。
pub const MARK_SLOW_MULT: f32 = 0.55;
/// 盯防基础持续回合(被目标敏捷「减时」削减,见 [`AGI_REDUCE_DIV`])。
pub const MARK_ROUNDS: i32 = 3;

/// PvE 中 NPC 按难度概率向玩家投一个负面道具(绊马索/惊马铃)。
/// 下标同 [`Difficulty::idx`](Difficulty::idx);难度越高越爱使坏。
pub const NPC_NEG_ITEM_PROB: [f64; 4] = [0.10, 0.22, 0.35, 0.40];

/// 每人马厩基础容量上限(只数在厩的马,退役不占格;仓库设施可扩到 [`STABLE_CAP_HARD_MAX`],
/// 见 [`effective_stable_cap`](super::logic::effective_stable_cap))。收紧到 16 是为了逼换代、别囤马。
pub const STABLE_CAP: usize = 16;

// —— 物品:比赛道具 + 训练增益饲料 ——

/// 物品大类(决定用在哪、抽卡归哪类、背包配色)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemKind {
    /// 比赛道具(赛中生效,`赛马比赛`/`赛马开房` 带上)。
    Race,
    /// 训练道具(`赛马训练` 时吃)。
    Train,
    /// 平时对一匹马使用的道具(`赛马用`/繁殖时带):恢复 / 养成 / 繁殖 / 趣味,效果按具体道具分发。
    Use,
}

/// 一种物品(同存共享背包 `game_item`)。判别式即背包号段内的序号,落库值勿改既有项的数字,新增往后排。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Item {
    // —— 比赛道具 · 自身增益 ——
    /// 冲刺:起跑后几回合提速。
    Boost = 0,
    /// 四叶草:整场提高自身暴击。
    Clover = 4,
    /// 终盘冲刺:最后一程提速。
    LateBoost = 10,
    /// 定心丸:整场抗后程掉速(等于多一截耐力)。
    StaminaTonic = 11,
    /// 稳行:整场发挥更稳(抖动减半)。
    Steady = 12,
    // —— 比赛道具 · 干扰对手 ——
    /// 绊马索:一名对手定身 1 回合(硬控)。
    Banana = 1,
    /// 鸣枪惊群:全体对手短暂减速 1 回合(AoE 软控)。
    Scare = 3,
    /// 盯防:死缠一名对手,持续减速最多 3 回合(对手敏捷越高越短)。
    Mark = 13,
    // —— 比赛道具 · 防御 ——
    /// 护身符:挡下一个负面。
    Shield = 2,
    /// 回马枪:挡下一个负面并反弹给一名对手。
    Reflect = 14,
    // —— 训练道具 ——
    /// 精饲料:训练增益(中)。
    Feed1 = 5,
    /// 滋补膏:训练增益(高)。
    Feed2 = 6,
    /// 专注饲料:本次不溢出、全压主练维。
    FocusFeed = 15,
    /// 集训券:本次训练不耗体力。
    DrillPass = 16,
    /// 破限丹:本次训练无视天赋线衰减。
    BreakPill = 17,
    // —— 恢复道具 ——
    /// 刷洗:回寿命(轻,降可回复上限少)。
    Care1 = 7,
    /// 推拿:回寿命(中)。
    Care2 = 8,
    /// 温泉疗养:回寿命(高,降可回复上限多)。
    Care3 = 9,
    /// 能量饮:回体力。
    EnergyDrink = 18,
    /// 金疮药:不花币立即治伤。
    Medicine = 19,
    /// 精草料:大幅回饱食。
    FineForage = 20,
    // —— 养成珍材 ——
    /// 育骨精料:永久 +一维资质(reach)。
    ReachTonic = 21,
    /// 洗髓草:重摇成长(growth)。
    GrowthHerb = 22,
    /// 特性秘传:随机加 1 条特性。
    TraitBook = 23,
    /// 静心符:重摇全部特性。
    TraitReroll = 24,
    // —— 繁殖珍材 ——
    /// 星辉石:下次繁殖必 +1 星(繁殖时带)。
    StarStone = 25,
    /// 红绳:立即清母马繁殖冷却。
    RedString = 26,
    /// 续种符:一匹马作种次数 −1(多配一次)。
    BreedCharm = 27,
    // —— 趣味 ——
    /// 染色剂:改毛色。
    Dye = 28,
    /// 改名牌:免费改名一次。
    NameTag = 29,
}

/// 赛马物品在共享背包 `game_item` 里的 item_id 号段基址(其它游戏用别的号段,跨插件唯一)。
pub const HORSE_ITEM_BASE: i32 = 1000;
/// 赛马号段宽度(预留,远大于现有物品数)。
pub const HORSE_ITEM_SPAN: i32 = 100;

/// 育骨精料一次 +的 reach 点数。
pub const REACH_TONIC_ADD: i32 = 12;
/// 能量饮回的体力。
pub const ENERGY_RESTORE: i32 = 50;
/// 精草料回的饱食。
pub const FINE_FORAGE_SATIETY: i32 = 70;

impl Item {
    pub const ALL: [Item; 30] = [
        Item::Boost,
        Item::Clover,
        Item::LateBoost,
        Item::StaminaTonic,
        Item::Steady,
        Item::Banana,
        Item::Scare,
        Item::Mark,
        Item::Shield,
        Item::Reflect,
        Item::Feed1,
        Item::Feed2,
        Item::FocusFeed,
        Item::DrillPass,
        Item::BreakPill,
        Item::Care1,
        Item::Care2,
        Item::Care3,
        Item::EnergyDrink,
        Item::Medicine,
        Item::FineForage,
        Item::ReachTonic,
        Item::GrowthHerb,
        Item::TraitBook,
        Item::TraitReroll,
        Item::StarStone,
        Item::RedString,
        Item::BreedCharm,
        Item::Dye,
        Item::NameTag,
    ];
    /// 比赛道具类(抽卡「道具」类/赛中带;与 [`GACHA_RACE_WEIGHTS`] 同序)。
    pub const RACE: [Item; 10] = [
        Item::Boost,
        Item::Clover,
        Item::LateBoost,
        Item::StaminaTonic,
        Item::Steady,
        Item::Banana,
        Item::Scare,
        Item::Mark,
        Item::Shield,
        Item::Reflect,
    ];
    /// 训练道具类(抽卡「饲料」类/训练吃;与 [`GACHA_TRAIN_WEIGHTS`] 同序)。
    pub const TRAIN: [Item; 5] = [Item::Feed1, Item::Feed2, Item::FocusFeed, Item::DrillPass, Item::BreakPill];
    /// 恢复道具类(抽卡「护理」类;与 [`GACHA_RECOVERY_WEIGHTS`] 同序)。
    pub const RECOVERY: [Item; 6] =
        [Item::Care1, Item::Care2, Item::Care3, Item::EnergyDrink, Item::Medicine, Item::FineForage];
    /// 珍材类(养成 + 繁殖 + 趣味,抽卡「珍材」类、稀有;与 [`GACHA_TREASURE_WEIGHTS`] 同序)。
    pub const TREASURE: [Item; 9] = [
        Item::ReachTonic,
        Item::GrowthHerb,
        Item::TraitBook,
        Item::TraitReroll,
        Item::StarStone,
        Item::RedString,
        Item::BreedCharm,
        Item::Dye,
        Item::NameTag,
    ];

    /// 共享背包里的全局 item_id(号段基址 + 序号)。
    pub fn global_id(self) -> i32 {
        HORSE_ITEM_BASE + self as i32
    }

    /// 从全局 item_id 还原(仅赛马号段)。
    pub fn from_global(id: i32) -> Option<Item> {
        Item::ALL.into_iter().find(|i| i.global_id() == id)
    }

    pub fn kind(self) -> ItemKind {
        use Item::*;
        match self {
            Boost | Clover | LateBoost | StaminaTonic | Steady | Banana | Scare | Mark | Shield | Reflect => {
                ItemKind::Race
            }
            Feed1 | Feed2 | FocusFeed | DrillPass | BreakPill => ItemKind::Train,
            _ => ItemKind::Use,
        }
    }

    /// 抽卡结果卡上的大类标签(道具/训练/恢复/珍材)。
    pub fn gacha_class_name(self) -> &'static str {
        use Item::*;
        match self {
            Boost | Clover | LateBoost | StaminaTonic | Steady | Banana | Scare | Mark | Shield | Reflect => "道具",
            Feed1 | Feed2 | FocusFeed | DrillPass | BreakPill => "训练",
            Care1 | Care2 | Care3 | EnergyDrink | Medicine | FineForage => "恢复",
            _ => "珍材",
        }
    }

    /// 护理道具回的寿命(非护理道具为 0)。
    pub fn life_restore(self) -> i32 {
        match self {
            Item::Care1 => 90,
            Item::Care2 => 200,
            Item::Care3 => 360,
            _ => 0,
        }
    }

    /// 护理道具每次永久扣的「可回复上限」(非护理道具为 0)。
    pub fn life_cap_cost(self) -> i32 {
        match self {
            Item::Care1 => 48,
            Item::Care2 => 90,
            Item::Care3 => 150,
            _ => 0,
        }
    }

    /// 饲料的训练增益等级(向好值档倾斜的权重加成);非饲料为 0。
    pub fn feed_bump(self) -> u32 {
        match self {
            Item::Feed1 => 4,
            Item::Feed2 => 9,
            _ => 0,
        }
    }

    /// 「赛马商店」直购价(金币);非商店道具返 `None`。养成珍材以商店直购为主路线。
    pub fn shop_price(self) -> Option<i64> {
        match self {
            Item::ReachTonic => Some(3500),
            Item::GrowthHerb => Some(2800),
            Item::TraitBook => Some(1800),
            Item::TraitReroll => Some(2200),
            Item::StarStone => Some(4000),
            Item::RedString => Some(1500),
            Item::BreedCharm => Some(3000),
            Item::Dye => Some(300),
            // 改名牌不上架商店:改名命令(RENAME_COST=50)直接可改,商店项是伪商品。
            _ => None,
        }
    }

    /// 道具「参考价值」(回收价 / 溢出折币的基准)。珍材复用 [`shop_price`](Item::shop_price)即直购价;
    /// 消耗品按强度定基准(大致 比赛道具 ≳ 训练 > 恢复,稀有/功效越高越值钱)。
    pub fn base_value(self) -> i64 {
        use Item::*;
        if let Some(p) = self.shop_price() {
            return p; // 珍材:基准即直购价
        }
        match self {
            // 比赛道具
            Boost => 60,
            Clover => 120,
            LateBoost => 90,
            StaminaTonic => 90,
            Steady => 70,
            Banana => 80,
            Scare => 90,
            Mark => 100,
            Shield => 70,
            Reflect => 130,
            // 训练道具
            Feed1 => 50,
            Feed2 => 110,
            FocusFeed => 80,
            DrillPass => 60,
            BreakPill => 150,
            // 恢复道具
            Care1 => 60,
            Care2 => 130,
            Care3 => 220,
            EnergyDrink => 40,
            Medicine => 80,
            FineForage => 24,
            // 改名牌已下架商店(shop_price=None),给个合理的回收/掉落基准(对齐 RENAME_COST)。
            NameTag => 50,
            // 珍材已在上面经 shop_price 返回;此处兜底新增项
            _ => 0,
        }
    }

    /// 回收(出售)价 = `base_value × SELL_RATE`(向下取整,至少 1)。
    pub fn sell_price(self) -> i64 {
        ((self.base_value() as f64 * SELL_RATE) as i64).max(1)
    }

    pub fn name(self) -> &'static str {
        use Item::*;
        match self {
            Boost => "冲刺",
            Clover => "四叶草",
            LateBoost => "终盘冲刺",
            StaminaTonic => "定心丸",
            Steady => "稳行",
            Banana => "绊马索",
            Scare => "鸣枪惊群",
            Mark => "盯防",
            Shield => "护身符",
            Reflect => "回马枪",
            Feed1 => "精饲料",
            Feed2 => "滋补膏",
            FocusFeed => "专注饲料",
            DrillPass => "集训券",
            BreakPill => "破限丹",
            Care1 => "刷洗",
            Care2 => "推拿",
            Care3 => "温泉疗养",
            EnergyDrink => "能量饮",
            Medicine => "金疮药",
            FineForage => "精草料",
            ReachTonic => "育骨精料",
            GrowthHerb => "洗髓草",
            TraitBook => "特性秘传",
            TraitReroll => "静心符",
            StarStone => "星辉石",
            RedString => "红绳",
            BreedCharm => "续种符",
            Dye => "染色剂",
            NameTag => "改名牌",
        }
    }

    /// 从输入解析(全名或别名)。
    pub fn parse(s: &str) -> Option<Item> {
        use Item::*;
        match s.trim() {
            "冲刺" | "加速" => Some(Boost),
            "四叶草" | "幸运草" | "幸运四叶草" => Some(Clover),
            "终盘冲刺" | "终盘" | "后程冲刺" => Some(LateBoost),
            "定心丸" | "定心" => Some(StaminaTonic),
            "稳行" => Some(Steady),
            "绊马索" | "绊马" => Some(Banana),
            "鸣枪惊群" | "鸣枪" | "惊群" => Some(Scare),
            "盯防" => Some(Mark),
            "护身符" | "护身" | "护盾" => Some(Shield),
            "回马枪" | "回马" => Some(Reflect),
            "精饲料" | "精料" => Some(Feed1),
            "滋补膏" | "补膏" | "滋补" => Some(Feed2),
            "专注饲料" | "专注" => Some(FocusFeed),
            "集训券" | "集训" => Some(DrillPass),
            "破限丹" | "破限" => Some(BreakPill),
            "刷洗" | "刷毛" => Some(Care1),
            "推拿" | "按摩" => Some(Care2),
            "温泉疗养" | "温泉" | "疗养" => Some(Care3),
            "能量饮" | "能量" => Some(EnergyDrink),
            "金疮药" | "金疮" => Some(Medicine),
            "精草料" | "精草" => Some(FineForage),
            "育骨精料" | "育骨" => Some(ReachTonic),
            "洗髓草" | "洗髓" => Some(GrowthHerb),
            "特性秘传" | "秘传" => Some(TraitBook),
            "静心符" | "静心" => Some(TraitReroll),
            "星辉石" | "星辉" => Some(StarStone),
            "红绳" => Some(RedString),
            "续种符" | "续种" => Some(BreedCharm),
            "染色剂" | "染色" => Some(Dye),
            "改名牌" | "名牌" => Some(NameTag),
            _ => None,
        }
    }

    pub fn effect_desc(self) -> &'static str {
        use Item::*;
        match self {
            Boost => "起跑后短暂提速",
            Clover => "整场更容易打出暴击;自己领先时弱些",
            LateBoost => "最后一程发力提速",
            StaminaTonic => "补后程掉速,越到后段越顶用;耐力本来高的话用处不大",
            Steady => "发挥更稳少出冷门——领先/PvP 才划算,落后慎用",
            Banana => "绊一名对手,原地踉跄一下",
            Scare => "一声炸响,全体对手短暂减速",
            Mark => "死缠一名对手持续减速,对手敏捷越高挣脱越快",
            Shield => "挡掉对手的下一次干扰",
            Reflect => "挡掉一次干扰,还反弹给一名对手",
            Feed1 => "训练时喂,更容易练出好值",
            Feed2 => "训练时喂,出好值的概率明显更高",
            FocusFeed => "训练时喂,这次不溢出、主练维涨更多",
            DrillPass => "这次训练不耗体力",
            BreakPill => "这次训练无视天赋线,练满也大涨",
            Care1 => "简单打理,回一点寿命(用多了上限会永久降一些)",
            Care2 => "舒筋活血,回一截寿命(用多了上限会永久降)",
            Care3 => "好生将养,回一大截寿命(用多了上限会永久降不少)",
            EnergyDrink => "灌一瓶,回点体力",
            Medicine => "敷上药,立刻治好伤(不花币)",
            FineForage => "上等草料,大幅回饱食",
            ReachTonic => "长期调养,永久提升一维资质上限",
            GrowthHerb => "脱胎换骨,重摇成长快慢",
            TraitBook => "得一手秘传,随机学一条特性",
            TraitReroll => "静下心来,重摇全部特性",
            StarStone => "繁殖时带上,下一胎必定升一星",
            RedString => "牵一根红绳,母马立刻可再繁殖",
            BreedCharm => "续上香火,这匹马多配一次",
            Dye => "给马换个毛色",
            NameTag => "免费改一次名",
        }
    }
}

/// 背包单种道具堆叠上限(溢出按回收价折币返还)。
pub const ITEM_STACK_CAP: i32 = 99;
/// 回收(出售)率:回收价 = `base_value × 此值`(向下取整,至少 1),溢出折币也走它。
/// 设 0.25 使「商店买→回收」必亏、不构成洗币/印钞,只给抽卡一个保底地板。
pub const SELL_RATE: f64 = 0.25;
/// 一场比赛最多带的道具数。
pub const MAX_RACE_ITEMS: usize = 2;

// —— 抽卡 ——

/// 单抽费用。
pub const GACHA_SINGLE_COST: i64 = 170;
/// 十连费用(含小折扣)。
pub const GACHA_TEN_COST: i64 = 1580;
/// 标准池软保底:累计多少抽必出高星马(★3+)。只有 ★3+ 清空标准池计数(std_pity),
/// 自然出的 ★1/★2 马不清(否则保底永不触发)。两池保底独立计数(见 [`GACHA_HORSE_POOL_PITY`])。
pub const GACHA_PITY: i32 = 30;
/// 马池软保底:马池自然出马率高(★3+≈15.6%/抽),独立计数、抽数定小些即可。
pub const GACHA_HORSE_POOL_PITY: i32 = 25;
/// 软保底渐升区间:末多少抽内出马权重明显抬升。
pub const GACHA_SOFT_PITY: i32 = 15;

/// 标准池大类权重:`(比赛道具, 训练道具, 恢复, 养成珍材, 马)`。定位是道具贩卖机,马稀、要马走马池。
pub const GACHA_CLASS_WEIGHTS: [u32; 5] = [72, 10, 8, 6, 4];

/// 马池单抽费用。马是稀缺产出,定贵些。
pub const GACHA_HORSE_POOL_SINGLE_COST: i64 = 350;
/// 马池十连费用(含小折扣)。
pub const GACHA_HORSE_POOL_TEN_COST: i64 = 3200;
/// 马池大类权重:`(比赛道具, 训练道具, 恢复, 养成珍材, 马)`。出马 ≥ 半数。
pub const GACHA_HORSE_POOL_CLASS_WEIGHTS: [u32; 5] = [19, 7, 5, 4, 65];

/// 比赛道具类内部权重(下标同 [`Item::RACE`]):冲刺/绊马索常见,稳行/盯防/护身符/回马枪稀。
pub const GACHA_RACE_WEIGHTS: [u32; 10] = [22, 8, 14, 12, 8, 16, 7, 6, 4, 3];

/// 训练道具类内部权重(下标同 [`Item::TRAIN`]):精饲料/专注常见,破限丹稀。
pub const GACHA_TRAIN_WEIGHTS: [u32; 5] = [40, 18, 22, 15, 5];

/// 恢复道具类内部权重(下标同 [`Item::RECOVERY`]):刷洗/能量饮常见,温泉疗养稀。
pub const GACHA_RECOVERY_WEIGHTS: [u32; 6] = [34, 16, 6, 20, 12, 12];

/// 珍材类内部权重(下标同 [`Item::TREASURE`]):常用养成材(育骨精料/洗髓草/特性秘传)给多些,星辉石最稀。
pub const GACHA_TREASURE_WEIGHTS: [u32; 9] = [16, 14, 12, 10, 8, 12, 8, 12, 8];

/// 标准池出马的星级权重(下标 = rarity-1)。高星收紧,稀有度更值钱。
pub const GACHA_HORSE_RARITY_WEIGHTS: [u32; 4] = [48, 37, 12, 3];

/// 马池出马的星级权重(下标 = rarity-1):比标准池更偏高星,但高星整体收紧,顶级马更稀有。
pub const GACHA_HORSE_POOL_RARITY_WEIGHTS: [u32; 4] = [38, 38, 19, 5];

/// 保底强制出马时的星级权重(下标 = rarity-1):纯 ★3/★4。
pub const GACHA_PITY_RARITY_WEIGHTS: [u32; 4] = [0, 0, 82, 18];

/// 抽到马但马厩已满时,按星级折算返还的金币(下标 = rarity-1,高星折返更多)。
/// (满厩抽卡已在抽前拦截/提示,见 do_gacha;此数组兜底标准池仍允许抽时的折返。)
pub const GACHA_HORSE_FULL_REFUND_BY_RARITY: [i64; 4] = [100, 220, 480, 900];

// —— 繁殖 ——

/// 繁殖费用按较高亲本星级计(下标 = rarity-1);不随代数无限涨,免得费用墙惩罚选育纵深本身。
pub const BREED_COST_BY_RARITY: [i64; 4] = [300, 500, 800, 1200];
/// 母马繁殖冷却小时数。
pub const BREED_COOLDOWN_HOURS: i64 = 48;
/// 普通马一生可作种次数上限(到顶不能再繁殖,可退役换币):防一匹神马无限刷、逼持续产新种。
pub const BREED_COUNT_MAX: i32 = 5;
/// 血统库种马的作种上限:比普通马高,但仍有顶。续种符可在此基础上 +1(每次一份珍材)。
pub const STUD_BREED_COUNT_MAX: i32 = 12;
/// 近亲检测上溯代数(N 代内有共同祖先即近亲)。
pub const BREED_INCEST_DEPTH: u32 = 3;

// 子代 reach 遗传(软基线回归模型,见 [`breed_child`](super::logic::breed_child)):
//   每维先取「偏向较优亲本」的中值,再向子代星级基线回归 + 噪声。
/// 子代每维 reach 向本星基线回归的强度。
pub const BREED_REACH_REVERT: f32 = 0.27;
/// 回归后叠加的每代噪声标准差(可负 → 有概率繁殖出更烂的)。
pub const BREED_REACH_NOISE: f32 = 5.5;
/// 每维取中值时偏向「较优亲本」的均值权重(0.5=纯均值,>0.5 偏优,可定向选育)。
pub const BREED_REACH_LEAN: f32 = 0.6;
/// 上述偏向权重的抽样标准差(给特化留方差与下行)。
pub const BREED_REACH_LEAN_SD: f32 = 0.26;

/// 繁殖子代 growth 向本星均值([`GROWTH_MEAN`])回归的强度(回归后再 .min(本星均值) 硬顶,繁殖不超均值、不传递)。
pub const GROWTH_BREED_REVERT: f32 = 0.50;
/// 繁殖子代 growth 的噪声标准差。
pub const GROWTH_BREED_NOISE: f32 = 4.0;

/// 繁殖产子星级 +1 的基础概率。
pub const BREED_RARITY_UP_PROB: f64 = 0.10;
/// 双亲均为 ★3+ 时星级 +1 的概率(更高)。
pub const BREED_RARITY_UP_PROB_HIGH: f64 = 0.25;
/// 繁殖产子星级 −1(回退,可繁殖出更烂的)的概率;近亲改用 [`BREED_RARITY_DOWN_PROB_INCEST`]。
pub const BREED_RARITY_DOWN_PROB: f64 = 0.10;
/// 近亲繁殖星级 −1 的概率(近亲只跌不升)。
pub const BREED_RARITY_DOWN_PROB_INCEST: f64 = 0.40;

// —— 退役 / 改名 ——

/// 退役一次性回馈金币地板(唯一地板,叠在按投入返还之上)。
pub const RETIRE_REWARD_BASE: i64 = 100;
/// 退役按生涯累计投入(invested)返还的比例(见 [`retire_reward`](super::logic::retire_reward))。
pub const RETIRE_INVEST_PCT: f64 = 0.18;

/// 改名费用(sink)。
pub const RENAME_COST: i64 = 50;
/// 马名最大字符数。
pub const NAME_MAX_CHARS: usize = 12;

// —— PvP 对战房(P4) ——

/// PvP 赛道长度。
pub const PVP_TRACK_LEN: f32 = 100.0;
/// PvP 整场「手感/状态」系数的标准差(乘进速度,全程相关):制造冷门、让旁注有悬念。
/// PvE 不用此项(其风险来自动态难度档,见 [`Difficulty::npc_ratio`])。
pub const PVP_FORM_SIGMA: f32 = 0.14;
/// 一个房最多参赛人数(满即自动开跑)。
pub const PVP_ROOM_CAP: usize = 8;
/// 下注池平台抽水比例(sink)。
pub const PVP_RAKE: f32 = 0.05;
/// 注额下限(挡 0 注空转)。
pub const PVP_STAKE_MIN: i64 = 20;
/// 不指定注额时的默认注。
pub const PVP_STAKE_DEFAULT: i64 = 50;
/// 注额上限。
pub const PVP_STAKE_MAX: i64 = 2000;
/// 开房后等待报名的超时(秒)。
pub const PVP_LOBBY_TIMEOUT_SECS: u64 = 120;
/// PvP 奖池按名次分配的系数(下标 = 名次-1)。
pub const PVP_PAYOUT_FACTOR: [f32; 3] = [0.6, 0.25, 0.15];

// —— 特性 / 词条(出生/繁殖得来的随机被动)——

/// 一条马匹特性(被动)。判别式即位掩码里的 bit 位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trait {
    /// 后程之王:后半程掉速更少。
    LateSurge = 0,
    /// 闪电起步:开局抢位加成更猛。
    QuickStart = 1,
    /// 暴击体质:暴击概率常驻 +。
    CritBeast = 2,
    /// 铁蹄:受伤概率减半。
    IronHoof = 3,
    /// 天才:训练好值档加权 +。
    Genius = 4,
    /// 韧者:发挥更稳(减抖动 + 小幅速度补偿)。
    Tenacious = 5,
    /// 追击者:落后追赶暴击更猛。
    Pursuer = 6,
    /// 反射神经:更易闪避、且大幅缩短被持续干扰的回合。
    Reflex = 7,
    /// 幸运儿:赛后掉落概率更高。
    Fortuitous = 8,
    /// 疾风:前半程速度加成。
    Gale = 9,
}

/// 一匹马最多带的特性数。
pub const TRAIT_MAX: u32 = 2;
/// 出生按星级随机得特性:每个特性槽(共 [`TRAIT_MAX`] 个)命中一条特性的概率。下标 = rarity-1。
pub const TRAIT_BIRTH_PROB: [f64; 4] = [0.08, 0.15, 0.28, 0.45];
/// 繁殖时父母每条特性遗传给子代的概率。
pub const TRAIT_INHERIT_PROB: f64 = 0.5;
/// 繁殖时子代变异出一条全新特性的概率。
pub const TRAIT_MUTATE_PROB: f64 = 0.06;

/// 特性「后程之王」:后半程额外加到后程系数上的值。
pub const TRAIT_LATE_BONUS: f32 = 0.10;
/// 特性「闪电起步」:起跑阶段额外的速度乘数加成。
pub const TRAIT_START_BONUS: f32 = 0.2;
/// 特性「暴击体质」:常驻暴击概率加成。
pub const TRAIT_CRIT_BONUS: f64 = 0.10;
/// 特性「铁蹄」:受伤概率乘数(减半)。
pub const TRAIT_INJURY_MULT: f64 = 0.5;
/// 特性「韧者」:抖动幅度乘数(更稳)。配合 [`TRAIT_TENACITY_SPEED_MULT`] 的小幅速度补偿,
/// 避免「减自身方差」(被 `max(0)` 截断的上偏置随之缩水)在各势位都成净负——与稳行同思路。
pub const TRAIT_JITTER_MULT: f32 = 0.70;
/// 特性「韧者」:小幅速度补偿(命中时乘进基础位移)。实测使 normal/hard/master 的胜率 delta 由净负回到 ≈0。
pub const TRAIT_TENACITY_SPEED_MULT: f32 = 1.05;
/// 特性「追击者」:落后追赶暴击的乘子。
pub const TRAIT_PURSUIT_MULT: f64 = 1.35;
/// 特性「疾风」:前半程(progress < [`TRAIT_GALE_PHASE`])速度乘子。
pub const TRAIT_GALE_MULT: f32 = 1.12;
/// 特性「疾风」生效的赛道前段比例。
pub const TRAIT_GALE_PHASE: f32 = 0.5;
/// 特性「反射神经」:闪避概率乘子(再封顶 [`AGI_DODGE_CAP_REFLEX`])。
pub const TRAIT_REFLEX_DODGE_MULT: f32 = 1.15;
/// 特性「幸运儿」:赛后掉落概率乘子。
pub const TRAIT_FORTUNE_DROP_MULT: f64 = 1.3;

impl Trait {
    pub const ALL: [Trait; 10] = [
        Trait::LateSurge,
        Trait::QuickStart,
        Trait::CritBeast,
        Trait::IronHoof,
        Trait::Genius,
        Trait::Tenacious,
        Trait::Pursuer,
        Trait::Reflex,
        Trait::Fortuitous,
        Trait::Gale,
    ];

    pub fn bit(self) -> i32 {
        1 << self as i32
    }

    pub fn in_mask(self, mask: i32) -> bool {
        mask & self.bit() != 0
    }

    pub fn from_mask(mask: i32) -> Vec<Trait> {
        Trait::ALL.into_iter().filter(|t| t.in_mask(mask)).collect()
    }

    pub fn name(self) -> &'static str {
        match self {
            Trait::LateSurge => "后程之王",
            Trait::QuickStart => "闪电起步",
            Trait::CritBeast => "暴击体质",
            Trait::IronHoof => "铁蹄",
            Trait::Genius => "天才",
            Trait::Tenacious => "韧者",
            Trait::Pursuer => "追击者",
            Trait::Reflex => "反射神经",
            Trait::Fortuitous => "幸运儿",
            Trait::Gale => "疾风",
        }
    }
}

/// 赛马榜展示的名次条数。
pub const RANK_TOP: u64 = 10;
/// 胜率榜的最低出战门槛(防小样本刷榜)。
pub const RANK_MIN_RACES: i32 = 30;

/// 每日首胜奖:当天该马第一次夺冠(PvE/PvP 通用)额外发的金币。
pub const DAILY_FIRST_WIN_BONUS: i64 = 50;

// —— 幸运:赛后产出(幸运退出赛中后唯一收益渠道,见 [`Stat::Luk`] / mod 比赛结算)——

/// 赛后掉落概率 = `clamp((幸运 - LUCK_DROP_FLOOR) / LUCK_DROP_DIV, 0, LUCK_DROP_CAP)`。
pub const LUCK_DROP_FLOOR: f64 = 30.0;
/// 掉落概率分母(幸运每点的边际)。
pub const LUCK_DROP_DIV: f64 = 340.0;
/// 掉落概率上限(幸运儿特性 ×1.3 后仍封此)。
pub const LUCK_DROP_CAP: f64 = 0.75;
/// 名次奖励的幸运加成上限:奖励 ×`(1 + clamp(幸运/LUCK_REWARD_DIV, 0, LUCK_REWARD_CAP))`。
pub const LUCK_REWARD_DIV: f32 = 700.0;
/// 名次奖励幸运加成的封顶。
pub const LUCK_REWARD_CAP: f32 = 0.30;
/// PvP 赛后掉落概率乘子:PvP 一场多名参赛者各自 roll,掉落总量随人数放大,出售后≈按场铸币;故 PvP 掉率减半,
/// 压低「互刷 PvP 刷掉落变现」这个水龙头(PvE 不受影响,传 1.0)。
pub const PVP_DROP_MULT: f64 = 0.5;
/// 赛后掉落命中后的品质大类权重 `(常见, 中档, 珍材)`。珍材档刻意稀(4),farming 仅加速不架空商店。
pub const DROP_QUALITY_WEIGHTS: [u32; 3] = [74, 22, 4];
/// 掉落·常见档物品池(便宜消耗品)。
pub const DROP_COMMON: [Item; 4] = [Item::Care1, Item::Feed1, Item::FineForage, Item::EnergyDrink];
/// 掉落·中档物品池(中价道具/恢复)。
pub const DROP_MID: [Item; 6] = [Item::Boost, Item::Banana, Item::Mark, Item::Scare, Item::Feed2, Item::Care2];
// 掉落·珍材档复用 [`Item::TREASURE`] + [`GACHA_TREASURE_WEIGHTS`](偏向便宜的红绳/染色/洗髓)。

// —— 成就 / 称号 / 图鉴 ——

/// 「名门」成就要求的代数门槛(养到第几代)。
pub const ACH_DYNASTY_GEN: i32 = 5;
/// 「百战」成就要求的生涯总胜场(全部马累计)。
pub const ACH_HUNDRED_WINS: i32 = 100;
/// 「大户」成就要求的在厩马匹数。
pub const ACH_TYCOON_HORSES: usize = 10;

/// 一个成就(判定条件见 [`logic`](super::logic),达成发一次性金币 + 可能解锁称号)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Achievement {
    /// 初出茅庐:任一马首次夺冠。
    FirstWin = 1,
    /// 百战之师:生涯累计 100 胜。
    HundredWins = 2,
    /// 育马有道:配出过后代(有第 2 代及以上的马)。
    FirstBreed = 3,
    /// 名门望族:养出第 5 代及以上的马。
    Dynasty = 4,
    /// 天选之驹:拥有一匹 ★4 马。
    Chosen = 5,
    /// 天赋异禀:拥有一匹带特性的马。
    Gifted = 6,
    /// 集色大师:在厩马覆盖全部 6 种毛色。
    Collector = 7,
    /// 赛马大户:在厩满 10 匹马。
    Tycoon = 8,
}

impl Achievement {
    pub const ALL: [Achievement; 8] = [
        Achievement::FirstWin,
        Achievement::HundredWins,
        Achievement::FirstBreed,
        Achievement::Dynasty,
        Achievement::Chosen,
        Achievement::Gifted,
        Achievement::Collector,
        Achievement::Tycoon,
    ];

    pub fn code(self) -> i32 {
        self as i32
    }

    pub fn name(self) -> &'static str {
        match self {
            Achievement::FirstWin => "初出茅庐",
            Achievement::HundredWins => "百战之师",
            Achievement::FirstBreed => "育马有道",
            Achievement::Dynasty => "名门望族",
            Achievement::Chosen => "天选之驹",
            Achievement::Gifted => "天赋异禀",
            Achievement::Collector => "集色大师",
            Achievement::Tycoon => "赛马大户",
        }
    }

    pub fn desc(self) -> &'static str {
        match self {
            Achievement::FirstWin => "任一匹马首次夺冠",
            Achievement::HundredWins => "生涯累计 100 胜",
            Achievement::FirstBreed => "配出过后代",
            Achievement::Dynasty => "养出第 5 代及以上的马",
            Achievement::Chosen => "拥有一匹 ★4 马",
            Achievement::Gifted => "拥有一匹带特性的马",
            Achievement::Collector => "在厩马集齐 6 种毛色",
            Achievement::Tycoon => "在厩满 10 匹马",
        }
    }

    pub fn reward(self) -> i64 {
        match self {
            Achievement::FirstWin => 30,
            Achievement::HundredWins => 300,
            Achievement::FirstBreed => 50,
            Achievement::Dynasty => 200,
            Achievement::Chosen => 200,
            Achievement::Gifted => 40,
            Achievement::Collector => 150,
            Achievement::Tycoon => 100,
        }
    }

    /// 解锁的称号(部分成就给;`None` 则不带称号)。
    pub fn title(self) -> Option<&'static str> {
        match self {
            Achievement::HundredWins => Some("百胜名驹"),
            Achievement::Dynasty => Some("名门"),
            Achievement::Chosen => Some("天选"),
            Achievement::Collector => Some("集色大师"),
            _ => None,
        }
    }
}

// ============================================================================
// 经济改造(慢攒养成 + 巨鳄有处花)。整体路线/数值见项目记忆 horse-economy-redesign。
// ============================================================================

// —— PvE 水龙头:报名费乘实力 ——
/// 报名费实力系数指数:fee_eff = round((entry_fee + entry_step × 当日账号场次) × power_factor^此值)。
/// power_factor 复用奖励侧 clamp(power/[`REWARD_POWER_REF`], [`REWARD_POWER_CLAMP`])。强马报名费涨得比奖快、净收敛。
pub const POWER_FEE_EXP: f32 = 1.5;

// —— 轻厩养税(兜底 sink:按马·按业务日)——
/// 免费额度:生涯累计获取序 `acq_seq` ≤ 此值的马永久免税(绝对口径、单调,退役减员不恢复=一次性新手保护)。
pub const STABLE_TAX_FREE_N: i32 = 4;
/// 星级日税(下标 = rarity-1,币/匹/业务日,减免前)。
pub const STABLE_TAX_BY_RARITY: [i64; 4] = [1, 3, 7, 14];
/// 设施(马场)养税减免上限(0..1)。
pub const STABLE_TAX_REDUCTION_CAP: f32 = 0.40;

// —— 设施投资(账号级四栋 + 按马两通道;造价 = round(base × ratio^(lv-1)),见 logic::facility_cost)——
// 训练场→降训练费 / 马场=照料中枢→降治疗费+养税减免+珍爱马投资槽 / 血统祠堂→降繁殖费 / 仓库→扩容量。
// 红线:账号级走降本/容量、按马走 reach/PvP,不对同一增益双计。
pub const FAC_TRAIN_MAX_LV: i16 = 8;
pub const FAC_TRAIN_COST_BASE: f64 = 800.0;
pub const FAC_TRAIN_COST_RATIO: f64 = 1.42;
/// 训练场每级降训练费比例(Lv8 → -40%)。
pub const FAC_TRAIN_DISCOUNT_PER_LV: f32 = 0.05;
pub const FAC_STABLE_MAX_LV: i16 = 8;
pub const FAC_STABLE_COST_BASE: f64 = 1000.0;
pub const FAC_STABLE_COST_RATIO: f64 = 1.42;
/// 马场每级降治疗费比例(Lv8 → -40%)。
pub const FAC_HEAL_DISCOUNT_PER_LV: f32 = 0.05;
/// 马场每级养税减免比例(Lv8 → -40%,再受 [`STABLE_TAX_REDUCTION_CAP`] 封顶)。
pub const FAC_TAX_REDUCTION_PER_LV: f32 = 0.05;
/// 珍爱马投资槽数随马场等级(下标 = 马场Lv 0..=8):限制按马投资规模。
pub const CHERISH_SLOTS_BY_STABLE_LV: [usize; 9] = [0, 1, 1, 2, 2, 2, 3, 3, 3];
pub const FAC_BLOOD_MAX_LV: i16 = 8;
pub const FAC_BLOOD_COST_BASE: f64 = 900.0;
pub const FAC_BLOOD_COST_RATIO: f64 = 1.42;
/// 血统祠堂每级降繁殖费比例(Lv8 → -40%)。
pub const FAC_BREED_DISCOUNT_PER_LV: f32 = 0.05;
pub const FAC_WAREHOUSE_MAX_LV: i16 = 8;
pub const FAC_WAREHOUSE_COST_BASE: f64 = 700.0;
pub const FAC_WAREHOUSE_COST_RATIO: f64 = 1.40;
/// 仓库每级扩在厩上限(在厩上限 = [`STABLE_CAP`] + 此 × 仓库Lv,封顶 [`STABLE_CAP_HARD_MAX`])。
pub const FAC_WAREHOUSE_CAP_PER_LV: usize = 1;
/// 扩容后的在厩硬上限(仓库满级 16 + 1×8 = 24)。
pub const STABLE_CAP_HARD_MAX: usize = 24;
/// 按马·专属训练台(抬该马自身 reach 上限)。
pub const DESK_MAX_LV: i16 = 5;
pub const DESK_COST_BASE: f64 = 600.0;
pub const DESK_COST_RATIO: f64 = 1.62;
/// 每级抬高该马每维 reach 上限点数(Lv5 → +15/维,叠加后仍 .min([`REACH_HARD_MAX`]))。
pub const DESK_REACH_PER_LV: i32 = 3;
/// 按马·专属战意调理(PvP-only 战力乘子)。
pub const PREP_MAX_LV: i16 = 5;
pub const PREP_COST_BASE: f64 = 400.0;
pub const PREP_COST_RATIO: f64 = 1.60;
/// 每级 PvP-only power 乘子增量(Lv5 → +10%,仅 PvP 跑判与赔率,不进 PvE `reward_for`)。
pub const PREP_PVP_MULT_PER_LV: f32 = 0.02;

/// 账号级设施(四栋)。造价 = round(cost_base × cost_ratio^(lv-1)),见 logic::facility_cost。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Facility {
    /// 训练场:降训练费。
    TrainGround,
    /// 马场(照料中枢):降治疗费 + 养税减免 + 珍爱马投资槽。
    Stable,
    /// 血统祠堂:降繁殖费。
    Bloodline,
    /// 仓库:扩在厩容量。
    Warehouse,
}

impl Facility {
    pub const ALL: [Facility; 4] = [Facility::TrainGround, Facility::Stable, Facility::Bloodline, Facility::Warehouse];

    pub fn parse(s: &str) -> Option<Facility> {
        match s.trim() {
            "训练场" | "训练" => Some(Facility::TrainGround),
            "马场" => Some(Facility::Stable),
            "血统祠堂" | "祠堂" | "血统" => Some(Facility::Bloodline),
            "仓库" => Some(Facility::Warehouse),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Facility::TrainGround => "训练场",
            Facility::Stable => "马场",
            Facility::Bloodline => "血统祠堂",
            Facility::Warehouse => "仓库",
        }
    }

    pub fn effect(self) -> &'static str {
        match self {
            Facility::TrainGround => "降训练费",
            Facility::Stable => "降治疗费、减厩养税,开珍爱槽",
            Facility::Bloodline => "降繁殖费",
            Facility::Warehouse => "扩在厩上限",
        }
    }

    pub fn max_lv(self) -> i16 {
        match self {
            Facility::TrainGround => FAC_TRAIN_MAX_LV,
            Facility::Stable => FAC_STABLE_MAX_LV,
            Facility::Bloodline => FAC_BLOOD_MAX_LV,
            Facility::Warehouse => FAC_WAREHOUSE_MAX_LV,
        }
    }

    pub fn cost_base(self) -> f64 {
        match self {
            Facility::TrainGround => FAC_TRAIN_COST_BASE,
            Facility::Stable => FAC_STABLE_COST_BASE,
            Facility::Bloodline => FAC_BLOOD_COST_BASE,
            Facility::Warehouse => FAC_WAREHOUSE_COST_BASE,
        }
    }

    pub fn cost_ratio(self) -> f64 {
        match self {
            Facility::TrainGround => FAC_TRAIN_COST_RATIO,
            Facility::Stable => FAC_STABLE_COST_RATIO,
            Facility::Bloodline => FAC_BLOOD_COST_RATIO,
            Facility::Warehouse => FAC_WAREHOUSE_COST_RATIO,
        }
    }
}

/// 按马设施(珍爱马专属两通道)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HorseFacility {
    /// 专属训练台:抬该马自身 reach 上限。
    Desk,
    /// 专属战意调理:PvP-only 战力乘子。
    Prep,
}

impl HorseFacility {
    pub fn parse(s: &str) -> Option<HorseFacility> {
        match s.trim() {
            "训练台" | "专属训练台" => Some(HorseFacility::Desk),
            "战意" | "战意调理" | "专属战意调理" => Some(HorseFacility::Prep),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            HorseFacility::Desk => "专属训练台",
            HorseFacility::Prep => "专属战意调理",
        }
    }

    pub fn effect(self) -> &'static str {
        match self {
            HorseFacility::Desk => "抬高这匹马的资质上限",
            HorseFacility::Prep => "加这匹马的 PvP 战力,只在 PvP 生效",
        }
    }

    pub fn max_lv(self) -> i16 {
        match self {
            HorseFacility::Desk => DESK_MAX_LV,
            HorseFacility::Prep => PREP_MAX_LV,
        }
    }

    pub fn cost_base(self) -> f64 {
        match self {
            HorseFacility::Desk => DESK_COST_BASE,
            HorseFacility::Prep => PREP_COST_BASE,
        }
    }

    pub fn cost_ratio(self) -> f64 {
        match self {
            HorseFacility::Desk => DESK_COST_RATIO,
            HorseFacility::Prep => PREP_COST_RATIO,
        }
    }
}

// —— 血统(只提下限、不抬上限;硬墙 [`REACH_HARD_MAX`]=220 / [`GROWTH_MAX`]=155 不变)——
// 代数不进养成公式(不抬潜力上限),只当血统标记(成就/血统库)。好血统价值在「下限稳」:子代每维
// reach ≥ FLOOR×较优亲本、growth ≥ FLOOR×较优亲本,配出的后代不拉胯,但也超不过亲本/星级天花板。
/// 子代每维 reach 的保底比例(× 较优亲本该维 reach)。轻保底,主要防极端坏苗。
pub const BREED_REACH_FLOOR: f32 = 0.65;
/// 子代 growth 的保底比例(× 较优亲本 growth)。
pub const BREED_GROWTH_FLOOR: f32 = 0.65;
/// 血统库基础容量(存退役种马,不占 [`STABLE_CAP`];扩容走仓库设施)。
pub const BLOODLINE_LIB_CAP: usize = 6;
/// 转血统资产(退役马存库)费用,按星(下标 = rarity-1);存库即放弃退役回馈。
pub const DEPOSIT_FEE_BY_RARITY: [i64; 4] = [200, 400, 800, 1500];
/// 从库内种马配种的繁殖费附加倍率。
pub const STUD_BREED_SURCHARGE: f32 = 1.5;

// —— PvP 段位赔率(无匹配·只开房;马主段位纯荣誉,不进赔率)——
/// ELO 初始分(马段位 + 马主段位同初始)。
pub const ELO_INIT: i32 = 1200;
/// ELO 标度(逻辑斯蒂分母)。
pub const ELO_SCALE: f64 = 400.0;
/// 马段位定级期(前 [`ELO_PLACEMENT_GAMES`] 场)K 值。
pub const ELO_K_PLACEMENT: f64 = 40.0;
pub const ELO_PLACEMENT_GAMES: i32 = 10;
/// 马段位稳定期 K 值。
pub const ELO_K_NORMAL: f64 = 24.0;
/// 马主段位 K 值(全程恒定;纯荣誉/排行,不进赔率)。
pub const ELO_K_OWNER: f64 = 12.0;
/// 即时 power 并入赔率组合分:C = 马段位ELO + 此 × (power - [`REWARD_POWER_REF`])。
pub const POWER_TO_ELO: f64 = 2.2;
/// 双修正派彩·赔率/名次价值扩散系数。
pub const PVP_ODDS_S1: f64 = 1.0;
/// 双修正派彩·期望名次差扩散系数。
pub const PVP_ODDS_S2: f64 = 0.30;
/// 奖圈内(前三/拿牌)倒扣押注上限比例:派彩 floor = (1 - 此值) × stake(= 0.25×stake)。圈外 floor=0(倒扣≤100%)。
pub const PVP_REVCAP_INRING: f64 = 0.75;

#[cfg(test)]
mod tests {
    use super::*;

    /// [`TRAIN_MAG_MEAN`] 是 [`TRAIN_TIERS`] 加权均值的手抄副本;断言两者一致,防以后改档位忘改它而整体漂移。
    #[test]
    fn train_mag_mean_matches_tiers() {
        let total: u32 = TRAIN_TIERS.iter().map(|t| t.0).sum();
        let mean: f32 = TRAIN_TIERS.iter().map(|&(w, lo, hi)| w as f32 * (lo + hi) / 2.0).sum::<f32>() / total as f32;
        assert!((mean - TRAIN_MAG_MEAN).abs() < 0.01, "TRAIN_MAG_MEAN={TRAIN_MAG_MEAN} 应 ≈ 档位加权均值 {mean}");
    }
}
