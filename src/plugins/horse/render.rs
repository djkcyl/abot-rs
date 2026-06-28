//! 赛马出图:马匹/马厩/比赛帧/结算/背包/抽卡/榜/成就,全走 [`Canvas`] 直绘。
//! 文字按中线纵向居中([`Canvas::text_mid`]),按墨迹实宽顺排,不手算基线。

use std::collections::HashSet;

use nagisa::prelude::*;
use nagisa::render::{
    Align, Block, Canvas, Color, Insets, OutputFormat, Radar, RenderOptions, parse_markup, render_document,
};

use super::consts::{self, Achievement, Item, Stat};
use super::entity::horse;
use super::race::RaceResult;
use crate::imaging::UserTheme;

// 皮肤 / 公共工具

/// 卡片统一宽度(逻辑像素)。
const W: f32 = 560.0;
/// 卡片内边距。
const PAD: f32 = 28.0;
/// 五维雷达归一参照 = [`DISPLAY_REF`](super::consts::DISPLAY_REF);与赛场标尺解耦,免得中后期早早撞满。
const RADAR_REF: f32 = super::consts::DISPLAY_REF as f32;

/// 两行行 名/说明 两条中线相对行中线 cy 的偏移;不对称是让整块重心压在 cy。
const L2_NAME_DY: f32 = -12.0;
const L2_SUB_DY: f32 = 16.0;
/// 两行列表行的行高(行间留白 > 行内名↔说明留白)。
const L2_ROW_H: f32 = 60.0;

/// 毛色标记色(固定不随主题);下标 = `color` 列。
const COAT: [(u8, u8, u8); 6] = [
    (0xb0, 0x3a, 0x2e), // 枣红
    (0x9c, 0x6b, 0x33), // 栗色
    (0x3a, 0x3f, 0x47), // 乌骓
    (0xc4, 0xca, 0xd4), // 白龙
    (0x5f, 0x84, 0x99), // 青骢
    (0xc8, 0x96, 0x3e), // 金棕
];

/// 解析十六进制色(失败回中性灰)。
fn col(hex: &str) -> Color {
    Color::hex(hex).unwrap_or(Color::rgb(0x88, 0x88, 0x88))
}

/// `Color` → `#rrggbb`(给文字盒上色;丢 alpha)。
fn hexs(c: Color) -> String {
    format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b)
}

fn alpha(c: Color, a: u8) -> Color {
    Color { a, ..c }
}

fn coat(color: i16) -> Color {
    let (r, g, b) = COAT[color.clamp(0, 5) as usize];
    Color::rgb(r, g, b)
}

/// 一次出图的成卡配色。
struct Skin {
    dark: bool,
    surface_top: Color,
    surface_bot: Color,
    panel: Color,
    text: Color,
    muted: Color,
    hairline: Color,
    primary: Color,
    deep: Color,
    vivid: Color,
    warm: Color,
    soft: Color,
    track: Color,
}

impl Skin {
    fn of(t: &UserTheme) -> Skin {
        let p = &t.palette;
        let (surface_top, surface_bot, panel, text, hairline) = if t.dark {
            (
                Color::rgb(0x1d, 0x24, 0x31),
                Color::rgb(0x12, 0x16, 0x1d),
                Color::rgba(0xff, 0xff, 0xff, 0x0c),
                Color::rgb(0xe6, 0xed, 0xf3),
                Color::rgba(0xff, 0xff, 0xff, 0x16),
            )
        } else {
            (
                Color::rgb(0xff, 0xff, 0xff),
                Color::rgb(0xed, 0xef, 0xf4),
                Color::rgba(0x10, 0x12, 0x18, 0x07),
                Color::rgb(0x1f, 0x23, 0x28),
                Color::rgba(0x10, 0x12, 0x18, 0x12),
            )
        };
        Skin {
            dark: t.dark,
            surface_top,
            surface_bot,
            panel,
            text,
            muted: col(&p.muted),
            hairline,
            primary: col(&p.primary),
            deep: col(&p.deep),
            vivid: col(&p.vivid),
            warm: col(&p.warm),
            soft: col(&p.soft),
            track: col(&p.track),
        }
    }
}

/// 卡片文字选项:复用主题字体,收紧行高(默认 1.5 太松,卡片靠坐标摆放要紧凑)。
fn card_opts(theme: &UserTheme) -> RenderOptions {
    let mut o = theme.opts();
    o.theme.line_height = 1.12;
    o
}

/// 各维条形 / 雷达取色(`soft` 太淡不作文字色,幸运用中性 `muted`)。
fn stat_paint(sk: &Skin, s: Stat) -> Color {
    match s {
        Stat::Spd => sk.primary,
        Stat::Sta => sk.vivid,
        Stat::Brs => sk.warm,
        Stat::Agi => sk.deep,
        Stat::Luk => sk.muted,
    }
}

/// 星级字串:实心 + 空心凑满四档。
fn stars(rarity: i16) -> String {
    let r = rarity.clamp(0, consts::RARITY_MAX) as usize;
    format!("{}{}", "★".repeat(r), "☆".repeat(consts::RARITY_MAX as usize - r))
}

fn sex_str(sex: i16) -> &'static str {
    if sex == 0 { "公" } else { "母" }
}

fn stat_abbr(s: Stat) -> &'static str {
    match s {
        Stat::Spd => "速",
        Stat::Sta => "耐",
        Stat::Brs => "爆",
        Stat::Agi => "敏",
        Stat::Luk => "运",
    }
}

/// 沿闭合多边形各边画虚线(nagisa Canvas 无虚线原语,手动按 `dash`/`gap` 分段)。
fn dashed_polygon(c: &mut Canvas, pts: &[(f32, f32)], dash: f32, gap: f32, w: f32, color: Color) {
    let n = pts.len();
    for i in 0..n {
        let (x0, y0) = pts[i];
        let (x1, y1) = pts[(i + 1) % n];
        let (dx, dy) = (x1 - x0, y1 - y0);
        let len = (dx * dx + dy * dy).sqrt();
        if len < 0.001 {
            continue;
        }
        let (ux, uy) = (dx / len, dy / len);
        let mut t = 0.0;
        while t < len {
            let t2 = (t + dash).min(len);
            c.line(x0 + ux * t, y0 + uy * t, x0 + ux * t2, y0 + uy * t2, w, color);
            t += dash + gap;
        }
    }
}

/// 成长系数 growth(×100)→ 快/中/慢标签。
fn growth_tag(growth: i32) -> &'static str {
    if growth >= 115 {
        "快"
    } else if growth <= 85 {
        "慢"
    } else {
        "中"
    }
}

/// 名次牌色(金 / 银 / 铜 / 中性)。
fn medal_color(sk: &Skin, rank: usize) -> Color {
    match rank {
        1 => Color::rgb(0xf0, 0xb2, 0x32),
        2 => Color::rgb(0xb4, 0xbd, 0xc8),
        3 => Color::rgb(0xcd, 0x84, 0x4e),
        _ => sk.track,
    }
}

/// 铺卡底:竖直渐变 + 顶部主色细条。
fn paint_bg(c: &mut Canvas, sk: &Skin, w: f32, h: f32) {
    c.v_gradient(0.0, 0.0, w, h, 0.0, sk.surface_top, sk.surface_bot);
    c.rect(0.0, 0.0, w, 5.0, 0.0, sk.primary);
}

/// 标题块:大标题 + 可选副标题 + 分隔线;返回分隔线 y(内容从其下排)。
fn title_block(
    c: &mut Canvas,
    o: &RenderOptions,
    sk: &Skin,
    title: &str,
    sub: Option<&str>,
) -> nagisa::render::Result<f32> {
    c.text_mid(PAD, 44.0, W - 2.0 * PAD, o, |t| {
        t.styled(title, |s| {
            s.weight(800).size(1.35).color(&hexs(sk.text));
        });
    })?;
    if let Some(sub) = sub {
        c.text_mid(PAD, 84.0, W - 2.0 * PAD, o, |t| {
            t.styled(sub, |s| {
                s.size(0.74).color(&hexs(sk.muted));
            });
        })?;
        c.line(PAD, 110.0, W - PAD, 110.0, 1.0, sk.hairline);
        Ok(110.0)
    } else {
        c.line(PAD, 78.0, W - PAD, 78.0, 1.0, sk.hairline);
        Ok(78.0)
    }
}

/// 名次小圆牌(底色按名次)+ 居中名次数字,整体居中于中线 `cy`。
fn rank_badge(
    c: &mut Canvas,
    o: &RenderOptions,
    sk: &Skin,
    cx: f32,
    cy: f32,
    r: f32,
    rank: usize,
) -> nagisa::render::Result<()> {
    c.disc(cx, cy, r, medal_color(sk, rank));
    let num = if rank <= 3 { Color::rgb(0x1a, 0x1d, 0x24) } else { sk.muted };
    c.text_mid(cx - r, cy, r * 2.0, o, |t| {
        t.align(Align::Center).styled(format!("{rank}"), |s| {
            s.weight(800).size(0.6).color(&hexs(num));
        });
    })?;
    Ok(())
}

/// 行斑马纹底(偶数行淡底,整体居中于 `cy`,高 `h`)。
fn zebra(c: &mut Canvas, sk: &Skin, cy: f32, h: f32) {
    c.rect(PAD - 6.0, cy - h / 2.0, W - 2.0 * PAD + 12.0, h, 9.0, sk.panel);
}

// 马匹卡

