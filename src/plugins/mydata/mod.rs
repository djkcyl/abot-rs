//! 个人数据插件 —— 命令 `个人数据` / `我的` / `资料`，展示用户的金币/经验/等级/发言数，
//! 再加上各插件经 [`ProfileSection`](crate::data::profile::ProfileSection) 自注册贡献的行
//! (如签到的连签)。命令-only(无自有表)：核心数据走 `AUser`，插件数据走 [`collect_grouped`]，
//! 故本插件**不**引用任何具体插件、也不破插件自有数据的墙。

use nagisa::prelude::*;

use crate::data::{collect_grouped, AUser};
use crate::COIN_NAME;

plugin! {
    key = "mydata",
    name = "个人数据",
    category = Tool,
    description = "查看个人数据",
    usage = "发送「个人数据」，查看自己的金币、经验、等级、签到、发言等。",
}

/// `个人数据` / `我的` / `资料` → 汇总展示发送者的数据(核心字段 + 各插件贡献行)。
#[command("个人数据", "我的数据", "我的", "资料", "mydata",
    description = "查看自己的数据",
    usage = "发送「个人数据」（或「我的」「资料」）查看金币、经验、等级，以及签到、发言等各功能的累计数据，赛马等游戏战绩另起一段显示。")]
async fn mydata(reply: Reply, user: AUser) -> HandlerResult {
    let info = user.level_info();
    let name = user.model.nickname.clone().unwrap_or_else(|| user.uin().to_string());

    let mut lines = vec![
        format!("{name} 的数据"),
        format!("{COIN_NAME}：{}", user.coin()),
        format!(
            "等级：Lv.{}（{}/{}），经验 {}",
            info.level, info.into_level, info.level_span, user.exp()
        ),
    ];
    // 各插件自注册的贡献行,按分组分桶:普通统计(签到/发言…)直接接在后面;
    // 游戏战绩(赛马…)另起一段,不和普通数据混在一起。
    let grouped = collect_grouped(user.db(), user.uin()).await;
    lines.extend(grouped.stats);
    if !grouped.games.is_empty() {
        lines.push("战绩".to_string());
        lines.extend(grouped.games);
    }

    // 回复触发的原消息(quote)。
    reply.reply(lines.join("\n")).await?;
    Ok(())
}
