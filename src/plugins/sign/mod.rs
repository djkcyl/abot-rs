//! 签到插件 —— 每日签到命令 `签到` / `sign`，**自带数据 + 逻辑**。
//!
//! 「插件自有数据」约定的样板：
//! - [`entity`] 定义本插件私有的 `sign` 表（与核心 `user` 表分离，按 `uin` 软关联）；
//! - [`migration`] 建该表，经 `PluginMigration` + `nagisa::inventory` **自注册**接入核心
//!   `Migrator`（核心不感知本插件）；
//! - [`logic`] 放连签结算逻辑（不在核心 `AUser` 上），触碰共享经济只走 `AUser::add_coin`。
//!
//! 本文件只做薄壳：命令词经 `#[command]` + `inventory` 自动注册，`AUser` / `Reply` 作为
//! 提取器由 dispatch 注入，handler 取发送者句柄（连接经 `user.db()` 拿，不另取 `Db`）→
//! 调 [`logic::do_sign`] → 文案化回复。

pub mod entity;
pub mod logic;
pub mod migration;
mod profile;

use chrono::{Local, Timelike};
use nagisa::prelude::*;

use crate::data::AUser;
use crate::plugins::sign::logic::{do_sign, SignOutcome};
use crate::COIN_NAME; // 全局货币名,单一来源(crate 根)

// 登记本模块即一个插件:`签到` 命令据此(最长模块前缀)归属 key="sign"。缺省字段
// (can_disable=true / default_enable=true)即娱乐类插件常态:可被群管开关、默认开启。
plugin! {
    key = "sign",
    name = "签到",
    category = Fun,
    description = "每日签到",
    usage = "发送「签到」，每天可签一次（凌晨 4 点刷新）。连签有额外奖励。",
}

/// 按当前小时数取问候语（原 ABot 口径）。深夜（凌晨/后半夜）落到「还没睡吗」一句。
fn greeting() -> &'static str {
    match Local::now().hour() {
        6..=8 => "早上好",
        9..=11 => "上午好",
        12..=13 => "中午好",
        14..=17 => "下午好",
        18..=23 => "晚上好",
        _ => "唔。。还没睡吗？要做一个乖孩子，早睡早起身体好喔！晚安❤",
    }
}

/// `签到` / `sign` → 每日签到。
///
/// 取发送者 `AUser` + 连接 `Db`，调 [`do_sign`]：当天首签返回 [`SignOutcome::Done`]
/// （金币各分项 + 连签 + 里程碑/首签/大奖 + 经验/等级），同日重复返回
/// [`SignOutcome::Already`]。两种结果各自文案化、经 `Reply` 构建体「@发送者 + 多行正文」
/// 回复；`do_sign` 返回 `nagisa::Result`，出错直接 `?` 上抛（dispatch 记日志、
/// 止于此）。
///
// TODO: 原 ABot 把签到结果经 text2image 渲染成图片再发；待 abot 有渲染模块后改回发图，
// 这里暂以纯文本发送。
#[command("签到", "sign",
    description = "每日签到领奖励",
    usage = "发送「签到」每天签一次，凌晨 4 点刷新；连续签到天数越多奖励越高，签到发金币和经验，连签满 7／30／100 天另有里程碑奖励。")]
async fn sign(reply: Reply, mut user: AUser) -> HandlerResult {
    // user 已持同一份连接（内部 Arc，克隆廉价）；克隆出来避免与 &mut user 借用冲突。
    let db = user.db().clone();
    let outcome = do_sign(&db, &mut user).await?;

    let greet = greeting();
    let text = match outcome {
        SignOutcome::Done {
            gold_add,
            continue_sign,
            milestone,
            first_sign,
            jackpot,
            exp_gain,
            level_change,
            level_info,
        } => {
            let line1 = format!("{greet}，签到成功");

            // 金币行：总额 + 条件性附注（大奖 / 里程碑 / 首签）。
            let mut line2 = format!("获得 {gold_add} {COIN_NAME}");
            if jackpot {
                line2.push_str("，触发大奖 +666");
            }
            if milestone > 0 {
                line2.push_str(&format!("，连签 {continue_sign} 天里程碑 +{milestone}"));
            }
            if first_sign {
                line2.push_str("，首签 +66");
            }

            // 经验行：本次经验 + 升级附注 + 当前等级与级内进度。
            let mut line3 = format!("经验 +{exp_gain}");
            if level_change.leveled_up() {
                line3.push_str(&format!("，升到 Lv.{}", level_change.after));
            }
            line3.push_str(&format!(
                "，当前 Lv.{}（{}/{}）",
                level_info.level, level_info.into_level, level_info.level_span
            ));

            let line4 = format!("连签 {continue_sign} 天，余额 {} {COIN_NAME}", user.coin());

            format!("{line1}\n{line2}\n{line3}\n{line4}")
        }
        SignOutcome::Already => {
            format!("{greet}，今天已经签到过了，凌晨 4 点后再来")
        }
    };

    // 回复触发的原消息(quote)。
    reply.reply(text).await?;
    Ok(())
}