/// 「标签 — 进度条 — 数值」属性行,三段横排全部居中于中线 `cy`。
#[allow(clippy::too_many_arguments)]
fn stat_row(
    c: &mut Canvas,
    o: &RenderOptions,
    sk: &Skin,
    x: f32,
    cy: f32,
    w: f32,
    label: &str,
    value: &str,
    bar: Color,
    frac: f32,
    ceiling: f32,
) -> nagisa::render::Result<()> {
    let (label_w, value_w) = (58.0, 34.0);
    let bar_x = x + label_w + 6.0;
    let bar_w = (w - label_w - value_w - 16.0).max(8.0);
    c.text_mid(x, cy, label_w, o, |t| {
        t.styled(label, |s| {
            s.size(0.72).weight(600).color(&hexs(sk.text));
        });
    })?;
    c.rect(bar_x, cy - 4.0, bar_w, 8.0, 4.0, sk.track);
    // 天赋线之后是边际递减区,淡一层提示再练收益很小。
    let cf = ceiling.clamp(0.0, 1.0);
    if cf < 1.0 {
        c.rect(bar_x + bar_w * cf, cy - 4.0, bar_w * (1.0 - cf), 8.0, 4.0, alpha(sk.muted, 0x22));
    }
    c.rect(bar_x, cy - 4.0, bar_w * frac.clamp(0.0, 1.0), 8.0, 4.0, bar);
    // 天赋线刻度。
    if cf > 0.02 && cf < 1.0 {
        let mx = bar_x + bar_w * cf;
        c.line(mx, cy - 6.0, mx, cy + 6.0, 1.5, alpha(sk.muted, 0xBB));
    }
    c.text_mid(x + w - value_w, cy, value_w, o, |t| {
        t.align(Align::Right).styled(value, |s| {
            s.size(0.78).weight(700).color(&hexs(bar));
        });
    })?;
    Ok(())
}

/// 马匹详情卡。`owner` 为主人显示标签(见 [`logic::owner_label`](super::logic::owner_label))。
pub fn horse_card(m: &horse::Model, owner: &str, theme: &UserTheme) -> Result<Vec<u8>> {
    horse_canvas(m, owner, theme).and_then(|c| c.encode(OutputFormat::Webp)).context("赛马出图")
}

fn horse_canvas(m: &horse::Model, owner: &str, theme: &UserTheme) -> nagisa::render::Result<Canvas> {
    let sk = Skin::of(theme);
    let o = card_opts(theme);
    let cur = super::logic::stats_of(m);
    let pots = [m.pot_spd, m.pot_sta, m.pot_brs, m.pot_agi, m.pot_luk];
    let traits = consts::Trait::from_mask(m.traits);

    // 竖向布局(逻辑像素)。下半段(特性 / 伤病 / 战绩)按游标排,先算好各中线再定卡高。
    let div1 = 136.0; // 头部分隔线
    let (bar0, bar_step) = (162.0, 32.0); // 属性首行中线 + 行距
    let (rcx, rcy, rr) = (152.0, 226.0, 68.0); // 雷达
    let div2 = 314.0; // 资源段上分隔线
    let res_y = 332.0; // 资源标签 + 值
    let res_bar = 348.0; // 资源进度条
    let res_bottom = res_bar + 6.0;
    let mut y = res_bottom;
    let trait_cy = if !traits.is_empty() {
        y += 26.0;
        let cy = y;
        y += 14.0; // chip 下沿
        Some(cy)
    } else {
        None
    };
    let injury_cy = if m.injury > 0 {
        y += 22.0;
        let cy = y;
        y += 8.0;
        Some(cy)
    } else {
        None
    };
    // 伤痕(后遗症):伤愈后残留、削四维 + 易复发——必须可见,别让玩家以为「治好了」却莫名变弱。
    let scar_cy = if m.scar > 0 {
        y += 22.0;
        let cy = y;
        y += 8.0;
        Some(cy)
    } else {
        None
    };
    let div3 = y + 12.0; // 战绩上分隔线
    let foot_y = div3 + 20.0; // 战绩 / 血统行
    let h = foot_y + 22.0;

    let mut c = Canvas::new(W, h, o.scale)?;
    paint_bg(&mut c, &sk, W, h);

    // 头部:名 + 星级、主人、基本信息行。
    c.text_mid(PAD, 46.0, 360.0, &o, |t| {
        t.styled(m.name.as_str(), |s| {
            s.weight(800).size(1.5).color(&hexs(sk.text));
        });
    })?;
    c.text_mid(W - PAD - 200.0, 46.0, 200.0, &o, |t| {
        t.align(Align::Right).styled(stars(m.rarity), |s| {
            s.size(1.05).color(&hexs(sk.warm));
        });
    })?;
    c.text_mid(PAD, 86.0, W - 2.0 * PAD, &o, |t| {
        t.styled("主人 ", |s| {
            s.size(0.74).color(&hexs(sk.muted));
        })
        .styled(owner, |s| {
            s.size(0.74).weight(600).color(&hexs(sk.vivid));
        });
    })?;
    let sub = format!(
        "#{} · {} · {} · 第 {} 代 · 成长{}",
        m.id,
        consts::color_name(m.color),
        sex_str(m.sex),
        m.generation,
        growth_tag(m.growth)
    );
    c.text_mid(PAD, 115.0, W - 2.0 * PAD, &o, |t| {
        t.styled(sub, |s| {
            s.size(0.74).color(&hexs(sk.muted));
        });
    })?;
    c.line(PAD, div1, W - PAD, div1, 1.0, sk.hairline);

    // 左:雷达 —— 实心多边形 = 当前值,叠一圈虚线轮廓 = 各维天赋(软上限)。
    c.disc(rcx, rcy, rr + 18.0, sk.panel);
    let cur_vals: Vec<f32> = Stat::ALL.iter().map(|s| (cur[s.idx()] as f32 / RADAR_REF).clamp(0.06, 1.0)).collect();
    let radar = Radar {
        fill: alpha(sk.primary, 0x40),
        stroke: sk.primary,
        stroke_w: 2.0,
        grid: sk.hairline,
        grid_w: 1.2,
        rings: 4,
        vertex_dot: Some((3.0, sk.primary)),
        start_deg: -90.0,
    };
    c.radar(rcx, rcy, rr, &cur_vals, &radar);
    // 天赋轮廓(虚线)。
    let n = Stat::ALL.len();
    let ceil_pts: Vec<(f32, f32)> = Stat::ALL
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let v = (super::logic::soft_ceiling(pots[s.idx()], m.growth) as f32 / RADAR_REF).clamp(0.06, 1.0);
            let ang = (-90.0 + 360.0 * i as f32 / n as f32).to_radians();
            (rcx + rr * v * ang.cos(), rcy + rr * v * ang.sin())
        })
        .collect();
    dashed_polygon(&mut c, &ceil_pts, 5.0, 4.0, 1.4, alpha(sk.primary, 0x99));
    for (i, s) in Stat::ALL.iter().enumerate() {
        let ang = (-90.0 + 360.0 * i as f32 / 5.0).to_radians();
        let (lx, ly) = (rcx + (rr + 14.0) * ang.cos(), rcy + (rr + 14.0) * ang.sin());
        c.text_mid(lx - 18.0, ly, 36.0, &o, |t| {
            t.align(Align::Center).styled(stat_abbr(*s), |x| {
                x.size(0.64).weight(700).color(&hexs(stat_paint(&sk, *s)));
            });
        })?;
    }

    // 右:属性条。
    let (bx0, bx1) = (296.0, W - PAD);
    let bw = bx1 - bx0;
    let mut cy = bar0;
    for s in Stat::ALL {
        let v = cur[s.idx()];
        let val = super::logic::aptitude_band(super::logic::soft_ceiling(pots[s.idx()], m.growth));
        let frac = (v as f32 / RADAR_REF).max(0.04);
        let ceil = super::logic::soft_ceiling(pots[s.idx()], m.growth) as f32 / RADAR_REF;
        stat_row(&mut c, &o, &sk, bx0, cy, bw, s.name(), val, stat_paint(&sk, s), frac, ceil)?;
        cy += bar_step;
    }

    // 资源条:体力 / 寿命 / 饱食。寿命按 lifespan_max 归一,余者按 /100。
    c.line(PAD, div2, W - PAD, div2, 1.0, sk.hairline);
    let life_frac = m.lifespan as f32 / m.lifespan_max.max(1) as f32;
    let life_c = if life_frac < 0.40 {
        sk.warm
    } else if life_frac < 0.70 {
        sk.deep
    } else {
        sk.vivid
    };
    let sat_c = if m.satiety < consts::SATIETY_LOW { sk.warm } else { sk.soft };
    // (标签, 当前值, 满格分母, 颜色)。
    let res = [
        ("体力", m.vitality, 100, sk.vivid),
        ("寿命", m.lifespan, m.lifespan_max.max(1), life_c),
        ("饱食", m.satiety, 100, sat_c),
    ];
    let seg = (W - 2.0 * PAD - 2.0 * 18.0) / 3.0;
    for (i, (name, val, max, rc)) in res.iter().enumerate() {
        let rx = PAD + i as f32 * (seg + 18.0);
        c.text_mid(rx, res_y, seg, &o, |t| {
            t.styled(*name, |x| {
                x.size(0.66).color(&hexs(sk.muted));
            })
            .styled(format!("  {val}"), |x| {
                x.size(0.74).weight(700).color(&hexs(*rc)).aside_right();
            });
        })?;
        c.rect(rx, res_bar, seg, 6.0, 3.0, sk.track);
        c.rect(rx, res_bar, seg * (*val as f32 / *max as f32).clamp(0.0, 1.0), 6.0, 3.0, *rc);
    }

    // 特性。
    if let Some(cy) = trait_cy {
        c.text_mid(PAD, cy, 56.0, &o, |t| {
            t.styled("特性", |x| {
                x.size(0.66).color(&hexs(sk.muted));
            });
        })?;
        let mut cx = PAD + 52.0;
        for tr in &traits {
            let cw = 100.0;
            c.rect(cx, cy - 14.0, cw, 28.0, 14.0, alpha(sk.vivid, if sk.dark { 0x33 } else { 0x20 }));
            c.text_mid(cx, cy, cw, &o, |t| {
                t.align(Align::Center).styled(tr.name(), |x| {
                    x.size(0.64).weight(700).color(&hexs(sk.vivid));
                });
            })?;
            cx += cw + 12.0;
        }
    }
    // 伤病。
    if let Some(cy) = injury_cy {
        c.disc(PAD + 6.0, cy, 5.0, sk.warm);
        c.text_mid(PAD + 18.0, cy, W - 2.0 * PAD, &o, |t| {
            t.styled(format!("{} · 先治疗或等它养好", super::logic::injury_name(m.injury)), |x| {
                x.size(0.72).weight(700).color(&hexs(sk.warm));
            });
        })?;
    }
    // 伤痕(后遗症)。
    if let Some(cy) = scar_cy {
        let mins = super::logic::scar_remaining(m).unwrap_or(0);
        let dur = if mins >= 60 { format!("{} 小时 {} 分", mins / 60, mins % 60) } else { format!("{mins} 分") };
        c.disc(PAD + 6.0, cy, 5.0, sk.deep);
        c.text_mid(PAD + 18.0, cy, W - 2.0 * PAD, &o, |t| {
            t.styled(format!("伤痕 · {dur}内易复发,部分属性会暂时变弱"), |x| {
                x.size(0.72).weight(700).color(&hexs(sk.deep));
            });
        })?;
    }
    // 战绩 + 血统。
    let lineage = match (m.father_id, m.mother_id) {
        (Some(f), Some(mo)) => format!("血统 父#{f} · 母#{mo}"),
        _ => "血统 初代".to_string(),
    };
    let rate =
        if m.races > 0 { format!("{}%", (m.wins as f32 / m.races as f32 * 100.0).round()) } else { "—".into() };
    c.line(PAD, div3, W - PAD, div3, 1.0, sk.hairline);
    c.text_mid(PAD, foot_y, W - 2.0 * PAD, &o, |t| {
        t.styled(format!("出战 {} · 胜 {}({rate})    {lineage}", m.races, m.wins), |x| {
            x.size(0.7).color(&hexs(sk.muted));
        });
    })?;
    Ok(c)
}

