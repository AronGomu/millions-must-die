# T3: Bitmap text packer

**Plan:** `./ai-artifacts/PLAN_2026_08_10_rts-engine-prototype.md`
**Depends:** T2
**Commit outcome:** `render::text::push_text` turns a `&str` into UI sprite instances that draw legible glyphs from the tracked font sheet.

## Context (self-contained)

- Goal: phase 1 is a thin vertical slice of an RTS engine prototype (camera,
  selection, workers, economy, building, unit production). It needs an on-screen
  HUD — resource totals, supply, selection, a build menu — because a prototype
  nobody can read is not demonstrable.
- This slice: the pure, headless, GPU-free half of text rendering. It converts
  strings into `SpriteInstance`s addressing the UI font slot. It draws nothing
  itself and knows nothing about the HUD's content; T13 composes the HUD from it.
- Out of scope here: the HUD layout, resource values, any RTS system, any change
  to the renderer or the shader, panels or backgrounds beyond the one prop cell
  named below.
- Assumptions in force: only uppercase, digits and a fixed punctuation set have
  real glyphs; everything else is a fallback box. `push_text` uppercases ASCII
  input, so a caller passing lowercase gets correct output rather than boxes —
  and the box glyph then only appears for genuinely unmapped characters.

## Requirements

- New module `crates/mmd-engine/src/render/text.rs`, exported from
  `crates/mmd-engine/src/render/mod.rs`.
- Glyph geometry constants mirroring the tracked sheet.
- `glyph_uv_rect(byte) -> [f32; 4]` — the UV rect of one glyph cell.
- `push_text(...)` — append one `SpriteInstance` per **drawn** glyph to a caller
  owned `Vec`, returning the advance width in pixels.
- `text_width(text, scale) -> f32` — the same advance without packing, so a
  caller can right-align or centre before it commits instances.
- Space is an advance with **no instance**: a HUD line of ten spaces must cost
  zero instances.
- Zero allocation when the caller's `Vec` has capacity.

## Inputs

- **Files to read**
  - `crates/mmd-engine/src/render/instance.rs` — `SpriteInstance::new`,
    `SpriteInstance::WHITE`.
  - `crates/mmd-engine/src/render/atlas.rs` — `frame_uv_rect` for the pattern to
    mirror.
  - `crates/mmd-engine/src/render/renderer.rs` — `DrawGroup`.
- **From Depends (T2) — spell out, the worker cannot read T2:**
  - The renderer now has a **flat texture table of 9 slots**. `DrawGroup.atlas_id`
    is a slot index, valid in `0..ATLAS_SLOT_COUNT` where
    `pub const ATLAS_SLOT_COUNT: usize = 9`.
  - Slot constants exported from `mmd_engine::render`:
    `SLOT_RTS_WORKER = 4`, `SLOT_RTS_SOLDIER = 5`, `SLOT_RTS_BUILDINGS = 6`,
    `SLOT_RTS_PROPS = 7`, `SLOT_UI_FONT = 8`.
  - A frame is submitted as
    ```rust
    pub struct ScenePass<'a> {
        pub world: &'a [DrawGroup],
        pub overlay: &'a [SpriteInstance],
        pub ui: &'a [DrawGroup],
    }
    ```
    via `SpriteRenderer::draw_offscreen_scene(scene)` /
    `draw_to_swapchain_scene(window, scene)` /
    `draw_offscreen_readback_scene(scene)`. `ui` is drawn **last**, on the
    depth-test-off pipeline, in screen pixels.
  - `SpriteRenderer::atlas_pixels(slot: u32) -> Option<&AtlasRgba>` gives CPU
    texels for a slot; `AtlasRgba::pixel(x, y) -> [u8; 4]`.
- **From T1 (asset facts you must not rediscover):**
  - `assets/sprites/generated/ui/font.png` is **128 × 48 px**: 16 columns × 6
    rows of 8 × 8 px glyph cells, covering ASCII `32..=127` in code-point order.
    Glyph for byte `c` is at column `(c - 32) % 16`, row `(c - 32) / 16`.
  - Set bits are opaque white `[255, 255, 255, 255]`; clear bits are
    `[0, 0, 0, 0]`.
  - Real glyphs exist for: space, `0`–`9`, `A`–`Z`, and
    `. , : / - + ( ) % [ ] < > ! ?`. Everything else in range — **including all
    lowercase** — is the fallback box
    `[0x7E, 0x42, 0x42, 0x42, 0x42, 0x42, 0x7E, 0x00]`.
  - `assets/sprites/generated/rts/props.png` is 128 × 256, a 4 × 8 grid of 32 px
    cells. Cell `(row 1, col 3)` is the **panel fill**: every texel
    `[13, 14, 19, 200]` (premultiplied `[16, 18, 24, 200]`).

## Exact design — no decisions left

