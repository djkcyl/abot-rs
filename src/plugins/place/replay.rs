//! 时间轴回放 —— 把 `place_history` 从空白逐步重演成 GIF。
//!
//! 一遍按时间升序扫历史,维护一块画布缓冲,每 `step` 次落格快照一帧,逐帧流式编码进 GIF
//! (不全帧驻留内存)。帧数有硬上限防爆;`step` 缺省按总数自动取,使总帧 ≈ 目标值。
//! GIF 本就是索引色,正好装下我们的 32 色 + 白。

use nagisa::prelude::*;
use chrono::{Duration as ChronoDuration, Local};
use image::codecs::gif::{GifEncoder, Repeat};
use image::{Delay, Frame};
use sea_orm::DatabaseConnection;

use super::colors::EMPTY;
use super::logic::history_in_range;
use super::render::{frame_rgba, H, W};

/// 回放参数(都可选)。
pub struct ReplayArgs {
    /// 最近 N 天;`None` = 全量。
    pub days: Option<i64>,
    /// 每帧合并几次落格;`None` = 按总数自动。
    pub step: Option<usize>,
    /// 总帧数上限;`None` = 默认 40。
    pub frames: Option<usize>,
}

const CELL: u32 = 4; // 每格像素(GIF 用小图)
const MAX_FRAMES: usize = 60; // 帧数硬上限
const FRAME_MS: u32 = 150; // 普通帧间隔
const HOLD_MS: u32 = 2000; // 末帧停留

/// 生成回放 GIF 字节。无历史则返回 `Ok(None)`。
///
/// 查询(异步)在本任务里做;**重演 + 逐帧光栅化 + GIF 编码是 CPU 密集**,放进
/// `spawn_blocking`,不阻塞 async 运行时(这是任何人可触发的命令)。
pub async fn render_replay(db: &DatabaseConnection, args: ReplayArgs) -> Result<Option<Vec<u8>>> {
    let since = args.days.map(|d| (Local::now() - ChronoDuration::days(d)).fixed_offset());
    let rows = history_in_range(db, since).await?;
    if rows.is_empty() {
        return Ok(None);
    }
    let gif = tokio::task::spawn_blocking(move || encode_replay(&rows, args.step, args.frames))
        .await
        .context("回放编码任务失败")??;
    Ok(Some(gif))
}

/// 同步重演 + 流式编码 GIF(在 `spawn_blocking` 里跑)。一遍扫历史、每 `step` 次落格快照一帧。
fn encode_replay(
    rows: &[(i32, i32, u8)],
    step_opt: Option<usize>,
    frames_opt: Option<usize>,
) -> Result<Vec<u8>> {
    let total = rows.len();
    let cap = frames_opt.unwrap_or(40).clamp(2, MAX_FRAMES);
    let step = step_opt.unwrap_or_else(|| total.div_ceil(cap)).max(1);

    let mut buf = vec![EMPTY; (W * H) as usize];
    let mut out = Vec::new();
    let mut enc = GifEncoder::new(&mut out);
    enc.set_repeat(Repeat::Infinite).context("设置 GIF 循环失败")?;
    let mut i = 0;
    while i < total {
        let end = (i + step).min(total);
        for &(x, y, c) in &rows[i..end] {
            if (0..W as i32).contains(&x) && (0..H as i32).contains(&y) {
                buf[(y as u32 * W + x as u32) as usize] = c;
            }
        }
        i = end;
        let ms = if i >= total { HOLD_MS } else { FRAME_MS };
        let frame = Frame::from_parts(frame_rgba(&buf, CELL), 0, 0, Delay::from_numer_denom_ms(ms, 1));
        enc.encode_frame(frame).context("编码 GIF 帧失败")?;
    }
    drop(enc); // flush trailer
    Ok(out)
}
