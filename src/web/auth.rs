//! 网页控制台鉴权 —— QQ 验证码登录 + token 存取 + 权限解析。
//!
//! 流程:网页 `POST /api/login/challenge` 取 6 位验证码([`LoginGate::challenge`] 存进带 TTL
//! 的 [`Rendezvous`],值 `Uin(0)`=待批准);用户私聊 bot 发 `登录 <码>`,登录命令把该码
//! 批准为发送者 uin([`LoginGate::approve`]);网页轮询 `POST /api/login/poll`
//! ([`LoginGate::poll`])拿到已批准 uin → 签发 token 存 `web_token`。WS 握手带 `?token=`,
//! [`crate::web::hub::Hub`] 查 token 得 [`AuthUser`]。

use nagisa::Rendezvous;
use nagisa::prelude::*;
use sea_orm::{ActiveModelTrait, ActiveValue::Set, DatabaseConnection, EntityTrait};
use std::collections::HashSet;
use std::sync::Arc;

use crate::web::entity;
use crate::web::registry::AuthUser;

/// 登录挑战门:验证码 → `Uin`(`Uin(0)`=待批准,非 0=已批准)。复用 `Rendezvous` 的 TTL
/// 存储。独立 newtype,避免与 App 默认 `Rendezvous<String,Uin>` 在 state map 按类型撞键。
#[derive(Clone)]
pub struct LoginGate {
    inner: Arc<Rendezvous<String, Uin>>,
}

impl Default for LoginGate {
    fn default() -> Self {
        Self::new()
    }
}

impl LoginGate {
    pub fn new() -> Self {
        Self { inner: Arc::new(Rendezvous::default()) }
    }
    /// 网页取码:存 `code → Uin(0)`(待批准)。
    pub fn challenge(&self, code: String) {
        self.inner.issue(code, Uin(0));
    }
    /// `登录` 命令批准:仅当该码尚存(未过期)时改存真实 uin,返回是否成功。
    pub fn approve(&self, code: &str, uin: Uin) -> bool {
        if self.inner.peek(&code.to_string()).is_some() {
            self.inner.issue(code.to_string(), uin);
            true
        } else {
            false
        }
    }
    /// 网页轮询:已批准(值非 0)则取走并返回 uin;否则 `None`。
    pub fn poll(&self, code: &str) -> Option<Uin> {
        match self.inner.peek(&code.to_string()) {
            Some(u) if u.0 != 0 => {
                self.inner.claim(&code.to_string());
                Some(u)
            }
            _ => None,
        }
    }
}

/// uin → authority:master=5、superuser=4、其余=1。
#[derive(Clone)]
pub struct AuthorityResolver {
    master: Uin,
    superusers: Arc<HashSet<Uin>>,
}

impl AuthorityResolver {
    pub fn new(master: Uin, superusers: HashSet<Uin>) -> Self {
        Self { master, superusers: Arc::new(superusers) }
    }
    pub fn of(&self, uin: Uin) -> u8 {
        if self.master.0 != 0 && uin == self.master {
            5
        } else if self.superusers.contains(&uin) {
            4
        } else {
            1
        }
    }
}

/// 随机 token(32 位十六进制)。
pub fn random_token() -> String {
    use rand::RngExt;
    let mut rng = rand::rng();
    (0..16).map(|_| format!("{:02x}", rng.random::<u8>())).collect()
}

/// 签发 token:写 `web_token`(有效期 7 天),返回 token。
pub async fn issue_token(
    db: &DatabaseConnection,
    uin: Uin,
    authority: u8,
) -> std::result::Result<String, sea_orm::DbErr> {
    let token = random_token();
    let now = chrono::Utc::now().fixed_offset();
    let expires = now + chrono::Duration::days(7);
    entity::ActiveModel {
        token: Set(token.clone()),
        uin: Set(uin.0),
        authority: Set(authority as i16),
        created_at: Set(now),
        expires_at: Set(expires),
    }
    .insert(db)
    .await?;
    Ok(token)
}

/// 查 token → `AuthUser`。不存在或已过期 → `None`。
pub async fn lookup_token(db: &DatabaseConnection, token: &str) -> Option<AuthUser> {
    let now = chrono::Utc::now().fixed_offset();
    let m = entity::Entity::find_by_id(token.to_string()).one(db).await.ok().flatten()?;
    if m.expires_at <= now {
        return None;
    }
    Some(AuthUser { uin: m.uin, authority: m.authority as u8 })
}

/// `登录 <验证码>`(仅私聊)—— 把网页验证码批准为发送者的 QQ 身份。
#[command(
    "登录",
    description = "绑定网页控制台登录",
    usage = "在网页控制台点登录拿到验证码后,私聊机器人发送「登录 验证码」即可批准这次网页登录。验证码有过期时间,过期就回网页重新取。"
)]
async fn login(
    _pm: PrivateMessage,
    reply: Reply,
    Sender(uin): Sender,
    args: ArgText,
    State(gate): State<LoginGate>,
) -> HandlerResult {
    let code = args.0.trim();
    if code.is_empty() {
        reply.reply("请发「登录 <验证码>」。").await?;
        return Ok(());
    }
    if gate.approve(code, uin) {
        reply.reply("登录已确认，回到网页即可。").await?;
    } else {
        reply.reply("验证码已过期，请在网页重新获取。").await?;
    }
    Ok(())
}

use axum::Json;
use axum::extract::State as AxumState;
use serde_json::{Value, json};

use crate::web::hub::Hub;

/// `POST /api/login/challenge` → 生成 6 位验证码,存进 LoginGate(待批准),返回给网页。
pub async fn login_challenge(AxumState(hub): AxumState<Arc<Hub>>) -> Json<Value> {
    let code = {
        use rand::RngExt;
        let mut rng = rand::rng();
        format!("{:06}", rng.random_range(0..1_000_000u32))
    };
    hub.login_gate().challenge(code.clone());
    Json(json!({ "code": code, "hint": format!("登录 {code}") }))
}

/// `POST /api/login/poll` body `{"code":"..."}` → 已批准则签发 token 返回 `{token, authority}`,
/// 否则 `{token: null}`(网页继续轮询直到超时)。
pub async fn login_poll(AxumState(hub): AxumState<Arc<Hub>>, Json(body): Json<Value>) -> Json<Value> {
    let code = body.get("code").and_then(|v| v.as_str()).unwrap_or("");
    match hub.login_gate().poll(code) {
        Some(uin) => {
            let authority = hub.authority().of(uin);
            match issue_token(hub.db(), uin, authority).await {
                Ok(token) => Json(json!({ "token": token, "authority": authority })),
                Err(_) => Json(json!({ "token": Value::Null, "error": "登录失败，请稍后重试" })),
            }
        }
        None => Json(json!({ "token": Value::Null })),
    }
}

// 登记网页控制台为一个插件,使 `登录` 命令有归属。
plugin! {
    key = "webconsole",
    name = "网页控制台",
    category = Tool,
}
