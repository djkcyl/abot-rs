//! 内容审核 —— 腾讯云 AI（TMS 文本 / IMS 图片）。无凭证或调用失败一律返 `Err`：审核拿不到结论就不
//! 放行，由调用方中止本次操作，不让内容趁审核不可用绕过（fail-closed）。
//!
//! 归 `integrations` 下、对 crate 公开，便于跨插件复用（投稿、入群审批等）。拿到的裁决按内容键入库 `content_moderation`
//! （图片用媒体库 md5、文本用文本 md5），兼作缓存：同内容再来读库，不重复调用。配置走环境变量，惰性读一次。

mod entity;
mod migration;
mod qrcode;
mod tencent;

use std::sync::OnceLock;

use anyhow::{Context, Result};
use sea_orm::DatabaseConnection;

/// 一次审核对外的裁决；完整明细见 `content_moderation` 表。
pub struct Verdict {
    /// 是否安全。
    pub safe: bool,
    /// 风险大类，安全时为空串。
    pub label: String,
    /// 子类，可为空串。
    pub sub_label: String,
    /// 裁决来源，目前恒为 `tencent`。
    pub source: &'static str,
}

impl Verdict {
    /// 安全裁决（建议 Pass）。
    fn pass() -> Self {
        Self { safe: true, label: String::new(), sub_label: String::new(), source: "tencent" }
    }

    /// 不安全裁决，带大类/子类与来源（`tencent` 腾讯云 / `local` 本地二维码闸）。
    pub(super) fn flagged(label: &str, sub_label: &str, source: &'static str) -> Self {
        Self { safe: false, label: label.to_string(), sub_label: sub_label.to_string(), source }
    }

    /// 由腾讯云裁决三元组构造：`Suggestion == Pass` 即安全，否则带回大类/子类。
    pub(super) fn from_tc(suggestion: &str, label: &str, sub_label: &str) -> Self {
        if suggestion == "Pass" { Self::pass() } else { Self::flagged(label, sub_label, "tencent") }
    }
}

/// 腾讯云这一路的配置。仅在凭证齐全时构造。
pub(crate) struct TcConfig {
    pub secret_id: String,
    pub secret_key: String,
    pub region: String,
    pub text_biztype: Option<String>,
    pub image_biztype: Option<String>,
}

/// 内容审核器：持腾讯云配置（可缺），惰性单例。
pub struct ContentModerator {
    tencent: Option<TcConfig>,
}

impl ContentModerator {
    /// 进程级单例，首次取用时从环境读配置。
    pub fn shared() -> &'static ContentModerator {
        static INSTANCE: OnceLock<ContentModerator> = OnceLock::new();
        INSTANCE.get_or_init(ContentModerator::from_env)
    }

    /// 从环境变量装配；缺密钥则腾讯云路为空，审核一律失败。
    fn from_env() -> Self {
        let tencent = match (env_nonempty("TENCENT_SECRET_ID"), env_nonempty("TENCENT_SECRET_KEY")) {
            (Some(secret_id), Some(secret_key)) => Some(TcConfig {
                secret_id,
                secret_key,
                region: env_nonempty("TENCENT_REGION").unwrap_or_else(|| "ap-guangzhou".into()),
                text_biztype: env_nonempty("TENCENT_TEXT_BIZTYPE"),
                image_biztype: env_nonempty("TENCENT_IMAGE_BIZTYPE"),
            }),
            _ => None,
        };
        Self { tencent }
    }

    /// 审核文本：键用文本 md5，连原文一并入库。无凭证或调用失败返 `Err`。
    pub async fn moderate_text(&self, db: &DatabaseConnection, text: &str) -> Result<Verdict> {
        let cfg = self.tencent.as_ref().context("内容审核未配置")?;
        let key = format!("{:x}", md5::compute(text.as_bytes()));
        if let Some(v) = entity::cached(db, "text", &key).await {
            return Ok(v);
        }
        let res = tencent::text_moderation(cfg, text).await.context("腾讯云文本审核失败")?;
        Ok(entity::store(db, "text", &key, Some(text), &res).await)
    }

    /// 审核图片：键用媒体库 md5（不存图片字节）。先读缓存，再过本地二维码闸（命中即判 `Ad/QRCode`，
    /// 无需腾讯云、也不受其可用性影响），最后走腾讯云。无凭证或调用失败返 `Err`。
    pub async fn moderate_image(&self, db: &DatabaseConnection, md5: &str, bytes: &[u8]) -> Result<Verdict> {
        if let Some(v) = entity::cached(db, "image", md5).await {
            return Ok(v);
        }
        if qrcode::has_qrcode(bytes) {
            return Ok(entity::store_qrcode(db, md5).await);
        }
        let cfg = self.tencent.as_ref().context("内容审核未配置")?;
        let res = tencent::image_moderation(cfg, bytes).await.context("腾讯云图片审核失败")?;
        Ok(entity::store(db, "image", md5, None, &res).await)
    }
}

/// 读环境变量，去首尾空白后为空则当作未设置。
fn env_nonempty(key: &str) -> Option<String> {
    std::env::var(key).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}
