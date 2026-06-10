//! 出图字体栈 —— 框架只内置黑体 + 等宽（crates.io 包体上限装不下更多），思源宋体与
//! 霞鹜文楷（楷体三档）是 abot 自备的 zstd 压缩资产，在这里合成共享 [`FontHandle`]。
//! `data()` 按 zstd 魔数自动解压，无需额外依赖；句柄构建一次全局复用（构建含解压，较贵）。
//!
//! 出图插件用法：`RenderOptions::default().with_fonts(fonts::handle())`。

use std::sync::OnceLock;

use nagisa::render::FontHandle;

/// abot 自备字体（zstd 压缩,来源与许可证见 `assets/fonts/`）。
const EXTRA_FONTS: &[&[u8]] = &[
    include_bytes!("../assets/fonts/NotoSerifSC.ttf.zst"),
    include_bytes!("../assets/fonts/LXGWWenKaiGB-Light.ttf.zst"),
    include_bytes!("../assets/fonts/LXGWWenKaiGB.ttf.zst"),
    include_bytes!("../assets/fonts/LXGWWenKaiGB-Medium.ttf.zst"),
];

/// 全局共享字体句柄：框架内置（黑体 + 等宽）+ 自备（宋体 + 楷体）+ 系统字体。
pub fn handle() -> FontHandle {
    static HANDLE: OnceLock<FontHandle> = OnceLock::new();
    HANDLE
        .get_or_init(|| {
            let mut b = FontHandle::builder().bundled().system();
            for f in EXTRA_FONTS {
                b = b.data(*f);
            }
            b.build().expect("构建字体栈（内置字体解压失败？）")
        })
        .clone()
}
