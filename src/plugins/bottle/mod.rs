//! 漂流瓶插件 —— 跨群共享的匿名投放/打捞玩法。
//!
//! 用户把文本与图片装进瓶子丢进「大海」，内容过审核后入池；别人随机捞起一个看，还能评分、评论。
//! 投放即审：文本走 [`ContentModerator`] 文本审核、图片走图片审核，命中即转人工待审（在网页「审核」页
//! 处理），否则直接入池。捞瓶/看瓶以合并转发呈现。自有三表 `bottle` / `bottle_score` / `bottle_discuss`，
//! 数据层见 [`entity`] / [`logic`]，审核接入见 `review`。

pub mod entity;
pub mod images;
pub mod logic;
mod migration;
mod render;
mod review;

use nagisa::prelude::*;
use serde_json::json;

use crate::config::Master;
use crate::data::AUser;
use crate::moderation::ContentModerator;
use crate::COIN_NAME;
use logic::{DeleteOutcome, DiscussOutcome, NewBottle};

plugin! {
    key = "bottle",
    name = "漂流瓶",
    category = Fun,
    description = "跨群漂流瓶，丢一个、捞一个",
    usage = "「丢漂流瓶 内容」投放（可加图），「-a」匿名、「-r 次数」限定可捞次数；\
「捞漂流瓶」随机捞一个；「查漂流瓶」看自己的瓶子、「查漂流瓶 编号」看某只；\
「删漂流瓶 编号」回收；「漂流瓶评分 编号 分数」打 1-5 分；「漂流瓶评论 编号 内容」写评论。",
    can_disable = true,
}

/// 投放基础花费。
const THROW_BASE: i64 = 2;
/// 含文本额外花费。
const THROW_PER_TEXT: i64 = 1;
/// 每张图片额外花费。
const THROW_PER_IMAGE: i64 = 3;
/// 打捞花费。
const FISH_COST: i64 = 3;
/// 删除（回收）退款。
const DELETE_REFUND: i64 = 1;

/// 投放消息携带的内容是否要装进瓶子的下限（文本去空白后非空，或有图片）。
/// `丢漂流瓶` / `扔漂流瓶` 的参数：`-a` 匿名旗标、`-r 次数` 限定可捞次数、`-p` 进交互发图、其余为瓶子文本。
///
/// 文本走 `#[arg(rest, raw)]` 收尾（保真空白、自动跳过前导旗标），故旗标与自由文本可共存，
/// 无需手解析消息段。图片不走 Args，由 handler 从 `m.content` 另取，或 `-p` 时在后续消息里收。
#[derive(Args)]
struct ThrowArgs {
    /// `-a` / `--anonymous`：匿名投放。
    #[arg(flag, short = 'a', long = "anonymous", desc = "匿名投放，不显示来源和投放者")]
    anonymous: bool,
    /// `-r` / `--remaining`：限定可捞次数（默认 -1 不限，范围 1..=1000）。
    #[arg(long = "remaining", short = 'r', default = "-1", name = "次数", desc = "限定可被捞次数（1-1000，不填则不限）")]
    remaining: i64,
    /// `-p` / `--pic`：进交互流程，在后续消息里把图片发给 bot（可多张）。
    #[arg(flag, short = 'p', long = "pic", desc = "进交互流程，在后续消息里把图片发给我（可多张）")]
    pic: bool,
    /// 瓶子文本（旗标之后的自由文本，保真）。
    #[arg(rest, raw, name = "内容", desc = "瓶子的文字（配图时可不写）")]
    text: String,
}

/// `丢漂流瓶` / `扔漂流瓶` —— 把文本与图片装进瓶子丢进大海。
///
/// 先校验内容非空、次数范围、余额；再过审核（文本 + 每张图），命中即转人工待审；
/// 最后带闸扣费、建瓶。匿名隐去来源；私聊瓶子 `group_id` 为 `None`。
#[command("丢漂流瓶", "扔漂流瓶",
    order = 1,
    description = "把文字或图片装进瓶子丢进大海",
    usage = "花费 2 币起，含文字加 1、每张图加 3；投放前会把花费报给你、回 y 确认才扣。投放后过内容审核，命中关键词的转人工待审。")]
