//! 赛马插件建表迁移,经 [`PluginMigration`] 自注册进核心 [`Migrator`](crate::data::migration::Migrator)。
//! 改表直接改这支迁移 + 活库手动 ALTER(或重置),不另起迁移。
//!
//! 五维当前值(`spd`/`sta`/`brs`/`agi`/`luk`)落库为「厘点」= 点 × [`STAT_SCALE`](super::consts::STAT_SCALE);
//! 活库迁移须 `UPDATE horse SET spd=spd*100, sta=sta*100, brs=brs*100, agi=agi*100, luk=luk*100`
//! 再 `ADD COLUMN train_total`(否则旧马五维被当成零点几、且缺列)。
//!
//! # 经济改造的活库手动迁移(fresh DB 由建表迁移自动覆盖,无需手动)
//!
//! - 加 horse 列:`ALTER TABLE horse ADD COLUMN acq_seq int NOT NULL DEFAULT 0,
//!   ADD COLUMN elo int NOT NULL DEFAULT 1200, ADD COLUMN elo_games int NOT NULL DEFAULT 0,
//!   ADD COLUMN desk_lv smallint NOT NULL DEFAULT 0, ADD COLUMN prep_lv smallint NOT NULL DEFAULT 0;`
//! - 回填获取序(决定厩养税免税:最早 N 匹永久免):
//!   `UPDATE horse h SET acq_seq = s.rn FROM
//!    (SELECT id, row_number() OVER (PARTITION BY owner_uin ORDER BY id) rn FROM horse) s WHERE h.id = s.id;`
//!   不回填则存量马 `acq_seq=0` 一律免税(与设计「第 5 匹起收」不符,可接受)。
//! - 抽卡保底拆列:`ALTER TABLE horse_gacha RENAME COLUMN pity TO std_pity;
//!   ALTER TABLE horse_gacha ADD COLUMN horse_pity int NOT NULL DEFAULT 0;`
//! - 新表 `horse_player_daily` / `horse_player_meta` / `horse_bloodline_lib` 由各自 [`PluginMigration`] 自动建。

use sea_orm_migration::prelude::*;

use crate::data::migration::PluginMigration;

nagisa::inventory::submit! {
    PluginMigration(|| Box::new(Migration))
}
// 背包走核心共享 `game_item`,故插件侧不另建背包表。
nagisa::inventory::submit! {
    PluginMigration(|| Box::new(GachaMigration))
}
nagisa::inventory::submit! {
    PluginMigration(|| Box::new(AchievementMigration))
}
nagisa::inventory::submit! {
    PluginMigration(|| Box::new(PlayerDailyMigration))
}
nagisa::inventory::submit! {
    PluginMigration(|| Box::new(PlayerMetaMigration))
}
nagisa::inventory::submit! {
    PluginMigration(|| Box::new(BloodlineLibMigration))
}

