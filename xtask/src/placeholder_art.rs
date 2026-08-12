//! Deterministic placeholder art generator for the phase-1 RTS slice.
//!
//! Two families, no external source image: `rts` (unit/building/prop
//! sheets, same 4x8x32px frame geometry as the phase-0 zombie atlases) and
//! `ui` (an 8x8 bitmap font). Every texel is premultiplied RGBA8, same
//! invariant `crate::atlases::assert_premultiplied` already enforces.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::atlases::{AtlasEntry, AtlasError, assert_premultiplied};
use crate::digest::sha256_hex;

/// Placeholder generator id, stamped into both new manifests.
pub const RTS_GENERATOR_ID: &str = "mmd-rts-placeholder-v1";
pub const UI_GENERATOR_ID: &str = "mmd-ui-font-v1";

/// RTS family frame geometry — identical to the zombie atlases so
/// `render::frame_uv_rect` addresses both with one function.
pub const RTS_FRAME_SIZE_PX: u32 = 32;
pub const RTS_FRAMES_X: u32 = 4;
pub const RTS_FRAMES_Y: u32 = 8;
pub const RTS_ATLAS_WIDTH_PX: u32 = RTS_FRAMES_X * RTS_FRAME_SIZE_PX; // 128
pub const RTS_ATLAS_HEIGHT_PX: u32 = RTS_FRAMES_Y * RTS_FRAME_SIZE_PX; // 256

/// The four RTS placeholder sheets, in manifest order. Index is the id.
pub const RTS_FILES: [&str; 4] = ["worker.png", "soldier.png", "buildings.png", "props.png"];

/// Bitmap font geometry.
pub const GLYPH_W_PX: u32 = 8;
pub const GLYPH_H_PX: u32 = 8;
pub const FONT_COLS: u32 = 16;
pub const FONT_ROWS: u32 = 6;
// Consumed by `extract_glyph_cell` in tests below and by the T3 text
// renderer's cell-address math (`(c - 32) % 16, (c - 32) / 16`); kept `pub`
// per this ticket's declared public API even though nothing in this crate
// calls it outside tests yet.
#[allow(dead_code)]
pub const FONT_FIRST_CHAR: u8 = 32;
pub const FONT_WIDTH_PX: u32 = FONT_COLS * GLYPH_W_PX; // 128
pub const FONT_HEIGHT_PX: u32 = FONT_ROWS * GLYPH_H_PX; // 48
pub const UI_FILES: [&str; 1] = ["font.png"];

pub const PLACEHOLDER_MANIFEST_VERSION: u32 = 1;

/// Matches `sim::tick::dir_from_vector`: 0=E,1=NE,2=N,3=NW,4=W,5=SW,6=S,7=SE.
#[allow(clippy::approx_constant)] // ticket-locked literal, not std::f32::consts::FRAC_1_SQRT_2
const DIR_DX: [f32; 8] = [1.0, 0.7071, 0.0, -0.7071, -1.0, -0.7071, 0.0, 0.7071];
#[allow(clippy::approx_constant)]
const DIR_DY: [f32; 8] = [0.0, -0.7071, -1.0, -0.7071, 0.0, 0.7071, 1.0, 0.7071];
const BOB: [i32; 4] = [0, -1, 0, 1];

const WORKER_BODY: [u8; 4] = [60, 150, 220, 255];
const WORKER_HEAD: [u8; 4] = [200, 210, 230, 255];
const SOLDIER_BODY: [u8; 4] = [190, 70, 60, 255];
const SOLDIER_HEAD: [u8; 4] = [220, 210, 200, 255];
const PIP_COLOR: [u8; 4] = [255, 230, 90, 255];

// T11 HUD props: the gear, the minimap frame and the six command-grid
// icons. One flat opaque colour each — deterministic and, at alpha 255,
// already premultiplied — since these are UI glyphs, not painterly art.
const GEAR_COLOR: [u8; 4] = [170, 175, 185, 255];
const MINIMAP_FRAME_COLOR: [u8; 4] = [45, 60, 80, 255];
const ICON_BUILD_HQ_COLOR: [u8; 4] = [70, 110, 170, 255];
const ICON_BUILD_DEPOT_COLOR: [u8; 4] = [80, 140, 110, 255];
const ICON_BUILD_BARRACKS_COLOR: [u8; 4] = [150, 110, 60, 255];
const ICON_TRAIN_WORKER_COLOR: [u8; 4] = [60, 150, 220, 255];
const ICON_TRAIN_SOLDIER_COLOR: [u8; 4] = [190, 70, 60, 255];
const ICON_SET_RALLY_COLOR: [u8; 4] = [240, 200, 60, 255];

/// Tracked placeholder set manifest (one per family).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlaceholderManifest {
    pub version: u32,
    pub generator: String,
    pub frame_width_px: u32,
    pub frame_height_px: u32,
    pub cols: u32,
    pub rows: u32,
    pub images: Vec<AtlasEntry>,
}

/// `<root>/assets/sprites/generated/rts`
pub fn rts_dir(workspace_root: &Path) -> PathBuf {
    workspace_root.join("assets/sprites/generated/rts")
}

/// `<root>/assets/sprites/generated/ui`
pub fn ui_dir(workspace_root: &Path) -> PathBuf {
    workspace_root.join("assets/sprites/generated/ui")
}

/// Premultiply one texel: `store = (round(c * a / 255), a)`.
fn premul(rgba: [u8; 4]) -> [u8; 4] {
    let [r, g, b, a] = rgba;
    let a16 = u16::from(a);
    [
        ((u16::from(r) * a16 + 127) / 255) as u8,
        ((u16::from(g) * a16 + 127) / 255) as u8,
        ((u16::from(b) * a16 + 127) / 255) as u8,
        a,
    ]
}

fn set_px(px: &mut [u8], w: u32, x: u32, y: u32, rgba: [u8; 4]) {
    let idx = ((y * w + x) * 4) as usize;
    px[idx..idx + 4].copy_from_slice(&rgba);
}

fn blit_rect(px: &mut [u8], w: u32, x0: u32, y0: u32, x1: u32, y1: u32, rgba: [u8; 4]) {
    for y in y0..=y1 {
        for x in x0..=x1 {
            set_px(px, w, x, y, rgba);
        }
    }
}

