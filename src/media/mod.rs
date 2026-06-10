//! 顶层图片缓存服务 —— 全 bot 共享的「收图 → 登记 → 排队下载 → 分片归档」一条线。
//!
//! 入口在 chatlog 顶层记录器:每条消息里的图片经 [`scan`] 挑出、[`ingest`] 先落一行
//! `media_file`(pending)再进下载队列;后台按队列限并发拉取,落盘到 `IMAGE_DIR`
//! (默认 `./data/images`)下按 md5 分片的目录。
//!
//! **归档内容寻址、盘上无后缀**:落盘文件名就是内容 md5(小写 32 位 hex,无扩展名),
//! `a1b2…` 存 `a1/b2/a1b2…`;同一张图全 bot 只一份、只下一次,「文件名 = 内容 md5」是
//! 归档不变量([`verify_file`] 据此校验)。wire 文件名只当下载前去重的提示(其 md5 主体
//! 实测即内容 md5,但**发端可控**,不合形即弃);后缀与真实格式是 `media_file` 里的元数据
//! (`claimed_ext` 发端报的、会谎;`format` 按下载字节魔数嗅探)。要后缀/格式一律查库,
//! 不从文件名猜。
//!
//! 其他插件**不自己下载**:对要用的图调 [`ingest`] + [`wait`] —— 已在盘上立即返回路径;
//! 还在队列/下载中则阻塞到完成(或超时/失败返回错误);要重发本地图用 [`resolve`] 取路径。
//! 服务须在迁移后由 `main` 经 [`init`] 装起:建目录、拉起队列、把上次进程残留的
//! pending 重新排队。

pub mod entity;
pub mod migration;
pub mod placeholder;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use anyhow::bail;
use nagisa::prelude::*;
use sea_orm::sea_query::{Expr, OnConflict};
use sea_orm::ActiveValue::Set;
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};
use tokio::sync::{broadcast, mpsc, Semaphore};

use self::entity as media_file;

/// 同时进行的下载任务上限。
const DOWNLOAD_CONCURRENCY: usize = 4;

/// 一处待取的图片来源:URL + 可选的提示(wire 名拆出的 md5 主体与后缀)。
pub struct MediaRef {
    url: String,
    /// wire 名主体给出的内容 md5 提示(下载前去重用;实测与内容相符,下载后仍以字节为准)。
    md5: Option<String>,
    /// 发端报的后缀(只进库当元数据,不进文件名)。
    claimed_ext: Option<String>,
}

impl MediaRef {
    /// 从已知 URL(可选 wire 文件名提示)构造来源 —— 给不经消息段拿图的调用方(头像等)用;
    /// 消息里的图一律走 [`scan`]。提示不合归档形(md5 主体 + 短扩展名)即整体弃之当无名
    /// (与 [`scan`] 同一道闸,提示进路径/进库、不收任意串)。
    pub fn new(url: impl Into<String>, name_hint: Option<String>) -> Self {
        let (md5, claimed_ext) = match name_hint.as_deref().and_then(parse_hint) {
            Some((m, e)) => (Some(m), Some(e)),
            None => (None, None),
        };
        Self { url: url.into(), md5, claimed_ext }
    }
}

/// 一张已落盘的图片:内容 md5(各插件存它)+ 盘上路径(读字节/重发用)。
#[derive(Clone, Debug)]
pub struct Stored {
    /// 内容 md5(`media_file.md5`,插件持久化用它、不存路径不存后缀)。
    pub md5: String,
    /// 分片目录下的完整路径(无后缀)。
    pub path: PathBuf,
}

/// 一桩下载的完成广播:成功带落盘结果(无名来源此时才知道真 md5),失败带原因。
type Outcome = std::result::Result<Stored, String>;

/// 队列里的一桩下载。
struct Job {
    /// 排队凭据 = 登记行主键(有提示即其 md5,无名为 `u<md5(url)>` 临时键)。
    ticket: String,
    url: String,
    /// wire 提示的内容 md5;`None` 则纯按下载字节定名。
    md5: Option<String>,
}

/// 服务本体:库连接 + 队列发送端 + 在途任务表(凭据 → 完成广播)。
struct MediaService {
    db: DatabaseConnection,
    tx: mpsc::UnboundedSender<Job>,
    inflight: Mutex<HashMap<String, broadcast::Sender<Outcome>>>,
}

static SERVICE: OnceLock<MediaService> = OnceLock::new();

