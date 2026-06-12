//! 画板插件 —— 一块**全局共享**的像素画布(r/place 风格)。群成员花游戏币 + 受冷却限制地
//! 往上落像素,几周合作画出一幅集体壁画。
//!
//! 设计要点:画布 256×144,32 色调色板;每格按累计落格量缓涨的币价扣费,落格后进冷却
//! (等级越高越短);出图读画布真值表,回放从周期快照 + 逐笔审计派生,全量回放按业务日缓存。
//!
//! 模块分工:
//! - `colors`:32 色调色板(索引↔RGB、名字/别名→索引、文字图例)。
//! - `entity`:四张插件自有表(`place_pixel` 真值 + `place_history` 审计 + 画布快照 + 回放缓存)。
//! - `migration`:建表迁移(经 `PluginMigration` 自注册)。
//! - `font`:5×7 点阵字(数字 + A–Z,渲染刻度 / 区块号 / 水印,零字体依赖)。
//! - `render`:全图 / 放大窗 / 区块 / 总览 / 色板 / 干净分享图。
//! - `logic`:冷却 / 币价 / 落格事务 / 历史查询 / 回放窗口与计费 / 缓存。
//! - `replay`:时间轴回放 GIF(无参走当日缓存免费,带窗现做按量收费)。
//! - `profile`:个人数据「落格数」战绩(进 mydata)。
//!
//! 命令:`画板`(全图 / 放大 / 干净分享图)、`色板`、`作画`(快捷 / 区块导航引导)、`回放`(GIF)。
//! 触碰共享经济只走 `AUser`/`add_coin_on`;画布与历史全落本插件自有表。

use std::time::Duration;

use nagisa::prelude::*;
use sea_orm::DatabaseConnection;

use crate::COIN_NAME;
use crate::data::{AUser, Db};
use logic::PlaceResult;

mod colors;
mod entity;
mod font;
mod logic;
mod migration;
mod profile;
mod render;
mod replay;

plugin! {
    key = "place",
    name = "画板",
    category = Fun,
    description = "大家共用的像素画板，一次落一格慢慢拼图，能放大看、出历程回放。",
    can_disable = true,
}

/// 把一段文本解析成坐标 `(x, y)`:用逗号 / 空白 / 常见分隔符切成两段,各解析为整数。
fn parse_xy(s: &str) -> Option<(i32, i32)> {
    let parts: Vec<&str> =
        s.split([',', '，', ' ', '\t', ';', '；', '|', ':', '：']).filter(|t| !t.is_empty()).collect();
    if parts.len() != 2 {
        return None;
    }
    Some((parts[0].parse().ok()?, parts[1].parse().ok()?))
}

/// 解析区块号:可带前缀 `#`/`＃`,值 1..=BLOCKS。
fn parse_block(s: &str) -> Option<u8> {
    let n: i64 = s.trim().trim_start_matches(['#', '＃']).parse().ok()?;
    (1..=render::BLOCKS as i64).contains(&n).then_some(n as u8)
}

/// 快捷解析:`rest` = 坐标 + 颜色(末词为颜色,其余为坐标)。三者齐全才返回(范围由落格事务校验)。
fn parse_quick(rest: &str) -> Option<(i32, i32, u8)> {
    let words: Vec<&str> = rest.split_whitespace().collect();
    if words.len() < 2 {
        return None;
    }
    let color = colors::parse_color(words[words.len() - 1])?;
    let (x, y) = parse_xy(&words[..words.len() - 1].join(" "))?;
    Some((x, y, color))
}

/// 坐标是否在画布内。
fn in_bounds(x: i32, y: i32) -> bool {
    (0..render::W as i32).contains(&x) && (0..render::H as i32).contains(&y)
}

/// 是否为「干净分享图」关键词。
fn is_clean_kw(s: &str) -> bool {
    matches!(s, "净" | "干净" | "clean" | "分享" | "share")
}

/// 冷却提示文案。
fn cooldown_msg(remain_min: i64, interval_min: i64, level: i64) -> String {
    format!("手还没缓过来，再等 {remain_min} 分钟（你 {level} 级，间隔 {interval_min} 分钟）")
}

