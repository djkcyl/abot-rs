//! 腾讯云内容安全（CMS）签名与调用 —— TC3-HMAC-SHA256。
//!
//! 文本走 TextModeration（tms），图片走 ImageModeration（ims）。两者按腾讯云 v3 规范拼规范请求、
//! 派生签名密钥、带 `X-TC-*` 头 POST。HTTP / 解析 / API 错误一律返 `Err`。
//!
//! 解析只留入库要用的字段（顶层裁决 + 命中明细）；retcode、HitInfos、Positions、空类目、
//! LibId/LibName 等噪声丢弃。

use anyhow::{Context, Result, bail};
use base64::Engine;
use hmac::{Hmac, KeyInit, Mac};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::sync::OnceLock;
use std::time::Duration;

use super::TcConfig;

type HmacSha256 = Hmac<Sha256>;

const VERSION: &str = "2020-12-29";
const CONTENT_TYPE: &str = "application/json; charset=utf-8";

/// 腾讯云一次审核的结果。
pub(super) struct TcResult {
    /// 建议处置：`Pass` / `Review` / `Block`（图片只有 Pass / Block）。非 `Pass` 即判不安全。
    pub suggestion: String,
    /// 命中大类（安全时图片为 `Normal`、文本为空串）。
    pub label: String,
    /// 子类，可空串。
    pub sub_label: String,
    /// 置信分 0–100。
    pub score: i32,
    /// 腾讯云 RequestId。
    pub request_id: String,
    /// 各项命中明细：文本为 `{keywords, items}`，图片为 `{ocr_text, items}`。
    pub details: Value,
}

/// 文本审核：host `tms.tencentcloudapi.com`，service `tms`。
pub(super) async fn text_moderation(cfg: &TcConfig, text: &str) -> Result<TcResult> {
    let content = base64::engine::general_purpose::STANDARD.encode(text.as_bytes());
    let mut body = json!({ "Content": content });
    if let Some(biz) = cfg.text_biztype.as_deref().filter(|s| !s.is_empty()) {
        body["BizType"] = Value::String(biz.to_string());
    }
    let resp = call(cfg, "tms.tencentcloudapi.com", "tms", "TextModeration", &body).await?;
    Ok(parse_text(&resp))
}

/// 图片审核：host `ims.tencentcloudapi.com`，service `ims`。用 base64 字节(FileContent)，不传 URL。
pub(super) async fn image_moderation(cfg: &TcConfig, bytes: &[u8]) -> Result<TcResult> {
    let content = base64::engine::general_purpose::STANDARD.encode(bytes);
    let mut body = json!({ "FileContent": content });
    if let Some(biz) = cfg.image_biztype.as_deref().filter(|s| !s.is_empty()) {
        body["BizType"] = Value::String(biz.to_string());
    }
    let resp = call(cfg, "ims.tencentcloudapi.com", "ims", "ImageModeration", &body).await?;
    Ok(parse_image(&resp))
}

/// 拼签名、发请求、取出 `Response` 对象；含 `Error` 字段即返 `Err`。
async fn call(cfg: &TcConfig, host: &str, service: &str, action: &str, body: &Value) -> Result<Value> {
    let payload = serde_json::to_vec(body).context("序列化请求体失败")?;

    let timestamp =
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).context("系统时间早于 UNIX 纪元")?.as_secs()
            as i64;
    let date =
        chrono::DateTime::from_timestamp(timestamp, 0).context("时间戳无法转为日期")?.format("%Y-%m-%d").to_string();

    let authorization = sign(&cfg.secret_id, &cfg.secret_key, host, service, &date, timestamp, &payload);

    let url = format!("https://{host}/");
    let text = client()
        .post(&url)
        .header("Authorization", authorization)
        .header("Content-Type", CONTENT_TYPE)
        .header("Host", host)
        .header("X-TC-Action", action)
        .header("X-TC-Timestamp", timestamp.to_string())
        .header("X-TC-Version", VERSION)
        .header("X-TC-Region", &cfg.region)
        .body(payload)
        .send()
        .await
        .context("请求腾讯云失败")?
        .text()
        .await
        .context("读取腾讯云响应失败")?;

    let v: Value = serde_json::from_str(&text).context("腾讯云响应非 JSON")?;
    let resp = v.get("Response").context("响应缺少 Response 字段")?;
    if let Some(err) = resp.get("Error") {
        let code = err.get("Code").and_then(Value::as_str).unwrap_or("Unknown");
        let msg = err.get("Message").and_then(Value::as_str).unwrap_or("");
        bail!("腾讯云返回错误 {code}: {msg}");
    }
    Ok(resp.clone())
}