/// 取进程级服务;[`init`] 之前调用是装配错误,直接 panic。
fn service() -> &'static MediaService {
    SERVICE.get().expect("media::init 未调用 —— main 须在迁移后初始化媒体服务")
}

/// 装起媒体服务:建目录、拉起下载队列、重排上次残留的 pending。
///
/// 须在 `Migrator::up` 之后调用(要写 `media_file` 表),进程内只一次。
pub async fn init(db: DatabaseConnection) -> anyhow::Result<()> {
    tokio::fs::create_dir_all(image_dir()).await?;

    let (tx, rx) = mpsc::unbounded_channel();
    if SERVICE.set(MediaService { db, tx, inflight: Mutex::new(HashMap::new()) }).is_err() {
        bail!("media::init 重复调用");
    }
    tokio::spawn(dispatch(rx));

    // 崩溃恢复:上次进程退出时还挂着的 pending 重新排队(来源 URL 可能已过期,失败会记 failed)。
    let stale = media_file::Entity::find()
        .filter(media_file::Column::Status.eq("pending"))
        .all(&service().db)
        .await?;
    if !stale.is_empty() {
        tracing::info!(count = stale.len(), "重新排队上次未完成的图片下载");
        for row in stale {
            let hint = is_md5(&row.md5).then(|| row.md5.clone());
            enqueue(row.md5, row.url, hint);
        }
    }
    Ok(())
}

// ———— 路径:分片归档 ————

/// 图片落盘根目录(环境变量 `IMAGE_DIR`,默认 `./data/images`);读一次缓存。
pub fn image_dir() -> &'static Path {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        std::env::var("IMAGE_DIR").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("./data/images"))
    })
    .as_path()
}

/// md5 的分片子目录:前 4 个字符切两级(`a1b2…` → `a1/b2`);不合形的归 `misc`
/// (正常写盘名全是 32 位 hex,此分支只为任意输入兜底)。
fn shard_rel(md5: &str) -> PathBuf {
    let b = md5.as_bytes();
    if b.len() >= 4 && b[..4].iter().all(|c| c.is_ascii_alphanumeric()) {
        PathBuf::from(&md5[0..2]).join(&md5[2..4])
    } else {
        PathBuf::from("misc")
    }
}

/// md5 在分片归档下的规范路径(不查盘)。
pub fn shard_path(md5: &str) -> PathBuf {
    image_dir().join(shard_rel(md5)).join(md5)
}

/// 在盘上找这份文件(分片路径);没有为 `None`。
pub fn locate(md5: &str) -> Option<PathBuf> {
    let sharded = shard_path(md5);
    sharded.exists().then_some(sharded)
}

/// 取这份 md5 的读取路径(即分片规范路径)。重发本地图、Web 媒体路由都用它。
pub fn resolve(md5: &str) -> PathBuf {
    shard_path(md5)
}

// ———— 对外:扫描 / 排队 / 等待 ————

/// 从消息段里挑出所有可下载的图片来源(只取 http(s) 的;本地 path 形态跳过)。纯函数。
pub fn scan(content: &[Segment]) -> Vec<MediaRef> {
    content
        .iter()
        .filter_map(|seg| match seg {
            Segment::Image { res, .. } => {
                let recv = res.recv.as_ref()?;
                let url = recv.url.clone().or_else(|| recv.id.clone())?;
                if !url.starts_with("http") {
                    return None; // 本地 path / 非网络来源 → 不下载
                }
                let hint = recv.raw.get("filename").and_then(|v| v.as_str()).map(normalize_name);
                Some(MediaRef::new(url, hint))
            }
            _ => None,
        })
        .collect()
}

