//! abot 库 crate —— 模块在此聚合，供 `main.rs`（二进制）使用。
//!
//! abot 只依赖 `nagisa` 门面（onebot + log 特性），数据走 sea-orm + Postgres。
//! （abot 不写测试:门禁 = build + clippy,验证靠真机连线跑。）

pub mod config;
pub mod data;

/// 出图字体栈:框架内置黑体/等宽 + abot 自备宋体/楷体(详见 `fonts::handle`)。
pub mod fonts;

/// 顶层图片缓存服务:收图登记 + 队列下载 + 分片归档,插件经 `wait` 等图就绪(详见 `media`)。
pub mod media;

/// crate 级内容审核器:腾讯云 AI 主、本地关键词/二维码兜底(详见 `moderation::ContentModerator`)。
pub mod moderation;

pub mod plugins;

/// 插件 WebUI 地基:进程内 axum 控制台(详见 `web::ConsoleService`)。
pub mod web;

/// 游戏货币名 —— **整个 abot 的全局设计常量**，单一来源。
///
/// 所有涉及货币的插件（签到 / 赠送 / 排行 / 赛马 …）一律引用 `crate::COIN_NAME`，
/// 不在各自模块里重复定义；改名只此一处。
/// （如需运行期可配，后续可提升进 [`config::Config`] 从环境读取，调用点不变。）
pub const COIN_NAME: &str = "游戏币";
