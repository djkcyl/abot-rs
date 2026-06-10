//! abot —— 基于 `nagisa` 门面（onebot + log 特性）的 QQ 机器人。
//!
//! 启动流程：
//! 1. `dotenvy` 加载 `.env`（本地密钥/连接串），随后所有配置经环境变量读取。
//! 2. `nagisa::log::init` 装统一记录器（控制台 + `RUST_LOG` 按来源过滤），持有其 guard。
//! 3. [`Config::from_env`] 装配运行配置（DB DSN / OneBot WS / 超管清单 / WEB_BIND）。
//! 4. 连 Postgres（sea-orm），把连接经 `App::data` 注入，供 `Db` 提取器取用；同时经
//!    `App::service_data` 注入，供 `ConsoleService` 的 `prepare` 经 `ServiceBus` 取用。
//! 5. 注册可选的 web 控制台服务（`ConsoleService`）；挂 `nagisa::log::EventLog` 顶层观察者
//!    （可读事件日志），注入主人句柄（供上线通知插件经 `State<Master>` 取用），跑 OneBot 直到 Ctrl-C。
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
    // 进程启动时刻：注入控制台供总览算在线时长。
    let boot = chrono::Utc::now();

    // 1. 加载 .env（缺失不报错——活体环境也可能直接由 shell 注入变量）。
    dotenvy::dotenv().ok();

    // 2. 统一记录器：控制台 + RUST_LOG 来源过滤。guard 必须活到进程退出
    //    （配置了文件层时它持有后台写线程；此处默认无文件，但仍保持持有约定）。
    // 默认过滤：info,但把 sqlx 的「每条 SQL 一行」噪声压到 warn(迁移的「Applying migration」走
    // 另一来源 sea_orm_migration,仍 info 可见)。`RUST_LOG` 存在时以它为准、覆盖此默认。
    // 开日志总线:控制台 WebUI 的「日志」页订阅它做实时尾随。bus 为 None(理论上不会发生,
    // 此处 bus: true)时该功能静默降级,不 panic。
    let (_log_guard, log_bus) = nagisa::log::init(nagisa::log::LogConfig {
        filter: "info,sqlx=warn".to_string(),
        bus: true,
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

    // 顶层图片缓存服务:建目录、拉起下载队列、重排残留 pending。
    // 插件经 abot::media(scan/ingest/wait/resolve)用图,自己不下载。
    abot::media::init(db.clone()).await?;

    // 装一个出站消息日志器：bot 发出的每条消息也落 chat_log（凑成双向会话历史）。与
    // nagisa-log 自己装的那个并存（多订阅）。落库在独立任务里跑，绝不阻塞发送。
    let db_for_log = db.clone();
    nagisa::add_outgoing_logger(Box::new(move |peer, segs, self_id, id| {
        let db = db_for_log.clone();
        let (peer, segs, id) = (*peer, segs.to_vec(), id.clone());
        tokio::spawn(async move {
            abot::plugins::chatlog::record_outgoing(&db, &peer, &segs, self_id, &id).await;
        });
    }));
    let config_store = abot::web::config::ConfigStore::load(&db).await?;
    // 装载持久化的插件开关覆盖:setting 行 ("__switches__","enabled") 的 value(jsonb)。
    // 缺行或反序列化失败都退回空覆盖(全按 default_enable)。
    let switches = abot::web::switches::load_overrides(&db).await;

    // 5. 组装并运行：超管集、注入主人句柄（上线通知插件经 State<Master> 取用，并据此在
    //    框架的 Ready 事件里给主人发上线通知）、可选 web 控制台（绑不上端口只记 warn、
    //    不拖垮 bot）、顶层可读事件日志、OneBot 正向 WS、Ctrl-C。
    let login_gate = abot::web::auth::LoginGate::new();
    let authority = abot::web::auth::AuthorityResolver::new(
        cfg.master,
        cfg.superusers.iter().copied().collect(),
    );
    let mut app = App::new()
        .restore_switches(switches) // 装载持久化的插件开关覆盖
        .data(db.clone())
        .service_data(db.clone())
        .data(Master(cfg.master))
        .data(config_store.clone())         // 插件经 State<ConfigStore> 读
        .service_data(config_store.clone()) // 控制台 ConfigProvider 用
        .data(login_gate.clone())           // 登录命令经 State<LoginGate> 取
        .service_data(login_gate.clone())   // 控制台 prepare 经 bus 取(同一份)
        .service_data(authority.clone())    // 控制台权限解析
        .service_data(abot::web::BootTime(boot)) // 控制台总览算在线时长
        .superusers(cfg.superusers.clone())
        .service_optional(abot::web::ConsoleService::new(cfg.web_bind))
        .on_top(nagisa::log::EventLog::new().observer());
    // 把日志总线交给控制台:有则注入,无则跳过(日志页自动空着)。
    if let Some(bus) = log_bus {
        app = app.service_data(bus);
    }
    // 把 router 据以门控的同一个 EnabledSet 句柄交给控制台:插件页据它读/改开关。
    // enabled_handle 借 &self、service_data 取走 self,故先取句柄再注入。
    let enabled = app.enabled_handle();
    app = app.service_data(enabled.clone());
    // 也作为 handler 数据注入:命令经 State<Arc<EnabledSet>> 读开关(help 给已停用的命令标注)。
    app = app.data(enabled);
    app.run_onebot(OneBotConfig::new(&cfg.onebot_ws), nagisa::ctrl_c_shutdown())
        .await?;

    Ok(())
}
