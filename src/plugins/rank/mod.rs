//! 排行榜插件 —— 命令 `查看排行榜`(总览)与各单榜(`查看游戏币榜` / `查看等级榜` / `查看发言榜` /
//! `查看签到榜`)。
//!
//! 命令-only(无自有表):各榜的数据由核心与各插件经 [`RankBoard`]
//! **自注册**贡献,本插件经 [`collect_boards`] 统一收集——故本插件**不**
//! 引用任何具体业务插件、各插件的自有数据也各管各的。新增榜:加一个 `RankBoard` impl(自动进总览)+
//! 在 `ViewRank` 的 `board` union 加一个词 + 下面分派加一臂(给单榜直达词,help 自动枚举)。
//!
//! # 作用域
//!
//! 所有榜按**全局数值**排。群里发 → 本群榜(经 `get_group_member_list` 圈本群成员 + 取群名片);
//! 私聊发 → 全局榜;群里跟「全局」二字也看全站。拿不到群名册时退回全局。
//!
//! # 只列建号用户
//!
//! 榜上**只展示有核心 `user` 行的人**——即用过 bot(`AUser` 提取器首次见面即建行,签到 / 个人数据 /
//! 转账 / 查榜等都算)的用户,纯潜水/只在群里说话、bot 只记了消息却没互动过的人不上榜。游戏币 /
//! 等级榜数据源即 `user` 表,天然如此;签到榜的数据(`sign_log`)也只可能来自建过号的人;发言榜数据源
//! 是 `chat_log`(混入大量未建号的纯发言者),故在那一侧显式按 `user` 表过滤(见 `chatlog::rank`)。
//! 这样每行必有 UID,也不会出现一排只有 QQ 号、没名没号的陌生人。
//!
//! # 名字三列
//!
//! 每行出 UID · QQ(前三后三打码)· 昵称。昵称按**统一**优先级解析:**自设昵称**([`AUser::alias`])>
//! **群名片**(群内榜:实时名册 → [`member_card`] 缓存;全局榜:最近一次名片)> **账号昵称**(QQ 昵称,
//! [`identity`] 表)> 兜底 `—`。三者各自独立存档(alias 在 `user`、群名片 per-(uin, gid) 在
//! `member_card`、账号昵称 per-uin 在 `identity`),群名片 / 账号昵称由 `data::identity` 每条消息同步;
//! 上榜者都建过号、也都发过消息,故通常都有名。

pub mod render;

use std::collections::{HashMap, HashSet};

use nagisa::prelude::*;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect};

use crate::data::AUser;
use crate::data::entity::{identity, member_card, user};
use crate::data::rank::{RankBoard, RankRow, collect_boards};
use crate::imaging::qq_avatar;
use render::NameCell;

plugin! {
    key = "rank",
    name = "排行榜",
    category = Tool,
    description = "游戏币、等级、发言、签到的排行榜，群里看本群、私聊看全站。",
}

/// 总览里各榜的展示顺序(键见各 `RankBoard::key`)。未列入的(将来新增)按注册顺序排在其后。
const BOARD_ORDER: &[&str] = &["coin", "level", "chat", "sign"];

/// 单榜 top-N 的 N。
const TOP_N: usize = 10;
/// 总览里每个榜出前几名。
const OVERVIEW_TOP: usize = 3;

/// 总览第一遍每个榜的中间结果:榜序号(`ordered_boards` 内)+ 前几名 + 我的名次。
type OverviewRaw = (usize, Vec<RankRow>, Option<(u32, i64)>);

/// 本次作用域:标签、群号(全局为 `None`)、参与人员集合(全局为 `None`)、群名片(实时名册)。
struct Scope {
    label: &'static str,
    gid: Option<i64>,
    members: Option<HashSet<i64>>,
    roster: HashMap<i64, String>,
}

/// 统一命令 `查看X榜` 的头匹配器 —— 声明式**区块序列**:固定块「查看」+ 榜名块(union)+ 固定块
/// 「榜」+ 可选作用域块(union)。`board` 决定看哪个榜、`scope` 有值即看全站。派生即自动:建匹配
/// 正则、套 `no_args`(`查看游戏币榜单` 不误触发)、生成 `command_words()`(查看排行榜 / 查看游戏币榜 …)
/// 供 help 枚举。新增榜:`board` 的 union 加一个词 + 下面分派加一臂(词须与分派臂、对应榜的 `title` 一致)。
#[derive(Slots)]
#[slots(lit("查看"), board, lit("榜"), scope)]
struct ViewRank {
    /// 榜名(选择参数):排行(总览)/ 游戏币 / 等级(别名经验)/ 发言 / 签到。
    #[slot(union = ["排行", "游戏币", "等级", "经验", "发言", "签到"], name = "board", desc = "看哪个榜(排行=总览,其余看单榜)")]
    board: String,
    /// 作用域(可选开关):跟「全局 / 全站」即看全站。
    #[slot(union = ["全局", "全站"], name = "全局", desc = "看全站;不填则群里看本群、私聊看全站")]
    scope: Option<String>,
}

