//! OverviewProvider —— 总览页的实时身份与统计(RPC `overview`,authority 1)。
//! 一次调用聚合机器人身份(uin/昵称/头像/在线/版本/在线时长/收发计数)与若干计数
//! (注册用户、消息总数、今日消息、群数、好友数)。每个远程调用各自兜底,单点失败不拖垮整体:
//! 拿不到的字段退回 null / 0。

use nagisa::async_trait;
use nagisa::Bot;
use sea_orm::{ConnectionTrait, DatabaseConnection, Statement};
use serde_json::{json, Value};
use std::sync::Arc;

use crate::web::registry::{
    AuthUser, ConsoleContext, ConsolePlugin, ConsolePluginCtor, ConsoleRegistry, WebListener,
};

pub struct OverviewProvider {
    bot: Bot,
    db: DatabaseConnection,
    boot: chrono::DateTime<chrono::Utc>,
}

impl ConsolePlugin for OverviewProvider {
    fn register(self: Arc<Self>, reg: &mut ConsoleRegistry) {
        reg.add_listener(Box::new(Overview(self)));
    }
}

/// 跑一句返回单列 `n`(i64)的 count，失败或无行 → 0。
async fn count_scalar(db: &DatabaseConnection, sql: &str) -> i64 {
    let stmt = Statement::from_string(db.get_database_backend(), sql.to_string());
    match db.query_one(stmt).await {
        Ok(Some(row)) => row.try_get::<i64>("", "n").unwrap_or(0),
        _ => 0,
    }
}

/// 近 30 天每天的消息量。只返回有行的那些天(前端补零拉满 30 天轴);失败或无行 → 空。
async fn daily_messages(db: &DatabaseConnection) -> Vec<Value> {
    let stmt = Statement::from_string(
        db.get_database_backend(),
        "SELECT to_char(time, 'YYYY-MM-DD') AS d, count(*) AS n FROM chat_log \
         WHERE time >= now() - interval '30 days' GROUP BY d ORDER BY d"
            .to_string(),
    );
    let rows = match db.query_all(stmt).await {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };
    rows.iter()
        .filter_map(|r| {
            let date = r.try_get::<String>("", "d").ok()?;
            let count = r.try_get::<i64>("", "n").unwrap_or(0);
            Some(json!({ "date": date, "count": count }))
        })
        .collect()
}

struct Overview(Arc<OverviewProvider>);

#[async_trait]
impl WebListener for Overview {
    fn event(&self) -> &'static str {
        "overview"
    }
    fn authority(&self) -> u8 {
        1
    }
    async fn handle(&self, _args: Value, _who: AuthUser) -> Result<Value, String> {
        let p = &self.0;
        let bot = &p.bot;
        let uin = bot.self_id().0;

        // 机器人身份与协议端状态:逐项兜底,拿不到退回 null / false。
        let nickname = bot.get_login_info().await.ok().map(|(_, n)| n);
        let avatar = format!("https://q.qlogo.cn/g?b=qq&nk={uin}&s=640");
        let status = bot.get_status().await.ok();
        let online = status.as_ref().map(|s| s.online).unwrap_or(false);
        let stat = status.as_ref().and_then(|s| s.stat.as_ref());
        let msg_received = stat.and_then(|s| s.message_received);
        let msg_sent = stat.and_then(|s| s.message_sent);
        let version = bot
            .get_impl_info()
            .await
            .ok()
            .map(|i| format!("{} {}", i.name, i.version));
        let uptime_secs = (chrono::Utc::now() - p.boot).num_seconds().max(0);

        // 统计计数:DB 计数 + 群/好友数(远程,失败退 0)。
        let users = count_scalar(&p.db, "SELECT count(*) AS n FROM \"user\"").await;
        let total_messages = count_scalar(&p.db, "SELECT count(*) AS n FROM chat_log").await;
        let today_messages = count_scalar(
            &p.db,
            "SELECT count(*) AS n FROM chat_log WHERE time >= date_trunc('day', now())",
        )
        .await;
        let groups = bot.get_group_list(true).await.map(|v| v.len()).unwrap_or(0);
        let friends = bot.get_friend_list(true).await.map(|v| v.len()).unwrap_or(0);
        let daily = daily_messages(&p.db).await;

        Ok(json!({
            "bot": {
                "uin": uin,
                "nickname": nickname,
                "avatar": avatar,
                "online": online,
                "version": version,
                "uptime_secs": uptime_secs,
                "msg_received": msg_received,
                "msg_sent": msg_sent,
            },
            "stats": {
                "users": users,
                "total_messages": total_messages,
                "today_messages": today_messages,
                "groups": groups,
                "friends": friends,
            },
            "daily_messages": daily,
        }))
    }
}

nagisa::inventory::submit! {
    ConsolePluginCtor(|cx: &ConsoleContext| -> Arc<dyn ConsolePlugin> {
        Arc::new(OverviewProvider {
            bot: cx.bot.clone(),
            db: cx.db.clone(),
            boot: cx.boot,
        })
    })
}
