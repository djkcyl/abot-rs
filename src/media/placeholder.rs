//! 失效图片占位图 —— 随机渐变色底,上书「图片已失效」与该图 md5。
//!
//! 文字层交给排版引擎(`nagisa::render`,abot 的字体栈)在**透明底**上排版出 RGBA,
//! 渐变底逐像素自画(对角线、两端随机色相、亮度压中低档保证白字可读),再 alpha 合成、
//! 编码 WebP(bot 内出图统一 WebP)。捞瓶重发与 WebUI 媒体路由共用:图没了也让人看见
//! 「这里有过一张图」和它的身份。

use nagisa::render::{
    render_to_rgba, Align, Block, Color, Document, FontRole, Inline, Insets, RenderOptions,
    TextStyle, Theme,
};

/// 渲染一张「图片已失效」占位 WebP(每次调用随机一组渐变色)。
pub fn missing_image_webp(md5: &str) -> anyhow::Result<Vec<u8>> {
    // —— 文字层:透明底、白字,标题 + 说明 + 等宽 md5。——
    let white = |a: u8| Color::rgba(0xff, 0xff, 0xff, a);
    let line = |text: &str, style: TextStyle| Block::Paragraph {
        inlines: vec![Inline::Text { text: text.to_string(), style }],
        align: Align::Center,
    };
    let doc = Document {
        blocks: vec![
            Block::Heading {
                level: 3,
                inlines: vec![Inline::Text {
                    text: "图片已失效".into(),
                    style: TextStyle { color: Some(white(255)), ..Default::default() },
                }],
                align: Align::Center,
            },
            line(
                "这里本来有张图,但已经无法显示",
                TextStyle { color: Some(white(225)), size: 0.85, ..Default::default() },
            ),
            line(
                md5,
                TextStyle {
                    color: Some(white(190)),
                    size: 0.72,
                    font: FontRole::Mono,
                    ..Default::default()
                },
            ),
        ],
    };
    let mut theme = Theme::dark();
    theme.background = Color::rgba(0, 0, 0, 0); // 透明底:这层只要文字
    let opts = RenderOptions::default()
        .with_width(500.0) // 够 32 位 md5 等宽一行排下
        .with_padding(Insets::symmetric(34.0, 24.0))
        .with_theme(theme)
        .with_fonts(crate::fonts::handle());
    let text = render_to_rgba(&doc, &opts)?;

    // —— 渐变底:两端随机色相(相距 90°-230°,免得渐变看不出来),对角线插值。——
    let (w, h) = text.dimensions();
    let h1 = rand::random::<f32>() * 360.0;
    let h2 = (h1 + 90.0 + rand::random::<f32>() * 140.0) % 360.0;
    let c1 = hsl_rgb(h1, 0.55, 0.52);
    let c2 = hsl_rgb(h2, 0.60, 0.36);
    let mut img = image::RgbaImage::new(w, h);
    let span = (w + h).saturating_sub(2).max(1) as f32;
    for (x, y, p) in img.enumerate_pixels_mut() {
        let t = (x + y) as f32 / span;
        let mix = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
        *p = image::Rgba([mix(c1[0], c2[0]), mix(c1[1], c2[1]), mix(c1[2], c2[2]), 255]);
    }

    // —— 文字层合成(render_to_rgba 出的是去预乘直 alpha)。——
    for (x, y, tp) in text.enumerate_pixels() {
        let a = tp[3] as f32 / 255.0;
        if a == 0.0 {
            continue;
        }
        let bp = img.get_pixel_mut(x, y);
        for i in 0..3 {
            bp[i] = (tp[i] as f32 * a + bp[i] as f32 * (1.0 - a)).round() as u8;
        }
    }

    let mut out = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(img).write_to(&mut out, image::ImageFormat::WebP)?;
    Ok(out.into_inner())
}

/// HSL → RGB(h 角度,s/l 0..=1)。
fn hsl_rgb(h: f32, s: f32, l: f32) -> [u8; 3] {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let hp = (h.rem_euclid(360.0)) / 60.0;
    let x = c * (1.0 - (hp % 2.0 - 1.0).abs());
    let (r, g, b) = match hp as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    [((r + m) * 255.0) as u8, ((g + m) * 255.0) as u8, ((b + m) * 255.0) as u8]
}
