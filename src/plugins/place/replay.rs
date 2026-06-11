//! 时间轴回放 —— 把 `place_history` 从空白逐步重演成 GIF。
//!
//! 节奏全自适应,用户只选时间窗:总时长 ≤ 30 秒(含末帧停留 2 秒),帧延迟 80–150ms,
//! 跳帧(每帧合并几笔)从 1 起按需增大、不设上限,见 [`adaptive`]。
//!
//! 编码直接走 `gif` crate:画布本就是 33 色索引色,全局调色板一次写死,逐帧喂索引
//! 字节,零量化;除首帧外每帧只编码本批落格的脏包围盒(GIF 子区域帧 + 保留上一帧),
//! 跳帧小时一帧只有几十字节,几百帧的 GIF 也只在百 KB 量级。逐帧流式编码,不全帧驻留。

use nagisa::prelude::*;
use chrono::{Duration as ChronoDuration, Local};
use gif::{DisposalMethod, Encoder, Frame, Repeat};
use sea_orm::DatabaseConnection;

use super::colors;
use super::logic;
use super::render::{H, W};

/// 回放参数:只有时间窗,节奏自适应。
pub struct ReplayArgs {
    /// 最近 N 天;`None` = 全量。
    pub days: Option<i64>,
}

const CELL: u32 = 4; // 每格像素(GIF 用小图)
const TOTAL_MS: u32 = 30_000; // GIF 总时长上限
const HOLD_MS: u32 = 2_000; // 末帧停留
const BUDGET_MS: u32 = TOTAL_MS - HOLD_MS; // 动画部分的时长预算
const DELAY_MAX_MS: u32 = 150; // 缺省帧延迟
const DELAY_MIN_MS: u32 = 80; // 帧延迟下限(帧数硬上界 = 预算/此值 = 350)

/// 自适应定参:总落格数 → `(step, 帧延迟 ms)`。
///
/// 预算内能一笔一帧(150ms 下 186 帧)就一笔一帧;超出后压缩量在两个旋钮间按平方根
/// 分摊——目标延迟 `d* = 150·√(186/T)`(下钳 80)先定 step,step 取整后延迟再回填
/// 用满预算(80–150 内取整到 10ms,GIF 延迟单位 1/100 秒)。大 T 收敛到
/// `step = ceil(T/350)`、延迟 80ms,正是把帧数顶到上界的最小跳帧,再大白扔信息。
fn adaptive(total: usize) -> (usize, u32) {
    let calm_cap = (BUDGET_MS / DELAY_MAX_MS) as usize;
    if total <= calm_cap {
        return (1, DELAY_MAX_MS);
    }
    let ratio = total as f64 / calm_cap as f64;
    let d_star = (DELAY_MAX_MS as f64 / ratio.sqrt()).max(DELAY_MIN_MS as f64);
    let step = ((total as f64 * d_star) / BUDGET_MS as f64).ceil() as usize;
    let frames = total.div_ceil(step);
    let delay = (BUDGET_MS / frames as u32) / 10 * 10;
    (step, delay.clamp(DELAY_MIN_MS, DELAY_MAX_MS))
}

/// 生成回放 GIF 字节。窗内无落格则返回 `Ok(None)`。
///
/// 数据经 [`logic::replay_window`]:全量从空白起步;带窗首帧是窗口起点时刻的真实画布
/// (快照恢复)。查询(异步)在本任务里做;**重演 + 光栅化 + GIF 编码是 CPU 密集**,
/// 放进 `spawn_blocking`,不阻塞 async 运行时。
pub async fn render_replay(db: &DatabaseConnection, args: ReplayArgs) -> Result<Option<Vec<u8>>> {
    let since = args.days.map(|d| (Local::now() - ChronoDuration::days(d)).fixed_offset());
    let win = logic::replay_window(db, since).await?;
    if win.rows.is_empty() {
        return Ok(None);
    }
    let gif = tokio::task::spawn_blocking(move || encode_replay(&win.base, &win.rows))
        .await
        .context("回放编码任务失败")??;
    Ok(Some(gif))
}

/// 无参回放的当日缓存出口:命中直接出;未命中现生成、落缓存再出(于是每天第一次
/// 触发即「自动生成」当日份,4 点的预热任务平时会先一步把它做好)。无历史返 `None`。
pub async fn full_cached(db: &DatabaseConnection) -> Result<Option<Vec<u8>>> {
    if let Some(gif) = logic::replay_cache_get(db).await? {
        return Ok(Some(gif));
    }
    let Some(gif) = render_replay(db, ReplayArgs { days: None }).await? else {
        return Ok(None);
    };
    logic::replay_cache_put(db, gif.clone()).await?;
    Ok(Some(gif))
}

/// 预热当日缓存:已有则不动,没有则生成落库。后台日任务用。
pub async fn warm_cache(db: &DatabaseConnection) -> Result<()> {
    if logic::replay_cache_get(db).await?.is_some() {
        return Ok(());
    }
    if let Some(gif) = render_replay(db, ReplayArgs { days: None }).await? {
        logic::replay_cache_put(db, gif).await?;
    }
    Ok(())
}

