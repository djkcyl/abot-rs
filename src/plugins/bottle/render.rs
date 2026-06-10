//! 捞瓶 / 查瓶的合并转发呈现 —— **全图渲染**(render 排版引擎,统一 WebP):
//! 首节点 = 瓶子卡片图(编号 / 评分·被捞·剩余 / 时间·来源 / 正文 / 操作提示)+ 瓶子原图;
//! 评论按页渲图(每页 [`COMMENTS_PER_IMAGE`] 楼,一页一节点),楼号跨页连续。
//!
//! 只读:评分均值走 [`logic::score_avg`]、评论走 [`logic::get_discuss`];原图按 md5 从本地
//! 归档读字节重发(base64,QQ 的图片 URL 会过期),读不出放渐变占位图。匿名瓶子署名 bot、
//! 隐来源群;非匿名署名投放者。任一渲染失败退回对应的文字形态,瓶子不至于看不了。

use nagisa::prelude::*;
use sea_orm::DatabaseConnection;

use super::entity::{bottle, discuss};
use super::logic;

/// 单页评论图的高度上限(**物理像素**,scale 2 下即逻辑 2000):装箱分页的界,不是硬截断
/// ——每页至少一楼,单楼超高就独占一页。
const COMMENTS_PAGE_MAX_PX: u32 = 4000;

/// 量高分页失败时的退路:固定每页楼数。
const COMMENTS_PER_IMAGE_FALLBACK: usize = 10;

/// 出图公共选项:960 逻辑宽(随引擎默认档)、WebP、abot 字体栈。
fn render_opts() -> nagisa::render::RenderOptions {
    use nagisa::render::{Insets, OutputFormat, RenderOptions};
    RenderOptions::default()
        .with_width(960.0)
        .with_padding(Insets::symmetric(36.0, 40.0))
        .with_fonts(crate::fonts::handle())
        .with_format(OutputFormat::Webp)
}

/// 把一只瓶子渲染成合并转发。
///
/// 节点构成:瓶子卡片图 + 原图(首节点)、评论图按页各一节点。`self_id` 用作匿名瓶子
/// 与评论节点的署名。
pub async fn bottle_forward(
    db: &DatabaseConnection,
    b: &bottle::Model,
    self_id: Uin,
) -> anyhow::Result<Segment> {
    let score = logic::score_avg(db, b.id).await?;
    let comments = logic::get_discuss(db, b.id).await?;

    // 匿名 → 署名 bot、名「匿名漂流瓶」;非匿名 → 署名投放者 uin + 显示名(缺则 QQ 号)。
    let (sender, sender_name) = if b.anonymous {
        (self_id, "匿名漂流瓶".to_string())
    } else {
        let name = b.nickname.clone().filter(|s| !s.trim().is_empty()).unwrap_or_else(|| b.uin.to_string());
        (Uin(b.uin), name)
    };

    // —— 首节点:卡片图(失败退文字)+ 瓶子原图(读不出放占位图)。——
    let mut content = Vec::new();
    match card_image(b, score) {
        Ok(webp) => content.push(Segment::image_bytes(webp)),
        Err(e) => {
            tracing::warn!(error = %e, "渲染瓶子卡片失败,退回文字");
            content.push(Segment::text(card_text(b, score)));
        }
    }
    for md5 in image_names(&b.images) {
        // 读字节发 base64:不依赖协议端可读 bot 的盘,也没有无后缀路径的兼容问题。
        // 读不出(被清理/盘损)不静默吞图:放渐变占位图,让捞的人知道这里本来有张图。
        match tokio::fs::read(crate::media::resolve(&md5)).await {
            Ok(bytes) => {
                content.push(Segment::image_bytes(bytes));
                tokio::spawn(crate::media::touch_used(md5)); // 重发即「使用」,刷 last_used
            }
            Err(e) => {
                tracing::warn!(%md5, error = %e, "读漂流瓶图片失败,放占位图");
                match crate::media::placeholder::missing_image_webp(&md5) {
                    Ok(webp) => content.push(Segment::image_bytes(webp)),
                    // 占位图都渲染不出(理论不至)才退回文字。
                    Err(pe) => {
                        tracing::warn!(error = %pe, "渲染占位图失败,退回文字");
                        content.push(Segment::text("〔这里有张图片,但已经失效看不了了〕"));
                    }
                }
            }
        }
    }
    let mut nodes = vec![ForwardNode::new(sender, sender_name, content)];

    // —— 评论:按渲染高度装箱分页(每页楼数动态),每页一图一节点,楼号跨页连续;
    //    量高失败退固定楼数分页,某页渲染失败该页退文字楼层。——
    let total = comments.len();
    let spans = paginate_comments(&comments).unwrap_or_else(|e| {
        tracing::warn!(error = %e, "评论量高分页失败,退回固定每页楼数");
        let mut spans = Vec::new();
        let mut s = 0;
        while s < total {
            let e = (s + COMMENTS_PER_IMAGE_FALLBACK).min(total);
            spans.push((s, e));
            s = e;
        }
        spans
    });
    let pages = spans.len();
    for (pi, &(s, e)) in spans.iter().enumerate() {
        let chunk = &comments[s..e];
        let node_name =
            if pages > 1 { format!("评论 {}/{pages}", pi + 1) } else { "评论".to_string() };
        match comments_image(chunk, s, total, pi + 1, pages) {
            Ok(webp) => {
                nodes.push(ForwardNode::new(self_id, node_name, vec![Segment::image_bytes(webp)]));
            }
            Err(err) => {
                tracing::warn!(error = %err, page = pi + 1, "渲染评论图失败,该页退回文字楼层");
                let mut lines = Vec::with_capacity(chunk.len());
                for (j, c) in chunk.iter().enumerate() {
                    lines.push(format!("{}楼 {}:{}", s + j + 1, commenter(c), c.text));
                }
                nodes.push(ForwardNode::text(self_id, node_name, lines.join("\n")));
            }
        }
    }

    Ok(Segment::forward(nodes))
}