/// 文本响应 → `TcResult`：顶层取 Suggestion/Label/SubLabel/Score；明细取 `DetailResults` 里非 Pass
/// 的类目，外加顶层聚合 keywords。
fn parse_text(resp: &Value) -> TcResult {
    let items: Vec<Value> = resp
        .get("DetailResults")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter(|d| str_at(d, "Suggestion") != "Pass")
                .map(|d| {
                    json!({
                        "label": str_at(d, "Label"),
                        "sub_label": str_at(d, "SubLabel"),
                        "suggestion": str_at(d, "Suggestion"),
                        "score": int_at(d, "Score"),
                        "keywords": str_vec(d.get("Keywords")),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    TcResult {
        suggestion: str_at(resp, "Suggestion"),
        label: str_at(resp, "Label"),
        sub_label: str_at(resp, "SubLabel"),
        score: int_at(resp, "Score"),
        request_id: str_at(resp, "RequestId"),
        details: json!({ "keywords": str_vec(resp.get("Keywords")), "items": items }),
    }
}

/// 图片响应 → `TcResult`：顶层取裁决；明细取 Label/Object/Lib 三组里命中（`HitFlag==1`）的场景，
/// OCR 文本拼成 `ocr_text`。
fn parse_image(resp: &Value) -> TcResult {
    let mut items = Vec::new();
    for group in ["LabelResults", "ObjectResults", "LibResults"] {
        let Some(arr) = resp.get(group).and_then(Value::as_array) else { continue };
        for d in arr.iter().filter(|d| int_at(d, "HitFlag") == 1) {
            items.push(json!({
                "scene": str_at(d, "Scene"),
                "label": str_at(d, "Label"),
                "sub_label": str_at(d, "SubLabel"),
                "suggestion": str_at(d, "Suggestion"),
                "score": int_at(d, "Score"),
            }));
        }
    }

    let ocr_text = resp
        .get("OcrResults")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|d| d.get("Text").and_then(Value::as_str))
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default();

    TcResult {
        suggestion: str_at(resp, "Suggestion"),
        label: str_at(resp, "Label"),
        sub_label: str_at(resp, "SubLabel"),
        score: int_at(resp, "Score"),
        request_id: str_at(resp, "RequestId"),
        details: json!({ "ocr_text": ocr_text, "items": items }),
    }
}

/// 取字符串字段，缺省空串。
fn str_at(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or("").to_string()
}

/// 取整数字段，缺省 0。
fn int_at(v: &Value, key: &str) -> i32 {
    v.get(key).and_then(Value::as_i64).unwrap_or(0) as i32
}

/// 取字符串数组字段，缺省空表。
fn str_vec(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
        .unwrap_or_default()
}

/// 按 TC3-HMAC-SHA256 规范算出 `Authorization` 头值。
fn sign(
    secret_id: &str,
    secret_key: &str,
    host: &str,
    service: &str,
    date: &str,
    timestamp: i64,
    payload: &[u8],
) -> String {
    // 1. 规范请求：签名头固定 content-type;host，URI 为 /，查询串空。
    let hashed_payload = hex::encode(Sha256::digest(payload));
    let canonical_request =
        format!("POST\n/\n\ncontent-type:{CONTENT_TYPE}\nhost:{host}\n\ncontent-type;host\n{hashed_payload}");

    // 2. 待签字符串。
    let credential_scope = format!("{date}/{service}/tc3_request");
    let hashed_canonical = hex::encode(Sha256::digest(canonical_request.as_bytes()));
    let string_to_sign = format!("TC3-HMAC-SHA256\n{timestamp}\n{credential_scope}\n{hashed_canonical}");

    // 3. 逐级派生签名密钥。
    let secret_date = hmac(format!("TC3{secret_key}").as_bytes(), date.as_bytes());
    let secret_service = hmac(&secret_date, service.as_bytes());
    let secret_signing = hmac(&secret_service, b"tc3_request");
    let signature = hex::encode(hmac(&secret_signing, string_to_sign.as_bytes()));

    // 4. 拼 Authorization（逗号+空格分隔，与腾讯云规范字节对齐）。
    format!(
        "TC3-HMAC-SHA256 Credential={secret_id}/{credential_scope}, \
         SignedHeaders=content-type;host, Signature={signature}"
    )
}

/// HMAC-SHA256，返回原始字节。
fn hmac(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC 接受任意长度密钥");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

/// 进程级共享 HTTP 客户端（rustls，15s 超时）。
fn client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| reqwest::Client::builder().timeout(Duration::from_secs(15)).build().unwrap_or_default())
}
