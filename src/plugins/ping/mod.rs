//! ping 插件 —— 最简存活探针:`ping` → `pong`。
//!
//! 独立成插件(经 `plugin!{}` 登记):确认 bot 在线、dispatch 链路通。属工具类,
//! 无私有数据 / 逻辑,只有一条薄壳命令。设 `can_disable = false`——探针应当永远应答,
//! 不被「一键禁用全部」之类的操作误关。

use nagisa::prelude::*;

plugin! {
    key = "ping",
    name = "存活探针",
    category = Tool,
    can_disable = false,
    description = "发 ping 回 pong，确认机器人在不在线。",
}

/// `ping` → 回 `pong`:最简存活探针,确认 bot 在线且 dispatch 链路通。
#[command("ping", description = "测试机器人是否在线", usage = "发送「ping」，机器人在线会回复「pong」。")]
async fn ping(reply: Reply) -> HandlerResult {
    reply.text("pong").await?;
    Ok(())
}
