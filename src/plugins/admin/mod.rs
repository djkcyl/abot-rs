//! 管理员命令插件 —— 给主人 / 超管的 CLI 式管理命令，对普通用户隐身：每条命令
//! `gate = superuser()`，非超管触发即静默不响应（连命令存在都不暴露）；插件 `hidden = true`
//! 不进帮助菜单。
//!
//! 当前只有一条：`coin` 调整他人游戏币（加 / 扣 / 设额）。命令-only（无自有表）——调整经
//! [`AUser`] 的共享经济 API 落账，日志即核心 `coin_log`：加用原子增量
//! [`add_coin`](AUser::add_coin)，扣用带地板 [`deduct_floor`](AUser::deduct_floor)（扣到 0 为止、
//! 绝不为负），设额用锁行 [`set_coin`](AUser::set_coin)。每笔的 `reason` 都带上操作者 QQ，故
//! 一行 `coin_log` 即「谁、给谁、变了多少、变完剩多少、为什么」的完整审计，不另立表。

use nagisa::prelude::*;

use crate::data::AUser;

plugin! {
    key = "admin",
    name = "管理员",
    category = Admin,
    // 主人 / 超管专用，普通用户不可见、不可用。
    hidden = true,
    description = "管理员命令",
}

/// `coin` 的子命令动作。大小写不敏感；中文别名 加 / 扣 / 设 同义。
#[derive(ArgEnum)]
enum CoinOp {
    /// 加币（原子增量）。
    #[arg(alias = "加")]
    Add,
    /// 扣币（扣到 0 为止，绝不为负）。
    #[arg(alias = "扣", alias = "减")]
    Sub,
    /// 设额（把余额设成指定值）。
    #[arg(alias = "设", alias = "设为", alias = "设定")]
    Set,
}

/// `coin <add|sub|set> <@对象|QQ号> <数额>` 的参数。`op` 必填——缺失 / 不识别即走 `on_parse_miss`
/// 回贴命令级 usage；`target` / `amount` 可选，缺啥进 handler 再针对性提示。
#[derive(Args)]
struct CoinArgs {
    /// 动作：add 加 / sub 扣 / set 设额（中文 加 / 扣 / 设 亦可）。
    op: CoinOp,
    /// 被调整者：群里 @ 对方，私聊输 QQ 号（`at_or_id` 两种皆可，@ 不占文本词，故 `amount` 始终对齐）。
    #[arg(at_or_id, name = "对象", desc = "要调整的人：群里 @ 对方，私聊输 QQ 号")]
    target: Option<Uin>,
    /// 数额：add / sub 为增 / 减量，set 为目标余额。
    #[arg(name = "数额", desc = "add/sub 的增减量，或 set 的目标值")]
    amount: Option<i64>,
}

/// `coin` —— 调整他人游戏币（管理员）。`coin <add|sub|set> <@对象|QQ号> <数额>`。`op` 缺失 / 不识别
/// 回 usage；`target` / `amount` 缺失在 handler 里针对性提示。落账即审计：每笔 `coin_log` 的 `reason`
/// 带上操作者 QQ。
#[command(
    "coin",
    gate = superuser(),
    description = "调整他人游戏币",
    usage = "管理员命令：coin add|sub|set <@对象 或 QQ号> <数额>。add 加、sub 扣（扣到 0 为止）、set 把余额设成指定值。"
)]
async fn coin(reply: Reply, admin: AUser, args: Args<CoinArgs>) -> HandlerResult {
    let CoinArgs { op, target, amount } = args.0;
    let db = admin.db().clone();
    let operator = admin.uin();

    // 目标：没给就提示（at_or_id 在群 @ / 私聊号皆可）。
    let Some(target) = target else {
        reply.reply("要调整谁？群里 @ 对方，或私聊直接输 QQ 号。").await?;
        return Ok(());
    };
    if !target.is_user() {
        reply.reply("QQ 号不太对，@ 对方或输入正确的 QQ 号。").await?;
        return Ok(());
    }

    // 数额：三种动作都要。
    let Some(amount) = amount else {
        reply.reply("还差个数额，比如「coin add @对方 100」。").await?;
        return Ok(());
    };

    // 目标必须已有档：管理员调账只对已存在的用户动手，不给陌生 QQ 号凭空建号。
    let Some(mut tgt) = AUser::find(&db, target.0).await? else {
        reply.reply(format!("{} 还没有记录，没法调整。", target.0)).await?;
        return Ok(());
    };
    let tid = tgt.id();

    match op {
        CoinOp::Add => {
            if amount <= 0 {
                reply.reply("加的数额要大于 0。").await?;
                return Ok(());
            }
            tgt.add_coin(amount, format!("管理员加币·操作者 {operator}")).await?;
            reply.reply(format!("已给 {}（UID {tid}）加 {amount} 游戏币，现 {}。", target.0, tgt.coin())).await?;
        }
        CoinOp::Sub => {
            if amount <= 0 {
                reply.reply("扣的数额要大于 0。").await?;
                return Ok(());
            }
            let took = tgt.deduct_floor(amount, format!("管理员扣币·操作者 {operator}")).await?;
            let msg = if took == 0 {
                format!("{}（UID {tid}）余额已是 0，没扣。", target.0)
            } else if took < amount {
                format!("{}（UID {tid}）只有 {took}，已全扣，现 0。", target.0)
            } else {
                format!("已扣 {}（UID {tid}）{took} 游戏币，现 {}。", target.0, tgt.coin())
            };
            reply.reply(msg).await?;
        }
        CoinOp::Set => {
            if amount < 0 {
                reply.reply("设定值不能为负。").await?;
                return Ok(());
            }
            let (old, new) = tgt.set_coin(amount, format!("管理员设额·操作者 {operator}")).await?;
            let msg = if old == new {
                format!("{}（UID {tid}）余额已经是 {new}，没改。", target.0)
            } else {
                format!("已把 {}（UID {tid}）余额设为 {new}（原 {old}）。", target.0)
            };
            reply.reply(msg).await?;
        }
    }
    Ok(())
}
