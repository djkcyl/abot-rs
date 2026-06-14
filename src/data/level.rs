//! 经验 / 等级的**共享**数学（与库无关的纯函数 + 小值对象）。
//!
//! 经验是一个**跨插件共享**的用户属性（与游戏币同性质），故等级换算放在核心数据层，
//! 经 [`AUser`](crate::data::AUser) 的 `add_exp`/`level`/`level_info` 暴露给各插件——
//! 插件**不**各自实现等级公式，统一走这里，口径一致。
//!
//! 曲线（几何 / 指数累计）：每升一级所需经验按 [`LEVEL_RATIO`] 倍递增——早期升级快、
//! 越往后越陡（冲段感）。到第 `L` 级所需**累计**经验 = 首项 [`LEVEL_BASE`]、公比
//! [`LEVEL_RATIO`] 的等比数列前 `L` 项和 `LEVEL_BASE * (LEVEL_RATIO^L - 1) / (LEVEL_RATIO - 1)`，
//! 故 0→0、1→100、2→210、3→331、5→611、10→1594、20→5727 …。曲线快慢/陡度只调这两个常量。

/// 等比数列首项：第 0→1 级所需经验；其后每级所需经验按 [`LEVEL_RATIO`] 倍递增。
pub const LEVEL_BASE: i64 = 100;
/// 每级所需经验相对上一级的倍率（> 1，越大越陡）。
pub const LEVEL_RATIO: f64 = 1.1;

/// 升到（恰好处于）第 `level` 级所需的**累计**经验。
///
/// `= LEVEL_BASE * (LEVEL_RATIO^level - 1) / (LEVEL_RATIO - 1)`（等比前 `level` 项和）：
/// 0 → 0、1 → 100、2 → 210、3 → 331、5 → 611 …。`level <= 0` 一律为 0。
pub fn exp_to_reach(level: i64) -> i64 {
    if level <= 0 {
        return 0;
    }
    let v = LEVEL_BASE as f64 * (LEVEL_RATIO.powi(level as i32) - 1.0) / (LEVEL_RATIO - 1.0);
    // 极高等级时几何和会超出 i64，饱和到上限以免溢出（实际等级远达不到这里）。
    if v >= i64::MAX as f64 { i64::MAX } else { v.round() as i64 }
}

/// 给定经验值，求其当前等级 = 满足 `exp_to_reach(L) <= exp` 的最大 `L`（下取整，最低 0）。
///
/// 先用对数求近似根，再**对照 `exp_to_reach` 修正 ±1**，规避浮点误差导致的临界点
/// （恰好等于某级阈值时）错判；负经验钳到 0 级。
pub fn level_of(exp: i64) -> i64 {
    if exp <= 0 {
        return 0;
    }
    // 解 LEVEL_BASE*(r^L - 1)/(r-1) <= exp，即 r^L <= 1 + exp*(r-1)/LEVEL_BASE。
    // 取 L = log_r(1 + exp*(r-1)/LEVEL_BASE) 下整为起点，再对照 exp_to_reach 校正。
    let approx = ((1.0 + exp as f64 * (LEVEL_RATIO - 1.0) / LEVEL_BASE as f64).ln() / LEVEL_RATIO.ln()).floor();
    let mut level = approx.max(0.0) as i64;

    // 向上修正：下一级阈值仍 <= exp 则更高（带上限护栏，防病态输入空转）。
    while level < 100_000 && exp_to_reach(level + 1) <= exp {
        level += 1;
    }
    // 向下修正：若本级阈值已 > exp，则实际更低（补偿 f64 上偏），但不低于 0。
    while level > 0 && exp_to_reach(level) > exp {
        level -= 1;
    }
    level
}

/// 一段「当前等级 + 在该级内的进度」的快照。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LevelInfo {
    /// 当前等级（`level_of(exp)`）。
    pub level: i64,
    /// 当前级内已积累的经验：`exp - exp_to_reach(level)`（`0 <= into_level < level_span`）。
    pub into_level: i64,
    /// 当前级到下一级的台阶宽度：`exp_to_reach(level + 1) - exp_to_reach(level)`。
    pub level_span: i64,
}

/// 由经验值算出 [`LevelInfo`]（当前级 + 级内进度 + 本级台阶宽度）。
pub fn level_info(exp: i64) -> LevelInfo {
    let level = level_of(exp);
    let base = exp_to_reach(level);
    LevelInfo {
        level,
        into_level: exp - base,
        // 下取整到 1:极高等级时 exp_to_reach 饱和到 i64::MAX,相邻两级阈值相等会得 0,
        // 消费方按 `into/ span` 显示进度会除零/显示 X/0。这里保底 1。
        level_span: (exp_to_reach(level + 1) - base).max(1),
    }
}

/// 一次加经验前后的等级对照（由 [`AUser::add_exp`](crate::data::AUser::add_exp) 返回）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LevelChange {
    /// 加经验前的等级。
    pub before: i64,
    /// 加经验后的等级。
    pub after: i64,
}

impl LevelChange {
    /// 这次变动是否升了级（`after > before`）。
    pub fn leveled_up(&self) -> bool {
        self.after > self.before
    }
}
