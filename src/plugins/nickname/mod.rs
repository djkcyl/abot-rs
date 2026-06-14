//! 改名插件 —— 命令 `改名` / `设置昵称` 给自己设一个**自设昵称**(`user.alias`),`昵称颜色`
//! 给这个昵称挑个**颜色**(`user.alias_color`)。两者都是核心 `user` 的共享字段,经
//! [`AUser::set_alias`] / [`AUser::set_alias_color`] 落库。
//!
//! 自设昵称呈现优先级最高:凡「展示这个人」的出图点(签到卡、个人数据卡、排行榜等),设了
//! 自设昵称就显示它、否则才退账号昵称 / 群名片(见 `data::identity` 与 `plugins::rank`、
//! `plugins::self_shown_name`)。设了颜色的,这些出图点显示昵称时还按颜色上色(经
//! [`imaging::readable_hex`](crate::imaging::readable_hex) 收对比,亮暗都立得住)。
//!
//! 颜色从预设里挑(八色,见 `NAMED_COLORS`);`昵称颜色`(不带参)出一张色板预览图,所见即卡片所得。
//!
//! 改名收费、上色更贵——给个性化加一道游戏币门槛,也是经济的一个去处。清除与查看免费。

use nagisa::prelude::*;
use nagisa::render::{Align, Doc, Insets, render_document};

use crate::data::AUser;
use crate::imaging::{UserTheme, readable_hex};

plugin! {
    key = "nickname",
    name = "改名",
    category = User,
    description = "给自己设个昵称、挑个颜色，签到卡和排行榜等处都按它显示。",
}

/// 自设昵称的字数上限(按字符计,CJK 一字一计)。
const ALIAS_MAX_CHARS: usize = 16;

/// 改名花费(游戏币)。
const RENAME_COST: i64 = 50;
/// 上色花费(游戏币)——比改名贵,颜色是更进一步的个性化。
const COLOR_COST: i64 = 200;

/// 自设昵称可选的预设颜色(名 → `#rrggbb` 色相基准),八色绕色环均匀铺开。出图时经
/// [`imaging::readable_hex`](crate::imaging::readable_hex) 按亮暗收对比。
const NAMED_COLORS: &[(&str, &str)] = &[
    ("红", "#e23b3b"),
    ("橙", "#e07a1f"),
    ("黄", "#d8a200"),
    ("绿", "#36a94a"),
    ("青", "#15b3a4"),
    ("蓝", "#3b7be0"),
    ("紫", "#8a5cc4"),
    ("粉", "#e06699"),
];

/// 预设颜色一览(提示语用)。
fn color_names() -> String {
    NAMED_COLORS.iter().map(|(n, _)| *n).collect::<Vec<_>>().join(" / ")
}

/// 颜色的中文呈现:命中预设出名字,否则原样回十六进制(兜底,正常只会收到预设)。
fn color_label(hex: &str) -> String {
    NAMED_COLORS
        .iter()
        .find(|(_, h)| h.eq_ignore_ascii_case(hex))
        .map(|(n, _)| (*n).to_string())
        .unwrap_or_else(|| hex.to_string())
}

/// 一句颜色输入解析的结果。
enum ColorInput {
    /// 设成这个预设色(`#rrggbb`)。
    Set(String),
    /// 清除上色。
    Clear,
    /// 认不出(不在预设里)。
    Unknown,
}

/// 解析一句颜色输入:清除词 / 预设色名(可带「色」尾);都不匹配 → [`ColorInput::Unknown`]。
fn parse_color(s: &str) -> ColorInput {
    let s = s.trim();
    if matches!(s, "清除" | "取消" | "默认" | "去色" | "无") {
        return ColorInput::Clear;
    }
    let name = s.strip_suffix('色').unwrap_or(s);
    match NAMED_COLORS.iter().find(|(n, _)| *n == name) {
        Some((_, hex)) => ColorInput::Set((*hex).to_string()),
        None => ColorInput::Unknown,
    }
}