/// Copy one `RTS_FRAME_SIZE_PX` square tile into a larger sheet at `(origin_x, origin_y)`.
fn blit_tile(sheet: &mut [u8], sheet_w: u32, origin_x: u32, origin_y: u32, tile: &[u8]) {
    let row_len = (RTS_FRAME_SIZE_PX * 4) as usize;
    for ly in 0..RTS_FRAME_SIZE_PX {
        let src = (ly * RTS_FRAME_SIZE_PX * 4) as usize;
        let dst = (((origin_y + ly) * sheet_w + origin_x) * 4) as usize;
        sheet[dst..dst + row_len].copy_from_slice(&tile[src..src + row_len]);
    }
}

/// Facing-pip square top-left, clamped so the 4x4 square stays inside the tile.
fn pip_origin(dx: f32, dy: f32) -> (u32, u32) {
    let cx = (16.0 + 6.0 * dx).round() as i32;
    let cy = (14.0 + 6.0 * dy).round() as i32;
    let max = RTS_FRAME_SIZE_PX as i32 - 4;
    let x0 = (cx - 2).clamp(0, max);
    let y0 = (cy - 2).clamp(0, max);
    (x0 as u32, y0 as u32)
}

/// Shift a tile's pixels vertically by `shift`; pixels pushed out are dropped.
fn shift_vertical(tile: &[u8], shift: i32) -> Vec<u8> {
    let mut out = vec![0u8; tile.len()];
    let row_len = (RTS_FRAME_SIZE_PX * 4) as usize;
    for y in 0..RTS_FRAME_SIZE_PX as i32 {
        let dy = y + shift;
        if dy < 0 || dy >= RTS_FRAME_SIZE_PX as i32 {
            continue;
        }
        let src = (y as u32 * RTS_FRAME_SIZE_PX * 4) as usize;
        let dst = (dy as u32 * RTS_FRAME_SIZE_PX * 4) as usize;
        out[dst..dst + row_len].copy_from_slice(&tile[src..src + row_len]);
    }
    out
}

/// 8 dirs x 4 frames animated unit sheet (worker / soldier share this shape).
fn render_unit_sheet(body: [u8; 4], head: [u8; 4], pip: [u8; 4]) -> Vec<u8> {
    let mut sheet = vec![0u8; (RTS_ATLAS_WIDTH_PX * RTS_ATLAS_HEIGHT_PX * 4) as usize];
    for (dir, (&dx, &dy)) in DIR_DX.iter().zip(DIR_DY.iter()).enumerate() {
        let (px0, py0) = pip_origin(dx, dy);
        for (frame, &shift) in BOB.iter().enumerate() {
            let mut tile = vec![0u8; (RTS_FRAME_SIZE_PX * RTS_FRAME_SIZE_PX * 4) as usize];
            blit_rect(&mut tile, RTS_FRAME_SIZE_PX, 10, 8, 21, 25, body);
            blit_rect(&mut tile, RTS_FRAME_SIZE_PX, 12, 2, 19, 9, head);
            blit_rect(
                &mut tile,
                RTS_FRAME_SIZE_PX,
                px0,
                py0,
                px0 + 3,
                py0 + 3,
                pip,
            );
            let shifted = shift_vertical(&tile, shift);
            let origin_x = frame as u32 * RTS_FRAME_SIZE_PX;
            let origin_y = dir as u32 * RTS_FRAME_SIZE_PX;
            blit_tile(&mut sheet, RTS_ATLAS_WIDTH_PX, origin_x, origin_y, &shifted);
        }
    }
    sheet
}

/// `(row, col) -> (fill, border, under_construction)` for the buildings sheet.
fn buildings_table(row: u32, col: u32) -> Option<([u8; 4], [u8; 4], bool)> {
    match (row, col) {
        (0, 0) => Some(([70, 110, 170, 255], [230, 235, 245, 255], false)),
        (0, 1) => Some(([80, 140, 110, 255], [230, 235, 245, 255], false)),
        (0, 2) => Some(([150, 110, 60, 255], [230, 235, 245, 255], false)),
        (1, 0) => Some(([35, 55, 85, 255], [120, 125, 130, 255], true)),
        (1, 1) => Some(([40, 70, 55, 255], [120, 125, 130, 255], true)),
        (1, 2) => Some(([75, 55, 30, 255], [120, 125, 130, 255], true)),
        (2, 0) => Some(([120, 200, 235, 255], [235, 250, 255, 255], false)),
        (2, 1) => Some(([170, 120, 220, 255], [240, 225, 255, 255], false)),
        (2, 2) => Some(([70, 95, 105, 255], [130, 140, 145, 255], false)),
        (2, 3) => Some(([85, 70, 100, 255], [135, 125, 145, 255], false)),
        _ => None,
    }
}

fn draw_building_cell(tile: &mut [u8], fill: [u8; 4], border: [u8; 4], hatch: bool) {
    blit_rect(tile, RTS_FRAME_SIZE_PX, 3, 3, 28, 28, fill);
    for x in 3..=28u32 {
        set_px(tile, RTS_FRAME_SIZE_PX, x, 3, border);
        set_px(tile, RTS_FRAME_SIZE_PX, x, 28, border);
    }
    for y in 3..=28u32 {
        set_px(tile, RTS_FRAME_SIZE_PX, 3, y, border);
        set_px(tile, RTS_FRAME_SIZE_PX, 28, y, border);
    }
    if hatch {
        for y in 0..RTS_FRAME_SIZE_PX {
            for x in 0..RTS_FRAME_SIZE_PX {
                if (x + y) % 4 == 0 {
                    set_px(tile, RTS_FRAME_SIZE_PX, x, y, [0, 0, 0, 0]);
                }
            }
        }
    }
}

fn render_buildings_sheet() -> Vec<u8> {
    let mut sheet = vec![0u8; (RTS_ATLAS_WIDTH_PX * RTS_ATLAS_HEIGHT_PX * 4) as usize];
    for row in 0..RTS_FRAMES_Y {
        for col in 0..RTS_FRAMES_X {
            let mut tile = vec![0u8; (RTS_FRAME_SIZE_PX * RTS_FRAME_SIZE_PX * 4) as usize];
            if let Some((fill, border, hatch)) = buildings_table(row, col) {
                draw_building_cell(&mut tile, fill, border, hatch);
            }
            let origin_x = col * RTS_FRAME_SIZE_PX;
            let origin_y = row * RTS_FRAME_SIZE_PX;
            blit_tile(&mut sheet, RTS_ATLAS_WIDTH_PX, origin_x, origin_y, &tile);
        }
    }
    sheet
}

