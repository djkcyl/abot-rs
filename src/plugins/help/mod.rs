//! 帮助插件 —— 命令 `help` / `帮助` / `菜单`，**自动**从插件注册表生成菜单与每条命令的用法。
//!
//! 不手维护任何清单:`nagisa::registered_plugins()` / `registered_triggers()` 由 inventory 在编译期
//! 收集,本命令据此分组、跳过 `hidden` 的后台插件、展开每个插件下的命令与别名。新插件/新命令加进来
//! 即自动出现,无需改本文件。
//!
//! - `help`(空参):按 `CATEGORY_ORDER` 分组渲成菜单卡片图([`render`],按用户出图主题),
//!   每个插件一行「序号 / 名字 / 简介 / 启停」;渲不出退文字总览(信息同卡片,合并转发)。
//! - `help 序号` / `help 功能名` / `help 命令`:先按菜单序号命中(与总览同一份顺序),再按
//!   命令词/命令名/命令 id(命中则展开它所属的整个插件),最后按插件名/插件 key;都没中就回
//!   一句「没找到」。详情也渲卡片图(每条命令一段:序号·主词·别名·启停、简介、用法、参数、
//!   备注),渲不出退文字合并转发(信息同卡片)。
//!
//! 正文构造做成纯函数 `overview_groups` / `detail_card`(卡片与文字退路同源),handler 只负责
//! 出图 / 切节点发送,便于在不发 QQ 消息的前提下核对输出。

pub mod render;

use nagisa::prelude::*;

use crate::data::AUser;

plugin! {
    key = "help",
    name = "帮助",
    category = Tool,
    description = "功能菜单，按序号或名字查详细命令和用法。",
}

/// 分类的展示顺序 + 中文名(注册表里没有的分类自动跳过)。
const CATEGORY_ORDER: &[(Category, &str)] = &[
    (Category::Fun, "娱乐"),
    (Category::Tool, "工具"),
    (Category::User, "用户"),
    (Category::Admin, "管理"),
    (Category::Push, "推送"),
    (Category::Core, "核心"),
];

/// 合并转发单节点字数上限(约 3000 字),正文超出则切多个节点。
const NODE_MAX_CHARS: usize = 3000;

/// `help` 的参数:尾随的目标串(菜单序号、功能名或命令词),保真收尾后由 handler 去空白。
#[derive(Args)]
struct HelpArgs {
    /// 要查看的菜单序号、功能名或命令词;缺则给出总览。
    #[arg(rest, raw, name = "功能", desc = "菜单序号、功能名或命令词；不填则列出全部")]
    text: String,
}

/// `help` / `帮助` / `菜单` —— 空参给菜单卡片图,带参给某个功能的详细命令卡;渲不出都退
/// 文字合并转发。
#[command(
    "help",
    "帮助",
    "菜单",
    description = "查看命令菜单与用法",
    usage = "例如「help 漂流瓶」或「help 1」看对应功能的全部命令。"
)]
async fn help(
    reply: Reply,
    user: AUser,
    m: MessageEvent,
    State(enabled): State<std::sync::Arc<EnabledSet>>,
    args: Args<HelpArgs>,
) -> HandlerResult {
    let me = m.self_id; // 合并转发各节点的发送者署名为 bot 自己
    // 按当前会话判断开关:per-peer 覆盖优先、回退全局,故 help 在哪发就反映哪儿的启用情况。
    let peer = Some(m.peer);
    let target = args.0.text.trim();
    let plugins = registered_plugins();
    let triggers = registered_triggers();

    if target.is_empty() {
        let groups = overview_groups(&plugins, &triggers, &enabled, peer);
        match render::menu_image(&groups, &user.render_theme()) {
            Ok(webp) => {
                // 回复触发的原消息(quote)。
                reply.msg().image_bytes(webp).quote().await?;
            }
            Err(e) => {
                tracing::warn!(error = %e, "渲染菜单卡片失败,退回文字");
                let nodes = ForwardNode::chunk_text(me, "命令菜单", render_overview(&groups), NODE_MAX_CHARS);
                reply.send(&[Segment::Forward(Forward::nodes(nodes).title("命令菜单"))]).await?;
            }
        }
        return Ok(());
    }

    // 详情:命中插件后取同一份素材,渲卡片图;渲不出退文字(按「一条命令」为单位切节点,
    // 命令块各自完整,绝不会被拆到两个节点)。
    let Some(plugin) = resolve_plugin(target, &plugins, &triggers) else {
        reply.reply("没找到这个功能，发送 help 看全部功能。").await?;
        return Ok(());
    };
    let card = detail_card(plugin, &triggers, &enabled, peer);
    match render::detail_image(&card, &user.render_theme()) {
        Ok(webp) => {
            reply.msg().image_bytes(webp).quote().await?;
        }
        Err(e) => {
            tracing::warn!(error = %e, "渲染功能详情卡片失败,退回文字");
            let title = format!("{} · 用法", card.name);
            let nodes = ForwardNode::chunk_items(me, title.clone(), detail_text_blocks(&card), "\n\n", NODE_MAX_CHARS);
            reply.send(&[Segment::Forward(Forward::nodes(nodes).title(title))]).await?;
        }
    }
    Ok(())
}

