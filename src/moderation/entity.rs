//! `content_moderation` 表 —— 审核结果记录兼缓存:按内容键(图片=媒体库 md5、文本=文本 md5)一行,
//! 存腾讯云裁决与命中明细。同内容再审直接读本表。文本行连原文(`content`)一并存,图片行 `content`
//! 为 NULL(按 md5 回媒体库取图)。只在拿到裁决时写——审核失败返 `Err`、不入库。

use sea_orm::entity::prelude::*;
use sea_orm::{ActiveValue::NotSet, Set};

use super::Verdict;
use super::tencent::TcResult;

/// `content_moderation` 行模型。
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "content_moderation")]
pub struct Model {
    /// 内容类别:`text` / `image`(与 `content_key` 共同作主键)。
    #[sea_orm(primary_key, auto_increment = false)]
    pub kind: String,
    /// 内容键:图片为媒体库 md5,文本为文本 md5。
    #[sea_orm(primary_key, auto_increment = false)]
    pub content_key: String,
    /// 文本原文(文本行有,图片行为 NULL)。
    pub content: Option<String>,
    /// 裁决来源,目前恒为 `tencent`。
    pub source: String,
    /// 建议处置 `Pass`/`Review`/`Block`;非 `Pass` 即不安全。
    pub suggestion: String,
    /// 命中大类(安全时图片为 `Normal`、文本为空串)。
    pub label: String,
    /// 子类,可空串。
    pub sub_label: String,
    /// 置信分 0–100。
    pub score: i32,
    /// 各项命中明细:文本为 `{keywords, items}`,图片为 `{ocr_text, items}`。
    pub details: Json,
    /// 腾讯云 RequestId。
    pub request_id: String,
    /// 入库时间(库侧 `now()`)。
    pub created_at: DateTimeWithTimeZone,
}

/// 独立表,无外联关系。
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

/// 读缓存:同内容已审过则返回其裁决(保留来源 `tencent`/`local`)。库出错按未命中处理。
pub(super) async fn cached(db: &DatabaseConnection, kind: &str, key: &str) -> Option<Verdict> {
    match Entity::find_by_id((kind.to_string(), key.to_string())).one(db).await {
        Ok(Some(row)) if row.suggestion == "Pass" => Some(Verdict::from_tc("Pass", "", "")),
        Ok(Some(row)) => {
            let source = if row.source == "local" { "local" } else { "tencent" };
            Some(Verdict::flagged(&row.label, &row.sub_label, source))
        }
        Ok(None) => None,
        Err(e) => {
            tracing::warn!(error = %e, "读审核缓存失败,按未命中处理");
            None
        }
    }
}

/// 本地二维码闸命中:入库一行(来源 `local`、判 `Ad/QRCode`)并返回裁决。写库失败不阻断。
pub(super) async fn store_qrcode(db: &DatabaseConnection, md5: &str) -> Verdict {
    let row = ActiveModel {
        kind: Set("image".to_string()),
        content_key: Set(md5.to_string()),
        content: Set(None),
        source: Set("local".to_string()),
        suggestion: Set("Block".to_string()),
        label: Set("Ad".to_string()),
        sub_label: Set("QRCode".to_string()),
        score: Set(0),
        details: Set(serde_json::json!({ "detector": "wechat-qrcode" })),
        request_id: Set(String::new()),
        created_at: NotSet,
    };
    if let Err(e) = Entity::insert(row).exec(db).await {
        tracing::warn!(error = %e, "写二维码审核结果失败");
    }
    Verdict::flagged("Ad", "QRCode", "local")
}

/// 入库并返回裁决。写库失败不阻断(裁决已拿到),记一条 warn。
pub(super) async fn store(
    db: &DatabaseConnection,
    kind: &str,
    key: &str,
    content: Option<&str>,
    res: &TcResult,
) -> Verdict {
    let row = ActiveModel {
        kind: Set(kind.to_string()),
        content_key: Set(key.to_string()),
        content: Set(content.map(str::to_string)),
        source: Set("tencent".to_string()),
        suggestion: Set(res.suggestion.clone()),
        label: Set(res.label.clone()),
        sub_label: Set(res.sub_label.clone()),
        score: Set(res.score),
        details: Set(res.details.clone()),
        request_id: Set(res.request_id.clone()),
        created_at: NotSet,
    };
    if let Err(e) = Entity::insert(row).exec(db).await {
        tracing::warn!(error = %e, "写审核结果失败");
    }
    Verdict::from_tc(&res.suggestion, &res.label, &res.sub_label)
}