// 马厩卡

/// 马厩总览卡:逐匹马一行(毛色点 + 名 + 星 + 状态 + 体力),全部居中于行中线。
pub fn stable_card(
    owner: &str,
    title: Option<&str>,
    horses: &[horse::Model],
    cap: usize,
    theme: &UserTheme,
) -> Result<Vec<u8>> {
    stable_canvas(owner, title, horses, cap, theme).and_then(|c| c.encode(OutputFormat::Webp)).context("赛马出图")
}

fn stable_canvas(
    owner: &str,
    title: Option<&str>,
    horses: &[horse::Model],
    cap: usize,
    theme: &UserTheme,
) -> nagisa::render::Result<Canvas> {
    let sk = Skin::of(theme);
    let o = card_opts(theme);
    let active = horses.iter().filter(|h| h.status != 2).count();
    let retired = horses.len() - active;
    let cnt = if retired > 0 {
        format!("在厩 {active}/{cap} · 退役 {retired}")
    } else {
        format!("在厩 {active}/{cap}")
    };
    let sub = match title {
        Some(t) => format!("称号「{t}」 · {cnt}"),
        None => cnt,
    };
    let row_h = 40.0;
    let h = (124.0 + horses.len().max(1) as f32 * row_h + 16.0).max(180.0);
    let mut c = Canvas::new(W, h, o.scale)?;
    paint_bg(&mut c, &sk, W, h);
    let div = title_block(&mut c, &o, &sk, &format!("{owner} 的马厩"), Some(&sub))?;

    if horses.is_empty() {
        c.text_mid(PAD, div + 40.0, W - 2.0 * PAD, &o, |t| {
            t.align(Align::Center).styled("还没有马,发「赛马领养」免费领第一匹", |s| {
                s.size(0.8).color(&hexs(sk.muted));
            });
        })?;
        return Ok(c);
    }
    let first = div + 4.0 + row_h / 2.0;
    for (i, m) in horses.iter().enumerate() {
        let cy = first + i as f32 * row_h;
        if i % 2 == 1 {
            zebra(&mut c, &sk, cy, row_h - 4.0);
        }
        c.disc(PAD + 8.0, cy, 7.0, coat(m.color));
        c.text_mid(PAD + 24.0, cy, 56.0, &o, |t| {
            t.styled(format!("#{}", m.id), |s| {
                s.size(0.66).color(&hexs(sk.muted));
            });
        })?;
        c.text_mid(PAD + 68.0, cy, 230.0, &o, |t| {
            t.styled(m.name.as_str(), |s| {
                s.size(0.84).weight(700).color(&hexs(sk.text));
            })
            .styled(format!("  {}", stars(m.rarity)), |s| {
                s.size(0.58).color(&hexs(sk.warm));
            });
        })?;
        let (tag, sc): (&str, Color) = if m.status == 2 {
            ("退役", sk.muted)
        } else if m.injury > 0 {
            (super::logic::injury_name(m.injury), sk.warm)
        } else if m.scar > 0 {
            ("伤痕", sk.deep)
        } else {
            ("在厩", sk.muted)
        };
        c.text_mid(W - PAD - 170.0, cy, 170.0, &o, |t| {
            t.align(Align::Right).styled(format!("{tag} · 体力 {}", m.vitality), |s| {
                s.size(0.68).color(&hexs(sc));
            });
        })?;
    }
    Ok(c)
}

// 比赛关键帧

/// 比赛关键帧:横向赛道,各马按位置推进(领先者在上),带毛色标记 + 事件标 + 终点线。
pub fn race_frame(result: &RaceResult, round: usize, theme: &UserTheme) -> Result<Vec<u8>> {
    race_frame_canvas(result, round, theme).and_then(|c| c.encode(OutputFormat::Webp)).context("赛马出图")
}

fn race_frame_canvas(result: &RaceResult, round: usize, theme: &UserTheme) -> nagisa::render::Result<Canvas> {
    let sk = Skin::of(theme);
    let o = card_opts(theme);
    let positions = &result.positions[round];
    let n = result.runners.len();
    let lane_h = 38.0;
    let top = 70.0;
    let h = top + n as f32 * lane_h + 18.0;
    let mut c = Canvas::new(W, h, o.scale)?;
    paint_bg(&mut c, &sk, W, h);

    let is_finish = round + 1 >= result.positions.len();
    let title = if round == 0 {
        "起跑！".to_string()
    } else if is_finish {
        "冲线！".to_string()
    } else {
        format!("第 {round} 回合")
    };
    c.text_mid(PAD, 32.0, W - 2.0 * PAD, &o, |t| {
        t.styled(title, |s| {
            s.weight(800).size(1.2).color(&hexs(sk.primary));
        });
    })?;
    c.line(PAD, 58.0, W - PAD, 58.0, 1.0, sk.hairline);

    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| positions[b].total_cmp(&positions[a]));

    let (tx0, tx1) = (150.0, W - PAD - 6.0);
    let track_w = tx1 - tx0;
    for (rank, &i) in order.iter().enumerate() {
        let runner = &result.runners[i];
        let cy = top + rank as f32 * lane_h + lane_h / 2.0;
        let name_col = if runner.is_npc { sk.muted } else { sk.text };
        c.text_mid(PAD, cy, 118.0, &o, |t| {
            t.styled(format!("{}. ", rank + 1), |s| {
                s.size(0.6).color(&hexs(sk.muted));
            })
            .styled(&runner.name, |s| {
                s.size(0.72).weight(if runner.is_npc { 500 } else { 700 }).color(&hexs(name_col));
            });
        })?;
        // 赛道 + 终点线 + 已跑填充 + 马标记。
        c.rect(tx0, cy - 4.0, track_w, 8.0, 4.0, sk.track);
        c.rect(tx1 - 1.0, cy - lane_h / 2.0 + 6.0, 3.0, lane_h - 12.0, 1.0, alpha(sk.warm, 0xaa));
        let frac = (positions[i] / result.track_len).clamp(0.0, 1.0);
        c.rect(tx0, cy - 4.0, track_w * frac, 8.0, 4.0, alpha(if runner.is_npc { sk.muted } else { sk.primary }, 0x99));
        let mx = tx0 + track_w * frac;
        c.disc(mx, cy, 9.0, coat(runner.color));
        c.ring(mx, cy, 9.0, 1.5, if sk.dark { sk.surface_bot } else { sk.surface_top });
        // 事件标。
        let fx = result.event_marks.get(round).and_then(|r| r.get(i)).copied().unwrap_or_default();
        let tag = if fx.injured {
            Some(("伤", sk.warm))
        } else if fx.frozen {
            Some(("冻", sk.deep))
        } else if fx.crit {
            Some(("暴", sk.warm))
        } else if fx.dodged {
            Some(("闪", sk.vivid))
        } else {
            None
        };
        if let Some((label, tc)) = tag {
            // 靠近终点时标记右侧没地方,改放标记左侧,避开与圆标记叠压。
            let bx = if frac > 0.75 { (mx - 30.0).max(tx0) } else { mx + 13.0 };
            c.text_mid(bx, cy, 24.0, &o, |t| {
                t.styled(label, |s| {
                    s.size(0.6).weight(800).color(&hexs(tc));
                });
            })?;
        }
    }
    Ok(c)
}

// 结算 / 对战(名次榜共用)

