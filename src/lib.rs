//! abot 库 crate —— 模块在此聚合，供 `main.rs`（二进制）使用。
//!
//! abot 只依赖 `nagisa` 门面（onebot + log 特性），数据走 sea-orm + Postgres。
//! （abot 不写测试:门禁 = build + clippy,验证靠真机连线跑。）

pub mod config;
pub mod data;
pub mod plugins;

/// 游戏货币名 —— **整个 abot 的全局设计常量**，单一来源。
///
/// 所有涉及货币的插件（签到 / 赠送 / 排行 / 赛马 …）一律引用 `crate::COIN_NAME`，
/// 不在各自模块里重复定义；改名只此一处。
/// （如需运行期可配，后续可提升进 [`config::Config`] 从环境读取，调用点不变。）
pub const COIN_NAME: &str = "游戏币";