/// `画板` —— 无参看全图;`画板 x,y` 看放大窗;`画板 净` 出干净分享图。
#[command(
    "画板",
    order = 1,
    description = "看公共像素画板",
    usage = "发送「画板」看全图，「画板 x,y」以该点为心看局部放大窗（每格更大、带坐标刻度和准星），\
「画板 净」出一张无网格无刻度的干净分享图（带日期水印）。画布 256×144 格、32 色，所有群共用同一块。"
)]
async fn place_view(reply: Reply, Db(db): Db, args: ArgText) -> HandlerResult {
    let rest = args.0.trim();
    let img = if rest.is_empty() {
        render::render_full(&db).await
    } else if is_clean_kw(rest) {
        render::render_clean(&db).await
    } else if let Some((x, y)) = parse_xy(rest) {
        if !in_bounds(x, y) {
            reply.reply("坐标超出范围（x 0-255，y 0-143）").await?;
            return Ok(());
        }
        render::render_zoom(&db, x, y).await
    } else {
        reply.reply("看全图发「画板」，看局部发「画板 x,y」，分享图发「画板 净」").await?;
        return Ok(());
    };
    match img {
        Ok(bytes) => reply.msg().image_bytes(bytes).send().await?,
        Err(e) => {
            tracing::warn!(error = %e, "渲染画板失败");
            reply.reply("画板出图失败了，过会儿再试").await?
        }
    };
    Ok(())
}

/// `画板色板` —— 32 色编号块图 + 文字图例。
#[command(
    "画板色板",
    order = 3,
    description = "看画板的 32 色调色板",
    usage = "发送「画板色板」，出一张 32 色的编号块图和文字图例。作画时按编号或颜色名选色。"
)]
async fn palette(reply: Reply) -> HandlerResult {
    match render::render_palette() {
        Ok(bytes) => reply.msg().image_bytes(bytes).text(colors::legend()).send().await?,
        Err(e) => {
            tracing::warn!(error = %e, "渲染画板色板失败,退回文字图例");
            reply.reply(colors::legend()).await?
        }
    };
    Ok(())
}

/// `画板回放` —— 画布历程 GIF。无参出**当日缓存**的全量回放(免费,业务日内一份,
/// 后台日任务预热、缺了首触发现做补上);`N天` 是现做的窗口回放(首帧为窗口起点时刻
/// 的真实画布),按窗内笔数计费(每 1000 笔 1 币,封顶 88),先报价、y/n 确认再扣。
#[command(
    "画板回放",
    order = 5,
    description = "把画板历程做成 GIF 回放",
    usage = "发送「画板回放」看全量回放（每天更新一份，免费），最长 30 秒，节奏按落格总量自动调。\
「画板回放 7天」现做最近 7 天的回放（从当时的画布演变到现在），按量收费：每 1000 笔 1 金币、\
封顶 88，做之前会报价，回复 y 才扣。"
)]
async fn replay_cmd(reply: Reply, Db(db): Db, mut user: AUser, session: Session, args: ArgText) -> HandlerResult {
    // —— 无参:当日缓存,免费。——
    let Some(days) = parse_replay_args(args.0.trim()).days else {
        match replay::full_cached(&db).await? {
            Some(gif) => reply.msg().image_bytes(gif).send().await?,
            None => reply.reply("还没有落格记录，画几笔才有回放").await?,
        };
        return Ok(());
    };

    // —— 带窗:称重 → 报价 y/n → 带闸扣币 → 现做(失败退款)。——
    let since = (chrono::Local::now() - chrono::Duration::days(days)).fixed_offset();
    let Some(start) = logic::window_start_id(&db, since).await? else {
        reply.reply(format!("最近 {days} 天没有人落格")).await?;
        return Ok(());
    };
    let strokes = logic::strokes_from(&db, start).await?;
    let cost = logic::replay_cost(strokes);
    if user.coin() < cost {
        reply
            .reply(format!("这段回放有 {strokes} 笔，现做要 {cost} {COIN_NAME}，你只有 {} {COIN_NAME}", user.coin()))
            .await?;
        return Ok(());
    }
    reply.reply(format!("最近 {days} 天共 {strokes} 笔，现做要 {cost} {COIN_NAME}。回复 y 确认、n 取消")).await?;
    let waiter = session.waiter().from_starter().build();
    let confirmed = waiter
        .recv_parse(Duration::from_secs(60), "取消", |s| match s.trim().to_lowercase().as_str() {
            "y" | "yes" | "是" | "确认" | "嗯" => Ok(true),
            "n" | "no" | "否" | "不" | "算了" => Ok(false),
            _ => Err("回复 y 确认、n 取消".to_string()),
        })
        .await;
    match confirmed {
        Some(true) => {}
        Some(false) => {
            reply.reply("行，不做了").await?;
            return Ok(());
        }
        None => {
            reply.reply("没等到确认，先不做了").await?;
            return Ok(());
        }
    }
    if !user.pay(cost, "画板回放").await? {
        reply.reply(format!("余额不够了，要 {cost} {COIN_NAME}")).await?;
        return Ok(());
    }
    match replay::render_replay(&db, replay::ReplayArgs { days: Some(days) }).await {
        Ok(Some(gif)) => {
            reply.msg().image_bytes(gif).send().await?;
        }
        Ok(None) => {
            user.add_coin(cost, "画板回放退款").await?;
            reply.reply("这段时间翻不出落格了，钱退你").await?;
        }
        Err(e) => {
            user.add_coin(cost, "画板回放退款").await?;
            tracing::warn!(error = %e, "窗口回放生成失败");
            reply.reply("出图失败了，钱退你，过会儿再试").await?;
        }
    }
    Ok(())
}