/// 把一批图片来源登记进 `media_file` 并排进下载队列,返回各自的排队凭据(供 [`wait`])。
///
/// 每处来源都记一次「遇见」(`seen_count`+1、`last_seen` 刷新、URL/后缀更新)。去重:
/// 同 md5 已在下载中 → 共用同一桩任务;已在盘上 → 不排队;登记过但失败/文件丢了 →
/// 用新 URL 重置回 pending 再下。登记失败只记 warn,不挡排队。
pub async fn ingest(refs: Vec<MediaRef>) -> Vec<String> {
    let svc = service();
    let mut tickets = Vec::with_capacity(refs.len());
    for r in refs {
        let ticket =
            r.md5.clone().unwrap_or_else(|| format!("u{:x}", md5::compute(r.url.as_bytes())));
        tickets.push(ticket.clone());

        // 已在盘上 → 记一次遇见(行缺失则按盘上字节自愈补 done 行),不排队。
        if r.md5.is_some()
            && let Some(path) = locate(&ticket)
        {
            let (format, size, animated) = sniff_disk(&path).await;
            record_seen(
                &svc.db,
                &ticket,
                &r.url,
                r.claimed_ext.as_deref(),
                Init::Done { format, size, animated },
            )
            .await;
            continue;
        }

        // 不在盘上:登记/记遇见(新行 pending)。
        record_seen(&svc.db, &ticket, &r.url, r.claimed_ext.as_deref(), Init::Pending).await;

        // 已在下载中 → 共用同一桩任务,凭据照常可等。
        if svc.inflight.lock().unwrap().contains_key(&ticket) {
            continue;
        }
        // 既有行可能是 failed / done 但文件丢了:重置回 pending(旧错误/结果清掉)再排队。
        set_pending(&svc.db, &ticket).await;
        enqueue(ticket, r.url, r.md5);
    }
    tickets
}

/// 等一桩图片就绪,返回落盘结果(并刷新 `last_used`)。
///
/// 还在队列/下载中 → 阻塞到完成或 `timeout`;已在盘上 → 立即返回;
/// 已记失败 / 没有记录 / 文件丢失 → 返回错误。其他插件「确保图可用」一律走这里。
pub async fn wait(ticket: &str, timeout: Duration) -> anyhow::Result<Stored> {
    // 在途:订阅完成广播再等(订阅先于广播发送,不会漏)。
    let rx = svc_subscribe(ticket);
    let stored = if let Some(mut rx) = rx {
        match tokio::time::timeout(timeout, rx.recv()).await {
            Ok(Ok(Ok(stored))) => stored,
            Ok(Ok(Err(msg))) => bail!("图片下载失败: {msg}"),
            // 广播通道异常关闭(理论不至):退回按盘/库兜底。
            Ok(Err(_)) => settled(ticket).await?,
            Err(_) => bail!("等待图片下载超时"),
        }
    } else {
        settled(ticket).await?
    };
    tokio::spawn(touch_used(stored.md5.clone()));
    Ok(stored)
}

/// 查一份图入库时嗅探的动图标志:`Some(true/false)` = 嗅探过;`None` = 无记录或未嗅探
/// (老行/查询失败),调用方手上有字节时可用 [`is_animated_image`] 兜底。
pub async fn animated_flag(md5: &str) -> Option<bool> {
    media_file::Entity::find_by_id(md5.to_owned())
        .one(&service().db)
        .await
        .ok()
        .flatten()
        .and_then(|m| m.animated)
}

/// 刷新一份图的 `last_used`(取用即「使用」:wait 取到 / 重发 / WebUI 取图)。
/// 行不存在静默跳过;失败只记 warn。
pub async fn touch_used(md5: String) {
    let res = media_file::Entity::update_many()
        .col_expr(media_file::Column::LastUsed, Expr::current_timestamp().into())
        .filter(media_file::Column::Md5.eq(&md5))
        .exec(&service().db)
        .await;
    if let Err(e) = res {
        tracing::warn!(%md5, error = %e, "刷新图片 last_used 失败");
    }
}

/// 在途表里找这桩任务并订阅其完成广播;不在途为 `None`。
fn svc_subscribe(ticket: &str) -> Option<broadcast::Receiver<Outcome>> {
    service().inflight.lock().unwrap().get(ticket).map(|s| s.subscribe())
}

/// 不在途的凭据按「盘 → 库」定结果:盘上有 → 成功;库里 failed/丢文件/没记录 → 对应错误。
async fn settled(md5: &str) -> anyhow::Result<Stored> {
    if let Some(path) = locate(md5) {
        return Ok(Stored { md5: md5.to_string(), path });
    }
    match media_file::Entity::find_by_id(md5).one(&service().db).await? {
        Some(r) if r.status == "failed" => {
            bail!("图片下载失败: {}", r.error.unwrap_or_else(|| "未知原因".into()))
        }
        Some(r) if r.status == "done" => bail!("图片已登记但文件不在盘上"),
        Some(_) => bail!("图片还在队列里但下载任务缺失"), // 崩溃残留,下次启动会重排
        None => bail!("没有这张图片的记录"),
    }
}

// ———— 队列与下载 ————