fn draw_diamond(tile: &mut [u8], color: [u8; 4]) {
    for y in 0..RTS_FRAME_SIZE_PX {
        for x in 0..RTS_FRAME_SIZE_PX {
            let d = (x as i32 - 16).abs() + (y as i32 - 16).abs();
            if d <= 10 {
                set_px(tile, RTS_FRAME_SIZE_PX, x, y, color);
            }
        }
    }
}

fn draw_props_cell(tile: &mut [u8], row: u32, col: u32) {
    match (row, col) {
        (0, 0) => {
            for y in 0..RTS_FRAME_SIZE_PX {
                for x in 0..RTS_FRAME_SIZE_PX {
                    let dx = x as f64 - 15.5;
                    let dy = y as f64 - 15.5;
                    let r = dx.hypot(dy).round() as i64;
                    if (12..=14).contains(&r) {
                        set_px(tile, RTS_FRAME_SIZE_PX, x, y, [40, 235, 120, 255]);
                    }
                }
            }
        }
        (0, 1) => blit_rect(
            tile,
            RTS_FRAME_SIZE_PX,
            4,
            4,
            27,
            27,
            premul([30, 180, 90, 140]),
        ),
        (0, 2) => blit_rect(
            tile,
            RTS_FRAME_SIZE_PX,
            4,
            4,
            27,
            27,
            premul([200, 50, 50, 140]),
        ),
        (0, 3) => {
            blit_rect(tile, RTS_FRAME_SIZE_PX, 14, 4, 16, 27, [240, 240, 240, 255]);
            for y in 5..=12u32 {
                for x in 17..=26u32 {
                    if (y as i64 - 5) <= (26 - x as i64) {
                        set_px(tile, RTS_FRAME_SIZE_PX, x, y, [240, 200, 60, 255]);
                    }
                }
            }
        }
        (1, 0) => draw_diamond(tile, [120, 200, 235, 255]),
        (1, 1) => draw_diamond(tile, [170, 120, 220, 255]),
        (1, 2) => blit_rect(
            tile,
            RTS_FRAME_SIZE_PX,
            10,
            10,
            21,
            21,
            [220, 220, 120, 255],
        ),
        (1, 3) => blit_rect(
            tile,
            RTS_FRAME_SIZE_PX,
            0,
            0,
            RTS_FRAME_SIZE_PX - 1,
            RTS_FRAME_SIZE_PX - 1,
            premul([16, 18, 24, 200]),
        ),
        (2, 0) => draw_hud_icon(tile, GEAR_COLOR),
        (2, 1) => draw_hud_icon(tile, MINIMAP_FRAME_COLOR),
        (2, 2) => draw_hud_icon(tile, ICON_BUILD_HQ_COLOR),
        (2, 3) => draw_hud_icon(tile, ICON_BUILD_DEPOT_COLOR),
        (3, 0) => draw_hud_icon(tile, ICON_BUILD_BARRACKS_COLOR),
        (3, 1) => draw_hud_icon(tile, ICON_TRAIN_WORKER_COLOR),
        (3, 2) => draw_hud_icon(tile, ICON_TRAIN_SOLDIER_COLOR),
        (3, 3) => draw_hud_icon(tile, ICON_SET_RALLY_COLOR),
        _ => {}
    }
}

/// One flat-filled square, inset by 4px — a HUD glyph cell (gear, minimap
/// frame, command icons).
fn draw_hud_icon(tile: &mut [u8], color: [u8; 4]) {
    blit_rect(tile, RTS_FRAME_SIZE_PX, 4, 4, 27, 27, color);
}

fn render_props_sheet() -> Vec<u8> {
    let mut sheet = vec![0u8; (RTS_ATLAS_WIDTH_PX * RTS_ATLAS_HEIGHT_PX * 4) as usize];
    for row in 0..RTS_FRAMES_Y {
        for col in 0..RTS_FRAMES_X {
            let mut tile = vec![0u8; (RTS_FRAME_SIZE_PX * RTS_FRAME_SIZE_PX * 4) as usize];
            draw_props_cell(&mut tile, row, col);
            let origin_x = col * RTS_FRAME_SIZE_PX;
            let origin_y = row * RTS_FRAME_SIZE_PX;
            blit_tile(&mut sheet, RTS_ATLAS_WIDTH_PX, origin_x, origin_y, &tile);
        }
    }
    sheet
}

