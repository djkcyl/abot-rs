//! 赛马插件自有实体。`horse` 存一匹马的当前态,与核心 `user` 仅软关联(`owner_uin`、父母 id 均不建 FK,
//! 留谱系可指向已退役/易主的马),共享经济只走 `AUser`。

/// `horse` 表实体。
pub mod horse {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
    #[sea_orm(table_name = "horse")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        /// 主人 QQ(软关联 `user`)。
        pub owner_uin: i64,
        pub name: String,
        /// 毛色(纯外观,不影响数值)。
        pub color: i16,
        /// 性别(0 公 / 1 母)。
        pub sex: i16,
        /// 代数(初代 1,后代 = max(父,母)+1)。
        pub generation: i32,
        /// 星级(★1–4,决定隐藏潜力带宽)。
        pub rarity: i16,
        /// 特性位掩码(见 [`consts::Trait`](super::super::consts::Trait))。
        pub traits: i32,

        pub spd: i32,
        pub sta: i32,
        pub brs: i32,
        pub agi: i32,
        pub luk: i32,

        /// 各维软上限尺度 `reach`:训练增量 ∝ `exp(-当前值/此值)`,软墙非硬墙。出生定,五维各一。
        pub pot_spd: i32,
        pub pot_sta: i32,
        pub pot_brs: i32,
        pub pot_agi: i32,
        pub pot_luk: i32,
        /// 成长系数:单次训练加成乘数,全维共用。存整数 = ×100(1.0 存 100)。
        pub growth: i32,

        /// 体力 0..=100(训练/比赛消耗,随时间恢复)。
        pub vitality: i32,
        /// 饱食度 0..=100(随时间变饿,喂养回升)。
        pub satiety: i32,
        /// 体力/饱食共用的上次结算时刻。
        pub state_at: DateTimeWithTimeZone,

        /// 寿命:不可逆生涯耗材(只随比赛/训练降、道具回),0 不致死但削耐力/训练效率、易伤。
        pub lifespan: i32,
        /// 可回复上限:护理回寿命的天花板,每次回复永久 −N、不可逆(`lifespan` 回不过它)。
        pub lifespan_cap: i32,
        /// 出生定的寿命上限:life_ratio = lifespan / lifespan_max,惩罚/受伤公式的分母。
        pub lifespan_max: i32,

        /// 伤病等级(0 无 / 1 轻 / 2 中 / 3 重;局内触发,赛后落库)。
        pub injury: i16,
        /// 伤病恢复到期(null = 无伤)。
        pub injury_until: Option<DateTimeWithTimeZone>,
        /// 伤痕重度(0 无 / 1 / 2 / 3):伤病好后留的隐患,带 stat 惩罚且易复发,按 `scar_until` 到期消。
        pub scar: i16,
        /// 伤痕到期(null = 无伤痕)。
        pub scar_until: Option<DateTimeWithTimeZone>,
        /// 母马繁殖冷却到期(null = 可繁殖)。
        pub breed_cd_until: Option<DateTimeWithTimeZone>,
        /// 已作种次数(达 [`BREED_COUNT_MAX`](super::super::consts::BREED_COUNT_MAX) 不能再繁殖)。
        pub breed_count: i32,

        /// 状态(0 在厩 / 2 退役;并发由同人单飞 `single_flight_user` 防护)。
        pub status: i16,
        /// 去规范化:胜场快照(榜直接读)。
        pub wins: i32,
        /// 去规范化:总场快照。
        pub races: i32,

        /// `train_today` 对应的业务日。
        pub train_day: Date,
        /// 今日已训练次数(业务日切换时归零)。
        pub train_today: i32,
        /// `race_today` 对应的业务日。
        pub race_day: Date,
        /// 今日已比赛次数(业务日切换时归零)。
        pub race_today: i32,
        /// 上次领「每日首胜奖」的业务日。
        pub bonus_day: Date,
        /// 当前赛季键(`YYYY-MM`),换月在 `finish_race` 里懒重置赛季胜场。
        pub season_key: String,
        /// 本赛季胜场(`season_key` 变更即归零)。
        pub season_wins: i32,
        /// 生涯累计养成投入(币):各养成处埋点累加,退役按比例返还。
        pub invested: i64,
        /// 生涯累计调教次数(每次训练 +1):驱动全局调教衰减(见
        /// [`train_total_decay`](super::super::logic::train_total_decay)),练得越多每次涨越少。
        pub train_total: i32,

        /// 父马 id(软自关联,null = 初代)。
        pub father_id: Option<i64>,
        /// 母马 id(软自关联,null = 初代)。
        pub mother_id: Option<i64>,
        pub created_at: DateTimeWithTimeZone,
    }

    /// `horse` 表无外联关系(父母为软关联,不建 FK)。
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

// 背包不另建表:赛马道具存进共享的 `game_item`(见 [`crate::data::inventory`])。这里只剩抽卡保底表。

/// `horse_gacha` 表实体(抽卡软保底计数,一人一行)。
pub mod gacha {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
    #[sea_orm(table_name = "horse_gacha")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub uin: i64,
        /// 距上次出马累计抽数(到保底强制出马后清零)。
        pub pity: i32,
    }

    /// 无外联关系。
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

/// `horse_achievement` 表实体(一人已达成的成就,一成就一行)。
pub mod achievement {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
    #[sea_orm(table_name = "horse_achievement")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub uin: i64,
        /// 成就代码(见 [`consts::Achievement`](super::super::consts::Achievement))。
        #[sea_orm(primary_key, auto_increment = false)]
        pub code: i32,
        pub earned_at: DateTimeWithTimeZone,
    }

    /// 无外联关系。
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}