/// 同步重演 + 流式编码 GIF(在 `spawn_blocking` 里跑)。
///
/// 首帧编 `base` 全画布定底(全量回放即空白,窗口回放即起点画布),之后一遍扫笔序列,
/// 每 `step` 笔出一帧:应用到画布缓冲、只编本批的脏包围盒子区域(`DisposalMethod::Keep`
/// 保留上一帧)。总帧数 = 1 + ceil(T/step),时长 = 帧数×延迟 + 末帧停留 ≤ 30s。
fn encode_replay(base: &[u8], rows: &[(i32, i32, u8)]) -> Result<Vec<u8>> {
    let (step, delay_ms) = adaptive(rows.len());

    // 全局调色板:索引 0 = 空(背景白),1–32 = 画板调色板。画布缓冲的索引值直接就是
    // GIF 像素,零转换。
    let mut palette = vec![255u8, 255, 255];
    for i in 1..=32u8 {
        palette.extend(colors::rgb(i));
    }

    let mut buf = base.to_vec();
    debug_assert_eq!(buf.len(), (W * H) as usize);
    let mut out = Vec::new();
    let mut enc = Encoder::new(&mut out, (W * CELL) as u16, (H * CELL) as u16, &palette)
        .context("建 GIF 编码器失败")?;
    enc.set_repeat(Repeat::Infinite).context("设置 GIF 循环失败")?;

    // 首帧:起点画布全帧。
    let mut head = rect_frame(&buf, 0, 0, W - 1, H - 1);
    head.delay = (delay_ms / 10) as u16;
    head.dispose = DisposalMethod::Keep;
    enc.write_frame(&head).context("编码 GIF 帧失败")?;

    let mut i = 0;
    while i < rows.len() {
        let end = (i + step).min(rows.len());
        // 应用本批,记脏包围盒(画布格坐标,含端点)。
        let (mut x0, mut y0, mut x1, mut y1) = (W as i32, H as i32, -1i32, -1i32);
        for &(x, y, c) in &rows[i..end] {
            if (0..W as i32).contains(&x) && (0..H as i32).contains(&y) {
                buf[(y as u32 * W + x as u32) as usize] = c;
                (x0, y0) = (x0.min(x), y0.min(y));
                (x1, y1) = (x1.max(x), y1.max(y));
            }
        }
        i = end;
        // 本批全越界(理论不该有)退全帧,保证时间轴不缺拍。
        let mut frame = if x1 < 0 {
            rect_frame(&buf, 0, 0, W - 1, H - 1)
        } else {
            rect_frame(&buf, x0 as u32, y0 as u32, x1 as u32, y1 as u32)
        };
        let ms = if i >= rows.len() { HOLD_MS } else { delay_ms };
        frame.delay = (ms / 10) as u16;
        frame.dispose = DisposalMethod::Keep;
        enc.write_frame(&frame).context("编码 GIF 帧失败")?;
    }
    drop(enc); // flush trailer
    Ok(out)
}

/// 画布格矩形(含端点)→ 放大 [`CELL`] 倍的索引色子区域帧(delay/dispose 由调用方填)。
fn rect_frame(buf: &[u8], x0: u32, y0: u32, x1: u32, y1: u32) -> Frame<'static> {
    let (w, h) = (x1 - x0 + 1, y1 - y0 + 1);
    let mut px = Vec::with_capacity((w * CELL * h * CELL) as usize);
    for gy in y0..=y1 {
        let row_start = px.len();
        for gx in x0..=x1 {
            let c = buf[(gy * W + gx) as usize];
            px.extend(std::iter::repeat_n(c, CELL as usize));
        }
        // 整行像素已铺好,纵向再复制 CELL−1 行。
        for _ in 1..CELL {
            px.extend_from_within(row_start..row_start + (w * CELL) as usize);
        }
    }
    Frame {
        left: (x0 * CELL) as u16,
        top: (y0 * CELL) as u16,
        width: (w * CELL) as u16,
        height: (h * CELL) as u16,
        buffer: px.into(),
        ..Frame::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 自适应定参对几个代表量级给出预期的 (step, 延迟)。
    #[test]
    fn adaptive_curve() {
        assert_eq!(adaptive(50), (1, 150)); // 远不满:一笔一帧,舒缓
        assert_eq!(adaptive(186), (1, 150)); // 150ms 容量刚好
        assert_eq!(adaptive(200), (2, 150)); // 过渡区:step 先动,延迟保持舒缓
        assert_eq!(adaptive(350), (2, 150));
        assert_eq!(adaptive(700), (2, 80)); // 帧数顶满,延迟压到底
        assert_eq!(adaptive(1_000_000), (2858, 80)); // 大 T:ceil(T/350)
    }

    /// 端到端编码:出的字节是合法 GIF,尺寸对,帧数 = 1(底)+ ceil(T/step),底帧承载
    /// 起点画布,增量帧只有脏矩形大。
    #[test]
    fn encode_valid_gif_with_delta_frames() {
        let mut base = vec![colors::EMPTY; (W * H) as usize];
        base[0] = 28; // 起点画布有一格黑,底帧须带出来
        let rows: Vec<(i32, i32, u8)> = (0..10).map(|i| (i, i, (i % 32 + 1) as u8)).collect();
        let bytes = encode_replay(&base, &rows).unwrap();
        let mut opts = gif::DecodeOptions::new();
        opts.set_color_output(gif::ColorOutput::Indexed);
        let mut dec = opts.read_info(std::io::Cursor::new(&bytes)).unwrap();
        assert_eq!(dec.width() as u32, W * CELL);
        assert_eq!(dec.height() as u32, H * CELL);
        // 底帧全画布,左上角那格是基底色;后续单笔帧只有一格大(CELL×CELL)。
        let f0 = dec.read_next_frame().unwrap().unwrap();
        assert_eq!((f0.width as u32, f0.height as u32), (W * CELL, H * CELL));
        assert_eq!(f0.buffer[0], 28);
        let f1 = dec.read_next_frame().unwrap().unwrap();
        assert_eq!((f1.width as u32, f1.height as u32), (CELL, CELL));
        let mut frames = 2;
        while dec.read_next_frame().unwrap().is_some() {
            frames += 1;
        }
        assert_eq!(frames, 11); // 1 底帧 + 10 笔(step=1,一笔一帧)
    }
}
