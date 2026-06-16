//! MC 服务器查询插件 —— 把 [`minecraft`](crate::integrations::minecraft) 集成接到聊天里。
//!
//! 一条命令 `mcping <地址> [--je|--be] [--full]`:默认先按 Java 版(SLP/TCP)查,连不上再自动换基岩版
//! (RakNet/UDP);带 `--je` / `--be` 旗标(框架 [`Args`] 解析)则锁定版本、不再嗅探。
//!
//! 两种出图样式:
//! - 默认 **列表样式**:原版 1.16.5「Play Multiplayer」整屏复刻(暗泥土 + 标题 + 选中条目 + LAN 扫描提示 +
//!   页脚按钮,16:9,见 [`render_select_server_png`]),条目走 vanilla 列表口径(名字 + 两行 MOTD + 信号格 + 人数)。
//! - `--full` **完整数据样式**:本仓的服务器信息卡([`render_server_card_png`]),图标 + 5 行(含延迟 /
//!   版本 / 模组 / Via 等)+ 右侧玩家 sample 悬浮窗,信息量大。
//!
//! 基岩经 [`to_ping_result`](minecraft::bedrock::to_ping_result) 折算后两样式通用。出图本不该失败,
//! 真失败即说明有 bug,直接当内部错误抛,不退文字。
//!
//! 群内还可维护一份**服务器清单**(`mc_server` 表,见 [`entity`]/[`migration`]):`mcadd` 加、`mcdel`
//! 删、`mclist` 把全清单并发批量 ping 后出一张多条目长图(放不下就拉高)。

use nagisa::prelude::*;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder, Set,
};

use crate::data::{AUser, Db};
use crate::integrations::minecraft::{
    self, BedrockOptions, CardOptions, PingError, PingResult, ScreenOptions,
};
use entity::server;

mod entity;
mod migration;

plugin! {
    key = "mcping",
    name = "MC 服务器查询",
    category = Tool,
    description = "查 Minecraft 服务器的在线人数、版本和 MOTD，Java / 基岩版都行。",
}

/// 单群清单上限。
const MAX_SERVERS: usize = 30;

/// 锁定查询的版本。无 = 自动嗅探(先 Java、连不上再基岩)。
#[derive(Clone, Copy)]
enum Edition {
    Java,
    Bedrock,
}

/// `mcping <地址> [--je|--be]` 的参数。地址是文本位置参数,`--je`/`--be` 是互斥旗标
/// (框架 `#[derive(Args)]` 解析,而非手抠文本)。
#[derive(Args)]
struct McpingArgs {
    /// 服务器地址(`host` / `host:port` / IP);缺则回 usage。
    #[arg(name = "地址", desc = "服务器地址，如 mc.hypixel.net 或 1.2.3.4:25565")]
    addr: Option<String>,
    /// `--je`:锁定 Java 版,不自动嗅探。
    #[arg(flag, desc = "强制按 Java 版查（不自动嗅探基岩）")]
    je: bool,
    /// `--be`:锁定基岩版,不自动嗅探。
    #[arg(flag, desc = "强制按基岩版查（不自动嗅探）")]
    be: bool,
    /// `--full`:出完整数据卡(图标 + 5 行 + 玩家 sample),不出列表整屏。
    #[arg(flag, desc = "出完整数据样式（信息卡），默认是原版列表样式")]
    full: bool,
}