/// 插件的「有效 key」:显式写了 `key=` 就用它,否则退回 `module_path` 的最后一段。
///
/// 解析触发器时框架已把 `plugin_key` 填成有效 key,这里给插件侧算同样的口径,二者才能对上。
fn effective_key(p: &PluginMeta) -> &str {
    if p.key.is_empty() { p.module_path.rsplit("::").next().unwrap_or(p.module_path) } else { p.key }
}

/// 插件在总览/详情头部用的简介:有 `plugin.description` 就用它;没有(单命令插件不写插件级描述)则
/// 取它**唯一**一条非隐藏命令的描述兜底——元数据只写在命令上、不和插件级重复。
fn plugin_desc<'a>(p: &'a PluginMeta, triggers: &'a [TriggerMeta]) -> &'a str {
    if !p.description.is_empty() {
        return p.description;
    }
    let key = effective_key(p);
    let mut it = triggers.iter().filter(|t| matches!(t.kind, TriggerKind::Command) && !t.hidden && t.plugin_key == key);
    match (it.next(), it.next()) {
        (Some(only), None) => only.description, // 恰好一条命令 → 用它的描述
        _ => "",
    }
}

/// 某个分类下要进菜单的插件,组内按名字排序(确定性顺序,总览编号与「help 序号」都从
/// 这份顺序出,二者永远对得上)。
fn category_plugins(plugins: &[PluginMeta], cat: Category) -> Vec<&PluginMeta> {
    let mut ps: Vec<&PluginMeta> = plugins.iter().filter(|p| !p.hidden && p.category == cat).collect();
    ps.sort_by_key(|p| p.name);
    ps
}

/// 菜单展示顺序拉平的全部插件(分类顺序 + 组内名字排序)。第 i 项即菜单序号 i+1,
/// 「help 序号」按它解析——新插件加进来序号会重排,但卡片与解析始终同一份顺序。
fn ordered_plugins(plugins: &[PluginMeta]) -> Vec<&PluginMeta> {
    CATEGORY_ORDER.iter().flat_map(|(cat, _)| category_plugins(plugins, *cat)).collect()
}

/// 总览的分组数据:按 [`CATEGORY_ORDER`] 分组,行带全菜单统一编号(简介经 [`plugin_desc`]
/// 兜底,停用态按会话算好)。卡片图与文字退路共用这一份。
pub(crate) fn overview_groups(
    plugins: &[PluginMeta],
    triggers: &[TriggerMeta],
    enabled: &EnabledSet,
    peer: Option<Peer>,
) -> Vec<(&'static str, Vec<render::MenuRow>)> {
    let mut groups = Vec::new();
    let mut idx = 0;
    for (cat, label) in CATEGORY_ORDER {
        let ps = category_plugins(plugins, *cat);
        if ps.is_empty() {
            continue;
        }
        let rows = ps
            .into_iter()
            .map(|p| {
                idx += 1;
                render::MenuRow {
                    idx,
                    name: p.name.to_string(),
                    desc: plugin_desc(p, triggers).to_string(),
                    off: plugin_off(p, enabled, peer),
                }
            })
            .collect();
        groups.push((*label, rows));
    }
    groups
}

