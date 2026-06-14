//! 出图公共底座 —— 全 bot 排版出图的统一 `RenderOptions`(abot 字体栈 + WebP + 页脚
//! 项目水印)与启动预热。各出图点在 [`UserTheme::opts`](或无用户语境的 [`render_opts`])
//! 之上自调宽度 / 边距 / 清晰度,水印想去掉就 `opts.footer = None`(如占位图这类小贴片
//! 不适合带)。
//!
//! # 用户主题
//!
//! 出图主题按**用户偏好**走:亮暗看 `user.theme` 列(auto / light / dark,经 [`pick_dark`]
//! 解析,auto 按当月典型日出日落天黑走暗),主题色看 `user.theme_color` 列(五套预设之一的键,见
//! [`THEMES`],空串走缺省远黛蓝)。两者经 [`UserTheme::resolve`] 一次解析出标准色卡
//! [`Palette`]——主题四色按本次亮暗收过对比、底色 / 色带 / 底槽按主色色相派生——出图点
//! 拿槽位直接上色,品牌底栏的色带也跟着主题走。

use nagisa::render::{Align, Color, OutputFormat, PageChrome, RenderOptions, Theme};

/// 一套主题:库键、中文名与四个色相基准(亮色域取值;暗色域出图时自动调亮收对比)。
/// 五套预设见 [`THEMES`]。
#[derive(Debug)]
pub struct ThemeSpec {
    /// 库值(`user.theme_color`)。
    pub key: &'static str,
    /// 中文名(「主题」命令词与呈现)。
    pub name: &'static str,
    primary: &'static str,
    deep: &'static str,
    vivid: &'static str,
    warm: &'static str,
}

/// 全部主题(五套),首个(远黛蓝,即品牌原配色)是缺省。四基准色槽位语义一致
/// (主 / 重 / 鲜 / 暖),插件换主题不换映射;暖槽各主题都偏暖——游戏币这类含义
/// 固定的数字不至于因为换主题而变味。
pub const THEMES: &[ThemeSpec] = &[
    ThemeSpec {
        key: "indigo", name: "远黛蓝", primary: "#4c63b6", deep: "#7a5cc4", vivid: "#0e9488", warm: "#bd6b32"
    },
    ThemeSpec {
        key: "teal", name: "松石青", primary: "#0e9488", deep: "#1f6e9c", vivid: "#5f9e2e", warm: "#b8872e"
    },
    ThemeSpec {
        key: "orange", name: "落霞橙", primary: "#c2661f", deep: "#b03346", vivid: "#8aa32e", warm: "#c79a2e"
    },
    ThemeSpec {
        key: "purple", name: "鸢尾紫", primary: "#7a5cc4", deep: "#4c63b6", vivid: "#b04fa3", warm: "#bd6b32"
    },
    ThemeSpec {
        key: "pink", name: "珊瑚粉", primary: "#c75c8a", deep: "#a23aa8", vivid: "#8a5cc4", warm: "#c2742e"
    },
];

/// 按库值取主题;空串或脏值回缺省(远黛蓝)。
pub fn theme_spec(key: &str) -> &'static ThemeSpec {
    THEMES.iter().find(|t| t.key == key).unwrap_or(&THEMES[0])
}

impl ThemeSpec {
    /// 出本主题在给定亮暗下的标准色卡:四基准色各自收对比(亮底压暗 / 暗底提亮),
    /// 底色三槽(淡底 / 色带 / 底槽)按主色色相定饱和明度派生,中性两槽亮暗各一套。
    pub fn palette(&self, dark: bool) -> Palette {
        let fit = |hex| fit_contrast(hex, dark);
        let (soft, band, track, on_color, muted) = if dark {
            (
                shade(self.primary, 0.30, 0.20),
                shade(self.primary, 0.28, 0.09),
                shade(self.primary, 0.16, 0.21),
                "#10151c",
                "#7d8590",
            )
        } else {
            (
                shade(self.primary, 0.50, 0.93),
                shade(self.primary, 0.40, 0.945),
                shade(self.primary, 0.22, 0.885),
                "#ffffff",
                "#8a8f98",
            )
        };
        Palette {
            primary: fit(self.primary),
            deep: fit(self.deep),
            vivid: fit(self.vivid),
            warm: fit(self.warm),
            soft,
            band,
            on_color: on_color.to_string(),
            muted: muted.to_string(),
            track,
        }
    }
}