/// `查看X榜` —— 一个命令、声明式槽序列;按 `board` 分派到总览或某个单榜,`scope` 切全站。
#[command(
    slots = ViewRank,
    description = "看排行榜(总览或单榜)",
    usage = "「查看排行榜」看各榜总览，「查看X榜」看单个榜（X 见上方参数）；群里看本群成员、私聊看全站，末尾加「全局」在群里也看全站。"
)]
async fn view_rank(reply: Reply, user: AUser, m: MessageEvent, bot: Bot, q: Slots<ViewRank>) -> HandlerResult {
    let global = q.0.scope.is_some();
    match q.0.board.as_str() {
        "排行" => show_overview(reply, user, &m, &bot, global).await,
        "游戏币" => show_board(reply, user, &m, &bot, "coin", global).await,
        "等级" | "经验" => show_board(reply, user, &m, &bot, "level", global).await,
        "发言" => show_board(reply, user, &m, &bot, "chat", global).await,
        "签到" => show_board(reply, user, &m, &bot, "sign", global).await,
        _ => Ok(()), // union 已限定,不会到这
    }
}

/// 按 [`BOARD_ORDER`] 排好的全部已注册榜(未列入的排在其后,保注册顺序)。
fn ordered_boards() -> Vec<Box<dyn RankBoard>> {
    let mut boards = collect_boards();
    boards.sort_by_key(|b| BOARD_ORDER.iter().position(|&k| k == b.key()).unwrap_or(usize::MAX));
    boards
}

/// 解析作用域(标签 + 群号 + 人员集合 + 群名片)。私聊 / 群里加「全局」→ 全局;群里默认取群名册
/// (成员 uin 集合 + 群名片);名册拿不到则退回全局。
async fn resolve_scope(bot: &Bot, m: &MessageEvent, global: bool) -> Scope {
    let global_scope = Scope { label: "全局", gid: None, members: None, roster: HashMap::new() };
    // 判群 + 取群号一律走 `peer`(`m.group` 是完整群实体,OneBot 群消息上为 `None`,不能用来判群)。
    if global || !m.peer.is_group() {
        return global_scope;
    }
    let gid = m.peer.id;
    match bot.get_group_member_list(gid, false).await {
        Ok(list) if !list.is_empty() => {
            let mut set = HashSet::with_capacity(list.len() + 1);
            let mut roster = HashMap::with_capacity(list.len());
            for mi in list {
                let uin = mi.user.0;
                set.insert(uin);
                let name = if mi.card.trim().is_empty() { mi.nickname } else { mi.card };
                let name = name.trim();
                if !name.is_empty() {
                    roster.insert(uin, name.to_string());
                }
            }
            set.insert(m.sender.0); // 发命令者必在本群,补上保自己名次算得对
            Scope { label: "本群", gid: Some(gid.0), members: Some(set), roster }
        }
        Ok(_) => {
            tracing::warn!(group = gid.0, "群成员名册为空,本群榜退回全局");
            global_scope
        }
        Err(e) => {
            tracing::warn!(group = gid.0, error = %e, "拉群成员名册失败,本群榜退回全局");
            global_scope
        }
    }
}

/// QQ 号前三后三打码(`294***755`);≤6 位的短号不打码。
fn mask_qq(uin: i64) -> String {
    let s = uin.to_string();
    if s.len() <= 6 { s } else { format!("{}***{}", &s[..3], &s[s.len() - 3..]) }
}

/// 非空去白返 `Some(String)`,空 / 全白返 `None`。
fn non_blank(s: &str) -> Option<String> {
    let t = s.trim();
    if t.is_empty() { None } else { Some(t.to_string()) }
}

