//! 帮助插件 —— 命令 `help` / `帮助` / `菜单`，**自动**从插件注册表生成命令菜单。
//!
//! 不手维护任何清单:`nagisa::registered_plugins()` 返回所有 `plugin!{}` 登记的插件(经 inventory
//! 编译期收集),本命令据此按 [`Category`] 分组、跳过 `hidden` 的后台插件(消息记录/上线通知),
//! 列出每个插件的名字 + 一句话简介。新插件加进来即自动出现在菜单里,无需改本文件。
//!
//! (展示的是插件**名字**;多数命令名即插件名——签到/转账/个人数据。命令字面量(如 ping)未在
//! 注册表里单列,故个别名字≠输入词;待 nagisa 暴露触发词后可再细化。)

use nagisa::prelude::*;

plugin! {
    key = "help",
    name = "帮助",
    category = Tool,
    description = "查看命令菜单",
    usage = "发送「帮助」/「菜单」查看全部命令。",
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

/// `help` / `帮助` / `菜单` → 合并转发:首节点是总览,其后每个非隐藏插件一节(标题 + 用法)。
#[command("help", "帮助", "菜单")]
async fn help(reply: Reply, m: MessageEvent) -> HandlerResult {
    let me = m.self_id; // 合并转发各节点的发送者署名为 bot 自己
    let plugins = registered_plugins();

    // 节点 1：总览(按分类列「名字 — 简介」)。
    let mut nodes = vec![ForwardNode::text(me, "命令菜单", overview(&plugins))];

    // 节点 2..：每个非隐藏插件一节,标题 +「用法」(没写 usage 就退回 description)。
    for (cat, _label) in CATEGORY_ORDER {
        let mut ps: Vec<&PluginMeta> =
            plugins.iter().filter(|p| !p.hidden && p.category == *cat).collect();
        ps.sort_by_key(|p| p.name);
        for p in ps {
            let detail = if p.usage.is_empty() { p.description } else { p.usage };
            let content = if detail.is_empty() {
                p.name.to_string()
            } else {
                format!("【{}】\n{detail}", p.name)
            };
            nodes.push(ForwardNode::text(me, p.name, content));
        }
    }

    reply.send(&[Segment::Forward(Forward::nodes(nodes).title("命令菜单"))]).await?;
    Ok(())
}

/// 总览节点正文:按分类列「名字 — 简介」(跳过隐藏插件、空分类)。
fn overview(plugins: &[PluginMeta]) -> String {
    let mut sections = Vec::new();
    for (cat, label) in CATEGORY_ORDER {
        let mut items: Vec<String> = plugins
            .iter()
            .filter(|p| !p.hidden && p.category == *cat)
            .map(|p| {
                if p.description.is_empty() {
                    p.name.to_string()
                } else {
                    format!("{} — {}", p.name, p.description)
                }
            })
            .collect();
        if items.is_empty() {
            continue;
        }
        items.sort(); // 注册顺序不保证稳定,排一下使菜单稳定
        sections.push(format!("【{label}】\n{}", items.join("\n")));
    }
    if sections.is_empty() {
        "暂无可用命令".to_string()
    } else {
        sections.join("\n\n")
    }
}