/// 一次出图的标准色卡 —— 全部值**已按本次亮暗调好对比**,出图点拿槽位直接上色。
/// 槽位用途全插件一致(主题换色不换语义):
#[derive(Clone, Debug)]
pub struct Palette {
    /// 主色:标题强调 / 进度条 / 选中格 / 楼号——主题的脸面色。
    pub primary: String,
    /// 重色:次级强调(里程碑一类的重点标)。
    pub deep: String,
    /// 鲜色:增量数字(经验一类)/ 活泼强调。
    pub vivid: String,
    /// 暖色:游戏币 / 奖励一类的数字。
    pub warm: String,
    /// 主色淡底:淡色填充(今天格 / 表头底)。
    pub soft: String,
    /// 底栏色带(主色极淡 / 极深,品牌底栏用)。
    pub band: String,
    /// 色块上的反白文字。
    pub on_color: String,
    /// 次要文字。
    pub muted: String,
    /// 进度条底槽 / 弱分隔(主色微染)。
    pub track: String,
}

/// 一次出图的用户主题:亮暗 + 解析好的标准色卡。从 `user.theme` / `user.theme_color`
/// 经 [`resolve`](Self::resolve)(即
/// [`AUser::render_theme`](crate::data::AUser::render_theme))一次解析,渲染全程使用。
#[derive(Clone, Debug)]
pub struct UserTheme {
    /// 本次走暗色。
    pub dark: bool,
    /// 本次标准色卡。
    pub palette: Palette,
}

impl UserTheme {
    /// 从用户两列偏好解析:亮暗经 [`pick_dark`],主题经 [`theme_spec`],随即出色卡。
    pub fn resolve(theme_pref: &str, color_pref: &str) -> Self {
        let dark = pick_dark(theme_pref);
        Self { dark, palette: theme_spec(color_pref).palette(dark) }
    }

    /// 公共出图选项:亮暗底座 + 主题色卡(强调色写进引擎 `Theme.accent`,引用条 / 链接
    /// 等引擎级强调跟着走;品牌底栏色带用色卡的 band 槽)。
    pub fn opts(&self) -> RenderOptions {
        build_opts(self.dark, &self.palette)
    }
}

/// 公共出图选项(亮色、缺省主题):abot 字体栈、WebP、品牌底栏。无用户语境的出图点
/// (预热 / 占位图)用;按用户出图一律走 [`UserTheme::opts`]。
pub fn render_opts() -> RenderOptions {
    build_opts(false, &theme_spec("").palette(false))
}

/// 拼一份出图选项:字体栈 + WebP + 品牌底栏(色带按色卡)、abot 统一的卡宽与缩放,
/// 暗色换引擎暗主题,强调色写进 `Theme.accent`。
///
/// 宽 840 是 abot 卡片的缺省口径(行式多列的帮助卡另覆盖 960,媒体占位图特意小不在此列);
/// 1.5 倍率在清晰度够用的前提下压字节(QQ 端展示尺寸有限,2.0 浪费)。
fn build_opts(dark: bool, pal: &Palette) -> RenderOptions {
    let mut o = RenderOptions::default()
        .with_width(840.0)
        .with_scale(1.5)
        .with_fonts(crate::fonts::handle())
        .with_format(OutputFormat::Webp)
        .with_footer_chrome(brand_footer(dark, &pal.band));
    if dark {
        o.theme = Theme::dark();
    }
    if let Some(c) = Color::hex(&pal.primary) {
        o.theme.accent = c;
    }
    o
}

/// 各月典型日出 / 日落钟点(分钟,东八区),取武汉(30.6°N——版图中部,代表全国大多数
/// 用户的昼夜感受)月中值:夏天五点多天亮、晚上七点多才黑,冬天七点多天亮、五点半
/// 就黑。锚点经太阳几何核算(冬至昼长约 10h13m、夏至约 14h04m、春分 12h),月中
/// 取整到 5 分钟。
const SUN_TABLE: [(u16, u16); 12] = [
    (7 * 60 + 25, 17 * 60 + 45), // 1 月
    (7 * 60 + 5, 18 * 60 + 10),  // 2 月
    (6 * 60 + 35, 18 * 60 + 30), // 3 月
    (6 * 60, 18 * 60 + 50),      // 4 月
    (5 * 60 + 35, 19 * 60 + 10), // 5 月
    (5 * 60 + 20, 19 * 60 + 25), // 6 月
    (5 * 60 + 30, 19 * 60 + 20), // 7 月
    (5 * 60 + 50, 19 * 60),      // 8 月
    (6 * 60 + 10, 18 * 60 + 30), // 9 月
    (6 * 60 + 30, 17 * 60 + 55), // 10 月
    (6 * 60 + 55, 17 * 60 + 30), // 11 月
    (7 * 60 + 15, 17 * 60 + 25), // 12 月
];

