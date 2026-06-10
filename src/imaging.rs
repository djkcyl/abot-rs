//! 出图公共底座 —— 全 bot 排版出图的统一 `RenderOptions`(abot 字体栈 + WebP + 页脚
//! 项目水印)与启动预热。各出图点在 [`render_opts`] 之上自调宽度 / 边距 / 清晰度,
//! 水印想去掉就 `opts.footer = None`(如占位图这类小贴片不适合带)。

use nagisa::render::{Align, OutputFormat, PageChrome, RenderOptions};

/// 公共出图选项:abot 字体栈、WebP、品牌底栏。
pub fn render_opts() -> RenderOptions {
    RenderOptions::default()
        .with_fonts(crate::fonts::handle())
        .with_format(OutputFormat::Webp)
        .with_footer_chrome(brand_footer())
}

/// 品牌底栏:浅青满幅色带(渚的水色)上一句
/// `ABot · 由 nagisa 驱动 · nagisa-render 排版 · A60`——名号加重立体、各带一个含蓄的
/// 品牌色(bot 靛蓝 / 框架青绿 / 引擎紫 / 作者暖橙),连接词浅灰斜体,居中。
fn brand_footer() -> PageChrome {
    PageChrome::rich(|p| {
        p.styled("ABot", |s| {
            s.weight(600).color("#4c63b6");
        });
        p.text("  ·  ");
        p.styled("由 ", |s| {
            s.italic();
        });
        p.styled("nagisa", |s| {
            s.weight(600).color("#0e9488");
        });
        p.styled(" 驱动", |s| {
            s.italic();
        });
        p.text("  ·  ");
        p.styled("nagisa-render", |s| {
            s.weight(600).color("#7a5cc4");
        });
        p.styled(" 排版", |s| {
            s.italic();
        });
        p.text("  ·  ");
        p.styled("A60", |s| {
            s.weight(600).color("#bd6b32");
        });
    })
    .align(Align::Center)
    .band("#ebf4f2")
}

/// 启动预热:字体栈构建(zstd 解压 + 字体库扫描)与首次整形 / 栅格的开销一次付清,
/// 不让第一个出图命令多等一秒。渲一张小图把链路全走一遍;失败上抛(出图链路坏了
/// 该在启动时就知道,而不是首个用户命令踩坑)。
pub fn warmup() -> anyhow::Result<()> {
    use nagisa::render::Doc;
    let mut d = Doc::new();
    d.paragraph(|p| {
        p.text("预热");
    });
    nagisa::render::render_document(&d.build(), &render_opts().fast())?;
    Ok(())
}