async fn throw(
    reply: Reply,
    mut user: AUser,
    m: MessageEvent,
    session: Session,
    args: Args<ThrowArgs>,
) -> HandlerResult {
    // 防并发：同一个人同时只能跑一条投放流程（`-p` 会等后续消息，必须串行化），守卫持到流程结束。
    let Some(_guard) = session.single_flight_user() else {
        reply.reply("你正在投放流程中，先发完或等它超时吧").await?;
        return Ok(());
    };

    let ThrowArgs { anonymous, remaining, pic, text } = args.0;
    let db = user.db().clone();

    // 文本去空白后的内容（空则视作纯图片瓶子）。`-p` 流程里若首条没文本，可由后续消息补上。
    let mut text = text.trim().to_string();

    // 次数范围：-1（默认）= 不限；显式给值须在 1..=1000（单只瓶子的可被捞总次数上限）。
    if remaining != -1 && !(1..=1000).contains(&remaining) {
        reply.reply("可捞次数要在 1 到 1000 之间，或不填表示不限").await?;
        return Ok(());
    }
    let remaining = remaining as i32;

    // 收集图片 + 拿到一个会话 waiter（作用域到当初发问的同一个人）。
    // `-p`：进交互、在后续消息里收图（顺带可补文本）；否则只从本条消息取图。
    let waiter = session.waiter().from_starter().build();
    let mut images = Vec::new();
    if pic {
        reply.reply("好的，把要放进瓶子的图片发给我吧（可多张，发完发「好了」，放弃发「取消」）").await?;
        loop {
            let Some(next) = waiter.recv::<MessageEvent>(std::time::Duration::from_secs(120)).await else {
                reply.reply("等太久了，这次先不丢了").await?;
                return Ok(());
            };
            let next_text = next.content.extract_text();
            let next_text = next_text.trim();
            if next_text == "取消" {
                reply.reply("已取消").await?;
                return Ok(());
            }
            if matches!(next_text, "好了" | "完成" | "发完了") || next_text.eq_ignore_ascii_case("ok") {
                break;
            }
            let imgs = images::fetch_and_store(&next.content).await;
            if !imgs.is_empty() {
                let n = imgs.len();
                images.extend(imgs);
                reply.reply(format!("收到 {n} 张，继续发或发「好了」")).await?;
            } else if !next_text.is_empty() && text.is_empty() {
                // 没图但有文字，且瓶子还没文本 → 当作瓶子文本收下。
                text = next_text.to_string();
                reply.reply("记下文字了，继续发图或发「好了」").await?;
            } else {
                reply.reply("没看到图片，再发一张，或发「好了」结束").await?;
            }
        }
    } else {
        // 非交互：图片只从触发消息里取（逐张下载落盘，顺带拿字节喂审核）。
        images = images::fetch_and_store(&m.content).await;
    }

    let has_text = !text.is_empty();
    if !has_text && images.is_empty() {
        reply.reply("瓶子里总得装点什么吧，写句话或配张图").await?;
        return Ok(());
    }

    // 花费：基础 + 含文本 + 每图。先挡明显不够的（不够就无需走确认）。
    let cost = THROW_BASE + if has_text { THROW_PER_TEXT } else { 0 } + THROW_PER_IMAGE * images.len() as i64;
    if user.coin() < cost {
        reply
            .reply(format!("投放这个瓶子要 {cost} {COIN_NAME}，你只有 {} {COIN_NAME}，不够呢", user.coin()))
            .await?;
        return Ok(());
    }

    // 投放前确认：把花费报给用户，等一句 y/n（非法自动重问，「取消」当否）。
    let summary = format!(
        "这个瓶子：{}{}，投放要花 {cost} {COIN_NAME}。确认丢出吗？回复 y 确认、n 取消",
        if has_text { "有文字" } else { "无文字" },
        if images.is_empty() { String::new() } else { format!("、{} 张图", images.len()) },
    );
    reply.reply(summary).await?;
    let confirmed = waiter
        .recv_parse(std::time::Duration::from_secs(60), "取消", |s| {
            let t = s.trim().to_lowercase();
            match t.as_str() {
                "y" | "yes" | "是" | "确认" | "嗯" | "丢" => Ok(true),
                "n" | "no" | "否" | "不" | "算了" => Ok(false),
                _ => Err("回复 y 确认、n 取消".to_string()),
            }
        })
        .await;
    match confirmed {
        Some(true) => {}
        Some(false) => {
            reply.reply("那这个瓶子先不丢了").await?;
            return Ok(());
        }
        None => {
            reply.reply("没等到确认，这次先不丢了").await?;
            return Ok(());
        }
    }

    // 内容审核（确认之后才做）：文本 + 每张图字节。任一不安全 → 转人工待审（pending）并记首个命中详情。
    let moderator = ContentModerator::shared();
    let mut hit: Option<serde_json::Value> = None;
    if has_text {
        let v = moderator.moderate_text(&text).await;
        if !v.safe {
            hit = Some(json!({
                "label": v.label,
                "sub_label": v.sub_label,
                "source": v.source,
                "where": "text",
            }));
        }
    }
    if hit.is_none() {
        for (i, img) in images.iter().enumerate() {
            let v = moderator.moderate_image(&img.bytes).await;
            if !v.safe {
                hit = Some(json!({
                    "label": v.label,
                    "sub_label": v.sub_label,
                    "source": v.source,
                    "where": format!("image#{}", i + 1),
                }));
                break;
            }
        }
    }
    let (status, moderation) = match hit {
        Some(m) => ("pending".to_string(), Some(m)),
        None => ("ai_approved".to_string(), None),
    };

    // 带闸扣费：余额够才扣、才建瓶（确认/审核期间余额可能已变，故扣不动就不建）。
    if !user.pay(cost, "投放漂流瓶").await? {
        reply.reply("金币不够了").await?;
        return Ok(());
    }

    let id = logic::create_bottle(
        &db,
        NewBottle {
            uin: user.uin(),
            nickname: sender_nickname(&m),
            group_id: source_group(&m),
            text: has_text.then_some(text),
            images: images.into_iter().map(|i| i.filename).collect(),
            anonymous,
            remaining,
            status: status.clone(),
            moderation,
        },
    )
    .await
    .context("建瓶")?;

    let msg = if status == "pending" {
        "漂流瓶已投放，正在等待审核～".to_string()
    } else {
        format!("漂流瓶已投放，编号 {id}")
    };
    reply.reply(msg).await?;
    Ok(())
}