/// 按用户主题偏好解析本次出图走不走暗色:`dark` / `light` 定死,其余(`auto` 或脏值)
/// 按本地钟点对照当月典型日出日落(`SUN_TABLE`,武汉口径)——天黑走暗,随月份
/// 自动变化,不是写死的两个钟点。
pub fn pick_dark(pref: &str) -> bool {
    use chrono::{Datelike, Timelike};
    match pref {
        "dark" => true,
        "light" => false,
        _ => {
            let now = chrono::Local::now();
            let minute = (now.hour() * 60 + now.minute()) as u16;
            let (rise, set) = SUN_TABLE[now.month0() as usize];
            minute < rise || minute >= set
        }
    }
}

/// 品牌底栏:满幅色带(色调跟用户主题的 band 槽)上一句 `ABot · 由 nagisa 驱动 ·
/// nagisa-render 排版 · A60`——名号加重立体、各带一个含蓄的品牌色(bot 靛蓝 / 框架青绿 /
/// 引擎紫 / 作者暖橙),连接词浅灰斜体,居中。品牌色按底色明暗各调一档。
fn brand_footer(dark: bool, band: &str) -> PageChrome {
    let (c_bot, c_fw, c_render, c_author) =
        if dark { ("#8fa3e8", "#3ec9b8", "#a98ee8", "#e0a06a") } else { ("#4c63b6", "#0e9488", "#7a5cc4", "#bd6b32") };
    PageChrome::rich(move |p| {
        p.styled("ABot", |s| {
            s.weight(600).color(c_bot);
        });
        p.text("  ·  ");
        p.styled("由 ", |s| {
            s.italic();
        });
        p.styled("nagisa", |s| {
            s.weight(600).color(c_fw);
        });
        p.styled(" 驱动", |s| {
            s.italic();
        });
        p.text("  ·  ");
        p.styled("nagisa-render", |s| {
            s.weight(600).color(c_render);
        });
        p.styled(" 排版", |s| {
            s.italic();
        });
        p.text("  ·  ");
        p.styled("A60", |s| {
            s.weight(600).color(c_author);
        });
    })
    .align(Align::Center)
    .band(band)
}

/// 把一个自设颜色收进本次亮暗的可读对比区间(保色相)供出图上色:空串或非法形状返
/// `None`(调用方退回缺省文字色),否则返回收好对比的 `#rrggbb`(亮底压暗 / 暗底提亮)。
/// 自设昵称颜色([`AUser::alias_color`](crate::data::AUser::alias_color))的统一出图口径——
/// 不论用户挑什么色,亮底压暗 / 暗底提亮后都立得住。
pub fn readable_hex(hex: &str, dark: bool) -> Option<String> {
    let hex = hex.trim();
    parse_hex(hex)?; // 校验形状,脏值不上色
    Some(fit_contrast(hex, dark))
}

/// 把基准色收进给定亮暗的可读对比区间:保色相,亮底压到相对亮度 Y ≤ 0.25(白字色标、
/// 白底彩字都立得住),暗底提到 Y ≥ 0.28。HSL 的 l 与感知亮度不是一回事(纯黄 l=0.5 在
/// 白底上仍刺眼),故按 Y 目标二分搜 l。
fn fit_contrast(hex: &str, dark: bool) -> String {
    let Some((r, g, b)) = parse_hex(hex) else {
        return hex.to_string();
    };
    let (h, s, l) = rgb_to_hsl(r, g, b);
    let y = luma(r, g, b);
    let l2 = if dark {
        if y >= 0.28 { l } else { search_l(h, s, 0.28, l..=1.0) }
    } else if y <= 0.25 {
        l
    } else {
        search_l(h, s, 0.25, 0.0..=l)
    };
    let (r2, g2, b2) = hsl_to_rgb(h, s, l2);
    format!("#{r2:02x}{g2:02x}{b2:02x}")
}

/// 取 `hex` 的色相,按给定饱和度 / 明度出一档派生色(淡底 / 色带 / 底槽用)。
fn shade(hex: &str, s: f32, l: f32) -> String {
    let (r, g, b) = parse_hex(hex).expect("主题基准色应为合法 hex");
    let (h, _, _) = rgb_to_hsl(r, g, b);
    let (r2, g2, b2) = hsl_to_rgb(h, s, l);
    format!("#{r2:02x}{g2:02x}{b2:02x}")
}

