//! sea-orm 迁移 —— 建**核心**三张表（`user` / `group` / `coin_log`），并暴露
//! [`Migrator`]；插件各自的表经 [`PluginMigration`] 自注册接入，核心**不**感知任何
//! 具体插件。
//!
//! `main` 连库后立刻 `Migrator::up(&db, None).await?`：已应用的迁移记在
//! `seaql_migrations` 里，重复运行是幂等的（不会重建已存在的表）。建表本身也用
//! `if_not_exists` 兜底，确保即便迁移追踪表被外部清掉也不致硬报错。
//!
//! 列的物理定义须与 [`entity`](crate::data::entity) 的字段一一对齐：类型、缺省、主键。
//! 缺省值在库侧给（`coin` 默认 10、`exp` 默认 0、时间戳 `now()`、`group.config` 为
//! `{}`），故新行 `insert` 不必逐字段填。
//!
//! # 「插件自有数据」约定
//!
//! 核心表只放**真正跨插件共享**的状态（用户金币/经验/封禁、群配置、金币流水）。任何
//! 插件私有的状态都归该插件**自己**的表，其建表迁移经 [`PluginMigration`] +
//! `nagisa::inventory` 自注册（与 `#[command]` 同款机制）。[`Migrator::migrations`] 把
//! 核心迁移与所有自注册的插件迁移拼成一支有序列表（核心在前、插件在后），核心代码
//! 始终**不引用**任何具体插件。

use sea_orm_migration::prelude::*;

/// 一支**插件自有**迁移的自注册槽位：包一个返回该插件 [`MigrationTrait`] 的构造函数。
///
/// 插件在自己的模块里 `nagisa::inventory::submit!{ PluginMigration(|| Box::new(..)) }`
/// 即把建表迁移登记进进程级集合；[`Migrator::migrations`] 经 `nagisa::inventory::iter`
/// 收集，故核心 `Migrator` 无需 `use` 任何插件即可应用其迁移（与 `#[command]` 的
/// `inventory` 收集同一机制）。函数指针是 `const`，可直接作为 `inventory` 项提交。
pub struct PluginMigration(pub fn() -> Box<dyn MigrationTrait>);
nagisa::inventory::collect!(PluginMigration);

/// 迁移器：登记**核心**迁移，并拼接所有自注册的 [`PluginMigration`]，供 `Migrator::up`
/// / `::down` 驱动。
pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        // 核心迁移在前（建共享表），随后追加每个插件自注册的迁移。inventory 收集顺序不保证,
        // 故插件迁移按其带日期序号的唯一名排序——同一插件先建表后改表的相对次序才稳定。
        let mut plugins: Vec<Box<dyn MigrationTrait>> =
            nagisa::inventory::iter::<PluginMigration>.into_iter().map(|p| (p.0)()).collect();
        plugins.sort_by(|a, b| a.name().cmp(b.name()));
        let mut all: Vec<Box<dyn MigrationTrait>> =
            vec![Box::new(m20260610_000001_create_core::Migration)];
        all.extend(plugins);
        all
    }
}

/// 唯一一支核心迁移：一次性建三张基础表。
///
/// 迁移名统一 `m20260610_0000NN_create_*` 序列(2026-06-10 重置基线:库从零重建,
/// 每张表一支干净建表迁移,不留「建表后追补改表」的历史层叠;核心 01,插件按序后排)。
mod m20260610_000001_create_core {
    use super::*;

    /// `user` 表的列标识（**仅**跨插件共享字段；签到等插件私有列不在此表）。
    #[derive(DeriveIden)]
    enum User {
        Table,
        Uin,
        Coin,
        Nickname,
        Exp,
        Banned,
        JoinTime,
    }

    /// `group` 表的列标识。
    #[derive(DeriveIden)]
    enum Group {
        Table,
        Gid,
        Config,
        CreatedAt,
    }

