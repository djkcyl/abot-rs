//! 个人数据卡片 —— 把「个人数据」汇总渲成一张图(经 [`imaging`](crate::imaging) 底座出
//! WebP)。海报式居中竖排,头部口径同签到卡:圆头像 / 名字·等级 / UID·QQ;主体是金币 +
//! 经验存量大数字同行(签到卡的同款排法,只是这里是存量不是增量)、等级进度条;往下
//! 是各插件经 [`ProfileSection`](crate::data::profile::ProfileSection) 贡献的统计行,游戏
//! 战绩(有则)另起一段、配色标题头。配色全部来自用户出图主题的标准色卡。渲染失败由
//! 调用方退回文字。

use nagisa::render::{Align, Doc, Insets, render_document};

use crate::COIN_NAME;
use crate::data::level::LevelInfo;
use crate::imaging::UserTheme;

/// 渲卡片要的全部数据(查询现成值 + 呈现素材,这里只管排版)。
pub struct MyDataCard {
    /// 显示名(群名片 / 昵称,缺则 QQ 号串)。
    pub name: String,
    /// 站内 UID(`user.id`,自增注册序号)。
    pub uid: i64,
    pub uin: i64,
    /// 头像字节(拉不到为 `None`,头部只排文字)。
    pub avatar: Option<Vec<u8>>,
    /// 金币余额。
    pub coin: i64,
    /// 经验总量。
    pub exp: i64,
    pub level: LevelInfo,
    /// 各插件贡献的普通统计行(签到 / 发言…),顺序 = 注册顺序。
    pub stats: Vec<String>,
    /// 各插件贡献的游戏战绩行(赛马 / 画板…),有则另起一段。
    pub games: Vec<String>,
    /// 出图主题(亮暗 + 标准色卡,按用户偏好经 `AUser::render_theme` 解析)。
    pub theme: UserTheme,
}

/// 本卡片的出图选项:公共底座(按用户主题)+ 卡片宽度与边距(同签到卡)。
fn render_opts(t: &UserTheme) -> nagisa::render::RenderOptions {
    t.opts().with_width(640.0).with_padding(Insets::symmetric(30.0, 36.0))
}

/// 把一份个人数据渲成卡片图(WebP 字节)。
pub fn card_image(c: &MyDataCard) -> anyhow::Result<Vec<u8>> {
    let pal = &c.theme.palette;
    let mut d = Doc::new();

    // —— 头部:圆头像、名字 + 等级、UID·QQ,全居中(同签到卡口径)。——
    if let Some(av) = &c.avatar {
        d.image_bytes(av.clone(), |i| {
            i.width_px(88.0).align(Align::Center).rounded(44.0);
        });
    }
    d.paragraph(|p| {
        p.align(Align::Center)
            .styled(c.name.as_str(), |s| {
                s.weight(600).size(1.15);
            })
            .styled(format!("  Lv.{}", c.level.level), |s| {
                s.weight(600).size(0.9).color(&pal.primary);
            });
    });
    d.paragraph(|p| {
        p.align(Align::Center).styled(format!("UID {} · QQ {}", c.uid, c.uin), |s| {
            s.color(&pal.muted).size(0.8);
        });
    });

    // —— 主体:金币 + 经验存量大数字同一行。——
    d.paragraph(|p| {
        p.align(Align::Center)
            .styled(c.coin.to_string(), |s| {
                s.weight(700).size(2.2).color(&pal.warm);
            })
            .styled(format!(" {COIN_NAME}    "), |s| {
                s.color(&pal.muted).size(0.95);
            })
            .styled(c.exp.to_string(), |s| {
                s.weight(700).size(2.2).color(&pal.vivid);
            })
            .styled(" 经验", |s| {
                s.color(&pal.muted).size(0.95);
            });
    });

    // —— 等级进度条(限宽居中)+ 说明行。——
    d.progress(c.level.into_level as f32 / c.level.level_span.max(1) as f32, |b| {
        b.width_percent(56.0).align(Align::Center).height(8.0).fill(&pal.primary).track(&pal.track);
    });
    d.paragraph(|p| {
        p.align(Align::Center).styled(
            format!(
                "本级 {}/{} · 距 Lv.{} 还差 {}",
                c.level.into_level,
                c.level.level_span,
                c.level.level + 1,
                c.level.level_span - c.level.into_level
            ),
            |s| {
                s.color(&pal.muted).size(0.8);
            },
        );
    });

    // —— 各插件统计行:分隔线下逐行居中,没贡献就整段不出。——
    if !c.stats.is_empty() {
        d.divider();
        for line in &c.stats {
            d.paragraph(|p| {
                p.align(Align::Center).styled(line.as_str(), |s| {
                    s.size(0.95);
                });
            });
        }
    }

    // —— 战绩段:色标小题头 + 逐行,没战绩不占地。——
    if !c.games.is_empty() {
        d.divider();
        d.paragraph(|p| {
            p.align(Align::Center).styled(" 战绩 ", |s| {
                s.color(&pal.on_color).bg(&pal.deep).size(0.85).weight(500);
            });
        });
        for line in &c.games {
            d.paragraph(|p| {
                p.align(Align::Center).styled(line.as_str(), |s| {
                    s.size(0.95);
                });
            });
        }
    }

    Ok(render_document(&d.build(), &render_opts(&c.theme))?)
}
