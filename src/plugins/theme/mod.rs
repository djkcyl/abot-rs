//! 主题插件 —— 出图卡片的个人偏好，一条 `主题` 命令管亮暗与主题色：`主题 暗`、
//! `主题 珊瑚粉`、`主题 暗 珊瑚粉` 一次设齐；不带参数进交互式配置（出色板等回复）。
//!
//! 偏好存核心 `user.theme` / `user.theme_color` 列（跨插件共享——所有出图点都按它走），
//! 出图点经 [`imaging::UserTheme`] 解析成标准色卡。亮暗取值 `auto`（缺省，按当月典型
//! 日出日落天黑走暗，见 [`imaging::pick_dark`]）/ `light` / `dark`；主题色取值为
//! [`imaging::THEMES`] 五套之一的键（空串走缺省远黛蓝）。回执渲成色板（五套主题各
//! 一列，上亮下暗两组色样 + 名字标），渲不出退文字。

use nagisa::prelude::*;
use nagisa::render::{Align, Doc, Insets, render_document};

use crate::data::AUser;
use crate::imaging::{self, UserTheme};

plugin! {
    key = "theme",
    name = "主题",
    category = Tool,
    description = "挑一套出图的主题色和亮暗，给你出的图都按这套来。",
}

/// 一次输入词解析出的一项设置。
enum Setting {
    /// 亮暗（库值 `auto` / `light` / `dark`）。
    Mode(&'static str),
    /// 主题色（库键，空串 = 缺省远黛蓝）。
    Color(&'static str),
}

/// 输入词 → 设置。亮暗收中英文与常见同义词；主题色收全名 / 旧名 / 单字 / 键（可带
/// 「色」尾），`默认` 清空。认不出返 `None`。
fn parse_setting(s: &str) -> Option<Setting> {
    let s = s.trim().to_lowercase();
    match s.as_str() {
        "亮" | "亮色" | "浅色" | "白天" | "light" => return Some(Setting::Mode("light")),
        "暗" | "暗色" | "深色" | "夜间" | "dark" => return Some(Setting::Mode("dark")),
        "自动" | "auto" => return Some(Setting::Mode("auto")),
        "默认" | "default" => return Some(Setting::Color("")),
        _ => {}
    }
    let name = s.strip_suffix('色').unwrap_or(&s);
    if let Some(spec) = imaging::THEMES.iter().find(|t| t.name == name || t.key == name) {
        return Some(Setting::Color(spec.key));
    }
    // 单字 / 旧名别名。
    let key = match name {
        "蓝" | "靛" | "靛蓝" => "indigo",
        "青" | "绿" | "青绿" => "teal",
        "橙" | "暖橙" => "orange",
        "紫" => "purple",
        "粉" => "pink",
        _ => return None,
    };
    Some(Setting::Color(key))
}

/// 把一句话按空白拆词逐个解析;任一词认不出返 `None`(认不出的词**不**回显给用户——
/// bot 复读任意输入有审核风险),全认出返设置序列。
fn parse_line(s: &str) -> Option<Vec<Setting>> {
    s.split_whitespace().map(parse_setting).collect()
}

/// 亮暗库值的中文呈现（`auto` 带解析后的实际亮暗）。
fn mode_text(pref: &str) -> String {
    match pref {
        "light" => "亮色".to_string(),
        "dark" => "暗色".to_string(),
        _ => {
            let now = if imaging::pick_dark("auto") { "暗" } else { "亮" };
            format!("跟随时间（{now}色）")
        }
    }
}

/// 主题色库键的中文呈现(脏值按缺省处理,与出图一致)。
fn color_text(v: &str) -> String {
    let name = imaging::theme_spec(v).name;
    if v.is_empty() { format!("{name}（默认）") } else { name.to_string() }
}

/// 主题名一览(提示语用)。
fn theme_names() -> String {
    imaging::THEMES.iter().map(|t| t.name).collect::<Vec<_>>().join(" / ")
}

/// 给定一串设置落库,返回「改了什么」的短句。
async fn apply(user: &mut AUser, settings: &[Setting]) -> Result<String> {
    let mut said = Vec::new();
    for s in settings {
        match s {
            Setting::Mode(v) => {
                user.set_theme(v).await?;
                said.push(format!("亮暗已设为{}", mode_text(v)));
            }
            Setting::Color(v) => {
                user.set_theme_color(v).await?;
                said.push(format!("主题色已设为{}", color_text(v)));
            }
        }
    }
    Ok(said.join("，"))
}

/// `主题` → 设置出图主题（颜色与亮暗）。带参数直接设，缺参数进交互式配置。
#[command(
    "主题",
    "theme",
    mention_me,
    description = "设置出图的主题色和亮暗",
    usage = "发送「主题 松石青」换主题色，「主题 暗」换亮暗，也可以一起发：「主题 暗 珊瑚粉」。颜色可选：远黛蓝（默认）／松石青／落霞橙／鸢尾紫／珊瑚粉；亮暗可选：亮／暗／自动（按日出日落，天黑换暗色）。群聊里要 @ 机器人才会响应。只发「主题」会出一张色板，按提示回复就能改。"
)]
async fn theme(reply: Reply, mut user: AUser, session: Session, args: Args<ThemeArgs>) -> HandlerResult {
    let line = [args.0.first, args.0.second].into_iter().flatten().collect::<Vec<_>>().join(" ");

    // —— 带参数:直接解析落库,回执色板。——
    if !line.trim().is_empty() {
        let Some(settings) = parse_line(&line) else {
            reply.reply(format!("没这个颜色，可选：{}；亮暗：亮 / 暗 / 自动", theme_names())).await?;
            return Ok(());
        };
        let said = apply(&mut user, &settings).await?;
        send_palette(&reply, &user, "主题已更新", said).await?;
        return Ok(());
    }

    // —— 不带参数:交互式配置。色板图 + 提示文字一条消息发出,再等回复(解析失败
    //    自动追问;触发已要求 @ 本 bot,回话本身不再要求)。——
    // 同一个人同时只跑一条配置流程,守卫持到流程结束。
    let Some(_guard) = session.single_flight_user() else {
        reply.reply("上一个配置还没结束，先把那边弄完").await?;
        return Ok(());
    };
    let prompt = "回复颜色名或亮 / 暗 / 自动来设置，可以一起发（比如「暗 珊瑚粉」）；回复「取消」退出";
    match palette_card(user.theme(), user.theme_color(), "主题设置") {
        Ok(webp) => {
            reply.msg().image_bytes(webp).text(prompt).quote().await?;
        }
        Err(e) => {
            tracing::warn!(error = %e, "渲染主题色板失败,退回文字");
            reply
                .reply(format!(
                    "当前主题：{} · {}\n颜色：{}\n{prompt}",
                    color_text(user.theme_color()),
                    mode_text(user.theme()),
                    theme_names()
                ))
                .await?;
        }
    }

    let waiter = session.waiter().from_starter().build();
    // 非法自动重问（hint 由 parser 给）；取消（[`is_cancel`](super::is_cancel) 词面）与超时分开回执。
    let parsed = waiter
        .recv_parse(std::time::Duration::from_secs(60), super::is_cancel, |t| match parse_line(t) {
            Some(v) if !v.is_empty() => Ok(v),
            _ => Err(format!("没这个颜色，可选：{}；亮暗：亮 / 暗 / 自动；回复「取消」退出", theme_names())),
        })
        .await;
    let settings = match parsed {
        Replied::Got(v) => v,
        Replied::Cancelled => {
            reply.reply("行，先不改了").await?;
            return Ok(());
        }
        Replied::TimedOut => {
            reply.reply("等了一分钟没等到回复，先不改了").await?;
            return Ok(());
        }
    };
    let said = apply(&mut user, &settings).await?;
    send_palette(&reply, &user, "主题已更新", said).await?;
    Ok(())
}

