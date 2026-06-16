//! 对接第三方/外部服务的能力模块 —— 凡是要**向外联网**的都归这里,与纯本地的核心
//! (config / data / web / plugins / 渲染栈)分开,免得都摊在 crate 根上。
//!
//! - [`moderation`]:内容审核(腾讯云 TMS/IMS + 本地二维码闸),裁决入库兼缓存。
//! - [`media`]:图片缓存服务(从 QQ/外链下载、归档、按 md5 复用),bot 启动时装配。

pub mod media;
pub mod moderation;
