//! ContactsProvider —— 好友/群管理。三个 RPC 监听器（无 DataService：均为按需远程调用，
//! 不在每次连接时拉取）：`contacts/list`（好友+群清单）、`group/members`（群成员）、
//! `group/action`（禁言/踢人/改名片/头衔/管理员/全体禁言/退群）。
//! 写操作前往审计 target 打一条 warn，连同实时日志面板可见。

use nagisa::async_trait;
use nagisa::{Bot, FriendInfo, MemberInfo, Role, Uin};
use serde_json::{Value, json};
use std::sync::Arc;

use crate::web::registry::{AuthUser, ConsoleContext, ConsolePlugin, ConsolePluginCtor, ConsoleRegistry, WebListener};

pub struct ContactsProvider {
    bot: Bot,
}

impl ConsolePlugin for ContactsProvider {
    fn register(self: Arc<Self>, reg: &mut ConsoleRegistry) {
        reg.add_listener(Box::new(ContactsList(self.bot.clone())));
        reg.add_listener(Box::new(GroupMembers(self.bot.clone())));
        reg.add_listener(Box::new(GroupAction(self.bot.clone())));
    }
}

/// 角色枚举 → 字符串。
fn role_str(role: Role) -> &'static str {
    match role {
        Role::Owner => "owner",
        Role::Admin => "admin",
        Role::Member => "member",
    }
}

fn friend_json(f: &FriendInfo) -> Value {
    json!({
        "uin": f.user.0,
        "nickname": f.nickname,
        "remark": f.remark,
    })
}

fn member_json(m: &MemberInfo) -> Value {
    json!({
        "uin": m.user.0,
        "nickname": m.nickname,
        "card": m.card,
        "role": role_str(m.role),
        "level": m.level,
        "title": m.title,
        "join_time": m.join_time,
        "last_sent_time": m.last_sent_time.unwrap_or(0),
        "mute_end_time": m.mute_end_time,
    })
}

struct ContactsList(Bot);
#[async_trait]
impl WebListener for ContactsList {
    fn event(&self) -> &'static str {
        "contacts/list"
    }
    fn authority(&self) -> u8 {
        4
    }
    async fn handle(&self, _args: Value, _who: AuthUser) -> Result<Value, String> {
        let bot = &self.0;
        let friends = bot.get_friend_list(true).await.map_err(|e| e.to_string())?;
        let groups = bot.get_group_list(true).await.map_err(|e| e.to_string())?;

        // bot 在每个群里的角色:逐群并发查 get_group_member_info(g, self_id),失败的给 null。
        let self_id = bot.self_id();
        let roles =
            futures::future::join_all(groups.iter().map(|g| bot.get_group_member_info(g.group, self_id, false))).await;

        let groups_json: Vec<Value> = groups
            .iter()
            .zip(roles)
            .map(|(g, role)| {
                let bot_role = role.ok().map(|m| role_str(m.role));
                json!({
                    "group_id": g.group.0,
                    "name": g.name,
                    "member_count": g.member_count,
                    "owner_id": g.owner_id.map(|u| u.0),
                    "bot_role": bot_role,
                })
            })
            .collect();

        Ok(json!({
            "friends": friends.iter().map(friend_json).collect::<Vec<_>>(),
            "groups": groups_json,
        }))
    }
}

struct GroupMembers(Bot);
#[async_trait]
impl WebListener for GroupMembers {
    fn event(&self) -> &'static str {
        "group/members"
    }
    fn authority(&self) -> u8 {
        4
    }
    async fn handle(&self, args: Value, _who: AuthUser) -> Result<Value, String> {
        let bot = &self.0;
        let group = args.get("group").and_then(|v| v.as_i64()).ok_or("缺少 group")?;
        let members = bot.get_group_member_list(Uin(group), false).await.map_err(|e| e.to_string())?;
        // 全员禁言状态:取自 get_group_info 的 shut_up_all_time。Some(0)=已知未开、Some(非0)=已知开启、
        // None=协议端不回该字段(未知)。前端据此:已知给开关、未知退回显式按钮。
        let whole_muted =
            bot.get_group_info(Uin(group), false).await.ok().and_then(|g| g.shut_up_all_time).map(|t| t != 0);
        // bot 自身在该群的角色:从成员表里挑出 uin == self_id 的那条。
        let bot_uin = bot.self_id().0;
        let bot_role = members.iter().find(|m| m.user.0 == bot_uin).map(|m| role_str(m.role));
        Ok(json!({
            "members": members.iter().map(member_json).collect::<Vec<_>>(),
            "whole_muted": whole_muted,
            "bot_role": bot_role,
            "bot_uin": bot_uin,
        }))
    }
}

struct GroupAction(Bot);

impl GroupAction {
    /// 读必需的 user id（多数动作需要）。
    fn user_arg(args: &Value) -> Result<Uin, String> {
        args.get("user").and_then(|v| v.as_i64()).map(Uin).ok_or_else(|| "缺少 user".to_string())
    }
}

#[async_trait]
impl WebListener for GroupAction {
    fn event(&self) -> &'static str {
        "group/action"
    }
    fn authority(&self) -> u8 {
        4
    }
    async fn handle(&self, args: Value, who: AuthUser) -> Result<Value, String> {
        let action = args.get("action").and_then(|v| v.as_str()).ok_or("缺少 action")?;
        let group = args.get("group").and_then(|v| v.as_i64()).map(Uin).ok_or("缺少 group")?;

        let bot = &self.0;
        let result = match action {
            "mute" => {
                let user = Self::user_arg(&args)?;
                let duration = args.get("duration").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                bot.set_group_member_mute(group, user, duration).await
            }
            "kick" => {
                let user = Self::user_arg(&args)?;
                let reject_add = args.get("reject_add").and_then(|v| v.as_bool()).unwrap_or(false);
                bot.kick_group_member(group, user, reject_add).await
            }
            "card" => {
                let user = Self::user_arg(&args)?;
                let card = args.get("card").and_then(|v| v.as_str()).unwrap_or("");
                bot.set_group_member_card(group, user, card).await
            }
            "title" => {
                let user = Self::user_arg(&args)?;
                let title = args.get("title").and_then(|v| v.as_str()).unwrap_or("");
                bot.set_group_member_special_title(group, user, title, -1).await
            }
            "admin" => {
                let user = Self::user_arg(&args)?;
                let enable = args.get("enable").and_then(|v| v.as_bool()).ok_or("缺少 enable")?;
                bot.set_group_admin(group, user, enable).await
            }
            "whole_mute" => {
                let enable = args.get("enable").and_then(|v| v.as_bool()).ok_or("缺少 enable")?;
                bot.set_group_whole_mute(group, enable).await
            }
            "leave" => {
                if who.authority < 5 {
                    return Err("退群/解散仅限主人".to_string());
                }
                let dismiss = args.get("dismiss").and_then(|v| v.as_bool()).unwrap_or(false);
                bot.leave_group(group, dismiss).await
            }
            other => return Err(format!("未知动作：{other}")),
        };

        result.map_err(|e| e.to_string())?;
        // 审计：成功的写操作记一条，连同实时日志面板可见。
        tracing::warn!(target: "abot::web::audit", %action, group = group.0, "网页控制台群管理操作");
        Ok(json!({ "ok": true }))
    }
}

nagisa::inventory::submit! {
    ConsolePluginCtor(|cx: &ConsoleContext| -> Arc<dyn ConsolePlugin> {
        Arc::new(ContactsProvider { bot: cx.bot.clone() })
    })
}