    /// `coin_log` 表的列标识。
    #[derive(DeriveIden)]
    enum CoinLog {
        Table,
        Id,
        Uin,
        Delta,
        Reason,
        At,
    }

    /// 这支迁移。迁移名带日期序号前缀（便于将来追加迁移时天然排序），记进
    /// `seaql_migrations` 作为已应用标记。
    pub struct Migration;

    impl MigrationName for Migration {
        fn name(&self) -> &str {
            "m20260610_000001_create_core"
        }
    }

    #[async_trait::async_trait]
    impl MigrationTrait for Migration {
        async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
            // user：主键 uin（非自增），coin 默认 10，exp 默认 0，banned 默认 false，
            // join_time 默认 now()。nickname 可空。**只**含跨插件共享字段——签到等插件
            // 私有列归各插件自己的表（见 plugins::sign）。
            manager
                .create_table(
                    Table::create()
                        .table(User::Table)
                        .if_not_exists()
                        .col(
                            ColumnDef::new(User::Uin)
                                .big_integer()
                                .not_null()
                                .primary_key(),
                        )
                        .col(
                            ColumnDef::new(User::Coin)
                                .big_integer()
                                .not_null()
                                .default(10),
                        )
                        .col(ColumnDef::new(User::Nickname).string().null())
                        .col(ColumnDef::new(User::Exp).big_integer().not_null().default(0))
                        .col(
                            ColumnDef::new(User::Banned)
                                .boolean()
                                .not_null()
                                .default(false),
                        )
                        .col(
                            ColumnDef::new(User::JoinTime)
                                .timestamp_with_time_zone()
                                .not_null()
                                .default(Expr::current_timestamp()),
                        )
                        .to_owned(),
                )
                .await?;

            // group：主键 gid（非自增），config 为 jsonb 默认 {}，created_at 默认 now()。
            manager
                .create_table(
                    Table::create()
                        .table(Group::Table)
                        .if_not_exists()
                        .col(
                            ColumnDef::new(Group::Gid)
                                .big_integer()
                                .not_null()
                                .primary_key(),
                        )
                        .col(
                            ColumnDef::new(Group::Config)
                                .json_binary()
                                .not_null()
                                .default(Expr::cust("'{}'::jsonb")),
                        )
                        .col(
                            ColumnDef::new(Group::CreatedAt)
                                .timestamp_with_time_zone()
                                .not_null()
                                .default(Expr::current_timestamp()),
                        )
                        .to_owned(),
                )
                .await?;

            // coin_log：自增主键 id（BIGSERIAL），uin/delta 带符号 i64，reason 文本，
            // at 默认 now()。追加式，无外键。
            manager
                .create_table(
                    Table::create()
                        .table(CoinLog::Table)
                        .if_not_exists()
                        .col(
                            ColumnDef::new(CoinLog::Id)
                                .big_integer()
                                .not_null()
                                .auto_increment()
                                .primary_key(),
                        )
                        .col(ColumnDef::new(CoinLog::Uin).big_integer().not_null())
                        .col(ColumnDef::new(CoinLog::Delta).big_integer().not_null())
                        .col(ColumnDef::new(CoinLog::Reason).string().not_null())
                        .col(
                            ColumnDef::new(CoinLog::At)
                                .timestamp_with_time_zone()
                                .not_null()
                                .default(Expr::current_timestamp()),
                        )
                        .to_owned(),
                )
                .await?;

            Ok(())
        }

        async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
            // 逆序删表（先删依赖侧——这里无 FK，顺序仅为对称）。
            manager
                .drop_table(Table::drop().table(CoinLog::Table).if_exists().to_owned())
                .await?;
            manager
                .drop_table(Table::drop().table(Group::Table).if_exists().to_owned())
                .await?;
            manager
                .drop_table(Table::drop().table(User::Table).if_exists().to_owned())
                .await?;
            Ok(())
        }
    }
}