/// 8x8 row-major glyph bitmaps for ASCII 32..=127, bit 7 = leftmost pixel.
/// Authored: space, digits, uppercase, and `. , : / - + ( ) % [ ] < > ! ?`.
/// Everything else (including all lowercase) is the fallback box glyph —
/// deliberate, since T3's `push_text` uppercases every HUD string and a box
/// glyph makes a missed uppercasing visible instead of silent.
const GLYPH_BITS: [[u8; 8]; 96] = [
    [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00], // 32 space
    [0x10, 0x10, 0x10, 0x10, 0x10, 0x00, 0x10, 0x00], // 33 !
    [0x7E, 0x42, 0x42, 0x42, 0x42, 0x42, 0x7E, 0x00], // 34 "
    [0x7E, 0x42, 0x42, 0x42, 0x42, 0x42, 0x7E, 0x00], // 35 #
    [0x7E, 0x42, 0x42, 0x42, 0x42, 0x42, 0x7E, 0x00], // 36 $
    [0x44, 0x48, 0x08, 0x10, 0x20, 0x24, 0x44, 0x00], // 37 %
    [0x7E, 0x42, 0x42, 0x42, 0x42, 0x42, 0x7E, 0x00], // 38 &
    [0x7E, 0x42, 0x42, 0x42, 0x42, 0x42, 0x7E, 0x00], // 39 '
    [0x08, 0x10, 0x20, 0x20, 0x20, 0x10, 0x08, 0x00], // 40 (
    [0x20, 0x10, 0x08, 0x08, 0x08, 0x10, 0x20, 0x00], // 41 )
    [0x7E, 0x42, 0x42, 0x42, 0x42, 0x42, 0x7E, 0x00], // 42 *
    [0x00, 0x10, 0x10, 0x7C, 0x10, 0x10, 0x00, 0x00], // 43 +
    [0x00, 0x00, 0x00, 0x00, 0x30, 0x30, 0x10, 0x00], // 44 ,
    [0x00, 0x00, 0x00, 0x7C, 0x00, 0x00, 0x00, 0x00], // 45 -
    [0x00, 0x00, 0x00, 0x00, 0x00, 0x30, 0x30, 0x00], // 46 .
    [0x04, 0x08, 0x10, 0x10, 0x20, 0x40, 0x00, 0x00], // 47 /
    [0x38, 0x44, 0x4C, 0x54, 0x64, 0x44, 0x38, 0x00], // 48 0
    [0x10, 0x30, 0x10, 0x10, 0x10, 0x10, 0x38, 0x00], // 49 1
    [0x38, 0x44, 0x04, 0x08, 0x10, 0x20, 0x7C, 0x00], // 50 2
    [0x38, 0x44, 0x04, 0x18, 0x04, 0x44, 0x38, 0x00], // 51 3
    [0x08, 0x18, 0x28, 0x48, 0x7C, 0x08, 0x08, 0x00], // 52 4
    [0x7C, 0x40, 0x78, 0x04, 0x04, 0x44, 0x38, 0x00], // 53 5
    [0x18, 0x20, 0x40, 0x78, 0x44, 0x44, 0x38, 0x00], // 54 6
    [0x7C, 0x04, 0x08, 0x10, 0x20, 0x20, 0x20, 0x00], // 55 7
    [0x38, 0x44, 0x44, 0x38, 0x44, 0x44, 0x38, 0x00], // 56 8
    [0x38, 0x44, 0x44, 0x3C, 0x04, 0x08, 0x30, 0x00], // 57 9
    [0x00, 0x30, 0x30, 0x00, 0x30, 0x30, 0x00, 0x00], // 58 :
    [0x7E, 0x42, 0x42, 0x42, 0x42, 0x42, 0x7E, 0x00], // 59 ;
    [0x08, 0x10, 0x20, 0x40, 0x20, 0x10, 0x08, 0x00], // 60 <
    [0x7E, 0x42, 0x42, 0x42, 0x42, 0x42, 0x7E, 0x00], // 61 =
    [0x20, 0x10, 0x08, 0x04, 0x08, 0x10, 0x20, 0x00], // 62 >
    [0x38, 0x44, 0x04, 0x08, 0x10, 0x00, 0x10, 0x00], // 63 ?
    [0x7E, 0x42, 0x42, 0x42, 0x42, 0x42, 0x7E, 0x00], // 64 @
    [0x38, 0x44, 0x44, 0x7C, 0x44, 0x44, 0x44, 0x00], // 65 A
    [0x78, 0x44, 0x44, 0x78, 0x44, 0x44, 0x78, 0x00], // 66 B
    [0x38, 0x44, 0x40, 0x40, 0x40, 0x44, 0x38, 0x00], // 67 C
    [0x78, 0x44, 0x44, 0x44, 0x44, 0x44, 0x78, 0x00], // 68 D
    [0x7C, 0x40, 0x40, 0x78, 0x40, 0x40, 0x7C, 0x00], // 69 E
    [0x7C, 0x40, 0x40, 0x78, 0x40, 0x40, 0x40, 0x00], // 70 F
    [0x38, 0x44, 0x40, 0x5C, 0x44, 0x44, 0x38, 0x00], // 71 G
    [0x44, 0x44, 0x44, 0x7C, 0x44, 0x44, 0x44, 0x00], // 72 H
    [0x38, 0x10, 0x10, 0x10, 0x10, 0x10, 0x38, 0x00], // 73 I
    [0x1C, 0x08, 0x08, 0x08, 0x48, 0x48, 0x30, 0x00], // 74 J
    [0x44, 0x48, 0x50, 0x60, 0x50, 0x48, 0x44, 0x00], // 75 K
    [0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x7C, 0x00], // 76 L
    [0x44, 0x6C, 0x54, 0x44, 0x44, 0x44, 0x44, 0x00], // 77 M
    [0x44, 0x64, 0x54, 0x4C, 0x44, 0x44, 0x44, 0x00], // 78 N
    [0x38, 0x44, 0x44, 0x44, 0x44, 0x44, 0x38, 0x00], // 79 O
    [0x78, 0x44, 0x44, 0x78, 0x40, 0x40, 0x40, 0x00], // 80 P
    [0x38, 0x44, 0x44, 0x44, 0x54, 0x48, 0x34, 0x00], // 81 Q
    [0x78, 0x44, 0x44, 0x78, 0x50, 0x48, 0x44, 0x00], // 82 R
    [0x3C, 0x40, 0x40, 0x38, 0x04, 0x04, 0x78, 0x00], // 83 S
    [0x7C, 0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x00], // 84 T
    [0x44, 0x44, 0x44, 0x44, 0x44, 0x44, 0x38, 0x00], // 85 U
    [0x44, 0x44, 0x44, 0x44, 0x44, 0x28, 0x10, 0x00], // 86 V
    [0x44, 0x44, 0x44, 0x54, 0x54, 0x6C, 0x44, 0x00], // 87 W
    [0x44, 0x44, 0x28, 0x10, 0x28, 0x44, 0x44, 0x00], // 88 X
    [0x44, 0x44, 0x28, 0x10, 0x10, 0x10, 0x10, 0x00], // 89 Y
    [0x7C, 0x04, 0x08, 0x10, 0x20, 0x40, 0x7C, 0x00], // 90 Z
    [0x18, 0x10, 0x10, 0x10, 0x10, 0x10, 0x18, 0x00], // 91 [
    [0x7E, 0x42, 0x42, 0x42, 0x42, 0x42, 0x7E, 0x00], // 92 backslash
    [0x30, 0x10, 0x10, 0x10, 0x10, 0x10, 0x30, 0x00], // 93 ]
    [0x7E, 0x42, 0x42, 0x42, 0x42, 0x42, 0x7E, 0x00], // 94 ^
    [0x7E, 0x42, 0x42, 0x42, 0x42, 0x42, 0x7E, 0x00], // 95 _
    [0x7E, 0x42, 0x42, 0x42, 0x42, 0x42, 0x7E, 0x00], // 96 `
    [0x7E, 0x42, 0x42, 0x42, 0x42, 0x42, 0x7E, 0x00], // 97 a
    [0x7E, 0x42, 0x42, 0x42, 0x42, 0x42, 0x7E, 0x00], // 98 b
    [0x7E, 0x42, 0x42, 0x42, 0x42, 0x42, 0x7E, 0x00], // 99 c
    [0x7E, 0x42, 0x42, 0x42, 0x42, 0x42, 0x7E, 0x00], // 100 d
    [0x7E, 0x42, 0x42, 0x42, 0x42, 0x42, 0x7E, 0x00], // 101 e
    [0x7E, 0x42, 0x42, 0x42, 0x42, 0x42, 0x7E, 0x00], // 102 f
    [0x7E, 0x42, 0x42, 0x42, 0x42, 0x42, 0x7E, 0x00], // 103 g
    [0x7E, 0x42, 0x42, 0x42, 0x42, 0x42, 0x7E, 0x00], // 104 h
    [0x7E, 0x42, 0x42, 0x42, 0x42, 0x42, 0x7E, 0x00], // 105 i
    [0x7E, 0x42, 0x42, 0x42, 0x42, 0x42, 0x7E, 0x00], // 106 j
    [0x7E, 0x42, 0x42, 0x42, 0x42, 0x42, 0x7E, 0x00], // 107 k
    [0x7E, 0x42, 0x42, 0x42, 0x42, 0x42, 0x7E, 0x00], // 108 l
    [0x7E, 0x42, 0x42, 0x42, 0x42, 0x42, 0x7E, 0x00], // 109 m
    [0x7E, 0x42, 0x42, 0x42, 0x42, 0x42, 0x7E, 0x00], // 110 n
    [0x7E, 0x42, 0x42, 0x42, 0x42, 0x42, 0x7E, 0x00], // 111 o
    [0x7E, 0x42, 0x42, 0x42, 0x42, 0x42, 0x7E, 0x00], // 112 p
    [0x7E, 0x42, 0x42, 0x42, 0x42, 0x42, 0x7E, 0x00], // 113 q
    [0x7E, 0x42, 0x42, 0x42, 0x42, 0x42, 0x7E, 0x00], // 114 r
    [0x7E, 0x42, 0x42, 0x42, 0x42, 0x42, 0x7E, 0x00], // 115 s
    [0x7E, 0x42, 0x42, 0x42, 0x42, 0x42, 0x7E, 0x00], // 116 t
    [0x7E, 0x42, 0x42, 0x42, 0x42, 0x42, 0x7E, 0x00], // 117 u
    [0x7E, 0x42, 0x42, 0x42, 0x42, 0x42, 0x7E, 0x00], // 118 v
    [0x7E, 0x42, 0x42, 0x42, 0x42, 0x42, 0x7E, 0x00], // 119 w
    [0x7E, 0x42, 0x42, 0x42, 0x42, 0x42, 0x7E, 0x00], // 120 x
    [0x7E, 0x42, 0x42, 0x42, 0x42, 0x42, 0x7E, 0x00], // 121 y
    [0x7E, 0x42, 0x42, 0x42, 0x42, 0x42, 0x7E, 0x00], // 122 z
    [0x7E, 0x42, 0x42, 0x42, 0x42, 0x42, 0x7E, 0x00], // 123 {
    [0x7E, 0x42, 0x42, 0x42, 0x42, 0x42, 0x7E, 0x00], // 124 |
    [0x7E, 0x42, 0x42, 0x42, 0x42, 0x42, 0x7E, 0x00], // 125 }
    [0x7E, 0x42, 0x42, 0x42, 0x42, 0x42, 0x7E, 0x00], // 126 ~
    [0x7E, 0x42, 0x42, 0x42, 0x42, 0x42, 0x7E, 0x00], // 127 DEL
];

