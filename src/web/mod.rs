//! 插件 WebUI 地基 —— 进程内 axum 控制台。
//!
//! [`ConsoleService`] 是一个注册为**可选**的 [`nagisa::Service`]:`prepare`
//! 绑端口并收集 `inventory` 的 console 插件建 [`hub::Hub`],`run` 跑 axum(HTTP 静态 +
//! 单条 WS),`cleanup` 由 shutdown 触发、在 `run` 内完成收束。绑不上端口只记日志,不拖垮 bot 主体。
//!
//! 数据交互两条原语(抄 Koishi):[`registry::WebDataService`] (被动推)与
//! [`registry::WebListener`] (主动 RPC)。内置 Provider 包括 plugins、review、config、database、logs,
//! 鉴权由连接携带的 `?token=` 解析真实 [`registry::AuthUser`],无/失效 token 为 authority 0。
//!
//! 实时日志:`main` 开 `LogBus` 经 `service_data` 注入,`run` 里的日志泵订阅日志、维护环形缓冲,
//! 经 [`hub::Hub::broadcast_patch`] 批量增量推给有权连接。

pub mod auth;
pub mod config;
pub mod protocol;
pub mod registry;
pub mod hub;
pub mod embed;
pub mod migration;
pub mod entity;
pub mod providers;
pub mod media;
pub mod review;
pub mod switches;

use crate::web::embed::static_handler;
use crate::web::hub::{ws_handler, Hub};
use crate::web::registry::{ConsoleContext, ConsolePluginCtor, ConsoleRegistry};
use axum::extract::{Path, Query};
use axum::http::{header, StatusCode};
use axum::response::IntoResponse;
use axum::routing::any;
use axum::routing::get;
use axum::Router;
use nagisa::log::LogBus;
use nagisa::prelude::*; // Service, ServiceBus, ShutdownToken, Error, Result, async_trait
use sea_orm::DatabaseConnection;
use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::Mutex;

/// 日志泵一次最多攒多久就 flush 一批(批量推,降帧)。
const LOG_FLUSH_INTERVAL: std::time::Duration = std::time::Duration::from_millis(200);
/// 日志环形缓冲上限。超出从头丢弃。
const LOG_BUF_CAP: usize = 500;

/// 最近日志的共享环形缓冲(JSON 行)。日志泵填充、LogsProvider 回填,共享同一个 `Arc`。
pub type LogBuf = Arc<Mutex<VecDeque<serde_json::Value>>>;

/// 进程启动时刻。`main` 经 `service_data` 注入,控制台 `prepare` 取出放进 [`ConsoleContext`]。
pub struct BootTime(pub chrono::DateTime<chrono::Utc>);

/// 插件 WebUI 的承载服务。注册为**可选**服务:绑端口失败只记日志,不拖垮 bot 主体。
///
/// `prepare`:从 `ServiceBus` 取共享 `Db`(`main` 经 `App::service_data(db)` 预置)、收集
/// `inventory` 的 console 插件建 [`Hub`]、绑端口;`run`:跑 axum(静态 + 单条 WS);
/// shutdown 触发的收束在 `run` 内完成。
pub struct ConsoleService {
    bind: SocketAddr,
    listener: Mutex<Option<TcpListener>>,
    hub: Mutex<Option<Arc<Hub>>>,
    /// `prepare` 暂存、`run` 取走:日志总线(可能为 None,无则不开日志泵)与共享日志缓冲。
    log: Mutex<Option<(Option<LogBus>, LogBuf)>>,
}

impl ConsoleService {
    /// 建一个监听 `bind` 的控制台服务。
    pub fn new(bind: SocketAddr) -> Arc<Self> {
        Arc::new(Self {
            bind,
            listener: Mutex::new(None),
            hub: Mutex::new(None),
            log: Mutex::new(None),
        })
    }
}

