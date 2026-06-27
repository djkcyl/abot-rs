//! 赛马养成插件:领养、训练、PvE/PvP 比赛(出图 + GIF 回放)、繁殖、抽卡、群内下注。
//! 随机性集中在 [`logic`],比赛走 [`race`] 模拟内核(seed 可复现),经济走 `AUser`、道具走
//! 共享背包 [`crate::data::inventory`]。命令以「赛马」开头。

pub mod consts;
pub mod entity;
pub mod logic;
pub mod migration;
pub mod race;
pub mod render;
pub mod replay;

use std::sync::Arc;
use std::time::Duration;

use nagisa::prelude::*;
use rand::SeedableRng;
use rand::rngs::StdRng;

use crate::data::AUser;
use consts::{Difficulty, Item, ItemKind, Stat};

plugin! {
    key = "horse",
    name = "赛马",
    category = Fun,
    description = "养成你的赛马，训练提升属性、参加比赛赢奖励，出关键帧和整场 GIF 回放。",
}

/// 领养给新马随机起的名字池。
const ADOPT_NAMES: [&str; 12] =
    ["疾风", "踏云", "逐日", "奔霄", "紫电", "流光", "骁勇", "越影", "追星", "御风", "踏雪", "惊鸿"];

fn fresh_rng() -> StdRng {
    StdRng::seed_from_u64(rand::random::<u64>())
}

/// `赛马` —— 总览帮助。
#[command(
    "赛马",
    description = "赛马养成总览",
    usage = "养成你的赛马:发「赛马领养」免费领第一匹,「赛马训练 <编号> <速度/耐力/爆发/敏捷/幸运>」练属性,\
「赛马比赛 <编号> [简单/普通/困难/大师]」跑一场 PvE,「赛马马厩」看你的所有马。"
)]
async fn overview(reply: Reply, user: AUser) -> HandlerResult {
    let db = user.db().clone();
    // 新人(还没马)只给最小闭环,养起来了再看全量。
    if logic::stable_count(&db, user.uin()).await? == 0 {
        reply
            .reply(
                "赛马养成 · 新手起步 ——\n\
                 1. 赛马领养　免费领第一匹马(还送一笔启动金)\n\
                 2. 赛马查 <编号>　看你的马\n\
                 3. 赛马训练 <编号> <速度/耐力/爆发/敏捷/幸运>　花币练属性\n\
                 4. 赛马比赛 <编号>　打一场简单赛赢奖励(便宜、容易赢、立刻有正反馈)\n\
                 每天发「签到」领币;领到马后再发「赛马」就能看到繁殖/抽卡/PvP 等全部玩法,想一次看懂全部规则发「赛马玩法」。",
            )
            .await?;
        return Ok(());
    }
    reply
        .reply(
            "赛马养成 ——\n\
             · 赛马玩法　一张图讲清完整玩法(养成/比赛/抽卡/繁殖/寿命换代)\n\
             · 赛马马厩 / 赛马查 <编号>　看马\n\
             · 赛马训练 <编号> <速度/耐力/爆发/敏捷/幸运> [饲料]　花币练属性(吃饱练得更出彩)\n\
             · 赛马比赛 <编号> [简单/普通/困难/大师] [道具]　跑一场 PvE(难度=下注风险:简单稳赢小赚、大师豪赌大彩头)\n\
             · 赛马喂养 / 赛马治疗 <编号>　回饱食 / 治伤病\n\
             · 赛马用 <编号> <道具> [参数]　对马用养成/恢复/繁殖/趣味道具(育骨精料/洗髓草/特性秘传/能量饮/染色剂…)\n\
             · 赛马商店 [道具] [数量]　金币直购养成珍材(幸运高的马赛后还会掉道具)\n\
             · 赛马出售 <道具> [数量]　把用不上的道具/重复珍材回收成金币\n\
             · 赛马繁殖 <公> <母>　选育后代(资质向双亲靠拢、有几率升星、凑特性;有作种次数上限)\n\
             · 赛马抽卡 / 赛马十连　标准池(偏道具/饲料)　|　赛马马池 / 赛马马池十连　专抽马\n\
             · 赛马开房 <编号> [注额]　群内 PvP 对战;别人发「赛马报名 <编号>」加入,房主发「赛马开跑」开始\n\
             · 赛马背包 / 赛马榜 [赛季/胜率] / 赛马成就 / 赛马改名 / 赛马退役 <编号>\n\
             属性靠训练随机成长,越练越慢但没有硬上限(到天赋线后涨得很慢);比赛和训练都耗寿命(不可逆生涯耗材,\
见底后训练变低效、赛中耐力下滑还更易伤,可用刷洗/推拿/温泉回一部分,但可回复上限会永久降);比赛还可能当场受伤、留伤痕易复发,\
马也会饿——记得喂养、备好护理道具。",
        )
        .await?;
    Ok(())
}

/// `赛马玩法` —— 把 README 渲成一张图文玩法说明发出。
#[command(
    "赛马玩法",
    description = "看赛马完整玩法",
    usage = "发「赛马玩法」,出一张图讲清赛马的完整玩法(养成、比赛、抽卡、繁殖、寿命换代等)。"
)]
async fn guide(reply: Reply, user: AUser) -> HandlerResult {
    let img = render::guide_image(&user.render_theme())?;
    reply.msg().image_bytes(img).quote().await?;
    Ok(())
}

/// `赛马领养` —— 马厩为空时免费领一匹初代马。
#[command(
    "赛马领养",
    description = "免费领第一匹马",
    usage = "马厩为空时发「赛马领养」,免费领一匹初代马,另送一笔启动金。"
)]
async fn adopt(reply: Reply, mut user: AUser) -> HandlerResult {
    let db = user.db().clone();
    if logic::stable_count(&db, user.uin()).await? > 0 {
        reply.reply("你已经有马了,发「赛马马厩」看看,或之后用繁殖再要新的").await?;
        return Ok(());
    }
    let mut rng = fresh_rng();
    let birth = logic::roll_starter(&mut rng);
    let name = ADOPT_NAMES[rand::random::<u32>() as usize % ADOPT_NAMES.len()];
    let color = (rand::random::<u32>() % consts::COLOR_COUNT as u32) as i16;
    let sex = (rand::random::<u32>() % 2) as i16;
    let horse = logic::create_horse(
        &db,
        logic::NewHorse {
            owner_uin: user.uin(),
            name,
            birth: &birth,
            color,
            sex,
            generation: 1,
            parents: (None, None),
            invested: 0,
        },
    )
    .await?;

    user.add_coin(consts::STARTER_GRANT, "赛马·新手启动").await?;

    let owner = logic::owner_label(&db, user.uin()).await?;
    let theme = user.render_theme();
    let card = render::horse_card(&horse, &owner, &theme)?;
    reply
        .msg()
        .image_bytes(card)
        .text(format!(
            "领到一匹 {} 的「{}」,好好养它!另送了启动金,先「赛马训练 {}」练一项,再「赛马比赛 {}」打场简单赛。",
            consts::color_name(color),
            name,
            horse.id,
            horse.id
        ))
        .quote()
        .await?;
    Ok(())
}

/// `赛马马厩` —— 看自己的马厩。
#[command("赛马马厩", description = "看你的所有马", usage = "发「赛马马厩」,出一张马厩卡列出你的所有马。")]
async fn stable(reply: Reply, user: AUser, m: MessageEvent) -> HandlerResult {
    let db = user.db().clone();
    // 只读:内存投影出当前态,不写库。
    let horses: Vec<_> = logic::stable(&db, user.uin()).await?.iter().map(logic::project).collect();
    let owner = crate::plugins::self_shown_name(&user, &m).text;
    let title = logic::user_title(&db, user.uin()).await?;
    let theme = user.render_theme();
    let card = render::stable_card(&owner, title, &horses, &theme)?;
    reply.msg().image_bytes(card).quote().await?;
    Ok(())
}

/// 解析「编号」(首词为 i64)。
fn parse_id(s: &str) -> Option<i64> {
    s.split_whitespace().next()?.parse().ok()
}

/// 取一匹本人名下的马;不存在或非本人 → 回一句原因并返 `None`。
async fn owned_horse(
    reply: &Reply,
    db: &sea_orm::DatabaseConnection,
    uin: i64,
    id: i64,
) -> HandlerResult2<entity::horse::Model> {
    match logic::get_horse(db, id).await? {
        Some(h) if h.owner_uin == uin => Ok(Some(h)),
        Some(_) => {
            reply.reply("这匹马不是你的").await?;
            Ok(None)
        }
        None => {
            reply.reply("没找到这匹马,看看编号对不对").await?;
            Ok(None)
        }
    }
}

/// `owned_horse` 的返回:`Ok(Some)` 拿到马、`Ok(None)` 已回过原因、`Err` 上抛。
type HandlerResult2<T> = nagisa::Result<Option<T>>;

/// `赛马查 <编号>` —— 看某匹马详情。
#[command("赛马查", description = "看某匹马的详情", usage = "发「赛马查 <编号>」,出该马的属性卡。")]
async fn inspect(reply: Reply, user: AUser, args: ArgText) -> HandlerResult {
    let Some(id) = parse_id(&args.0) else {
        reply.reply("发「赛马查 <编号>」,编号看马厩").await?;
        return Ok(());
    };
    let db = user.db().clone();
    let Some(horse) = owned_horse(&reply, &db, user.uin(), id).await? else {
        return Ok(());
    };
    // 只读:内存投影出当前态,不写库。
    let horse = logic::project(&horse);
    let owner = logic::owner_label(&db, user.uin()).await?;
    let theme = user.render_theme();
    let card = render::horse_card(&horse, &owner, &theme)?;
    reply.msg().image_bytes(card).quote().await?;
    Ok(())
}

