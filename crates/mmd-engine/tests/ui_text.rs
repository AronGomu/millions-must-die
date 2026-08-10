//! T3 — headless tests for `render::text`: the pure, GPU-free half of bitmap
//! text packing. No GPU case lives here; the two that need a device are in
//! `gpu_smoke.rs`.

use mmd_engine::render::{
    AtlasRgba, FONT_COLS, FONT_FIRST_CHAR, FONT_LAST_CHAR, FONT_REPLACEMENT, RING_SENTINEL,
    SLOT_UI_FONT, SpriteInstance, begin_text_group, glyph_uv_rect, load_ui_font, push_text,
    text_width, ui_atlas_dir,
};
use mmd_engine::render::{DrawGroup, GLYPH_H_PX, GLYPH_W_PX};
use mmd_engine::workspace_root;

#[test]
fn glyph_rect_tiles_the_sheet_without_gaps() {
    for byte in FONT_FIRST_CHAR..=FONT_LAST_CHAR {
        let r = glyph_uv_rect(byte);
        assert!(
            (r[2] - r[0] - 8.0 / 128.0).abs() < 1e-6,
            "byte {byte}: u-span {r:?}"
        );
        assert!(
            (r[3] - r[1] - 8.0 / 48.0).abs() < 1e-6,
            "byte {byte}: v-span {r:?}"
        );
        for c in r {
            assert!((0.0..=1.0).contains(&c), "byte {byte}: {r:?} out of 0..=1");
        }
    }
}

#[test]
fn glyph_rects_are_unique_per_code_point() {
    let rects: Vec<[f32; 4]> = (FONT_FIRST_CHAR..=FONT_LAST_CHAR)
        .map(glyph_uv_rect)
        .collect();
    assert_eq!(rects.len(), 96);
    let mut unique = rects.clone();
    unique.sort_by(|a, b| a.partial_cmp(b).unwrap());
    unique.dedup();
    assert_eq!(unique.len(), 96, "expected 96 distinct glyph rects");
}

#[test]
fn out_of_range_byte_maps_to_the_replacement() {
    assert_eq!(glyph_uv_rect(200), glyph_uv_rect(b'?'));
}

#[test]
fn no_glyph_can_be_mistaken_for_a_ring() {
    for byte in FONT_FIRST_CHAR..=FONT_LAST_CHAR {
        let r = glyph_uv_rect(byte);
        assert!(r[0] >= 0.0, "byte {byte}: uv_rect.x {} < 0.0", r[0]);
        assert!(r[0] > RING_SENTINEL);
    }
}

#[test]
fn push_text_emits_one_instance_per_visible_glyph() {
    let mut out = Vec::new();
    push_text(&mut out, "AB1", [0.0, 0.0], 1.0, SpriteInstance::WHITE);
    assert_eq!(out.len(), 3);
}

#[test]
fn space_advances_without_an_instance() {
    let mut out = Vec::new();
    push_text(&mut out, "A B", [0.0, 0.0], 1.0, SpriteInstance::WHITE);
    assert_eq!(out.len(), 2);
    assert_eq!(out[1].pos[0], out[0].pos[0] + 2.0 * 8.0 * 1.0);
}

#[test]
fn a_string_of_spaces_emits_nothing() {
    let mut out = Vec::new();
    let advance = push_text(&mut out, "     ", [0.0, 0.0], 1.0, SpriteInstance::WHITE);
    assert_eq!(out.len(), 0);
    assert_eq!(advance, 40.0);
}

#[test]
fn lowercase_is_uppercased_not_boxed() {
    let mut lower = Vec::new();
    let mut upper = Vec::new();
    push_text(&mut lower, "abc", [1.0, 2.0], 1.0, SpriteInstance::WHITE);
    push_text(&mut upper, "ABC", [1.0, 2.0], 1.0, SpriteInstance::WHITE);
    assert_eq!(lower, upper);
}

#[test]
fn advance_matches_text_width() {
    for scale in [1.0f32, 2.5] {
        let mut out = Vec::new();
        let advance = push_text(
            &mut out,
            "HELLO 123",
            [0.0, 0.0],
            scale,
            SpriteInstance::WHITE,
        );
        assert_eq!(advance, text_width("HELLO 123", scale));
    }
}

#[test]
fn glyphs_advance_left_to_right_only() {
    let mut out = Vec::new();
    push_text(&mut out, "ABCD", [10.0, 20.0], 1.0, SpriteInstance::WHITE);
    assert_eq!(out.len(), 4);
    for w in out.windows(2) {
        assert_eq!(w[0].pos[1], w[1].pos[1]);
        assert_eq!(w[1].pos[0] - w[0].pos[0], 8.0);
    }
}