/// 把一桩任务登入在途表并送进队列;同凭据已在途则不重复(竞态下后到者直接共用)。
fn enqueue(ticket: String, url: String, md5: Option<String>) {
    let svc = service();
    {
        let mut map = svc.inflight.lock().unwrap();
        if map.contains_key(&ticket) {
            return;
        }
        let (otx, _) = broadcast::channel(1);
        map.insert(ticket.clone(), otx);
    }
    // 接收端只在进程退出时消失,送不进去无需处理。
    let _ = svc.tx.send(Job { ticket, url, md5 });
}

/// 队列泵:逐桩领任务,限 [`DOWNLOAD_CONCURRENCY`] 并发分派下载。
async fn dispatch(mut rx: mpsc::UnboundedReceiver<Job>) {
    let sem = Arc::new(Semaphore::new(DOWNLOAD_CONCURRENCY));
    while let Some(job) = rx.recv().await {
        let permit = sem.clone().acquire_owned().await.expect("semaphore 不会关闭");
        tokio::spawn(async move {
            let _permit = permit;
            process(job).await;
        });
    }
}

/// 跑完一桩下载:落盘 + 更新登记行,**之后**才摘在途表并广播(晚到的等待者按盘/库兜底)。
async fn process(job: Job) {
    let svc = service();
    let outcome = match download(&job).await {
        Ok((stored, size, format, animated)) => {
            finish_db(&svc.db, &job.ticket, &stored, &job.url, size, format, animated).await;
            Ok(stored)
        }
        Err(e) => {
            tracing::warn!(url = %job.url, error = %e, "下载图片失败");
            let msg: String = e.to_string().chars().take(300).collect();
            mark_failed(&svc.db, &job.ticket, &msg).await;
            Err(msg)
        }
    };
    let sender = svc.inflight.lock().unwrap().remove(&job.ticket);
    if let Some(s) = sender {
        let _ = s.send(outcome); // 没人等也正常(chatlog 是发后不理)
    }
}

/// 下载一张并落盘(分片目录,临时名写入 + 原子改名:盘上出现正式名即内容完整)。
///
/// 落盘名一律 `md5(bytes)`(无后缀):wire 提示只参与下载前去重,提示与字节不符记 warn、
/// 按真值归档(登记行随之改名)。返回(落盘结果, 字节数, 嗅探格式, 是否动图)。
/// 已存在跳过写盘(去重)。
async fn download(job: &Job) -> anyhow::Result<(Stored, i64, Option<String>, bool)> {
    let resp = client().get(&job.url).send().await?.error_for_status()?;
    let bytes = resp.bytes().await?;
    let digest = format!("{:x}", md5::compute(&bytes));
    if let Some(claimed) = &job.md5
        && *claimed != digest
    {
        tracing::warn!(url = %job.url, %claimed, actual = %digest, "wire 文件名与内容 md5 不符,按真值归档");
    }
    let format = format_tag(&bytes);
    let animated = is_animated_image(&bytes);
    let path = shard_path(&digest);
    if !path.exists() {
        let dir = path.parent().expect("分片路径必有父目录");
        tokio::fs::create_dir_all(dir).await?;
        let tmp = dir.join(format!(".tmp.{digest}"));
        tokio::fs::write(&tmp, &bytes).await?;
        tokio::fs::rename(&tmp, &path).await?;
        tracing::debug!(md5 = %digest, bytes = bytes.len(), "已归档图片");
    }
    Ok((Stored { md5: digest, path }, bytes.len() as i64, format, animated))
}

/// 下载成功后的登记收尾:真 md5 与凭据不同(无名来源/提示谎报)时删凭据行,
/// 真 md5 行 upsert 成 done(带字节数、嗅探格式、是否动图)。
async fn finish_db(
    db: &DatabaseConnection,
    ticket: &str,
    stored: &Stored,
    url: &str,
    size: i64,
    format: Option<String>,
    animated: bool,
) {
    if stored.md5 != ticket
        && let Err(e) = media_file::Entity::delete_by_id(ticket).exec(db).await
    {
        tracing::warn!(ticket, error = %e, "删图片临时登记行失败");
    }
    let row = media_file::ActiveModel {
        md5: Set(stored.md5.clone()),
        url: Set(url.to_string()),
        status: Set("done".into()),
        error: Set(None),
        size: Set(Some(size)),
        format: Set(format),
        animated: Set(Some(animated)),
        done_at: Set(Some(chrono::Utc::now().fixed_offset())),
        ..Default::default()
    };
    let upsert = media_file::Entity::insert(row).on_conflict(
        OnConflict::column(media_file::Column::Md5)
            .update_columns([
                media_file::Column::Url,
                media_file::Column::Status,
                media_file::Column::Error,
                media_file::Column::Size,
                media_file::Column::Format,
                media_file::Column::Animated,
                media_file::Column::DoneAt,
            ])
            .to_owned(),
    );
    if let Err(e) = upsert.exec(db).await {
        tracing::warn!(md5 = %stored.md5, error = %e, "更新图片登记为 done 失败");
    }
}

