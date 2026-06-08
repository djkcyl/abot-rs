//! 图片归档 —— 把消息里的图片下载到**本地盘**(不上对象存储)，按 md5 文件名命名、去重。
//!
//! 命名：直接用 wire 的 `filename`(新版 QQ 即 `{MD5}.ext`，见可读日志那边的发现)；缺失才退回
//! 自算 `md5(bytes)` + 由 content-type 猜扩展名(老 abot 口径)。目录由环境变量 `IMAGE_DIR` 指定
//! (默认 `./data/images`)，这是消息记录插件**私有**的可调项,故就近读、不进核心 config。
//!
//! 下载在 detached 任务里跑(见 [`super`] 的 recorder),绝不阻塞消息处理；失败只记日志、跳过。
//! ⚠️ `multimedia.nt.qq.com.cn` 的 TLS 偏门(老 abot 要特判)，rustls 若握手失败再议。

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use nagisa::prelude::*;

/// 一桩图片下载任务：来源 URL + 可选的已知文件名(wire `filename`，即 md5)。
pub struct ImageJob {
    url: String,
    name: Option<String>,
}

/// 从消息内容里收集所有图片下载任务(只取 http(s) 的；本地 path 形态跳过)。
pub fn collect_jobs(content: &[Segment]) -> Vec<ImageJob> {
    content
        .iter()
        .filter_map(|seg| match seg {
            Segment::Image { res, .. } => {
                let recv = res.recv.as_ref()?;
                let url = recv.url.clone().or_else(|| recv.id.clone())?;
                if !url.starts_with("http") {
                    return None; // 本地 path / 非网络来源 → 不下载
                }
                let name = recv.raw.get("filename").and_then(|v| v.as_str()).map(normalize_name);
                Some(ImageJob { url, name })
            }
            _ => None,
        })
        .collect()
}

/// 逐个下载并落盘(已存在即跳过)。整体不返回错误：单张失败只记日志、继续下一张。
pub async fn archive(jobs: Vec<ImageJob>) {
    let dir = image_dir();
    if let Err(e) = tokio::fs::create_dir_all(dir).await {
        tracing::warn!(error = %e, dir = ?dir, "创建图片目录失败");
        return;
    }
    for job in jobs {
        // 已知文件名且已存在 → 下载前就跳过(省一次网络往返)。
        if let Some(name) = &job.name
            && dir.join(name).exists()
        {
            continue;
        }
        if let Err(e) = fetch_one(dir, &job).await {
            tracing::warn!(url = %job.url, error = %e, "下载图片失败,跳过");
        }
    }
}

/// 下载一张并落盘。文件名优先用 wire `filename`，否则 `md5(bytes).<ext>`。
async fn fetch_one(dir: &Path, job: &ImageJob) -> anyhow::Result<()> {
    let resp = client().get(&job.url).send().await?.error_for_status()?;
    let ct = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let bytes = resp.bytes().await?;
    let name = job.name.clone().unwrap_or_else(|| {
        format!("{:x}.{}", md5::compute(&bytes), ext_from_ct(ct.as_deref()))
    });
    let path = dir.join(&name);
    if path.exists() {
        return Ok(()); // 并发下别处刚下完同一张
    }
    tokio::fs::write(&path, &bytes).await?;
    tracing::debug!(file = %name, bytes = bytes.len(), "已归档图片");
    Ok(())
}

/// 规范化 wire 文件名为干净的 md5 命名:去掉 QQ 老格式 gchat 的 `{}`/`-`、主体小写,扩展名小写。
/// 老格式 `{3844E673-6F30-DEB5-DA1D-9B7DDE511DD8}.gif` → `3844e6736f30deb5da1d9b7dde511dd8.gif`
/// (主体即 md5);新格式本就是 `{MD5}.ext`。如此同一图在新老两端命名一致 → 去重生效。
fn normalize_name(filename: &str) -> String {
    let (stem, ext) = filename.rsplit_once('.').unwrap_or((filename, ""));
    let stem: String =
        stem.chars().filter(|c| !matches!(c, '{' | '}' | '-')).collect::<String>().to_lowercase();
    if ext.is_empty() {
        stem
    } else {
        format!("{stem}.{}", ext.to_lowercase())
    }
}

/// 由 content-type 猜扩展名(仅在无 wire 文件名时的兜底)。
fn ext_from_ct(ct: Option<&str>) -> &'static str {
    match ct.unwrap_or("") {
        s if s.contains("png") => "png",
        s if s.contains("gif") => "gif",
        s if s.contains("webp") => "webp",
        s if s.contains("jpeg") || s.contains("jpg") => "jpg",
        _ => "bin",
    }
}

/// 进程级共享的 HTTP 客户端(rustls，30s 超时)。
fn client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap_or_default()
    })
}

/// 图片落盘目录(环境变量 `IMAGE_DIR`，默认 `./data/images`)；读一次缓存。
fn image_dir() -> &'static Path {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        std::env::var("IMAGE_DIR").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("./data/images"))
    })
    .as_path()
}
