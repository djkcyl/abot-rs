//! 本地二维码闸 —— 纯 Rust（tract）跑微信开源 wechat_qrcode 的 SSD 检测模型。
//!
//! 腾讯云 IMS 不识别二维码,这一路补上:图里有码即判广告拦,免得引流码直接过。模型经 caffe→onnx
//! 转换、在置信分支(`mbox_conf_softmax`)截断丢掉 PriorBox/DetectionOutput,只做检测(有没有码)、不解码;
//! ~900KB,`include_bytes!` 嵌进二进制。对普通码与中等风格化码有效,极端艺术码是开源模型的天花板会漏。
//!
//! 判定:把图缩到 384×384 灰度 / 255 喂模型,数置信分支里「是二维码」概率过 [`CONF`] 的 anchor,
//! 满 [`MIN_HITS`] 个即命中(真码动辄几十上百,零星误报压得住)。要换更强的检测器,替掉 onnx 即可。

use std::io::Cursor;
use std::sync::{Arc, OnceLock};

use tract_onnx::prelude::*;

/// 检测模型输入边长(SSD 固定 384×384 灰度)。
const INPUT: usize = 384;
/// 单 anchor 判为二维码的置信阈。
const CONF: f32 = 0.9;
/// 至少多少个高置信 anchor 才算命中。
const MIN_HITS: usize = 5;

/// 嵌入的检测模型(截断到置信分支的 onnx)。
static MODEL_BYTES: &[u8] = include_bytes!("qrcode_detect.onnx");

type QrModel = Arc<TypedRunnableModel>;

/// 惰性载入模型;失败则记一次错并永久关闭这一路(返 `None`:二维码不拦,但不崩、不影响腾讯云那路)。
fn model() -> Option<&'static QrModel> {
    static MODEL: OnceLock<Option<QrModel>> = OnceLock::new();
    MODEL
        .get_or_init(|| match build() {
            Ok(m) => Some(m),
            Err(e) => {
                tracing::error!(error = %e, "二维码检测模型载入失败,本地二维码闸关闭");
                None
            }
        })
        .as_ref()
}

fn build() -> TractResult<QrModel> {
    tract_onnx::onnx()
        .model_for_read(&mut Cursor::new(MODEL_BYTES))?
        .with_input_fact(0, f32::fact([1, 1, INPUT, INPUT]).into())?
        .into_optimized()?
        .into_runnable()
}

/// 图里是否有二维码。不是图片 / 模型不可用 / 推理失败一律返 `false`(这一路只补充、不阻断)。
pub(super) fn has_qrcode(bytes: &[u8]) -> bool {
    let Some(model) = model() else { return false };
    let Ok(img) = image::load_from_memory(bytes) else { return false };
    let luma =
        image::imageops::resize(&img.to_luma8(), INPUT as u32, INPUT as u32, image::imageops::FilterType::Triangle);
    let input = tract_ndarray::Array4::<f32>::from_shape_fn((1, 1, INPUT, INPUT), |(_, _, y, x)| {
        luma.get_pixel(x as u32, y as u32)[0] as f32 / 255.0
    });
    let Ok(out) = model.run(tvec!(Tensor::from(input).into())) else { return false };
    let Ok(view) = out[0].to_plain_array_view::<f32>() else { return false };
    // 输出 [1, N, 2]:每 anchor 的 [背景, 二维码] 概率;隔一取「是二维码」那列,数过阈值的。
    view.iter().skip(1).step_by(2).filter(|&&q| q > CONF).count() >= MIN_HITS
}