/// 总览的文字退路(渲染失败时用,信息同卡片):每组「【分类】」+ 每行「序号. 名字 —— 简介」,
/// 停用的名后标「（已停用）」;末尾给查详情提示。
pub(crate) fn render_overview(groups: &[(&str, Vec<render::MenuRow>)]) -> String {
    let mut sections = Vec::new();
    for (label, rows) in groups {
        let mut lines = vec![format!("【{label}】")];
        for r in rows {
            let name = name_with_off(&r.name, r.off);
            if r.desc.is_empty() {
                lines.push(format!("{}. {name}", r.idx));
            } else {
                lines.push(format!("{}. {name} —— {}", r.idx, r.desc));
            }
        }
        sections.push(lines.join("\n"));
    }

    let mut body = if sections.is_empty() { "暂无可用命令".to_string() } else { sections.join("\n\n") };
    body.push_str("\n\n发送「help 序号或功能名」看某个功能的详细命令，例如「help 1」「help 漂流瓶」。");
    body
}

/// 把 `target` 解析到要展开的插件,没命中返回 `None`。
///
/// 命中优先级:纯数字先按菜单序号(与总览同一份顺序);再按命令命中(非隐藏的命令触发器,
/// 命令词 / 命令名 / 命令 id 任一对上)→ 展开它所属的整个插件;最后按插件名 / 插件有效 key。
/// `target` 应已去空白。
fn resolve_plugin<'a>(target: &str, plugins: &'a [PluginMeta], triggers: &[TriggerMeta]) -> Option<&'a PluginMeta> {
    // (a) 菜单序号命中(超界即没找到)。
    if let Ok(n) = target.parse::<usize>() {
        return n.checked_sub(1).and_then(|i| ordered_plugins(plugins).get(i).copied());
    }
    // (b) 命令命中:其所属插件(经 `plugin_key` 对上有效 key)。
    let cmd_hit = triggers.iter().find(|t| {
        matches!(t.kind, TriggerKind::Command)
            && !t.hidden
            && (t.words.contains(&target) || t.name == target || t.id == target)
    });
    if let Some(t) = cmd_hit {
        return plugins.iter().find(|p| !p.hidden && effective_key(p) == t.plugin_key);
    }
    // (c) 插件命中:按名字或有效 key。
    plugins.iter().find(|p| !p.hidden && (p.name == target || effective_key(p) == target))
}

/// 一个插件的详情素材:头部(名字 / 简介 / 停用态)+ 每条命令(按 order 排,小在前,稳定排序
/// 故并列保持注册序;用法 synopsis 与参数说明自动生成)。卡片图与文字退路共用这一份。
pub(crate) fn detail_card(
    plugin: &PluginMeta,
    triggers: &[TriggerMeta],
    enabled: &EnabledSet,
    peer: Option<Peer>,
) -> render::DetailCard {
    let key = effective_key(plugin);

    let mut cmds: Vec<&TriggerMeta> = triggers
        .iter()
        .filter(|t| matches!(t.kind, TriggerKind::Command) && !t.hidden && t.plugin_key == key)
        .collect();
    cmds.sort_by_key(|t| t.order);

    let cmds = cmds
        .into_iter()
        .enumerate()
        .map(|(i, t)| {
            let on = enabled.is_enabled_keyed(
                key,
                t.key,
                plugin.default_enable,
                plugin.can_disable,
                t.default_enable,
                t.can_disable,
                peer,
            );
            let primary = t.words.first().copied().unwrap_or(t.name);
            // 槽序列命令带显式用法模板(`查看<游戏币|发言>榜[全局]`):用它当用法行,并隐藏「别名」——
            // 那些词(查看游戏币榜…)是参数取值不是真别名。其余命令照旧:`主词 + 参数` 自动 synopsis。
            let has_synopsis = !t.synopsis.is_empty();
            render::CmdInfo {
                idx: i + 1,
                primary: primary.to_string(),
                aliases: if has_synopsis {
                    Vec::new()
                } else {
                    t.words.get(1..).unwrap_or(&[]).iter().map(|w| w.to_string()).collect()
                },
                off: !on,
                desc: t.description.to_string(),
                synopsis: if has_synopsis { t.synopsis.to_string() } else { synopsis(primary, t.args) },
                params: t.args.iter().map(|a| (param_head(a), a.desc.to_string())).collect(),
                note: t.usage.to_string(),
            }
        })
        .collect();

    render::DetailCard {
        name: plugin.name.to_string(),
        desc: plugin_desc(plugin, triggers).to_string(),
        off: plugin_off(plugin, enabled, peer),
        cmds,
    }
}