/// `赛马训练 <编号> <项> [饲料]` —— 花币 + 体力练一项属性,随机产出;可吃饲料提高好值概率。
#[command(
    "赛马训练",
    description = "花币练一项属性",
    usage = "发「赛马训练 <编号> <速度/耐力/爆发/敏捷/幸运> [训练道具]」,花游戏币和体力练该项;\
每次涨多少随机、越练越贵涨得越少。可带一个训练道具:饲料(提高出好值概率)/专注饲料(不溢出、主练维涨更多)/\
集训券(不耗体力)/破限丹(无视天赋线、练满也大涨);马饿着(饱食低)则更难出好值。"
)]
async fn train(reply: Reply, mut user: AUser, session: Session, args: ArgText) -> HandlerResult {
    // 同人单飞:挡并发训练 / 训练与比赛交叉,避免对同一匹马读改写丢更新。
    let Some(_guard) = session.single_flight_user() else {
        reply.reply("上一条还在处理,稍等").await?;
        return Ok(());
    };
    let mut it = args.0.split_whitespace();
    let (Some(id), Some(stat_word)) = (it.next().and_then(|s| s.parse::<i64>().ok()), it.next()) else {
        reply.reply("发「赛马训练 <编号> <速度/耐力/爆发/敏捷/幸运> [训练道具]」").await?;
        return Ok(());
    };
    let Some(stat) = Stat::parse(stat_word) else {
        reply.reply("练哪项?速度 / 耐力 / 爆发 / 敏捷 / 幸运 选一个").await?;
        return Ok(());
    };
    let train_item = it.next().and_then(Item::parse).filter(|i| i.kind() == ItemKind::Train);

    let db = user.db().clone();
    let Some(mut horse) = owned_horse(&reply, &db, user.uin(), id).await? else {
        return Ok(());
    };
    if horse.status == 2 {
        reply.reply("这匹马已经退役了,练不了").await?;
        return Ok(());
    }
    logic::settle_state(&db, &mut horse).await?;
    if horse.vitality < consts::VIT_TRAIN {
        reply.reply(format!("这马累了,歇会儿再来(体力 {}/{})", horse.vitality, consts::VIT_MAX)).await?;
        return Ok(());
    }
    let today = crate::data::util::business_day();
    let cost = logic::train_cost(&horse, stat, today);
    if !user.pay(cost, "赛马·训练").await? {
        reply.reply("训练得花点游戏币,你余额不够").await?;
        return Ok(());
    }

    // 带闸扣训练道具(饲料/专注/集训/破限),没有则当没带。
    let aid_used = match train_item {
        Some(f) if logic::take_item(&db, user.uin(), f, 1).await? => Some(f),
        _ => None,
    };
    let aid = logic::TrainAid::of(aid_used);
    let hungry = horse.satiety < consts::SATIETY_LOW;
    let well_fed = horse.satiety >= consts::SATIETY_HIGH;

    let mut rng = fresh_rng();
    let roll = logic::apply_train(&db, &mut horse, stat, aid, hungry, well_fed, &mut rng).await?;
    // 真埋点:训练花的币计入生涯投入(退役按比例返还)。
    logic::add_invested(&db, &mut horse, cost).await?;

    let (fs, fg) = roll.focus;
    // 增量点数可含小数;<0.05 记「几乎没涨」(亚点进度仍累进不丢)。
    let mut caption = if fg >= 0.05 {
        format!("{} +{fg:.1}", fs.name())
    } else {
        format!("{} 几乎没涨(接近瓶颈)", fs.name())
    };
    if let Some((ss, sg)) = roll.spill
        && sg >= 0.05
    {
        caption.push_str(&format!(",顺带 {} +{sg:.1}", ss.name()));
    }
    if let Some(f) = aid_used {
        caption.push_str(&format!(",用了 {}", f.name()));
    }
    if hungry {
        caption.push_str(",马有点饿(效果打折)");
    } else if well_fed {
        caption.push_str(",吃得饱(出好值更容易)");
    }
    caption.push_str(&format!("(体力 {}/{} · 累计调教 {} 次)", horse.vitality, consts::VIT_MAX, horse.train_total));

    let owner = logic::owner_label(&db, user.uin()).await?;
    let theme = user.render_theme();
    let card = render::horse_card(&horse, &owner, &theme)?;
    reply.msg().image_bytes(card).text(caption).quote().await?;
    Ok(())
}

/// 实况最多播几帧。
const LIVE_FRAMES: usize = 3;
/// 实况帧之间的停顿。
const LIVE_FRAME_GAP: Duration = Duration::from_millis(550);

/// 从关键帧里均匀挑最多 `max` 个回合(含起跑与冲线)做实况播报。
fn pick_frames(key_rounds: &[usize], max: usize) -> Vec<usize> {
    if max <= 1 || key_rounds.len() <= max {
        return key_rounds.iter().take(max.max(1)).copied().collect();
    }
    let mut out = Vec::with_capacity(max);
    for k in 0..max {
        out.push(key_rounds[k * (key_rounds.len() - 1) / (max - 1)]);
    }
    out.dedup();
    out
}

/// `赛马比赛 <编号> [难度]` —— 发起对 NPC 的 PvE 比赛。
#[command(
    "赛马比赛",
    description = "跑一场 PvE 比赛赢奖励",
    usage = "发「赛马比赛 <编号> [简单/普通/困难/大师]」,对几匹 NPC 跑一场。难度 = **下注风险**:对手按你的实力\
缩放,简单=对手弱于你(稳赢小赚)、大师=对手强于你(豪赌大彩头),不论你多强都是真比赛。越强的马同档赢得越多。\
会发过程帧、结算卡和整场 GIF;名次越高奖励越多,花报名费和体力。"
)]
async fn race_cmd(reply: Reply, mut user: AUser, session: Session, m: MessageEvent, args: ArgText) -> HandlerResult {
    let Some(_guard) = session.single_flight_user() else {
        reply.reply("你已经有一场比赛在跑了,等它结束").await?;
        return Ok(());
    };
    // 编号 + 可选难度 + 可选道具(顺序无关)。
    let mut it = args.0.split_whitespace();
    let Some(id) = it.next().and_then(|s| s.parse::<i64>().ok()) else {
        reply.reply("发「赛马比赛 <编号> [简单/普通/困难/大师] [道具...]」").await?;
        return Ok(());
    };
    let mut difficulty: Option<Difficulty> = None;
    let mut want_items: Vec<Item> = Vec::new();
    for tok in it {
        if let Some(d) = Difficulty::try_parse(tok) {
            difficulty = Some(d);
        } else if let Some(item) = Item::parse(tok)
            && item.kind() == ItemKind::Race // 只收比赛道具,饲料不能带进赛场
            && want_items.len() < consts::MAX_RACE_ITEMS
        {
            want_items.push(item);
        }
    }

    let db = user.db().clone();
    let Some(mut horse) = owned_horse(&reply, &db, user.uin(), id).await? else {
        return Ok(());
    };
    if horse.status == 2 {
        reply.reply("这匹马已经退役了,上不了场").await?;
        return Ok(());
    }
    // 不指定难度:首场默认简单(与新手引导一致),之后默认普通。
    let difficulty = difficulty.unwrap_or(if horse.races == 0 { Difficulty::Easy } else { Difficulty::Normal });
    logic::settle_state(&db, &mut horse).await?;
    if logic::is_injured(&horse) {
        let mins = logic::injury_remaining(&horse).unwrap_or(0);
        reply
            .reply(format!(
                "这匹马{}还没好,先「赛马治疗 {id}」或等 {} 小时 {} 分",
                logic::injury_name(horse.injury),
                mins / 60,
                mins % 60
            ))
            .await?;
        return Ok(());
    }
    if horse.vitality < consts::VIT_RACE {
        reply.reply(format!("体力不够跑一场(体力 {}/{}),歇会儿再来", horse.vitality, consts::VIT_MAX)).await?;
        return Ok(());
    }
    // 报名费随当日已赛场次递增。
    let fee = difficulty.entry_fee()
        + difficulty.entry_step() * logic::races_today(&horse, crate::data::util::business_day());
    if !user.pay(fee, "赛马·报名").await? {
        reply.reply("报名得花点币,你余额不够").await?;
        return Ok(());
    }

    // 带闸扣道具,没有的略过并提示。
    let mut items: Vec<Item> = Vec::new();
    let mut missing: Vec<&str> = Vec::new();
    for it in want_items {
        if logic::take_item(&db, user.uin(), it, 1).await? {
            items.push(it);
        } else {
            missing.push(it.name());
        }
    }
    if !missing.is_empty() {
        reply.reply(format!("你没有 {},这场没用上", missing.join("、"))).await?;
    }

    // 跑模拟(seed 可复现)。effective 值按当前状态打折,见 condition_stats。
    let owner_name = crate::plugins::self_shown_name(&user, &m).text;
    let player = race::RunnerInfo { name: horse.name.clone(), owner: owner_name, color: horse.color, is_npc: false };
    let stats = condition_stats(&horse);
    let player_ctx =
        race::InjuryCtx { life_frac: logic::life_ratio(&horse) as f64, scar: horse.scar, races: horse.races };
    let seed = rand::random::<u64>();
    let result = Arc::new(race::simulate(player, stats, horse.traits, player_ctx, difficulty, &items, seed));

    let theme = user.render_theme();

    // 1) 实况:开跑提示 + 挑几个关键节点播报。
    reply.reply(format!("🏇「{}」{}赛开跑!", horse.name, difficulty.name())).await?;
    for (i, &round) in pick_frames(&result.key_rounds, LIVE_FRAMES).iter().enumerate() {
        if i > 0 {
            tokio::time::sleep(LIVE_FRAME_GAP).await;
        }
        reply.msg().image_bytes(render::race_frame(&result, round, &theme)?).send().await?;
    }

    // 2) 结算:写战绩 + 名次奖(× 幸运加成) / 每日首胜 / 伤病 / 赛后掉落。
    let place = result.player_place;
    let won = place == 1;
    // 名次奖实力系数走原始五维(前四维均值,排除幸运):condition_stats 的折扣只作用于模拟,奖励侧不重复罚;
    // 幸运由下方 luck_mult 另算,不在 player_power 里双计。
    let stats = logic::stats_of(&horse); // 点数口径(列存厘点)
    let base_reward = race::reward_for(place, difficulty, race::player_power(&stats));
    // 幸运给名次奖一点加成(幸运=产出维的收益侧)。
    let luck_mult = 1.0 + (stats[Stat::Luk.idx()] as f32 / consts::LUCK_REWARD_DIV).clamp(0.0, consts::LUCK_REWARD_CAP);
    let reward = (base_reward as f32 * luck_mult).round() as i64;
    logic::finish_race(&db, &mut horse, won).await?;
    if reward > 0 {
        user.add_coin(reward, "赛马·名次奖").await?;
    }
    // 每日首胜:原子领取(跨 PvE/PvP 只发一次),抢到才发奖。
    let bonus = if won && logic::claim_first_win_today(&db, user.uin(), horse.id).await? {
        user.add_coin(consts::DAILY_FIRST_WIN_BONUS, "赛马·每日首胜").await?;
        consts::DAILY_FIRST_WIN_BONUS
    } else {
        0
    };
    let mut rng = fresh_rng();
    // 伤病在比赛内核局内已判定,这里只取本场最坏伤等落库。
    let injury = result.injuries[result.player_idx];
    if injury > 0 {
        logic::set_injury(&db, &mut horse, injury).await?;
    }
    // 赛后掉落(幸运产出维):命中则入袋,溢出折币。
    let drop = logic::roll_drop(stats[Stat::Luk.idx()], consts::Trait::Fortuitous.in_mask(horse.traits), 1.0, &mut rng);
    if let Some(it) = drop {
        let overflow = logic::add_item(&db, user.uin(), it, 1).await?;
        if overflow > 0 {
            user.add_coin(overflow as i64 * it.sell_price(), "赛马·掉落折币").await?;
        }
    }

    // 3) 结算图。
    let card = render::result_card(&result, reward, bonus, injury, difficulty.name(), &theme)?;
    reply.msg().image_bytes(card).send().await?;
    if let Some(it) = drop {
        reply.reply(format!("🍀 赛后捡到「{}」(幸运掉落)", it.name())).await?;
    }

    // 4) GIF 回放单独发。
    let gif = replay::render(result.clone()).await?;
    reply.msg().image_bytes(gif).send().await?;

    award_achievements(&reply, &mut user).await?;
    Ok(())
}