/// `mcping <地址> [--je|--be] [--full]` —— 查 MC 服务器,出图(默认列表样式,`--full` 出数据卡)。
#[command(
    "mcping",
    description = "查 MC 服务器（Java / 基岩版）",
    usage = "发送「mcping <地址>」，如「mcping mc.hypixel.net」。默认先按 Java 查、连不上自动换基岩，锁定版本加 --je / --be。出图默认是原版「Select Server」列表样式；加 --full 出完整数据卡（含延迟 / 模组 / 玩家列表等）。"
)]
async fn mcping(reply: Reply, args: Args<McpingArgs>) -> HandlerResult {
    let McpingArgs { addr, je, be, full } = args.0;
    let addr = addr.unwrap_or_default();
    let addr = addr.trim();
    if addr.is_empty() {
        reply.reply("发「mcping <地址>」查服务器，如「mcping mc.hypixel.net」").await?;
        return Ok(());
    }
    let edition = match (je, be) {
        (true, true) => {
            reply.reply("--je 和 --be 不能一起用，二选一或都不加（自动判断）").await?;
            return Ok(());
        }
        (true, false) => Some(Edition::Java),
        (false, true) => Some(Edition::Bedrock),
        (false, false) => None,
    };

    let result = match fetch(addr, edition).await {
        Ok(r) => r,
        Err(msg) => {
            reply.reply(msg).await?;
            return Ok(());
        }
    };

    // 出图理论上不会失败;真失败即有 bug,当内部错误抛(dispatch 记 warn),不糊弄用户。
    // 默认列表样式(原版整屏),`--full` 出完整数据卡。
    let drawn = if full {
        minecraft::render_server_card_png(&result, &CardOptions::default())
    } else {
        minecraft::render_select_server_png(&result, &ScreenOptions::default())
    };
    let png = match drawn {
        Ok(p) => p,
        Err(e) => nagisa::bail!("MC 出图失败: {e}"),
    };
    reply.msg().image_bytes(png).send().await?;
    Ok(())
}

/// 按版本取一次结果,统一成 [`PingResult`](供出卡)。`Err` 是给用户看的提示串。
///
/// 自动档:先 Java,连不上(任何 [`PingError`])再试基岩;两边都不应即报「都没响应」。
async fn fetch(addr: &str, edition: Option<Edition>) -> std::result::Result<PingResult, String> {
    match edition {
        Some(Edition::Java) => minecraft::ping(addr).await.map_err(|e| ping_failure(addr, &e)),
        Some(Edition::Bedrock) => bedrock(addr).await.map_err(|e| ping_failure(addr, &e)),
        None => match minecraft::ping(addr).await {
            Ok(r) => Ok(r),
            Err(_) => bedrock(addr)
                .await
                .map_err(|_| format!("连不上 {addr}，Java / 基岩版都没响应")),
        },
    }
}

/// 基岩 ping → 折算成统一 [`PingResult`](复用 Java 整卡渲染)。映射口径(无 favicon、MOTD 拼行、
/// 版本名带 `Bedrock` 前缀 + 游戏模式)由集成层 [`to_ping_result`](minecraft::bedrock::to_ping_result)
/// 统一负责,这里不另写。
async fn bedrock(addr: &str) -> std::result::Result<PingResult, PingError> {
    let r = minecraft::ping_bedrock(addr, &BedrockOptions::default()).await?;
    Ok(minecraft::bedrock::to_ping_result(&r))
}

/// ping 失败的用户提示:超时单独点出来,其余直接用 [`PingError`] 的 `Display`。
fn ping_failure(addr: &str, e: &PingError) -> String {
    match e {
        PingError::Timeout => format!("连不上 {addr}，可能离线或地址写错了"),
        other => format!("查 {addr} 失败：{other}"),
    }
}

// ============================== 群内服务器清单 ==============================

/// 取群号(仅群聊;私聊为 `None`,清单功能不在私聊用)。
fn group_of(reply: &Reply) -> Option<i64> {
    (reply.peer().scene == Scene::Group).then(|| reply.peer().id.0)
}

/// 发送者是否群主 / 管理(改清单的门槛)。取不到成员信息按非管理处理(默认拒)。
fn is_operator(m: &MessageEvent) -> bool {
    m.member.as_ref().map(|mi| mi.is_operator()).unwrap_or(false)
}

