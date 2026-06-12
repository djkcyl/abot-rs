//! 画板渲染 —— 把画布画成 PNG(内存字节,直接发出)。三种图:
//! - [`render_full`]:全图(每格 5px → 1280×720)+ 每 16 格参考网格 + 坐标刻度。
//! - [`render_zoom`]:以某点为心的 48×27 放大窗(每格 20px)+ 逐格网格 + 坐标 + 目标格准星。
//! - [`render_palette`]:32 色编号块(8×4),编号用点阵数字。
//!
//! 文字(刻度/编号)一律用 [`font`] 的点阵数字,**零字体依赖**。颜色经 [`colors`] 索引→RGB。

use chrono::Local;
use image::codecs::webp::WebPEncoder;
use image::{ExtendedColorType, ImageEncoder, Rgb, RgbImage};
use nagisa::prelude::*;
use sea_orm::{DatabaseConnection, EntityTrait};

use super::colors::{self, EMPTY};
use super::entity::pixel;
use super::font;

/// 画布逻辑尺寸(格)。
pub const W: u32 = 256;
pub const H: u32 = 144;

const GRID: Rgb<u8> = Rgb([210, 210, 210]);
const LABEL: Rgb<u8> = Rgb([90, 90, 90]);
const BG: Rgb<u8> = Rgb([255, 255, 255]);
const MARK: Rgb<u8> = Rgb([255, 0, 0]); // 放大窗目标格准星

/// 读出整块画布的调色板索引缓冲(行优先,空格默认 [`EMPTY`] 哨兵 0,渲染为背景白)。
///
/// 只投影 `(x, y, color)` 三列(`select_only` + `into_tuple`),不物化整行 Model(省去 uin/at 反序列化)。
/// 对连接泛型:出图传连接、落格事务内存快照传 `&txn` 都走这一个。
pub(super) async fn load_canvas<C: sea_orm::ConnectionTrait>(db: &C) -> Result<Vec<u8>> {
    use sea_orm::QuerySelect;
    let mut buf = vec![EMPTY; (W * H) as usize];
    let rows: Vec<(i32, i32, i32)> = pixel::Entity::find()
        .select_only()
        .column(pixel::Column::X)
        .column(pixel::Column::Y)
        .column(pixel::Column::Color)
        .into_tuple()
        .all(db)
        .await
        .context("读画布")?;
    for (x, y, color) in rows {
        if (0..W as i32).contains(&x) && (0..H as i32).contains(&y) && (1..=32).contains(&color) {
            buf[(y as u32 * W + x as u32) as usize] = color as u8;
        }
    }
    Ok(buf)
}

/// 填一个实心矩形(自动夹到图内)。
fn fill_rect(img: &mut RgbImage, x: u32, y: u32, w: u32, h: u32, c: Rgb<u8>) {
    let (iw, ih) = (img.width(), img.height());
    for yy in y..(y + h).min(ih) {
        for xx in x..(x + w).min(iw) {
            img.put_pixel(xx, yy, c);
        }
    }
}

/// 描一个矩形边框(线宽 `t`)。
fn rect_outline(img: &mut RgbImage, x: u32, y: u32, w: u32, h: u32, c: Rgb<u8>, t: u32) {
    fill_rect(img, x, y, w, t, c);
    fill_rect(img, x, y + h.saturating_sub(t), w, t, c);
    fill_rect(img, x, y, t, h, c);
    fill_rect(img, x + w.saturating_sub(t), y, t, h, c);
}

/// 把图编码成 WebP 字节(无损;bot 内出图统一 WebP)。
fn encode(img: &RgbImage) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    WebPEncoder::new_lossless(&mut out)
        .write_image(img.as_raw(), img.width(), img.height(), ExtendedColorType::Rgb8)
        .context("编码 WebP")?;
    Ok(out)
}

/// 全图:每格 5px,四周留白画每 16 格的坐标刻度。
pub async fn render_full(db: &DatabaseConnection) -> Result<Vec<u8>> {
    let buf = load_canvas(db).await?;
    const S: u32 = 5;
    const M: u32 = 48;
    let (bw, bh) = (W * S, H * S);
    let mut img = RgbImage::from_pixel(bw + 2 * M, bh + 2 * M, BG);

    for y in 0..H {
        for x in 0..W {
            let idx = buf[(y * W + x) as usize];
            if idx != EMPTY {
                fill_rect(&mut img, M + x * S, M + y * S, S, S, Rgb(colors::rgb(idx)));
            }
        }
    }
    // 每 16 格参考网格。
    for gx in (0..=W).step_by(16) {
        fill_rect(&mut img, M + gx * S, M, 1, bh, GRID);
    }
    for gy in (0..=H).step_by(16) {
        fill_rect(&mut img, M, M + gy * S, bw, 1, GRID);
    }
    // 坐标刻度(每 16 格,标签**居中在格子上**:+半格偏移,而非压在网格线上)。
    for k in (0..W).step_by(16) {
        let s = k.to_string();
        let lx = (M + k * S + S / 2).saturating_sub(font::text_width(&s, 2) / 2);
        font::draw_text(&mut img, lx, M.saturating_sub(22), &s, 2, LABEL);
    }
    for k in (0..H).step_by(16) {
        let s = k.to_string();
        font::draw_text(&mut img, 6, (M + k * S + S / 2).saturating_sub(7), &s, 2, LABEL);
    }
    encode(&img)
}

