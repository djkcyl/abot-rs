//! 帮助插件 —— 命令 `help` / `帮助` / `菜单`，**自动**从插件注册表生成菜单与每条命令的用法。
//!
//! 不手维护任何清单:`nagisa::registered_plugins()` / `registered_triggers()` 由 inventory 在编译期
//! 收集,本命令据此分组、跳过 `hidden` 的后台插件、展开每个插件下的命令与别名。新插件/新命令加进来
//! 即自动出现,无需改本文件。
//!
//! - `help`(空参):按 `CATEGORY_ORDER` 分组,每个插件列「名字 —— 简介」,末尾给一句怎么看详情的提示。
//! - `help 功能名` / `help 命令`:先按命令词/命令名/命令 id 命中(命中则展开它所属的整个插件),
//!   再按插件名/插件 key 命中;都没中就回一句「没找到」。展开时逐条列出命令的主词、别名、简介与用法。
//!
//! 正文构造做成纯函数 `render_overview` / `render_detail`,handler 只负责切节点 + 合并转发发送,
//! 便于在不发 QQ 消息的前提下核对输出。

use nagisa::prelude::*;

plugin! {
    key = "help",
    name = "帮助",
    category = Tool,
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

/// `help` 的参数:尾随的目标串(功能名或命令词),保真收尾后由 handler 去空白。
#[derive(Args)]
struct HelpArgs {
    /// 要查看的功能名或命令词;缺则给出总览。
    #[arg(rest, raw, name = "功能名", desc = "功能名或命令词；不填则列出全部")]
    text: String,
}

/// `help` / `帮助` / `菜单` —— 空参给总览,带参给某个功能的详细命令,都以合并转发呈现。
#[command(
    "help",
    "帮助",
    "菜单",
    description = "查看命令菜单与用法",
    usage = "例如「help 漂流瓶」看漂流瓶的全部命令。"
)]
async fn help(
    reply: Reply,
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
        let overview = render_overview(&plugins, &triggers, &enabled, peer);
        let nodes = ForwardNode::chunk_text(me, "命令菜单", overview, NODE_MAX_CHARS);
        reply.send(&[Segment::Forward(Forward::nodes(nodes).title("命令菜单"))]).await?;
        return Ok(());
    }

    // 详情:正文(每条命令一块)与标题各自解析(同一份命中规则,标题取所属插件名 + 「· 用法」)。
    let Some(blocks) = render_detail(target, &plugins, &triggers, &enabled, peer) else {
        reply.reply("没找到这个功能，发送 help 看全部功能。").await?;
        return Ok(());
    };
    let title = detail_title(target, &plugins, &triggers).unwrap_or_else(|| "用法".to_string());
    // 按「一条命令」为单位切节点：命令块各自完整，绝不会被拆到两个节点。
    let nodes = ForwardNode::chunk_items(me, title.clone(), blocks, "\n\n", NODE_MAX_CHARS);
    reply.send(&[Segment::Forward(Forward::nodes(nodes).title(title))]).await?;
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

/// 总览正文:按 [`CATEGORY_ORDER`] 分组,每个非隐藏插件一行「名字 —— 简介」,组内按名字排序;末尾给提示。
/// 简介经 [`plugin_desc`] 兜底(单命令插件取那条命令的描述);被整体停用的插件名后标「（已停用）」。
pub(crate) fn render_overview(
    plugins: &[PluginMeta],
    triggers: &[TriggerMeta],
    enabled: &EnabledSet,
    peer: Option<Peer>,
) -> String {
    let mut sections = Vec::new();
    for (cat, label) in CATEGORY_ORDER {
        let mut ps: Vec<&PluginMeta> = plugins.iter().filter(|p| !p.hidden && p.category == *cat).collect();
        if ps.is_empty() {
            continue;
        }
        ps.sort_by_key(|p| p.name);
        let mut lines = vec![format!("【{label}】")];
        for p in ps {
            let name = name_with_off(p.name, plugin_off(p, enabled, peer));
            let desc = plugin_desc(p, triggers);
            if desc.is_empty() {
                lines.push(name);
            } else {
                lines.push(format!("{name} —— {desc}"));
            }
        }
        sections.push(lines.join("\n"));
    }

    let mut body = if sections.is_empty() { "暂无可用命令".to_string() } else { sections.join("\n\n") };
    body.push_str("\n\n发送「help 功能名」看某个功能的详细命令，例如「help 漂流瓶」。");
    body
}