```rust
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
pub fn glyph_uv_rect(byte: u8) -> [f32; 4];

/// Advance width of `text` at `scale`, in screen pixels, without packing.
pub fn text_width(text: &str, scale: f32) -> f32;

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
) -> f32;

/// Clear `group` and repack it as the font slot's UI group.
///
/// A convenience for the HUD: sets `atlas_id` to [`SLOT_UI_FONT`] and clears the
/// instance vector without releasing its capacity.
pub fn begin_text_group(group: &mut DrawGroup);
```

Implementation rules, exactly:

- `glyph_uv_rect(byte)`:
  ```rust
  let b = if (FONT_FIRST_CHAR..=FONT_LAST_CHAR).contains(&byte) { byte } else { FONT_REPLACEMENT };
  let i = (b - FONT_FIRST_CHAR) as u32;
  let col = i % FONT_COLS;
  let row = i / FONT_COLS;
  let sheet_w = FONT_COLS as f32 * GLYPH_W_PX;   // 128.0
  let sheet_h = FONT_ROWS as f32 * GLYPH_H_PX;   // 48.0
  [ col as f32 * GLYPH_W_PX / sheet_w,
    row as f32 * GLYPH_H_PX / sheet_h,
    (col + 1) as f32 * GLYPH_W_PX / sheet_w,
    (row + 1) as f32 * GLYPH_H_PX / sheet_h ]
  ```
- Advance per glyph is `(GLYPH_W_PX + GLYPH_TRACKING_PX) * scale`.
- Quad size per glyph is `[GLYPH_W_PX * scale, GLYPH_H_PX * scale]`.
- `text_width(text, scale) == text.chars().count() as f32 * (GLYPH_W_PX + GLYPH_TRACKING_PX) * scale`
  for ASCII input. Non-ASCII `char`s count as one cell each (they map to
  `FONT_REPLACEMENT`), so width is byte-independent and stays alignable.
- `push_text` iterates `text.chars()`; a `char` above `0x7F` is treated as
  `FONT_REPLACEMENT`. `to_ascii_uppercase` is applied to the byte, not the char.
- Every emitted `uv_rect[0]` is `>= 0.0`, so no glyph can ever collide with the
  shader's ring sentinel (`RING_SENTINEL = -1.0`, branch taken when
  `uv_rect.x < 0.0`). Assert this in a test.

## TDD

1. **Red** — write the tests below in a new
   `crates/mmd-engine/tests/ui_text.rs`, plus the two GPU cases in
   `crates/mmd-engine/tests/gpu_smoke.rs`. Watch them fail.
2. **Green** — implement `render/text.rs`.
3. **Refactor** — none expected. Keep green.

## Test plan

| Test | Input | Expect |
| ---- | ----- | ------ |
| `glyph_rect_tiles_the_sheet_without_gaps` | bytes 32..=127 | for each, `u1 - u0 == 8.0/128.0` and `v1 - v0 == 8.0/48.0`, all within `0.0..=1.0` |
| `glyph_rects_are_unique_per_code_point` | bytes 32..=127 | 96 distinct rects |
| `out_of_range_byte_maps_to_the_replacement` | `glyph_uv_rect(200)` | equals `glyph_uv_rect(b'?')` |
| `no_glyph_can_be_mistaken_for_a_ring` | bytes 32..=127 | every `uv_rect[0] >= 0.0` |
| `push_text_emits_one_instance_per_visible_glyph` | `"AB1"` | `out.len() == 3` |
| `space_advances_without_an_instance` | `"A B"` | `out.len() == 2`, and the second instance's `pos[0] == pos0[0] + 2.0 * 8.0 * scale` |
| `a_string_of_spaces_emits_nothing` | `"     "` | `out.len() == 0`, return value `40.0` at `scale == 1.0` |
| `lowercase_is_uppercased_not_boxed` | `"abc"` vs `"ABC"` | identical instance vectors |
| `advance_matches_text_width` | `"HELLO 123"`, `scale` 1.0 and 2.5 | `push_text` return equals `text_width` for both |
| `glyphs_advance_left_to_right_only` | `"ABCD"` | `pos[1]` equal for all four; `pos[0]` strictly increasing by `8.0 * scale` |
| `scale_scales_size_and_advance` | `"A"` at scale 3.0 | `size == [24.0, 24.0]`, return `24.0` |
| `tint_is_carried_through` | tint `[0.1, 0.2, 0.3, 0.4]` | every instance's `tint` equals it |
| `newline_draws_the_replacement_glyph` | `"A\nB"` | 3 instances; the middle one's uv equals `glyph_uv_rect(b'?')` |
| `non_ascii_char_costs_exactly_one_cell` | `"Aé"` | 2 instances, second uv equals the replacement's; `text_width` returns `16.0` at scale 1.0 |
| `push_text_does_not_allocate_when_reserved` | `Vec::with_capacity(64)`, then `push_text` of 32 glyphs | capacity unchanged (assert `out.capacity()` before and after) |
| `begin_text_group_sets_the_font_slot` | a `DrawGroup { atlas_id: 3, instances: vec![x] }` | `atlas_id == SLOT_UI_FONT`, `instances.is_empty()`, capacity retained |
| `authored_glyphs_are_not_the_fallback_box` (GPU-free, reads the tracked PNG via `render::load_ui_font`) | `'A'`, `'0'`, `':'`, `'%'` | each glyph cell's 64 texels differ from the box glyph's texels |
| `lowercase_cell_is_the_fallback_box` (same) | `'a'` cell in the sheet | equals the box glyph's texels |
| `text_renders_visible_pixels` (GPU) | `push_text(&mut v, "A", [64.0, 64.0], 4.0, WHITE)` into a UI group on `SLOT_UI_FONT`, `draw_offscreen_readback_scene` | at least 8 pixels in the rect `64..96 × 64..96` have `a == 255` |
| `text_is_drawn_over_the_world` (GPU) | an opaque world quad covering `64..96 × 64..96` on slot 0, plus the above UI glyph | at least one pixel inside the glyph's set-bit area is white `[255,255,255,255]` |

