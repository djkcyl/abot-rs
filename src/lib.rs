//! abot 库 crate —— 模块在此聚合，供 `main.rs`（二进制）使用。
//!
//! abot 只依赖 `nagisa` 门面（onebot + log 特性），数据走 sea-orm + Postgres。
//! （abot 不写测试:门禁 = build + clippy,验证靠真机连线跑。）

// 各模块自带 `//!` 文档;这里不再叠外层简介——rustdoc 会把两层拼接后按本作用域解析
// 内层链接,既报「不在 scope」又丢行号。
pub mod config;
pub mod data;
pub mod fonts;
pub mod imaging;
pub mod integrations;
pub mod plugins;
pub mod web;