/// 比赛用的 effective 五维:按饥饿/寿命/伤痕给相应维打折。
fn condition_stats(h: &entity::horse::Model) -> [i32; consts::STAT_COUNT] {
    let mut s = logic::stats_of(h);
    if h.satiety < consts::SATIETY_LOW {
        let i = Stat::Spd.idx();
        s[i] = (s[i] as f32 * consts::HUNGRY_SPEED_MULT).round() as i32;
    }
    let r = logic::life_ratio(h);
    if r < consts::LIFESPAN_LATE_RACE_RATIO {
        let i = Stat::Sta.idx();
        s[i] = (s[i] as f32 * logic::stamina_life_mult(r)).round() as i32;
    }
    if h.scar > 0 {
        let pen = 1.0 - consts::SCAR_STAT_PENALTY[(h.scar.clamp(1, 3) - 1) as usize];
        for st in [Stat::Spd, Stat::Sta, Stat::Brs, Stat::Agi] {
            let i = st.idx();
            s[i] = (s[i] as f32 * pen).round() as i32;
        }
    }
    s
}

/// 评估并发放新达成的成就:逐个发金币并播报,无新成就则静默。各结算入口结束后调。
async fn award_achievements(reply: &Reply, user: &mut AUser) -> nagisa::Result<()> {
    let db = user.db().clone();
    let newly = logic::evaluate_and_grant(&db, user.uin()).await?;
    if newly.is_empty() {
        return Ok(());
    }
    let mut parts = Vec::new();
    for a in &newly {
        user.add_coin(a.reward(), format!("赛马·成就·{}", a.name())).await?;
        let title = a.title().map(|t| format!("·称号「{t}」")).unwrap_or_default();
        parts.push(format!("「{}」{title}", a.name()));
    }
    reply.reply(format!("达成成就 {}(另发了金币奖励)", parts.join("、"))).await?;
    Ok(())
}

/// `赛马成就` —— 看成就与称号(顺带补发漏算的成就)。
#[command(
    "赛马成就",
    description = "看赛马成就与称号",
    usage = "发「赛马成就」,出你的成就墙(已达成/未达成)和当前称号;顺带补发漏算的成就。"
)]
async fn achievements(reply: Reply, mut user: AUser, m: MessageEvent) -> HandlerResult {
    // 补发漏算的成就。
    award_achievements(&reply, &mut user).await?;
    let db = user.db().clone();
    let earned = logic::earned_achievements(&db, user.uin()).await?;
    let title = logic::user_title(&db, user.uin()).await?;
    let owner = crate::plugins::self_shown_name(&user, &m).text;
    let card = render::achievement_card(&owner, title, &earned, &user.render_theme())?;
    reply.msg().image_bytes(card).quote().await?;
    Ok(())
}

/// `赛马背包` —— 看持有的道具。
#[command("赛马背包", description = "看你的道具和饲料", usage = "发「赛马背包」,列出你持有的比赛道具和训练饲料。")]
async fn backpack(reply: Reply, user: AUser, m: MessageEvent) -> HandlerResult {
    let db = user.db().clone();
    let items = logic::backpack(&db, user.uin()).await?;
    let owner = crate::plugins::self_shown_name(&user, &m).text;
    let card = render::backpack_card(&owner, &items, &user.render_theme())?;
    reply.msg().image_bytes(card).quote().await?;
    Ok(())
}

/// `赛马繁殖 <公> <母>` —— 用两匹马繁殖一匹后代。
#[command(
    "赛马繁殖",
    description = "用两匹马繁殖后代",
    usage = "发「赛马繁殖 <公马编号> <母马编号>」,花游戏币选育一匹后代——资质向双亲靠拢、有几率升星\
(也可能降星、配出更差的),还能凑特性;母马有冷却、每匹作种有次数上限、近亲会衰退。"
)]
async fn breed(reply: Reply, mut user: AUser, session: Session, args: ArgText) -> HandlerResult {
    // 同人单飞:挡并发繁殖绕母马冷却/在厩上限。
    let Some(_guard) = session.single_flight_user() else {
        reply.reply("上一条还在处理,稍等").await?;
        return Ok(());
    };
    let mut it = args.0.split_whitespace();
    let (Some(a), Some(b)) = (it.next().and_then(|s| s.parse().ok()), it.next().and_then(|s| s.parse::<i64>().ok()))
    else {
        reply.reply("发「赛马繁殖 <公马编号> <母马编号> [星辉石]」").await?;
        return Ok(());
    };
    // 可选第三参:星辉石(下一胎必 +1 星)。
    let want_star_stone = it.next().and_then(Item::parse) == Some(Item::StarStone);
    if a == b {
        reply.reply("得用两匹不同的马").await?;
        return Ok(());
    }
    let db = user.db().clone();
    let Some(ha) = owned_horse(&reply, &db, user.uin(), a).await? else { return Ok(()) };
    let Some(hb) = owned_horse(&reply, &db, user.uin(), b).await? else { return Ok(()) };

    // 定公母。
    let (mut father, mut mother) = match (ha.sex, hb.sex) {
        (0, 1) => (ha, hb),
        (1, 0) => (hb, ha),
        _ => {
            reply.reply("繁殖需要一公一母").await?;
            return Ok(());
        }
    };
    // 结算到期状态后查伤:带伤的马先治好再配。
    logic::settle_state(&db, &mut father).await?;
    logic::settle_state(&db, &mut mother).await?;
    if logic::is_injured(&father) || logic::is_injured(&mother) {
        reply.reply("有马还带着伤,先治好再配").await?;
        return Ok(());
    }
    for h in [&father, &mother] {
        if h.breed_count >= consts::BREED_COUNT_MAX {
            reply.reply(format!("「{}」作种次数到上限了,换一匹来配(到顶的可退役换币)", h.name)).await?;
            return Ok(());
        }
    }
    let now = chrono::Local::now().fixed_offset();
    if let Some(until) = mother.breed_cd_until
        && until > now
    {
        let mins = (until - now).num_minutes().max(1);
        reply.reply(format!("母马还在休养,{} 小时 {} 分后能再繁殖", mins / 60, mins % 60)).await?;
        return Ok(());
    }
    if logic::stable_active_count(&db, user.uin()).await? >= consts::STABLE_CAP {
        reply.reply("在厩满了,先退役一匹再繁殖(退役不占格)").await?;
        return Ok(());
    }
    let incest = logic::is_incest(&db, father.id, mother.id, consts::BREED_INCEST_DEPTH).await?;
    let cost = logic::breed_cost(&father, &mother);
    // 星辉石先于扣费取下:没有就回原因不扣费(免静默降级产普通胎);扣费失败再把石头退回(不吞石头)。
    let star_stone = if want_star_stone {
        if !logic::take_item(&db, user.uin(), Item::StarStone, 1).await? {
            reply.reply("你没有星辉石,想升星先去抽卡或商店备一颗;不带星辉石也能繁殖").await?;
            return Ok(());
        }
        true
    } else {
        false
    };
    if !user.pay(cost, "赛马·繁殖").await? {
        if star_stone {
            logic::add_item(&db, user.uin(), Item::StarStone, 1).await?;
        }
        reply.reply("繁殖得花点币,你余额不够").await?;
        return Ok(());
    }
    let mut rng = fresh_rng();
    let child = logic::breed_child(&father, &mother, incest, star_stone, &mut rng);
    let name = ADOPT_NAMES[rand::random::<u32>() as usize % ADOPT_NAMES.len()];
    // 真埋点:繁殖费(带星辉石另加其回收价)计入子代生涯投入。
    let foal_invested = cost + if star_stone { Item::StarStone.sell_price() } else { 0 };
    let foal = logic::create_horse(
        &db,
        logic::NewHorse {
            owner_uin: user.uin(),
            name,
            birth: &child.birth,
            color: child.color,
            sex: child.sex,
            generation: child.generation,
            parents: (Some(father.id), Some(mother.id)),
            invested: foal_invested,
        },
    )
    .await?;
    logic::set_breed_cd(&db, mother.id).await?;
    logic::bump_breed_count(&db, father.id).await?;
    logic::bump_breed_count(&db, mother.id).await?;

    let owner = logic::owner_label(&db, user.uin()).await?;
    let theme = user.render_theme();
    let card = render::horse_card(&foal, &owner, &theme)?;
    let note = if star_stone {
        "(用了星辉石,升一星)"
    } else if incest {
        "(近亲繁殖,星级只跌不升)"
    } else {
        ""
    };
    reply.msg().image_bytes(card).text(format!("配出一匹第 {} 代后代{note}", child.generation)).quote().await?;

    award_achievements(&reply, &mut user).await?;
    Ok(())
}

