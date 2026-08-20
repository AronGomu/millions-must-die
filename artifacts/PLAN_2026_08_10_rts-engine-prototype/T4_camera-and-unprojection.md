# T4: Panning camera + unprojection

**Plan:** `./artifacts/PLAN_2026_08_10_rts-engine-prototype.md`
**Depends:** none
**Commit outcome:** the isometric camera pans, clamps to the map, and a screen pixel resolves back to the exact cell it was drawn from.

## Context (self-contained)

- Goal: phase 1 is a thin vertical slice of an RTS engine prototype (camera,
  selection, workers, economy, building, unit production) on a new horde-free
  scene.
- This slice: phase 0 shipped a **fixed** camera — `IsoView::new` centres the
  scenario's destination cell and never moves. ADR 012 names "scrolling,
  edge-pan, zoom and selection" as phase-1 work. This ticket delivers the pan
  half and the inverse projection every mouse interaction depends on. **Zoom is
  deliberately not in this plan** (it multiplies through the depth
  normalisation, the AABB cull and every render golden); the tile size stays
  fixed at `2 * cell_size_px` × `cell_size_px`.
- Out of scope here: mouse plumbing, selection, entities, the RTS scenario, the
  renderer, the shader, any `sim::` file. Nothing outside the two files named
  below may change behaviour.
- Assumptions in force: the phase-0 fixed camera must remain reachable and
  bit-identical — `IsoView::new(w, h, dest, cell, view)` keeps its exact current
  meaning and value, so the phase-0 golden and every pinned instance test stay
  green.

## Requirements

- `iso_unproject` — the exact inverse of the existing `iso_project`.
- `IsoView::unproject(sx, sy) -> [f32; 2]` and
  `IsoView::with_center_cell(...)`, both keeping the depth scalars consistent
  with the new origin.
- A `Camera` type owning a cell-space centre, clamped panning, and edge-pan
  resolution from a mouse position.
- No behaviour change to `IsoView::new`, `iso_project`, `iso_origin`,
  `iso_depth` or `quad_is_visible`.

## Inputs

- **Files to read**
  - `crates/mmd-engine/src/render/instance.rs` — `iso_project`, `iso_origin`,
    `iso_depth`, `IsoView`, `ISO_TILE_W_PER_CELL`, `ISO_TILE_H_PER_CELL`,
    `ISO_DEPTH_EPSILON`.
  - `crates/mmd-engine/src/render/mod.rs` — the export list.
  - `crates/mmd-engine/src/sim/tick.rs` — `TICK_DT` (`1.0 / 60.0`).
- **From Depends:** none.
- **Facts you must not rediscover** — quoted from `render/instance.rs`:
  - Forward projection:
    ```rust
    pub fn iso_project(cx: f32, cy: f32, tile_w: f32, tile_h: f32, origin: [f32; 2]) -> [f32; 2] {
        [ origin[0] + (cx - cy) * tile_w * 0.5,
          origin[1] + (cx + cy) * tile_h * 0.5 ]
    }
    ```
  - `IsoView` fields: `tile_w`, `tile_h`, `origin`, `map_height_px`,
    `depth_scale`, `depth_bias`, `view_size`.
  - `IsoView::new` computes
    `tile_w = cell_size_px * 2.0`, `tile_h = cell_size_px * 1.0`,
    `origin = iso_origin(width, height, dest, tile_w, tile_h, view_size)`,
    `map_h = (width + height) * tile_h * 0.5`, and
    `(depth_scale, depth_bias) = if map_h > 0 && finite { (1.0/map_h, -origin[1]/map_h) } else { (0.0, 0.0) }`.
  - `iso_origin` centres the *destination cell's centre* (`dest.x + 0.5`,
    `dest.y + 0.5`) in the view.
  - The depth key is `ground_y * depth_scale + depth_bias`, i.e. the agent's
    position down the **map** diamond in `[0, 1]`. Because `depth_bias` carries
    `-origin.y`, moving the camera changes `origin` **and** `depth_bias`
    together, and the key stays camera-independent. Any new origin must
    recompute `depth_bias` or the horde will re-sort itself as the camera scrolls.

