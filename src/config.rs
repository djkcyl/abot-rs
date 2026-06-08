//! 运行配置 —— **只**从环境变量读取（经 `dotenvy` 在 `main` 里加载 `.env`）。
//!
//! 设计上**没有**文件型配置（无 `config.toml` / `config.json`）：所有可调项都是
//! 进程环境变量，缺省值对准本机的活体 Postgres / Lagrange，不改配就能连。
//!
//! - `DATABASE_URL` —— sea-orm 连接串（**必须是 Postgres**，缺省指向本机 `abot` 库）。
//! - `ONEBOT_WS_URL` —— Lagrange 正向 WS 端点（应用作为客户端拨入）。
//! - `MASTER` —— 机器人主人（owner）的 QQ 号；启动就绪后给他私聊发上线通知,且**总是**
//!   被并入超管集。缺省即作者本人；设 `0` 表示「无主人」（不发通知、不并超管）。
//! - `SUPERUSERS` —— 逗号分隔的 QQ 号清单，喂给 `superuser()` 规则；空/缺失即只有 master。

use nagisa::Uin;

/// 默认 Postgres DSN：本机 `abot` 库（与 `.env` / 活体环境一致）。
const DEFAULT_DATABASE_URL: &str = "postgres://abot:abot@127.0.0.1:5432/abot";
/// 默认 OneBot 正向 WS 端点：本机 Lagrange。
const DEFAULT_ONEBOT_WS_URL: &str = "ws://127.0.0.1:41573/onebot/v11/ws";
/// 默认主人 QQ。`0` 表示无主人；经 `MASTER` 环境变量设成你自己的 QQ 号。
const DEFAULT_MASTER: i64 = 0;

/// 注入用的「主人」句柄：由 `main` 经 `App::data(Master(cfg.master))` 注入，供
/// `#[event(Ready)]` 等 handler 经 `State<Master>` 取用。`Uin(0)` 表示无主人。
#[derive(Debug, Clone, Copy)]
pub struct Master(pub Uin);

/// 进程级运行配置。由 [`Config::from_env`] 自环境变量装配。
#[derive(Debug, Clone)]
pub struct Config {
    /// sea-orm 连接串（Postgres）。
    pub database_url: String,
    /// OneBot v11 正向 WS 端点。
    pub onebot_ws: String,
    /// 机器人主人（owner）的 QQ 号；`Uin(0)` 表示无主人（不发上线通知、不并超管）。
    pub master: Uin,
    /// 超级用户 QQ 号清单（喂给 `superuser()` 规则）。**总是**含 `master`（若非 0）。
    pub superusers: Vec<Uin>,
}

impl Config {
    /// 从环境变量装配配置（`.env` 由 `main` 经 `dotenvy` 预先加载）。
    ///
    /// 缺失项回落到本机默认；`SUPERUSERS` 解析失败的条目被静默跳过（不让一个脏号
    /// 拖垮整个启动）。`master`（非 0）始终并入 `superusers`——主人当然是超管。
    pub fn from_env() -> Self {
        let database_url =
            std::env::var("DATABASE_URL").unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_string());
        let onebot_ws =
            std::env::var("ONEBOT_WS_URL").unwrap_or_else(|_| DEFAULT_ONEBOT_WS_URL.to_string());
        let master = Uin(std::env::var("MASTER")
            .ok()
            .and_then(|s| s.trim().parse::<i64>().ok())
            .unwrap_or(DEFAULT_MASTER));
        let mut superusers = std::env::var("SUPERUSERS")
            .map(|s| parse_superusers(&s))
            .unwrap_or_default();
        // 主人恒为超管：非 0 且尚未在清单里则并入。
        if master.0 != 0 && !superusers.contains(&master) {
            superusers.push(master);
        }
        Self { database_url, onebot_ws, master, superusers }
    }
}

/// 解析逗号分隔的超管清单。空白条目跳过；非整数条目跳过（脏号不应阻断启动）。
fn parse_superusers(raw: &str) -> Vec<Uin> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse::<i64>().ok())
        .map(Uin)
        .collect()
}
