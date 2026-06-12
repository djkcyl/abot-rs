//! 运行时核心 Hub —— 持两注册表(按 key/event 索引)、连接表与广播。
//!
//! 每个 WS 连接由一个 task 拥有其 socket;Hub 给它配一个 `mpsc::UnboundedSender<ServerMsg>`
//! 作为「推送口」。连接 task `select!` 于 socket 入站与推送出站之间;DataService 刷新时
//! Hub 向所有有权连接的推送口发 `Data`。鉴权是**单一 intercept**:每端点带 authority,
//! 连接带 [`AuthUser`],调用/推送前统一判权。连接携带的 `?token=` 解析为真实 [`AuthUser`];
//! 无 token 或 token 已失效则 authority 为 0。

use crate::web::protocol::{ClientMsg, ServerMsg};
use crate::web::registry::{AuthUser, ConsoleRegistry, WebDataService, WebListener};
use axum::extract::ws::{Message, WebSocket};
use axum::extract::{Query, State, WebSocketUpgrade};
use axum::response::Response;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::{Mutex, mpsc};

/// 单个连接的推送口 + 鉴权态。
struct Client {
    tx: mpsc::UnboundedSender<ServerMsg>,
    auth: AuthUser,
}

/// WebUI 运行时核心。
pub struct Hub {
    data_services: HashMap<&'static str, Box<dyn WebDataService>>,
    listeners: HashMap<&'static str, Box<dyn WebListener>>,
    clients: Mutex<HashMap<u64, Client>>,
    next_id: AtomicU64,
    db: sea_orm::DatabaseConnection,
    login_gate: crate::web::auth::LoginGate,
    authority: crate::web::auth::AuthorityResolver,
}

impl Hub {
    /// 从收集好的注册表建 Hub。
    pub fn new(
        reg: ConsoleRegistry,
        db: sea_orm::DatabaseConnection,
        login_gate: crate::web::auth::LoginGate,
        authority: crate::web::auth::AuthorityResolver,
    ) -> Arc<Self> {
        let mut data_services: HashMap<&'static str, Box<dyn WebDataService>> = HashMap::new();
        for ds in reg.data_services {
            let key = ds.key();
            if data_services.contains_key(key) {
                tracing::error!(key, "重复的 WebDataService key,忽略后注册的那个");
                continue;
            }
            data_services.insert(key, ds);
        }
        let mut listeners: HashMap<&'static str, Box<dyn WebListener>> = HashMap::new();
        for l in reg.listeners {
            let event = l.event();
            if listeners.contains_key(event) {
                tracing::error!(event, "重复的 WebListener event,忽略后注册的那个");
                continue;
            }
            listeners.insert(event, l);
        }
        Arc::new(Self {
            data_services,
            listeners,
            clients: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            db,
            login_gate,
            authority,
        })
    }

    /// 控制台共享的数据库连接。
    pub fn db(&self) -> &sea_orm::DatabaseConnection {
        &self.db
    }
    /// 登录挑战门。
    pub fn login_gate(&self) -> &crate::web::auth::LoginGate {
        &self.login_gate
    }
    /// 权限解析器。
    pub fn authority(&self) -> &crate::web::auth::AuthorityResolver {
        &self.authority
    }

    /// 单点鉴权:访问者 authority 是否够到该端点所需。
    fn allowed(who: AuthUser, required: u8) -> bool {
        who.authority >= required
    }

    /// 重新取某 DataService 的全量并广播给有权连接。Provider 内容变化时调此方法。
    pub async fn refresh(&self, key: &str) {
        let Some(ds) = self.data_services.get(key) else {
            return;
        };
        let value = ds.get().await;
        let required = ds.authority();
        let clients = self.clients.lock().await;
        for client in clients.values() {
            if Self::allowed(client.auth, required) {
                let _ = client.tx.send(ServerMsg::Data { key: key.to_string(), value: value.clone() });
            }
        }
    }

