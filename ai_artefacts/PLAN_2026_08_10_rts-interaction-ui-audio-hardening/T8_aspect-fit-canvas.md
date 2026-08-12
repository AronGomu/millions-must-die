# T8: Add aspect-fit logical canvas

**Plan:** `./ai_artefacts/PLAN_2026_08_10_rts-interaction-ui-audio-hardening.md`  
**Depends:** T1  
**Commit outcome:** fixed 1920×1080 frame fits any drawable in exact 16:9 content rect; bars clear; live pointer inverse matches visuals; script coords stay canonical.

## Context (self-contained)

- Goal: make fullscreen/windowed/resizing honest without responsive render rewrite.
- This slice: pure viewport math, destination blit, mouse inverse, content-edge motion.
- Out of scope here: mode switching, focus/grab, camera frontier, menu/HUD redesign.
- Assumptions: logical canvas remains 1920×1080; destination rect dimensions are exact multiples of 16×9; nearest filter stays; clicks in bars are ignored; bar motion clamps to content edge so edge-pan can work.
- Decision: `docs/ADR/018_ADR_settings_window_canvas_and_camera.md`.

## Requirements

- Create `crates/mmd-engine/src/render/viewport.rs`:
  ```rust
  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub struct RectU32 { pub x: u32, pub y: u32, pub w: u32, pub h: u32 }

  #[derive(Clone, Copy, Debug, PartialEq)]
  pub struct DisplayViewport {
      pub window_units: [u32; 2],
      pub drawable_px: [u32; 2],
      pub content_px: RectU32,
  }

  #[derive(Clone, Copy, Debug, PartialEq)]
  pub struct MappedPointer { pub logical: [f32; 2], pub inside_content: bool }

  pub fn aspect_fit_16_9(drawable_px: [u32; 2]) -> Option<RectU32>;
  impl DisplayViewport {
      pub fn new(window_units: [u32; 2], drawable_px: [u32; 2]) -> Option<Self>;
      pub fn map_pointer(self, window_point: [f32; 2]) -> MappedPointer;
  }
  ```
- Exact fit: `k=min(drawable_w/16,drawable_h/9)` integer floor; content = `16k×9k`, centered. `k==0` → None.
- Pointer: window units→drawable pixels using per-axis ratio; test half-open content bounds; clamp to content; map to `[0,1920]×[0,1080]`.
- Click/button events require `inside_content`; motion always updates clamped logical cursor. Script coordinates bypass transform.
- Change `render/unsafe_sys.rs::present_blit` to blit source into `content_px`; existing swapchain `CLEAR` leaves bars cleared.
- Renderer derives content rect from actual swapchain pixel size. Do not change 1920×1080 offscreen texture, golden, shaders, `SpriteInstance`, atlas table.
- `src/rts_run.rs` maintains latest viewport from `Window::size()` + `size_in_pixels()`; updates on resize/pixel-size/display change events later. For now refresh before each live event batch/present.
- `edge_pan_dir` receives mapped logical point and logical size; clamped bar motion triggers at content edge. Bar clicks/orders do nothing.

## Inputs

- `render/renderer.rs`, `render/unsafe_sys.rs::present_blit`.
- `src/rts_run.rs` raw mouse routing.
- pinned SDL: `Window::{size,size_in_pixels}`; mouse event coords are window units.
- **From Depends:** T1 keeps logical RTS sprite/map scale; no direct API.

## TDD

1. **Red** — wide/tall/odd/HiDPI transform + bar tests; renderer dest rect test; unchanged golden.
2. **Green** — pure viewport module; destination blit; app mapping.
3. **Refactor** — one transform used by present/input; remove direct raw mouse→canvas route.

## Test plan

| Test | Input | Expect |
| --- | --- | --- |
| `exact_canvas_uses_full_drawable` | 1920×1080 | rect 0,0,1920,1080 |
| `ultrawide_pillarboxes` | 3440×1440 | centered exact-16:9 rect |
| `four_three_letterboxes` | 1280×1024 | centered exact-16:9 rect |
| `odd_drawable_keeps_exact_ratio` | 1366×768 | 1360×765 centered |
| `hidpi_pointer_inverse_round_trips` | 1280×720 units, 2560×1440 px | corners/center map exact |
| `bar_click_is_outside` | bar point | `inside_content=false` |
| `bar_motion_clamps_to_edge` | bar motion | logical x=0/1920 |
| existing GPU golden | fixed offscreen | exact unchanged |

## Impl steps

- [x] 1. Add `display_viewport.rs` red tests.
- [x] 2. Implement viewport structs/math; export from `render/mod.rs`.
- [x] 3. Change present destination region; assert bars clear in renderer test seam.
- [x] 4. Store/refresh viewport in RTS live path.
- [x] 5. Map all live mouse motion/button coordinates; reject bar button events.
- [x] 6. Keep scripted input untouched; add CLI regression proving canonical script coords.
- [x] 7. Run host golden comparison without regeneration.

## Outputs

- New: `render/viewport.rs`, `tests/display_viewport.rs`.
- Modified: render exports/renderer/unsafe sys/app/renderer tests.
- Public API: viewport types/signatures above.
- Behavior: aspect-fit present + exact inverse live input.
- Migrate/config: none.

## Validation

- [x] `cargo test -p mmd-engine --locked --test display_viewport`
- [x] `cargo test -p mmd-engine --locked --test render_correctness golden_frame_matches`
- [x] `cargo test -p millions_must_die --locked --test rts_cli_contract script_coordinates_remain_logical`
- [x] `cargo check --workspace --all-targets --all-features --locked`
- [ ] manual check: resize non-16:9 window → no stretch; bar click causes no world action — **unchecked in the review-fix pass: this needs a real window and nobody opened one.** The equivalent human steps live in `ai_artefacts/manual_test_checklist.md` § T8 (resize to 1280x1024 or an ultrawide, confirm no stretch and flat bars; click in a bar and confirm no world action).
- [ ] app functional: `cargo run -- rts --frames 60` — **unchecked for the same reason: without `SDL_VIDEODRIVER=offscreen` this opens a real window.** The offscreen equivalent is on the merge gate; the windowed one is `ai_artefacts/manual_test_checklist.md` § T8.
- [x] commit msg draft: `feat(render): preserve logical RTS canvas across window shapes`