## Exact design — no decisions left

### Additions to `crates/mmd-engine/src/render/instance.rs`

```rust
/// Exact inverse of [`iso_project`]: a screen pixel back to cell space.
///
/// From `sx = ox + (cx - cy) * tw/2` and `sy = oy + (cx + cy) * th/2`:
///   `u = (sx - ox) / (tw/2) = cx - cy`
///   `v = (sy - oy) / (th/2) = cx + cy`
///   `cx = (u + v) / 2`,  `cy = (v - u) / 2`
///
/// A zero `tile_w` or `tile_h` has no inverse; it yields `[f32::NAN, f32::NAN]`
/// rather than an infinity, so a caller's bounds check rejects it instead of
/// indexing a cell at the far edge of the grid.
pub fn iso_unproject(sx: f32, sy: f32, tile_w: f32, tile_h: f32, origin: [f32; 2]) -> [f32; 2];

impl IsoView {
    /// [`iso_unproject`] through this view's tile and origin.
    pub fn unproject(&self, sx: f32, sy: f32) -> [f32; 2];

    /// The cell containing a screen pixel, or `None` when it falls outside the
    /// `width` x `height` grid this view was built for.
    ///
    /// `width`/`height` are parameters rather than stored state: `IsoView` is a
    /// projection, not a map, and giving it a second copy of the grid size is
    /// how the two would drift.
    pub fn cell_at(&self, sx: f32, sy: f32, width: u32, height: u32) -> Option<Cell>;

    /// This view re-derived around a new cell-space centre.
    ///
    /// `map_height_px`, `tile_w`, `tile_h` and `view_size` are unchanged;
    /// `origin` and `depth_bias` move together so the depth key stays the
    /// agent's position down the map diamond and does not shift under the
    /// camera.
    pub fn with_center_cell(&self, center: [f32; 2]) -> Self;
}
```

`with_center_cell` body, exactly:

```rust
let centred = iso_project(center[0], center[1], self.tile_w, self.tile_h, [0.0, 0.0]);
let origin = [self.view_size[0] * 0.5 - centred[0], self.view_size[1] * 0.5 - centred[1]];
let depth_bias = if self.map_height_px > 0.0 { -origin[1] / self.map_height_px } else { 0.0 };
Self { origin, depth_bias, ..*self }
```

`cell_at` body, exactly:

```rust
let c = self.unproject(sx, sy);
if !c[0].is_finite() || !c[1].is_finite() || c[0] < 0.0 || c[1] < 0.0 {
    return None;
}
let (x, y) = (c[0] as u32, c[1] as u32);
if x >= width || y >= height { None } else { Some(Cell { x, y }) }
```

### New module `crates/mmd-engine/src/render/camera.rs`