fn render_font_sheet() -> Vec<u8> {
    let mut sheet = vec![0u8; (FONT_WIDTH_PX * FONT_HEIGHT_PX * 4) as usize];
    for (code, bits) in GLYPH_BITS.iter().enumerate() {
        let code = code as u32;
        let col = code % FONT_COLS;
        let row = code / FONT_COLS;
        let origin_x = col * GLYPH_W_PX;
        let origin_y = row * GLYPH_H_PX;
        for (ly, byte) in bits.iter().enumerate() {
            for lx in 0..GLYPH_W_PX {
                let set = (byte >> (7 - lx)) & 1 == 1;
                let rgba = if set {
                    [255, 255, 255, 255]
                } else {
                    [0, 0, 0, 0]
                };
                set_px(
                    &mut sheet,
                    FONT_WIDTH_PX,
                    origin_x + lx,
                    origin_y + ly as u32,
                    rgba,
                );
            }
        }
    }
    sheet
}

fn encode_png(width: u32, height: u32, pixels: &[u8]) -> Result<Vec<u8>, AtlasError> {
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(png::Compression::Fastest);
        encoder.set_filter(png::Filter::NoFilter);
        let mut writer = encoder
            .write_header()
            .map_err(|e| AtlasError::Encode(e.to_string()))?;
        writer
            .write_image_data(pixels)
            .map_err(|e| AtlasError::Encode(e.to_string()))?;
    }
    Ok(out)
}

/// Encode one rts sheet PNG; `id` indexes `RTS_FILES`.
pub fn encode_rts_png(id: u32) -> Result<Vec<u8>, AtlasError> {
    let pixels = match id {
        0 => render_unit_sheet(WORKER_BODY, WORKER_HEAD, PIP_COLOR),
        1 => render_unit_sheet(SOLDIER_BODY, SOLDIER_HEAD, PIP_COLOR),
        2 => render_buildings_sheet(),
        3 => render_props_sheet(),
        _ => {
            return Err(AtlasError::Layout(format!(
                "rts id {id} >= {}",
                RTS_FILES.len()
            )));
        }
    };
    encode_png(RTS_ATLAS_WIDTH_PX, RTS_ATLAS_HEIGHT_PX, &pixels)
}

/// Encode one ui sheet PNG; `id` indexes `UI_FILES`.
pub fn encode_ui_png(id: u32) -> Result<Vec<u8>, AtlasError> {
    match id {
        0 => encode_png(FONT_WIDTH_PX, FONT_HEIGHT_PX, &render_font_sheet()),
        _ => Err(AtlasError::Layout(format!(
            "ui id {id} >= {}",
            UI_FILES.len()
        ))),
    }
}

fn write_placeholder_manifest(
    out_dir: &Path,
    manifest: &PlaceholderManifest,
) -> Result<(), AtlasError> {
    let path = out_dir.join("manifest.json");
    let mut body =
        serde_json::to_string_pretty(manifest).map_err(|e| AtlasError::Manifest(e.to_string()))?;
    body.push('\n');
    fs::write(path, body).map_err(|e| AtlasError::Io(e.to_string()))
}