/// 在 `[top, top+plot_h]` 区域画全程轨迹:底=起跑、顶=终点,每匹马一条毛色折线,交叉即反超。供结算卡内嵌。
fn draw_trajectory(
    c: &mut Canvas,
    o: &RenderOptions,
    sk: &Skin,
    result: &RaceResult,
    top: f32,
    plot_h: f32,
) -> nagisa::render::Result<()> {
    let (px0, px1) = (PAD + 42.0, W - PAD);
    let pw = px1 - px0;
    let py1 = top + plot_h;
    c.rect(px0, top, pw, plot_h, 8.0, sk.panel);
    c.line(px0, top, px1, top, 1.2, alpha(sk.warm, 0xaa)); // 终点线
    c.line(px0, py1, px1, py1, 1.0, sk.hairline); // 起跑线
    c.text_mid(PAD, top, 38.0, o, |t| {
        t.align(Align::Right).styled("终点", |s| {
            s.size(0.52).color(&hexs(sk.warm));
        });
    })?;
    c.text_mid(PAD, py1, 38.0, o, |t| {
        t.align(Align::Right).styled("起跑", |s| {
            s.size(0.52).color(&hexs(sk.muted));
        });
    })?;
    let rounds = result.positions.len();
    let denom = (rounds - 1).max(1) as f32;
    let xat = |r: usize| px0 + r as f32 / denom * pw;
    let yat = |pos: f32| py1 - (pos / result.track_len).clamp(0.0, 1.0) * plot_h;
    // 真人马线垫一圈与底相反明度的描边,任意毛色(含近白的白龙)都能在深 / 浅卡上读出。
    let halo = if sk.dark { Color::rgba(0xff, 0xff, 0xff, 0x4d) } else { Color::rgba(0x10, 0x12, 0x18, 0x4d) };
    for (i, runner) in result.runners.iter().enumerate() {
        // 每条线用自家毛色,和名次榜的毛色点对上(兼作图例);真人线更粗带描边,NPC 线略细略淡。
        let lc = if runner.is_npc { alpha(coat(runner.color), 0xc4) } else { coat(runner.color) };
        let lw = if runner.is_npc { 1.8 } else { 2.6 };
        // 描边整条先走一遍,再覆盖本色,免得后段描边在转折处啃掉前段本色。
        if !runner.is_npc {
            for r in 1..rounds {
                c.line(
                    xat(r - 1),
                    yat(result.positions[r - 1][i]),
                    xat(r),
                    yat(result.positions[r][i]),
                    lw + 1.8,
                    halo,
                );
            }
        }
        for r in 1..rounds {
            c.line(xat(r - 1), yat(result.positions[r - 1][i]), xat(r), yat(result.positions[r][i]), lw, lc);
        }
    }
    Ok(())
}

/// 玩家这场的赛况摘要。
fn race_summary(t: &super::race::RunnerTally) -> String {
    let mut parts = Vec::new();
    if t.crits > 0 {
        parts.push(format!("触发暴击 {} 次", t.crits));
    }
    if t.frozen > 0 {
        parts.push(format!("被冻 {} 回合", t.frozen));
    }
    if t.dodged > 0 {
        parts.push(format!("闪避 {} 次", t.dodged));
    }
    if parts.is_empty() { "全程稳健发挥".to_string() } else { parts.join(" · ") }
}

/// 名次行(名次牌 + 毛色点 + 名字 + 主人 + [标签] + 右尾),全部居中于中线 `cy`。`owner` 非空则在名字后接「· 主人」
/// (区分同名马),`tag` 是小注(如「你的马」/「冠军」),`tail` 是右侧成绩(如奖励)。
#[allow(clippy::too_many_arguments)]
fn place_row(
    c: &mut Canvas,
    o: &RenderOptions,
    sk: &Skin,
    cy: f32,
    rank: usize,
    name: &str,
    owner: &str,
    coat_c: Color,
    emphasize: bool,
    tag: Option<&str>,
    tail: Option<(String, Color)>,
) -> nagisa::render::Result<()> {
    if emphasize {
        c.rect(
            PAD - 6.0,
            cy - 18.0,
            W - 2.0 * PAD + 12.0,
            36.0,
            9.0,
            alpha(sk.primary, if sk.dark { 0x26 } else { 0x16 }),
        );
    }
    rank_badge(c, o, sk, PAD + 14.0, cy, 13.0, rank)?;
    c.disc(PAD + 38.0, cy, 6.0, coat_c);
    // 与底相反明度的细边,让近白的白龙点也能从行底读出。
    c.ring(
        PAD + 38.0,
        cy,
        6.0,
        1.2,
        if sk.dark { Color::rgba(0xff, 0xff, 0xff, 0x3a) } else { Color::rgba(0x10, 0x12, 0x18, 0x3a) },
    );
    // 名字 / 主人 / 标签各自居中于中线 cy(小字号才不相对大字号下沉),按墨迹实宽往右摆。
    let name_col = if emphasize { sk.primary } else { sk.text };
    let mut x = PAD + 52.0;
    let adv = c.text_mid(x, cy, 300.0, o, |t| {
        t.styled(name, |s| {
            s.size(0.86).weight(if emphasize { 800 } else { 600 }).color(&hexs(name_col));
        });
    })?;
    x += adv + 12.0;
    if !owner.is_empty() {
        let adv = c.text_mid(x, cy, 220.0, o, |t| {
            t.styled(format!("· {owner}"), |s| {
                s.size(0.62).color(&hexs(sk.muted));
            });
        })?;
        x += adv + 12.0;
    }
    if let Some(tag) = tag {
        let tag_col = if tag == "冠军" { sk.warm } else { sk.muted };
        c.text_mid(x, cy, 140.0, o, |t| {
            t.styled(tag, |s| {
                s.size(0.62).weight(700).color(&hexs(tag_col));
            });
        })?;
    }
    if let Some((txt, tc)) = tail {
        c.text_mid(W - PAD - 150.0, cy, 150.0, o, |t| {
            t.align(Align::Right).styled(txt, |s| {
                s.size(0.82).weight(700).color(&hexs(tc));
            });
        })?;
    }
    Ok(())
}

/// PvE 结算卡:名次榜(玩家高亮 + 「你的马」)+ 名次/奖励 + 每日首胜 + 伤病 + 赛况摘要,结算内容全在这一张。
/// `bonus` 为每日首胜奖(0 = 无),`injury` 为本场受伤等级(0 = 无)。
pub fn result_card(
    result: &RaceResult,
    reward: i64,
    bonus: i64,
    injury: i16,
    difficulty_name: &str,
    theme: &UserTheme,
) -> Result<Vec<u8>> {
    result_canvas(result, reward, bonus, injury, difficulty_name, theme)
        .and_then(|c| c.encode(OutputFormat::Webp))
        .context("赛马出图")
}

fn result_canvas(
    result: &RaceResult,
    reward: i64,
    bonus: i64,
    injury: i16,
    difficulty_name: &str,
    theme: &UserTheme,
) -> nagisa::render::Result<Canvas> {
    let sk = Skin::of(theme);
    let o = card_opts(theme);
    let n = result.runners.len();
    let row_h = 36.0;
    let (chart_top, plot_h) = (118.0, 142.0);
    let rank_top = chart_top + plot_h + 14.0;
    // 结算块:奖励横幅 + 赛况摘要;有首胜 / 伤病各加一行。
    let extra = if bonus > 0 { 28.0 } else { 0.0 } + if injury > 0 { 28.0 } else { 0.0 };
    let h = rank_top + n as f32 * row_h + 107.0 + extra;
    let mut c = Canvas::new(W, h, o.scale)?;
    paint_bg(&mut c, &sk, W, h);
    title_block(&mut c, &o, &sk, "比赛结果", Some(&format!("{difficulty_name}赛")))?;

    // 全程轨迹(节点历史)内嵌结算卡。
    draw_trajectory(&mut c, &o, &sk, result, chart_top, plot_h)?;

    // 名次榜(毛色点对应轨迹线色,兼作图例)。
    let first = rank_top + row_h / 2.0;
    for (rank, &i) in result.order.iter().enumerate() {
        let runner = &result.runners[i];
        let is_player = i == result.player_idx;
        let cy = first + rank as f32 * row_h;
        place_row(
            &mut c,
            &o,
            &sk,
            cy,
            rank + 1,
            &runner.name,
            &runner.owner,
            coat(runner.color),
            is_player,
            is_player.then_some("你的马"),
            None,
        )?;
    }
    // 结算块。
    let banner = rank_top + n as f32 * row_h + 14.0;
    c.line(PAD, banner, W - PAD, banner, 1.0, sk.hairline);
    let prize = if reward > 0 { format!("+{reward} 游戏币") } else { "无奖励".into() };
    let mut y = banner + 30.0;
    c.text_mid(PAD, y, W - 2.0 * PAD, &o, |t| {
        t.styled(format!("你的马 第 {} 名", result.player_place), |s| {
            s.size(0.92).weight(700).color(&hexs(sk.text));
        })
        .styled(format!("   {prize}"), |s| {
            s.size(0.92).weight(800).color(&hexs(if reward > 0 { sk.warm } else { sk.muted }));
        });
    })?;
    y += 34.0;
    if bonus > 0 {
        c.disc(PAD + 6.0, y, 5.0, sk.warm);
        c.text_mid(PAD + 18.0, y, W - 2.0 * PAD, &o, |t| {
            t.styled(format!("今日首胜,额外 +{bonus} 游戏币"), |s| {
                s.size(0.74).weight(700).color(&hexs(sk.warm));
            });
        })?;
        y += 28.0;
    }
    if injury > 0 {
        c.disc(PAD + 6.0, y, 5.0, sk.deep);
        c.text_mid(PAD + 18.0, y, W - 2.0 * PAD, &o, |t| {
            t.styled(
                format!("这一场让马受了{},先治疗或等它养好", super::logic::injury_name(injury)),
                |s| {
                    s.size(0.74).weight(700).color(&hexs(sk.deep));
                },
            );
        })?;
        y += 28.0;
    }
    if let Some(tl) = result.tallies.get(result.player_idx) {
        c.text_mid(PAD, y, W - 2.0 * PAD, &o, |t| {
            t.styled(format!("赛况:{}", race_summary(tl)), |s| {
                s.size(0.72).color(&hexs(sk.muted));
            });
        })?;
    }
    Ok(c)
}