/// 批量解析一组 uin 的身份三列(UID · 打码 QQ · 显示名)。显示名优先级(见 [`pick_name`]):
/// 自设昵称 > 群名片 > 账号昵称 > 兜底 `—`。本群口径按本群取名片(实时名册 → member_card 缓存)、
/// 全局口径取该人最近一次群名片;账号昵称取自 [`identity`] 表。
async fn resolve_cells(db: &sea_orm::DatabaseConnection, uins: &[i64], scope: &Scope) -> HashMap<i64, NameCell> {
    let mut out: HashMap<i64, NameCell> = HashMap::new();
    let uniq: Vec<i64> = {
        let mut seen = HashSet::new();
        uins.iter().copied().filter(|u| seen.insert(*u)).collect()
    };
    if uniq.is_empty() {
        return out;
    }

    // 核心 user:UID / 自设昵称 + 其颜色(账号昵称已拆到 identity 表,单独查)。
    let mut urows: HashMap<i64, (i64, String, String)> = HashMap::new();
    match user::Entity::find()
        .select_only()
        .column(user::Column::Uin)
        .column(user::Column::Id)
        .column(user::Column::Alias)
        .column(user::Column::AliasColor)
        .filter(user::Column::Uin.is_in(uniq.iter().copied()))
        .into_tuple::<(i64, i64, String, String)>()
        .all(db)
        .await
    {
        Ok(rows) => {
            for (u, id, alias, color) in rows {
                urows.insert(u, (id, alias, color));
            }
        }
        Err(e) => tracing::warn!(error = %e, "查 user 身份失败"),
    }

    // 账号昵称:identity 表(每条消息给所有发送者缓存),全局榜兜底名 / 群名片缺时回退都用它。
    let mut idents: HashMap<i64, String> = HashMap::new();
    match identity::Entity::find()
        .select_only()
        .column(identity::Column::Uin)
        .column(identity::Column::Nickname)
        .filter(identity::Column::Uin.is_in(uniq.iter().copied()))
        .into_tuple::<(i64, String)>()
        .all(db)
        .await
    {
        Ok(rows) => {
            for (u, nick) in rows {
                idents.insert(u, nick);
            }
        }
        Err(e) => tracing::warn!(error = %e, "查账号昵称失败"),
    }

    // 群名片:本群口径取本群一格;全局口径取每人最近一次名片(updated_at 降序、首现即最近)。
    let mut cards: HashMap<i64, String> = HashMap::new();
    let base =
        member_card::Entity::find().select_only().column(member_card::Column::Uin).column(member_card::Column::Card);
    let res = match scope.gid {
        Some(g) => {
            base.filter(member_card::Column::Gid.eq(g))
                .filter(member_card::Column::Uin.is_in(uniq.iter().copied()))
                .into_tuple::<(i64, String)>()
                .all(db)
                .await
        }
        None => {
            base.filter(member_card::Column::Uin.is_in(uniq.iter().copied()))
                .order_by_desc(member_card::Column::UpdatedAt)
                .into_tuple::<(i64, String)>()
                .all(db)
                .await
        }
    };
    match res {
        Ok(rows) => {
            for (u, c) in rows {
                cards.entry(u).or_insert(c); // 全局口径首现 = 最近
            }
        }
        Err(e) => tracing::warn!(error = %e, "查群名片失败"),
    }

    for u in uniq {
        let (uid, alias, alias_color) = match urows.get(&u) {
            Some((id, alias, color)) => (Some(*id), alias.as_str(), color.as_str()),
            None => (None, "", ""),
        };
        let (name, color) = pick_name(alias, alias_color, scope, u, &cards, idents.get(&u).map(String::as_str));
        out.insert(u, NameCell { uid, qq: mask_qq(u), name, color });
    }
    out
}

/// 按**统一**优先级挑显示名 + 自设颜色:自设昵称(带其颜色)> 群名片 > 账号昵称 > 兜底 `—`。
/// 自设昵称(用户经「改名」自定)最优先,且只有它带自设颜色(`#rrggbb` 原始色相,出图经
/// [`imaging::readable_hex`](crate::imaging::readable_hex) 收对比;退到名片 / 昵称则返空串=缺省色);
/// 群名片次之、账号昵称(QQ 昵称)垫底——群名片更贴近「这个圈子里大家怎么称呼他」,故压过全局账号
/// 昵称。群名片来源随作用域:群内先取实时名册(最新)、再取本群缓存名片;全局取该人最近一次名片(无名册)。
fn pick_name(
    alias: &str,
    alias_color: &str,
    scope: &Scope,
    uin: i64,
    cards: &HashMap<i64, String>,
    nickname: Option<&str>,
) -> (String, String) {
    if let Some(a) = non_blank(alias) {
        return (a, alias_color.to_string());
    }
    // 群名片优先于账号昵称。群内先用实时名册(最新),全局无名册、直接落缓存名片。
    if scope.gid.is_some()
        && let Some(c) = scope.roster.get(&uin).and_then(|s| non_blank(s))
    {
        return (c, String::new());
    }
    if let Some(c) = cards.get(&uin).and_then(|s| non_blank(s)) {
        return (c, String::new());
    }
    if let Some(n) = nickname.and_then(non_blank) {
        return (n, String::new());
    }
    ("—".to_string(), String::new())
}