/// 取本群清单(按添加序)。DB 错转 [`nagisa::Error`] 上抛(dispatch 记 warn)。
async fn load_servers(db: &DatabaseConnection, group_id: i64) -> std::result::Result<Vec<server::Model>, nagisa::Error> {
    server::Entity::find()
        .filter(server::Column::GroupId.eq(group_id))
        .order_by_asc(server::Column::Id)
        .all(db)
        .await
        .map_err(|e| nagisa::Error::action(format!("查服务器清单失败: {e}")))
}

/// `mcadd <地址> [名字]` —— 把一台服务器存进本群清单。
#[command(
    "mcadd",
    "mc添加",
    description = "把服务器加进本群清单",
    usage = "发送「mcadd <地址> [名字]」，把一台 MC 服务器存进本群清单，之后用「mclist」一键批量查。名字可省（默认 Minecraft Server）。"
)]
async fn mc_add(reply: Reply, user: AUser, m: MessageEvent, Db(db): Db, args: ArgText) -> HandlerResult {
    let Some(group_id) = group_of(&reply) else {
        reply.reply("服务器清单只在群里用").await?;
        return Ok(());
    };
    if !is_operator(&m) {
        reply.reply("只有群主 / 管理能改服务器清单").await?;
        return Ok(());
    }
    // 第一段为地址,其余为名字(名字可含空格)。
    let rest = args.0.trim();
    let mut it = rest.splitn(2, char::is_whitespace);
    let address = it.next().unwrap_or("").trim().to_string();
    if address.is_empty() {
        reply.reply("发「mcadd <地址> [名字]」添加，如「mcadd mc.hypixel.net Hypixel」").await?;
        return Ok(());
    }
    let name = it.next().map(str::trim).filter(|s| !s.is_empty()).unwrap_or("Minecraft Server").to_string();

    let existing = load_servers(&db, group_id).await?;
    if existing.iter().any(|m| m.address.eq_ignore_ascii_case(&address)) {
        reply.reply(format!("「{address}」已经在清单里了")).await?;
        return Ok(());
    }
    if existing.len() >= MAX_SERVERS {
        reply.reply(format!("本群清单已满（最多 {MAX_SERVERS} 台），先删几台再加")).await?;
        return Ok(());
    }

    server::ActiveModel {
        group_id: Set(group_id),
        name: Set(name.clone()),
        address: Set(address.clone()),
        added_by: Set(user.uin()),
        at: Set(chrono::Local::now().fixed_offset()),
        ..Default::default()
    }
    .insert(&db)
    .await
    .map_err(|e| nagisa::Error::action(format!("加服务器失败: {e}")))?;

    reply.reply(format!("已添加「{name}」（{address}），现在第 {} 台。发「mclist」批量查", existing.len() + 1)).await?;
    Ok(())
}

/// `mcdel <序号|名字>` —— 从本群清单删一台。
#[command(
    "mcdel",
    "mc删除",
    "mc移除",
    description = "从本群清单删服务器",
    usage = "发送「mcdel <序号>」删掉清单里第几台（序号见「mclist」），也可直接给名字或地址。"
)]
async fn mc_del(reply: Reply, m: MessageEvent, Db(db): Db, args: ArgText) -> HandlerResult {
    let Some(group_id) = group_of(&reply) else {
        reply.reply("服务器清单只在群里用").await?;
        return Ok(());
    };
    if !is_operator(&m) {
        reply.reply("只有群主 / 管理能改服务器清单").await?;
        return Ok(());
    }
    let key = args.0.trim();
    if key.is_empty() {
        reply.reply("发「mcdel <序号>」删，序号见「mclist」").await?;
        return Ok(());
    }
    let list = load_servers(&db, group_id).await?;
    if list.is_empty() {
        reply.reply("本群还没有保存的服务器").await?;
        return Ok(());
    }
    // 序号(1 起)优先,否则按名字 / 地址不区分大小写匹配。
    let target = match key.parse::<usize>() {
        Ok(idx) if idx >= 1 && idx <= list.len() => Some(&list[idx - 1]),
        Ok(_) => None,
        Err(_) => list.iter().find(|m| m.name.eq_ignore_ascii_case(key) || m.address.eq_ignore_ascii_case(key)),
    };
    let Some(m) = target else {
        reply.reply(format!("没找到「{key}」，发「mclist」看序号")).await?;
        return Ok(());
    };
    let (name, addr) = (m.name.clone(), m.address.clone());
    server::Entity::delete_by_id(m.id)
        .exec(&db)
        .await
        .map_err(|e| nagisa::Error::action(format!("删除失败: {e}")))?;
    reply.reply(format!("已删除「{name}」（{addr}）")).await?;
    Ok(())
}

