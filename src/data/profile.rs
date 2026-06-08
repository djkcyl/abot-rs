//! `ProfileSection` —— 「个人数据」的**插件自注册贡献槽**，与 [`PluginMigration`](crate::data::migration::PluginMigration) 同款机制。
//!
//! 「个人数据」要展示的不止核心字段（金币/经验/等级），还含各插件私有的统计（签到连签、发言数、
//! 将来赛马战绩 …）。为不破坏「插件自有数据」的墙——个人数据插件**不**直接读各插件表——这里给一个
//! 自注册槽：每个插件 `submit!` 一个 [`ProfileProvider`]，按 `(db, uin)` 产出自己的一行；个人数据
//! 经 [`collect_grouped`] 统一收集，核心/个人数据都**不**引用任何具体插件。
//!
//! **分组**（[`ProfileGroup`]）：普通统计（金币/签到/发言…）与**游戏战绩**（赛马/猜拳…）分桶，
//! 个人数据据此把战绩另起一段，不和普通数据混在一起。顺序 = `inventory` 注册顺序。

use nagisa::async_trait;
use sea_orm::DatabaseConnection;

/// 贡献行的分组：普通统计 vs 游戏战绩（战绩在个人数据里另起一段展示）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProfileGroup {
    /// 普通统计（金币/签到/发言…）。
    Stat,
    /// 游戏战绩（赛马/猜拳…），单独成组。
    Game,
}

/// 一个「个人数据贡献者」的自注册槽位：包一个构造 [`ProfileProvider`] 的函数指针。
pub struct ProfileSection(pub fn() -> Box<dyn ProfileProvider>);
nagisa::inventory::collect!(ProfileSection);

/// 一个插件对「个人数据」的贡献：给定连接 + 用户 `uin`，产出一行展示文本（无数据 → `None`）。
#[async_trait]
pub trait ProfileProvider: Send + Sync {
    /// 本贡献所属分组。默认普通统计；游戏插件覆写为 [`ProfileGroup::Game`]。
    fn group(&self) -> ProfileGroup {
        ProfileGroup::Stat
    }
    /// 该用户在本插件的一行数据（如「📅 连签 3 天」/「🐎 赛马 5 胜」）；无则 `None`。
    async fn line(&self, db: &DatabaseConnection, uin: i64) -> Option<String>;
}

/// 收集后的贡献行，按分组分桶。
#[derive(Default)]
pub struct GroupedProfile {
    /// 普通统计行（签到/发言…）。
    pub stats: Vec<String>,
    /// 游戏战绩行（赛马…）。
    pub games: Vec<String>,
}

/// 收集所有已注册插件对某用户的贡献行，按 [`ProfileGroup`] 分桶（顺序 = 注册顺序）。
pub async fn collect_grouped(db: &DatabaseConnection, uin: i64) -> GroupedProfile {
    let mut out = GroupedProfile::default();
    for section in nagisa::inventory::iter::<ProfileSection> {
        let provider = (section.0)();
        if let Some(line) = provider.line(db, uin).await {
            match provider.group() {
                ProfileGroup::Stat => out.stats.push(line),
                ProfileGroup::Game => out.games.push(line),
            }
        }
    }
    out
}
