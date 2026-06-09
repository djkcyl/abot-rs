//! 控制台内置 Provider —— 各自经 `inventory` 自注册一个 [`ConsolePlugin`](crate::web::registry::ConsolePlugin)。
//! 包括:`overview`(总览身份与统计)、`plugins`(只读插件清单)、`review`(通用审核框架)、`config`(配置读写)、
//! `database`(DB 表管理:增删改查)、`contacts`(好友/群管理)、`chatlog`(聊天记录查看)、
//! `messaging`(消息发送台)、`sessions`(会话/Token 管理)、`logs`(实时日志尾随)。

pub mod chatlog;
pub mod config;
pub mod contacts;
pub mod database;
pub mod logs;
pub mod messaging;
pub mod overview;
pub mod plugins;
pub mod sessions;