/// `捞漂流瓶` / `捡漂流瓶` —— 随机捞起一个瓶子，合并转发呈现。
///
/// 无候选不扣费；有候选则带闸扣费、记一次打捞、再渲染。
#[command("捞漂流瓶", "捡漂流瓶",
    order = 2,
    description = "从大海里随机捞起一个瓶子",
    usage = "发送「捞漂流瓶」随机捞起一个别人丢的瓶子，花费 3 币；大海里没瓶子时不扣费。捞到的瓶子以合并转发呈现，记得它的编号，可以评分或评论。")]
async fn fish(reply: Reply, mut user: AUser, bot: Bot) -> HandlerResult {
    let db = user.db().clone();

    let Some(bottle) = logic::select_candidate(&db).await.context("挑选可捞的瓶子")? else {
        reply.reply("大海里暂时没有漂流瓶……过会儿再来吧").await?;
        return Ok(());
    };

    if !user.pay(FISH_COST, "打捞漂流瓶").await? {
        reply
            .reply(format!("打捞一次要 {FISH_COST} {COIN_NAME}，你现在只有 {} {COIN_NAME}", user.coin()))
            .await?;
        return Ok(());
    }

    logic::record_pickup(&db, bottle.id).await.context("记录打捞")?;
    let forward = render::bottle_forward(&db, &bottle, bot.self_id()).await.context("渲染漂流瓶")?;
    reply.send(&[forward]).await?;
    Ok(())
}

/// `查漂流瓶` —— 无编号列出自己的瓶子，带编号看某只详情。
///
/// 列表只列自己的；详情对本人 / 主人可看完整（含状态），他人仅能看已通过瓶子的公开内容，
/// 待审 / 驳回的对他人一律「没有这个漂流瓶」（不泄露存在）。
#[command("查漂流瓶",
    order = 3,
    description = "查看自己丢过的瓶子，或按编号看某只",
    usage = "别人的瓶子只有通过审核的才看得到。")]
