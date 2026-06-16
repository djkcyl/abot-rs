//! 捞瓶 / 查瓶的合并转发呈现:
//! 卡片图一节点(编号 / 评分·被捞·剩余 / 时间·来源 / 投放者 / 正文,整卡一张图),瓶内原图各占一节点
//! (原样字节,动图照常会动);评论按页渲图(一页一节点,楼号跨页连续)。投放者与评论者名
//! 走自设昵称(带色,经 [`readable_hex`](crate::imaging::readable_hex) 收对比)、退快照名 / QQ 号;
//! 匿名瓶署名 bot、隐来源。「取原文」另走 [`original_forward`](原始文字 + 原图字节,不过排版引擎)。
//!
//! 只读:评分均值走 [`logic::score_avg`]、评论走 [`logic::get_discuss`];原图按 md5 从本地归档读字节
//! 重发(QQ 图床 URL 会过期),读不出放渐变占位图。任一渲染失败退回对应文字形态。

use std::collections::HashMap;

use nagisa::prelude::*;
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QuerySelect};

use crate::data::entity::user;
use crate::imaging::UserTheme;

use super::entity::{bottle, discuss};
use super::logic;

/// 单页评论图的高度上限(**物理像素**,scale 1.5 下即逻辑约 2667):装箱分页的界,不是硬截断
/// ——每页至少一楼,单楼超高就独占一页。
const COMMENTS_PAGE_MAX_PX: u32 = 4000;

/// 量高分页失败时的退路:固定每页楼数。
const COMMENTS_PER_IMAGE_FALLBACK: usize = 10;

/// 出图公共选项:用户主题底座(色卡 / 亮暗 / 底栏色带)+ 本插件的边距口径。
fn render_opts(t: &UserTheme) -> nagisa::render::RenderOptions {
    use nagisa::render::Insets;
    t.opts().with_padding(Insets::symmetric(36.0, 40.0))
}

