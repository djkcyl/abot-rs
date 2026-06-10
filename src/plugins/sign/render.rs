//! 签到卡片 —— 签到成功的结算渲成一张图(经 [`imaging`](crate::imaging) 底座出 WebP):
//! 头像·名字·等级开头,金币 / 经验 / 连签三块数据,金币明细一行,彩头(大奖 / 里程碑 /
//! 首签礼 / 升级)各一枚色标,等级进度条收尾。渲染失败由调用方退回文字。

use nagisa::render::{render_document, Align, Doc, Insets, Length};

use crate::data::level::LevelInfo;
use crate::plugins::sign::logic::{FIRST_GIFT, JACKPOT_GOLD};
use crate::COIN_NAME;

/// 卡片用色:金币暖橙 / 经验青绿 / 连签与等级靛蓝 / 里程碑紫(与品牌底栏同一组色)。
const C_GOLD: &str = "#bd6b32";
const C_EXP: &str = "#0e9488";
const C_INDIGO: &str = "#4c63b6";
const C_PURPLE: &str = "#7a5cc4";
/// 辅助文字灰与进度条底色。
const C_MUTED: &str = "#8a8f98";
const C_TRACK: &str = "#dbe2ec";

/// 渲卡片要的全部数据(结算现成值 + 呈现素材,这里只管排版)。
pub struct SignCard {
    /// 显示名(群名片 / 昵称,缺则 QQ 号串)。
    pub name: String,
    pub uin: i64,
    /// 头像字节(拉不到为 `None`,头部只排文字)。
    pub avatar: Option<Vec<u8>>,
    /// 按钟点的问候语。
    pub greet: &'static str,
    /// 金币总数与三个常规分项(里程碑 / 首签 / 大奖另走彩头色标)。
    pub gold_add: i64,
    pub base: i64,
    pub streak_bonus: i64,
    pub luck: i64,
    pub milestone: i64,
    pub first_sign: bool,
    pub jackpot: bool,
    pub exp_gain: i64,
    /// 升级后的新等级(本次没升为 `None`)。
    pub leveled_to: Option<i64>,
    pub level: LevelInfo,
    pub continue_sign: i32,
    pub total_sign: i32,
    /// 发奖后的余额。
    pub balance: i64,
}

/// 本卡片的出图选项:公共底座 + 窄卡宽度与边距(个人结算卡,不用瓶子那么宽)。
fn render_opts() -> nagisa::render::RenderOptions {
    crate::imaging::render_opts().with_width(600.0).with_padding(Insets::symmetric(32.0, 36.0))
}