fn load_placeholder_manifest(out_dir: &Path) -> Result<PlaceholderManifest, AtlasError> {
    let path = out_dir.join("manifest.json");
    let raw = fs::read_to_string(&path).map_err(|e| AtlasError::Io(e.to_string()))?;
    serde_json::from_str(&raw).map_err(|e| AtlasError::Manifest(e.to_string()))
}

#[allow(clippy::too_many_arguments)]
fn generate_family(
    out_dir: &Path,
    generator: &str,
    frame_w: u32,
    frame_h: u32,
    cols: u32,
    rows: u32,
    files: &[&str],
    encode: impl Fn(u32) -> Result<Vec<u8>, AtlasError>,
) -> Result<PlaceholderManifest, AtlasError> {
    fs::create_dir_all(out_dir).map_err(|e| AtlasError::Io(e.to_string()))?;
    let mut images = Vec::with_capacity(files.len());
    for (id, file) in files.iter().enumerate() {
        let png = encode(id as u32)?;
        let path = out_dir.join(file);
        fs::write(&path, &png).map_err(|e| AtlasError::Io(e.to_string()))?;
        images.push(AtlasEntry {
            id: id as u32,
            file: file.to_string(),
            sha256: sha256_hex(&png),
        });
    }
    let manifest = PlaceholderManifest {
        version: PLACEHOLDER_MANIFEST_VERSION,
        generator: generator.to_string(),
        frame_width_px: frame_w,
        frame_height_px: frame_h,
        cols,
        rows,
        images,
    };
    write_placeholder_manifest(out_dir, &manifest)?;
    Ok(manifest)
}

/// Generate both placeholder families under `rts_out` / `ui_out`.
pub fn generate_placeholders(
    rts_out: &Path,
    ui_out: &Path,
) -> Result<(PlaceholderManifest, PlaceholderManifest), AtlasError> {
    let rts = generate_family(
        rts_out,
        RTS_GENERATOR_ID,
        RTS_FRAME_SIZE_PX,
        RTS_FRAME_SIZE_PX,
        RTS_FRAMES_X,
        RTS_FRAMES_Y,
        &RTS_FILES,
        encode_rts_png,
    )?;
    let ui = generate_family(
        ui_out,
        UI_GENERATOR_ID,
        GLYPH_W_PX,
        GLYPH_H_PX,
        FONT_COLS,
        FONT_ROWS,
        &UI_FILES,
        encode_ui_png,
    )?;
    Ok((rts, ui))
}