#[async_trait]
impl Service for ConsoleService {
    fn id(&self) -> &'static str {
        "web-console"
    }

    async fn prepare(&self, bus: &ServiceBus) -> nagisa::Result<()> {
        // 取 main 经 App::service_data 预置的共享数据库连接。
        let db = bus.get::<DatabaseConnection>().ok_or_else(|| {
            nagisa::Error::action(
                "web-console: ServiceBus 缺少 DatabaseConnection",
            )
        })?;
        let bot = bus
            .get::<nagisa::Bot>()
            .map(|b| (*b).clone())
            .ok_or_else(|| nagisa::Error::action("web-console: ServiceBus 缺少 Bot"))?;
        let config = bus
            .get::<crate::web::config::ConfigStore>()
            .map(|c| (*c).clone())
            .ok_or_else(|| nagisa::Error::action("web-console: ServiceBus 缺少 ConfigStore"))?;
        // 日志缓冲:LogsProvider(回填)与日志泵(填充)共享同一个 Arc。
        let log_buf = Arc::new(Mutex::new(VecDeque::new()));
        // 启动时刻:main 经 service_data 注入,缺则退回当下。
        let boot = bus.get::<BootTime>().map(|b| b.0).unwrap_or_else(chrono::Utc::now);
        // 插件开关表:main 经 service_data 注入 router 同款句柄(缺则报错,main 总会注入)。
        let enabled = bus
            .get::<std::sync::Arc<nagisa::EnabledSet>>()
            .map(|e| (*e).clone())
            .ok_or_else(|| nagisa::Error::action("web-console: ServiceBus 缺少 EnabledSet"))?;
        let cx = ConsoleContext { db: (*db).clone(), bot, config, log_buf: log_buf.clone(), boot, enabled };
        // 日志总线可缺(main 未开 bus 时为 None):缺则日志泵不开,日志页空着,不报错。
        let log_bus = bus.get::<LogBus>().map(|b| (*b).clone());

        // 收集所有 console 插件 → 注册表 → Hub。
        let mut reg = ConsoleRegistry::default();
        for ctor in nagisa::inventory::iter::<ConsolePluginCtor> {
            (ctor.0)(&cx).register(&mut reg);
        }
        // 从 ServiceBus 取 main 注入的真实鉴权句柄。
        let login_gate = bus
            .get::<crate::web::auth::LoginGate>()
            .map(|g| (*g).clone())
            .ok_or_else(|| nagisa::Error::action("web-console: ServiceBus 缺少 LoginGate"))?;
        let authority = bus
            .get::<crate::web::auth::AuthorityResolver>()
            .map(|a| (*a).clone())
            .ok_or_else(|| nagisa::Error::action("web-console: ServiceBus 缺少 AuthorityResolver"))?;
        let hub = Hub::new(reg, cx.db.clone(), login_gate, authority);

        // 绑端口。失败 → Err;因本服务可选,Supervisor 只记 warn、不拖垮 bot。
        let listener = TcpListener::bind(self.bind)
            .await
            .map_err(|e| nagisa::Error::action(format!("web-console: 绑定 {} 失败: {e}", self.bind)))?;
        tracing::info!(addr = %self.bind, "web 控制台已就绪");

        *self.listener.lock().await = Some(listener);
        *self.hub.lock().await = Some(hub);
        *self.log.lock().await = Some((log_bus, log_buf));
        Ok(())
    }

    async fn run(self: Arc<Self>, _bus: ServiceBus, shutdown: ShutdownToken) -> nagisa::Result<()> {
        let listener = self
            .listener
            .lock()
            .await
            .take()
            .ok_or_else(|| nagisa::Error::action("web-console: prepare 未建 listener"))?;
        let hub = self
            .hub
            .lock()
            .await
            .take()
            .ok_or_else(|| nagisa::Error::action("web-console: prepare 未建 hub"))?;

        // 日志泵:有总线才开。随 shutdown 一并收束(token 可 clone,各自取一份)。
        if let Some((Some(log_bus), log_buf)) = self.log.lock().await.take() {
            let pump_hub = hub.clone();
            let pump_shutdown = shutdown.clone();
            tokio::spawn(log_pump(log_bus, log_buf, pump_hub, pump_shutdown));
        }

        let app = Router::new()
            .route("/api/ws", any(ws_handler))
            .route("/api/login/challenge", axum::routing::post(crate::web::auth::login_challenge))
            .route("/api/login/poll", axum::routing::post(crate::web::auth::login_poll))
            .route("/api/media/{name}", get(serve_media))
            .fallback(static_handler)
            .with_state(hub);

        axum::serve(listener, app)
            .with_graceful_shutdown(async move { shutdown.cancelled().await })
            .await
            .map_err(|e| nagisa::Error::action(format!("web-console serve 出错: {e}")))?;
        Ok(())
    }
}