async fn check(reply: Reply, user: AUser, bot: Bot, State(master): State<Master>, args: Args<CheckArgs>) -> HandlerResult {
    let db = user.db().clone();
    let me = user.uin();

    let Some(id) = args.0.id else {
        // 无编号：列出自己的瓶子（近 20 条）。
        let rows = logic::list_user_bottles(&db, me, 20).await.context("列出我的漂流瓶")?;
        if rows.is_empty() {
            reply.reply("你还没有丢过漂流瓶").await?;
            return Ok(());
        }
        // 批量取这批瓶子的评分（一次查询），再按瓶逐行展示。
        let ids: Vec<i64> = rows.iter().map(|b| b.id).collect();
        let scores = logic::score_avgs(&db, &ids).await.context("取漂流瓶评分")?;
        // 列表走合并转发：**按「一条瓶子」为单位切节点**（chunk_items，不会把某条瓶子拆到两节点）。
        let mut items: Vec<String> = Vec::with_capacity(rows.len() + 1);
        items.push(format!("你的漂流瓶（近 {} 个）", rows.len()));
        items.extend(rows.iter().map(|b| fmt_list_line(b, scores.get(&b.id).copied())));
        let nodes = ForwardNode::chunk_items(bot.self_id(), "我的漂流瓶", items, "\n", 3000);
        reply.send(&[Segment::Forward(Forward::nodes(nodes).title("我的漂流瓶"))]).await?;
        return Ok(());
    };

    let Some(b) = logic::get_bottle(&db, id).await.context("按编号取瓶")? else {
        reply.reply("没有这个漂流瓶").await?;
        return Ok(());
    };
    if b.isdelete {
        reply.reply("没有这个漂流瓶").await?;
        return Ok(());
    }

    // 可见性：本人 / 主人可看（任何状态）；他人仅可看已通过的公开瓶子；否则当作不存在。
    let is_owner = b.uin == me;
    let is_master = master.0.0 != 0 && me == master.0.0;
    let public = matches!(b.status.as_str(), "approved" | "ai_approved");
    if !is_owner && !is_master && !public {
        reply.reply("没有这个漂流瓶").await?;
        return Ok(());
    }

    let forward = render::bottle_forward(&db, &b, bot.self_id()).await.context("渲染漂流瓶")?;
    reply.send(&[forward]).await?;
    Ok(())
}

/// `查漂流瓶` 的参数：可选编号。
#[derive(Args)]
struct CheckArgs {
    /// 瓶子编号；缺则列出自己的瓶子。
    #[arg(name = "编号", desc = "某只瓶子的编号；不填则列出自己的瓶子")]
    id: Option<i64>,
}

/// `删漂流瓶` —— 软删自己的瓶子并退款；主人可删任意瓶子。
#[command("删漂流瓶",
    order = 4,
    description = "回收自己丢的瓶子并退还 1 币",
    usage = "只能删自己的瓶子，主人可删任意；回收退还 1 币。")]
async fn remove(reply: Reply, mut user: AUser, State(master): State<Master>, args: Args<RemoveArgs>) -> HandlerResult {
    let db = user.db().clone();
    let me = user.uin();
    let is_master = master.0.0 != 0 && me == master.0.0;
    let id = args.0.id;

    match logic::delete_bottle(&db, id, me, is_master).await.context("回收漂流瓶")? {
        DeleteOutcome::Deleted => {
            user.add_coin(DELETE_REFUND, "回收漂流瓶退款").await?;
            reply.reply(format!("已回收漂流瓶 {id}，退还 {DELETE_REFUND} {COIN_NAME}")).await?;
        }
        DeleteOutcome::NotFound => {
            reply.reply("没有这个漂流瓶").await?;
        }
        DeleteOutcome::NotOwner => {
            reply.reply("只能删自己的漂流瓶").await?;
        }
    }
    Ok(())
}

/// `删漂流瓶` 的参数：必填编号。
#[derive(Args)]
struct RemoveArgs {
    /// 要回收的瓶子编号。
    #[arg(name = "编号", desc = "要回收的瓶子编号")]
    id: i64,
}

/// `漂流瓶评分` —— 给瓶子打 1-5 分，可改分（upsert）。
#[command("漂流瓶评分",
    order = 5,
    description = "给捞到的瓶子打 1-5 分",
    usage = "同一个瓶子可重复打分，新分覆盖旧分。")]
