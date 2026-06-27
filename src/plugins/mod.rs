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

use crate::data::AUser;

pub mod admin;
pub mod bottle;
pub mod chatlog;
pub mod help;
pub mod horse;
pub mod mcping;
pub mod mydata;
pub mod nickname;
pub mod online;
pub mod ping;
pub mod place;
pub mod rank;
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

/// 出图用的显示名:文字 + 可选自设颜色。`color` 非空仅当文字取自用户**自设昵称**
/// (`alias`)——此时出图给它上色(经 [`imaging::readable_hex`](crate::imaging::readable_hex)
/// 收对比);退到群名片 / 账号昵称时无色(那不是「自定义昵称」)。
pub(crate) struct ShownName {
    /// 显示文字。
    pub text: String,
    /// 自设颜色(`#rrggbb` 原始色相,空 = 用缺省文字色)。
    pub color: String,
}

/// 发送者出图显示名(自己卡片用):设了自设昵称就用它、带自设颜色;否则退群名片 / 账号
/// 昵称(无色)。签到卡 / 个人数据卡等「展示自己」的出图点统一走它——故自定义昵称与颜色
/// 在自己的卡片上也现身,与排行榜「列出别人」口径([`rank::pick_name`](crate::plugins::rank))一致。
pub(crate) fn self_shown_name(user: &AUser, m: &MessageEvent) -> ShownName {
    let alias = user.alias().trim();
    if alias.is_empty() {
        ShownName { text: display_name(m, user.uin()), color: String::new() }
    } else {
        ShownName { text: alias.to_string(), color: user.alias_color().to_string() }
    }
}

/// 取消词面统一判定:`n` / `no` / `否` / `不` / `取消` / `算了` / `退出` … 任一即视作取消。多处交互
/// (y/n 确认、发图收尾、主题/坐标追问)共用,免得各写各的、词面覆盖不一。
pub(crate) fn is_cancel(s: &str) -> bool {
    matches!(
        s.trim().to_lowercase().as_str(),
        "n" | "no" | "否" | "不" | "不要" | "取消" | "算了" | "退出" | "cancel" | "quit"
    )
}

/// 肯定词面:`y` / `yes` / `是` / `确认` / `好` / `行` … 任一即视作确认。与 [`is_cancel`] 互补,
/// 喂 y/n 确认流(肯定→继续、取消→中止、其余→重问)。
pub(crate) fn is_yes(s: &str) -> bool {
    matches!(
        s.trim().to_lowercase().as_str(),
        "y" | "yes" | "是" | "确认" | "嗯" | "好" | "好的" | "行" | "可以" | "ok"
    )
}
