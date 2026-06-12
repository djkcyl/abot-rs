//! 内容审核 —— 腾讯云 AI 为主、本地关键词/二维码兜底，二者取并集判不安全。
//!
//! 放 crate 级而非塞进某个插件，便于将来投稿、入群审批等场景复用。配置一律走环境变量，
//! 经 [`ContentModerator::shared`] 惰性读一次：缺腾讯云凭证就只走本地，无词表就只靠腾讯云，
//! 两头皆空则一切放行（`source = "none"`）。**腾讯云调用出错不会拖垮审核**——记一条 warn
//! 后退回本地结果，把不安全内容交由下游人工复审，是有意为之的 fail-safe。

mod local;
mod tencent;

use std::sync::OnceLock;

use local::{KeywordMatcher, has_qrcode};

/// 一次审核的裁决。
pub struct Verdict {
    /// 是否安全（可直接放行）。
    pub safe: bool,
    /// 风险大类，如 `Porn`/`Abuse`/`Keyword`/`AD`；安全时为空串。
    pub label: String,
    /// 子类，如 `QRCode`；可为空串。
    pub sub_label: String,
    /// 裁决来源：`tencent` / `local` / `none`。
    pub source: &'static str,
}

impl Verdict {
    /// 安全裁决（无命中），标注来源。
    fn safe(source: &'static str) -> Self {
        Self { safe: true, label: String::new(), sub_label: String::new(), source }
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

/// 内容审核器：持腾讯云配置（可缺）、本地关键词自动机，惰性单例。
pub struct ContentModerator {
    tencent: Option<TcConfig>,
    keywords: KeywordMatcher,
}

impl ContentModerator {
    /// 进程级单例，首次取用时从环境读配置、建关键词自动机。
    pub fn shared() -> &'static ContentModerator {
        static INSTANCE: OnceLock<ContentModerator> = OnceLock::new();
        INSTANCE.get_or_init(ContentModerator::from_env)
    }

    /// 从环境变量装配。缺密钥 → 腾讯云路关闭；缺词表 → 空词表。
    fn from_env() -> Self {
        let secret_id = env_nonempty("TENCENT_SECRET_ID");
        let secret_key = env_nonempty("TENCENT_SECRET_KEY");
        let tencent = match (secret_id, secret_key) {
            (Some(secret_id), Some(secret_key)) => Some(TcConfig {
                secret_id,
                secret_key,
                region: env_nonempty("TENCENT_REGION").unwrap_or_else(|| "ap-guangzhou".into()),
                text_biztype: env_nonempty("TENCENT_TEXT_BIZTYPE"),
                image_biztype: env_nonempty("TENCENT_IMAGE_BIZTYPE"),
            }),
            _ => None,
        };

        let keywords = KeywordMatcher::new(&load_keywords());
        Self { tencent, keywords }
    }

    /// 审核文本：本地关键词 + 腾讯云文本审核取并集。
    pub async fn moderate_text(&self, text: &str) -> Verdict {
        if self.keywords.text_hit(text) {
            return Verdict { safe: false, label: "Keyword".into(), sub_label: String::new(), source: "local" };
        }
        match &self.tencent {
            Some(cfg) => match tencent::text_moderation(cfg, text).await {
                Ok(v) => tc_to_verdict(v),
                Err(e) => {
                    tracing::warn!(error = %e, "腾讯云文本审核失败,退回本地结果");
                    Verdict::safe("none")
                }
            },
            None => Verdict::safe("none"),
        }
    }

    /// 审核图片：本地二维码 + 腾讯云图片审核取并集。
    pub async fn moderate_image(&self, bytes: &[u8]) -> Verdict {
        if has_qrcode(bytes) {
            return Verdict { safe: false, label: "AD".into(), sub_label: "QRCode".into(), source: "local" };
        }
        match &self.tencent {
            Some(cfg) => match tencent::image_moderation(cfg, bytes).await {
                Ok(v) => tc_to_verdict(v),
                Err(e) => {
                    tracing::warn!(error = %e, "腾讯云图片审核失败,退回本地结果");
                    Verdict::safe("none")
                }
            },
            None => Verdict::safe("none"),
        }
    }
}

/// 腾讯云裁决转成对外 `Verdict`：安全则清空标签，不安全则带回 label/sub_label。
fn tc_to_verdict(v: tencent::TcVerdict) -> Verdict {
    if v.safe {
        Verdict::safe("tencent")
    } else {
        Verdict { safe: false, label: v.label, sub_label: v.sub_label, source: "tencent" }
    }
}

/// 读环境变量，去首尾空白后为空则当作未设置。
fn env_nonempty(key: &str) -> Option<String> {
    std::env::var(key).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// 从 `BOTTLE_KEYWORD_FILE` 读关键词（换行分隔，去空白去空行）。缺文件/读失败 → 空表。
fn load_keywords() -> Vec<String> {
    let Some(path) = env_nonempty("BOTTLE_KEYWORD_FILE") else {
        return Vec::new();
    };
    match std::fs::read_to_string(&path) {
        Ok(content) => content.lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_string).collect(),
        Err(e) => {
            tracing::warn!(file = %path, error = %e, "读取关键词词表失败,按空表处理");
            Vec::new()
        }
    }
}
