//! 入瓶图片 —— 复用顶层媒体服务([`crate::integrations::media`]):把投放消息里的图登记 + 排队下载,
//! **阻塞等**每张落盘完成,再读回字节喂内容审核([`crate::integrations::moderation`])。
//!
//! 与 chatlog 那条「发后不理」的归档路径同源同队列:同一张图全 bot 只下一次,这里只是
//! 多了一步 `wait`(投放流程必须拿到字节才能过审)。单张失败/超时只记 warn、跳过,不中断整批。

use std::time::Duration;

use nagisa::prelude::*;

/// 单张图从排队到落盘的最长等待(下载本身 30s 超时 + 队列排队余量)。
const WAIT_TIMEOUT: Duration = Duration::from_secs(60);

/// 一张已落盘的图片:内容 md5(存进瓶子的 `images` 数组)+ 原始字节(喂审核)。
pub struct StoredImage {
    /// 内容 md5(捞取时经 [`crate::integrations::media::resolve`] 取路径重发)。
    pub md5: String,
    /// 原始图片字节,供调用方喂给图片审核。
    pub bytes: bytes::Bytes,
}

/// 从消息段里筛出图片,经媒体服务排队下载并等到落盘,返回(md5 + 字节)。
///
/// 已缓存的立即返回;还在队列里的阻塞到完成。单张失败只记 warn、跳过,整批不报错。
pub async fn fetch_and_store(content: &[Segment]) -> Vec<StoredImage> {
    let refs = crate::integrations::media::scan(content);
    if refs.is_empty() {
        return Vec::new();
    }
    let tickets = crate::integrations::media::ingest(refs).await;
    let mut out = Vec::with_capacity(tickets.len());
    for ticket in tickets {
        let stored = match crate::integrations::media::wait(&ticket, WAIT_TIMEOUT).await {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(ticket, error = %e, "等漂流瓶图片下载失败,跳过");
                continue;
            }
        };
        match tokio::fs::read(&stored.path).await {
            Ok(bytes) => out.push(StoredImage { md5: stored.md5, bytes: bytes.into() }),
            Err(e) => tracing::warn!(md5 = %stored.md5, error = %e, "读漂流瓶图片失败,跳过"),
        }
    }
    out
}
