//! 帮助卡片 —— `help` 的总览菜单与功能详情都渲成图(经 [`imaging`](crate::imaging) 底座出
//! WebP)。菜单卡:标题与查详情提示居中,往下每个分类一块软底圆角面板——题头「分类 ·
//! N 个功能」,板内横线四列表(序号 / 功能名 / 简介 / 启停)。详情卡:功能名与简介各占
//! 一行,往下每条命令一块描边圆角面板:题行(序号·主词·别名·启停)、简介、带题头栏的
//! 用法代码块、「参数」小标 + 无线两列参数表、备注灰斜体。启停一律彩色文字(启用主色 /
//! 停用灰),配色全部来自用户出图主题的标准色卡。渲染失败由调用方退回文字。

use nagisa::render::{Align, Doc, Insets, Length, render_document};

use crate::imaging::{Palette, UserTheme};

/// 菜单里的一个功能行(排版素材,编号 / 分组 / 停用判定在 `mod.rs` 算好)。
pub struct MenuRow {
    /// 全菜单统一编号(`help 序号` 按它解析)。
    pub idx: usize,
    /// 插件名(菜单上的功能名)。
    pub name: String,
    /// 给用户看的简介(可空,列内自动换行)。
    pub desc: String,
    /// 在当前会话被整体停用。
    pub off: bool,
}

/// 详情卡里的一条命令(排版素材,文字退路同源)。
pub struct CmdInfo {
    /// 在本功能内的编号(按命令 order 排)。
    pub idx: usize,
    /// 主命令词。
    pub primary: String,
    /// 其余别名。
    pub aliases: Vec<String>,
    /// 在当前会话被停用。
    pub off: bool,
    /// 命令简介(可空)。
    pub desc: String,
    /// 自动生成的用法 synopsis(`主词 [-a] <编号> [内容]`)。
    pub synopsis: String,
    /// 逐参数说明(前缀, 说明;说明可空)。
    pub params: Vec<(String, String)>,
    /// 备注(命令级 `usage`,参数表达不了的行为;可空)。
    pub note: String,
}

/// 详情卡的全部素材(一个功能 + 它的全部命令)。
pub struct DetailCard {
    /// 功能名(插件名)。
    pub name: String,
    /// 功能简介(可空)。
    pub desc: String,
    /// 功能在当前会话被整体停用。
    pub off: bool,
    pub cmds: Vec<CmdInfo>,
}

/// 两张卡共用的出图选项:公共底座(按用户主题)+ 卡片宽度与边距(行式多列内容,比缺省卡宽)。
fn render_opts(t: &UserTheme) -> nagisa::render::RenderOptions {
    t.opts().with_width(960.0).with_padding(Insets::symmetric(30.0, 32.0))
}

/// 启停文字的用词与配色:启用主色、停用灰。
fn status_word(off: bool) -> &'static str {
    if off { "停用" } else { "启用" }
}
fn status_color(pal: &Palette, off: bool) -> &str {
    if off { &pal.muted } else { &pal.primary }
}