async fn rate(reply: Reply, user: AUser, args: Args<RateArgs>) -> HandlerResult {
    let db = user.db().clone();
    let RateArgs { id, score } = args.0;

    if !(1..=5).contains(&score) {
        reply.reply("评分要在 1 到 5 之间").await?;
        return Ok(());
    }
    match logic::get_bottle(&db, id).await.context("按编号取瓶")? {
        Some(b) if !b.isdelete => {}
        _ => {
            reply.reply("没有这个漂流瓶").await?;
            return Ok(());
        }
    }
    logic::set_score(&db, id, user.uin(), score as i16).await.context("写评分")?;
    reply.reply(format!("已为漂流瓶 {id} 评 {score} 分")).await?;
    Ok(())
}

/// `漂流瓶评分` 的参数：编号 + 分数。
#[derive(Args)]
struct RateArgs {
    /// 瓶子编号。
    #[arg(name = "编号", desc = "瓶子编号")]
    id: i64,
    /// 分数（1-5）。
    #[arg(name = "分数", desc = "1 到 5")]
    score: i64,
}

/// `漂流瓶评论` —— 给瓶子写评论（3-500 字，每人每瓶至多 3 条）。
#[command("漂流瓶评论",
    order = 6,
    description = "给捞到的瓶子写一条评论",
    usage = "同一个瓶子每人最多评 3 条。")]
async fn comment(reply: Reply, user: AUser, m: MessageEvent, args: Args<CommentArgs>) -> HandlerResult {
    let db = user.db().clone();
    let CommentArgs { id, text } = args.0;
    let text = text.trim().to_string();

    let len = text.chars().count();
    if !(3..=500).contains(&len) {
        reply.reply("评论要 3 到 500 字").await?;
        return Ok(());
    }
    match logic::get_bottle(&db, id).await.context("按编号取瓶")? {
        Some(b) if !b.isdelete => {}
        _ => {
            reply.reply("没有这个漂流瓶").await?;
            return Ok(());
        }
    }
    match logic::add_discuss(&db, id, user.uin(), sender_nickname(&m), &text).await.context("写评论")? {
        DiscussOutcome::Added => {
            reply.reply(format!("已评论漂流瓶 {id}")).await?;
        }
        DiscussOutcome::LimitReached => {
            reply.reply("你对这个漂流瓶已经评论 3 条啦").await?;
        }
    }
    Ok(())
}

/// `漂流瓶评论` 的参数：编号 + 自由文本内容。
#[derive(Args)]
struct CommentArgs {
    /// 瓶子编号。
    #[arg(name = "编号", desc = "瓶子编号")]
    id: i64,
    /// 评论内容（自由文本，保真）。
    #[arg(rest, raw, name = "内容", desc = "评论内容（3 到 500 字）")]
    text: String,
}

/// 列表单行：`#编号 时间 · 被捞 N(剩 M) · 评分 X · 状态 · 匿名`。`score` 为该瓶去极值均值（无则「无评分」）。
fn fmt_list_line(b: &entity::bottle::Model, score: Option<f64>) -> String {
    let remaining = if b.remaining_pickups < 0 { "不限".to_string() } else { b.remaining_pickups.to_string() };
    let score_text = match score {
        Some(s) => format!("{s} 分"),
        None => "无评分".to_string(),
    };
    let mut line = format!(
        "#{} {} · 被捞 {}(剩 {}) · 评分 {} · {}",
        b.id,
        b.created_at.format("%m-%d %H:%M"),
        b.total_pickups,
        remaining,
        score_text,
        status_text(&b.status),
    );
    if b.anonymous {
        line.push_str(" · 匿名");
    }
    line
}

/// 审核状态的中文呈现。
fn status_text(status: &str) -> &'static str {
    match status {
        "pending" => "待审核",
        "approved" | "ai_approved" => "已通过",
        "rejected" => "未通过",
        _ => "未知",
    }
}

/// 取发送者显示名：群消息用群名片/昵称，私聊用好友备注/昵称；都取不到为 `None`。
fn sender_nickname(m: &MessageEvent) -> Option<String> {
    let name = m
        .member
        .as_ref()
        .map(|mi| mi.display_name())
        .or_else(|| m.friend.as_ref().map(|f| f.display_name()))
        .unwrap_or("");
    let name = name.trim();
    (!name.is_empty()).then(|| name.to_string())
}

/// 来源群号：群消息为 `Some(群号)`，私聊为 `None`。
fn source_group(m: &MessageEvent) -> Option<i64> {
    m.peer.is_group().then_some(m.peer.id.0)
}
