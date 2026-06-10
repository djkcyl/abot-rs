//! 捞瓶 / 查瓶的合并转发呈现 —— 把一只瓶子渲染成多节点的合并转发：瓶子节点（编号 / 评分 /
//! 丢出时间 / 来源 / 文本 / 图）+ 每条评论一节点 + 末尾操作提示节点。
//!
//! 只读：评分均值走 [`logic::score_avg`]、评论走 [`logic::get_discuss`]，图片从盘按文件名
//! 重发（[`crate::media::resolve`]，QQ 的图片 URL 会过期，故捞取时一律本地重发）。匿名瓶子的
//! 首节点署名 bot 自己、名「匿名漂流瓶」，并隐藏来源群；非匿名署名投放者。

use nagisa::prelude::*;
use sea_orm::DatabaseConnection;

use super::entity::bottle;
use super::logic;

/// 把一只瓶子渲染成合并转发。
///
/// 节点构成：瓶子节点（编号 / 评分 / 丢出时间 / 来源 + 文本 + 图片）、评论楼层各一节点、
/// 末尾操作提示节点。`self_id` 用作匿名瓶子与提示节点的署名。
pub async fn bottle_forward(
    db: &DatabaseConnection,
    b: &bottle::Model,
    self_id: Uin,
) -> anyhow::Result<Segment> {
    let score = logic::score_avg(db, b.id).await?;
    let comments = logic::get_discuss(db, b.id).await?;

    // —— 首节点：瓶子本体。——
    // 匿名 → 署名 bot、名「匿名漂流瓶」；非匿名 → 署名投放者 uin + 显示名（缺则 QQ 号）。
    let (sender, sender_name) = if b.anonymous {
        (self_id, "匿名漂流瓶".to_string())
    } else {
        let name = b.nickname.clone().filter(|s| !s.trim().is_empty()).unwrap_or_else(|| b.uin.to_string());
        (Uin(b.uin), name)
    };

    let score_text = match score {
        Some(s) => format!("{s} 分"),
        None => "暂无".to_string(),
    };
    let mut header = format!(
        "编号 {}\n评分 {}\n丢出时间 {}",
        b.id,
        score_text,
        b.created_at.format("%Y-%m-%d %H:%M:%S"),
    );
    // 来源：非匿名且有来源群才显示；匿名隐藏来源（连「来自群」也不露）。
    if !b.anonymous
        && let Some(gid) = b.group_id
    {
        header.push_str(&format!("\n来自群 {gid}"));
    }

    let mut content = vec![Segment::text(header)];
    if let Some(text) = b.text.as_deref().filter(|t| !t.is_empty()) {
        content.push(Segment::text(text.to_string()));
    }
    for md5 in image_names(&b.images) {
        content.push(Segment::image_path(crate::media::resolve(&md5)));
        tokio::spawn(crate::media::touch_used(md5)); // 重发即「使用」,刷 last_used
    }

    let mut nodes = vec![ForwardNode::new(sender, sender_name, content)];

    // —— 评论楼层：每条一节点，楼层号从 1 起。——
    for (i, c) in comments.iter().enumerate() {
        let name = c.nickname.clone().filter(|s| !s.trim().is_empty()).unwrap_or_else(|| c.uin.to_string());
        nodes.push(ForwardNode::new(
            Uin(c.uin),
            name,
            vec![Segment::text(format!("{}楼：{}", i + 1, c.text))],
        ));
    }

    // —— 末节点：操作提示。——
    nodes.push(ForwardNode::text(
        self_id,
        "漂流瓶",
        format!("发送「漂流瓶评分 {0} 分数」评分，「漂流瓶评论 {0} 内容」评论", b.id),
    ));

    Ok(Segment::forward(nodes))
}

/// 解析瓶子 `images`（JSONB 字符串数组）成内容 md5 序列；非数组 / 非字符串项跳过。
fn image_names(images: &serde_json::Value) -> Vec<String> {
    images
        .as_array()
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
        .unwrap_or_default()
}