#[test]
fn scale_scales_size_and_advance() {
    let mut out = Vec::new();
    let advance = push_text(&mut out, "A", [0.0, 0.0], 3.0, SpriteInstance::WHITE);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].size, [24.0, 24.0]);
    assert_eq!(advance, 24.0);
}

#[test]
fn tint_is_carried_through() {
    let tint = [0.1, 0.2, 0.3, 0.4];
    let mut out = Vec::new();
    push_text(&mut out, "AB1", [0.0, 0.0], 1.0, tint);
    assert!(!out.is_empty());
    for inst in &out {
        assert_eq!(inst.tint, tint);
    }
}

#[test]
fn newline_draws_the_replacement_glyph() {
    let mut out = Vec::new();
    push_text(&mut out, "A\nB", [0.0, 0.0], 1.0, SpriteInstance::WHITE);
    assert_eq!(out.len(), 3);
    assert_eq!(out[1].uv_rect, glyph_uv_rect(b'?'));
}

#[test]
fn non_ascii_char_costs_exactly_one_cell() {
    let mut out = Vec::new();
    push_text(&mut out, "Aé", [0.0, 0.0], 1.0, SpriteInstance::WHITE);
    assert_eq!(out.len(), 2);
    assert_eq!(out[1].uv_rect, glyph_uv_rect(FONT_REPLACEMENT));
    assert_eq!(text_width("Aé", 1.0), 16.0);
}

#[test]
fn push_text_does_not_allocate_when_reserved() {
    let mut out = Vec::with_capacity(64);
    let cap_before = out.capacity();
    let text = "A".repeat(32);
    push_text(&mut out, &text, [0.0, 0.0], 1.0, SpriteInstance::WHITE);
    assert_eq!(out.len(), 32);
    assert_eq!(out.capacity(), cap_before, "push_text must not reallocate");
}

#[test]
fn begin_text_group_sets_the_font_slot() {
    let mut group = DrawGroup {
        atlas_id: 3,
        instances: vec![SpriteInstance::new(
            [0.0, 0.0],
            [1.0, 1.0],
            [0.0, 0.0, 1.0, 1.0],
            SpriteInstance::WHITE,
        )],
    };
    let cap_before = group.instances.capacity();
    begin_text_group(&mut group);
    assert_eq!(group.atlas_id, SLOT_UI_FONT);
    assert!(group.instances.is_empty());
    assert_eq!(group.instances.capacity(), cap_before);
}

/// Byte pattern of the sheet's fallback box glyph, exactly as authored (T1).
const BOX_BITS: [u8; 8] = [0x7E, 0x42, 0x42, 0x42, 0x42, 0x42, 0x7E, 0x00];

/// Decode [`BOX_BITS`] into the 64 RGBA texels it authors: set bit → opaque
/// white, clear bit → transparent black (T1's stated pixel format).
fn box_texels() -> [[u8; 4]; 64] {
    let mut out = [[0u8; 4]; 64];
    for (row, bits) in BOX_BITS.iter().enumerate() {
        for col in 0..8u32 {
            let set = (bits >> (7 - col)) & 1 == 1;
            out[row * 8 + col as usize] = if set {
                [255, 255, 255, 255]
            } else {
                [0, 0, 0, 0]
            };
        }
    }
    out
}

/// The 64 texels of one glyph cell in the tracked font sheet.
fn glyph_texels(atlas: &AtlasRgba, byte: u8) -> [[u8; 4]; 64] {
    let i = (byte - FONT_FIRST_CHAR) as u32;
    let col = i % FONT_COLS;
    let row = i / FONT_COLS;
    let x0 = col * GLYPH_W_PX as u32;
    let y0 = row * GLYPH_H_PX as u32;
    let mut out = [[0u8; 4]; 64];
    for dy in 0..8u32 {
        for dx in 0..8u32 {
            out[(dy * 8 + dx) as usize] = atlas.pixel(x0 + dx, y0 + dy);
        }
    }
    out
}

#[test]
fn authored_glyphs_are_not_the_fallback_box() {
    let atlas = load_ui_font(&ui_atlas_dir(&workspace_root())).expect("load tracked font sheet");
    let box_texels = box_texels();
    for byte in [b'A', b'0', b':', b'%'] {
        let texels = glyph_texels(&atlas, byte);
        assert_ne!(
            texels, box_texels,
            "byte {byte} ({:?}) reads as the fallback box",
            byte as char
        );
    }
}

#[test]
fn lowercase_cell_is_the_fallback_box() {
    let atlas = load_ui_font(&ui_atlas_dir(&workspace_root())).expect("load tracked font sheet");
    let texels = glyph_texels(&atlas, b'a');
    assert_eq!(texels, box_texels(), "'a' cell is not the fallback box");
}