/// `赛马改名 <编号> <新名>` —— 花币给马改名。
#[command("赛马改名", description = "给马改名", usage = "发「赛马改名 <编号> <新名字>」,花游戏币给马改个名。")]
async fn rename(reply: Reply, mut user: AUser, args: ArgText) -> HandlerResult {
    let rest = args.0.trim();
    let Some((id_str, name)) = rest.split_once(char::is_whitespace) else {
        reply.reply("发「赛马改名 <编号> <新名字>」").await?;
        return Ok(());
    };
    let Some(id) = id_str.trim().parse::<i64>().ok() else {
        reply.reply("编号要是数字,看马厩").await?;
        return Ok(());
    };
    let name = name.trim();
    let chars = name.chars().count();
    if chars == 0 || chars > consts::NAME_MAX_CHARS {
        reply.reply(format!("名字要 1 到 {} 个字", consts::NAME_MAX_CHARS)).await?;
        return Ok(());
    }
    let db = user.db().clone();
    let Some(mut horse) = owned_horse(&reply, &db, user.uin(), id).await? else { return Ok(()) };
    if !user.pay(consts::RENAME_COST, "赛马·改名").await? {
        reply.reply("改名得花点币,你余额不够").await?;
        return Ok(());
    }
    logic::rename(&db, &mut horse, name).await?;
    let owner = logic::owner_label(&db, user.uin()).await?;
    let card = render::horse_card(&horse, &owner, &user.render_theme())?;
    reply.msg().image_bytes(card).text("改好了").quote().await?;
    Ok(())
}

/// `赛马退役 <编号>` —— 二次确认后退役一匹马领回馈。
#[command(
    "赛马退役",
    description = "退役一匹马领回馈",
    usage = "发「赛马退役 <编号>」,确认后让这匹马退役、领一笔回馈金币(退役后不能比赛/训练,但仍可作种繁殖)。"
)]
async fn retire(reply: Reply, mut user: AUser, session: Session, args: ArgText) -> HandlerResult {
    // 同人单飞:覆盖确认 + 落库全程,挡并发双领回馈。
    let Some(_guard) = session.single_flight_user() else {
        reply.reply("上一条还在处理,稍等").await?;
        return Ok(());
    };
    let Some(id) = parse_id(&args.0) else {
        reply.reply("发「赛马退役 <编号>」").await?;
        return Ok(());
    };
    let db = user.db().clone();
    let Some(mut horse) = owned_horse(&reply, &db, user.uin(), id).await? else { return Ok(()) };
    if horse.status == 2 {
        reply.reply("这匹马已经退役了").await?;
        return Ok(());
    }
    let reward = logic::retire_reward(&horse);
    reply
        .reply(format!("确认让「{}」退役吗?会领一笔回馈金币,之后不能比赛/训练。回复 y 确认、n 取消", horse.name))
        .await?;
    let waiter = session.waiter().from_starter().build();
    match waiter
        .recv_parse(Duration::from_secs(60), super::is_cancel, |s| {
            if super::is_yes(s) { Ok(()) } else { Err("回复 y 确认、n 取消".to_string()) }
        })
        .await
    {
        Replied::Got(()) => {}
        Replied::Cancelled => {
            reply.reply("行,不退了").await?;
            return Ok(());
        }
        Replied::TimedOut => {
            reply.reply("没等到确认,先不退了").await?;
            return Ok(());
        }
    }
    logic::set_status(&db, &mut horse, 2).await?;
    user.add_coin(reward, "赛马·退役回馈").await?;
    reply.reply(format!("「{}」退役了,领到一笔回馈金币", horse.name)).await?;
    Ok(())
}

/// `赛马抽卡` / `赛马十连` —— 花币抽道具或新马。
#[command(
    "赛马抽卡",
    description = "单抽:出道具或新马",
    usage = "发「赛马抽卡」单抽一次,大概率出道具、小概率出新马(偏低星)。"
)]
async fn gacha_single(reply: Reply, user: AUser, session: Session) -> HandlerResult {
    do_gacha(reply, user, session, 1, false).await
}

#[command(
    "赛马十连",
    description = "十连抽",
    usage = "发「赛马十连」抽十次(有小折扣),这一趟至少出一匹马;抽得够多还会保底高星。"
)]
async fn gacha_ten(reply: Reply, user: AUser, session: Session) -> HandlerResult {
    do_gacha(reply, user, session, 10, false).await
}

/// `赛马马池` —— 专抽马的卡池(贵但大概率出马)。
#[command(
    "赛马马池",
    description = "马池单抽:大概率出马",
    usage = "发「赛马马池」单抽一次,比标准池贵但大概率出马(偏低星);想专门搏好马走这里。"
)]
async fn gacha_horse_single(reply: Reply, user: AUser, session: Session) -> HandlerResult {
    do_gacha(reply, user, session, 1, true).await
}

/// `赛马马池十连` —— 马池十连。
#[command(
    "赛马马池十连",
    description = "马池十连",
    usage = "发「赛马马池十连」抽十次(有小折扣),出马率高、这一趟至少出一匹;抽得够多还会保底高星。"
)]
async fn gacha_horse_ten(reply: Reply, user: AUser, session: Session) -> HandlerResult {
    do_gacha(reply, user, session, 10, true).await
}

/// 把一匹抽到/兜底的马放进马厩:在厩未满则建马入厩,满则折币返还。`stable_n`(本调用会自增)、
/// `refund`/`lines` 为累计的在厩数、返还、展示行。
async fn grant_horse(
    db: &sea_orm::DatabaseConnection,
    uin: i64,
    birth: &logic::Birth,
    stable_n: &mut usize,
    lines: &mut Vec<render::GachaLine>,
    refund: &mut i64,
) -> nagisa::Result<()> {
    if *stable_n < consts::STABLE_CAP {
        let name = ADOPT_NAMES[rand::random::<u32>() as usize % ADOPT_NAMES.len()];
        let color = (rand::random::<u32>() % consts::COLOR_COUNT as u32) as i16;
        let sex = (rand::random::<u32>() % 2) as i16;
        let rarity = birth.rarity;
        let foal = logic::create_horse(
            db,
            logic::NewHorse {
                owner_uin: uin,
                name,
                birth,
                color,
                sex,
                generation: 1,
                parents: (None, None),
                invested: 0,
            },
        )
        .await?;
        *stable_n += 1;
        lines.push(render::GachaLine { text: format!("{rarity}★ 新马 · {name} #{}", foal.id), rare: true });
    } else {
        *refund += consts::GACHA_HORSE_FULL_REFUND;
        lines.push(render::GachaLine { text: "出马但马厩满,折成了金币".to_string(), rare: false });
    }
    Ok(())
}

/// 抽卡公共流程:扣币 → 逐抽产出(道具入袋 / 马入厩,溢出折币)→ 落保底 → 出结果卡。`horse_pool` 走马池。
/// 两池共用同一 ★3+ 保底计数。同人单飞:挡并发抽卡丢保底/绕在厩上限多发马。
async fn do_gacha(reply: Reply, mut user: AUser, session: Session, count: usize, horse_pool: bool) -> HandlerResult {
    let Some(_guard) = session.single_flight_user() else {
        reply.reply("上一抽还在处理,稍等").await?;
        return Ok(());
    };
    let (cost, class_weights, horse_rarity) = if horse_pool {
        let c = if count == 1 { consts::GACHA_HORSE_POOL_SINGLE_COST } else { consts::GACHA_HORSE_POOL_TEN_COST };
        (c, &consts::GACHA_HORSE_POOL_CLASS_WEIGHTS, &consts::GACHA_HORSE_POOL_RARITY_WEIGHTS)
    } else {
        let c = if count == 1 { consts::GACHA_SINGLE_COST } else { consts::GACHA_TEN_COST };
        (c, &consts::GACHA_CLASS_WEIGHTS, &consts::GACHA_HORSE_RARITY_WEIGHTS)
    };
    if !user.pay(cost, "赛马·抽卡").await? {
        reply.reply("抽卡得花点币,你余额不够").await?;
        return Ok(());
    }
    let db = user.db().clone();
    let uin = user.uin();
    let mut pity = logic::gacha_pity(&db, uin).await?;
    let mut stable_n = logic::stable_active_count(&db, uin).await?;
    let mut rng = fresh_rng();
    let mut lines: Vec<render::GachaLine> = Vec::new();
    let mut refund: i64 = 0;
    let mut got_horse = false;

    for _ in 0..count {
        match logic::gacha_pull(&mut pity, class_weights, horse_rarity, &mut rng) {
            logic::Pull::Item(it) => {
                let overflow = logic::add_item(&db, uin, it, 1).await?;
                refund += overflow as i64 * it.sell_price();
                // 珍材整类稀有,外加各类内的高端道具(普通恢复/饲料不算)。
                let rare = Item::TREASURE.contains(&it)
                    || matches!(
                        it,
                        Item::Steady
                            | Item::Mark
                            | Item::Reflect
                            | Item::Clover
                            | Item::Feed2
                            | Item::BreakPill
                            | Item::Care3
                    );
                let tail = if overflow > 0 { "(已满,折币)" } else { "" };
                lines
                    .push(render::GachaLine {
                        text: format!("{} · {}{tail}", it.gacha_class_name(), it.name()), rare
                    });
            }
            logic::Pull::Horse(birth) => {
                got_horse = true;
                grant_horse(&db, uin, &birth, &mut stable_n, &mut lines, &mut refund).await?;
            }
        }
    }
    // 十连保底:整轮没出马则按本池星级权重补一匹(不动 ★3+ 保底计数,纯额外兜底)。
    if count >= 10 && !got_horse {
        let birth = logic::roll_birth(horse_rarity, &mut rng);
        grant_horse(&db, uin, &birth, &mut stable_n, &mut lines, &mut refund).await?;
    }
    logic::set_gacha_pity(&db, uin, pity).await?;
    if refund > 0 {
        user.add_coin(refund, "赛马·抽卡返还").await?;
    }
    lines.push(render::GachaLine {
        text: format!("距高星保底还差 {} 抽", (consts::GACHA_PITY - pity).max(0)),
        rare: false,
    });

    let title = if count == 1 { "抽卡" } else { "十连" };
    let card = render::gacha_card(title, &lines, &user.render_theme())?;
    reply.msg().image_bytes(card).quote().await?;

    award_achievements(&reply, &mut user).await?;
    Ok(())
}