/// PvP 结算卡:全名次(冠军金牌高亮 + 「冠军」)+ 各名次分得的奖池。群内观赛卡,不带「你的马」视角。
pub fn pvp_result_card(result: &RaceResult, shares: &[i64], rake: i64, theme: &UserTheme) -> Result<Vec<u8>> {
    pvp_result_canvas(result, shares, rake, theme).and_then(|c| c.encode(OutputFormat::Webp)).context("赛马出图")
}

fn pvp_result_canvas(
    result: &RaceResult,
    shares: &[i64],
    rake: i64,
    theme: &UserTheme,
) -> nagisa::render::Result<Canvas> {
    let sk = Skin::of(theme);
    let o = card_opts(theme);
    let n = result.runners.len();
    let row_h = 36.0;
    let (chart_top, plot_h) = (80.0, 142.0);
    let rank_top = chart_top + plot_h + 14.0;
    let h = rank_top + n as f32 * row_h + 64.0; // 末尾派彩行到卡底留 ~18px
    let mut c = Canvas::new(W, h, o.scale)?;
    paint_bg(&mut c, &sk, W, h);
    title_block(&mut c, &o, &sk, "对战结果", None)?;

    // 全程轨迹(节点历史)内嵌结算卡。
    draw_trajectory(&mut c, &o, &sk, result, chart_top, plot_h)?;

    // 名次榜(毛色点对应轨迹线色,兼作图例;冠军金牌高亮)。
    let first = rank_top + row_h / 2.0;
    for (rank, &i) in result.order.iter().enumerate() {
        let runner = &result.runners[i];
        let share = shares.get(rank).copied().unwrap_or(0);
        let tail = if share > 0 { Some((format!("+{share}"), sk.warm)) } else { None };
        let tag = if rank == 0 { Some("冠军") } else { None };
        place_row(
            &mut c,
            &o,
            &sk,
            first + rank as f32 * row_h,
            rank + 1,
            &runner.name,
            &runner.owner,
            coat(runner.color),
            rank == 0,
            tag,
            tail,
        )?;
    }
    let banner = rank_top + n as f32 * row_h + 14.0;
    c.line(PAD, banner, W - PAD, banner, 1.0, sk.hairline);
    let total: i64 = shares.iter().sum();
    let txt = if total > 0 {
        format!("奖池派彩 {total} 游戏币(抽水 {rake})")
    } else {
        "友谊赛,无奖池".to_string()
    };
    c.text_mid(PAD, banner + 24.0, W - 2.0 * PAD, &o, |t| {
        t.align(Align::Center).styled(txt, |s| {
            s.size(0.82).weight(700).color(&hexs(sk.warm));
        });
    })?;
    Ok(c)
}

// 背包卡

/// 背包卡:逐种物品一行(名 + 效果 + 数量),道具 / 饲料分色。
pub fn backpack_card(owner: &str, items: &[(Item, i32)], theme: &UserTheme) -> Result<Vec<u8>> {
    backpack_canvas(owner, items, theme).and_then(|c| c.encode(OutputFormat::Webp)).context("赛马出图")
}

fn backpack_canvas(owner: &str, items: &[(Item, i32)], theme: &UserTheme) -> nagisa::render::Result<Canvas> {
    let sk = Skin::of(theme);
    let o = card_opts(theme);
    let row_h = L2_ROW_H;
    let h = (78.0 + items.len().max(1) as f32 * row_h + 16.0).max(150.0);
    let mut c = Canvas::new(W, h, o.scale)?;
    paint_bg(&mut c, &sk, W, h);
    let div = title_block(&mut c, &o, &sk, &format!("{owner} 的背包"), None)?;

    if items.is_empty() {
        c.text_mid(PAD, div + 40.0, W - 2.0 * PAD, &o, |t| {
            t.align(Align::Center).styled("背包空空的,抽卡或比赛掉落能得道具", |s| {
                s.size(0.8).color(&hexs(sk.muted));
            });
        })?;
        return Ok(c);
    }
    let first = div + 4.0 + row_h / 2.0;
    for (i, (it, qty)) in items.iter().enumerate() {
        let cy = first + i as f32 * row_h;
        if i % 2 == 1 {
            zebra(&mut c, &sk, cy, row_h - 2.0);
        }
        let dot = match it.kind() {
            consts::ItemKind::Train => sk.vivid,
            consts::ItemKind::Use => sk.deep,
            consts::ItemKind::Race => sk.primary,
        };
        // 名(上)/ 说明(下)整块居中于 cy;圆点与数量直接居中于 cy → 与整块同高。
        c.disc(PAD + 10.0, cy, 5.0, dot);
        c.text_mid(PAD + 24.0, cy + L2_NAME_DY, 360.0, &o, |t| {
            t.styled(it.name(), |s| {
                s.size(0.84).weight(700).color(&hexs(sk.text));
            });
        })?;
        c.text_mid(PAD + 24.0, cy + L2_SUB_DY, 360.0, &o, |t| {
            t.styled(it.effect_desc(), |s| {
                s.size(0.62).color(&hexs(sk.muted));
            });
        })?;
        c.text_mid(W - PAD - 70.0, cy, 70.0, &o, |t| {
            t.align(Align::Right).styled(format!("×{qty}"), |s| {
                s.size(0.84).weight(800).color(&hexs(sk.warm));
            });
        })?;
    }
    Ok(c)
}

// 抽卡卡

/// 抽卡结果一行。
pub struct GachaLine {
    pub text: String,
    /// 稀有则高亮。
    pub rare: bool,
}

/// 抽卡结果卡:逐条产出,稀有高亮。
pub fn gacha_card(title: &str, lines: &[GachaLine], theme: &UserTheme) -> Result<Vec<u8>> {
    gacha_canvas(title, lines, theme).and_then(|c| c.encode(OutputFormat::Webp)).context("赛马出图")
}

fn gacha_canvas(title: &str, lines: &[GachaLine], theme: &UserTheme) -> nagisa::render::Result<Canvas> {
    let sk = Skin::of(theme);
    let o = card_opts(theme);
    let row_h = 40.0;
    let h = (78.0 + lines.len().max(1) as f32 * row_h + 16.0).max(150.0);
    let mut c = Canvas::new(W, h, o.scale)?;
    paint_bg(&mut c, &sk, W, h);
    let div = title_block(&mut c, &o, &sk, &format!("赛马 · {title}"), None)?;

    let first = div + 4.0 + row_h / 2.0;
    for (i, line) in lines.iter().enumerate() {
        let cy = first + i as f32 * row_h;
        if line.rare {
            // chip 填充比行高窄,相邻两条高亮间留得出暗缝。
            c.rect(
                PAD - 6.0,
                cy - row_h / 2.0 + 4.0,
                W - 2.0 * PAD + 12.0,
                row_h - 8.0,
                8.0,
                alpha(sk.warm, if sk.dark { 0x26 } else { 0x1c }),
            );
        }
        c.disc(PAD + 7.0, cy, 4.0, if line.rare { sk.warm } else { sk.muted });
        let tc = if line.rare { sk.warm } else { sk.text };
        c.text_mid(PAD + 20.0, cy, W - 2.0 * PAD - 20.0, &o, |t| {
            t.styled(&line.text, |s| {
                s.size(0.78).weight(if line.rare { 800 } else { 500 }).color(&hexs(tc));
            });
        })?;
    }
    Ok(c)
}

// 榜

/// 赛马榜一行。
pub struct RankRow {
    pub horse: String,
    pub rarity: i16,
    pub owner: String,
    pub stat: String,
}

/// 赛马榜卡。
pub fn rank_card(title: &str, rows: &[RankRow], theme: &UserTheme) -> Result<Vec<u8>> {
    rank_canvas(title, rows, theme).and_then(|c| c.encode(OutputFormat::Webp)).context("赛马出图")
}

fn rank_canvas(title: &str, rows: &[RankRow], theme: &UserTheme) -> nagisa::render::Result<Canvas> {
    let sk = Skin::of(theme);
    let o = card_opts(theme);
    let row_h = L2_ROW_H;
    let h = (78.0 + rows.len().max(1) as f32 * row_h + 16.0).max(150.0);
    let mut c = Canvas::new(W, h, o.scale)?;
    paint_bg(&mut c, &sk, W, h);
    let div = title_block(&mut c, &o, &sk, title, None)?;

    if rows.is_empty() {
        c.text_mid(PAD, div + 40.0, W - 2.0 * PAD, &o, |t| {
            t.align(Align::Center).styled("还没有达标的马,快去比赛吧", |s| {
                s.size(0.8).color(&hexs(sk.muted));
            });
        })?;
        return Ok(c);
    }
    let first = div + 4.0 + row_h / 2.0;
    for (i, r) in rows.iter().enumerate() {
        let cy = first + i as f32 * row_h;
        if i % 2 == 1 {
            zebra(&mut c, &sk, cy, row_h - 2.0);
        }
        // 名 + 星(上)/ 主人(下)整块居中于 cy;名次牌与成绩直接居中于 cy → 与整块同高。
        rank_badge(&mut c, &o, &sk, PAD + 16.0, cy, 14.0, i + 1)?;
        let adv = c.text_mid(PAD + 40.0, cy + L2_NAME_DY, 240.0, &o, |t| {
            t.styled(&r.horse, |s| {
                s.size(0.84).weight(700).color(&hexs(sk.text));
            });
        })?;
        c.text_mid(PAD + 40.0 + adv + 10.0, cy + L2_NAME_DY, 120.0, &o, |t| {
            t.styled(stars(r.rarity), |s| {
                s.size(0.56).color(&hexs(sk.warm));
            });
        })?;
        c.text_mid(PAD + 40.0, cy + L2_SUB_DY, 240.0, &o, |t| {
            t.styled(format!("by {}", r.owner), |s| {
                s.size(0.6).color(&hexs(sk.muted));
            });
        })?;
        c.text_mid(W - PAD - 170.0, cy, 170.0, &o, |t| {
            t.align(Align::Right).styled(&r.stat, |s| {
                s.size(0.8).weight(700).color(&hexs(sk.primary));
            });
        })?;
    }
    Ok(c)
}

