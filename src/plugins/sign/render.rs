//! 签到卡片 —— 签到成功的结算渲成一张图(经 [`imaging`](crate::imaging) 底座出 WebP)。
//! 海报式居中竖排:圆头像 / 名字·等级 / UID·QQ·第 N 次 / 问候(楷体放大) / 金币 + 经验
//! 大数字同行 / 连签 / 彩头色标(有则) / 等级进度条 / 收尾行。配色全部来自用户出图
//! 主题的标准色卡([`UserTheme`] 的 [`Palette`](crate::imaging::Palette):金币暖槽 /
//! 经验鲜槽 / 等级与进度主槽 / 里程碑重槽,亮暗与底栏色带都跟着主题)。渲染失败由
//! 调用方退回文字。

use nagisa::render::{render_document, Align, Doc, FontRole, Insets};

use crate::data::level::LevelInfo;
use crate::imaging::UserTheme;
use crate::plugins::sign::logic::{FIRST_GIFT, JACKPOT_GOLD};
use crate::COIN_NAME;

/// 渲卡片要的全部数据(结算现成值 + 呈现素材,这里只管排版)。
pub struct SignCard {
    /// 显示名(群名片 / 昵称,缺则 QQ 号串)。
    pub name: String,
    /// 站内 UID(`user.id`,自增注册序号)。
    pub uid: i64,
    pub uin: i64,
    /// 头像字节(拉不到为 `None`,头部只排文字)。
    pub avatar: Option<Vec<u8>>,
    /// 按钟点的问候语。
    pub greet: &'static str,
    pub gold_add: i64,
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
    /// 出图主题(亮暗 + 标准色卡,按用户偏好经 `AUser::render_theme` 解析)。
    pub theme: UserTheme,
}

/// 本卡片的出图选项:公共底座(按用户主题)+ 卡片宽度与边距。
fn render_opts(t: &UserTheme) -> nagisa::render::RenderOptions {
    t.opts().with_width(640.0).with_padding(Insets::symmetric(30.0, 36.0))
}

/// 把一次签到结算渲成卡片图(WebP 字节)。
pub fn card_image(c: &SignCard) -> anyhow::Result<Vec<u8>> {
    let pal = &c.theme.palette;
    let mut d = Doc::new();

    // —— 头部:圆头像、名字 + 等级、UID·QQ·第 N 次,全居中。——
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
        p.align(Align::Center).styled(
            format!("UID {} · QQ {} · 第 {} 次签到", c.uid, c.uin, c.total_sign),
            |s| {
                s.color(&pal.muted).size(0.8);
            },
        );
    });

    // 问候:楷体放大,卡片上最有人味的一句,给足存在感。
    d.paragraph(|p| {
        p.align(Align::Center).styled(c.greet, |s| {
            s.font(FontRole::Kai).size(1.3).weight(500);
        });
    });

    // —— 主体:金币 + 经验大数字同一行。——
    d.paragraph(|p| {
        p.align(Align::Center)
            .styled(format!("+{}", c.gold_add), |s| {
                s.weight(700).size(2.2).color(&pal.warm);
            })
            .styled(format!(" {COIN_NAME}    "), |s| {
                s.color(&pal.muted).size(0.95);
            })
            .styled(format!("+{}", c.exp_gain), |s| {
                s.weight(700).size(2.2).color(&pal.vivid);
            })
            .styled(" 经验", |s| {
                s.color(&pal.muted).size(0.95);
            });
    });
    d.paragraph(|p| {
        p.align(Align::Center).styled(format!("连签 {} 天", c.continue_sign), |s| {
            s.size(0.95);
        });
    });

    // —— 彩头色标:大奖 / 里程碑 / 首签礼 / 升级,有哪个排哪个,没有不占行。——
    let mut chips: Vec<(String, &str)> = Vec::new();
    if c.jackpot {
        chips.push((format!(" 大奖 +{JACKPOT_GOLD} "), &pal.warm));
    }
    if c.milestone > 0 {
        chips.push((format!(" 里程碑 +{} ", c.milestone), &pal.deep));
    }
    if c.first_sign {
        chips.push((format!(" 首签礼 +{FIRST_GIFT} "), &pal.vivid));
    }
    if let Some(to) = c.leveled_to {
        chips.push((format!(" 升到 Lv.{to} "), &pal.primary));
    }
    if !chips.is_empty() {
        d.paragraph(|p| {
            p.align(Align::Center);
            for (i, (text, bg)) in chips.into_iter().enumerate() {
                if i > 0 {
                    p.text("  ");
                }
                p.styled(text, |s| {
                    s.color(&pal.on_color).bg(bg).size(0.85).weight(500);
                });
            }
        });
    }

    // —— 等级进度条(限宽居中)+ 收尾行。——
    d.progress(c.level.into_level as f32 / c.level.level_span.max(1) as f32, |b| {
        b.width_percent(56.0).align(Align::Center).height(8.0).fill(&pal.primary).track(&pal.track);
    });
    d.paragraph(|p| {
        p.align(Align::Center).styled(
            format!(
                "距 Lv.{} 还差 {} · 余额 {} {COIN_NAME}",
                c.level.level + 1,
                c.level.level_span - c.level.into_level,
                c.balance
            ),
            |s| {
                s.color(&pal.muted).size(0.8);
            },
        );
    });

    Ok(render_document(&d.build(), &render_opts(&c.theme))?)
}