/// 渲一张昵称颜色色板预览图:八个预设各一格(原色色条 + 色名按本次亮暗收对比上色——所见
/// 即卡片所得),当前色打勾;整卡按用户现行主题出图。渲不出由调用方退文字。`current` 是当前
/// 自设颜色(空 = 未上色),`has_alias` 决定底部提示是「换色」还是「先设昵称」。
fn palette_card(theme: &UserTheme, current: &str, has_alias: bool) -> anyhow::Result<Vec<u8>> {
    let pal = &theme.palette;
    let dark = theme.dark;
    let mut d = Doc::new();

    d.paragraph(|p| {
        p.align(Align::Center).styled("昵称颜色", |s| {
            s.weight(600).size(1.2);
        });
    });
    // 现状行:当前色名上主色加重,其余辅助灰。
    d.paragraph(|p| {
        p.align(Align::Center)
            .styled("现在：", |s| {
                s.color(&pal.muted).size(0.85);
            })
            .styled(if current.is_empty() { "未上色".to_string() } else { color_label(current) }, |s| {
                s.color(&pal.primary).weight(600).size(0.85);
            });
    });

    // 两行 × 四列预设色卡:每格一条原色色块 + 一行色名(按本次亮暗收对比上色)。
    for chunk in NAMED_COLORS.chunks(4) {
        d.columns(|cols| {
            cols.gap(14.0);
            for (name, hex) in chunk {
                let is_cur = current.eq_ignore_ascii_case(hex);
                let readable = readable_hex(hex, dark);
                cols.col(|c| {
                    c.progress(1.0, |b| {
                        b.height(22.0).fill(hex).radius(4.0);
                    });
                    c.paragraph(|p| {
                        p.align(Align::Center);
                        let label = if is_cur { format!("✓ {name}") } else { (*name).to_string() };
                        p.styled(label, |s| {
                            s.weight(600).size(0.95);
                            if let Some(col) = &readable {
                                s.color(col);
                            }
                        });
                    });
                });
            }
        });
    }

    // 底部用法:已有昵称给换色提示,否则先催设昵称。
    let tip = if has_alias {
        format!("发送「昵称颜色 蓝」之类换色，要 {COLOR_COST} 游戏币；「昵称颜色 清除」去色")
    } else {
        format!("先发「改名 <昵称>」设个昵称，再来上色（要 {COLOR_COST} 游戏币）")
    };
    d.paragraph(|p| {
        p.align(Align::Center).styled(tip, |s| {
            s.color(&pal.muted).size(0.85);
        });
    });

    Ok(render_document(&d.build(), &theme.opts().with_padding(Insets::symmetric(28.0, 34.0)))?)
}

/// `改名` 的参数:尾随的昵称(保真收尾,handler 去空白)。
#[derive(Args)]
struct NameArgs {
    /// 要设的昵称;填「清除」可取消,不填则看当前。
    #[arg(rest, raw, name = "昵称", desc = "要设的昵称；填「清除」取消，不填看当前")]
    text: String,
}

/// `昵称颜色` 的参数:尾随的颜色词。
#[derive(Args)]
struct ColorArgs {
    /// 预设颜色名;填「清除」去色,不填看色板预览。
    #[arg(rest, raw, name = "颜色", desc = "预设颜色名；填「清除」去色，不填看色板")]
    text: String,
}

/// `改名` / `设置昵称` —— 设 / 改 / 清除自设昵称;不带参看当前。改名收费、清除与查看免费。
#[command(
    "改名",
    "设置昵称",
    description = "设置自设昵称",
    usage = "发送「改名 <昵称>」给自己设个昵称（花 50 游戏币），签到卡、排行榜等出图处都优先显示它；发送「改名 清除」取消（免费），发送「改名」看当前。设好后可发「昵称颜色」给它上色。"
)]
async fn rename(reply: Reply, mut user: AUser, args: Args<NameArgs>) -> HandlerResult {
    let raw = args.0.text.trim();

    // 不带参:看当前。
    if raw.is_empty() {
        let msg = if user.alias().is_empty() {
            format!("你还没设昵称。发送「改名 <昵称>」设一个，花 {RENAME_COST} 游戏币。")
        } else {
            let color = if user.alias_color().is_empty() {
                String::new()
            } else {
                format!("，颜色{}", color_label(user.alias_color()))
            };
            format!("当前昵称：{}{color}。发送「改名 <新昵称>」改，或「改名 清除」取消。", user.alias())
        };
        reply.reply(msg).await?;
        return Ok(());
    }

    // 清除:免费(没买东西)。颜色留着,下次再设昵称仍是这个色。
    if raw == "清除" || raw == "取消" {
        if user.alias().is_empty() {
            reply.reply("你本来就没设昵称。").await?;
        } else {
            user.set_alias("").await?;
            reply.reply("已清除，之后显示账号昵称或群名片。").await?;
        }
        return Ok(());
    }

    // 字数关。
    if raw.chars().count() > ALIAS_MAX_CHARS {
        reply.reply(format!("昵称太长，最多 {ALIAS_MAX_CHARS} 个字。")).await?;
        return Ok(());
    }

    // 与现昵称相同:不收费、不重复落库。
    if user.alias() == raw {
        reply.reply("你的昵称已经是这个了。").await?;
        return Ok(());
    }

    // 收费改名:带闸扣费,不够就明说。
    if !user.pay(RENAME_COST, "改名").await? {
        reply.reply(format!("改名要 {RENAME_COST} 游戏币，你只有 {}，不够。", user.coin())).await?;
        return Ok(());
    }
    user.set_alias(raw).await?;
    reply.reply(format!("好，以后叫你{raw}，花了 {RENAME_COST} 游戏币，余额 {}。", user.coin())).await?;
    Ok(())
}