/// 启动时挂回放缓存的**日预热**任务:先补当日缺份,之后每个业务日界(凌晨 4 点)
/// 过后 5 分钟重生成——全量回放最重的一次扫描固定摊在凌晨,白天无参触发全走缓存。
/// `Ready` 每次 run 恰一次;进程级一次闸防多挂。
#[event(Ready, id = "replay_warm")]
async fn replay_warm(_r: Ready, Db(db): Db) -> HandlerResult {
    use std::sync::atomic::{AtomicBool, Ordering};
    static STARTED: AtomicBool = AtomicBool::new(false);
    if STARTED.swap(true, Ordering::SeqCst) {
        return Ok(());
    }
    tokio::spawn(async move {
        loop {
            if let Err(e) = replay::warm_cache(&db).await {
                tracing::warn!(error = %e, "预热回放缓存失败");
            }
            let next =
                crate::data::util::business_day_start() + chrono::Duration::days(1) + chrono::Duration::minutes(5);
            let wait = (next - chrono::Local::now().fixed_offset()).to_std().unwrap_or(Duration::from_secs(3600));
            tokio::time::sleep(wait).await;
        }
    });
    Ok(())
}

/// 解析回放参数:`N天`/`Nd`(最近天数)。认得就收,其余忽略;节奏不开口子,全自适应。
fn parse_replay_args(rest: &str) -> replay::ReplayArgs {
    let days = rest.split_whitespace().find_map(|tok| parse_days(&tok.to_lowercase()));
    replay::ReplayArgs { days }
}

/// `7天` / `7d` / `7day` → `Some(7)`;无天数后缀 → `None`。
fn parse_days(low: &str) -> Option<i64> {
    let num = low.trim_end_matches('天').trim_end_matches("days").trim_end_matches("day").trim_end_matches('d');
    if num == low {
        return None; // 没后缀,不当天数
    }
    let n: i64 = num.parse().ok()?;
    (n > 0).then_some(n)
}