// 成就卡

/// 成就墙卡:当前称号 + 全部成就(已达成高亮 + 一次性奖励)。
pub fn achievement_card(owner: &str, title: Option<&str>, earned: &HashSet<i32>, theme: &UserTheme) -> Result<Vec<u8>> {
    achievement_canvas(owner, title, earned, theme).and_then(|c| c.encode(OutputFormat::Webp)).context("赛马出图")
}

fn achievement_canvas(
    owner: &str,
    title: Option<&str>,
    earned: &HashSet<i32>,
    theme: &UserTheme,
) -> nagisa::render::Result<Canvas> {
    let sk = Skin::of(theme);
    let o = card_opts(theme);
    let all = Achievement::ALL;
    let done = all.iter().filter(|a| earned.contains(&a.code())).count();
    let sub = match title {
        Some(t) => format!("称号「{t}」 · 已达成 {done}/{}", all.len()),
        None => format!("暂无称号(达成带称号的成就解锁) · 已达成 {done}/{}", all.len()),
    };
    let row_h = L2_ROW_H;
    let h = 110.0 + all.len() as f32 * row_h + 18.0;
    let mut c = Canvas::new(W, h, o.scale)?;
    paint_bg(&mut c, &sk, W, h);
    let div = title_block(&mut c, &o, &sk, &format!("{owner} 的成就"), Some(&sub))?;

    let first = div + 4.0 + row_h / 2.0;
    for (i, a) in all.iter().enumerate() {
        let cy = first + i as f32 * row_h;
        let got = earned.contains(&a.code());
        if got {
            zebra(&mut c, &sk, cy, row_h - 2.0);
        }
        let name_c = if got { sk.text } else { sk.muted };
        let reward_c = if got { sk.warm } else { sk.muted };
        // 名(+称号,上)/ 说明(下)整块居中于 cy;圆点与奖励直接居中于 cy → 与整块同高。
        c.disc(PAD + 12.0, cy, 6.0, if got { sk.primary } else { sk.track });
        let adv = c.text_mid(PAD + 28.0, cy + L2_NAME_DY, 240.0, &o, |t| {
            t.styled(a.name(), |s| {
                s.size(0.84).weight(if got { 800 } else { 500 }).color(&hexs(name_c));
            });
        })?;
        if let Some(tl) = a.title() {
            c.text_mid(PAD + 28.0 + adv + 10.0, cy + L2_NAME_DY, 200.0, &o, |t| {
                t.styled(format!("称号「{tl}」"), |s| {
                    s.size(0.56).color(&hexs(sk.vivid));
                });
            })?;
        }
        c.text_mid(PAD + 28.0, cy + L2_SUB_DY, 360.0, &o, |t| {
            t.styled(a.desc(), |s| {
                s.size(0.6).color(&hexs(sk.muted));
            });
        })?;
        c.text_mid(W - PAD - 64.0, cy, 64.0, &o, |t| {
            t.align(Align::Right).styled(format!("+{}", a.reward()), |s| {
                s.size(0.84).weight(700).color(&hexs(reward_c));
            });
        })?;
    }
    Ok(c)
}

// 设施卡

/// 设施卡的一行(账号级一栋设施)。
pub struct FacilityRow {
    pub name: &'static str,
    pub effect: &'static str,
    pub lv: i16,
    pub max_lv: i16,
    /// 升下一级造价;`None` = 已满级。
    pub next_cost: Option<i64>,
}

/// 账号马场设施卡:四栋等级 / 作用 / 下一级造价 + 珍爱槽 + 余额。
pub fn facility_card(
    owner: &str,
    rows: &[FacilityRow],
    cherish_used: usize,
    cherish_cap: usize,
    coin: i64,
    theme: &UserTheme,
) -> Result<Vec<u8>> {
    facility_canvas(owner, rows, cherish_used, cherish_cap, coin, theme)
        .and_then(|c| c.encode(OutputFormat::Webp))
        .context("赛马出图")
}

fn facility_canvas(
    owner: &str,
    rows: &[FacilityRow],
    cherish_used: usize,
    cherish_cap: usize,
    coin: i64,
    theme: &UserTheme,
) -> nagisa::render::Result<Canvas> {
    let sk = Skin::of(theme);
    let o = card_opts(theme);
    let row_h = L2_ROW_H;
    let h = 110.0 + rows.len() as f32 * row_h + 78.0;
    let mut c = Canvas::new(W, h, o.scale)?;
    paint_bg(&mut c, &sk, W, h);
    let div = title_block(&mut c, &o, &sk, &format!("{owner} 的马场"), Some("账号设施 · 升级惠及名下全部马"))?;
    let first = div + 4.0 + row_h / 2.0;
    let dots = [sk.primary, sk.warm, sk.vivid, sk.deep];
    for (i, r) in rows.iter().enumerate() {
        let cy = first + i as f32 * row_h;
        if i % 2 == 1 {
            zebra(&mut c, &sk, cy, row_h - 2.0);
        }
        c.disc(PAD + 10.0, cy, 5.0, dots[i % dots.len()]);
        c.text_mid(PAD + 24.0, cy + L2_NAME_DY, 320.0, &o, |t| {
            t.styled(r.name, |s| {
                s.size(0.84).weight(700).color(&hexs(sk.text));
            });
        })?;
        c.text_mid(PAD + 24.0, cy + L2_SUB_DY, 360.0, &o, |t| {
            t.styled(r.effect, |s| {
                s.size(0.56).color(&hexs(sk.muted));
            });
        })?;
        c.text_mid(W - PAD - 160.0, cy + L2_NAME_DY, 160.0, &o, |t| {
            t.align(Align::Right).styled(format!("Lv {}/{}", r.lv, r.max_lv), |s| {
                s.size(0.8).weight(800).color(&hexs(sk.warm));
            });
        })?;
        let foot = match r.next_cost {
            Some(cost) => format!("升级 {cost} 币"),
            None => "已满级".to_string(),
        };
        c.text_mid(W - PAD - 160.0, cy + L2_SUB_DY, 160.0, &o, |t| {
            t.align(Align::Right).styled(foot, |s| {
                s.size(0.6).color(&hexs(if r.next_cost.is_some() { sk.muted } else { sk.soft }));
            });
        })?;
    }
    let fy = div + rows.len() as f32 * row_h + 22.0;
    c.line(PAD, fy, W - PAD, fy, 1.0, sk.hairline);
    c.text_mid(PAD, fy + 26.0, W - 2.0 * PAD, &o, |t| {
        t.align(Align::Center).styled(
            format!("珍爱马投资槽 {cherish_used}/{cherish_cap}　·　余额 {coin} 币"),
            |s| {
                s.size(0.66).color(&hexs(sk.muted));
            },
        );
    })?;
    Ok(c)
}

// 血统库卡

/// 血统库卡:库内种马一览(星级 / 代数 / 编号)。
pub fn bloodline_card(owner: &str, studs: &[horse::Model], cap: usize, theme: &UserTheme) -> Result<Vec<u8>> {
    bloodline_canvas(owner, studs, cap, theme).and_then(|c| c.encode(OutputFormat::Webp)).context("赛马出图")
}

fn bloodline_canvas(
    owner: &str,
    studs: &[horse::Model],
    cap: usize,
    theme: &UserTheme,
) -> nagisa::render::Result<Canvas> {
    let sk = Skin::of(theme);
    let o = card_opts(theme);
    let row_h = L2_ROW_H;
    let h = (110.0 + studs.len().max(1) as f32 * row_h + 16.0).max(180.0);
    let mut c = Canvas::new(W, h, o.scale)?;
    paint_bg(&mut c, &sk, W, h);
    let div = title_block(
        &mut c,
        &o,
        &sk,
        &format!("{owner} 的血统库"),
        Some(&format!("种马 {}/{} · 配种「赛马繁殖 <母> <种马>」", studs.len(), cap)),
    )?;
    if studs.is_empty() {
        c.text_mid(PAD, div + 40.0, W - 2.0 * PAD, &o, |t| {
            t.align(Align::Center).styled("库里还没有种马,「赛马存种 <编号>」把好马存进来", |s| {
                s.size(0.8).color(&hexs(sk.muted));
            });
        })?;
        return Ok(c);
    }
    let first = div + 4.0 + row_h / 2.0;
    for (i, m) in studs.iter().enumerate() {
        let cy = first + i as f32 * row_h;
        if i % 2 == 1 {
            zebra(&mut c, &sk, cy, row_h - 2.0);
        }
        c.disc(PAD + 10.0, cy, 5.0, coat(m.color));
        c.text_mid(PAD + 24.0, cy + L2_NAME_DY, 360.0, &o, |t| {
            t.styled(&m.name, |s| {
                s.size(0.84).weight(700).color(&hexs(sk.text));
            });
        })?;
        c.text_mid(PAD + 24.0, cy + L2_SUB_DY, 360.0, &o, |t| {
            t.styled(format!("第 {} 代 · #{}", m.generation, m.id), |s| {
                s.size(0.6).color(&hexs(sk.muted));
            });
        })?;
        c.text_mid(W - PAD - 130.0, cy, 130.0, &o, |t| {
            t.align(Align::Right).styled(stars(m.rarity), |s| {
                s.size(0.82).weight(700).color(&hexs(sk.warm));
            });
        })?;
    }
    Ok(c)
}