/// `赛马喂养 <编号>` —— 买基础草料回饱食(日常维护,金币内循环)。
#[command(
    "赛马喂养",
    description = "喂草料回饱食",
    usage = "发「赛马喂养 <编号>」,花几枚游戏币买草料回饱食度。马饿着会影响训练和比赛;\
更好的饲料(精饲料/滋补膏)留着训练时吃。"
)]
async fn feed(reply: Reply, mut user: AUser, args: ArgText) -> HandlerResult {
    let Some(id) = parse_id(&args.0) else {
        reply.reply("发「赛马喂养 <编号>」").await?;
        return Ok(());
    };
    let db = user.db().clone();
    let Some(mut horse) = owned_horse(&reply, &db, user.uin(), id).await? else { return Ok(()) };
    logic::settle_state(&db, &mut horse).await?;
    if horse.satiety >= consts::VIT_MAX {
        reply.reply("这马吃得很饱,先不用喂").await?;
        return Ok(());
    }
    if !user.pay(consts::FORAGE_COST, "赛马·草料").await? {
        reply.reply("买草料得花点币,你余额不够").await?;
        return Ok(());
    }
    logic::feed_basic(&db, &mut horse).await?;
    // 真埋点:喂养花的币计入生涯投入。
    logic::add_invested(&db, &mut horse, consts::FORAGE_COST).await?;
    reply.reply(format!("喂了点草料,饱食 {}/{}", horse.satiety, consts::VIT_MAX)).await?;
    Ok(())
}

/// 带闸扣一个道具:成则 `true`,没有则回一句并返 `false`。
async fn take_or_reply(reply: &Reply, db: &sea_orm::DatabaseConnection, uin: i64, item: Item) -> nagisa::Result<bool> {
    if logic::take_item(db, uin, item, 1).await? {
        Ok(true)
    } else {
        reply.reply(format!("你没有 {},先去抽卡", item.name())).await?;
        Ok(false)
    }
}

/// `赛马用 <编号> <道具> [维度/颜色/新名]` —— 对一匹马使用养成 / 恢复 / 繁殖 / 趣味道具(效果按具体道具)。
#[command(
    "赛马用",
    description = "对马使用道具(养成/恢复/繁殖/趣味)",
    usage = "发「赛马用 <编号> <道具> [参数]」对一匹马用道具:育骨精料 <维度>(永久 +资质)、洗髓草(重摇成长)、\
特性秘传(学一条特性)、静心符(重摇特性)、能量饮(回体力)、金疮药(治伤)、精草料(回饱食)、\
刷洗/推拿/温泉疗养(回寿命,但可回复上限永久降)、红绳(清母马繁殖冷却)、续种符(多配一次)、染色剂 <毛色>、改名牌 <新名>。\
星辉石请在「赛马繁殖」时带。"
)]
async fn use_item(reply: Reply, mut user: AUser, session: Session, args: ArgText) -> HandlerResult {
    // 同人单飞:挡并发对同一匹马读改写(资质/成长/特性等)丢更新。
    let Some(_guard) = session.single_flight_user() else {
        reply.reply("上一条还在处理,稍等").await?;
        return Ok(());
    };
    let mut it = args.0.split_whitespace();
    let Some(id) = it.next().and_then(|s| s.parse::<i64>().ok()) else {
        reply.reply("发「赛马用 <编号> <道具> [维度/颜色/新名]」").await?;
        return Ok(());
    };
    let Some(item) = it.next().and_then(Item::parse).filter(|i| i.kind() == ItemKind::Use) else {
        reply.reply("用哪个道具?填养成/恢复/繁殖/趣味道具名(看「赛马背包」);星辉石请在繁殖时带").await?;
        return Ok(());
    };
    let rest: Vec<&str> = it.collect();

    let db = user.db().clone();
    let uin = user.uin();
    let Some(mut horse) = owned_horse(&reply, &db, uin, id).await? else { return Ok(()) };
    logic::settle_state(&db, &mut horse).await?;
    let mut rng = fresh_rng();

    match item {
        Item::ReachTonic => {
            let Some(stat) = rest.first().and_then(|s| Stat::parse(s)) else {
                reply.reply("育骨精料要指定维度:赛马用 <编号> 育骨精料 <速度/耐力/爆发/敏捷/幸运>").await?;
                return Ok(());
            };
            if !take_or_reply(&reply, &db, uin, item).await? {
                return Ok(());
            }
            logic::apply_reach_tonic(&db, &mut horse, stat).await?;
            let card = render::horse_card(&horse, &logic::owner_label(&db, uin).await?, &user.render_theme())?;
            reply.msg().image_bytes(card).text(format!("{} 资质提升了", stat.name())).quote().await?;
        }
        Item::GrowthHerb => {
            if !take_or_reply(&reply, &db, uin, item).await? {
                return Ok(());
            }
            logic::reroll_growth(&db, &mut horse, &mut rng).await?;
            let card = render::horse_card(&horse, &logic::owner_label(&db, uin).await?, &user.render_theme())?;
            reply.msg().image_bytes(card).text("重摇了它的成长").quote().await?;
        }
        Item::TraitBook => {
            if consts::Trait::from_mask(horse.traits).len() as u32 >= consts::TRAIT_MAX {
                reply.reply("已经有两条特性了,想换发「静心符」洗").await?;
                return Ok(());
            }
            if !take_or_reply(&reply, &db, uin, item).await? {
                return Ok(());
            }
            let got = logic::add_random_trait(&db, &mut horse, &mut rng).await?;
            let card = render::horse_card(&horse, &logic::owner_label(&db, uin).await?, &user.render_theme())?;
            let msg = match got {
                Some(t) => format!("学会了特性「{}」", t.name()),
                None => "没学到新特性".to_string(),
            };
            reply.msg().image_bytes(card).text(msg).quote().await?;
        }
        Item::TraitReroll => {
            if !take_or_reply(&reply, &db, uin, item).await? {
                return Ok(());
            }
            logic::reroll_traits(&db, &mut horse, &mut rng).await?;
            let ts = consts::Trait::from_mask(horse.traits);
            let card = render::horse_card(&horse, &logic::owner_label(&db, uin).await?, &user.render_theme())?;
            let msg = if ts.is_empty() {
                "重摇后没有特性,手气背了点".to_string()
            } else {
                format!("重摇出特性:{}", ts.iter().map(|t| t.name()).collect::<Vec<_>>().join("、"))
            };
            reply.msg().image_bytes(card).text(msg).quote().await?;
        }
        Item::EnergyDrink => {
            if horse.vitality >= consts::VIT_MAX {
                reply.reply("体力满着呢,先不用").await?;
                return Ok(());
            }
            if !take_or_reply(&reply, &db, uin, item).await? {
                return Ok(());
            }
            logic::restore_vitality(&db, &mut horse, consts::ENERGY_RESTORE).await?;
            reply.reply(format!("灌了瓶能量饮,体力到 {}/{}", horse.vitality, consts::VIT_MAX)).await?;
        }
        Item::Medicine => {
            if !logic::is_injured(&horse) {
                reply.reply("这马没受伤,不用治").await?;
                return Ok(());
            }
            if !take_or_reply(&reply, &db, uin, item).await? {
                return Ok(());
            }
            let was = logic::injury_name(horse.injury);
            logic::heal(&db, &mut horse).await?;
            reply.reply(format!("敷上金疮药,治好了{was};但留下伤痕,一段时间内易复发、属性略降,歇几场再上更稳")).await?;
        }
        Item::FineForage => {
            if horse.satiety >= consts::VIT_MAX {
                reply.reply("吃得很饱,先不用喂").await?;
                return Ok(());
            }
            if !take_or_reply(&reply, &db, uin, item).await? {
                return Ok(());
            }
            logic::restore_satiety(&db, &mut horse, consts::FINE_FORAGE_SATIETY).await?;
            reply.reply(format!("喂了上等草料,饱食到 {}/{}", horse.satiety, consts::VIT_MAX)).await?;
        }
        Item::Care1 | Item::Care2 | Item::Care3 => {
            if horse.lifespan >= horse.lifespan_cap {
                reply.reply("这马寿命满着呢,先不用护理").await?;
                return Ok(());
            }
            if !take_or_reply(&reply, &db, uin, item).await? {
                return Ok(());
            }
            logic::apply_restore(&db, &mut horse, item.life_restore(), item.life_cap_cost()).await?;
            reply
                .reply(format!(
                    "用了 {},寿命回到 {}/{}(用多了可回复上限会永久降)",
                    item.name(),
                    horse.lifespan,
                    horse.lifespan_cap,
                ))
                .await?;
        }
        Item::RedString => {
            if horse.sex != 1 {
                reply.reply("红绳是给母马解繁殖冷却的").await?;
                return Ok(());
            }
            let now = chrono::Local::now().fixed_offset();
            if horse.breed_cd_until.is_none_or(|u| u <= now) {
                reply.reply("这匹母马没在冷却,用不上红绳").await?;
                return Ok(());
            }
            if !take_or_reply(&reply, &db, uin, item).await? {
                return Ok(());
            }
            logic::clear_breed_cd(&db, &mut horse).await?;
            reply.reply("红绳一牵,母马现在就能再繁殖").await?;
        }
        Item::BreedCharm => {
            if horse.breed_count <= 0 {
                reply.reply("这匹马还没作过种,用不上续种符").await?;
                return Ok(());
            }
            if !take_or_reply(&reply, &db, uin, item).await? {
                return Ok(());
            }
            logic::reduce_breed_count(&db, &mut horse).await?;
            reply.reply("用了续种符,这匹马能多配一次").await?;
        }
        Item::Dye => {
            let Some(c) = rest.first().and_then(|s| consts::color_index(s)) else {
                reply.reply("染什么色?枣红 / 栗色 / 乌骓 / 白龙 / 青骢 / 金棕 选一个").await?;
                return Ok(());
            };
            if c == horse.color {
                reply.reply("已经是这个毛色了").await?;
                return Ok(());
            }
            if !take_or_reply(&reply, &db, uin, item).await? {
                return Ok(());
            }
            logic::set_color(&db, &mut horse, c).await?;
            let card = render::horse_card(&horse, &logic::owner_label(&db, uin).await?, &user.render_theme())?;
            reply.msg().image_bytes(card).text(format!("染成了{}", consts::color_name(c))).quote().await?;
        }
        Item::NameTag => {
            let name = rest.join(" ");
            let name = name.trim();
            let chars = name.chars().count();
            if chars == 0 || chars > consts::NAME_MAX_CHARS {
                reply.reply(format!("名字要 1 到 {} 个字", consts::NAME_MAX_CHARS)).await?;
                return Ok(());
            }
            if !take_or_reply(&reply, &db, uin, item).await? {
                return Ok(());
            }
            logic::rename(&db, &mut horse, name).await?;
            let card = render::horse_card(&horse, &logic::owner_label(&db, uin).await?, &user.render_theme())?;
            reply.msg().image_bytes(card).text("改好名了").quote().await?;
        }
        Item::StarStone => {
            reply.reply("星辉石在繁殖时带:发「赛马繁殖 <公> <母> 星辉石」,下一胎必升一星").await?;
            return Ok(());
        }
        _ => {
            reply.reply("这个道具不在这里用").await?;
            return Ok(());
        }
    }

    // 真埋点:用掉养成珍材按回收价计入生涯投入(防白嫖造币);走到这必已用掉。
    if consts::Item::TREASURE.contains(&item) {
        logic::add_invested(&db, &mut horse, item.sell_price()).await?;
    }

    award_achievements(&reply, &mut user).await?;
    Ok(())
}