/// `主题` 的参数:至多两个设置词(颜色 / 亮暗,顺序随意)。都可缺:全缺进交互式。
#[derive(Args)]
struct ThemeArgs {
    /// 第一个设置词。
    #[arg(name = "设置", desc = "颜色名或亮 / 暗 / 自动")]
    first: Option<String>,
    /// 第二个设置词。
    #[arg(name = "设置2", desc = "可再带一项，颜色和亮暗一起设")]
    second: Option<String>,
}

/// 出色板引用回复;渲不出记日志、退给定文字。
async fn send_palette(reply: &Reply, user: &AUser, title: &str, fallback: String) -> Result<()> {
    match palette_card(user.theme(), user.theme_color(), title) {
        Ok(webp) => {
            reply.msg().image_bytes(webp).quote().await?;
        }
        Err(e) => {
            tracing::warn!(error = %e, "渲染主题色板失败,退回文字");
            reply.reply(fallback).await?;
        }
    }
    Ok(())
}

/// 渲一张色板：标题 / 现状行 / 五套主题各一列（纸样式色卡：上半亮色四条、下半暗色
/// 四条，主色加高；名字标在列底，当前主题的名字上主色底反白）/ 用法一句。整卡按
/// 用户现行主题出图（底栏色带也跟着）。`theme_pref` / `color_pref` 是两列库值。
pub fn palette_card(theme_pref: &str, color_pref: &str, title: &str) -> anyhow::Result<Vec<u8>> {
    let t = UserTheme::resolve(theme_pref, color_pref);
    let pal = &t.palette;
    let current_key = imaging::theme_spec(color_pref).key;
    let mut d = Doc::new();

    d.paragraph(|p| {
        p.align(Align::Center).styled(title, |s| {
            s.weight(600).size(1.2);
        });
    });
    // 现状行:色名上主题主色加重,其余辅助灰;与底部提示同字号(辅助文字一个口径)。
    d.paragraph(|p| {
        p.align(Align::Center)
            .styled("现在：", |s| {
                s.color(&pal.muted).size(0.85);
            })
            .styled(color_text(color_pref), |s| {
                s.color(&pal.primary).weight(600).size(0.85);
            })
            .styled(format!(" · {}", mode_text(theme_pref)), |s| {
                s.color(&pal.muted).size(0.85);
            });
    });

    // 五列纸样色卡。每列:亮色组(主色条加高,重/鲜/暖三细条)、暗色组同构、名字标。
    d.columns(|cols| {
        cols.gap(14.0);
        for spec in imaging::THEMES {
            cols.col(|c| {
                for dark in [false, true] {
                    let row = spec.palette(dark);
                    let mut bar = |hex: &str, h: f32| {
                        c.progress(1.0, |b| {
                            b.height(h).fill(hex).radius(3.0);
                        });
                    };
                    bar(&row.primary, 22.0);
                    bar(&row.deep, 9.0);
                    bar(&row.vivid, 9.0);
                    bar(&row.warm, 9.0);
                }
                c.paragraph(|p| {
                    p.align(Align::Center);
                    if spec.key == current_key {
                        p.styled(format!(" ✓ {} ", spec.name), |s| {
                            s.bg(&t.palette.primary).color(&t.palette.on_color).weight(600).size(0.85);
                        });
                    } else {
                        p.styled(spec.name, |s| {
                            s.size(0.85);
                        });
                    }
                });
            });
        }
    });

    d.paragraph(|p| {
        p.align(Align::Center).styled(
            "每列上半亮色、下半暗色 · 发送「主题 颜色」换色，「主题 亮 / 暗 / 自动」换亮暗",
            |s| {
                s.color(&pal.muted).size(0.85);
            },
        );
    });

    Ok(render_document(&d.build(), &t.opts().with_padding(Insets::symmetric(28.0, 34.0)))?)
}
