//! 比赛 GIF 回放:把 [`RaceResult`] 的位置时间线重演成动图。逐回合整帧(回合少,不做增量
//! 矩形),编码 CPU 密集,放 `spawn_blocking` 跑。

use gif::{Encoder, Frame, Repeat};
use nagisa::prelude::*;

use super::consts::COLOR_COUNT;
use super::race::RaceResult;

const CELL: u32 = 4; // 每逻辑格的像素
const TRACK_W: u32 = 96; // 赛道逻辑格宽
const LANE_H: u32 = 7; // 每泳道逻辑格高
const HORSE: u32 = 5; // 马色块边长(格)

const BUDGET_MS: u32 = 8_000; // 动画时长预算
const HOLD_MS: u32 = 2_000; // 末帧停留
const DELAY_MIN: u32 = 80;
const DELAY_MAX: u32 = 150;
const FRAME_CAP: usize = 90; // 帧数上界

/// 调色板:0 泳道底、1 泳道底(隔行)、2 终点线,3.. 为各毛色。
fn palette() -> Vec<u8> {
    let mut p = vec![
        200, 230, 180, // 0 泳道底
        186, 218, 166, // 1 泳道底(隔行)
        220, 60, 60, // 2 终点线
    ];
    // 6 种毛色(与 consts::COLOR_NAMES 同序)。
    const HORSE_RGB: [[u8; 3]; COLOR_COUNT as usize] = [
        [139, 58, 42],   // 枣红
        [160, 82, 45],   // 栗色
        [48, 48, 48],    // 乌骓
        [232, 232, 232], // 白龙
        [122, 139, 139], // 青骢
        [200, 144, 46],  // 金棕
    ];
    for c in HORSE_RGB {
        p.extend(c);
    }
    p
}

/// 毛色索引 → 调色板下标。
fn horse_idx(color: i16) -> u8 {
    3 + color.clamp(0, COLOR_COUNT - 1) as u8
}

/// 自适应:总回合数 → `(step, 帧延迟 ms)`。预算内一回合一帧,超出则跳帧。
fn adaptive(rounds: usize) -> (usize, u32) {
    let calm = (BUDGET_MS / DELAY_MAX) as usize; // 150ms 下的容量
    if rounds <= calm {
        return (1, DELAY_MAX);
    }
    let step = rounds.div_ceil(FRAME_CAP).max(1);
    let frames = rounds.div_ceil(step).max(1) as u32;
    let delay = (BUDGET_MS / frames / 10 * 10).clamp(DELAY_MIN, DELAY_MAX);
    (step, delay)
}

/// 在逻辑格缓冲里画一帧(某回合的画面),返回放大 [`CELL`] 倍的索引像素。
fn draw_frame(result: &RaceResult, round: usize) -> (Vec<u8>, u32, u32) {
    let n = result.runners.len() as u32;
    let gw = TRACK_W;
    let gh = n * LANE_H;
    let mut grid = vec![0u8; (gw * gh) as usize];

    let finish_col = gw - 2;
    let span = (gw - HORSE - 2) as f32; // 马色块左缘可达的最右格

    for (li, runner) in result.runners.iter().enumerate() {
        let lane_top = li as u32 * LANE_H;
        let bg = (li % 2) as u8; // 隔行底色
        for ry in 0..LANE_H {
            for cx in 0..gw {
                let idx = ((lane_top + ry) * gw + cx) as usize;
                grid[idx] = if cx >= finish_col { 2 } else { bg };
            }
        }
        // 马:5×5 色块,x 由该回合位置定。
        let pos = result.positions[round][li];
        let frac = (pos / result.track_len).clamp(0.0, 1.0);
        let hx = (frac * span).round() as u32;
        let hy = lane_top + 1;
        let col = horse_idx(runner.color);
        for dy in 0..HORSE {
            for dx in 0..HORSE {
                let (x, y) = (hx + dx, hy + dy);
                if x < gw && y < gh {
                    grid[(y * gw + x) as usize] = col;
                }
            }
        }
    }

    // 放大 CELL 倍。
    let (pw, ph) = (gw * CELL, gh * CELL);
    let mut px = vec![0u8; (pw * ph) as usize];
    for y in 0..gh {
        for x in 0..gw {
            let v = grid[(y * gw + x) as usize];
            for dy in 0..CELL {
                for dx in 0..CELL {
                    px[((y * CELL + dy) * pw + x * CELL + dx) as usize] = v;
                }
            }
        }
    }
    (px, pw, ph)
}

/// 把一场比赛编码成回放 GIF 字节(同步,放 `spawn_blocking` 跑)。
pub fn encode(result: &RaceResult) -> Result<Vec<u8>> {
    let rounds = result.positions.len();
    let (step, delay) = adaptive(rounds);
    let pal = palette();
    let n = result.runners.len() as u32;
    let (pw, ph) = (TRACK_W * CELL, n * LANE_H * CELL);

    let mut out = Vec::new();
    {
        let mut enc = Encoder::new(&mut out, pw as u16, ph as u16, &pal).context("建赛马 GIF 编码器")?;
        enc.set_repeat(Repeat::Infinite).context("设 GIF 循环")?;

        let mut r = 0usize;
        while r < rounds {
            let (px, fw, fh) = draw_frame(result, r);
            let is_last = r + step >= rounds;
            let frame = Frame {
                width: fw as u16,
                height: fh as u16,
                buffer: px.into(),
                delay: (if is_last { HOLD_MS } else { delay } / 10) as u16,
                ..Frame::default()
            };
            enc.write_frame(&frame).context("编赛马 GIF 帧")?;
            r += step;
        }
    }
    Ok(out)
}

/// 异步出口:在 `spawn_blocking` 里编码,不阻塞运行时。
pub async fn render(result: std::sync::Arc<RaceResult>) -> Result<Vec<u8>> {
    tokio::task::spawn_blocking(move || encode(&result)).await.context("赛马回放编码任务")?
}
