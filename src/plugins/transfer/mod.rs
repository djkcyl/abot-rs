//! 转账插件 —— 命令 `转账 @某人 <数额 | --all>`，把游戏币转给别人。命令-only(无自有表)：经
//! [`AUser::transfer_to`](crate::data::AUser::transfer_to) 在**一个事务**里原子带闸扣付款人 + 入账
//! 收款人，余额不足整体回滚。
//!
//! 参数(`#[derive(Args)]`)**都可选**:`target`(`#[arg(at_or_id)]`——群里 @、私聊输 QQ 号,二选一)
//! / `amount` / `--all` 缺啥都照样进 handler、由其解释。`amount` 可为负——用于「负数惩罚」彩蛋。
//!
//! 小巧思 + 惩罚：
//! - **负数惩罚**：填负数 = 妄图反向偷钱 → 不转,反而罚款 `abs(数额)`(封顶到余额)。
//! - **--all**：一把梭,转出当前全部余额。
//! - **每日限额**：每业务日(凌晨 4 点刷新)最多转出 `DAILY_LIMIT`;已转额从 `coin_log` 派生,不另立表。
//! - 同付款人的转账经 `single_flight` 串行(配合带闸扣款,杜绝超支/超限额竞态)。

use nagisa::prelude::*;
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};

use crate::data::AUser;
use crate::data::entity::coin_log;

plugin! {
    key = "transfer",
    name = "转账",
    category = Fun,
    description = "把游戏币转给别人，@ 对方或填 QQ 号都行。",
}

/// 每日转出上限。今日已转额从 `coin_log` 派生。
const DAILY_LIMIT: i64 = 200;

/// `转账 @某人 <数额 | --all>` 的参数。**都可选**:缺啥由 handler 解释,不靠解析层回贴 usage。
#[derive(Args)]
struct TransferArgs {
    /// 收款人:群里 @ 对方,私聊里直接输 QQ 号(`#[arg(at_or_id)]` 两种皆可)。@ 不占文本词,故
    /// 后面的 `amount` 始终对齐。
    #[arg(at_or_id, name = "对方", desc = "收款人：群里 @ 对方，私聊输 QQ 号")]
    target: Option<Uin>,
    /// `--all` / `-a`：转出当前全部余额(与数额二选一)。
    #[arg(flag, short = 'a', desc = "转出全部余额")]
    all: bool,
    /// 转账数额(文本位置参数)。可为负——触发负数惩罚彩蛋。
    #[arg(name = "金额", desc = "要转出的金额（不与 -a 同用）")]
    amount: Option<i64>,
}

/// `转账` / `transfer` → 给 @目标转游戏币。参数都可选,命令总能进来,缺啥在这里解释。
#[command("转账", "transfer", description = "把游戏币转给别人", usage = "每业务日有转出上限，凌晨 4 点刷新。")]
async fn transfer(reply: Reply, mut user: AUser, session: Session, args: Args<TransferArgs>) -> HandlerResult {
    let TransferArgs { target, all, amount } = args.0;
    let me = user.uin();
    // user 已持同一份连接（内部 Arc，克隆廉价）；克隆出来供 transferred_today 等只读查询用，
    // 避免与 user 自身的 &mut 借用（pay/transfer_to）冲突。
    let db = user.db().clone();

    // 串行化同一付款人的转账(配合带闸扣款,杜绝超支与超日限额的并发/双发竞态)。
    let Some(_guard) = session.single_flight_user() else {
        return Ok(());
    };

    let Some(target) = target else {
        reply.reply("请 @ 要转账的人，或直接输入对方 QQ 号").await?;
        return Ok(());
    };
    if !target.is_user() {
        reply.reply("QQ 号不太对，请 @ 对方或输入正确的 QQ 号").await?;
        return Ok(());
    }
    if target.0 == me {
        reply.reply("不能转给自己").await?;
        return Ok(());
    }

    // 负数惩罚:填负数 = 想反向偷钱 → 不转,反扣 abs(数额)(封顶到余额)。`checked_abs` 防
    // `i64::MIN.abs()` 溢出。带闸扣(并发下余额可能已不足,扣不动就不谎称已扣)。
    if let Some(n) = amount
        && n < 0
    {
        let fine = n.checked_abs().unwrap_or(i64::MAX).min(user.coin());
        let fined = fine > 0 && user.pay(fine, "罚款·非法转账负数").await?;
        let msg = if fined {
            format!("转账金额不能为负，已扣除 {fine} 游戏币")
        } else {
            "转账金额不能为负".to_string()
        };
        reply.reply(msg).await?;
        return Ok(());
    }

    // 数额:--all 取全部余额;否则取 amount(此时已知非负)。
    let num = if all {
        user.coin()
    } else {
        match amount {
            Some(n) => n,
            None => {
                reply.reply("请输入转账金额，或用 --all 转出全部").await?;
                return Ok(());
            }
        }
    };

    if num <= 0 {
        reply.reply("转账金额必须大于 0").await?;
        return Ok(());
    }

    // 每日限额:今日已转 + 本次 ≤ DAILY_LIMIT(同人 single_flight 串行,此读-判不竞态)。
    let sent_today = transferred_today(&db, me).await?;
    if sent_today + num > DAILY_LIMIT {
        let left = (DAILY_LIMIT - sent_today).max(0);
        reply.reply(format!("超过每日转账上限 {DAILY_LIMIT}，今天还能转 {left} 游戏币（凌晨 4 点刷新）")).await?;
        return Ok(());
    }

    // 原子转账:带闸扣付款人 + 入账收款人,余额不足整体回滚(transfer_to 内部建收款人行)。
    if !user.transfer_to(target.0, num, "转账").await? {
        reply.reply(format!("余额不足，当前 {} 游戏币", user.coin())).await?;
        return Ok(());
    }
    reply
        .msg()
        .reply_to_trigger()
        .text(format!("已转账 {num} 游戏币 给 "))
        .at(target)
        .text(format!("，余额 {} 游戏币", user.coin()))
        .send()
        .await?;
    Ok(())
}

/// 本业务日(凌晨 4 点起,全 bot 统一口径)已转出的游戏币额 = `coin_log` 里本人
/// reason=转账 的负向流水之和的绝对值。
async fn transferred_today(db: &DatabaseConnection, uin: i64) -> Result<i64> {
    let start = crate::data::util::business_day_start();
    let rows = coin_log::Entity::find()
        .filter(coin_log::Column::Uin.eq(uin))
        .filter(coin_log::Column::Reason.eq("转账"))
        .filter(coin_log::Column::Delta.lt(0))
        .filter(coin_log::Column::At.gte(start))
        .all(db)
        .await
        .context("查今日转账额")?;
    Ok(rows.iter().map(|r| -r.delta).sum())
}