/// 把一次签到结算渲成卡片图(WebP 字节)。
pub fn card_image(c: &SignCard) -> anyhow::Result<Vec<u8>> {
    let mut d = Doc::new();

    // —— 头部:头像 | 名字(+等级标)/ 第 N 次签到 / 问候。拉不到头像就只排文字。——
    match &c.avatar {
        Some(avatar) => {
            let bytes = avatar.clone();
            d.columns(|cols| {
                cols.gap(24.0)
                    .col(|b| {
                        b.image_bytes(bytes, |i| {
                            i.rounded(14.0);
                        });
                    })
                    .col_weighted(3.4, |b| {
                        header(b, c);
                    });
            });
        }
        None => {
            header(&mut d, c);
        }
    }

    // —— 三块数据:金币 / 经验 / 连签,各自居中、大数字带色。——
    d.divider();
    let stats = [
        (format!("+{}", c.gold_add), C_GOLD, COIN_NAME),
        (format!("+{}", c.exp_gain), C_EXP, "经验"),
        (format!("{} 天", c.continue_sign), C_INDIGO, "连签"),
    ];
    d.columns(|cols| {
        for (num, color, label) in stats {
            cols.col(|b| {
                b.paragraph(|p| {
                    p.align(Align::Center).styled(num, |s| {
                        s.weight(600).size(1.8).color(color);
                    });
                });
                b.paragraph(|p| {
                    p.align(Align::Center).styled(label, |s| {
                        s.color(C_MUTED).size(0.85);
                    });
                });
            });
        }
    });

    // 金币明细(常规三项;彩头不在此重复,走下面的色标)。
    d.paragraph(|p| {
        p.align(Align::Center).styled(
            format!("基础 {} + 连签加成 {} + 手气 {}", c.base, c.streak_bonus, c.luck),
            |s| {
                s.color(C_MUTED).size(0.8);
            },
        );
    });

    // —— 彩头色标:大奖 / 里程碑 / 首签礼 / 升级,有哪个排哪个。——
    let mut chips: Vec<(String, &str)> = Vec::new();
    if c.jackpot {
        chips.push((format!(" 大奖 +{JACKPOT_GOLD} "), C_GOLD));
    }
    if c.milestone > 0 {
        chips.push((format!(" 连签 {} 天里程碑 +{} ", c.continue_sign, c.milestone), C_PURPLE));
    }
    if c.first_sign {
        chips.push((format!(" 首签礼 +{FIRST_GIFT} "), C_EXP));
    }
    if let Some(to) = c.leveled_to {
        chips.push((format!(" 升到 Lv.{to} "), C_INDIGO));
    }
    if !chips.is_empty() {
        d.paragraph(|p| {
            p.align(Align::Center);
            for (i, (text, bg)) in chips.into_iter().enumerate() {
                if i > 0 {
                    p.text("  ");
                }
                p.styled(text, |s| {
                    s.color("#ffffff").bg(bg).size(0.85).weight(500);
                });
            }
        });
    }

    // —— 等级进度:数字一行 + 进度条(单行双格表:左格按进度占百分比宽、两格上底色,
    //    行字号缩到 0.35 把行高压成细条)。——
    d.divider();
    d.paragraph(|p| {
        p.styled(format!("Lv.{}", c.level.level), |s| {
            s.weight(600);
        });
        p.styled(
            format!(
                "  {} / {} · 距 Lv.{} 还差 {}",
                c.level.into_level,
                c.level.level_span,
                c.level.level + 1,
                c.level.level_span - c.level.into_level
            ),
            |s| {
                s.color(C_MUTED).size(0.9);
            },
        );
    });
    let pct = (c.level.into_level as f32 / c.level.level_span.max(1) as f32 * 100.0)
        .clamp(1.0, 99.0);
    d.table(|t| {
        // 格子放一个空格:空格子的行高会退到整行基准,缩字号就压不薄;有字形才吃
        // row_style 的 0.3 倍行高,得到细条。
        t.row([" ", " "]);
        t.width(0, Length::Percent(pct));
        t.col_fill(0, C_INDIGO);
        t.col_fill(1, C_TRACK);
        t.no_grid();
        t.expand();
        t.pad_x(0.0);
        t.pad_y(2.0);
        t.row_style(0, |s| {
            s.size(0.3);
        });
    });

    // 尾行:余额。
    d.paragraph(|p| {
        p.align(Align::Center).styled(format!("钱包余额 {} {COIN_NAME}", c.balance), |s| {
            s.color(C_MUTED).size(0.85);
        });
    });

    Ok(render_document(&d.build(), &render_opts())?)
}

/// 头部文字栏:名字 + 等级标一行、第 N 次签到一行、问候一行(楷体,亲和)。
fn header(b: &mut Doc, c: &SignCard) {
    use nagisa::render::FontRole;
    b.heading(3, |h| {
        h.text(c.name.as_str());
        h.styled(format!(" Lv.{} ", c.level.level), |s| {
            s.color("#ffffff").bg(C_INDIGO).size(0.5).weight(600);
        });
    });
    b.paragraph(|p| {
        p.styled(format!("第 {} 次签到 · QQ {}", c.total_sign, c.uin), |s| {
            s.color(C_MUTED).size(0.85);
        });
    });
    b.paragraph(|p| {
        p.styled(c.greet, |s| {
            s.font(FontRole::Kai).size(0.95);
        });
    });
}