/// 并发拉前 3 名的圆头像(其余不拉)。返回与 `top` 前缀对齐的 `Option` 序列。
async fn fetch_avatars(top: &[RankRow]) -> Vec<Option<Vec<u8>>> {
    futures::future::join_all(top.iter().take(OVERVIEW_TOP).map(|r| qq_avatar(r.uin))).await
}

/// 总览:各榜前几名 + 我的名次,渲一张仪表盘卡(渲不出退文字)。
async fn show_overview(reply: Reply, user: AUser, m: &MessageEvent, bot: &Bot, global: bool) -> HandlerResult {
    let scope = resolve_scope(bot, m, global).await;
    let db = user.db();
    let boards = ordered_boards();

    let mut raw: Vec<OverviewRaw> = Vec::with_capacity(boards.len());
    let mut uins: Vec<i64> = Vec::new();
    for (idx, board) in boards.iter().enumerate() {
        let top = board.top(db, scope.members.as_ref(), OVERVIEW_TOP).await;
        let mine = board.rank_of(db, scope.members.as_ref(), user.uin()).await;
        uins.extend(top.iter().map(|r| r.uin));
        raw.push((idx, top, mine));
    }
    let cells = resolve_cells(db, &uins, &scope).await;

    let boards_data = raw
        .into_iter()
        .map(|(idx, top, mine)| {
            let board = &boards[idx];
            render::OverviewBoard {
                title: board.title(),
                top: top
                    .iter()
                    .enumerate()
                    .map(|(i, r)| render::OverviewEntry {
                        rank: (i + 1) as u32,
                        cell: take_cell(&cells, r.uin),
                        value_text: board.format_value(r.value),
                    })
                    .collect(),
                mine: mine.map(|(rank, value)| (rank, board.format_value(value))),
            }
        })
        .collect();

    let card = render::OverviewCard { scope_label: scope.label, boards: boards_data, theme: user.render_theme() };
    match render::overview_image(&card) {
        Ok(webp) => {
            reply.msg().image_bytes(webp).quote().await?;
        }
        Err(e) => {
            tracing::warn!(error = %e, "渲染排行榜总览失败,退回文字");
            reply.reply(render::overview_text(&card)).await?;
        }
    }
    Ok(())
}

/// 单榜:完整 top-N + 我的名次,渲一张榜卡(渲不出退文字)。
async fn show_board(reply: Reply, user: AUser, m: &MessageEvent, bot: &Bot, key: &str, global: bool) -> HandlerResult {
    let boards = collect_boards();
    let Some(board) = boards.iter().find(|b| b.key() == key) else {
        reply.reply("这个榜还没有").await?;
        return Ok(());
    };

    let scope = resolve_scope(bot, m, global).await;
    let db = user.db();

    let top = board.top(db, scope.members.as_ref(), TOP_N).await;
    let mine = board.rank_of(db, scope.members.as_ref(), user.uin()).await;

    let mut uins: Vec<i64> = top.iter().map(|r| r.uin).collect();
    uins.push(user.uin());
    let cells = resolve_cells(db, &uins, &scope).await;

    let avatars = fetch_avatars(&top).await;
    let in_top = top.iter().any(|r| r.uin == user.uin());

    let rows = top
        .iter()
        .enumerate()
        .map(|(i, r)| render::BoardEntry {
            rank: (i + 1) as u32,
            cell: take_cell(&cells, r.uin),
            value_text: board.format_value(r.value),
            avatar: avatars.get(i).cloned().flatten(),
            is_me: r.uin == user.uin(),
        })
        .collect();

    let mine = mine.map(|(rank, value)| render::MyStanding {
        rank,
        cell: take_cell(&cells, user.uin()),
        value_text: board.format_value(value),
        in_top,
    });

    let card =
        render::BoardCard { title: board.title(), scope_label: scope.label, rows, mine, theme: user.render_theme() };
    match render::board_image(&card) {
        Ok(webp) => {
            reply.msg().image_bytes(webp).quote().await?;
        }
        Err(e) => {
            tracing::warn!(error = %e, "渲染排行榜单榜失败,退回文字");
            reply.reply(render::board_text(&card)).await?;
        }
    }
    Ok(())
}

/// 从已解析的身份表取一行(缺失兜底:UID 未知、QQ 打码、名 `—`、无色)。
fn take_cell(cells: &HashMap<i64, NameCell>, uin: i64) -> NameCell {
    match cells.get(&uin) {
        Some(c) => NameCell { uid: c.uid, qq: c.qq.clone(), name: c.name.clone(), color: c.color.clone() },
        None => NameCell { uid: None, qq: mask_qq(uin), name: "—".to_string(), color: String::new() },
    }
}