/// `昵称颜色` / `名字颜色` —— 给自设昵称设 / 改 / 清除颜色;不带参出色板预览图。上色收费、
/// 清除与查看免费;只收预设八色、须先有昵称才谈上色。
#[command(
    "昵称颜色",
    "名字颜色",
    description = "给自设昵称上色",
    usage = "发送「昵称颜色 <颜色>」给昵称上色（花 200 游戏币，比改名贵），之后所有出图处的昵称都带这个色；颜色八选一：红／橙／黄／绿／青／蓝／紫／粉。发送「昵称颜色」看色板预览，「昵称颜色 清除」去色（免费）。要先有昵称才能上色。"
)]
async fn nickname_color(reply: Reply, mut user: AUser, args: Args<ColorArgs>) -> HandlerResult {
    let raw = args.0.text.trim();

    // 不带参:出一张色板预览图(渲不出退文字)。
    if raw.is_empty() {
        let theme = user.render_theme();
        match palette_card(&theme, user.alias_color(), !user.alias().is_empty()) {
            Ok(webp) => {
                reply.msg().image_bytes(webp).quote().await?;
            }
            Err(e) => {
                tracing::warn!(error = %e, "渲染昵称色板失败,退回文字");
                let cur = if user.alias_color().is_empty() {
                    "未上色".to_string()
                } else {
                    color_label(user.alias_color())
                };
                reply
                    .reply(format!(
                        "当前昵称颜色：{cur}。可选：{}；发送「昵称颜色 <颜色>」设（花 {COLOR_COST} 游戏币），「昵称颜色 清除」去色。",
                        color_names()
                    ))
                    .await?;
            }
        }
        return Ok(());
    }

    match parse_color(raw) {
        // 清除:免费。
        ColorInput::Clear => {
            if user.alias_color().is_empty() {
                reply.reply("你的昵称本来就没上色。").await?;
            } else {
                user.set_alias_color("").await?;
                reply.reply("已去色，昵称恢复缺省颜色。").await?;
            }
        }
        // 认不出:给可选项,不回显用户输入。
        ColorInput::Unknown => {
            reply.reply(format!("没这个颜色，可选：{}。发送「昵称颜色」看色板。", color_names())).await?;
        }
        // 设 / 改:先得有昵称、再带闸收费。
        ColorInput::Set(hex) => {
            if user.alias().is_empty() {
                reply.reply(format!("先设个昵称再上色，发送「改名 <昵称>」（花 {RENAME_COST} 游戏币）。")).await?;
                return Ok(());
            }
            if user.alias_color().eq_ignore_ascii_case(&hex) {
                reply.reply("你的昵称已经是这个颜色了。").await?;
                return Ok(());
            }
            if !user.pay(COLOR_COST, "昵称颜色").await? {
                reply.reply(format!("上色要 {COLOR_COST} 游戏币，你只有 {}，不够。", user.coin())).await?;
                return Ok(());
            }
            user.set_alias_color(&hex).await?;
            reply
                .reply(format!(
                    "好，昵称颜色设成{}，花了 {COLOR_COST} 游戏币，余额 {}。之后出图就用这个颜色。",
                    color_label(&hex),
                    user.coin()
                ))
                .await?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 预设色名 / 带「色」尾 / 清除词各走对分支;自定义十六进制与已砍的色都判 Unknown。
    #[test]
    fn color_parsing() {
        assert!(matches!(parse_color("红"), ColorInput::Set(h) if h == "#e23b3b"));
        assert!(matches!(parse_color("红色"), ColorInput::Set(h) if h == "#e23b3b"));
        assert!(matches!(parse_color("  蓝  "), ColorInput::Set(h) if h == "#3b7be0"));
        assert!(matches!(parse_color("清除"), ColorInput::Clear));
        assert!(matches!(parse_color("去色"), ColorInput::Clear));
        // 只认预设:自定义十六进制、砍掉的色、生造的词都认不出。
        assert!(matches!(parse_color("#ff8800"), ColorInput::Unknown));
        assert!(matches!(parse_color("灰"), ColorInput::Unknown));
        assert!(matches!(parse_color("彩虹"), ColorInput::Unknown));
    }

    /// 预设八色齐整(数量 + 各自是合法 `#rrggbb`),回显大小写不敏感。
    #[test]
    fn preset_palette() {
        assert_eq!(NAMED_COLORS.len(), 8);
        for (_, hex) in NAMED_COLORS {
            assert!(crate::imaging::readable_hex(hex, false).is_some(), "{hex} 应是合法预设色");
        }
        assert_eq!(color_label("#e23b3b"), "红");
        assert_eq!(color_label("#E23B3B"), "红");
    }
}
