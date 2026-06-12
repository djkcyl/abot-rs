//! 插件层 —— 各业务插件的挂载点。
//!
//! 「一个模块 = 一个插件」:每个插件模块在自己 `mod.rs` 顶部用 `plugin!{}` 登记一份
//! `PluginSpec`(名字 / 类别 / 主开关策略),其下的 `#[command]` / `#[event]` 触发器按
//! 最长模块前缀自动归属本插件。两类登记都经 `inventory` 编译期收集,`App::new()` 一次性
//! 收齐,无需在 `main` 里显式 `.command(..)`。
//!
//! 本文件**只**声明子模块、不直接挂任何裸命令——裸命令会落到无名的 `abot::plugins`
//! 伪插件上,既无身份也无主开关。可读事件日志同理不在这里手搓,它是
//! `nagisa::log::EventLog` 观察者,由 `main` 挂在 `App::on_top`。跨插件共用的
//! 呈现小助手(如 `display_name`)放这里,不算裸命令。
//!
//! `main` 对 `plugins::*` 的 glob-use 保活整棵模块树,触发器 / 插件方得自动收录。

use nagisa::prelude::MessageEvent;

pub mod bottle;
pub mod chatlog;
pub mod help;
pub mod mydata;
pub mod online;
pub mod ping;
pub mod place;
pub mod sign;
pub mod theme;
pub mod transfer;

/// 发送者显示名：群名片/昵称，私聊好友备注/昵称；都取不到用 QQ 号串。
pub(crate) fn display_name(m: &MessageEvent, uin: i64) -> String {
    m.member
        .as_ref()
        .map(|mi| mi.display_name())
        .or_else(|| m.friend.as_ref().map(|f| f.display_name()))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| uin.to_string())
}