/// 把分组好的菜单渲成卡片图(WebP 字节)。`groups` 已按展示顺序排好(分类名, 功能行)。
pub fn menu_image(groups: &[(&str, Vec<MenuRow>)], theme: &UserTheme) -> anyhow::Result<Vec<u8>> {
    let opts = render_opts(theme);
    let pal = &theme.palette;

    // 除简介外三列统一定宽(简介列吃 expand 的剩余宽度):各分类表各自算列宽,不定宽会被
    // 长简介把名字 / 启停挤到换行,面板间列宽也不齐。名字列按全菜单最长名字估宽(CJK 一字
    // 一 em、ASCII 半字),各列都加引擎表格格内边距(em 的 0.32 × 两侧)。
    let em = opts.theme.base_size;
    let cell_pad = em * 0.32 * 2.0 + 4.0;
    let name_w = |s: &str| s.chars().map(|c| if c.is_ascii() { 0.55 } else { 1.0 }).sum::<f32>() * em;
    let idx_col = em * 0.55 * 2.0 + cell_pad;
    let name_col = groups.iter().flat_map(|(_, rows)| rows).map(|r| name_w(&r.name)).fold(0.0, f32::max) + cell_pad;
    let status_col = em * 2.0 + cell_pad;

    let mut d = Doc::new();

    // —— 标题 + 查详情提示(提示放头部,看完名字自然知道下一步)。——
    d.paragraph(|p| {
        p.align(Align::Center).styled("命令菜单", |s| {
            s.weight(700).size(1.5);
        });
    });
    d.paragraph(|p| {
        p.align(Align::Center).styled("发送「help 序号或功能名」看详细命令，比如「help 1」", |s| {
            s.color(&pal.muted).size(0.8);
        });
    });

    // —— 每个分类一块软底圆角面板:题头「分类 · N 个功能」+ 横线四列表。——
    for (label, rows) in groups {
        d.panel(|pb| {
            pb.bg(&pal.soft).rounded(14.0).pad(18.0);
            pb.paragraph(|p| {
                p.styled(*label, |s| {
                    s.weight(700).color(&pal.primary);
                })
                .styled(format!("  ·  {} 个功能", rows.len()), |s| {
                    s.color(&pal.muted).size(0.8);
                });
            });
            pb.table(|t| {
                // 只留行间横线分行,竖线 / 外框交给面板。
                t.grid_vertical(false);
                t.grid_outer(false);
                t.expand();
                t.pad_y(7.0);
                t.width(0, Length::Px(idx_col));
                t.width(1, Length::Px(name_col));
                t.width(3, Length::Px(status_col));
                t.align([Align::Right, Align::Left, Align::Left, Align::Center]);
                for r in rows {
                    t.row_rich(|row| {
                        row.cell(|c| {
                            c.styled(r.idx.to_string(), |s| {
                                s.color(&pal.muted).size(0.9);
                            });
                        });
                        row.cell(|c| {
                            c.styled(r.name.as_str(), |s| {
                                s.weight(600);
                                if r.off {
                                    s.color(&pal.muted);
                                }
                            });
                        });
                        row.cell(|c| {
                            c.styled(r.desc.as_str(), |s| {
                                s.size(0.95);
                                if r.off {
                                    s.color(&pal.muted);
                                }
                            });
                        });
                        row.cell(|c| {
                            c.styled(status_word(r.off), |s| {
                                s.size(0.85).weight(500).color(status_color(pal, r.off));
                            });
                        });
                    });
                }
            });
        });
    }

    Ok(render_document(&d.build(), &opts)?)
}

/// 把一个功能的详情渲成卡片图(WebP 字节)。
pub fn detail_image(card: &DetailCard, theme: &UserTheme) -> anyhow::Result<Vec<u8>> {
    let opts = render_opts(theme);
    let pal = &theme.palette;
    let mut d = Doc::new();

    // —— 头部:功能名(整体停用时挂灰字标注)与简介各占一行。——
    d.paragraph(|p| {
        p.styled(card.name.as_str(), |s| {
            s.weight(700).size(1.4);
        });
        if card.off {
            p.styled("  · 停用", |s| {
                s.size(0.85).color(&pal.muted);
            });
        }
    });
    if !card.desc.is_empty() {
        d.paragraph(|p| {
            p.styled(card.desc.as_str(), |s| {
                s.color(&pal.muted).size(0.9);
            });
        });
    }

    // —— 每条命令一块描边圆角面板:题行、简介、用法代码块、参数、备注。——
    for cmd in &card.cmds {
        d.panel(|pb| {
            pb.border(1.0, &pal.track).rounded(12.0).pad(16.0);
            pb.paragraph(|p| {
                p.styled(format!("{}. ", cmd.idx), |s| {
                    s.weight(700).color(&pal.primary);
                })
                .styled(cmd.primary.as_str(), |s| {
                    s.weight(700).size(1.05);
                    if cmd.off {
                        s.color(&pal.muted);
                    }
                });
                if !cmd.aliases.is_empty() {
                    p.styled(format!("（别名：{}）", cmd.aliases.join("、")), |s| {
                        s.color(&pal.muted).size(0.85);
                    });
                }
                p.styled(format!("  · {}", status_word(cmd.off)), |s| {
                    s.size(0.85).color(status_color(pal, cmd.off));
                });
            });
            if !cmd.desc.is_empty() {
                pb.paragraph(|p| {
                    p.styled(cmd.desc.as_str(), |s| {
                        s.size(0.95);
                    });
                });
            }
            pb.code("用法", cmd.synopsis.as_str());
            if !cmd.params.is_empty() {
                pb.paragraph(|p| {
                    p.styled("参数", |s| {
                        s.color(&pal.muted).size(0.85).weight(500);
                    });
                });
                pb.table(|t| {
                    t.no_grid();
                    t.expand();
                    t.pad_y(4.0);
                    for (head, desc) in &cmd.params {
                        t.row([head.as_str(), desc.as_str()]);
                    }
                    t.col_style(0, |s| {
                        s.size(0.9).weight(500);
                    });
                    t.col_style(1, |s| {
                        s.size(0.9);
                    });
                });
            }
            if !cmd.note.is_empty() {
                pb.paragraph(|p| {
                    p.styled(cmd.note.as_str(), |s| {
                        s.color(&pal.muted).size(0.85).italic();
                    });
                });
            }
        });
    }

    Ok(render_document(&d.build(), &opts)?)
}
