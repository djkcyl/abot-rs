//! 排行榜卡片 —— 总览仪表盘与单榜都渲成图(经 [`imaging`](crate::imaging) 底座出 WebP)。
//!
//! 每一行的身份都出**三列**:UID(站内编号)· QQ(前三后三打码)· 昵称(群内榜=群名片、全局榜=
//! 账号昵称,自设昵称优先,解析口径见 [`mod`](super))。
//!
//! - **总览** [`overview_image`]:每个榜各出前几名 + 一行「我的名次」,每榜一块软底圆角面板。
//! - **单榜** [`board_image`]:前 3 名带圆头像做 podium(并排栏,名下缀 UID·QQ),其余名次走横线表
//!   (名次 / UID / QQ / 昵称 / 数值五列);调用者在榜内则高亮其行,不在前列则末尾补「我的名次」。
//!
//! 配色全部来自用户出图主题的标准色卡([`UserTheme`] 的 [`Palette`])。名次徽用奖牌 emoji(前三)
//! 或纯数字。渲染失败由调用方退回文字([`overview_text`] / [`board_text`],信息同卡)。

use nagisa::render::{Align, Doc, Insets, Length, render_document};

use crate::imaging::{Palette, UserTheme};

/// 一行的身份三列:UID(无 user 行的潜水者为 `None`)、打码 QQ、显示名(已按优先级解析)。
pub struct NameCell {
    pub uid: Option<i64>,
    pub qq: String,
    pub name: String,
    /// 显示名的自设颜色(`#rrggbb` 原始色相,空 = 缺省文字色;只在名取自自设昵称时带)。
    pub color: String,
}

/// 总览卡:每个榜的前几名 + 调用者在各榜的名次。
pub struct OverviewCard {
    /// 作用域标签(`"全局"` / `"本群"`)。
    pub scope_label: &'static str,
    pub boards: Vec<OverviewBoard>,
    pub theme: UserTheme,
}

/// 总览里的一个榜:标题 + 前几名 + 调用者名次串。
pub struct OverviewBoard {
    pub title: &'static str,
    pub top: Vec<OverviewEntry>,
    /// (名次, 数值串);没上榜为 `None`。
    pub mine: Option<(u32, String)>,
}

/// 总览里一行。
pub struct OverviewEntry {
    pub rank: u32,
    pub cell: NameCell,
    pub value_text: String,
}

/// 单榜卡:完整 top-N + 调用者名次。
pub struct BoardCard {
    pub title: &'static str,
    pub scope_label: &'static str,
    pub rows: Vec<BoardEntry>,
    pub mine: Option<MyStanding>,
    pub theme: UserTheme,
}

/// 单榜里一行。
pub struct BoardEntry {
    pub rank: u32,
    pub cell: NameCell,
    pub value_text: String,
    /// 头像字节(只前 3 名拉,拉不到为 `None`)。
    pub avatar: Option<Vec<u8>>,
    /// 是不是调用者本人(高亮其行)。
    pub is_me: bool,
}

/// 调用者在单榜里的名次。
pub struct MyStanding {
    pub rank: u32,
    pub cell: NameCell,
    pub value_text: String,
    /// 已在上方名次行里出现(则不另起「我的名次」尾行)。
    pub in_top: bool,
}

/// 名次徽:前三奖牌 emoji,其余纯数字。
fn medal(rank: u32) -> String {
    match rank {
        1 => "🥇".to_string(),
        2 => "🥈".to_string(),
        3 => "🥉".to_string(),
        n => n.to_string(),
    }
}

/// UID 列文字:有 user 行出 `UID123`,潜水者(无行)出 `—`。
fn uid_text(uid: Option<i64>) -> String {
    uid.map(|n| format!("UID{n}")).unwrap_or_else(|| "—".to_string())
}

/// podium 名下的 UID·QQ 子行。
fn sub_line(cell: &NameCell) -> String {
    match cell.uid {
        Some(n) => format!("UID{n} · {}", cell.qq),
        None => cell.qq.clone(),
    }
}

/// 本卡的出图选项:公共底座(按用户主题)+ 加宽卡(容三列身份)+ 边距。
fn render_opts(t: &UserTheme) -> nagisa::render::RenderOptions {
    t.opts().with_width(1040.0).with_padding(Insets::symmetric(30.0, 30.0))
}

