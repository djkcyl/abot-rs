//! 上线通知插件 —— bot 就绪后给主人私聊发一条上线通知。
//!
//! 监听框架的 `Ready` 生命周期事件:登录解析出**可用账号**(`self_id != 0`)后,框架
//! 发一次 `Ready`(无可用账号则根本不发),故本逻辑天然以「有可用账号」为前提——
//! 没账号就不跑。这正是 nagisa「插件就绪后启动」契约的样板:启动逻辑 = `#[event(Ready)]`。
//!
//! 主人 QQ 经 [`Master`] 状态由 `main` 注入(来自 `Config.master`),
//! handler 经 `State<Master>` 取用;`Master(Uin(0))` 表示无主人,直接跳过。

use nagisa::prelude::*;

use crate::config::Master;

plugin! {
    key = "online",
    name = "上线通知",
    category = Push,
    // 后台插件(只给主人私聊),不进用户菜单。
    hidden = true,
    description = "上线通知",
}

/// bot 就绪(有可用账号)→ 给主人私聊发上线通知(账号 / 已加载插件数 / 命令数 / 本地时间)。
///
/// `master.0.0 == 0`(无主人)直接跳过;发送失败经 `?` 上抛由 dispatch 记 warn(止于此)。
#[event(Ready, id = "notice")]
async fn notice(ready: Ready, bot: Bot, State(master): State<Master>) -> HandlerResult {
    let owner: Uin = master.0; // Arc<Master> → Master(Uin)：.0 即主人 Uin
    if owner.0 == 0 {
        return Ok(()); // 无主人,跳过
    }
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
    // 只数命令型触发器(registered_triggers 含事件型,不能直接当命令数)。
    let commands = nagisa::registered_triggers()
        .iter()
        .filter(|t| matches!(t.kind, nagisa::TriggerKind::Command))
        .count();
    let text = format!(
        "abot 已上线\n账号 {}\n插件 {}，命令 {}\n{now}",
        ready.self_id.0,
        nagisa::registered_plugins().len(),
        commands,
    );
    bot.send(&Peer::friend(owner), &[Segment::text(text)]).await?;
    tracing::info!(master = owner.0, "已向主人发送上线通知");
    Ok(())
}
