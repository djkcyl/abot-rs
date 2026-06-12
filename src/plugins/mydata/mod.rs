//! 个人数据插件 —— 命令 `个人数据` / `我的` / `资料`，展示用户的金币/经验/等级/发言数，
//! 再加上各插件经 [`ProfileSection`](crate::data::profile::ProfileSection) 自注册贡献的行
//! (如签到的连签)。命令-only(无自有表)：核心数据走 `AUser`，插件数据走 [`collect_grouped`]，
//! 故本插件**不**引用任何具体插件、也不破插件自有数据的墙。汇总渲成卡片图回复
//! ([`render`],渲不出退文字)。

pub mod render;

use nagisa::prelude::*;

use crate::COIN_NAME;
use crate::data::{AUser, collect_grouped};
use crate::plugins::display_name;

plugin! {
    key = "mydata",
    name = "个人数据",
    category = Tool,
    description = "游戏币、经验、等级和各功能的累计，汇总成一张卡片。",
}

/// `个人数据` / `我的` / `资料` → 汇总展示发送者的数据(核心字段 + 各插件贡献行)。
///
/// 核心字段取自 `AUser`,插件行经 [`collect_grouped`] 收集,拼成 [`render::MyDataCard`]
/// 渲卡片图引用回复;渲染失败退文字(信息同卡片)。
#[command(
    "个人数据",
    "我的数据",
    "我的",
    "资料",
    "mydata",
    description = "查看自己的数据",
    usage = "发送「个人数据」（或「我的」「资料」）查看金币、经验、等级，以及签到、发言等各功能的累计数据，赛马等游戏战绩另起一段显示。"
)]
async fn mydata(reply: Reply, user: AUser, m: MessageEvent) -> HandlerResult {
    // 各插件自注册的贡献行,按分组分桶:普通统计(签到/发言…)与游戏战绩(画板…)分段。
    let grouped = collect_grouped(user.db(), user.uin()).await;

    let card = render::MyDataCard {
        name: display_name(&m, user.uin()),
        uid: user.id(),
        uin: user.uin(),
        avatar: crate::imaging::qq_avatar(user.uin()).await,
        coin: user.coin(),
        exp: user.exp(),
        level: user.level_info(),
        stats: grouped.stats,
        games: grouped.games,
        theme: user.render_theme(),
    };
    match render::card_image(&card) {
        Ok(webp) => {
            // 回复触发的原消息(quote)。
            reply.msg().image_bytes(webp).quote().await?;
        }
        Err(e) => {
            tracing::warn!(error = %e, "渲染个人数据卡片失败,退回文字");
            reply.reply(text_summary(&card)).await?;
        }
    }
    Ok(())
}

/// 卡片的文字退路(渲染失败时用,信息同卡片)。
fn text_summary(c: &render::MyDataCard) -> String {
    let mut lines = vec![
        format!("{} 的数据", c.name),
        format!("{COIN_NAME}：{}", c.coin),
        format!("等级：Lv.{}（{}/{}），经验 {}", c.level.level, c.level.into_level, c.level.level_span, c.exp),
    ];
    lines.extend(c.stats.iter().cloned());
    if !c.games.is_empty() {
        lines.push("战绩".to_string());
        lines.extend(c.games.iter().cloned());
    }
    lines.join("\n")
}