/// 详情的文字退路(渲染失败时用,信息同卡片):头部块(名字 + 简介)+ 每条命令一块
/// (`▸ 序号. 主词（别名：…）`、简介、用法、逐参数、备注)。由调用方用 `chunk_items`
/// 按块切节点,保证不拆散一条命令。
pub(crate) fn detail_text_blocks(card: &render::DetailCard) -> Vec<String> {
    let name = name_with_off(&card.name, card.off);
    let head = if card.desc.is_empty() { format!("【{name}】") } else { format!("【{name}】{}", card.desc) };

    let mut blocks = vec![head];
    for c in &card.cmds {
        let mut header = format!("▸ {}. {}", c.idx, c.primary);
        if !c.aliases.is_empty() {
            header.push_str(&format!("（别名：{}）", c.aliases.join("、")));
        }
        // 被停用的命令标注一下(不隐藏)。
        if c.off {
            header.push_str(" 〔已停用〕");
        }

        let mut lines = vec![header];
        if !c.desc.is_empty() {
            lines.push(format!("  {}", c.desc));
        }
        lines.push(format!("  用法：{}", c.synopsis));
        for (head, desc) in &c.params {
            if desc.is_empty() {
                lines.push(format!("  · {head}"));
            } else {
                lines.push(format!("  · {head}：{desc}"));
            }
        }
        if !c.note.is_empty() {
            lines.push(format!("  备注：{}", c.note));
        }
        blocks.push(lines.join("\n"));
    }
    blocks
}

/// 插件是否被整体停用(可停用 + 其总开关在该会话为关)。
fn plugin_off(p: &PluginMeta, enabled: &EnabledSet, peer: Option<Peer>) -> bool {
    p.can_disable && !enabled.is_enabled(effective_key(p), p.default_enable, peer)
}

/// 名字按需缀「（已停用）」。
fn name_with_off(name: &str, off: bool) -> String {
    if off { format!("{name}（已停用）") } else { name.to_string() }
}

/// 用法 synopsis：`主词 [-a] [-r 次数] <编号> [内容]`。
fn synopsis(primary: &str, args: &[ArgSpec]) -> String {
    let mut s = primary.to_string();
    for a in args {
        s.push(' ');
        s.push_str(&syn_token(a));
    }
    s
}

/// 单个参数在 synopsis 里的记号：旗标 `[-a]`、选项 `[-r 名]`、必填位置 `<名>`、可选/rest `[名]`。
fn syn_token(a: &ArgSpec) -> String {
    match a.kind {
        ArgKind::Flag => format!("[{}]", flag_short_or_long(a)),
        ArgKind::Opt => {
            let body = format!("{} {}", flag_short_or_long(a), a.name);
            if a.required { body } else { format!("[{body}]") }
        }
        ArgKind::Rest => format!("[{}]", a.name),
        // 位置 / at_or_id / 元素：必填 `<名>`、可选 `[名]`。
        _ => {
            if a.required {
                format!("<{}>", a.name)
            } else {
                format!("[{}]", a.name)
            }
        }
    }
}

/// 旗标在 synopsis 里的短名优先记号：有短名用 `-a`，否则 `--long`，再否则显示名。
fn flag_short_or_long(a: &ArgSpec) -> String {
    if !a.short.is_empty() {
        format!("-{}", a.short)
    } else if !a.long.is_empty() {
        format!("--{}", a.long)
    } else {
        a.name.to_string()
    }
}

/// 参数说明行的前缀：旗标/选项给 `-a, --anonymous`（选项再带显示名），位置参给显示名。
fn param_head(a: &ArgSpec) -> String {
    let flags = match (a.short.is_empty(), a.long.is_empty()) {
        (false, false) => format!("-{}, --{}", a.short, a.long),
        (false, true) => format!("-{}", a.short),
        (true, false) => format!("--{}", a.long),
        (true, true) => String::new(),
    };
    match a.kind {
        ArgKind::Flag => {
            if flags.is_empty() {
                a.name.to_string()
            } else {
                flags
            }
        }
        ArgKind::Opt => {
            if flags.is_empty() {
                a.name.to_string()
            } else {
                format!("{flags} {}", a.name)
            }
        }
        _ => a.name.to_string(),
    }
}