/// 渲日历要的数据(查询现成值,这里只管排版)。
pub struct CalendarCard {
    /// 显示名(群名片 / 昵称,缺则 QQ 号串)。
    pub name: String,
    /// 站内 UID(`user.id`)。
    pub uid: i64,
    pub uin: i64,
    /// 头像字节(拉不到为 `None`,头部只排文字)。
    pub avatar: Option<Vec<u8>>,
    /// 日历年月(业务日口径——凌晨 4 点前算前一天,故月初凌晨可能仍是上月)。
    pub year: i32,
    pub month: u32,
    /// 当月签过的「几号」集合。
    pub days: std::collections::HashSet<u32>,
    /// 今天(业务日)是几号;仅当日历就是当前月时为 `Some`。
    pub today: Option<u32>,
    pub continue_sign: i32,
    pub total_sign: i32,
    /// 出图主题(同签到卡)。
    pub theme: UserTheme,
}

/// 把一个月的签到记录渲成卡(WebP 字节)。版面从上到下:用户信息(圆头像 / 名字 /
/// UID·QQ)、年月标题、带网格线的七列月历(周一起头,签过满底色反白、**今天的日数
/// 下加着重点**(引擎文字装饰,画进行距不撑格)且未签时淡底、未到的日子弱化)、
/// 底部统计行(连签 / 本月 / 累计)。
pub fn calendar_image(c: &CalendarCard) -> anyhow::Result<Vec<u8>> {
    use chrono::Datelike;

    let pal = &c.theme.palette;
    let first = chrono::NaiveDate::from_ymd_opt(c.year, c.month, 1)
        .ok_or_else(|| anyhow::anyhow!("非法年月 {}-{}", c.year, c.month))?;
    let next = if c.month == 12 {
        chrono::NaiveDate::from_ymd_opt(c.year + 1, 1, 1)
    } else {
        chrono::NaiveDate::from_ymd_opt(c.year, c.month + 1, 1)
    }
    .expect("下月一号必然合法");
    let n_days = (next - first).num_days() as u32;
    let offset = first.weekday().num_days_from_monday() as usize;

    let mut d = Doc::new();

    // —— 顶部:用户信息(同签到卡口径)。——
    if let Some(av) = &c.avatar {
        d.image_bytes(av.clone(), |i| {
            i.width_px(72.0).align(Align::Center).rounded(36.0);
        });
    }
    d.paragraph(|p| {
        p.align(Align::Center).styled(c.name.as_str(), |s| {
            s.weight(600).size(1.1);
        });
    });
    d.paragraph(|p| {
        p.align(Align::Center).styled(format!("UID {} · QQ {}", c.uid, c.uin), |s| {
            s.color(&pal.muted).size(0.8);
        });
    });

    // 年月标题。
    d.paragraph(|p| {
        p.align(Align::Center).styled(format!("{} 年 {} 月", c.year, c.month), |s| {
            s.weight(600).size(1.3);
        });
    });

    // —— 月历:周一起头,首周前与末周后补空串对齐七列;开网格线、表头浅底,照着
    //    真日历来。今天的日数下加着重点(引擎文字装饰,画进行距、不撑格)。——
    let mut cells: Vec<String> = vec![String::new(); offset];
    cells.extend((1..=n_days).map(|day| day.to_string()));
    while !cells.len().is_multiple_of(7) {
        cells.push(String::new());
    }
    d.table(|t| {
        t.head(["一", "二", "三", "四", "五", "六", "日"]);
        t.align([Align::Center; 7]);
        t.expand();
        t.pad_y(8.0);
        for week in cells.chunks(7) {
            t.row(week.iter().cloned());
        }
        for (i, cell) in cells.iter().enumerate() {
            let Ok(day) = cell.parse::<u32>() else { continue };
            let (row, col) = (i / 7, i % 7);
            if c.days.contains(&day) {
                // 签过:满底色 + 反白加重。
                t.cell_fill(row, col, &pal.primary);
                t.cell_style(row, col, |s| {
                    s.color(&pal.on_color).weight(600);
                });
            } else if c.today == Some(day) {
                // 今天还没签:主色淡底 + 主色加重。
                t.cell_fill(row, col, &pal.soft);
                t.cell_style(row, col, |s| {
                    s.color(&pal.primary).weight(600);
                });
            } else if c.today.is_some_and(|t0| day > t0) {
                // 未到的日子:弱化。
                t.cell_style(row, col, |s| {
                    s.color(&pal.muted);
                });
            }
            // 今天的日数下加着重点(颜色跟随该格文字色:已签反白点、未签主题色点)。
            if c.today == Some(day) {
                t.cell_style(row, col, |s| {
                    s.dot();
                });
            }
        }
    });

    // —— 底部:统计行。——
    d.paragraph(|p| {
        p.align(Align::Center).styled(
            format!(
                "连签 {} 天 · 本月 {} 天 · 累计 {} 次",
                c.continue_sign,
                c.days.len(),
                c.total_sign
            ),
            |s| {
                s.color(&pal.muted).size(0.9);
            },
        );
    });

    Ok(render_document(&d.build(), &render_opts(&c.theme))?)
}