/// 把 `target` 解析到要展开的插件,没命中返回 `None`。
///
/// 命中优先级:先按命令命中(非隐藏的命令触发器,命令词 / 命令名 / 命令 id 任一对上)→ 展开它
/// 所属的整个插件;再按插件名 / 插件有效 key 命中。`target` 应已去空白。
fn resolve_plugin<'a>(target: &str, plugins: &'a [PluginMeta], triggers: &[TriggerMeta]) -> Option<&'a PluginMeta> {
    // (a) 命令命中:其所属插件(经 `plugin_key` 对上有效 key)。
    let cmd_hit = triggers.iter().find(|t| {
        matches!(t.kind, TriggerKind::Command)
            && !t.hidden
            && (t.words.contains(&target) || t.name == target || t.id == target)
    });
    if let Some(t) = cmd_hit {
        return plugins.iter().find(|p| !p.hidden && effective_key(p) == t.plugin_key);
    }
    // (b) 插件命中:按名字或有效 key。
    plugins.iter().find(|p| !p.hidden && (p.name == target || effective_key(p) == target))
}

/// 某个功能的详情正文,**按命令分块**返回(头部块 + 每条命令一块);没命中返回 `None`
/// (命中规则见 [`resolve_plugin`])。分块交给 `chunk_items` 按条目切节点,保证不拆散一条命令。
pub(crate) fn render_detail(
    target: &str,
    plugins: &[PluginMeta],
    triggers: &[TriggerMeta],
    enabled: &EnabledSet,
    peer: Option<Peer>,
) -> Option<Vec<String>> {
    let plugin = resolve_plugin(target, plugins, triggers)?;
    Some(render_plugin_detail(plugin, triggers, enabled, peer))
}

/// 命中功能时的合并转发标题 `{插件名} · 用法`,没命中返回 `None`(与 [`render_detail`] 同源)。
fn detail_title(target: &str, plugins: &[PluginMeta], triggers: &[TriggerMeta]) -> Option<String> {
    resolve_plugin(target, plugins, triggers).map(|p| format!("{} · 用法", p.name))
}

/// 渲染一个插件的详情:分块返回——头部块(名字 + 简介)+ 每条命令一块。每条命令的用法/参数已自动
/// 生成,故头部只留插件名与简介,不再重复插件级用法。由调用方用 `chunk_items` 按块切节点。
fn render_plugin_detail(
    plugin: &PluginMeta,
    triggers: &[TriggerMeta],
    enabled: &EnabledSet,
    peer: Option<Peer>,
) -> Vec<String> {
    let key = effective_key(plugin);

    // 插件整体停用则名字也标注。
    let name = name_with_off(plugin.name, plugin_off(plugin, enabled, peer));
    let head = if plugin.description.is_empty() {
        format!("【{name}】")
    } else {
        format!("【{name}】{}", plugin.description)
    };

    // 该插件下的非隐藏命令触发器,按 order 排(小在前,稳定排序故并列保持注册序)。
    let mut cmds: Vec<&TriggerMeta> = triggers
        .iter()
        .filter(|t| matches!(t.kind, TriggerKind::Command) && !t.hidden && t.plugin_key == key)
        .collect();
    cmds.sort_by_key(|t| t.order);

    // 逐条算它在该会话是否被停用,渲染。
    let mut blocks = vec![head];
    for t in cmds {
        let on = enabled.is_enabled_keyed(
            key,
            t.key,
            plugin.default_enable,
            plugin.can_disable,
            t.default_enable,
            t.can_disable,
            peer,
        );
        blocks.push(render_command(t, !on));
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

/// 渲染一条命令(CLI 式):`▸ 主词（别名：…）`、简介、**自动生成的**用法 synopsis、逐参数说明,
/// 末尾 `备注` 放参数表达不了的行为(花费/确认/冷却等,取自命令级 `usage`)。
fn render_command(t: &TriggerMeta, off: bool) -> String {
    let primary = t.words.first().copied().unwrap_or(t.name);
    let mut header = format!("▸ {primary}");
    let aliases = t.words.get(1..).unwrap_or(&[]);
    if !aliases.is_empty() {
        header.push_str(&format!("（别名：{}）", aliases.join("、")));
    }
    // 被停用的命令标注一下(不隐藏)。
    if off {
        header.push_str(" 〔已停用〕");
    }

    let mut lines = vec![header];
    if !t.description.is_empty() {
        lines.push(format!("  {}", t.description));
    }
    // 用法 synopsis：主词 + 各参数记号（无参数则只有主词）。
    lines.push(format!("  用法：{}", synopsis(primary, t.args)));
    // 逐参数说明（旗标/选项给短长名 + 显示名，位置参给显示名；后接说明）。
    for a in t.args {
        lines.push(format!("  · {}", param_line(a)));
    }
    // 备注：参数表达不了的行为(花费/确认/冷却/规则…)。
    if !t.usage.is_empty() {
        lines.push(format!("  备注：{}", t.usage));
    }
    lines.join("\n")
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

/// 单条参数说明：`前缀：说明`（无说明则只给前缀）。
fn param_line(a: &ArgSpec) -> String {
    let head = param_head(a);
    if a.desc.is_empty() { head } else { format!("{head}：{}", a.desc) }
}