fn scratch_dir(tag: &str) -> Result<PathBuf, AtlasError> {
    let dir = std::env::temp_dir().join(format!(
        "mmd-placeholder-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    fs::create_dir_all(&dir).map_err(|e| AtlasError::Io(e.to_string()))?;
    Ok(dir)
}

#[allow(clippy::too_many_arguments)]
fn check_family(
    out_dir: &Path,
    generator: &str,
    frame_w: u32,
    frame_h: u32,
    cols: u32,
    rows: u32,
    files: &[&str],
    encode: impl Fn(u32) -> Result<Vec<u8>, AtlasError>,
) -> Result<(), AtlasError> {
    let on_disk = load_placeholder_manifest(out_dir)?;
    let tmp = scratch_dir(generator)?;
    let fresh = generate_family(&tmp, generator, frame_w, frame_h, cols, rows, files, encode)?;
    if fresh != on_disk {
        return Err(AtlasError::Check(format!(
            "{generator}: regenerated manifest differs from tracked manifest.json"
        )));
    }
    for entry in &on_disk.images {
        let tracked =
            fs::read(out_dir.join(&entry.file)).map_err(|e| AtlasError::Io(e.to_string()))?;
        let expected = sha256_hex(&tracked);
        if expected != entry.sha256 {
            return Err(AtlasError::HashMismatch {
                file: entry.file.clone(),
                expected: entry.sha256.clone(),
                actual: expected,
            });
        }
        let regenerated =
            fs::read(tmp.join(&entry.file)).map_err(|e| AtlasError::Io(e.to_string()))?;
        if tracked != regenerated {
            return Err(AtlasError::Check(format!(
                "png bytes drift for {}",
                entry.file
            )));
        }
        assert_premultiplied(&entry.file, &tracked)?;
    }
    Ok(())
}

/// Verify tracked placeholder families match a clean regeneration.
pub fn check_placeholders(rts_out: &Path, ui_out: &Path) -> Result<(), AtlasError> {
    check_family(
        rts_out,
        RTS_GENERATOR_ID,
        RTS_FRAME_SIZE_PX,
        RTS_FRAME_SIZE_PX,
        RTS_FRAMES_X,
        RTS_FRAMES_Y,
        &RTS_FILES,
        encode_rts_png,
    )?;
    check_family(
        ui_out,
        UI_GENERATOR_ID,
        GLYPH_W_PX,
        GLYPH_H_PX,
        FONT_COLS,
        FONT_ROWS,
        &UI_FILES,
        encode_ui_png,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn temp_out() -> PathBuf {
        tempfile::tempdir().expect("tempdir").keep()
    }

    fn decode_png(bytes: &[u8]) -> (u32, u32, Vec<u8>) {
        let decoder = png::Decoder::new(Cursor::new(bytes));
        let mut reader = decoder.read_info().expect("info");
        let mut buf = vec![0; reader.output_buffer_size().unwrap_or(0)];
        let info = reader.next_frame(&mut buf).expect("frame");
        buf.truncate(info.buffer_size());
        (info.width, info.height, buf)
    }

    /// Extract one `RTS_FRAME_SIZE_PX` cell from a decoded rts sheet at
    /// grid `(col, row)` — works for both `(frame, dir)` unit sheets and
    /// `(col, row)` static tables since the addressing math is identical.
    fn extract_cell(px: &[u8], w: u32, col: u32, row: u32) -> Vec<u8> {
        let origin_x = col * RTS_FRAME_SIZE_PX;
        let origin_y = row * RTS_FRAME_SIZE_PX;
        let mut out = Vec::with_capacity((RTS_FRAME_SIZE_PX * RTS_FRAME_SIZE_PX * 4) as usize);
        for ly in 0..RTS_FRAME_SIZE_PX {
            for lx in 0..RTS_FRAME_SIZE_PX {
                let x = origin_x + lx;
                let y = origin_y + ly;
                let idx = ((y * w + x) * 4) as usize;
                out.extend_from_slice(&px[idx..idx + 4]);
            }
        }
        out
    }

    fn pip_centroid_x(px: &[u8], w: u32, frame: u32, dir: u32) -> f64 {
        let origin_x = frame * RTS_FRAME_SIZE_PX;
        let origin_y = dir * RTS_FRAME_SIZE_PX;
        let mut sum = 0f64;
        let mut count = 0f64;
        for ly in 0..RTS_FRAME_SIZE_PX {
            for lx in 0..RTS_FRAME_SIZE_PX {
                let x = origin_x + lx;
                let y = origin_y + ly;
                let idx = ((y * w + x) * 4) as usize;
                let rgba = [px[idx], px[idx + 1], px[idx + 2], px[idx + 3]];
                if rgba == PIP_COLOR {
                    sum += f64::from(lx);
                    count += 1.0;
                }
            }
        }
        assert!(count > 0.0, "no pip pixels found dir={dir}");
        sum / count
    }

    fn extract_glyph_cell(px: &[u8], w: u32, code: u8) -> Vec<u8> {
        let idx = u32::from(code - FONT_FIRST_CHAR);
        let col = idx % FONT_COLS;
        let row = idx / FONT_COLS;
        let origin_x = col * GLYPH_W_PX;
        let origin_y = row * GLYPH_H_PX;
        let mut out = Vec::with_capacity((GLYPH_W_PX * GLYPH_H_PX * 4) as usize);
        for ly in 0..GLYPH_H_PX {
            for lx in 0..GLYPH_W_PX {
                let x = origin_x + lx;
                let y = origin_y + ly;
                let i = ((y * w + x) * 4) as usize;
                out.extend_from_slice(&px[i..i + 4]);
            }
        }
        out
    }

    fn glyph_pixels(bits: [u8; 8]) -> Vec<u8> {
        let mut out = Vec::with_capacity((GLYPH_W_PX * GLYPH_H_PX * 4) as usize);
        for byte in bits.iter() {
            for col in 0..8u32 {
                let set = (byte >> (7 - col)) & 1 == 1;
                if set {
                    out.extend_from_slice(&[255, 255, 255, 255]);
                } else {
                    out.extend_from_slice(&[0, 0, 0, 0]);
                }
            }
        }
        out
    }

    #[test]
    fn generates_both_placeholder_families() {
        let rts_out = temp_out();
        let ui_out = temp_out();
        let (rts, ui) = generate_placeholders(&rts_out, &ui_out).expect("generate");
        assert_eq!(rts.images.len(), 4);
        assert_eq!(ui.images.len(), 1);
        assert_eq!(rts.version, PLACEHOLDER_MANIFEST_VERSION);
        assert_eq!(ui.version, PLACEHOLDER_MANIFEST_VERSION);
        assert_eq!(rts.generator, RTS_GENERATOR_ID);
        assert_eq!(ui.generator, UI_GENERATOR_ID);
    }

    #[test]
    fn placeholder_generation_is_idempotent() {
        let rts_out1 = temp_out();
        let ui_out1 = temp_out();
        let (rts1, ui1) = generate_placeholders(&rts_out1, &ui_out1).expect("gen1");
        let rts_out2 = temp_out();
        let ui_out2 = temp_out();
        let (rts2, ui2) = generate_placeholders(&rts_out2, &ui_out2).expect("gen2");
        assert_eq!(rts1, rts2);
        assert_eq!(ui1, ui2);
        for entry in &rts1.images {
            let a = fs::read(rts_out1.join(&entry.file)).expect("a");
            let b = fs::read(rts_out2.join(&entry.file)).expect("b");
            assert_eq!(a, b, "{}", entry.file);
        }
        for entry in &ui1.images {
            let a = fs::read(ui_out1.join(&entry.file)).expect("a");
            let b = fs::read(ui_out2.join(&entry.file)).expect("b");
            assert_eq!(a, b, "{}", entry.file);
        }
    }

    #[test]
    fn rts_sheets_have_the_phase0_frame_geometry() {
        let rts_out = temp_out();
        let ui_out = temp_out();
        let (rts, _ui) = generate_placeholders(&rts_out, &ui_out).expect("gen");
        assert_eq!(rts.cols, 4);
        assert_eq!(rts.rows, 8);
        assert_eq!(rts.frame_width_px, 32);
        assert_eq!(rts.frame_height_px, 32);
        for entry in &rts.images {
            let bytes = fs::read(rts_out.join(&entry.file)).expect("png");
            let (w, h, _) = decode_png(&bytes);
            assert_eq!(w, RTS_ATLAS_WIDTH_PX);
            assert_eq!(h, RTS_ATLAS_HEIGHT_PX);
        }
    }

    #[test]
    fn font_sheet_has_the_locked_glyph_grid() {
        let rts_out = temp_out();
        let ui_out = temp_out();
        let (_rts, ui) = generate_placeholders(&rts_out, &ui_out).expect("gen");
        assert_eq!(ui.cols, FONT_COLS);
        assert_eq!(ui.rows, FONT_ROWS);
        assert_eq!(ui.frame_width_px, GLYPH_W_PX);
        assert_eq!(ui.frame_height_px, GLYPH_H_PX);
        let entry = &ui.images[0];
        let bytes = fs::read(ui_out.join(&entry.file)).expect("png");
        let (w, h, _) = decode_png(&bytes);
        assert_eq!(w, FONT_WIDTH_PX);
        assert_eq!(h, FONT_HEIGHT_PX);
    }

    #[test]
    fn every_generated_texel_is_premultiplied() {
        let rts_out = temp_out();
        let ui_out = temp_out();
        let (rts, ui) = generate_placeholders(&rts_out, &ui_out).expect("gen");
        for entry in &rts.images {
            let bytes = fs::read(rts_out.join(&entry.file)).expect("png");
            assert_premultiplied(&entry.file, &bytes).expect("premultiplied");
        }
        for entry in &ui.images {
            let bytes = fs::read(ui_out.join(&entry.file)).expect("png");
            assert_premultiplied(&entry.file, &bytes).expect("premultiplied");
        }
    }

    #[test]
    fn worker_and_soldier_differ() {
        let worker = encode_rts_png(0).expect("worker");
        let soldier = encode_rts_png(1).expect("soldier");
        assert_ne!(worker, soldier);
    }

    #[test]
    fn facing_pip_moves_with_the_direction_row() {
        let bytes = encode_rts_png(0).expect("worker");
        let (w, _h, px) = decode_png(&bytes);
        let east_x = pip_centroid_x(&px, w, 0, 0);
        let west_x = pip_centroid_x(&px, w, 0, 4);
        assert!(east_x > west_x, "east={east_x} west={west_x}");
    }

    #[test]
    fn animation_frames_are_not_all_equal() {
        let bytes = encode_rts_png(0).expect("worker");
        let (w, _h, px) = decode_png(&bytes);
        let tiles: Vec<Vec<u8>> = (0..4).map(|f| extract_cell(&px, w, f, 0)).collect();
        let all_equal = tiles.iter().all(|t| *t == tiles[0]);
        assert!(!all_equal, "all animation frames identical");
    }

    #[test]
    fn building_table_marks_construction() {
        let bytes = encode_rts_png(2).expect("buildings");
        let (w, _h, px) = decode_png(&bytes);
        let opaque = |col: u32, row: u32| -> u32 {
            extract_cell(&px, w, col, row)
                .chunks_exact(4)
                .filter(|p| p[3] > 0)
                .count() as u32
        };
        let finished = opaque(1, 0);
        let under_construction = opaque(1, 1);
        assert!(
            under_construction < finished,
            "uc={under_construction} finished={finished}"
        );
    }

    #[test]
    fn depleted_nodes_are_distinct_from_full_nodes() {
        let bytes = encode_rts_png(2).expect("buildings");
        let (w, _h, px) = decode_png(&bytes);
        assert_ne!(extract_cell(&px, w, 0, 2), extract_cell(&px, w, 2, 2));
        assert_ne!(extract_cell(&px, w, 1, 2), extract_cell(&px, w, 3, 2));
    }

    #[test]
    fn unused_table_rows_are_fully_transparent() {
        let buildings = encode_rts_png(2).expect("buildings");
        let (w, _h, px) = decode_png(&buildings);
        for row in 3..RTS_FRAMES_Y {
            for col in 0..RTS_FRAMES_X {
                let cell = extract_cell(&px, w, col, row);
                assert!(
                    cell.chunks_exact(4).all(|p| p == [0, 0, 0, 0]),
                    "buildings row={row} col={col} not transparent"
                );
            }
        }
        let props = encode_rts_png(3).expect("props");
        let (w2, _h2, px2) = decode_png(&props);
        for row in 4..RTS_FRAMES_Y {
            for col in 0..RTS_FRAMES_X {
                let cell = extract_cell(&px2, w2, col, row);
                assert!(
                    cell.chunks_exact(4).all(|p| p == [0, 0, 0, 0]),
                    "props row={row} col={col} not transparent"
                );
            }
        }
    }

    #[test]
    fn hud_icon_cells_are_opaque_and_distinct() {
        let props = encode_rts_png(3).expect("props");
        let (w, _h, px) = decode_png(&props);
        let mut cells = Vec::new();
        for row in 2..=3u32 {
            for col in 0..RTS_FRAMES_X {
                let cell = extract_cell(&px, w, col, row);
                assert!(
                    cell.chunks_exact(4).any(|p| p[3] > 0),
                    "row={row} col={col} must draw something"
                );
                cells.push(cell);
            }
        }
        for i in 0..cells.len() {
            for j in (i + 1)..cells.len() {
                assert_ne!(
                    cells[i], cells[j],
                    "HUD icon cells {i} and {j} are identical"
                );
            }
        }
    }

    #[test]
    fn glyph_for_capital_a_is_not_the_fallback_box() {
        let idx = (b'A' - FONT_FIRST_CHAR) as usize;
        assert_ne!(
            GLYPH_BITS[idx],
            [0x7E, 0x42, 0x42, 0x42, 0x42, 0x42, 0x7E, 0x00]
        );
    }

    #[test]
    fn lowercase_is_the_fallback_box() {
        let bytes = encode_ui_png(0).expect("font");
        let (w, _h, px) = decode_png(&bytes);
        let cell = extract_glyph_cell(&px, w, b'a');
        let expected = glyph_pixels([0x7E, 0x42, 0x42, 0x42, 0x42, 0x42, 0x7E, 0x00]);
        assert_eq!(cell, expected);
    }

    #[test]
    fn every_ascii_code_point_has_a_cell() {
        let bytes = encode_ui_png(0).expect("font");
        let (w, h, px) = decode_png(&bytes);
        for code in 32u8..=127 {
            let idx = u32::from(code - FONT_FIRST_CHAR);
            let col = idx % FONT_COLS;
            let row = idx / FONT_COLS;
            assert!(
                col * GLYPH_W_PX < w && row * GLYPH_H_PX < h,
                "code {code} out of bounds"
            );
            let cell = extract_glyph_cell(&px, w, code);
            assert_eq!(cell.len(), (GLYPH_W_PX * GLYPH_H_PX * 4) as usize);
        }
    }

    #[test]
    fn check_passes_on_freshly_generated_output() {
        let rts_out = temp_out();
        let ui_out = temp_out();
        generate_placeholders(&rts_out, &ui_out).expect("gen");
        check_placeholders(&rts_out, &ui_out).expect("check");
    }

    #[test]
    fn check_fails_on_a_drifted_png() {
        let rts_out = temp_out();
        let ui_out = temp_out();
        generate_placeholders(&rts_out, &ui_out).expect("gen");
        let path = rts_out.join("props.png");
        let mut bytes = fs::read(&path).expect("read");
        let last = bytes.len() - 1;
        bytes[last] ^= 0xFF;
        fs::write(&path, &bytes).expect("write");
        match check_placeholders(&rts_out, &ui_out) {
            Err(AtlasError::HashMismatch { .. }) => {}
            other => panic!("expected HashMismatch, got {other:?}"),
        }
    }

    #[test]
    fn check_fails_on_a_drifted_manifest() {
        let rts_out = temp_out();
        let ui_out = temp_out();
        generate_placeholders(&rts_out, &ui_out).expect("gen");
        let manifest_path = rts_out.join("manifest.json");
        let raw = fs::read_to_string(&manifest_path).expect("read");
        let mutated = raw.replace(RTS_GENERATOR_ID, "mmd-rts-placeholder-vX");
        fs::write(&manifest_path, mutated).expect("write");
        match check_placeholders(&rts_out, &ui_out) {
            Err(AtlasError::Check(_)) => {}
            other => panic!("expected Check error, got {other:?}"),
        }
    }
}