```rust
//! Clamped panning camera over the isometric projection.

use crate::render::instance::{IsoView, iso_project};
use crate::scenario::Cell;

/// Pan speed in cells per second, for both keyboard and edge pan.
///
/// 24 cells/s is three times the agent walk speed
/// (`sim::SPEED_CELLS_PER_SEC = 8.0`), so a player can outrun the unit they
/// just ordered — the property that makes a camera feel responsive rather than
/// the number itself.
pub const CAMERA_PAN_CELLS_PER_SEC: f32 = 24.0;

/// Distance from a view border, in screen pixels, inside which the pointer pans.
pub const EDGE_PAN_MARGIN_PX: f32 = 12.0;

/// Cell-space camera centre plus the projection it produces.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    /// The cell-space point the view centre looks at.
    center: [f32; 2],
    /// Grid width in cells — the clamp bound.
    width: u32,
    /// Grid height in cells — the clamp bound.
    height: u32,
    /// Projection with everything but `origin`/`depth_bias` fixed for the scene.
    view: IsoView,
}

impl Camera {
    /// Build from a scene's geometry, starting centred on `start`.
    ///
    /// `start` is clamped like every later pan, so a scenario naming a centre
    /// off its own grid cannot open the game looking at nothing.
    pub fn new(width: u32, height: u32, cell_size_px: f32, view_size: [f32; 2], start: [f32; 2]) -> Self;

    /// The current projection. Packers and the renderer's depth uniform read this.
    pub fn iso_view(&self) -> IsoView;

    /// The cell-space centre.
    pub fn center(&self) -> [f32; 2];

    /// Move the centre by a cell-space delta and clamp.
    ///
    /// Clamp is `0.0 ..= width as f32` and `0.0 ..= height as f32` — the closed
    /// cell-space rectangle of the grid, so the centre may sit exactly on the
    /// far edge and never outside it. A non-finite delta is ignored outright
    /// rather than poisoning the centre.
    pub fn pan_cells(&mut self, dx: f32, dy: f32);

    /// Apply one tick of panning from a direction vector.
    ///
    /// `dir` components are expected in `-1.0..=1.0`; the step is
    /// `dir * CAMERA_PAN_CELLS_PER_SEC * dt`. A diagonal is **not** normalised:
    /// holding two keys pans faster on the diagonal, which is what every RTS
    /// this one is modelled on does.
    pub fn pan_tick(&mut self, dir: [f32; 2], dt: f32);

    /// Centre the camera on a cell (clamped). Used by "jump to base".
    pub fn look_at_cell(&mut self, cell: Cell);
}

/// Edge-pan direction for a pointer at `mouse` inside a `view` rect.
///
/// Returns one of `-1.0`, `0.0`, `1.0` per axis. `+x` means the world scrolls
/// so the camera centre moves toward larger `cell.x - cell.y` (screen right);
/// `+y` toward the bottom of the screen. A pointer outside the view produces
/// `[0.0, 0.0]` — a window that lost focus must not scroll forever.
pub fn edge_pan_dir(mouse: [f32; 2], view: [f32; 2]) -> [f32; 2];
```

`edge_pan_dir` body, exactly:

```rust
if !mouse[0].is_finite() || !mouse[1].is_finite()
    || mouse[0] < 0.0 || mouse[1] < 0.0
    || mouse[0] > view[0] || mouse[1] > view[1] {
    return [0.0, 0.0];
}
let x = if mouse[0] <= EDGE_PAN_MARGIN_PX { -1.0 }
        else if mouse[0] >= view[0] - EDGE_PAN_MARGIN_PX { 1.0 } else { 0.0 };
let y = if mouse[1] <= EDGE_PAN_MARGIN_PX { -1.0 }
        else if mouse[1] >= view[1] - EDGE_PAN_MARGIN_PX { 1.0 } else { 0.0 };
[x, y]
```

**Screen direction vs cell direction.** `edge_pan_dir` returns a *screen*
direction. `Camera::pan_tick` takes a *cell-space* delta. The conversion is the
inverse projection of a direction (no origin term):

```rust
/// A screen-space pan direction converted to the cell-space delta that moves
/// the view that way. Pure inverse of the projection's linear part.
pub fn screen_dir_to_cells(dir: [f32; 2], tile_w: f32, tile_h: f32) -> [f32; 2] {
    let u = if tile_w != 0.0 { dir[0] / (tile_w * 0.5) } else { 0.0 };
    let v = if tile_h != 0.0 { dir[1] / (tile_h * 0.5) } else { 0.0 };
    [(u + v) * 0.5, (v - u) * 0.5]
}
```

Callers do `camera.pan_tick(screen_dir_to_cells(edge_pan_dir(m, view), tw, th), dt)`.
Keyboard pan uses the same path with a hand-built screen direction, so keyboard
and edge pan cannot disagree about which way is "right".

### Exports