/// 横线名次表(名次 / UID / QQ / 昵称 / 数值五列),`me_rows` 标的行整行淡底高亮。
/// `dark` 供自设昵称颜色按本次亮暗收对比。
fn lines_table(d: &mut Doc, em: f32, pal: &Palette, dark: bool, rows: &[(String, &NameCell, &str, bool)]) {
    let idx_col = em * 2.4;
    let uid_col = em * 4.6;
    let qq_col = em * 5.6;
    let value_col = em * 9.2;
    d.table(|t| {
        t.grid_vertical(false);
        t.grid_outer(false);
        t.expand();
        t.pad_y(7.0);
        t.width(0, Length::Px(idx_col));
        t.width(1, Length::Px(uid_col));
        t.width(2, Length::Px(qq_col));
        t.width(4, Length::Px(value_col));
        t.align([Align::Center, Align::Left, Align::Left, Align::Left, Align::Right]);
        for (badge, cell, value, _is_me) in rows {
            t.row_rich(|row| {
                row.cell(|c| {
                    c.styled(badge.as_str(), |s| {
                        s.color(&pal.muted).size(0.95);
                    });
                });
                row.cell(|c| {
                    c.styled(uid_text(cell.uid), |s| {
                        s.color(&pal.muted).size(0.85);
                    });
                });
                row.cell(|c| {
                    c.styled(cell.qq.as_str(), |s| {
                        s.color(&pal.muted).size(0.85);
                    });
                });
                let name_col = crate::imaging::readable_hex(&cell.color, dark);
                row.cell(|c| {
                    c.styled(cell.name.as_str(), |s| {
                        s.weight(500);
                        if let Some(col) = &name_col {
                            s.color(col);
                        }
                    });
                });
                row.cell(|c| {
                    c.styled(*value, |s| {
                        s.size(0.95).weight(500);
                    });
                });
            });
        }
        for (i, (_, _, _, is_me)) in rows.iter().enumerate() {
            if *is_me {
                t.row_fill(i, &pal.soft);
            }
        }
    });
}

/// 把总览渲成卡片图(WebP 字节)。
pub fn overview_image(card: &OverviewCard) -> anyhow::Result<Vec<u8>> {
    let opts = render_opts(&card.theme);
    let pal = &card.theme.palette;
    let em = opts.theme.base_size;
    let mut d = Doc::new();

    d.paragraph(|p| {
        p.align(Align::Center)
            .styled("排行榜", |s| {
                s.weight(700).size(1.5);
            })
            .styled(format!("  · {}", card.scope_label), |s| {
                s.weight(600).size(0.95).color(&pal.primary);
            });
    });
    d.paragraph(|p| {
        p.align(Align::Center).styled(
            "发送「查看游戏币榜」「查看发言榜」等看完整榜，群里加「全局」看全站",
            |s| {
                s.color(&pal.muted).size(0.8);
            },
        );
    });

    for b in &card.boards {
        d.panel(|pb| {
            pb.bg(&pal.soft).rounded(14.0).pad(18.0);
            pb.paragraph(|p| {
                p.styled(b.title, |s| {
                    s.weight(700).color(&pal.primary);
                });
            });
            if b.top.is_empty() {
                pb.paragraph(|p| {
                    p.styled("暂无数据", |s| {
                        s.color(&pal.muted).size(0.9);
                    });
                });
            } else {
                let rows: Vec<(String, &NameCell, &str, bool)> =
                    b.top.iter().map(|e| (medal(e.rank), &e.cell, e.value_text.as_str(), false)).collect();
                lines_table(pb, em, pal, card.theme.dark, &rows);
            }
            if let Some((rank, value)) = &b.mine {
                pb.paragraph(|p| {
                    p.styled(format!("我 · 第 {rank} 名 · {value}"), |s| {
                        s.color(&pal.muted).size(0.8);
                    });
                });
            }
        });
    }

    Ok(render_document(&d.build(), &opts)?)
}