/// `画板历史` —— **超管**全权:无坐标=全局最近 1000 笔、`x,y`=该格、`@某人`/`<QQ>`=某人;
/// **非超管**只能查**自己的**记录(任何过滤参数都忽略,也查不了别人)。一律**合并转发**:同一人多笔
/// 用**嵌套合并转发**收拢,每条消息合并多笔(≤3000 字),顶层最多 100 子消息(嵌套转发算 1)。
/// 每笔带**绘制序号 #id**。
#[command(
    "画板历史",
    order = 4,
    description = "看自己在画板上的落格记录",
    usage = "发送「画板历史」，以合并转发列出自己每一笔落格（带绘制序号、坐标、颜色、时间）。\
超管还可加「x,y」查某格被谁画过、加「@某人」或 QQ 号查某人的落格，普通人只能看自己的。"
)]
async fn history(reply: Reply, Db(db): Db, m: MessageEvent, sus: State<Superusers>, args: ArgText) -> HandlerResult {
    let sender = m.sender.0;
    let is_su = (*sus).0.contains(&Uin(sender));
    let rest = args.0.trim();

    let (rows, header) = if !is_su {
        // 非超管:只能查自己,忽略任何过滤参数。
        (logic::person_history(&db, sender, 1000).await?, "你的落格".to_string())
    } else if let Some(u) = m.content.mentions().first().map(|u| u.0).or_else(|| parse_uin(rest)) {
        // 超管 + 指定人(@ 提及优先,其次裸 QQ 号)。
        (logic::person_history(&db, u, 1000).await?, format!("用户 {u} 的落格"))
    } else if rest.is_empty() {
        (logic::recent_history(&db, 1000).await?, "画板最近落格".to_string())
    } else if let Some((x, y)) = parse_xy(rest) {
        if !in_bounds(x, y) {
            reply.reply("坐标超出范围（x 0-255，y 0-143）").await?;
            return Ok(());
        }
        let total = logic::cell_count(&db, x, y).await?;
        (logic::cell_history(&db, x, y, 1000).await?, format!("({x},{y}) 共被画 {total} 次"))
    } else {
        reply.reply("用法：画板历史 / 画板历史 x,y / 画板历史 @某人").await?;
        return Ok(());
    };

    if rows.is_empty() {
        reply.reply(if is_su { "还没有相关落格记录" } else { "你还没画过" }).await?;
        return Ok(());
    }
    reply.send(&history_segments(m.self_id, &header, &rows)).await?;
    Ok(())
}

/// 单个 QQ 号(纯数字、≥10000 视作 uin),用于按人过滤的文本输入(`画板历史 123456`)。
fn parse_uin(s: &str) -> Option<i64> {
    let n: i64 = s.trim().parse().ok()?;
    Uin(n).is_user().then_some(n)
}

/// 单条记录文本:`#绘制序号 (x,y) → 颜色  时间`。
fn fmt_record(r: &entity::history::Model) -> String {
    format!(
        "#{} ({},{}) → {}  {}",
        r.id,
        r.x,
        r.y,
        colors::name(r.new_color.clamp(1, 32) as u8),
        r.at.format("%m-%d %H:%M")
    )
}

/// 把历史按人分组 → 顶层合并转发:概述 + 每人一节(多笔嵌套合并转发、一笔单节点),
/// 顶层最多 100 子消息(含概述,嵌套转发算 1),超出的人截断并在概述里注明。
fn history_segments(me: Uin, header: &str, rows: &[entity::history::Model]) -> Vec<Segment> {
    use std::collections::HashMap;
    const MAX_TOP: usize = 100; // 顶层子消息上限(含概述)
    let cap_people = MAX_TOP - 1;

    // 按人分组,保留首现顺序(rows 已按时间倒序 → 人按最近活跃排序,人内也最近在前)。
    let mut order: Vec<i64> = Vec::new();
    let mut by: HashMap<i64, Vec<&entity::history::Model>> = HashMap::new();
    for r in rows {
        let e = by.entry(r.uin).or_default();
        if e.is_empty() {
            order.push(r.uin);
        }
        e.push(r);
    }

    let total_people = order.len();
    let head = if total_people > cap_people {
        format!("{header}\n{} 笔 · {total_people} 人（仅显示活跃前 {cap_people} 人）", rows.len())
    } else {
        format!("{header}\n{} 笔 · {total_people} 人", rows.len())
    };

    let mut top = vec![ForwardNode::text(me, "画板历史", head)];
    for uin in order.into_iter().take(cap_people) {
        let recs = &by[&uin];
        let user = Uin(uin);
        let content = if recs.len() == 1 {
            vec![Segment::text(fmt_record(recs[0]))]
        } else {
            // 同一人多笔 → 嵌套合并转发：按「一笔记录」为单位切节点(chunk_items,不把一笔拆到两节点)。
            let lines: Vec<String> = recs.iter().map(|&r| fmt_record(r)).collect();
            let title = format!("{uin} · {} 笔", recs.len());
            let nodes = ForwardNode::chunk_items(user, user.0.to_string(), lines, "\n", 3000);
            vec![Segment::Forward(Forward::nodes(nodes).title(title))]
        };
        top.push(ForwardNode::new(user, uin.to_string(), content).at_time(recs[0].at.timestamp()));
    }

    vec![Segment::Forward(Forward::nodes(top).title("画板历史"))]
}