/// 瓶子卡片图:编号(+匿名标)/ 评分·被捞·剩余 / 时间·来源 / 正文 / 操作提示。
pub fn card_image(b: &bottle::Model, score: Option<f64>) -> anyhow::Result<Vec<u8>> {
    use nagisa::render::{render_document, Doc};

    let mut d = Doc::new();
    d.heading(2, |h| {
        h.text(format!("漂流瓶 #{}", b.id));
        if b.anonymous {
            h.styled("  匿名", |s| {
                s.color("#8a8f98").size(0.55);
            });
        }
    });

    // 数据行:评分 · 被捞 · 剩余可捞。
    d.paragraph(|p| {
        p.styled(
            format!("评分 {} · 被捞 {} 次 · 剩余可捞 {}", score_text(score), b.total_pickups, remaining_text(b)),
            |s| {
                s.color("#6b7280").size(0.92);
            },
        );
    });
    // 时间 / 来源行(匿名隐来源,连「来自群」也不露)。
    let mut meta = format!("丢出于 {}", b.created_at.format("%Y-%m-%d %H:%M:%S"));
    if !b.anonymous
        && let Some(gid) = b.group_id
    {
        meta.push_str(&format!(" · 来自群 {gid}"));
    }
    d.paragraph(|p| {
        p.styled(meta, |s| {
            s.color("#6b7280").size(0.92);
        });
    });

    // 正文(若有):逐行成段,空行跳过(段距本身就是分隔)。
    if let Some(text) = b.text.as_deref().filter(|t| !t.trim().is_empty()) {
        d.divider();
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            d.paragraph(|p| {
                p.text(line);
            });
        }
    }

    // 操作提示脚注。
    d.divider();
    d.paragraph(|p| {
        p.styled(
            format!("评分:发送「漂流瓶评分 {0} 分数」    评论:发送「漂流瓶评论 {0} 内容」", b.id),
            |s| {
                s.color("#9aa0a8").size(0.8);
            },
        );
    });

    Ok(render_document(&d.build(), &render_opts())?)
}

/// 卡片的文字退路(渲染失败时用,信息同卡片)。
fn card_text(b: &bottle::Model, score: Option<f64>) -> String {
    let mut out = format!(
        "漂流瓶 #{}{}\n评分 {} · 被捞 {} 次 · 剩余可捞 {}\n丢出于 {}",
        b.id,
        if b.anonymous { "(匿名)" } else { "" },
        score_text(score),
        b.total_pickups,
        remaining_text(b),
        b.created_at.format("%Y-%m-%d %H:%M:%S"),
    );
    if !b.anonymous
        && let Some(gid) = b.group_id
    {
        out.push_str(&format!(" · 来自群 {gid}"));
    }
    if let Some(text) = b.text.as_deref().filter(|t| !t.trim().is_empty()) {
        out.push('\n');
        out.push_str(text);
    }
    out.push_str(&format!("\n发送「漂流瓶评分 {0} 分数」评分,「漂流瓶评论 {0} 内容」评论", b.id));
    out
}

/// 构建一页评论的文档(量高与真渲共用):标题「评论 N 条(·第 i/k 页)」+ 每楼
/// 「楼号 名字 · 时间」+ 内容,楼层间分割线。`offset` 为本页首楼在全部评论里的下标
/// (楼号跨页连续)。
fn comments_doc(
    chunk: &[discuss::Model],
    offset: usize,
    total: usize,
    page: usize,
    pages: usize,
) -> nagisa::render::Document {
    use nagisa::render::Doc;

    let mut d = Doc::new();
    d.heading(4, |h| {
        h.text(format!("评论 {total} 条"));
        if pages > 1 {
            h.styled(format!("  第 {page}/{pages} 页"), |s| {
                s.color("#8a8f98").size(0.7);
            });
        }
    });
    for (j, c) in chunk.iter().enumerate() {
        if j > 0 {
            d.divider();
        }
        let when = c.created_at.format("%m-%d %H:%M").to_string();
        d.paragraph(|p| {
            p.styled(format!("{} 楼", offset + j + 1), |s| {
                s.bold().size(0.85);
            });
            p.styled(format!("  {} · {when}", commenter(c)), |s| {
                s.color("#8a8f98").size(0.85);
            });
        });
        d.paragraph(|p| {
            p.text(c.text.as_str());
        });
    }
    d.build()
}