/// 把单榜渲成卡片图(WebP 字节)。
pub fn board_image(card: &BoardCard) -> anyhow::Result<Vec<u8>> {
    let opts = render_opts(&card.theme);
    let pal = &card.theme.palette;
    let em = opts.theme.base_size;
    let mut d = Doc::new();

    d.paragraph(|p| {
        p.align(Align::Center)
            .styled(card.title, |s| {
                s.weight(700).size(1.5);
            })
            .styled(format!("  · {}", card.scope_label), |s| {
                s.weight(600).size(0.95).color(&pal.primary);
            });
    });

    if card.rows.is_empty() {
        d.paragraph(|p| {
            p.align(Align::Center).styled("暂无数据", |s| {
                s.color(&pal.muted).size(0.95);
            });
        });
        return Ok(render_document(&d.build(), &opts)?);
    }

    // —— 前 3 名 podium:并排栏,圆头像 + 奖牌 + 昵称 + UID·QQ 子行 + 数值。——
    let podium: Vec<&BoardEntry> = card.rows.iter().take(3).collect();
    d.columns(|cb| {
        cb.gap(14.0);
        for e in &podium {
            cb.col(|c| {
                if let Some(av) = &e.avatar {
                    c.image_bytes(av.clone(), |i| {
                        i.width_px(84.0).align(Align::Center).rounded(42.0);
                    });
                }
                c.paragraph(|p| {
                    p.align(Align::Center).styled(medal(e.rank), |s| {
                        s.size(1.3);
                    });
                });
                let name_col = crate::imaging::readable_hex(&e.cell.color, card.theme.dark);
                c.paragraph(|p| {
                    p.align(Align::Center).styled(e.cell.name.as_str(), |s| {
                        s.weight(600).size(1.0);
                        // 自设颜色优先于「是我」的主色高亮——颜色是用户的身份选择。
                        if let Some(col) = &name_col {
                            s.color(col);
                        } else if e.is_me {
                            s.color(&pal.primary);
                        }
                    });
                });
                c.paragraph(|p| {
                    p.align(Align::Center).styled(sub_line(&e.cell), |s| {
                        s.color(&pal.muted).size(0.75);
                    });
                });
                c.paragraph(|p| {
                    p.align(Align::Center).styled(e.value_text.as_str(), |s| {
                        s.weight(600).size(0.9);
                    });
                });
            });
        }
    });

    // —— 第 4 名起:横线五列表,调用者所在行高亮。——
    let rest: Vec<(String, &NameCell, &str, bool)> =
        card.rows.iter().skip(3).map(|e| (e.rank.to_string(), &e.cell, e.value_text.as_str(), e.is_me)).collect();
    if !rest.is_empty() {
        lines_table(&mut d, em, pal, card.theme.dark, &rest);
    }

    // —— 调用者不在前列:末尾补一行「我的名次」。——
    if let Some(me) = &card.mine
        && !me.in_top
    {
        d.divider();
        d.paragraph(|p| {
            p.align(Align::Center)
                .styled(format!("我 · 第 {} 名", me.rank), |s| {
                    s.weight(600).color(&pal.primary);
                })
                .styled(format!(" · {} · {}", sub_line(&me.cell), me.value_text), |s| {
                    s.color(&pal.muted).size(0.85);
                });
        });
    }

    Ok(render_document(&d.build(), &opts)?)
}

/// 总览的文字退路(渲染失败时用,信息同卡片)。
pub fn overview_text(card: &OverviewCard) -> String {
    let mut lines = vec![format!("排行榜 · {}", card.scope_label)];
    for b in &card.boards {
        lines.push(format!("【{}】", b.title));
        if b.top.is_empty() {
            lines.push("  暂无数据".to_string());
        } else {
            for e in &b.top {
                lines.push(format!(
                    "  {}. {}（{}/{}）—— {}",
                    e.rank,
                    e.cell.name,
                    uid_text(e.cell.uid),
                    e.cell.qq,
                    e.value_text
                ));
            }
        }
        if let Some((rank, value)) = &b.mine {
            lines.push(format!("  我:第 {rank} 名 · {value}"));
        }
    }
    lines.join("\n")
}

/// 单榜的文字退路(渲染失败时用,信息同卡片)。
pub fn board_text(card: &BoardCard) -> String {
    let mut lines = vec![format!("{} · {}", card.title, card.scope_label)];
    if card.rows.is_empty() {
        lines.push("暂无数据".to_string());
        return lines.join("\n");
    }
    for e in &card.rows {
        let me = if e.is_me { " ←" } else { "" };
        lines.push(format!(
            "{}. {}（{}/{}）—— {}{me}",
            e.rank,
            e.cell.name,
            uid_text(e.cell.uid),
            e.cell.qq,
            e.value_text
        ));
    }
    if let Some(me) = &card.mine
        && !me.in_top
    {
        lines.push(format!("我:第 {} 名 · {}", me.rank, me.value_text));
    }
    lines.join("\n")
}