/// `赛马治疗 <编号>` —— 花币立即治好受伤的马。
#[command(
    "赛马治疗",
    description = "花币治好受伤的马",
    usage = "发「赛马治疗 <编号>」,花游戏币立即治愈伤病(伤越重越贵);也可以不治、等它自然养好。"
)]
async fn heal(reply: Reply, mut user: AUser, args: ArgText) -> HandlerResult {
    let Some(id) = parse_id(&args.0) else {
        reply.reply("发「赛马治疗 <编号>」").await?;
        return Ok(());
    };
    let db = user.db().clone();
    let Some(mut horse) = owned_horse(&reply, &db, user.uin(), id).await? else { return Ok(()) };
    logic::settle_state(&db, &mut horse).await?;
    if !logic::is_injured(&horse) {
        reply.reply("这马没受伤,不用治").await?;
        return Ok(());
    }
    let cost = logic::heal_cost(&horse);
    let was = logic::injury_name(horse.injury);
    if !user.pay(cost, "赛马·治疗").await? {
        reply.reply(format!("治{was}得花点币,你余额不够")).await?;
        return Ok(());
    }
    logic::heal(&db, &mut horse).await?;
    // 真埋点:治疗花的币计入生涯投入。
    logic::add_invested(&db, &mut horse, cost).await?;
    reply.reply(format!("治好了{was};但留下伤痕,一段时间内易复发、属性略降,歇几场再上更稳")).await?;
    Ok(())
}

/// `赛马商店 [道具] [数量]` —— 用金币直购养成珍材(养成材料的主获取路线)。
#[command(
    "赛马商店",
    description = "金币直购养成珍材",
    usage = "发「赛马商店」看珍材价目;「赛马商店 <道具> [数量]」直购\
(育骨精料/洗髓草/特性秘传/静心符/星辉石/红绳/续种符/染色剂)。"
)]
async fn shop(reply: Reply, mut user: AUser, session: Session, args: ArgText) -> HandlerResult {
    let mut it = args.0.split_whitespace();
    let Some(word) = it.next() else {
        // 目录
        let mut lines = String::from("赛马商店 · 养成珍材(发「赛马商店 <道具> [数量]」购买)\n");
        for item in Item::TREASURE {
            if let Some(p) = item.shop_price() {
                lines.push_str(&format!("· {} {p}币 —— {}\n", item.name(), item.effect_desc()));
            }
        }
        lines.push_str(&format!("你的余额:{} 币", user.coin()));
        reply.reply(lines).await?;
        return Ok(());
    };
    let Some(item) = Item::parse(word).filter(|i| i.shop_price().is_some()) else {
        reply.reply("没这件商品,发「赛马商店」看目录").await?;
        return Ok(());
    };
    let qty = it.next().and_then(|s| s.parse::<i32>().ok()).unwrap_or(1).clamp(1, consts::ITEM_STACK_CAP);
    // 同人单飞:扣币 + 入袋全程串行,挡并发重复购买。
    let Some(_guard) = session.single_flight_user() else {
        reply.reply("上一条还在处理,稍等").await?;
        return Ok(());
    };
    let price = item.shop_price().unwrap();
    let total = price * qty as i64;
    let db = user.db().clone();
    if !user.pay(total, "赛马·商店").await? {
        reply.reply(format!("买 {}×{qty} 要 {total} 币,你只有 {},不够", item.name(), user.coin())).await?;
        return Ok(());
    }
    // 入袋(夹堆叠上限);溢出按原价全额退,避免高价珍材撞上限被贱卖。
    let overflow = logic::add_item(&db, user.uin(), item, qty).await?;
    if overflow > 0 {
        user.add_coin(overflow as i64 * price, "赛马·商店超量退款").await?;
    }
    let got = qty - overflow;
    reply.reply(format!("买了 {}×{got},花 {} 币", item.name(), price * got as i64)).await?;
    Ok(())
}

/// `赛马出售 [道具] [数量]` —— 把背包里任意道具按回收价(基准价的 [`SELL_RATE`](consts::SELL_RATE))折成金币。
#[command(
    "赛马出售",
    description = "把道具回收成金币",
    usage = "发「赛马出售」看背包各道具的回收价;「赛马出售 <道具> [数量]」按回收价折成金币\
(用不上的道具、重复的珍材都能换钱;商店买回的不划算)。"
)]
async fn sell(reply: Reply, mut user: AUser, session: Session, args: ArgText) -> HandlerResult {
    let db = user.db().clone();
    let mut it = args.0.split_whitespace();
    let Some(word) = it.next() else {
        // 目录:列背包里可回收的道具与单价 + 全部回收的合计
        let bag = logic::backpack(&db, user.uin()).await?;
        if bag.is_empty() {
            reply.reply("背包是空的,没东西可回收").await?;
            return Ok(());
        }
        let mut lines = String::from("赛马出售 · 回收价(发「赛马出售 <道具> [数量]」)\n");
        let mut total = 0i64;
        for (item, qty) in &bag {
            let unit = item.sell_price();
            total += unit * *qty as i64;
            lines.push_str(&format!("· {}×{qty}　{unit} 币/个\n", item.name()));
        }
        lines.push_str(&format!("全部回收可得 {total} 币"));
        reply.reply(lines).await?;
        return Ok(());
    };
    let Some(item) = Item::parse(word) else {
        reply.reply("没这件道具,发「赛马出售」看背包").await?;
        return Ok(());
    };
    let qty = it.next().and_then(|s| s.parse::<i32>().ok()).unwrap_or(1).max(1);
    // 同人单飞:扣道具 + 加币全程串行,挡并发重复出售。
    let Some(_guard) = session.single_flight_user() else {
        reply.reply("上一条还在处理,稍等").await?;
        return Ok(());
    };
    if !logic::take_item(&db, user.uin(), item, qty).await? {
        reply.reply(format!("你没有 {qty} 个{},发「赛马背包」看持有", item.name())).await?;
        return Ok(());
    }
    let gain = item.sell_price() * qty as i64;
    user.add_coin(gain, "赛马·出售").await?;
    reply.reply(format!("回收 {}×{qty},得 {gain} 币", item.name())).await?;
    Ok(())
}

