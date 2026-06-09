//! 入站图片落盘 —— 把投放消息里的图片下载到本地盘（`IMAGE_DIR`，默认 `./data/images`），
//! 文件名取 wire `filename`（新版 QQ 即 `{MD5}.ext`）或自算 `md5(bytes)`，已存在即去重。
//!
//! 与 chatlog 的 `media` 同款下载思路，但**返回原始字节**：投放流程要把字节喂给内容审核
//! （[`super::super::super::moderation`]），不能下完就丢。单张失败只记 warn、跳过，不中断整批。

use std::path::PathBuf;
use std::time::Duration;

use nagisa::prelude::*;

/// 一张已落盘的图片：文件名（存进瓶子的 `images` 数组）+ 原始字节（喂审核）。
pub struct StoredImage {
    /// 落盘文件名（`IMAGE_DIR` 下的相对名，捞取时 `Segment::image_path` 据此重发）。
    pub filename: String,
    /// 原始图片字节，供调用方喂给图片审核。
    pub bytes: bytes::Bytes,
}

/// 从消息段里筛出图片，逐张下载落盘到 [`image_dir`]，返回（文件名 + 字节）。
///
/// 只取 http(s) 来源的图片段（本地 path 形态跳过）。文件名优先用 wire `filename`，否则
/// `md5(bytes).<ext>`（扩展名由 content-type 猜）。已存在的文件不重复写盘，但仍回字节。
/// 单张下载失败只记 warn、跳过，整批不报错。
pub async fn fetch_and_store(content: &[Segment]) -> Vec<StoredImage> {
    let dir = image_dir();
    if let Err(e) = tokio::fs::create_dir_all(&dir).await {
        tracing::warn!(error = %e, dir = ?dir, "创建图片目录失败");
        return Vec::new();
    }

    let mut out = Vec::new();
    for url in collect_urls(content) {
        match fetch_one(&dir, &url).await {
            Ok(img) => out.push(img),
            Err(e) => tracing::warn!(url = %url, error = %e, "下载漂流瓶图片失败,跳过"),
        }
    }
    out
}

/// 图片落盘目录（环境变量 `IMAGE_DIR`，默认 `./data/images`）。
pub fn image_dir() -> PathBuf {
    std::env::var("IMAGE_DIR").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("./data/images"))
}

/// 一张图的下载来源：URL + 可选的已知 wire 文件名（即 md5）。
struct ImageUrl {
    url: String,
    name: Option<String>,
}

impl std::fmt::Display for ImageUrl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.url)
    }
}

/// 从消息内容里收集所有图片下载来源（只取 http(s) 的；本地 path 形态跳过）。
fn collect_urls(content: &[Segment]) -> Vec<ImageUrl> {
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
                Some(ImageUrl { url, name })
            }
            _ => None,
        })
        .collect()
}

/// 下载一张并落盘，返回文件名 + 字节。文件名优先 wire `filename`，否则 `md5(bytes).<ext>`；
/// 文件已存在则跳过写盘（去重）但仍回字节。
async fn fetch_one(dir: &std::path::Path, job: &ImageUrl) -> anyhow::Result<StoredImage> {
    let resp = client().get(&job.url).send().await?.error_for_status()?;
    let ct = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let bytes = resp.bytes().await?;
    let name = job
        .name
        .clone()
        .unwrap_or_else(|| format!("{:x}.{}", md5::compute(&bytes), ext_from_ct(ct.as_deref())));
    let path = dir.join(&name);
    if !path.exists() {
        tokio::fs::write(&path, &bytes).await?;
        tracing::debug!(file = %name, bytes = bytes.len(), "已落盘漂流瓶图片");
    }
    Ok(StoredImage { filename: name, bytes })
}

/// 规范化 wire 文件名为干净的 md5 命名：去掉 QQ 老格式 gchat 的 `{}`/`-`、主体小写、扩展名小写。
/// 与 chatlog 同款，使同一图在新老两端命名一致 → 去重生效。
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

/// 由 content-type 猜扩展名（仅在无 wire 文件名时的兜底）。
fn ext_from_ct(ct: Option<&str>) -> &'static str {
    match ct.unwrap_or("") {
        s if s.contains("png") => "png",
        s if s.contains("gif") => "gif",
        s if s.contains("webp") => "webp",
        s if s.contains("jpeg") || s.contains("jpg") => "jpg",
        _ => "bin",
    }
}

/// 进程级共享的 HTTP 客户端（rustls，30s 超时）。
fn client() -> &'static reqwest::Client {
    use std::sync::OnceLock;
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder().timeout(Duration::from_secs(30)).build().unwrap_or_default()
    })
}