/// 区块网格:4×4=16 块,每块 64×36 格(各 16:9)。供 `作画` 导航选块。
pub const BLOCK_COLS: i32 = 4;
pub const BLOCK_ROWS: i32 = 4;
pub const BLOCK_W: i32 = W as i32 / BLOCK_COLS; // 64
pub const BLOCK_H: i32 = H as i32 / BLOCK_ROWS; // 36
/// 区块总数(1..=BLOCKS)。
pub const BLOCKS: u8 = (BLOCK_COLS * BLOCK_ROWS) as u8;

/// 区块号(1..=16)→ 左上角格坐标 `(x0, y0)`(行优先)。越界钳到合法区间。
pub fn block_origin(n: u8) -> (i32, i32) {
    let i = (n.clamp(1, BLOCKS) - 1) as i32;
    ((i % BLOCK_COLS) * BLOCK_W, (i / BLOCK_COLS) * BLOCK_H)
}

/// 通用窗口渲染:画布 `(x0,y0)` 起 `ww×wh` 格,每格 `cell` px,逐格网格 + 每 `label_step` 格坐标
/// (居中在格子上)+ 可选目标格红框 `mark`。[`render_zoom`] / [`render_block`] 都走它。
fn render_window(
    buf: &[u8],
    origin: (i32, i32),
    size: (i32, i32),
    cell: u32,
    mark: Option<(i32, i32)>,
    label_step: usize,
) -> Result<Vec<u8>> {
    const M: u32 = 44;
    let (x0, y0) = origin;
    let (ww, wh) = size;
    let (bw, bh) = (ww as u32 * cell, wh as u32 * cell);
    let mut img = RgbImage::from_pixel(bw + 2 * M, bh + 2 * M, BG);

    for row in 0..wh {
        for col in 0..ww {
            let idx = buf[((y0 + row) as u32 * W + (x0 + col) as u32) as usize];
            let c = if idx == EMPTY { BG } else { Rgb(colors::rgb(idx)) };
            fill_rect(&mut img, M + col as u32 * cell, M + row as u32 * cell, cell, cell, c);
        }
    }
    for col in 0..=ww as u32 {
        fill_rect(&mut img, M + col * cell, M, 1, bh, GRID);
    }
    for row in 0..=wh as u32 {
        fill_rect(&mut img, M, M + row * cell, bw, 1, GRID);
    }
    for col in (0..ww).step_by(label_step) {
        let s = (x0 + col).to_string();
        let lx = (M + col as u32 * cell + cell / 2).saturating_sub(font::text_width(&s, 2) / 2);
        font::draw_text(&mut img, lx, M.saturating_sub(20), &s, 2, LABEL);
    }
    for row in (0..wh).step_by(label_step) {
        let s = (y0 + row).to_string();
        let ly = (M + row as u32 * cell + cell / 2).saturating_sub(7);
        font::draw_text(&mut img, 4, ly, &s, 2, LABEL);
    }
    if let Some((mx, my)) = mark
        && (x0..x0 + ww).contains(&mx)
        && (y0..y0 + wh).contains(&my)
    {
        let px = M + (mx - x0) as u32 * cell;
        let py = M + (my - y0) as u32 * cell;
        rect_outline(&mut img, px, py, cell, cell, MARK, 2);
    }
    encode(&img)
}

/// 放大窗:以 `(cx, cy)` 为心取 48×27 格窗口(贴边夹紧),目标格红框。
pub async fn render_zoom(db: &DatabaseConnection, cx: i32, cy: i32) -> Result<Vec<u8>> {
    let buf = load_canvas(db).await?;
    const WW: i32 = 48;
    const WH: i32 = 27;
    let x0 = (cx - WW / 2).clamp(0, W as i32 - WW);
    let y0 = (cy - WH / 2).clamp(0, H as i32 - WH);
    render_window(&buf, (x0, y0), (WW, WH), 20, Some((cx, cy)), 4)
}

/// 区块放大:第 `n` 块(64×36 格)放大,绝对坐标,无准星。
pub async fn render_block(db: &DatabaseConnection, n: u8) -> Result<Vec<u8>> {
    let buf = load_canvas(db).await?;
    let (x0, y0) = block_origin(n);
    render_window(&buf, (x0, y0), (BLOCK_W, BLOCK_H), 14, None, 8)
}