/// `mclist [--full]` 的参数。
#[derive(Args)]
struct McListArgs {
    /// `--full`:每台出完整数据卡(竖排长图),而非原版列表样式。
    #[arg(flag, desc = "出完整数据样式（每台一张数据卡，竖排长图）")]
    full: bool,
}

/// `mclist [--full]` —— 把本群清单里的服务器**全部并行** ping 一遍出图;`--full` 出竖排数据卡长图。
#[command(
    "mclist",
    "mc列表",
    "mc服务器",
    description = "批量查本群清单里的所有服务器",
    usage = "发送「mclist」，把本群保存的服务器全部并行 ping，出一张原版列表样式的长图（条目多就拉高）；加 --full 改出每台一张完整数据卡的竖排长图。"
)]
async fn mc_list(reply: Reply, Db(db): Db, args: Args<McListArgs>) -> HandlerResult {
    let Some(group_id) = group_of(&reply) else {
        reply.reply("服务器清单只在群里用").await?;
        return Ok(());
    };
    let list = load_servers(&db, group_id).await?;
    if list.is_empty() {
        reply.reply("本群还没保存服务器，发「mcadd <地址> [名字]」加一台").await?;
        return Ok(());
    }

    // 全部**并行** ping:每台一个 tokio 任务(跨线程真并行,不再一条条等),自动嗅探 Java→基岩。
    let tasks: Vec<_> = list
        .into_iter()
        .map(|m| {
            tokio::spawn(async move {
                let result = fetch(&m.address, None).await.ok();
                minecraft::ListEntry { name: m.name, result }
            })
        })
        .collect();
    let mut entries = Vec::with_capacity(tasks.len());
    for t in tasks {
        if let Ok(e) = t.await {
            entries.push(e);
        }
    }

    if args.0.full {
        // 完整数据样式:每台一张数据卡,竖排成长图;连不上的卡不出、列在文字里。
        let mut cards = Vec::new();
        let mut down = Vec::new();
        for e in &entries {
            match &e.result {
                Some(r) => {
                    let opts = CardOptions { title: Some(e.name.clone()), ..Default::default() };
                    cards.push(minecraft::render_server_card(r, &opts));
                }
                None => down.push(e.name.as_str()),
            }
        }
        if cards.is_empty() {
            reply.reply("清单里的服务器都连不上").await?;
            return Ok(());
        }
        let stacked = minecraft::render::stack_vertical(&cards, 16, [20, 20, 24, 255]);
        let png = minecraft::render::encode_png(&stacked)
            .map_err(|e| nagisa::Error::action(format!("MC 列表出图失败: {e}")))?;
        let mut msg = reply.msg().image_bytes(png);
        if !down.is_empty() {
            msg = msg.text(format!("连不上：{}", down.join("、")));
        }
        msg.send().await?;
        return Ok(());
    }

    let png = match minecraft::render_server_list_png(&entries, &ScreenOptions::default()) {
        Ok(p) => p,
        Err(e) => nagisa::bail!("MC 列表出图失败: {e}"),
    };
    reply.msg().image_bytes(png).send().await?;
    Ok(())
}
