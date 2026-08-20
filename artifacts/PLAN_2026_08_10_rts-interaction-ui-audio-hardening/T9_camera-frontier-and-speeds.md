# T9: Add camera frontier and split speeds

**Plan:** `./artifacts/PLAN_2026_08_10_rts-interaction-ui-audio-hardening.md`  
**Depends:** T7, T8  
**Commit outcome:** camera center stays inside viewport-shrunk projected frontier; keyboard/edge pan use independent persisted 48 defaults; current frame depth uniforms follow camera.

## Context (self-contained)

- Goal: separate engine map, playable map, camera area; remove stale fixed-speed/raw-edge model.
- This slice: projected frontier, split input/speeds, minimap-ready look-at point, current scene uniforms.
- Out of scope here: minimap render/click, settings UI, window mode/focus.
- Assumptions: frontier uses projected map AABB shrink, not impossible full-rectangle erosion of isometric diamond; undersized projected axis collapses to midpoint; diagonal pan not normalized.
- Decision: `docs/ADR/018_ADR_settings_window_canvas_and_camera.md`.

## Requirements

- Replace `CAMERA_PAN_CELLS_PER_SEC` with settings-driven intent:
  ```rust
  #[derive(Clone, Copy, Debug, PartialEq)]
  pub struct CameraPanIntent {
      pub keyboard_dir: [f32; 2],
      pub edge_dir: [f32; 2],
      pub keyboard_speed: f32,
      pub edge_speed: f32,
  }
  ```
- Add `CameraFrontier` in `render/camera.rs` storing projected center bounds. For map tile `tw/th`:
  - projected map AABB x=`[-height*tw/2, width*tw/2]`, y=`[0,(width+height)*th/2]`;
  - inset x by logical view width/2; y by logical view height/2;
  - if min>max, collapse that axis to AABB midpoint.
- Clamp: project candidate center with origin `[0,0]`; clamp x/y; unproject to cell point.
- `Camera::look_at_point([f32;2])` accepts fractional map point + frontier clamp. Keep `look_at_cell` wrapper.
- `screen_axes_to_cells([sx,sy]) -> [sx+sy, sy-sx]`; velocity = keyboard basis×keyboard speed + edge basis×edge speed; no diagonal normalization.
- `RtsWorld` stores separate transient keyboard/edge dirs + two speeds. Add:
  ```rust
  pub fn set_keyboard_pan_dir(&mut self, dir: [f32; 2]);
  pub fn set_edge_pan_dir(&mut self, dir: [f32; 2]);
  pub fn set_camera_speeds(&mut self, keyboard: f32, edge: f32);
  pub fn look_at_map_point(&mut self, point: [f32; 2]);
  ```
- App loads T7 settings and applies speeds before first tick. Arrow held state affects keyboard only; pointer motion affects edge only.
- Fix stale depth uniforms: extend `ScenePass` with `frame_uniforms: Option<FrameUniforms>`; `RtsFrame::scene()` supplies current `world.iso_view().frame_uniforms()`. Legacy phase-0 wrappers pass None and retain existing uniform/golden behavior.
- Camera center remains state-hashed. Transient dirs/speeds remain un-hashed config/input.

## Inputs

- `render/camera.rs`, `render/instance.rs::{iso_project,iso_unproject,IsoView,FrameUniforms}`.
- `rts/world.rs` camera fields/tick.
- `rts/pack.rs::RtsFrame::scene`.
- `render/renderer.rs::ScenePass` uniform upload.
- **From Depends:** T7 exact speeds/defaults; T8 fixed logical 1920×1080 canvas + mapped edge coords.

## TDD

1. **Red** — projected bounds/current vertices/clamp/undersized/split-speed/depth-follow tests.
2. **Green** — frontier math + world intent + per-scene uniforms.
3. **Refactor** — remove old merged `pan_dir`; preserve phase-0 wrapper behavior exactly.

## Test plan

| Test | Input | Expect |
| --- | --- | --- |
| `frontier_shrinks_projected_map_by_view` | 320×320, tile 8×4, view 1920×1080 | exact projected x[-320,320], y[540,740] |
| `camera_cannot_cross_any_frontier_edge` | extreme pan/look | clamped projected center |
| `undersized_axis_collapses_to_midpoint` | small map | fixed axis center |
| `keyboard_and_edge_speeds_are_independent` | dirs separately | 48×dt each; additive together |
| `screen_axes_map_to_cell_axes` | four cardinals | exact expected cell deltas |
| `look_at_point_clamps_fractional_target` | outside minimap target | legal center |
| `scene_pass_uses_current_camera_uniforms` | pan between frames | new bias uploaded |
| phase-0 golden | no RTS frame uniforms | unchanged |

## Impl steps

- [x] 1. Add camera frontier/speed tests + renderer stale-uniform regression. (`crates/mmd-engine/tests/camera.rs`, `rts_pack.rs` new tests)
- [x] 2. Implement `CameraFrontier` projected math and look-at. (`render/camera.rs`)
- [x] 3. Add split intent API/world fields; remove merged pan dir. (`rts/world.rs`, `CameraPanIntent`)
- [x] 4. Apply T7 speeds in app; keep script/live input shared. (`src/rts_run.rs::run` calls `world.set_camera_speeds`)
- [x] 5. Add optional per-pass frame uniforms; wire RTS current view. (`ScenePass.frame_uniforms`, `RtsFrame::scene`, `pack_frame`)
- [x] 6. Update state hash tests for camera center only. (`state_hash_sees_the_camera` unchanged shape, still only hashes `camera.center()`)
- [x] 7. Run phase-0 render/golden regressions without regeneration. (`golden_frame_matches` passes unchanged with `MMD_REQUIRE_GPU=1`)

## Outputs

- Modified: camera/instance/renderer/pack/world/app + camera/render tests.
- Public API: intent/frontier/world setters above; `ScenePass.frame_uniforms`.
- Behavior: bounded camera + separate speeds + correct depth under pan.
- Migrate/config: consumes T7 fields; no schema change.

## Validation

- [x] `cargo test -p mmd-engine --locked --test camera` — 19 passed
- [x] `cargo test -p mmd-engine --locked --test render_correctness depth` — 2 passed (headless-safe subset; full GPU set green under `MMD_REQUIRE_GPU=1`)
- [x] `cargo test -p mmd-engine --locked --test rts_pack camera` — 7 passed (full file: 39 passed)
- [x] `cargo test -p mmd-engine --locked --test render_correctness golden_frame_matches` — 1 passed under `MMD_REQUIRE_GPU=1` (real Vulkan device, byte-identical golden)
- [x] `cargo check --workspace --all-targets --all-features --locked` — clean
- [x] manual check: hold arrows + edge-pan at all map edges → camera stops at frontier — proxied via `cargo run -- rts --frames 3000 --inject-input "1:pan:right"`: exit line `camera=206,126` projects to `[320, 664]`, exactly on the computed `x` frontier edge `[-320, 320]` and never crosses it
- [x] commit msg draft: `feat(rts): clamp split-speed camera to projected map frontier` — used verbatim as the commit subject