/// 色板:32 色编号块(8 列 × 4 行),编号用点阵数字。
pub fn render_palette() -> Result<Vec<u8>> {
    const COLS: u32 = 8;
    const ROWS: u32 = 4;
    const CW: u32 = 120;
    const CH: u32 = 96;
    const M: u32 = 16;
    let mut img = RgbImage::from_pixel(COLS * CW + 2 * M, ROWS * CH + 2 * M, Rgb([245, 245, 245]));

    for idx in 1..=32u8 {
        let i = (idx - 1) as u32;
        let (col, row) = (i % COLS, i / COLS);
        let (cellx, celly) = (M + col * CW, M + row * CH);
        // 色块。
        let (sx, sy, sw, sh) = (cellx + 8, celly + 8, CW - 16, 60);
        fill_rect(&mut img, sx, sy, sw, sh, Rgb(colors::rgb(idx)));
        rect_outline(&mut img, sx, sy, sw, sh, Rgb([60, 60, 60]), 1);
        // 编号。
        let s = idx.to_string();
        let lx = cellx + (CW - font::text_width(&s, 3)) / 2;
        font::draw_text(&mut img, lx, celly + 72, &s, 3, Rgb([30, 30, 30]));
    }
    encode(&img)
}

/// 总览:全图 + 粗区块分隔 + 每块大号区块编号(深底白字),供 `作画` 选块。
pub async fn render_overview(db: &DatabaseConnection) -> Result<Vec<u8>> {
    let buf = load_canvas(db).await?;
    const S: u32 = 5;
    const M: u32 = 24;
    let (bw, bh) = (W * S, H * S);
    let mut img = RgbImage::from_pixel(bw + 2 * M, bh + 2 * M, BG);

    for y in 0..H {
        for x in 0..W {
            let idx = buf[(y * W + x) as usize];
            if idx != EMPTY {
                fill_rect(&mut img, M + x * S, M + y * S, S, S, Rgb(colors::rgb(idx)));
            }
        }
    }
    // 粗区块分隔线。
    let div = Rgb([40, 40, 40]);
    for c in 0..=BLOCK_COLS as u32 {
        fill_rect(&mut img, M + c * BLOCK_W as u32 * S, M, 2, bh, div);
    }
    for r in 0..=BLOCK_ROWS as u32 {
        fill_rect(&mut img, M, M + r * BLOCK_H as u32 * S, bw, 2, div);
    }
    // 每块大号编号(居中,深底白字)。
    for n in 1..=BLOCKS {
        let (x0, y0) = block_origin(n);
        let s = n.to_string();
        let scale = 5;
        let (tw, th) = (font::text_width(&s, scale), 7 * scale);
        let cx = M + (x0 as u32 + BLOCK_W as u32 / 2) * S;
        let cy = M + (y0 as u32 + BLOCK_H as u32 / 2) * S;
        let bx = cx.saturating_sub(tw / 2 + 6);
        let by = cy.saturating_sub(th / 2 + 6);
        fill_rect(&mut img, bx, by, tw + 12, th + 12, Rgb([0, 0, 0]));
        font::draw_text(
            &mut img,
            cx.saturating_sub(tw / 2),
            cy.saturating_sub(th / 2),
            &s,
            scale,
            Rgb([255, 255, 255]),
        );
    }
    encode(&img)
}

/// 干净分享图:纯像素放大(无网格 / 刻度 / 留白)+ 左下角点阵水印 `ABOT PLACE  <日期>`。
pub async fn render_clean(db: &DatabaseConnection) -> Result<Vec<u8>> {
    let buf = load_canvas(db).await?;
    const S: u32 = 6;
    let mut img = RgbImage::from_pixel(W * S, H * S, BG);
    for y in 0..H {
        for x in 0..W {
            let idx = buf[(y * W + x) as usize];
            if idx != EMPTY {
                fill_rect(&mut img, x * S, y * S, S, S, Rgb(colors::rgb(idx)));
            }
        }
    }
    // 水印:左下角深底白字。日期用本地时区。
    let mark = format!("ABOT PLACE  {}", Local::now().format("%Y-%m-%d"));
    let scale = 3;
    let (tw, th, pad) = (font::text_width(&mark, scale), 7 * scale, 8);
    let bx = 8;
    let by = img.height().saturating_sub(th + 2 * pad + 8);
    fill_rect(&mut img, bx, by, tw + 2 * pad, th + 2 * pad, Rgb([0, 0, 0]));
    font::draw_text(&mut img, bx + pad, by + pad, &mark, scale, Rgb([255, 255, 255]));
    encode(&img)
}
