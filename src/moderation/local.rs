//! 本地兜底审核 —— 不依赖外部服务，无凭证也能跑。
//!
//! 文本走 `aho-corasick` 关键词自动机；图片用 `image` 解码成灰度图后交 `rqrr` 探二维码。
//! 两者都设计成「失败即放行」：没词表就不命中，不是图片就当没二维码，绝不因兜底环节卡死流程。

use aho_corasick::AhoCorasick;

/// 关键词命中器：持一个已建好的 `aho-corasick` 自动机。
///
/// 空词表也能正常 `is_match`（恒返 false），故缺省词表 = 永不命中。
pub struct KeywordMatcher {
    ac: AhoCorasick,
}

impl KeywordMatcher {
    /// 从一组词建自动机。`words` 应已是去空白、去空行后的词。
    pub fn new(words: &[String]) -> Self {
        // AhoCorasick::new 仅在模式过多/过大时报错(实务上不会),失败就退回空表。
        let ac = AhoCorasick::new(words)
            .unwrap_or_else(|_| AhoCorasick::new::<_, &str>([]).expect("空模式集建自动机不会失败"));
        Self { ac }
    }

    /// 文本是否命中任一关键词。空词表恒为 false。
    pub fn text_hit(&self, text: &str) -> bool {
        self.ac.is_match(text)
    }
}

/// 图片里是否存在二维码。
///
/// 先用 `image` 解码：解不出（不是图片 / 格式不支持）即视作无二维码、放行兜底。
/// 再转灰度交 `rqrr` 探 finder 图案——能定位到网格就算有二维码，不必真解出载荷。
pub fn has_qrcode(bytes: &[u8]) -> bool {
    let Ok(img) = image::load_from_memory(bytes) else {
        return false; // 不是图片 / 不支持的格式 → 不拦
    };
    let luma = img.to_luma8();
    let mut prep = rqrr::PreparedImage::prepare(luma);
    !prep.detect_grids().is_empty()
}