/// `画板作画` —— 快捷 `x,y 颜色` 一步落格;否则进区块导航引导(`#N` 选块 / `x,y` 指定点 / 空→总览选块)。
/// 同人并发经 `single_flight` 串行(防双击)。
#[command(
    "画板作画",
    order = 2,
    description = "在公共画板上落一个格子",
    usage = "发送「画板作画 x,y 颜色」一步落格，坐标如 100,72（x 0-255、y 0-143），颜色取调色板编号或颜色名；\
也可只发「画板作画」按区块导航一步步选位选色，或「画板作画 #N」先进某区块。每落一格按累计落格量缓涨地扣币（1 起、封顶 20）、\
加 2 经验，并进入冷却（2 小时起，等级越高越短、最低 15 分钟）。"
)]
async fn draw(reply: Reply, user: AUser, session: Session, args: ArgText) -> HandlerResult {
    // user 已持同一份连接（内部 Arc）；直接借用,不再单取 Db。
    let db = user.db();
    let uin = user.uin();
    let group_id = (reply.peer().scene == Scene::Group).then(|| reply.peer().id.0);

    // 防双击:占用中直接静默返回。
    let Some(_guard) = session.single_flight_user() else {
        return Ok(());
    };
    let waiter = session.waiter().from(*reply.peer(), Uin(uin)).build();
    let rest = args.0.trim();

    // 快捷:坐标 + 颜色一行,直接落格。
    if let Some((x, y, color)) = parse_quick(rest) {
        return finish_place(&reply, db, &user, group_id, x, y, color).await;
    }

    // 其余都进引导,入口先查冷却(别让人填完才失败)。
    if let Some((remain, interval)) = logic::cooldown_remaining(db, uin, user.level()).await? {
        reply.reply(cooldown_msg(remain, interval, user.level())).await?;
        return Ok(());
    }

    // 决定要画的格子:#N 区块 / x,y 点 / 空→总览选块。
    let target = if let Some(n) = parse_block(rest) {
        pick_coord_in_block(&reply, db, &waiter, n).await?
    } else if let Some((x, y)) = parse_xy(rest) {
        if !in_bounds(x, y) {
            reply.reply("坐标超出范围（x 0-255，y 0-143）").await?;
            return Ok(());
        }
        Some((x, y))
    } else if rest.is_empty() {
        pick_target_overview(&reply, db, &waiter).await?
    } else {
        reply.reply("发「作画 x,y 颜色」一步落格，或只发「作画」按引导来").await?;
        return Ok(());
    };

    let Some((x, y)) = target else {
        return Ok(()); // 取消/超时,已告知用户
    };
    ask_color_and_place(&reply, db, &user, &waiter, group_id, x, y).await
}

/// 在会话里等一条**坐标**(非法由 recv_parse 自动重问;取消/超时回 `None` 并告知)。
async fn recv_coord(reply: &Reply, waiter: &Waiter) -> Result<Option<(i32, i32)>> {
    let got = waiter
        .recv_parse(Duration::from_secs(60), "取消", |txt| match parse_xy(txt) {
            Some((x, y)) if in_bounds(x, y) => Ok((x, y)),
            Some(_) => Err("坐标超出范围（x 0-255，y 0-143），再发一次".to_string()),
            None => Err("坐标格式 x,y，例如 100,72，再发一次（或「取消」）".to_string()),
        })
        .await;
    if got.is_none() {
        reply.reply("已取消").await?;
    }
    Ok(got)
}

/// 在会话里等一个**颜色**(非法由 recv_parse 自动重问;取消/超时回 `None` 并告知)。
async fn recv_color(reply: &Reply, waiter: &Waiter) -> Result<Option<u8>> {
    let got = waiter
        .recv_parse(Duration::from_secs(60), "取消", |txt| {
            colors::parse_color(txt)
                .ok_or_else(|| "没有这个颜色，回编号或名字（发「色板」看可选，或「取消」）".to_string())
        })
        .await;
    if got.is_none() {
        reply.reply("已取消").await?;
    }
    Ok(got)
}