/// `赛马榜 [赛季/胜率]` —— 生涯胜场榜 / 本赛季榜 / 胜率榜。
#[command(
    "赛马榜",
    description = "看赛马排行榜",
    usage = "发「赛马榜」看生涯胜场榜;「赛马榜 赛季」看本月赛季榜(每月清零,新人也有机会);\
「赛马榜 胜率」看胜率榜(出战够多场才上)。"
)]
async fn rank(reply: Reply, user: AUser, args: ArgText) -> HandlerResult {
    let db = user.db().clone();
    let mode = args.0.trim();
    let (title, horses): (&str, Vec<entity::horse::Model>) = if mode.starts_with("赛季") {
        ("赛马 · 本赛季榜", logic::top_horses_season(&db, consts::RANK_TOP).await?)
    } else if mode.starts_with("胜率") {
        ("赛马 · 胜率榜", logic::top_horses_winrate(&db, consts::RANK_TOP as usize).await?)
    } else {
        ("赛马 · 胜场榜", logic::top_horses(&db, consts::RANK_TOP).await?)
    };
    let uins: Vec<i64> = horses.iter().map(|h| h.owner_uin).collect();
    let names = logic::owner_names(&db, &uins).await?;
    let rows: Vec<render::RankRow> = horses
        .iter()
        .map(|h| {
            let stat = if mode.starts_with("赛季") {
                format!("本赛季 {} 胜", h.season_wins)
            } else if mode.starts_with("胜率") {
                let pct = if h.races > 0 { h.wins as f32 / h.races as f32 * 100.0 } else { 0.0 };
                format!("胜率 {pct:.0}%({} 战)", h.races)
            } else {
                format!("胜 {} / {} 场", h.wins, h.races)
            };
            render::RankRow {
                horse: h.name.clone(),
                rarity: h.rarity,
                owner: names.get(&h.owner_uin).cloned().unwrap_or_else(|| "玩家".into()),
                stat,
            }
        })
        .collect();
    let card = render::rank_card(title, &rows, &user.render_theme())?;
    reply.msg().image_bytes(card).quote().await?;
    Ok(())
}

/// PvP 一名参赛者(房内态)。
struct Entrant {
    uin: i64,
    horse: entity::horse::Model,
    items: Vec<Item>,
}

/// 校验一匹马能否参赛(本人名下、未退役、无伤、体力够);不行则回原因、返 `None`。`m` 结算后返回。
async fn pvp_validate(
    reply: &Reply,
    db: &sea_orm::DatabaseConnection,
    uin: i64,
    id: i64,
) -> HandlerResult2<entity::horse::Model> {
    let Some(mut horse) = owned_horse(reply, db, uin, id).await? else { return Ok(None) };
    if horse.status == 2 {
        reply.reply("这匹马退役了,上不了场").await?;
        return Ok(None);
    }
    logic::settle_state(db, &mut horse).await?;
    if logic::is_injured(&horse) {
        reply.reply(format!("「{}」还带着伤,先治好", horse.name)).await?;
        return Ok(None);
    }
    if horse.vitality < consts::VIT_RACE {
        reply.reply(format!("「{}」体力不够({}/{})", horse.name, horse.vitality, consts::VIT_MAX)).await?;
        return Ok(None);
    }
    Ok(Some(horse))
}

/// PvP 大厅公示用的「战力名片」:★ + 五维,让报名/旁注者看清对手强弱。
fn horse_power_label(h: &entity::horse::Model) -> String {
    let s = logic::stats_of(h); // 点数(列存厘点)
    format!("★{} 速{} 耐{} 爆{} 敏{} 运{}", h.rarity, s[0], s[1], s[2], s[3], s[4])
}

/// 带闸取出要带的比赛道具(没有的略过),返回实际取到的。
async fn take_race_items(db: &sea_orm::DatabaseConnection, uin: i64, want: &[Item]) -> nagisa::Result<Vec<Item>> {
    let mut used = Vec::new();
    for &it in want {
        if logic::take_item(db, uin, it, 1).await? {
            used.push(it);
        }
    }
    Ok(used)
}

/// 一条消息的纯文本(拼所有文本段)。
fn msg_text(m: &MessageEvent) -> String {
    m.content.iter().filter_map(|s| s.as_text()).collect::<Vec<_>>().join("")
}

/// 解析「注额 + 道具」(顺序无关),用于开房/报名尾部。
fn parse_stake_items(toks: std::str::SplitWhitespace<'_>) -> (Option<i64>, Vec<Item>) {
    let (mut stake, mut items) = (None, Vec::new());
    for t in toks {
        if let Ok(n) = t.parse::<i64>() {
            stake = Some(n.clamp(consts::PVP_STAKE_MIN, consts::PVP_STAKE_MAX));
        } else if let Some(it) = Item::parse(t)
            && it.kind() == ItemKind::Race
            && items.len() < consts::MAX_RACE_ITEMS
        {
            items.push(it);
        }
    }
    (stake, items)
}

