//! 腾讯云内容安全（CMS）签名与调用 —— TC3-HMAC-SHA256。
//!
//! 文本走 TextModeration（tms），图片走 ImageModeration（ims）。两者都按腾讯云 v3 签名规范
//! 自己拼规范请求、派生密钥、算签名，再带一组 `X-TC-*` 头 POST 上去。任何 HTTP / 解析 / API
//! 层面的错误都返 `Err`，由上层退回本地结果——签名错或服务抖动只是关掉 AI 这一路，不卡用户。

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

/// 腾讯云审核裁决：是否安全 + 风险大类 / 子类。
pub struct TcVerdict {
    pub safe: bool,
    pub label: String,
    pub sub_label: String,
}

/// 文本审核：host `tms.tencentcloudapi.com`，service `tms`。
pub async fn text_moderation(cfg: &TcConfig, text: &str) -> Result<TcVerdict> {
    let content = base64::engine::general_purpose::STANDARD.encode(text.as_bytes());
    let mut body = json!({ "Content": content });
    if let Some(biz) = cfg.text_biztype.as_deref().filter(|s| !s.is_empty()) {
        body["BizType"] = Value::String(biz.to_string());
    }
    call(cfg, "tms.tencentcloudapi.com", "tms", "TextModeration", &body).await
}

/// 图片审核：host `ims.tencentcloudapi.com`，service `ims`。用 base64 字节(FileContent)，不传 URL。
pub async fn image_moderation(cfg: &TcConfig, bytes: &[u8]) -> Result<TcVerdict> {
    let content = base64::engine::general_purpose::STANDARD.encode(bytes);
    let mut body = json!({ "FileContent": content });
    if let Some(biz) = cfg.image_biztype.as_deref().filter(|s| !s.is_empty()) {
        body["BizType"] = Value::String(biz.to_string());
    }
    call(cfg, "ims.tencentcloudapi.com", "ims", "ImageModeration", &body).await
}

/// 拼签名、发请求、解响应。任何环节出错都向上抛 `Err`。
async fn call(cfg: &TcConfig, host: &str, service: &str, action: &str, body: &Value) -> Result<TcVerdict> {
    let payload = serde_json::to_vec(body).context("序列化请求体失败")?;

    let timestamp =
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).context("系统时间早于 UNIX 纪元")?.as_secs()
            as i64;
    let date =
        chrono::DateTime::from_timestamp(timestamp, 0).context("时间戳无法转为日期")?.format("%Y-%m-%d").to_string();

    let authorization = sign(&cfg.secret_id, &cfg.secret_key, host, service, &date, timestamp, &payload);

    let url = format!("https://{host}/");
    let resp = client()
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
        .context("请求腾讯云失败")?;

    let text = resp.text().await.context("读取腾讯云响应失败")?;
    parse_response(&text)
}

/// 解析 `{"Response": {...}}`。有 `Error` 字段即视为失败（向上抛，触发本地兜底）。
fn parse_response(text: &str) -> Result<TcVerdict> {
    let v: Value = serde_json::from_str(text).context("腾讯云响应非 JSON")?;
    let resp = v.get("Response").context("响应缺少 Response 字段")?;

    if let Some(err) = resp.get("Error") {
        let code = err.get("Code").and_then(Value::as_str).unwrap_or("Unknown");
        let msg = err.get("Message").and_then(Value::as_str).unwrap_or("");
        bail!("腾讯云返回错误 {code}: {msg}");
    }

    let suggestion = resp.get("Suggestion").and_then(Value::as_str).unwrap_or("");
    let label = resp.get("Label").and_then(Value::as_str).unwrap_or("").to_string();
    let sub_label = resp.get("SubLabel").and_then(Value::as_str).unwrap_or("").to_string();

    Ok(TcVerdict { safe: suggestion == "Pass", label, sub_label })
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