// ——————————————————————————— 玩法说明(渲染 README) ———————————————————————————

/// 赛马 README(随二进制内嵌),`赛马玩法` 渲成图发出,与文档同源。
const GUIDE_MD: &str = include_str!("README.md");

/// 把赛马 README 渲成一张玩法说明图。markdown 解析直接走 nagisa 的 [`parse_markup`]
/// (GFM:标题/段落/列表/表格/分隔线 + 内联粗体·代码),文字多故比常规卡片放宽到 1400。
pub fn guide_image(theme: &UserTheme) -> Result<Vec<u8>> {
    let opts = theme.opts().with_width(1400.0).with_padding(Insets::symmetric(28.0, 40.0));
    let mut doc = parse_markup(GUIDE_MD).context("解析玩法文档")?;
    // parse_markup 出的表格默认自然宽、左对齐;改成铺满可用宽(README 表格都在顶层)。
    for block in &mut doc.blocks {
        if let Block::Table(t) = block {
            t.style.expand = true;
        }
    }
    render_document(&doc, &opts).context("赛马玩法出图")
}

#[cfg(test)]
mod preview {
    use super::super::race;
    use super::*;

    fn sample(id: i64, name: &str, spd: i32, color: i16, traits: i32, injury: i16) -> horse::Model {
        let t = chrono::DateTime::parse_from_rfc3339("2026-06-20T12:00:00+08:00").unwrap();
        horse::Model {
            id,
            owner_uin: 1,
            name: name.into(),
            color,
            sex: (id % 2) as i16,
            generation: 4,
            rarity: 3,
            traits,
            // 列存厘点(× STAT_SCALE),展示走 stats_of 折回点数。
            spd: spd * consts::STAT_SCALE,
            sta: 96 * consts::STAT_SCALE,
            brs: 120 * consts::STAT_SCALE,
            agi: 64 * consts::STAT_SCALE,
            luk: 88 * consts::STAT_SCALE,
            pot_spd: 100,
            pot_sta: 72,
            pot_brs: 88,
            pot_agi: 48,
            pot_luk: 60,
            growth: 115,
            vitality: 72,
            satiety: 24,
            state_at: t,
            lifespan: 800,
            lifespan_cap: 800,
            lifespan_max: 800,
            injury,
            injury_until: None,
            scar: 0,
            scar_until: None,
            breed_cd_until: None,
            breed_count: 0,
            status: if id == 8 { 2 } else { 0 },
            wins: 38,
            races: 57,
            train_day: t.date_naive(),
            train_today: 0,
            race_day: t.date_naive(),
            race_today: 0,
            bonus_day: t.date_naive(),
            season_key: String::new(),
            season_wins: 0,
            invested: 20000,
            train_total: 120,
            acq_seq: 10,
            elo: consts::ELO_INIT,
            elo_games: 0,
            desk_lv: 0,
            prep_lv: 0,
            father_id: Some(3),
            mother_id: Some(5),
            created_at: t,
        }
    }