**Mutation verification (mandatory).** Inject, confirm red, revert, confirm green:
1. `GLYPH_TRACKING_PX = 1.0` → kills `advance_matches_text_width` only if
   `text_width` is *not* also derived from the constant; make sure one of
   `glyphs_advance_left_to_right_only` or `space_advances_without_an_instance`
   hardcodes `8.0` so the constant cannot be moved silently. It does.
2. Emit an instance for space → kills `a_string_of_spaces_emits_nothing`.
3. Drop the uppercasing → kills `lowercase_is_uppercased_not_boxed`.
4. `glyph_uv_rect` swaps `col` and `row` → kills `glyph_rect_tiles_the_sheet_without_gaps`.
5. `FONT_REPLACEMENT = b'A'` → kills `newline_draws_the_replacement_glyph`.
6. Return `0.0` from `push_text` → kills `advance_matches_text_width`.

## Impl steps

- [x] 1. Create `crates/mmd-engine/src/render/text.rs` with the constants block verbatim.
- [x] 2. Add `mod text;` and the `pub use text::{...}` line to `crates/mmd-engine/src/render/mod.rs`, exporting every public item listed above.
- [x] 3. Create `crates/mmd-engine/tests/ui_text.rs` and write every headless test from the table. Watch them fail.
- [x] 4. Add the two GPU tests to `crates/mmd-engine/tests/gpu_smoke.rs`, using the existing device auto-skip helper from `crates/mmd-engine/tests/common/mod.rs`. (Plan defect: no such helper exists in `tests/common/mod.rs`; the actual auto-skip pattern the repo uses for the merge gate lives as private fns in `tests/render_correctness.rs`. Replicated that pattern locally in `gpu_smoke.rs` — see Assumptions in the worker report.)
- [x] 5. Implement `glyph_uv_rect` exactly as quoted.
- [x] 6. Implement `text_width`.
- [x] 7. Implement `push_text` (uppercase, skip space, advance, size, tint).
- [x] 8. Implement `begin_text_group`.
- [x] 9. Run `cargo test -p mmd-engine --test ui_text` to green.
- [x] 10. Run `MMD_REQUIRE_GPU=1 cargo test -p mmd-engine --test gpu_smoke` to green.
- [x] 11. Run the mutation list; record kills in the commit body.
- [x] 12. Run the full validation block.

## Outputs

- **Files created**
  - `crates/mmd-engine/src/render/text.rs`
  - `crates/mmd-engine/tests/ui_text.rs`
- **Files edited**
  - `crates/mmd-engine/src/render/mod.rs`
  - `crates/mmd-engine/tests/gpu_smoke.rs`
- **Public API added:** `render::text::{GLYPH_W_PX, GLYPH_H_PX, FONT_COLS, FONT_ROWS, FONT_FIRST_CHAR, FONT_LAST_CHAR, FONT_REPLACEMENT, GLYPH_TRACKING_PX, glyph_uv_rect, text_width, push_text, begin_text_group}`.
- **Behaviour change:** none for any existing command. Nothing calls the packer yet.
- **Migration / config:** none.

## Validation

- [x] `cargo fmt --all -- --check`
- [x] `MMD_REQUIRE_GPU=1 cargo test --workspace --locked`
- [x] `VK_DRIVER_FILES=/nonexistent cargo test --workspace --locked` — GPU cases skip cleanly
- [x] `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- [x] `nix flake check`
- [x] `cargo run -p xtask -- atlases --check`
- [x] `git diff --stat HEAD -- lab/goldens/` — **empty**
- [x] `cargo run -- run --agents 5000 --frames 300` — exit 0
- [x] app functional — no broken path from this slice
- [x] commit msg draft: `feat(render): pack bitmap-font text into UI draw groups`