/// 把登记行标成 failed + 原因(行不存在只记 warn)。
async fn mark_failed(db: &DatabaseConnection, ticket: &str, msg: &str) {
    let row = media_file::ActiveModel {
        md5: Set(ticket.to_string()),
        status: Set("failed".into()),
        error: Set(Some(msg.to_string())),
        ..Default::default()
    };
    if let Err(e) = media_file::Entity::update(row).exec(db).await {
        tracing::warn!(ticket, error = %e, "更新图片登记为 failed 失败");
    }
}

/// 新行的初始形态:还要下载(pending),或已在盘上(done,带自愈嗅探出的格式/字节数/动图标志)。
enum Init {
    Pending,
    Done { format: Option<String>, size: Option<i64>, animated: Option<bool> },
}

/// 记一次「遇见」:行不存在按 `init` 形态插入(`seen_count` 库默认 1);已存在则
/// `seen_count`+1、`last_seen` 刷新、URL/后缀更新成最近一次的,**不动状态**
/// (下载生命周期由队列收尾函数管)。失败只记 warn。
async fn record_seen(
    db: &DatabaseConnection,
    key: &str,
    url: &str,
    claimed_ext: Option<&str>,
    init: Init,
) {
    let mut row = media_file::ActiveModel {
        md5: Set(key.to_string()),
        url: Set(url.to_string()),
        claimed_ext: Set(claimed_ext.map(str::to_string)),
        ..Default::default()
    };
    match init {
        Init::Pending => row.status = Set("pending".into()),
        Init::Done { format, size, animated } => {
            row.status = Set("done".into());
            row.format = Set(format);
            row.size = Set(size);
            row.animated = Set(animated);
            row.done_at = Set(Some(chrono::Utc::now().fixed_offset()));
        }
    }
    let upsert = media_file::Entity::insert(row).on_conflict(
        OnConflict::column(media_file::Column::Md5)
            .update_columns([media_file::Column::Url, media_file::Column::ClaimedExt])
            .value(
                media_file::Column::SeenCount,
                // PG 在 ON CONFLICT DO UPDATE 里要求列引用带表限定,否则报 ambiguous。
                Expr::col((media_file::Entity, media_file::Column::SeenCount)).add(1),
            )
            .value(media_file::Column::LastSeen, Expr::current_timestamp())
            .to_owned(),
    );
    if let Err(e) = upsert.exec(db).await {
        tracing::warn!(md5 = %key, error = %e, "记图片遇见失败");
    }
}

/// 把既有行重置回 pending(清掉旧错误/结果),供重下。行刚由 [`record_seen`] 确保存在。
async fn set_pending(db: &DatabaseConnection, key: &str) {
    let row = media_file::ActiveModel {
        md5: Set(key.to_string()),
        status: Set("pending".into()),
        error: Set(None),
        size: Set(None),
        format: Set(None),
        animated: Set(None),
        done_at: Set(None),
        ..Default::default()
    };
    if let Err(e) = media_file::Entity::update(row).exec(db).await {
        tracing::warn!(md5 = %key, error = %e, "重置图片登记为 pending 失败");
    }
}

// ———— 杂项 ————

/// 规范化 wire 文件名:去掉 QQ 老格式 gchat 的 `{}`/`-`、整体小写。
/// 老格式 `{3844E673-…}.GIF` → `3844e673….gif`(主体即 md5);新格式本就是 `{MD5}.ext`。
fn normalize_name(filename: &str) -> String {
    filename.chars().filter(|c| !matches!(c, '{' | '}' | '-')).collect::<String>().to_lowercase()
}