    /// PvP 样本一行:名、主人、毛色、五维、特性位、道具。
    type PvpSpec = (&'static str, &'static str, i16, [i32; 5], i32, &'static [Item]);

    /// 全真人 PvP 样本:5 匹各异毛色、各有主人名,跑一场可复现的对战(供对战结果卡 / 对齐回归共用)。
    fn pvp_sample() -> race::RaceResult {
        let specs: [PvpSpec; 5] = [
            ("疾风踏雪", "A60", 3, [132, 96, 120, 64, 88], consts::Trait::LateSurge.bit(), &[Item::Boost]),
            ("追风", "老王", 5, [112, 104, 92, 82, 74], 0, &[]),
            ("赤兔", "小李", 0, [104, 92, 112, 72, 92], consts::Trait::CritBeast.bit(), &[]),
            ("影疾", "阿强", 4, [96, 86, 100, 98, 76], 0, &[Item::Clover]),
            ("黑旋风", "Momo", 2, [88, 80, 96, 90, 82], 0, &[]),
        ];
        let entrants: Vec<_> = specs
            .iter()
            .map(|(name, owner, color, stats, traits, items)| race::PvpEntrant {
                info: race::RunnerInfo { name: (*name).into(), owner: (*owner).into(), color: *color, is_npc: false },
                stats: *stats,
                traits: *traits,
                items: items.to_vec(),
                life_frac: 1.0,
                scar: 0,
                races: i32::MAX,
            })
            .collect();
        race::simulate_pvp(entrants, consts::PVP_TRACK_LEN, 7)
    }

    /// 每张卡在亮 / 暗主题下都能渲染出非空 WebP(不 panic、尺寸合法)——守住所有卡片路径不回归。
    #[test]
    fn all_cards_render() {
        for theme in [UserTheme::resolve("dark", ""), UserTheme::resolve("light", "teal")] {
            let info = race::RunnerInfo { name: "疾风踏雪".into(), owner: "A60".into(), color: 3, is_npc: false };
            let r = race::simulate(
                info,
                [132, 96, 120, 64, 88],
                consts::Trait::LateSurge.bit(),
                race::InjuryCtx { life_frac: 1.0, scar: 0, races: i32::MAX },
                consts::Difficulty::Normal,
                &[],
                7,
            );
            let m = sample(7, "疾风踏雪", 132, 3, consts::Trait::LateSurge.bit() | consts::Trait::CritBeast.bit(), 1);
            assert!(!horse_card(&m, "A60 #42", &theme).unwrap().is_empty());
            let last = r.positions.len() - 1;
            assert!(!race_frame(&r, 0, &theme).unwrap().is_empty());
            assert!(!race_frame(&r, last, &theme).unwrap().is_empty());
            assert!(!result_card(&r, 60, 40, 1, "普通", &theme).unwrap().is_empty());
            assert!(!pvp_result_card(&r, &[60, 25, 15], 5, &theme).unwrap().is_empty());
            let horses: Vec<_> = (1..=6)
                .map(|i| sample(i, &format!("赛马{i}号"), 80 + i as i32 * 8, (i % 6) as i16, 0, (i % 3) as i16))
                .collect();
            assert!(!stable_card("A60", Some("名门"), &horses, consts::STABLE_CAP, &theme).unwrap().is_empty());
            assert!(!stable_card("A60", None, &[], consts::STABLE_CAP, &theme).unwrap().is_empty());
            let items = vec![(Item::Boost, 12), (Item::Feed1, 7)];
            assert!(!backpack_card("A60", &items, &theme).unwrap().is_empty());
            assert!(!backpack_card("A60", &[], &theme).unwrap().is_empty());
            let lines = vec![
                GachaLine { text: "道具 · 冲刺".into(), rare: false },
                GachaLine { text: "3★ 新马 · 逐日 #42".into(), rare: true },
            ];
            assert!(!gacha_card("十连", &lines, &theme).unwrap().is_empty());
            let rows = vec![RankRow {
                horse: "赛马1号".into(),
                rarity: 4,
                owner: "玩家1".into(),
                stat: "胜 39 / 59 场".into(),
            }];
            assert!(!rank_card("赛马 · 胜场榜", &rows, &theme).unwrap().is_empty());
            assert!(!rank_card("赛马 · 胜率榜", &[], &theme).unwrap().is_empty());
            let earned: HashSet<i32> = [1, 3, 5].into_iter().collect();
            assert!(!achievement_card("A60", Some("天选"), &earned, &theme).unwrap().is_empty());
            let frows = vec![
                FacilityRow {
                    name: "训练场", effect: "每级降训练费 5%", lv: 3, max_lv: 8, next_cost: Some(1613)
                },
                FacilityRow { name: "仓库", effect: "每级 +1 在厩上限", lv: 8, max_lv: 8, next_cost: None },
            ];
            assert!(!facility_card("A60", &frows, 2, 3, 12345, &theme).unwrap().is_empty());
            assert!(!bloodline_card("A60", &horses, 6, &theme).unwrap().is_empty());
            assert!(!bloodline_card("A60", &[], 6, &theme).unwrap().is_empty());
            // 玩法说明图(渲染 README):两主题下都能渲出非空图、不 panic。
            assert!(!guide_image(&theme).unwrap().is_empty());
        }
    }

    /// 出图画廊:每张卡渲成 PNG 落到 `/tmp/horsecards/`,供肉眼审稿。默认 `#[ignore]`,手动跑:
    /// `cargo test -p abot horse::render::preview::gallery -- --ignored --nocapture`。
    #[test]
    #[ignore]
    fn gallery() {
        let dir = "/tmp/horsecards";
        std::fs::create_dir_all(dir).unwrap();
        let put = |name: &str, c: Canvas| {
            std::fs::write(format!("{dir}/{name}.png"), c.encode(OutputFormat::Png).unwrap()).unwrap();
        };
        let dk = UserTheme::resolve("dark", "");
        let lt = UserTheme::resolve("light", "teal");
        let pk = UserTheme::resolve("dark", "pink");

        let strong = sample(7, "疾风踏雪", 132, 3, consts::Trait::LateSurge.bit() | consts::Trait::CritBeast.bit(), 1);
        put("01_horse_dark", horse_canvas(&strong, "A60 #42", &dk).unwrap());
        put("02_horse_light", horse_canvas(&strong, "A60 #42", &lt).unwrap());
        let mut weak = sample(2, "踏云", 28, 5, 0, 0);
        weak.rarity = 2;
        weak.generation = 1;
        weak.father_id = None;
        weak.mother_id = None;
        put("03_horse_fresh", horse_canvas(&weak, "新人 #7", &dk).unwrap());

        let info = race::RunnerInfo { name: "疾风踏雪".into(), owner: "A60".into(), color: 3, is_npc: false };
        let r = race::simulate(
            info,
            [132, 96, 120, 64, 88],
            consts::Trait::LateSurge.bit(),
            race::InjuryCtx { life_frac: 1.0, scar: 0, races: i32::MAX },
            consts::Difficulty::Normal,
            &[Item::Boost],
            7,
        );
        let mid = r.key_rounds.get(r.key_rounds.len() / 2).copied().unwrap_or(0);
        let last = r.positions.len() - 1;
        put("04_race_start", race_frame_canvas(&r, 0, &dk).unwrap());
        put("05_race_mid", race_frame_canvas(&r, mid, &dk).unwrap());
        put("06_race_finish", race_frame_canvas(&r, last, &dk).unwrap());
        put("07_result", result_canvas(&r, 60, 40, 1, "普通", &dk).unwrap());
        put("08_pvp", pvp_result_canvas(&pvp_sample(), &[60, 30, 15], 5, &lt).unwrap());

        let horses: Vec<_> = (1..=8)
            .map(|i| {
                let mut hh = sample(
                    i,
                    &format!("赛马{i}号"),
                    70 + i as i32 * 10,
                    (i % 6) as i16,
                    if i % 3 == 0 { consts::Trait::Genius.bit() } else { 0 },
                    (i % 3) as i16,
                );
                hh.rarity = ((i % 4) + 1) as i16;
                hh
            })
            .collect();
        put("09_stable", stable_canvas("A60", Some("名门"), &horses, consts::STABLE_CAP, &dk).unwrap());

        let items = vec![
            (Item::Boost, 12),
            (Item::Mark, 3),
            (Item::Reflect, 1),
            (Item::Scare, 5),
            (Item::Clover, 2),
            (Item::Feed1, 7),
            (Item::BreakPill, 2),
            (Item::EnergyDrink, 4),
            (Item::ReachTonic, 1),
            (Item::Dye, 3),
        ];
        put("10_backpack", backpack_canvas("A60", &items, &lt).unwrap());

        let lines = vec![
            GachaLine { text: "道具 · 冲刺".into(), rare: false },
            GachaLine { text: "饲料 · 精饲料".into(), rare: false },
            GachaLine { text: "4★ 新马 · 逐日 #42".into(), rare: true },
            GachaLine { text: "道具 · 四叶草".into(), rare: true },
            GachaLine { text: "距 ★3+ 保底还差 38 抽".into(), rare: false },
        ];
        put("11_gacha", gacha_canvas("十连", &lines, &pk).unwrap());

        let rows: Vec<_> = (1..=8)
            .map(|i| RankRow {
                horse: format!("赛马{i}号"),
                rarity: ((i % 4) + 1) as i16,
                owner: format!("玩家{i}"),
                stat: format!("胜 {} / {} 场", 45 - i * 3, 70 - i * 3),
            })
            .collect();
        put("12_rank", rank_canvas("赛马 · 胜场榜", &rows, &dk).unwrap());

        let earned: HashSet<i32> = [1, 3, 5, 6].into_iter().collect();
        put("13_achievement", achievement_canvas("A60", Some("天选"), &earned, &lt).unwrap());

        let frows: Vec<_> = consts::Facility::ALL
            .iter()
            .enumerate()
            .map(|(i, &f)| {
                let lv = (i as i16 * 3).min(f.max_lv());
                let next_cost = if lv >= f.max_lv() {
                    None
                } else {
                    Some(crate::plugins::horse::logic::facility_cost(f.cost_base(), f.cost_ratio(), lv + 1))
                };
                FacilityRow { name: f.name(), effect: f.effect(), lv, max_lv: f.max_lv(), next_cost }
            })
            .collect();
        put("15_facility", facility_canvas("A60", &frows, 2, 3, 24680, &dk).unwrap());
        put("16_bloodline", bloodline_canvas("A60", &horses[..4], 6, &lt).unwrap());

        std::fs::write(format!("{dir}/14_guide.webp"), guide_image(&lt).unwrap()).unwrap();
        eprintln!("画廊已渲染到 {dir}/");
    }

    /// 名次行「名字」与「· 主人 / 标签」各自竖向居中于行中线 cy(分段各自居中,小字号才不下沉)。
    /// 量两段墨迹竖向中点对 cy 的偏差守住回归。
    #[test]
    fn pvp_row_text_vertically_centered() {
        let lt = UserTheme::resolve("light", "teal");
        let scale = lt.opts().scale;
        let img = pvp_result_canvas(&pvp_sample(), &[60, 30, 15], 5, &lt).unwrap().into_rgba().unwrap();
        let (w, h) = (img.width(), img.height());
        let (row_h, rank_top) = (36.0_f32, 80.0 + 142.0 + 14.0);
        let first = rank_top + row_h / 2.0;
        let inked = |x: u32, ya: u32, yb: u32| x < w && (ya..yb).any(|y| img.get_pixel(x, y).0[3] > 60);
        // [xa,xb) 段、[ya,yb) 窗口内墨迹的竖向中点(物理像素)。
        let band_mid = |xa: u32, xb: u32, ya: u32, yb: u32| -> Option<f32> {
            let (mut top, mut bot) = (None, 0u32);
            for y in ya..yb {
                if (xa..xb.min(w)).any(|x| img.get_pixel(x, y).0[3] > 60) {
                    top.get_or_insert(y);
                    bot = y;
                }
            }
            top.map(|t| (t + bot) as f32 / 2.0)
        };
        let x0 = ((PAD + 52.0) * scale) as u32;
        let x_end = ((W - PAD - 90.0) * scale) as u32; // 名字 + 主人 + 标签区,排除右尾「+分」
        let gap_px = (8.0 * scale) as u32; // 名↔主人空档(12 逻辑像素)大于汉字字间隙
        for k in 0..5 {
            let cy = (first + k as f32 * row_h) * scale;
            let (ya, yb) = ((cy - 26.0 * scale) as u32, ((cy + 26.0 * scale) as u32).min(h));
            // 名字段右缘:从 x0 起,见墨后第一处 ≥ gap 的连续空列。
            let (mut seen, mut run, mut name_end) = (false, 0u32, x_end);
            for x in x0..x_end {
                if inked(x, ya, yb) {
                    if seen && run >= gap_px {
                        name_end = x - run;
                        break;
                    }
                    seen = true;
                    run = 0;
                } else if seen {
                    run += 1;
                }
            }
            let name_mid = band_mid(x0, name_end, ya, yb).expect("名字墨迹");
            assert!((name_mid - cy).abs() <= 2.5, "row{k} 名字偏 {:.1}px", name_mid - cy);
            if let Some(owner_mid) = band_mid(name_end, x_end, ya, yb) {
                assert!(
                    (owner_mid - cy).abs() <= 3.0,
                    "row{k} 主人 / 标签偏 {:.1}px(应各自居中,别共基线下沉)",
                    owner_mid - cy
                );
            }
        }
    }

    /// 两行行(名 + 说明)的居中回归:名 + 说明整块、右侧数值都竖向居中于行中线 cy。用背包卡守住。
    #[test]
    fn two_line_row_block_centered() {
        let lt = UserTheme::resolve("light", "teal");
        let scale = lt.opts().scale;
        let items = [(Item::Boost, 12), (Item::Banana, 3), (Item::Feed1, 7)];
        let img = backpack_canvas("A60", &items, &lt).unwrap().into_rgba().unwrap();
        let (w, h) = (img.width(), img.height());
        let row_h = L2_ROW_H;
        let first = 78.0 + 4.0 + row_h / 2.0; // 背包无副标题 → div = 78
        let band_mid = |xa: u32, xb: u32, ya: u32, yb: u32| -> Option<f32> {
            let (mut top, mut bot) = (None, 0u32);
            for y in ya..yb {
                if (xa..xb.min(w)).any(|x| img.get_pixel(x, y).0[3] > 60) {
                    top.get_or_insert(y);
                    bot = y;
                }
            }
            top.map(|t| (t + bot) as f32 / 2.0)
        };
        for k in 0..items.len() {
            let cy = (first + k as f32 * row_h) * scale;
            let (ya, yb) = ((cy - 24.0 * scale) as u32, ((cy + 24.0 * scale) as u32).min(h));
            // 名 + 说明整块(左侧文字区,排除右侧×数量)。
            let block = band_mid(((PAD + 24.0) * scale) as u32, ((W - PAD - 100.0) * scale) as u32, ya, yb)
                .expect("文字块墨迹");
            assert!((block - cy).abs() <= 2.5, "row{k} 两行整块偏 {:.1}px(应整块居中于行中线)", block - cy);
            // 右侧×数量。
            let qty =
                band_mid(((W - PAD - 90.0) * scale) as u32, ((W - PAD) * scale) as u32, ya, yb).expect("数量墨迹");
            assert!((qty - cy).abs() <= 2.5, "row{k} 数量偏 {:.1}px(应居中于行中线、与整块同高)", qty - cy);
        }
    }
}