/// 把一只瓶子渲染成合并转发。
///
/// 节点构成:卡片图一节点、瓶内原图各占一节点、评论图按页各一节点。`self_id` 用作匿名瓶子
/// 与评论节点的署名。
pub async fn bottle_forward(
    db: &DatabaseConnection,
    b: &bottle::Model,
    self_id: Uin,
    t: &UserTheme,
) -> anyhow::Result<Segment> {
    let score = logic::score_avg(db, b.id).await?;
    let comments = logic::get_discuss(db, b.id).await?;

    // 投放者 + 评论者的自设昵称(实时查,出图带色用;匿名瓶不显投放者)。
    let mut uins: Vec<i64> = comments.iter().map(|c| c.uin).collect();
    uins.push(b.uin);
    uins.sort_unstable();
    uins.dedup();
    let aliases = fetch_aliases(db, &uins).await;

    // 投放者:匿名署名 bot;否则取自设昵称(带色)/ 快照名,作节点署名与卡内「投放者」行。
    let (node_sender, node_name, poster) = if b.anonymous {
        (self_id, "匿名漂流瓶".to_string(), None)
    } else {
        let (name, color) = name_with_color(aliases.get(&b.uin), b.nickname.as_deref(), b.uin);
        (Uin(b.uin), name.clone(), Some((name, color)))
    };

    // —— 卡片图各占一节点(信息 + 正文,不含瓶内图片;卡渲不出退文字卡片);瓶内每张原图再各占一节点
    //    (各自一条消息,动图自然会动)。——
    let images = load_bottle_images(&b.images).await;
    let mut nodes = Vec::new();
    match card_image(b, score, poster.as_ref().map(|(n, c)| (n.as_str(), c.as_str())), t) {
        Ok(webp) => nodes.push(ForwardNode::new(node_sender, node_name.clone(), vec![Segment::image_bytes(webp)])),
        Err(e) => {
            tracing::warn!(error = %e, "渲染瓶子卡片失败,退回文字卡片");
            nodes.push(ForwardNode::text(node_sender, node_name.clone(), card_text(b, score)));
        }
    }
    for img in &images {
        let seg = match &img.bytes {
            Some(bytes) => Segment::image_bytes(bytes.clone()),
            None => Segment::text(MISSING_IMAGE_TEXT),
        };
        nodes.push(ForwardNode::new(node_sender, node_name.clone(), vec![seg]));
    }

    // —— 评论:按渲染高度装箱分页(每页楼数动态),每页一图一节点,楼号跨页连续;
    //    量高失败退固定楼数分页,某页渲染失败该页退文字楼层。——
    let total = comments.len();
    let spans = paginate_comments(&comments, t, &aliases).unwrap_or_else(|e| {
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
        let node_name = if pages > 1 { format!("评论 {}/{pages}", pi + 1) } else { "评论".to_string() };
        match comments_image(chunk, s, total, pi + 1, pages, t, &aliases) {
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

/// 图片失效时的文字退路。
const MISSING_IMAGE_TEXT: &str = "〔这里有张图片,但已经失效看不了了〕";

/// 时间的本地时区呈现:库存 timestamptz 取出带 UTC 偏移,直接 `format` 会显示成 UTC 钟点。
pub(super) fn local_time(t: &sea_orm::prelude::DateTimeWithTimeZone) -> chrono::DateTime<chrono::Local> {
    t.with_timezone(&chrono::Local)
}

/// 批量查一组 uin 的自设昵称 + 颜色(`alias` / `alias_color`)。查不到的 uin 不入表(由调用方退快照名)。
async fn fetch_aliases(db: &DatabaseConnection, uins: &[i64]) -> HashMap<i64, (String, String)> {
    let mut map = HashMap::new();
    if uins.is_empty() {
        return map;
    }
    match user::Entity::find()
        .select_only()
        .column(user::Column::Uin)
        .column(user::Column::Alias)
        .column(user::Column::AliasColor)
        .filter(user::Column::Uin.is_in(uins.iter().copied()))
        .into_tuple::<(i64, String, String)>()
        .all(db)
        .await
    {
        Ok(rows) => {
            for (u, alias, color) in rows {
                map.insert(u, (alias, color));
            }
        }
        Err(e) => tracing::warn!(error = %e, "查投放者/评论者自设昵称失败"),
    }
    map
}

/// 出图显示名 + 颜色:有自设昵称用它(带其颜色),否则用投放/评论时的快照名(无色),再否则 QQ 号。
/// 颜色为空串即出图用缺省文字色([`readable_hex`](crate::imaging::readable_hex) 对空串返 `None`)。
fn name_with_color(alias: Option<&(String, String)>, snapshot: Option<&str>, uin: i64) -> (String, String) {
    if let Some((a, c)) = alias
        && !a.trim().is_empty()
    {
        return (a.clone(), c.clone());
    }
    match snapshot.map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => (s.to_string(), String::new()),
        None => (uin.to_string(), String::new()),
    }
}

/// 把一只瓶子的**原始内容**装成合并转发(「取原文」用):首节点为内容说明(几字几图,
/// 纯图瓶子不至于像「丢了文字」),原文为可复制的文本段、原图为本地归档字节,各占一个
/// 节点,不走排版引擎。署名同 [`bottle_forward`]:匿名署 bot,否则用投放者自设昵称(退快照名 /
/// QQ 号);节点头 QQ 不给上色,故只取名不带色。
pub async fn original_forward(db: &DatabaseConnection, b: &bottle::Model, self_id: Uin) -> Segment {
    let (sender, sender_name) = if b.anonymous {
        (self_id, "匿名漂流瓶".to_string())
    } else {
        let aliases = fetch_aliases(db, &[b.uin]).await;
        let (name, _) = name_with_color(aliases.get(&b.uin), b.nickname.as_deref(), b.uin);
        (Uin(b.uin), name)
    };
    let text = b.text.as_deref().filter(|t| !t.trim().is_empty());
    let images = load_bottle_images(&b.images).await;

    // 说明节点(bot 署名):这只瓶子里有什么。
    let text_desc = match text {
        Some(t) => format!("文字 {} 字", t.chars().count()),
        None => "没有文字".to_string(),
    };
    let image_desc =
        if images.is_empty() { "没有图片".to_string() } else { format!("图片 {} 张", images.len()) };
    let mut nodes =
        vec![ForwardNode::text(self_id, "漂流瓶原文", format!("漂流瓶 #{} 的原始内容:{text_desc},{image_desc}", b.id))];

    if let Some(t) = text {
        nodes.push(ForwardNode::text(sender, sender_name.clone(), t));
    }
    if !images.is_empty() {
        let segs = images
            .iter()
            .map(|img| match &img.bytes {
                Some(bytes) => Segment::image_bytes(bytes.clone()),
                None => Segment::text(MISSING_IMAGE_TEXT),
            })
            .collect();
        nodes.push(ForwardNode::new(sender, sender_name, segs));
    }
    Segment::Forward(Forward::nodes(nodes).title(format!("漂流瓶 #{} 原内容", b.id)))
}

/// 已读出的一张瓶子图。
pub struct BottleImage {
    /// 图片字节;失效且占位图也渲不出(理论不至)为 `None`,由调用方落成文字提示。
    pub bytes: Option<Vec<u8>>,
}

/// 按 md5 从本地归档逐张读瓶子原图字节(发 base64 都用字节:不依赖协议端可读 bot 的盘,
/// 也没有无后缀路径的兼容问题)。读不出(被清理/盘损)不静默吞图:换渐变占位图字节,让看的人
/// 知道这里本来有张图。
async fn load_bottle_images(images: &serde_json::Value) -> Vec<BottleImage> {
    let mut out = Vec::new();
    for md5 in image_names(images) {
        match tokio::fs::read(crate::integrations::media::resolve(&md5)).await {
            Ok(bytes) => {
                out.push(BottleImage { bytes: Some(bytes) });
                tokio::spawn(crate::integrations::media::touch_used(md5)); // 重发即「使用」,刷 last_used
            }
            Err(e) => {
                tracing::warn!(%md5, error = %e, "读漂流瓶图片失败,换占位图");
                match crate::integrations::media::placeholder::missing_image_webp(&md5) {
                    Ok(webp) => out.push(BottleImage { bytes: Some(webp) }),
                    Err(pe) => {
                        tracing::warn!(error = %pe, "渲染占位图失败,该位退文字");
                        out.push(BottleImage { bytes: None });
                    }
                }
            }
        }
    }
    out
}

/// 瓶子卡片图:编号(+匿名标)/ 评分·被捞·剩余 / 时间·来源 / 投放者(自设昵称带色)/ 正文 /
/// 操作提示。瓶内原图不进卡片,由 [`bottle_forward`] 原样发进合并转发。`poster` 为非匿名瓶的
/// 投放者(显示名, 颜色),匿名为 `None`。
pub fn card_image(
    b: &bottle::Model,
    score: Option<f64>,
    poster: Option<(&str, &str)>,
    t: &UserTheme,
) -> anyhow::Result<Vec<u8>> {
    use nagisa::render::{Align, Doc, render_document};

    let pal = &t.palette;
    let mut d = Doc::new();
    d.heading(2, |h| {
        h.text("漂流瓶 ");
        h.styled(format!("#{}", b.id), |s| {
            s.color(&pal.primary);
        });
        if b.anonymous {
            h.styled("  匿名", |s| {
                s.color(&pal.muted).size(0.55);
            });
        }
    });

    // 数据行:评分 · 被捞 · 剩余可捞。
    d.paragraph(|p| {
        p.styled(
            format!("评分 {} · 被捞 {} 次 · 剩余可捞 {}", score_text(score), b.total_pickups, remaining_text(b)),
            |s| {
                s.color(&pal.muted).size(0.92);
            },
        );
    });
    // 时间 / 来源行(匿名隐来源,连「来自群」也不露)。
    let mut meta = format!("丢出于 {}", local_time(&b.created_at).format("%Y-%m-%d %H:%M:%S"));
    if !b.anonymous
        && let Some(gid) = b.group_id
    {
        meta.push_str(&format!(" · 来自群 {gid}"));
    }
    d.paragraph(|p| {
        p.styled(meta, |s| {
            s.color(&pal.muted).size(0.92);
        });
    });

    // 投放者(非匿名):自设昵称带色,退快照名 / QQ 号则缺省色。
    if let Some((name, color)) = poster {
        let col = crate::imaging::readable_hex(color, t.dark);
        d.paragraph(|p| {
            p.styled("投放者 ", |s| {
                s.color(&pal.muted).size(0.92);
            });
            p.styled(name, |s| {
                s.size(0.92);
                match &col {
                    Some(c) => {
                        s.color(c);
                    }
                    None => {
                        s.color(&pal.muted);
                    }
                }
            });
        });
    }

    // 正文(若有):逐行成段,空行跳过(段距本身就是分隔)。
    if let Some(text) = b.text.as_deref().filter(|t| !t.trim().is_empty()) {
        d.divider();
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            d.paragraph(|p| {
                p.text(line);
            });
        }
    }

    // 操作提示脚注:两行各自居中(一长行折行会参差,底部观感就歪了)。评分/评论/取原文一律对着本条
    // 转发回复,编号从回复反查。
    d.divider();
    d.paragraph(|p| {
        p.align(Align::Center).styled("对着本条转发回复即可:", |s| {
            s.color(&pal.muted).size(0.8);
        });
    });
    d.paragraph(|p| {
        p.align(Align::Center).styled(
            "「漂流瓶评分 分数」 · 「漂流瓶评论 内容」 · 「取原文」",
            |s| {
                s.color(&pal.muted).size(0.8);
            },
        );
    });

    Ok(render_document(&d.build(), &render_opts(t))?)
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
        local_time(&b.created_at).format("%Y-%m-%d %H:%M:%S"),
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
    out.push_str(&format!(
        "\n发送「漂流瓶评分 {0} 分数」评分,「漂流瓶评论 {0} 内容」评论;回复本条发送「取原文」取出原始内容",
        b.id
    ));
    out
}

/// 构建一页评论的文档(量高与真渲共用):标题「评论 N 条(·第 i/k 页)」+ 每楼
/// 「楼号 名字 · 时间」+ 内容,楼层间分割线。评论者名走自设昵称(带色)/ 快照名。`offset`
/// 为本页首楼在全部评论里的下标(楼号跨页连续)。
fn comments_doc(
    chunk: &[discuss::Model],
    offset: usize,
    total: usize,
    page: usize,
    pages: usize,
    t: &UserTheme,
    aliases: &HashMap<i64, (String, String)>,
) -> nagisa::render::Document {
    use nagisa::render::Doc;

    let pal = &t.palette;
    let mut d = Doc::new();
    d.heading(4, |h| {
        h.text(format!("评论 {total} 条"));
        if pages > 1 {
            h.styled(format!("  第 {page}/{pages} 页"), |s| {
                s.color(&pal.muted).size(0.7);
            });
        }
    });
    for (j, c) in chunk.iter().enumerate() {
        if j > 0 {
            d.divider();
        }
        let when = local_time(&c.created_at).format("%m-%d %H:%M").to_string();
        let (cname, ccolor) = name_with_color(aliases.get(&c.uin), c.nickname.as_deref(), c.uin);
        let ccol = crate::imaging::readable_hex(&ccolor, t.dark);
        d.paragraph(|p| {
            p.styled(format!("{} 楼", offset + j + 1), |s| {
                s.bold().size(0.85).color(&pal.primary);
            });
            p.styled("  ", |s| {
                s.color(&pal.muted).size(0.85);
            });
            p.styled(cname, |s| {
                s.size(0.85);
                match &ccol {
                    Some(col) => {
                        s.color(col);
                    }
                    None => {
                        s.color(&pal.muted);
                    }
                }
            });
            p.styled(format!(" · {when}"), |s| {
                s.color(&pal.muted).size(0.85);
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
    t: &UserTheme,
    aliases: &HashMap<i64, (String, String)>,
) -> anyhow::Result<Vec<u8>> {
    use nagisa::render::render_document;
    Ok(render_document(&comments_doc(chunk, offset, total, page, pages, t, aliases), &render_opts(t))?)
}

/// 评论按渲染高度装箱分页:逐楼试加、量高([`nagisa::render::measure_document`],只排版
/// 不绘制),超过 `COMMENTS_PAGE_MAX_PX` 就在上一楼收页。每页至少一楼(单楼超高独占
/// 一页)。返回各页在 `comments` 里的 `(起, 止)` 下标(止开区间)。
pub fn paginate_comments(
    comments: &[discuss::Model],
    t: &UserTheme,
    aliases: &HashMap<i64, (String, String)>,
) -> anyhow::Result<Vec<(usize, usize)>> {
    use nagisa::render::measure_document;

    let opts = render_opts(t);
    let n = comments.len();
    let total = n;
    let mut spans = Vec::new();
    let mut start = 0;
    while start < n {
        // 至少装一楼;之后每多装一楼量一次高,超限即收页。
        // (页标会让标题行多一小段,量高时按多页形态算,高度不受页码数字影响。)
        let mut cut = start + 1;
        while cut < n {
            let doc = comments_doc(&comments[start..=cut], start, total, 1, 2, t, aliases);
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
    t: &UserTheme,
) -> anyhow::Result<Vec<u8>> {
    use nagisa::render::{Align, Doc, render_document};

    let pal = &t.palette;
    let mut d = Doc::new();
    d.heading(3, |h| {
        h.text(format!("你的漂流瓶(近 {} 个)", rows.len()));
    });
    d.table(|tb| {
        tb.head(["编号", "丢出时间", "被捞(剩)", "评分", "状态", "匿名"]);
        tb.align([Align::Left, Align::Left, Align::Center, Align::Center, Align::Center, Align::Center]);
        tb.expand(); // 铺满内容宽,列距舒展
        for (i, b) in rows.iter().enumerate() {
            let score = scores.get(&b.id).map(|s| format!("{s}分")).unwrap_or_else(|| "—".to_string());
            tb.row([
                format!("#{}", b.id),
                local_time(&b.created_at).format("%m-%d %H:%M").to_string(),
                format!("{}({})", b.total_pickups, remaining_text(b)),
                score,
                status_text(&b.status).to_string(),
                if b.anonymous { "是" } else { "" }.to_string(),
            ]);
            // 编号列上主色,行有锚点好对着说事。
            tb.cell_style(i, 0, |s| {
                s.color(&pal.primary).weight(600);
            });
        }
    });

    Ok(render_document(&d.build(), &render_opts(t))?)
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

/// 评论者文字退路显示名(渲染失败的文字楼层用):快照名非空用它,否则 QQ 号。
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
    images.as_array().map(|arr| arr.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default()
}
