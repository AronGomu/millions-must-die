//! Bitmap-font text packing for the screen-space UI layer.

use crate::render::atlas::SLOT_UI_FONT;
use crate::render::instance::SpriteInstance;
use crate::render::renderer::DrawGroup;

/// Glyph cell width in the tracked font sheet, in source pixels.
pub const GLYPH_W_PX: f32 = 8.0;
/// Glyph cell height in the tracked font sheet, in source pixels.
pub const GLYPH_H_PX: f32 = 8.0;
/// Glyph cells across the sheet.
pub const FONT_COLS: u32 = 16;
/// Glyph cell rows down the sheet.
pub const FONT_ROWS: u32 = 6;
/// First ASCII code point the sheet carries.
pub const FONT_FIRST_CHAR: u8 = 32;
/// Last ASCII code point the sheet carries, inclusive.
pub const FONT_LAST_CHAR: u8 = 127;
/// Byte substituted for anything outside `FONT_FIRST_CHAR..=FONT_LAST_CHAR`.
///
/// `?` rather than the box glyph: the box already means "in range but not
/// authored", and collapsing the two would hide a caller feeding non-ASCII.
pub const FONT_REPLACEMENT: u8 = b'?';
/// Extra pixels between glyph cells, in unscaled font pixels.
///
/// Zero: the 8x8 cell already carries one blank column on the right of every
/// authored 5x7 form, so glyphs do not touch and no second spacing rule can
/// drift from the art.
pub const GLYPH_TRACKING_PX: f32 = 0.0;

/// UV rect of one glyph cell, for a byte already inside the sheet's range.
///
/// Out-of-range bytes resolve to [`FONT_REPLACEMENT`] rather than panicking, so
/// a HUD string can never abort a frame.
pub fn glyph_uv_rect(byte: u8) -> [f32; 4] {
    let b = if (FONT_FIRST_CHAR..=FONT_LAST_CHAR).contains(&byte) {
        byte
    } else {
        FONT_REPLACEMENT
    };
    let i = (b - FONT_FIRST_CHAR) as u32;
    let col = i % FONT_COLS;
    let row = i / FONT_COLS;
    let sheet_w = FONT_COLS as f32 * GLYPH_W_PX; // 128.0
    let sheet_h = FONT_ROWS as f32 * GLYPH_H_PX; // 48.0
    [
        col as f32 * GLYPH_W_PX / sheet_w,
        row as f32 * GLYPH_H_PX / sheet_h,
        (col + 1) as f32 * GLYPH_W_PX / sheet_w,
        (row + 1) as f32 * GLYPH_H_PX / sheet_h,
    ]
}

/// Advance width of `text` at `scale`, in screen pixels, without packing.
pub fn text_width(text: &str, scale: f32) -> f32 {
    text.chars().count() as f32 * (GLYPH_W_PX + GLYPH_TRACKING_PX) * scale
}

/// Append one instance per drawn glyph and return the advance width.
///
/// `pos` is the **top-left** of the first glyph cell, in screen pixels, matching
/// `SpriteInstance::pos`. Glyphs advance along +x only; `push_text` never wraps
/// and never inserts a newline — a `\n` in `text` is an unmapped byte and draws
/// the replacement glyph, which is what makes an accidental multi-line string
/// visible instead of silently clipped.
///
/// ASCII lowercase is uppercased before lookup. A space advances the cursor and
/// pushes **no** instance: a blank cell would be an invisible quad the GPU still
/// rasterises, and the HUD pads with spaces.
///
/// `tint` is premultiplied RGBA, as everywhere else in the renderer.
///
/// Allocates nothing when `out` has spare capacity.
pub fn push_text(
    out: &mut Vec<SpriteInstance>,
    text: &str,
    pos: [f32; 2],
    scale: f32,
    tint: [f32; 4],
) -> f32 {
    let advance = (GLYPH_W_PX + GLYPH_TRACKING_PX) * scale;
    let size = [GLYPH_W_PX * scale, GLYPH_H_PX * scale];
    let mut cursor_x = pos[0];
    for ch in text.chars() {
        if ch == ' ' {
            cursor_x += advance;
            continue;
        }
        let byte = if ch.is_ascii() {
            (ch as u8).to_ascii_uppercase()
        } else {
            FONT_REPLACEMENT
        };
        out.push(SpriteInstance::new(
            [cursor_x, pos[1]],
            size,
            glyph_uv_rect(byte),
            tint,
        ));
        cursor_x += advance;
    }
    cursor_x - pos[0]
}

/// Clear `group` and repack it as the font slot's UI group.
///
/// A convenience for the HUD: sets `atlas_id` to [`SLOT_UI_FONT`] and clears the
/// instance vector without releasing its capacity.
pub fn begin_text_group(group: &mut DrawGroup) {
    group.atlas_id = SLOT_UI_FONT;
    group.instances.clear();
}