`crates/mmd-engine/src/render/mod.rs`: add `mod camera;` and
`pub use camera::{CAMERA_PAN_CELLS_PER_SEC, Camera, EDGE_PAN_MARGIN_PX, edge_pan_dir, screen_dir_to_cells};`
plus `iso_unproject` in the existing `instance` re-export list.

## TDD

1. **Red** — write every test below in a new
   `crates/mmd-engine/tests/camera.rs`. Watch them fail.
2. **Green** — implement.
3. **Refactor** — none expected. Keep green.

## Test plan

| Test | Input | Expect |
| ---- | ----- | ------ |
| `unproject_inverts_project_exactly` | 400 cell points on a 320×320 grid at `cell_size_px = 4`, projected then unprojected | each component within `1e-3` of the original |
| `project_inverts_unproject_exactly` | 400 screen points inside `1920×1080` | round-trip within `1e-3` |
| `unproject_of_a_degenerate_tile_is_not_finite` | `iso_unproject(10.0, 10.0, 0.0, 0.0, [0.0, 0.0])` | both components `is_nan()` |
| `cell_at_returns_the_cell_a_sprite_was_packed_from` | for cells `(0,0)`, `(1,0)`, `(0,1)`, `(159,160)`, `(319,319)`: project the cell **centre**, then `cell_at` | returns exactly that cell |
| `cell_at_rejects_offscreen_and_off_grid` | screen points far above the diamond, and negative coords | `None` |
| `iso_view_new_is_unchanged` | `IsoView::new(480, 270, Cell{x:240,y:135}, 4.0, [1920.0,1080.0])` | all seven fields equal the values recorded before this ticket (hardcode them in the test) |
| `with_center_cell_matches_new_for_the_destination_centre` | `IsoView::new(w,h,dest,c,v)` vs `IsoView::new(...).with_center_cell([dest.x as f32 + 0.5, dest.y as f32 + 0.5])` | equal `origin` and `depth_bias` (within `1e-6`) |
| `depth_key_is_camera_independent` | one cell's ground point, projected and depth-keyed under three different camera centres | the three depth values are equal within `1e-6` |
| `panning_moves_the_origin_the_other_way` | pan `+2` cells in `x` | `origin[0]` decreases by `2.0 * tile_w * 0.5` |
| `pan_is_clamped_to_the_grid` | pan `-10_000` then `+10_000` cells on a 320×320 grid | centre lands exactly `[0.0, 0.0]` then `[320.0, 320.0]` |
| `start_center_is_clamped` | `Camera::new(64, 64, 4.0, view, [999.0, -999.0])` | `center() == [64.0, 0.0]` |
| `a_non_finite_pan_is_ignored` | `pan_cells(f32::NAN, 1.0)` | centre unchanged |
| `pan_tick_uses_the_documented_speed` | `pan_tick([1.0, 0.0], 1.0 / 60.0)` | `center()[0]` advanced by `24.0 / 60.0` |
| `diagonal_pan_is_not_normalised` | `pan_tick([1.0, 1.0], dt)` vs `pan_tick([1.0, 0.0], dt)` | the diagonal moves each axis by the same amount as the cardinal did on its axis |
| `edge_pan_fires_only_inside_the_margin` | `x = 0.0`, `11.9`, `12.0`, `12.1`, `1907.9`, `1908.0`, `1920.0` at `y = 540.0` | `-1, -1, -1, 0, 0, 1, 1` |
| `edge_pan_outside_the_view_is_zero` | `[-5.0, 540.0]`, `[1925.0, 540.0]`, `[960.0, -1.0]`, `[f32::NAN, 0.0]` | `[0.0, 0.0]` for all four |
| `edge_pan_corner_pans_both_axes` | `[2.0, 2.0]` | `[-1.0, -1.0]` |
| `screen_right_moves_the_camera_screen_right` | `screen_dir_to_cells([1.0, 0.0], 8.0, 4.0)`, applied to a camera, then project the centre | the projected centre's `x` increased |
| `screen_down_moves_the_camera_screen_down` | `screen_dir_to_cells([0.0, 1.0], 8.0, 4.0)` likewise | the projected centre's `y` increased |
| `look_at_cell_centres_that_cell` | `look_at_cell(Cell{x:10,y:200})` then `iso_view().project(10.5, 200.5)` | equals `[960.0, 540.0]` within `1e-3` |

