//! 赛马插件建表迁移,经 [`PluginMigration`] 自注册进核心 [`Migrator`](crate::data::migration::Migrator)。
//! 改表直接改这支迁移 + 活库手动 ALTER(或重置),不另起迁移。
//!
//! 五维当前值(`spd`/`sta`/`brs`/`agi`/`luk`)落库为「厘点」= 点 × [`STAT_SCALE`](super::consts::STAT_SCALE);
//! 活库迁移须 `UPDATE horse SET spd=spd*100, sta=sta*100, brs=brs*100, agi=agi*100, luk=luk*100`
//! 再 `ADD COLUMN train_total`(否则旧马五维被当成零点几、且缺列)。

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
    Pity,
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
                    .col(ColumnDef::new(HorseGacha::Uin).big_integer().not_null().primary_key())
                    .col(ColumnDef::new(HorseGacha::Pity).integer().not_null().default(0))
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