/// 进区块 `n` 放大图 → 等坐标。
async fn pick_coord_in_block(
    reply: &Reply,
    db: &DatabaseConnection,
    waiter: &Waiter,
    n: u8,
) -> Result<Option<(i32, i32)>> {
    let img = render::render_block(db, n).await?;
    reply.msg().image_bytes(img).text(format!("区块 #{n}，回要画的坐标 x,y（「取消」退出）")).send().await?;
    recv_coord(reply, waiter).await
}

/// 发总览(带区块号)→ 等「区块号 #N」或「坐标 x,y」;选了区块再进区块图等坐标。
async fn pick_target_overview(reply: &Reply, db: &DatabaseConnection, waiter: &Waiter) -> Result<Option<(i32, i32)>> {
    let img = render::render_overview(db).await?;
    reply
        .msg()
        .image_bytes(img)
        .text(format!("回区块号 #N（1-{}），或直接回坐标 x,y（「取消」退出）", render::BLOCKS))
        .send()
        .await?;
    // 区块号 #N 或坐标 x,y,二选一;非法由 recv_parse 自动重问。
    enum Pick {
        Block(u8),
        Coord(i32, i32),
    }
    let got = waiter
        .recv_parse(Duration::from_secs(60), "取消", |txt| {
            if let Some(n) = parse_block(txt) {
                return Ok(Pick::Block(n));
            }
            match parse_xy(txt) {
                Some((x, y)) if in_bounds(x, y) => Ok(Pick::Coord(x, y)),
                Some(_) => Err("坐标超出范围，或回区块号 #N，再发一次".to_string()),
                None => Err(format!("回区块号 #N（1-{}）或坐标 x,y，再发一次（或「取消」）", render::BLOCKS)),
            }
        })
        .await;
    match got {
        Some(Pick::Block(n)) => pick_coord_in_block(reply, db, waiter, n).await,
        Some(Pick::Coord(x, y)) => Ok(Some((x, y))),
        None => {
            reply.reply("已取消").await?;
            Ok(None)
        }
    }
}

/// 已知坐标 → 发该点放大窗 + 色板,等颜色 → 落格。
async fn ask_color_and_place(
    reply: &Reply,
    db: &DatabaseConnection,
    user: &AUser,
    waiter: &Waiter,
    group_id: Option<i64>,
    x: i32,
    y: i32,
) -> HandlerResult {
    let zoom = render::render_zoom(db, x, y).await?;
    let pal = render::render_palette()?;
    reply
        .msg()
        .image_bytes(zoom)
        .image_bytes(pal)
        .text(format!("({x},{y}) 要什么颜色？回编号或名字（「取消」退出）\n{}", colors::legend()))
        .send()
        .await?;
    let Some(color) = recv_color(reply, waiter).await? else {
        return Ok(());
    };
    finish_place(reply, db, user, group_id, x, y, color).await
}

/// 落格 + 按结果回文案。成功附该处放大窗。
async fn finish_place(
    reply: &Reply,
    db: &DatabaseConnection,
    user: &AUser,
    group_id: Option<i64>,
    x: i32,
    y: i32,
    color: u8,
) -> HandlerResult {
    match logic::try_place(user, db, group_id, x, y, color).await? {
        PlaceResult::Placed { cost, balance } => {
            let line =
                format!("已落格 ({x},{y}) {}，花费 {cost} {COIN_NAME}，+2 经验，余额 {balance}", colors::name(color));
            let mut msg = reply.msg().text(line);
            if let Ok(zoom) = render::render_zoom(db, x, y).await {
                msg = msg.image_bytes(zoom);
            }
            msg.send().await?;
        }
        PlaceResult::Cooldown { remain_min, interval_min } => {
            reply.reply(cooldown_msg(remain_min, interval_min, user.level())).await?;
        }
        PlaceResult::Poor { cost, have } => {
            reply.reply(format!("余额不足，这格要 {cost} {COIN_NAME}，你只有 {have}")).await?;
        }
        PlaceResult::Same => {
            reply.reply(format!("这格已经是{}了", colors::name(color))).await?;
        }
        PlaceResult::OutOfRange => {
            reply.reply("坐标超出范围（x 0-255，y 0-143）").await?;
        }
    }
    Ok(())
}