/// `赛马开房 <编号> [注额] [道具]` —— 群内开一局 PvP,收报名、下注池抽水零和结算。
#[command(
    "赛马开房",
    description = "群内开一局 PvP 对战",
    usage = "在群里发「赛马开房 <你的马号> [注额] [道具]」开一局(注额不填给个默认值);别人发「赛马报名 <他的马号> [道具]」\
加入(下相同的注),房主发「赛马开跑」开始、「赛马散场」取消。奖池按名次分给前几名(抽一点水)。\
围观的可发「赛马押 <参赛马号> <注额>」旁注,押中冠军的人按注额比例分旁注池。"
)]
async fn pvp_open(reply: Reply, mut user: AUser, session: Session, args: ArgText) -> HandlerResult {
    if !reply.peer().is_group() {
        reply.reply("PvP 要在群里开").await?;
        return Ok(());
    }
    let peer = *reply.peer();
    let Some(_room) = session.single_flight(Scope::peer(peer)) else {
        reply.reply("本群已经有一局在开了,发「赛马报名 <你的马号>」加入").await?;
        return Ok(());
    };

    let mut it = args.0.split_whitespace();
    let Some(id) = it.next().and_then(|s| s.parse::<i64>().ok()) else {
        reply.reply("发「赛马开房 <你的马号> [注额] [道具]」").await?;
        return Ok(());
    };
    let (stake, want_items) = parse_stake_items(it);
    let stake = stake.unwrap_or(consts::PVP_STAKE_DEFAULT);

    let db = user.db().clone();
    let Some(host_horse) = pvp_validate(&reply, &db, user.uin(), id).await? else { return Ok(()) };
    if !user.pay(stake, "赛马·下注").await? {
        reply.reply("你的游戏币不够下注").await?;
        return Ok(());
    }
    let host_items = take_race_items(&db, user.uin(), &want_items).await?;
    let mut entrants = vec![Entrant { uin: user.uin(), horse: host_horse, items: host_items }];
    // 旁注 (uin, 押的马号, 注额):非参赛者押某匹参赛马,赛后按 parimutuel 分旁注池。
    let mut side_bets: Vec<(i64, i64, i64)> = Vec::new();
    reply
        .reply(format!(
            "🏇 开了一局赛马!房主马 #{} {}。注额 {stake} 币。\n发「赛马报名 <你的马号> [道具]」加入(也会亮出你的马),房主发「赛马开跑」开始、「赛马散场」取消。\n围观的也能玩:发「赛马押 <参赛马号> <注额>」押你看好的马。",
            entrants[0].horse.id,
            horse_power_label(&entrants[0].horse)
        ))
        .await?;

    // 群级 waiter 收报名/开跑/散场;block(false) 只观察、不吞掉群里其它命令。
    let waiter = session.waiter().scope(Scope::peer(peer)).block(false).build();
    enum End {
        Start,
        Cancel,
        Timeout,
    }
    // 绝对截止:recv 每次调用各自计时,活跃群里逐条消息会无限续命;按固定 deadline 算剩余,到点即超时
    // 关房,否则 single_flight 房锁可能长期占住本群。
    let deadline = std::time::Instant::now() + Duration::from_secs(consts::PVP_LOBBY_TIMEOUT_SECS);
    let end = loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            break End::Timeout;
        }
        let Some(gm) = waiter.recv::<GroupMessage>(remaining).await else {
            break End::Timeout;
        };
        let sender = gm.0.sender.0;
        let text = msg_text(&gm.0);
        let text = text.trim();

        if let Some(rest) = text.strip_prefix("赛马报名") {
            if entrants.iter().any(|e| e.uin == sender) {
                reply.reply("你已经在这局里了").await?;
                continue;
            }
            if side_bets.iter().any(|b| b.0 == sender) {
                reply.reply("你已经旁注押注了,不能再下场跑(押注和参赛二选一)").await?;
                continue;
            }
            let mut toks = rest.split_whitespace();
            let Some(hid) = toks.next().and_then(|s| s.parse::<i64>().ok()) else {
                reply.reply("发「赛马报名 <你的马号> [道具]」").await?;
                continue;
            };
            let (_, jitems) = parse_stake_items(toks);
            let Some(jhorse) = pvp_validate(&reply, &db, sender, hid).await? else { continue };
            let mut juser = AUser::get(&db, sender).await?;
            if !juser.pay(stake, "赛马·下注").await? {
                reply.reply("你的游戏币不够下注").await?;
                continue;
            }
            let used = take_race_items(&db, sender, &jitems).await?;
            let label = horse_power_label(&jhorse);
            let hid_shown = jhorse.id;
            entrants.push(Entrant { uin: sender, horse: jhorse, items: used });
            let n = entrants.len();
            reply.msg().at(Uin(sender)).text(format!(" 的马加入了(#{hid_shown} {label}),当前 {n} 人")).send().await?;
            if entrants.len() >= consts::PVP_ROOM_CAP {
                break End::Start;
            }
        } else if let Some(rest) = text.strip_prefix("赛马押") {
            if entrants.iter().any(|e| e.uin == sender) {
                reply.reply("你在场上跑,不用旁注").await?;
                continue;
            }
            if side_bets.iter().any(|b| b.0 == sender) {
                reply.reply("你已经押过这局了").await?;
                continue;
            }
            let mut toks = rest.split_whitespace();
            let (Some(tid), Some(amt)) =
                (toks.next().and_then(|s| s.parse::<i64>().ok()), toks.next().and_then(|s| s.parse::<i64>().ok()))
            else {
                reply.reply("发「赛马押 <参赛马号> <注额>」").await?;
                continue;
            };
            let amt = amt.clamp(consts::PVP_STAKE_MIN, consts::PVP_STAKE_MAX);
            if !entrants.iter().any(|e| e.horse.id == tid) {
                reply.reply("没有这匹参赛马,押注得选场上的马号").await?;
                continue;
            }
            let mut bu = AUser::get(&db, sender).await?;
            if !bu.pay(amt, "赛马·旁注").await? {
                reply.reply("你的游戏币不够押注").await?;
                continue;
            }
            side_bets.push((sender, tid, amt));
            reply.msg().at(Uin(sender)).text(format!(" 押了 {amt} 在 #{tid}")).send().await?;
        } else if text == "赛马开跑" && sender == user.uin() {
            if entrants.len() >= 2 {
                break End::Start;
            }
            reply.reply("至少要 2 匹马才能开跑,再等等").await?;
        } else if text == "赛马散场" && sender == user.uin() {
            break End::Cancel;
        }
    };

    if !matches!(end, End::Start) {
        // 退注 + 退道具 + 退旁注。
        for e in &entrants {
            let mut u = AUser::get(&db, e.uin).await?;
            if stake > 0 {
                u.add_coin(stake, "赛马·退注").await?;
            }
            for &it in &e.items {
                logic::add_item(&db, e.uin, it, 1).await?;
            }
        }
        for &(buin, _, amt) in &side_bets {
            let mut bu = AUser::get(&db, buin).await?;
            bu.add_coin(amt, "赛马·退旁注").await?;
        }
        reply
            .reply(if matches!(end, End::Cancel) {
                "房主散场,注额已退"
            } else {
                "等太久没开成,这局取消、注额已退"
            })
            .await?;
        return Ok(());
    }

    // 开跑前重拉最新态并重校验:PvP 是 peer 锁,报名到开跑期间主人可能在别处(user 锁)把马练/赛/治/退役过,
    // 报名时的快照可能已过期。任一匹变退役/受伤/体力不足,就退还全部注额、道具、旁注并取消本局。
    for e in entrants.iter_mut() {
        if let Some(mut fresh) = logic::get_horse(&db, e.horse.id).await? {
            logic::settle_state(&db, &mut fresh).await?;
            e.horse = fresh;
        }
    }
    if let Some(bad) = entrants.iter().find_map(|e| {
        let reason = if e.horse.status == 2 {
            "退役了"
        } else if logic::is_injured(&e.horse) {
            "受伤了"
        } else if e.horse.vitality < consts::VIT_RACE {
            "体力不够"
        } else {
            return None;
        };
        Some(format!("「{}」{reason}", e.horse.name))
    }) {
        for e in &entrants {
            let mut u = AUser::get(&db, e.uin).await?;
            if stake > 0 {
                u.add_coin(stake, "赛马·退注").await?;
            }
            for &it in &e.items {
                logic::add_item(&db, e.uin, it, 1).await?;
            }
        }
        for &(buin, _, amt) in &side_bets {
            let mut bu = AUser::get(&db, buin).await?;
            bu.add_coin(amt, "赛马·退旁注").await?;
        }
        reply.reply(format!("开跑前有马状态变了({bad}),这局取消、注额与旁注已全退")).await?;
        return Ok(());
    }

    // 奖池 + 抽水,派彩按名次分。
    let pool = stake * entrants.len() as i64;
    let rake = (pool as f32 * consts::PVP_RAKE).round() as i64;
    let payout = pool - rake;
    let theme = user.render_theme();
    let seed = rand::random::<u64>();
    let entrant_uins: Vec<i64> = entrants.iter().map(|e| e.uin).collect();
    let owner_map = logic::owner_names(&db, &entrant_uins).await?;
    let pvp_entrants: Vec<race::PvpEntrant> = entrants
        .iter()
        .map(|e| race::PvpEntrant {
            info: race::RunnerInfo {
                name: e.horse.name.clone(),
                owner: owner_map.get(&e.uin).cloned().unwrap_or_default(),
                color: e.horse.color,
                is_npc: false,
            },
            stats: condition_stats(&e.horse),
            traits: e.horse.traits,
            items: e.items.clone(),
            life_frac: logic::life_ratio(&e.horse) as f64,
            scar: e.horse.scar,
            races: e.horse.races,
        })
        .collect();
    let result = Arc::new(race::simulate_pvp(pvp_entrants, consts::PVP_TRACK_LEN, seed));

    // 实况:挑几个关键节点播报。
    reply.reply("🏇 开跑!").await?;
    for (i, &round) in pick_frames(&result.key_rounds, LIVE_FRAMES).iter().enumerate() {
        if i > 0 {
            tokio::time::sleep(LIVE_FRAME_GAP).await;
        }
        reply.msg().image_bytes(render::race_frame(&result, round, &theme)?).send().await?;
    }

    // 每匹马结算(扣体力/扣寿命/判伤)。伤病在比赛内核局内已判定,这里只取本场最坏伤等落库。
    let winner_idx = result.order[0];
    let mut rng = fresh_rng();
    for (i, e) in entrants.iter_mut().enumerate() {
        logic::finish_race(&db, &mut e.horse, i == winner_idx).await?;
        let sev = result.injuries[i];
        if sev > 0 {
            logic::set_injury(&db, &mut e.horse, sev).await?;
        }
        // 赛后掉落(幸运产出维):PvP 掉率减半(压"互刷变现");命中入袋,溢出折币。
        if let Some(it) = logic::roll_drop(
            logic::stats_of(&e.horse)[Stat::Luk.idx()],
            consts::Trait::Fortuitous.in_mask(e.horse.traits),
            consts::PVP_DROP_MULT,
            &mut rng,
        ) {
            let overflow = logic::add_item(&db, e.uin, it, 1).await?;
            if overflow > 0 {
                let mut du = AUser::get(&db, e.uin).await?;
                du.add_coin(overflow as i64 * it.sell_price(), "赛马·掉落折币").await?;
            }
        }
    }
    // 每日首胜:冠军原子领取(跨 PvP/PvE 只发一次)。
    let champ_first_win =
        logic::claim_first_win_today(&db, entrants[winner_idx].uin, entrants[winner_idx].horse.id).await?;
    // 派彩:按名次系数分奖池,前三(不足则全员)分,取整零头归冠军。
    let mut shares = vec![0i64; result.order.len()];
    if payout > 0 {
        let places = result.order.len().min(consts::PVP_PAYOUT_FACTOR.len());
        let mut handed = 0i64;
        for (rank, &factor) in consts::PVP_PAYOUT_FACTOR.iter().enumerate().take(places) {
            let amount = (payout as f32 * factor).round() as i64;
            shares[rank] = amount;
            handed += amount;
        }
        shares[0] += payout - handed; // 取整零头归冠军,保证派彩恰为 payout
        for (rank, &order_idx) in result.order.iter().enumerate() {
            if shares[rank] > 0 {
                let mut wu = AUser::get(&db, entrants[order_idx].uin).await?;
                wu.add_coin(shares[rank], "赛马·赢注").await?;
            }
        }
    }

    reply.msg().image_bytes(render::pvp_result_card(&result, &shares, rake, &theme)?).send().await?;
    if champ_first_win {
        let mut wu = AUser::get(&db, entrants[winner_idx].uin).await?;
        wu.add_coin(consts::DAILY_FIRST_WIN_BONUS, "赛马·每日首胜").await?;
        reply.msg().at(Uin(entrants[winner_idx].uin)).text(" 今日首胜,有额外奖励").send().await?;
    }

    // 旁注 parimutuel 结算:押中冠军的人按注额比例分旁注池(扣抽水);无人押中则全额退还、不抽水。
    if !side_bets.is_empty() {
        let winner_id = entrants[winner_idx].horse.id;
        let winners: Vec<&(i64, i64, i64)> = side_bets.iter().filter(|b| b.1 == winner_id).collect();
        let win_stake: i64 = winners.iter().map(|b| b.2).sum();
        if win_stake == 0 {
            for &(buin, _, amt) in &side_bets {
                let mut bu = AUser::get(&db, buin).await?;
                bu.add_coin(amt, "赛马·旁注退还").await?;
            }
            reply.reply("旁注没人押中冠军,旁注已全额退还").await?;
        } else {
            let side_pool: i64 = side_bets.iter().map(|b| b.2).sum();
            let side_rake = (side_pool as f32 * consts::PVP_RAKE).round() as i64;
            let side_payout = side_pool - side_rake;
            let mut handed = 0i64;
            for (i, b) in winners.iter().enumerate() {
                // 末位拿零头,保证派彩恰为 side_payout。
                let share = if i + 1 == winners.len() {
                    side_payout - handed
                } else {
                    (side_payout as i128 * b.2 as i128 / win_stake as i128) as i64
                };
                handed += share;
                let mut bu = AUser::get(&db, b.0).await?;
                bu.add_coin(share, "赛马·旁注赢").await?;
            }
            reply
                .reply(format!(
                    "旁注派彩:押中冠军「{}」的 {} 人分了旁注池",
                    entrants[winner_idx].horse.name,
                    winners.len()
                ))
                .await?;
        }
    }

    let gif = replay::render(result.clone()).await?;
    reply.msg().image_bytes(gif).send().await?;

    // 冠军未必是房主,单独取句柄评估成就。
    let mut wu = AUser::get(&db, entrants[winner_idx].uin).await?;
    award_achievements(&reply, &mut wu).await?;
    Ok(())
}
