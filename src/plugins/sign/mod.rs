//! 签到插件 —— 每日签到命令 `签到` / `sign` 与月历 `签到日历`，**自带数据 + 逻辑**。
//!
//! 「插件自有数据」约定的样板：
//! - [`entity`] 定义本插件私有的 `sign_log` 表（每日一行流水，签到数据的**单一真相**——
//!   去重 / 连签 / 累计 / 日历全由它派生，不设汇总行；与核心 `user` 表分离，按 `uin` 软关联）；
//! - [`migration`] 建该表，经 `PluginMigration` + `nagisa::inventory` **自注册**接入核心
//!   `Migrator`（核心不感知本插件）；
//! - [`logic`] 放派生结算逻辑（不在核心 `AUser` 上），触碰共享经济只走 `AUser::add_coin`。
//!
//! 本文件只做薄壳：命令词经 `#[command]` + `inventory` 自动注册，`AUser` / `Reply` 作为
//! 提取器由 dispatch 注入，handler 取发送者句柄（连接经 `user.db()` 拿，不另取 `Db`）→
//! 调 [`logic::do_sign`] → 结算渲成卡片图回复（[`render`]，渲不出退文字）。

pub mod entity;
pub mod logic;
pub mod migration;
mod profile;
pub mod render;

use chrono::{Local, Timelike};
use nagisa::prelude::*;

use crate::COIN_NAME; // 全局货币名,单一来源(crate 根)
use crate::data::AUser;
use crate::plugins::display_name;
use crate::plugins::sign::logic::{FIRST_GIFT, JACKPOT_GOLD, SignOutcome, do_sign};

// 登记本模块即一个插件:`签到` 命令据此(最长模块前缀)归属 key="sign"。缺省字段
// (can_disable=true / default_enable=true)即娱乐类插件常态:可被群管开关、默认开启。
plugin! {
    key = "sign",
    name = "签到",
    category = Fun,
    description = "每天签一次领游戏币和经验，连签越久奖励越高。",
}

/// 按当前小时数取问候语。深夜（凌晨/后半夜）落到「还没睡吗」一句。
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
/// （金币各分项 + 连签 + 里程碑/首签/大奖 + 经验/等级），渲成签到卡片图引用回复
/// （[`render::card_image`]，渲染失败退文字）；同日重复返回 [`SignOutcome::Already`]，
/// 一句文字打发。`do_sign` 返回 `nagisa::Result`，出错直接 `?` 上抛（dispatch 记日志、
/// 止于此）。
#[command(
    "签到",
    "sign",
    description = "每日签到领奖励",
    usage = "发送「签到」每天签一次，凌晨 4 点刷新；连续签到天数越多奖励越高，签到发金币和经验，连签满 7／30／100 天另有里程碑奖励。"
)]
async fn sign(reply: Reply, mut user: AUser, m: MessageEvent) -> HandlerResult {
    // user 已持同一份连接（内部 Arc，克隆廉价）；克隆出来避免与 &mut user 借用冲突。
    let db = user.db().clone();
    let outcome = do_sign(&db, &mut user).await?;
    let greet = greeting();

    let SignOutcome::Done {
        gold_add,
        continue_sign,
        total_sign,
        milestone,
        first_sign,
        jackpot,
        exp_gain,
        level_change,
        level_info,
        ..
    } = outcome
    else {
        reply.reply(format!("{greet}，今天已经签到过了，凌晨 4 点后再来")).await?;
        return Ok(());
    };

    let card = render::SignCard {
        name: display_name(&m, user.uin()),
        uid: user.id(),
        uin: user.uin(),
        avatar: crate::imaging::qq_avatar(user.uin()).await,
        greet,
        gold_add,
        milestone,
        first_sign,
        jackpot,
        exp_gain,
        leveled_to: level_change.leveled_up().then_some(level_change.after),
        level: level_info,
        continue_sign,
        total_sign,
        balance: user.coin(),
        theme: user.render_theme(),
    };
    match render::card_image(&card) {
        Ok(webp) => {
            reply.msg().image_bytes(webp).quote().await?;
        }
        Err(e) => {
            tracing::warn!(error = %e, "渲染签到卡片失败,退回文字");
            reply.reply(text_summary(&card)).await?;
        }
    }
    Ok(())
}

/// `签到日历` → 本月签到月历卡(业务日口径,凌晨 4 点前算前一天)。
///
/// 查 [`logic::calendar_data`](当月 `sign_log` + 汇总行)→ [`render::calendar_image`]
/// 渲月历图引用回复;渲染失败退一行文字(签过的日子序列)。没签过也出图(空日历)。
#[command(
    "签到日历",
    description = "看本月签到日历",
    usage = "发送「签到日历」，看本月哪些天签了到。凌晨 4 点前签的算前一天。"
)]
async fn calendar(reply: Reply, user: AUser, m: MessageEvent) -> HandlerResult {
    use chrono::Datelike;

    let today = crate::data::util::business_day();
    let data = logic::calendar_data(user.db(), user.uin(), today.year(), today.month(), today).await?;
    let month_days: Vec<u32> = data.days.iter().map(|d| d.day()).collect();

    let card = render::CalendarCard {
        name: display_name(&m, user.uin()),
        uid: user.id(),
        uin: user.uin(),
        avatar: crate::imaging::qq_avatar(user.uin()).await,
        year: today.year(),
        month: today.month(),
        days: month_days.iter().copied().collect(),
        today: Some(today.day()),
        continue_sign: data.continue_sign,
        total_sign: data.total_sign,
        theme: user.render_theme(),
    };
    match render::calendar_image(&card) {
        Ok(webp) => {
            reply.msg().image_bytes(webp).quote().await?;
        }
        Err(e) => {
            tracing::warn!(error = %e, "渲染签到日历失败,退回文字");
            let listed = if month_days.is_empty() {
                "这个月还没签过到".to_string()
            } else {
                let days = month_days.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("、");
                format!("签了 {} 天:{} 号", month_days.len(), days)
            };
            reply
                .reply(format!(
                    "{} 年 {} 月,{listed};连签 {} 天,累计 {} 次",
                    today.year(),
                    today.month(),
                    data.continue_sign,
                    data.total_sign
                ))
                .await?;
        }
    }
    Ok(())
}

/// 卡片的文字退路(渲染失败时用,信息同卡片)。
fn text_summary(c: &render::SignCard) -> String {
    let mut line2 = format!("获得 {} {COIN_NAME}", c.gold_add);
    if c.jackpot {
        line2.push_str(&format!("，触发大奖 +{JACKPOT_GOLD}"));
    }
    if c.milestone > 0 {
        line2.push_str(&format!("，连签 {} 天里程碑 +{}", c.continue_sign, c.milestone));
    }
    if c.first_sign {
        line2.push_str(&format!("，首签 +{FIRST_GIFT}"));
    }

    let mut line3 = format!("经验 +{}", c.exp_gain);
    if let Some(to) = c.leveled_to {
        line3.push_str(&format!("，升到 Lv.{to}"));
    }
    line3.push_str(&format!("，当前 Lv.{}（{}/{}）", c.level.level, c.level.into_level, c.level.level_span));

    format!("{}，签到成功\n{line2}\n{line3}\n连签 {} 天，余额 {} {COIN_NAME}", c.greet, c.continue_sign, c.balance)
}