    /// 从后台任务把一段增量推给所有有权连接(authority ≥ `required`)。日志泵据此批量推日志行。
    /// 与 [`refresh`](Self::refresh) 同款迭代/判权,但发的是 [`ServerMsg::Patch`]、且 value 由调用方给。
    pub async fn broadcast_patch(&self, key: &str, value: serde_json::Value, required: u8) {
        let clients = self.clients.lock().await;
        for client in clients.values() {
            if Self::allowed(client.auth, required) {
                let _ = client.tx.send(ServerMsg::Patch { key: key.to_string(), value: value.clone() });
            }
        }
    }

    /// 处理一个升级后的 WS 连接,直到对端断开或停机。
    async fn handle_socket(self: Arc<Self>, mut socket: WebSocket, token: Option<String>) {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, mut rx) = mpsc::unbounded_channel::<ServerMsg>();
        let auth = match token {
            Some(t) => crate::web::auth::lookup_token(&self.db, &t).await.unwrap_or(AuthUser { uin: 0, authority: 0 }),
            None => AuthUser { uin: 0, authority: 0 },
        };
        self.clients.lock().await.insert(id, Client { tx, auth });

        // 连接即推:把有权访问的 DataService 全量发一遍。
        for (key, ds) in &self.data_services {
            if Self::allowed(auth, ds.authority()) {
                let value = ds.get().await;
                if send(&mut socket, ServerMsg::Data { key: (*key).to_string(), value }).await.is_err() {
                    self.clients.lock().await.remove(&id);
                    return;
                }
            }
        }

        loop {
            tokio::select! {
                // 出站推送。
                pushed = rx.recv() => {
                    match pushed {
                        Some(msg) => { if send(&mut socket, msg).await.is_err() { break; } }
                        None => break,
                    }
                }
                // 入站消息。
                incoming = socket.recv() => {
                    match incoming {
                        Some(Ok(Message::Text(text))) => {
                            match serde_json::from_str::<ClientMsg>(text.as_str()) {
                                Ok(ClientMsg::Ping) => {
                                    if send(&mut socket, ServerMsg::Pong).await.is_err() { break; }
                                }
                                Ok(ClientMsg::Send { id: rpc_id, event, args }) => {
                                    let reply = self.dispatch_rpc(&event, args, auth).await;
                                    let msg = match reply {
                                        Ok(value) => ServerMsg::Response { id: rpc_id, value: Some(value), error: None },
                                        Err(error) => ServerMsg::Response { id: rpc_id, value: None, error: Some(error) },
                                    };
                                    if send(&mut socket, msg).await.is_err() { break; }
                                }
                                Err(_) => { /* 非法帧:忽略 */ }
                            }
                        }
                        Some(Ok(Message::Close(_))) | None => break,
                        Some(Ok(_)) => { /* 二进制/ping/pong 帧:忽略 */ }
                        Some(Err(_)) => break,
                    }
                }
            }
        }

        self.clients.lock().await.remove(&id);
    }

    /// 派发一次 RPC:找 listener、判权、调用。
    async fn dispatch_rpc(
        &self,
        event: &str,
        args: serde_json::Value,
        who: AuthUser,
    ) -> Result<serde_json::Value, String> {
        let Some(listener) = self.listeners.get(event) else {
            return Err(format!("未知请求:{event}"));
        };
        if !Self::allowed(who, listener.authority()) {
            return Err("权限不足".to_string());
        }
        listener.handle(args, who).await
    }
}

/// 序列化并发一条 ServerMsg。
async fn send(socket: &mut WebSocket, msg: ServerMsg) -> Result<(), axum::Error> {
    let text = serde_json::to_string(&msg).unwrap_or_else(|_| "{}".to_string());
    socket.send(Message::Text(text.into())).await
}

/// axum WS 升级入口:升级后交给 `Hub::handle_socket`。
pub async fn ws_handler(
    ws: WebSocketUpgrade,
    Query(q): Query<HashMap<String, String>>,
    State(hub): State<Arc<Hub>>,
) -> Response {
    let token = q.get("token").cloned();
    ws.on_upgrade(move |socket| hub.handle_socket(socket, token))
}