#[derive(DeriveIden)]
enum Horse {
    Table,
    Id,
    OwnerUin,
    Name,
    Color,
    Sex,
    Generation,
    Rarity,
    Traits,
    Spd,
    Sta,
    Brs,
    Agi,
    Luk,
    PotSpd,
    PotSta,
    PotBrs,
    PotAgi,
    PotLuk,
    Growth,
    Vitality,
    Satiety,
    StateAt,
    Lifespan,
    LifespanCap,
    LifespanMax,
    Injury,
    InjuryUntil,
    Scar,
    ScarUntil,
    BreedCdUntil,
    BreedCount,
    Status,
    Wins,
    Races,
    TrainDay,
    TrainToday,
    RaceDay,
    RaceToday,
    BonusDay,
    SeasonKey,
    SeasonWins,
    Invested,
    TrainTotal,
    AcqSeq,
    Elo,
    EloGames,
    DeskLv,
    PrepLv,
    FatherId,
    MotherId,
    CreatedAt,
}

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260610_000003_create_horse"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let i32col = |c: Horse| ColumnDef::new(c).integer().not_null().default(0).to_owned();
        let i16col = |c: Horse| ColumnDef::new(c).small_integer().not_null().default(0).to_owned();
        manager
            .create_table(
                Table::create()
                    .table(Horse::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Horse::Id).big_integer().not_null().auto_increment().primary_key())
                    .col(ColumnDef::new(Horse::OwnerUin).big_integer().not_null())
                    .col(ColumnDef::new(Horse::Name).string().not_null())
                    .col(i16col(Horse::Color))
                    .col(i16col(Horse::Sex))
                    .col(ColumnDef::new(Horse::Generation).integer().not_null().default(1))
                    .col(ColumnDef::new(Horse::Rarity).small_integer().not_null().default(1))
                    .col(i32col(Horse::Traits))
                    .col(i32col(Horse::Spd))
                    .col(i32col(Horse::Sta))
                    .col(i32col(Horse::Brs))
                    .col(i32col(Horse::Agi))
                    .col(i32col(Horse::Luk))
                    .col(i32col(Horse::PotSpd))
                    .col(i32col(Horse::PotSta))
                    .col(i32col(Horse::PotBrs))
                    .col(i32col(Horse::PotAgi))
                    .col(i32col(Horse::PotLuk))
                    .col(ColumnDef::new(Horse::Growth).integer().not_null().default(100))
                    .col(ColumnDef::new(Horse::Vitality).integer().not_null().default(100))
                    .col(ColumnDef::new(Horse::Satiety).integer().not_null().default(100))
                    .col(
                        ColumnDef::new(Horse::StateAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(i32col(Horse::Lifespan))
                    .col(i32col(Horse::LifespanCap))
                    .col(i32col(Horse::LifespanMax))
                    .col(i16col(Horse::Injury))
                    .col(ColumnDef::new(Horse::InjuryUntil).timestamp_with_time_zone().null())
                    .col(i16col(Horse::Scar))
                    .col(ColumnDef::new(Horse::ScarUntil).timestamp_with_time_zone().null())
                    .col(ColumnDef::new(Horse::BreedCdUntil).timestamp_with_time_zone().null())
                    .col(i32col(Horse::BreedCount))
                    .col(i16col(Horse::Status))
                    .col(i32col(Horse::Wins))
                    .col(i32col(Horse::Races))
                    .col(ColumnDef::new(Horse::TrainDay).date().not_null().default("1970-01-01"))
                    .col(i32col(Horse::TrainToday))
                    .col(ColumnDef::new(Horse::RaceDay).date().not_null().default("1970-01-01"))
                    .col(i32col(Horse::RaceToday))
                    .col(ColumnDef::new(Horse::BonusDay).date().not_null().default("1970-01-01"))
                    .col(ColumnDef::new(Horse::SeasonKey).string().not_null().default(""))
                    .col(i32col(Horse::SeasonWins))
                    .col(ColumnDef::new(Horse::Invested).big_integer().not_null().default(0))
                    .col(i32col(Horse::TrainTotal))
                    .col(i32col(Horse::AcqSeq))
                    .col(
                        ColumnDef::new(Horse::Elo)
                            .integer()
                            .not_null()
                            .default(crate::plugins::horse::consts::ELO_INIT),
                    )
                    .col(i32col(Horse::EloGames))
                    .col(i16col(Horse::DeskLv))
                    .col(i16col(Horse::PrepLv))
                    .col(ColumnDef::new(Horse::FatherId).big_integer().null())
                    .col(ColumnDef::new(Horse::MotherId).big_integer().null())
                    .col(
                        ColumnDef::new(Horse::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await?;
        // 列马厩按主人查;status 在内存里过滤,不进索引。
        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_horse_owner")
                    .table(Horse::Table)
                    .col(Horse::OwnerUin)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table(Table::drop().table(Horse::Table).if_exists().to_owned()).await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum HorseGacha {
    Table,
    Uin,
    StdPity,
    HorsePity,
}

pub struct GachaMigration;

impl MigrationName for GachaMigration {
    fn name(&self) -> &str {
        "m20260610_000004_create_horse_gacha"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for GachaMigration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(HorseGacha::Table)
                    .if_not_exists()
                    // 两池独立保底:std_pity(标准池)/ horse_pity(马池)。活库迁移:
                    //   ALTER TABLE horse_gacha RENAME COLUMN pity TO std_pity;
                    //   ALTER TABLE horse_gacha ADD COLUMN horse_pity int NOT NULL DEFAULT 0;
                    .col(ColumnDef::new(HorseGacha::Uin).big_integer().not_null().primary_key())
                    .col(ColumnDef::new(HorseGacha::StdPity).integer().not_null().default(0))
                    .col(ColumnDef::new(HorseGacha::HorsePity).integer().not_null().default(0))
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table(Table::drop().table(HorseGacha::Table).if_exists().to_owned()).await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum HorseAchievement {
    Table,
    Uin,
    Code,
    EarnedAt,
}

pub struct AchievementMigration;

impl MigrationName for AchievementMigration {
    fn name(&self) -> &str {
        "m20260610_000006_create_horse_achievement"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for AchievementMigration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(HorseAchievement::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(HorseAchievement::Uin).big_integer().not_null())
                    .col(ColumnDef::new(HorseAchievement::Code).integer().not_null())
                    .col(
                        ColumnDef::new(HorseAchievement::EarnedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .primary_key(Index::create().col(HorseAchievement::Uin).col(HorseAchievement::Code))
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table(Table::drop().table(HorseAchievement::Table).if_exists().to_owned()).await?;
        Ok(())
    }
}

// 经济改造新增表:各自唯一迁移名,别撞 000007。

#[derive(DeriveIden)]
enum HorsePlayerDaily {
    Table,
    Uin,
    Day,
    AccountRacesToday,
}

pub struct PlayerDailyMigration;

impl MigrationName for PlayerDailyMigration {
    fn name(&self) -> &str {
        "m20260701_000007_create_horse_player_daily"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for PlayerDailyMigration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(HorsePlayerDaily::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(HorsePlayerDaily::Uin).big_integer().not_null())
                    .col(ColumnDef::new(HorsePlayerDaily::Day).date().not_null())
                    .col(ColumnDef::new(HorsePlayerDaily::AccountRacesToday).integer().not_null().default(0))
                    .primary_key(Index::create().col(HorsePlayerDaily::Uin).col(HorsePlayerDaily::Day))
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table(Table::drop().table(HorsePlayerDaily::Table).if_exists().to_owned()).await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum HorsePlayerMeta {
    Table,
    Uin,
    TrainLv,
    StableLv,
    BloodLv,
    WarehouseLv,
    OwnerElo,
    OwnerEloGames,
    TaxSettledDay,
}

pub struct PlayerMetaMigration;

impl MigrationName for PlayerMetaMigration {
    fn name(&self) -> &str {
        "m20260701_000008_create_horse_player_meta"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for PlayerMetaMigration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let i16d = |c: HorsePlayerMeta| ColumnDef::new(c).small_integer().not_null().default(0).to_owned();
        manager
            .create_table(
                Table::create()
                    .table(HorsePlayerMeta::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(HorsePlayerMeta::Uin).big_integer().not_null().primary_key())
                    .col(i16d(HorsePlayerMeta::TrainLv))
                    .col(i16d(HorsePlayerMeta::StableLv))
                    .col(i16d(HorsePlayerMeta::BloodLv))
                    .col(i16d(HorsePlayerMeta::WarehouseLv))
                    .col(
                        ColumnDef::new(HorsePlayerMeta::OwnerElo)
                            .integer()
                            .not_null()
                            .default(crate::plugins::horse::consts::ELO_INIT),
                    )
                    .col(ColumnDef::new(HorsePlayerMeta::OwnerEloGames).integer().not_null().default(0))
                    .col(ColumnDef::new(HorsePlayerMeta::TaxSettledDay).date().not_null().default("1970-01-01"))
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table(Table::drop().table(HorsePlayerMeta::Table).if_exists().to_owned()).await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum HorseBloodlineLib {
    Table,
    Uin,
    HorseId,
    At,
}

pub struct BloodlineLibMigration;

impl MigrationName for BloodlineLibMigration {
    fn name(&self) -> &str {
        "m20260701_000009_create_horse_bloodline_lib"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for BloodlineLibMigration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(HorseBloodlineLib::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(HorseBloodlineLib::Uin).big_integer().not_null())
                    .col(ColumnDef::new(HorseBloodlineLib::HorseId).big_integer().not_null())
                    .col(
                        ColumnDef::new(HorseBloodlineLib::At)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .primary_key(Index::create().col(HorseBloodlineLib::Uin).col(HorseBloodlineLib::HorseId))
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table(Table::drop().table(HorseBloodlineLib::Table).if_exists().to_owned()).await?;
        Ok(())
    }
}
