//! 赛马插件自有实体。`horse` 存一匹马的当前态,与核心 `user` 仅软关联(`owner_uin`、父母 id 均不建 FK,
//! 留谱系可指向已退役/易主的马),共享经济只走 `AUser`。

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

        /// 生涯累计获取序(出生时 = owner 当时马数 + 1,永不改变):厩养税免税判定 `acq_seq ≤ STABLE_TAX_FREE_N`。
        /// 旧马迁移回填(按 owner、id 升序补 1..N)。0 视作未回填,按 id 兜底(见 logic 免税判定)。
        pub acq_seq: i32,
        /// 马的 PvP 段位分(ELO,初始 [`ELO_INIT`](super::super::consts::ELO_INIT));定 PvP 赔率,不影响 PvE。
        pub elo: i32,
        /// 马的 PvP 累计场次(定级期 K 值判定)。
        pub elo_games: i32,
        /// 按马设施·专属训练台等级(抬该马自身 reach 上限,封顶 [`DESK_MAX_LV`](super::super::consts::DESK_MAX_LV))。
        pub desk_lv: i16,
        /// 按马设施·专属战意调理等级(PvP-only 战力乘子,封顶 [`PREP_MAX_LV`](super::super::consts::PREP_MAX_LV))。
        pub prep_lv: i16,

        /// 父马 id(软自关联,null = 初代)。
        pub father_id: Option<i64>,
        /// 母马 id(软自关联,null = 初代)。
        pub mother_id: Option<i64>,
        pub created_at: DateTimeWithTimeZone,
    }

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
        /// 标准池距上次出 ★3+ 马累计抽数(到保底强制出马后清零)。
        pub std_pity: i32,
        /// 马池独立保底计数(到保底强制出马后清零)。
        pub horse_pity: i32,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

/// `horse_player_daily` 表实体:账号·业务日计数(报名费日内递增的账号级口径)。复合主键 (uin, day) 天滚动。
pub mod player_daily {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
    #[sea_orm(table_name = "horse_player_daily")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub uin: i64,
        /// 业务日(凌晨4点边界)。
        #[sea_orm(primary_key, auto_increment = false)]
        pub day: Date,
        /// 当日该账号已报名比赛次数(驱动报名费日内递增,跨业务日因主键含 day 自然归零)。
        pub account_races_today: i32,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

/// `horse_player_meta` 表实体:账号级持久态(设施等级 + 马主段位 + 厩养税结算)。一人一行。
pub mod player_meta {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
    #[sea_orm(table_name = "horse_player_meta")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub uin: i64,
        /// 账号级设施·训练场等级(降训练费)。
        pub train_lv: i16,
        /// 账号级设施·马场等级(降治疗费 + 养税减免 + 珍爱马投资槽)。
        pub stable_lv: i16,
        /// 账号级设施·血统祠堂等级(降繁殖费)。
        pub blood_lv: i16,
        /// 账号级设施·仓库等级(扩在厩上限)。
        pub warehouse_lv: i16,
        /// 马主段位分(ELO,纯荣誉/排行,不进赔率)。
        pub owner_elo: i32,
        pub owner_elo_games: i32,
        /// 上次厩养税结算的业务日。
        pub tax_settled_day: Date,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

/// `horse_bloodline_lib` 表实体:血统库成员(退役种马存库,不占 [`STABLE_CAP`](super::consts::STABLE_CAP))。复合主键 (uin, horse_id)。
pub mod bloodline_lib {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
    #[sea_orm(table_name = "horse_bloodline_lib")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub uin: i64,
        #[sea_orm(primary_key, auto_increment = false)]
        pub horse_id: i64,
        pub at: DateTimeWithTimeZone,
    }

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

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}