/// 静态图片路由 `GET /api/media/{name}?sig=<签名>` —— 把落盘在 `IMAGE_DIR` 下的瓶子/聊天图片
/// 按文件名读出来回给前端,供审核页直接 `<img>` 显示。**不鉴权**(图片标签无从带 token),改用
/// URL 签名挡住对目录的枚举/伪造:签名由审核详情用进程级密钥生成(见 [`media`](crate::web::media)),
/// 签名缺失或不匹配一律 404。
///
/// 防目录穿越:文件名为空、含 `/`、`\`、`..` 一律 404。签名**有效**但文件读不出(被清理/
/// 盘损)回一张占位图(200,随机渐变底 + 「图片已失效」 + md5,与捞瓶占位同款):审核页上
/// 让人知道这里有图、只是失效了,而不是裂图标。无效签名仍 404,不暴露存在性。
/// `/api/media/{name}` 的查询串：图片访问签名。
#[derive(serde::Deserialize)]
struct MediaQuery {
    sig: Option<String>,
}

async fn serve_media(
    Path(name): Path<String>,
    Query(q): Query<MediaQuery>,
) -> impl IntoResponse {
    if name.is_empty() || name.contains('/') || name.contains('\\') || name.contains("..") {
        return StatusCode::NOT_FOUND.into_response();
    }
    // 签名校验:缺签名或不匹配进程密钥 → 404(不暴露文件是否存在)。
    if !q.sig.as_deref().is_some_and(|s| crate::web::media::verify(&name, s)) {
        return StatusCode::NOT_FOUND.into_response();
    }
    // 路径解析交给顶层媒体服务:文件名 → 分片归档路径。
    let path = crate::media::resolve(&name);
    let Ok(bytes) = tokio::fs::read(&path).await else {
        return match crate::media::placeholder::missing_image_webp(&name) {
            Ok(webp) => ([(header::CONTENT_TYPE, "image/webp")], webp).into_response(),
            Err(_) => StatusCode::NOT_FOUND.into_response(),
        };
    };
    // 盘上文件无后缀(内容寻址),Content-Type 按字节魔数嗅探,认不出回 octet-stream。
    let ct = crate::media::sniff_image_ct(&bytes).unwrap_or("application/octet-stream");
    tokio::spawn(crate::media::touch_used(name)); // 取图即「使用」,刷 last_used
    ([(header::CONTENT_TYPE, ct)], bytes).into_response()
}

/// 日志泵:订阅日志总线,把记录渲染成紧凑 JSON 行,推进共享环形缓冲,并每 ~200ms 批量
/// `broadcast_patch("logs", …, 4)` 给有权连接。
///
/// 收束:`shutdown` 触发或总线 `Closed` 时退出;`Lagged`(消费过慢被覆盖)跳过那批继续。
///
/// 防回环:本任务内**绝不**打 info+ 级别日志(会回灌总线、自我放大);确需报错时只用以
/// `nagisa::log` 开头的 target(如 `nagisa::log::webui`),总线会按保留前缀过滤掉。保持静默。
async fn log_pump(bus: LogBus, buf: LogBuf, hub: Arc<Hub>, shutdown: ShutdownToken) {
    use tokio::sync::broadcast::error::RecvError;

    let mut rx = bus.subscribe();
    let mut flush = tokio::time::interval(LOG_FLUSH_INTERVAL);
    flush.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    // 自上次 flush 起新攒的行,到点一次性推走。
    let mut batch: Vec<serde_json::Value> = Vec::new();

    loop {
        tokio::select! {
            biased;
            () = shutdown.cancelled() => break,
            rec = rx.recv() => match rec {
                Ok(record) => {
                    let line = render_record(&record);
                    {
                        let mut b = buf.lock().await;
                        b.push_back(line.clone());
                        while b.len() > LOG_BUF_CAP {
                            b.pop_front();
                        }
                    }
                    batch.push(line);
                }
                // 消费过慢被环形缓冲覆盖:跳过丢失的那批,继续(不死)。
                Err(RecvError::Lagged(_)) => continue,
                // 发送端全 drop:不会再有新记录,退出。
                Err(RecvError::Closed) => break,
            },
            _ = flush.tick() => {
                if !batch.is_empty() {
                    let lines = std::mem::take(&mut batch);
                    hub.broadcast_patch("logs", serde_json::json!(lines), 4).await;
                }
            }
        }
    }
    // 退出前不强制 flush:停机/总线关闭后没有连接需要这批增量。
}

/// 把一条 [`LogRecord`](nagisa::log::LogRecord) 渲染成前端要的紧凑 JSON 行。
fn render_record(record: &nagisa::log::LogRecord) -> serde_json::Value {
    let ts = record
        .timestamp
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    serde_json::json!({
        "ts": ts,
        "level": record.level.as_str(),
        "target": record.source,
        "msg": record.message,
    })
}