**Mutation verification (mandatory).** Inject, confirm red, revert, confirm green:
1. `iso_unproject` returns `[(u - v) * 0.5, (v + u) * 0.5]` → kills the two round-trip tests and `cell_at_returns_the_cell_a_sprite_was_packed_from`.
2. `with_center_cell` keeps the old `depth_bias` → kills `depth_key_is_camera_independent`.
3. Clamp uses `width - 1` instead of `width` → kills `pan_is_clamped_to_the_grid`.
4. `edge_pan_dir` uses `<` instead of `<=` at the margin → kills `edge_pan_fires_only_inside_the_margin` (the `12.0` case).
5. Drop the outside-the-view guard → kills `edge_pan_outside_the_view_is_zero`.
6. `pan_tick` normalises the diagonal → kills `diagonal_pan_is_not_normalised`.
7. `screen_dir_to_cells` swaps its two outputs → kills the two direction tests.

## Impl steps

- [x] 1. Add `iso_unproject` to `crates/mmd-engine/src/render/instance.rs`.
- [x] 2. Add `IsoView::unproject`, `IsoView::cell_at`, `IsoView::with_center_cell` to the same file, bodies exactly as quoted.
- [x] 3. Create `crates/mmd-engine/src/render/camera.rs` with the constants and `Camera`.
- [x] 4. Implement `edge_pan_dir` and `screen_dir_to_cells` with the bodies quoted.
- [x] 5. Add `mod camera;` and the two `pub use` lines to `crates/mmd-engine/src/render/mod.rs`.
- [x] 6. Create `crates/mmd-engine/tests/camera.rs` and write every test from the table. Watch them fail.
- [x] 7. Implement `Camera::new` / `iso_view` / `center` / `pan_cells` / `pan_tick` / `look_at_cell` until green.
- [x] 8. Record the seven pre-existing `IsoView::new` field values in `iso_view_new_is_unchanged` by reading them from `git stash`-clean `main` first, so the assertion is a pin and not a restatement.
- [x] 9. Run the mutation list; record kills in the commit body.
- [x] 10. Run the full validation block.

## Outputs

- **Files created**
  - `crates/mmd-engine/src/render/camera.rs`
  - `crates/mmd-engine/tests/camera.rs`
- **Files edited**
  - `crates/mmd-engine/src/render/instance.rs`
  - `crates/mmd-engine/src/render/mod.rs`
- **Public API added:** `render::iso_unproject`, `IsoView::{unproject, cell_at, with_center_cell}`, `render::{Camera, CAMERA_PAN_CELLS_PER_SEC, EDGE_PAN_MARGIN_PX, edge_pan_dir, screen_dir_to_cells}`.
- **Behaviour change:** none for any existing command. `Runtime` still builds a fixed `IsoView` and nothing calls `Camera` yet.
- **Migration / config:** none.

## Validation

- [x] `cargo fmt --all -- --check`
- [x] `cargo test -p mmd-engine --test camera` — all green
- [x] `MMD_REQUIRE_GPU=1 cargo test --workspace --locked`
- [x] `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- [x] `nix flake check`
- [x] `git diff --stat HEAD -- lab/goldens/` — **empty**
- [x] `cargo run -- run --agents 5000 --frames 300` — exit 0, `hash=` on the exit line unchanged from before this ticket
- [x] app functional — no broken path from this slice
- [x] commit msg draft: `feat(render): add a clamped panning camera and screen-to-cell unprojection`