/// 解析 `#rgb` / `#rrggbb`(`#` 可省、大小写不限)成 RGB。其余形状返 `None`。
fn parse_hex(s: &str) -> Option<(u8, u8, u8)> {
    let s = s.trim().trim_start_matches('#');
    let v = |c: u8| (c as char).to_digit(16).map(|d| d as u8);
    match s.as_bytes() {
        [r, g, b] => Some((v(*r)? * 17, v(*g)? * 17, v(*b)? * 17)),
        [r1, r2, g1, g2, b1, b2] => Some((v(*r1)? * 16 + v(*r2)?, v(*g1)? * 16 + v(*g2)?, v(*b1)? * 16 + v(*b2)?)),
        _ => None,
    }
}

/// sRGB 相对亮度(2.2 幂近似,做对比收敛够用)。
fn luma(r: u8, g: u8, b: u8) -> f32 {
    let lin = |c: u8| (c as f32 / 255.0).powf(2.2);
    0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b)
}

/// 定色相饱和度,在给定 l 区间里二分出最接近目标相对亮度的 l(Y 随 l 单调)。
fn search_l(h: f32, s: f32, target_y: f32, range: std::ops::RangeInclusive<f32>) -> f32 {
    let (mut lo, mut hi) = (*range.start(), *range.end());
    for _ in 0..20 {
        let mid = (lo + hi) / 2.0;
        let (r, g, b) = hsl_to_rgb(h, s, mid);
        if luma(r, g, b) < target_y {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    (lo + hi) / 2.0
}

/// RGB → HSL(h ∈ [0,360),s/l ∈ [0,1])。
fn rgb_to_hsl(r: u8, g: u8, b: u8) -> (f32, f32, f32) {
    let (r, g, b) = (r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    let d = max - min;
    if d == 0.0 {
        return (0.0, 0.0, l);
    }
    let s = d / (1.0 - (2.0 * l - 1.0).abs());
    let h = if max == r {
        60.0 * (((g - b) / d).rem_euclid(6.0))
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    (h, s, l)
}

/// HSL → RGB(入参域同 [`rgb_to_hsl`] 的返回)。
fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (u8, u8, u8) {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0).rem_euclid(2.0) - 1.0).abs());
    let m = l - c / 2.0;
    let (r, g, b) = match h {
        h if h < 60.0 => (c, x, 0.0),
        h if h < 120.0 => (x, c, 0.0),
        h if h < 180.0 => (0.0, c, x),
        h if h < 240.0 => (0.0, x, c),
        h if h < 300.0 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let to = |v: f32| ((v + m).clamp(0.0, 1.0) * 255.0).round() as u8;
    (to(r), to(g), to(b))
}

/// 拉一张 QQ 头像(q.qlogo.cn,640px)。给出图卡片嵌头部用;头像会换,不进媒体归档,
/// 现拉现用。拉不到返回 `None`(只记日志),卡片缺头像照常渲。
pub async fn qq_avatar(uin: i64) -> Option<Vec<u8>> {
    use std::sync::OnceLock;
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    let client = CLIENT.get_or_init(|| {
        reqwest::Client::builder().timeout(std::time::Duration::from_secs(5)).build().unwrap_or_default()
    });
    let url = format!("https://q.qlogo.cn/g?b=qq&nk={uin}&s=640");
    let resp = match client.get(&url).send().await.and_then(|r| r.error_for_status()) {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(uin, error = %e, "拉 QQ 头像失败");
            return None;
        }
    };
    match resp.bytes().await {
        Ok(b) => Some(b.to_vec()),
        Err(e) => {
            tracing::warn!(uin, error = %e, "读 QQ 头像字节失败");
            None
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// readable_hex 空 / 脏值不上色(None);有效色收进对比区间——亮底压暗、暗底提亮,
    /// 故纯白在亮底变深、在暗底维持亮。
    #[test]
    fn readable_contrast() {
        assert_eq!(readable_hex("", false), None);
        assert_eq!(readable_hex("   ", false), None);
        assert_eq!(readable_hex("nope", false), None);

        let light = readable_hex("#ffffff", false).unwrap();
        let (r, g, b) = parse_hex(&light).unwrap();
        assert!(luma(r, g, b) <= 0.30, "亮底白字应被压暗到可读, got {light}");

        let dark = readable_hex("#ffffff", true).unwrap();
        let (r, g, b) = parse_hex(&dark).unwrap();
        assert!(luma(r, g, b) >= 0.25, "暗底应维持足够亮, got {dark}");
    }
}