/// 一页评论渲成图(WebP)。
pub fn comments_image(
    chunk: &[discuss::Model],
    offset: usize,
    total: usize,
    page: usize,
    pages: usize,
) -> anyhow::Result<Vec<u8>> {
    use nagisa::render::render_document;
    Ok(render_document(&comments_doc(chunk, offset, total, page, pages), &render_opts())?)
}

/// 评论按渲染高度装箱分页:逐楼试加、量高([`nagisa::render::measure_document`],只排版
/// 不绘制),超过 [`COMMENTS_PAGE_MAX_PX`] 就在上一楼收页。每页至少一楼(单楼超高独占
/// 一页)。返回各页在 `comments` 里的 `(起, 止)` 下标(止开区间)。
pub fn paginate_comments(comments: &[discuss::Model]) -> anyhow::Result<Vec<(usize, usize)>> {
    use nagisa::render::measure_document;

    let opts = render_opts();
    let n = comments.len();
    let total = n;
    let mut spans = Vec::new();
    let mut start = 0;
    while start < n {
        // 至少装一楼;之后每多装一楼量一次高,超限即收页。
        // (页标会让标题行多一小段,量高时按多页形态算,高度不受页码数字影响。)
        let mut cut = start + 1;
        while cut < n {
            let doc = comments_doc(&comments[start..=cut], start, total, 1, 2);
            let (_, h) = measure_document(&doc, &opts)?;
            if h > COMMENTS_PAGE_MAX_PX {
                break;
            }
            cut += 1;
        }
        spans.push((start, cut));
        start = cut;
    }
    Ok(spans)
}

/// 「查漂流瓶」列表渲成表格图:编号 / 丢出时间 / 被捞(剩) / 评分 / 状态 / 匿名。
/// `scores` 为各瓶去极值均值(缺即无评分)。
pub fn list_image(
    rows: &[bottle::Model],
    scores: &std::collections::HashMap<i64, f64>,
) -> anyhow::Result<Vec<u8>> {
    use nagisa::render::{render_document, Align, Doc};

    let mut d = Doc::new();
    d.heading(3, |h| {
        h.text(format!("你的漂流瓶(近 {} 个)", rows.len()));
    });
    d.table(|t| {
        t.head(["编号", "丢出时间", "被捞(剩)", "评分", "状态", "匿名"]);
        t.align([Align::Left, Align::Left, Align::Center, Align::Center, Align::Center, Align::Center]);
        t.expand(); // 铺满内容宽,列距舒展
        for b in rows {
            let score =
                scores.get(&b.id).map(|s| format!("{s}分")).unwrap_or_else(|| "—".to_string());
            t.row([
                format!("#{}", b.id),
                b.created_at.format("%m-%d %H:%M").to_string(),
                format!("{}({})", b.total_pickups, remaining_text(b)),
                score,
                status_text(&b.status).to_string(),
                if b.anonymous { "是" } else { "" }.to_string(),
            ]);
        }
    });

    Ok(render_document(&d.build(), &render_opts())?)
}

/// 审核状态的中文呈现。
pub fn status_text(status: &str) -> &'static str {
    match status {
        "pending" => "待审核",
        "approved" | "ai_approved" => "已通过",
        "rejected" => "未通过",
        _ => "未知",
    }
}

/// 评论者显示名:昵称非空用昵称,否则 QQ 号。
fn commenter(c: &discuss::Model) -> String {
    c.nickname.clone().filter(|s| !s.trim().is_empty()).unwrap_or_else(|| c.uin.to_string())
}

/// 评分的呈现文本。
fn score_text(score: Option<f64>) -> String {
    score.map(|s| format!("{s} 分")).unwrap_or_else(|| "暂无".to_string())
}

/// 剩余可捞次数的呈现文本(-1 = 不限)。
fn remaining_text(b: &bottle::Model) -> String {
    if b.remaining_pickups < 0 { "不限".to_string() } else { b.remaining_pickups.to_string() }
}

/// 解析瓶子 `images`(JSONB 字符串数组)成内容 md5 序列;非数组 / 非字符串项跳过。
fn image_names(images: &serde_json::Value) -> Vec<String> {
    images
        .as_array()
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
        .unwrap_or_default()
}