/// 是不是一段内容 md5(32 位小写 hex 已由调用方保证大小写;此处只验形)。
fn is_md5(s: &str) -> bool {
    s.len() == 32 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// 解析(已规范化的)wire 文件名提示:`<32 位 hex>.<1-5 位小写字母数字>` → (md5, 后缀)。
/// 名字提示是发端可控的,**只**放行这一形(QQ 正常 wire 名即此形),其余整体弃之。
fn parse_hint(name: &str) -> Option<(String, String)> {
    let (stem, ext) = name.rsplit_once('.')?;
    let ok = is_md5(stem)
        && (1..=5).contains(&ext.len())
        && ext.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit());
    ok.then(|| (stem.to_string(), ext.to_string()))
}

/// 校验一份归档文件:重算盘上字节的 md5 与文件名(= 主键)比对(归档不变量
/// 「文件名 = 内容 md5」)。读不出 / 不是 md5 形 / 摘要不符 → `false`。
/// 维护排查用;平时读图不必走它(写盘是临时名 + 原子改名,正式名出现即内容完整)。
pub async fn verify_file(md5: &str) -> bool {
    if !is_md5(md5) {
        return false;
    }
    let Ok(bytes) = tokio::fs::read(resolve(md5)).await else { return false };
    format!("{:x}", md5::compute(&bytes)) == md5
}

/// 按魔数嗅探图片 MIME;认不出为 `None`。
/// 走 `image::guess_format`(纯签名表、不依赖解码 feature),覆盖 PNG/JPEG/GIF/WEBP/
/// BMP/TIFF/ICO/AVIF/QOI 等。发端报的后缀会谎(动画表情常见 `.suf` 实为 PNG),
/// 呈现给浏览器时以字节真相为准。
pub fn sniff_image_ct(bytes: &[u8]) -> Option<&'static str> {
    image::guess_format(bytes).ok().map(|f| f.to_mime_type())
}

/// 字节嗅探是否**动图**(GIF / 动画 WebP / APNG)——内嵌渲染会把动图压成单帧,呈现方
/// 据此决定原样发段还是嵌进排版文档。
/// - GIF:一律当动图。静态 GIF 罕见,误判的代价只是不内嵌、改原样发段,呈现无损。
/// - WebP:`VP8X` 扩展头的 animation 位(RIFF 偏移 12 为 `VP8X` 时,偏移 20 的 bit 0x02)。
/// - PNG:`IDAT` 之前出现 `acTL` 块即 APNG(动画表情常见 `.suf` 实为 APNG)。
///
/// 其余格式当静图。
pub fn is_animated_image(bytes: &[u8]) -> bool {
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return true;
    }
    if bytes.len() > 20 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return &bytes[12..16] == b"VP8X" && bytes[20] & 0x02 != 0;
    }
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        // acTL 须在 IDAT 之前(APNG 规范);先定位 IDAT,只在其前找 acTL。
        let idat = bytes.windows(4).position(|w| w == b"IDAT").unwrap_or(bytes.len());
        return bytes[..idat].windows(4).any(|w| w == b"acTL");
    }
    false
}

/// 嗅探格式的入库短标签(MIME 去掉 `image/` 前缀:`png`/`jpeg`/`gif`/`webp`/…)。
fn format_tag(bytes: &[u8]) -> Option<String> {
    sniff_image_ct(bytes).map(|ct| ct.trim_start_matches("image/").to_string())
}

/// 读盘上文件的头部嗅探格式与动图标志 + 取字节数(自愈补行用;读不出皆 `None`)。
/// 读 4KB:格式签名 64 字节就够,动图判定(APNG 的 `acTL` 须在 `IDAT` 前)要看更深——
/// 头部塞满辅助块把 `IDAT` 挤出 4KB 的 PNG 理论存在,误判为静图,代价仅是呈现方不打动图标。
async fn sniff_disk(path: &Path) -> (Option<String>, Option<i64>, Option<bool>) {
    use tokio::io::AsyncReadExt;
    let size = tokio::fs::metadata(path).await.ok().map(|m| m.len() as i64);
    let mut head = [0u8; 4096];

    let (format, animated) = match tokio::fs::File::open(path).await {
        Ok(mut f) => match f.read(&mut head).await {
            Ok(n) => (format_tag(&head[..n]), Some(is_animated_image(&head[..n]))),
            Err(_) => (None, None),
        },
        Err(_) => (None, None),
    };
    (format, size, animated)
}

/// 进程级共享的 HTTP 客户端(rustls,30s 超时)。
fn client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder().timeout(Duration::from_secs(30)).build().unwrap_or_default()
    })
}
