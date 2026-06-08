//! abot —— 基于 `nagisa` 门面（onebot + log 特性）的 QQ 机器人。
//!
//! 启动流程：
//! 1. `dotenvy` 加载 `.env`（本地密钥/连接串），随后所有配置经环境变量读取。
//! 2. `nagisa::log::init` 装统一记录器（控制台 + `RUST_LOG` 按来源过滤），持有其 guard。
//! 3. [`Config::from_env`] 装配运行配置（DB DSN / OneBot WS / 超管清单）。
//! 4. 连 Postgres（sea-orm），把连接经 `App::data` 注入，供 `Db` 提取器取用。
//! 5. 挂 `nagisa::log::EventLog` 顶层观察者（可读事件日志），注入主人句柄（供上线通知插件
//!    经 `State<Master>` 取用），跑 OneBot 直到 Ctrl-C。
//!
//! 命令/事件（如 `ping`→`pong`、`online` 插件的 `#[event(Ready)]` 上线通知）经 `#[command]`
//! / `#[event]` + `inventory` 自动注册，`plugins` 模块的 `use` 保证其编译单元被链接器收录。

use abot::config::{Config, Master};
use abot::data::migration::Migrator;
use sea_orm_migration::MigratorTrait;
// `plugins` 必须被引用，否则链接器可能丢弃其编译单元，连同 #[command] 的
// inventory 注册项一起消失（命令将静默不挂载）。glob-use 即足以保活。
#[allow(unused_imports)]
use abot::plugins::*;
use nagisa::prelude::*;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 1. 加载 .env（缺失不报错——活体环境也可能直接由 shell 注入变量）。
    dotenvy::dotenv().ok();

    // 2. 统一记录器：控制台 + RUST_LOG 来源过滤。guard 必须活到进程退出
    //    （配置了文件层时它持有后台写线程；此处默认无文件，但仍保持持有约定）。
    // 默认过滤：info,但把 sqlx 的「每条 SQL 一行」噪声压到 warn(迁移的「Applying migration」走
    // 另一来源 sea_orm_migration,仍 info 可见)。`RUST_LOG` 存在时以它为准、覆盖此默认。
    let _log_guard = nagisa::log::init(nagisa::log::LogConfig {
        filter: "info,sqlx=warn".to_string(),
        ..nagisa::log::LogConfig::default()
    });

    // 3. 装配配置。
    let cfg = Config::from_env();
    tracing::info!(
        onebot_ws = %cfg.onebot_ws,
        superusers = cfg.superusers.len(),
        "abot 启动中…"
    );

    // 4. 连 Postgres（sea-orm）并跑迁移（幂等，记在 seaql_migrations）。连接句柄经
    //    App::data 注入，供 Db 提取器克隆取用。
    let db = sea_orm::Database::connect(&cfg.database_url).await?;
    Migrator::up(&db, None).await?;
    tracing::info!("已连接数据库并应用迁移");

    // 5. 组装并运行：超管集、注入主人句柄（上线通知插件经 State<Master> 取用，并据此在
    //    框架的 Ready 事件里给主人发上线通知）、顶层可读事件日志、OneBot 正向 WS、Ctrl-C。
    App::new()
        .data(db)
        .data(Master(cfg.master))
        .superusers(cfg.superusers.clone())
        .on_top(nagisa::log::EventLog::new().observer())
        .run_onebot(OneBotConfig::new(&cfg.onebot_ws), nagisa::ctrl_c_shutdown())
        .await?;

    Ok(())
}
