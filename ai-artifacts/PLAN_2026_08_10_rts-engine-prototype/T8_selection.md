# T8: Selection

**Plan:** `./ai-artifacts/PLAN_2026_08_10_rts-engine-prototype.md`
**Depends:** T4, T6
**Commit outcome:** a click picks the unit under the cursor and a drag rectangle picks every own unit inside it; the selection is queryable and order-stable.

## Context (self-contained)

- Goal: phase 1 is a thin vertical slice of an RTS engine prototype (camera,
  selection, workers, economy, building, unit production) on a horde-free scene.
- This slice: turning a screen-space pointer gesture into a set of entity
  handles. It is pure logic — no SDL, no rendering, no orders.
- Out of scope here: drawing the selection ring or the drag rectangle (T12),
  the HUD panel (T13), mouse plumbing (T14), issuing orders to the selection
  (T7 already provides `order_move_group`; wiring the two is T14).
- Assumptions in force: `docs/DESIGN.md` records **"Unlimited unit selection"**
  as a design decision. The cap here is therefore the entity store's own ceiling,
  not a smaller RTS-traditional 12 or 24.

## Requirements

- A `Selection` set with stable ascending-slot iteration order.
- Click picking with a documented priority: own unit → own building → neutral
  node → nothing.
- Additive (shift) click that toggles.
- Screen-rectangle box selection that picks **player-owned units only**.
- Selection self-heals: an entity that dies leaves the selection.

## Inputs

- **Files to read**
  - `crates/mmd-engine/src/rts/entity.rs`, `world.rs` (T6).
  - `crates/mmd-engine/src/render/instance.rs` — `IsoView`, `project`,
    `unproject`, `cell_at`.
  - `crates/mmd-engine/src/render/camera.rs` — `Camera`.
  - `crates/mmd-engine/src/scenario.rs` — `Cell`, footprint constants.
- **From Depends (T6) — spell out, the worker cannot read T6:**
  - `mmd_engine::rts` exports `EntityStore`, `EntityId { index: u32, generation: u32 }`,
    `EntityKind::{Unit(UnitKind), Building(BuildingKind), Node(ResourceKind)}`,
    `MAX_ENTITIES = 2_048`, `OWNER_PLAYER = 0`, `OWNER_NEUTRAL = 255`.
  - `EntityStore`: `slot(id) -> Option<usize>`, `id_at(slot) -> Option<EntityId>`,
    `contains(id)`, `alive(slot)`, `kind(slot)`, `owner(slot)`,
    `position(slot) -> [f32; 2]` (cell space), `collect_live(&self, out: &mut Vec<usize>)`.
  - `BuildingKind::footprint_cells()` gives `12` (Hq), `8` (Depot), `10` (Barracks).
    A building's `position` is its footprint **centre** in cell space.
  - `RtsWorld` has `scenario()`, `entities()`, `entities_mut()`, `tick()`,
    `state_hash()`, `start_hq()`; `testkit::RtsHarness::scene()` builds one over
    the tracked 320 × 320 scene (HQ min corner `(160, 160)`, six workers at
    `(162..=167, 178)`, 8 crystal + 2 gas nodes).
- **From Depends (T4) — spell out, the worker cannot read T4:**
  - `IsoView` fields `tile_w`, `tile_h`, `origin`, `map_height_px`,
    `depth_scale`, `depth_bias`, `view_size`.
  - `IsoView::project(cx, cy) -> [f32; 2]` maps cell space to screen pixels:
    `sx = origin.x + (cx - cy) * tile_w / 2`, `sy = origin.y + (cx + cy) * tile_h / 2`.
  - `IsoView::unproject(sx, sy) -> [f32; 2]` is its exact inverse.
  - `IsoView::cell_at(sx, sy, width, height) -> Option<Cell>`.
  - `IsoView::with_center_cell(center) -> Self` re-derives `origin` **and**
    `depth_bias` together.
  - `render::Camera::{new, iso_view, center, pan_cells, pan_tick, look_at_cell}`,
    `render::{edge_pan_dir, screen_dir_to_cells, CAMERA_PAN_CELLS_PER_SEC, EDGE_PAN_MARGIN_PX}`.
- **Fact you must not rediscover:** the tracked scene's `collision_radius_q8` is
  `1536`, i.e. a body radius of exactly `6.0` cells
  (`1536 / 256`), available as `Scenario::collision_radius_cells()`.

## Exact design — no decisions left

### `crates/mmd-engine/src/rts/selection.rs`

```rust
/// Selection ceiling.
///
/// `docs/DESIGN.md` records "Unlimited unit selection" as a design decision, so
/// the only honest ceiling is the entity store's own. A smaller,
/// RTS-traditional 12 would be a gameplay rule this prototype has not chosen.
pub const MAX_SELECTION: usize = MAX_ENTITIES;

/// Pick radius for a unit, in cells, as a multiple of its body radius.
///
/// `1.0` — the pointer must land inside the circle the separation pass
/// separates on. Any other value would make "what you clicked" and "what
/// collides" two different shapes, and the hitbox overlay (`H`) would stop
/// being an explanation of the click.
pub const UNIT_PICK_RADIUS_SCALE: f32 = 1.0;

/// An ordered set of entity handles.
///
/// Iteration is by **ascending entity slot**, not by click order: an order
/// issued to a selection must be reproducible, and click order is not part of
/// the world's state.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Selection { /* private: Vec<EntityId>, kept sorted by (index, generation) */ }

impl Selection {
    pub fn new() -> Self;
    pub fn len(&self) -> usize;
    pub fn is_empty(&self) -> bool;
    pub fn clear(&mut self);
    pub fn contains(&self, id: EntityId) -> bool;
    /// Ascending by slot index.
    pub fn ids(&self) -> &[EntityId];
    /// Insert, keeping sort order. `false` when already present or at the cap.
    pub fn insert(&mut self, id: EntityId) -> bool;
    /// Remove. `false` when absent.
    pub fn remove(&mut self, id: EntityId) -> bool;
    /// Insert if absent, remove if present. Returns the new membership.
    pub fn toggle(&mut self, id: EntityId) -> bool;
    /// Replace the whole set with `ids`, sorted and deduplicated.
    pub fn replace(&mut self, ids: &[EntityId]);
    /// Drop every handle the store no longer resolves. Returns how many went.
    pub fn retain_live(&mut self, store: &EntityStore) -> usize;
    /// The first id, in slot order — the "primary" selection the HUD describes.
    pub fn primary(&self) -> Option<EntityId>;
    pub fn hash_into(&self, h: &mut sha2::Sha256);
}
```

### Picking — free functions in the same module

```rust
/// What a click landed on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pick {
    Unit(EntityId),
    Building(EntityId),
    Node(EntityId),
    Nothing,
}

/// Resolve a screen-space click against the world.
///
/// Priority, in order, and the first hit wins:
/// 1. **Own units** — the player-owned unit whose centre is nearest the click's
///    cell-space point, within `body_radius * UNIT_PICK_RADIUS_SCALE` cells.
///    Nearest, not first-found, so overlapping bodies pick the one under the
///    cursor rather than the one with the lowest slot.
/// 2. **Own buildings** — the building whose footprint rectangle contains the
///    clicked cell. Footprints cannot overlap (T10 rejects that), so at most one.
/// 3. **Resource nodes** — the node occupying the clicked cell exactly.
/// 4. Nothing.
///
/// Units come first because a worker standing on its own base must be
/// clickable; the building is the larger target and would always win otherwise.
pub fn pick_at(
    world: &RtsWorld,
    view: &IsoView,
    screen: [f32; 2],
) -> Pick;

/// Every player-owned **unit** whose ground point lies inside a screen-space
/// rectangle, in ascending slot order, appended to `out` (cleared first).
///
/// Units only. A box that selected buildings would make "select all and move"
/// silently mean something different, and no RTS this one is modelled on does
/// it. Neutral nodes are never boxed.
///
/// The rectangle is normalised, so a drag in any direction works; a degenerate
/// rectangle (zero width or height) selects nothing.
pub fn box_select(
    world: &RtsWorld,
    view: &IsoView,
    a: [f32; 2],
    b: [f32; 2],
    out: &mut Vec<EntityId>,
);

/// Screen rectangle normalised to `(min, max)`.
pub fn normalise_rect(a: [f32; 2], b: [f32; 2]) -> ([f32; 2], [f32; 2]);

/// Whether a drag is long enough to be a box rather than a click.
///
/// Below this, a press-and-release is a click even if the pointer twitched.
pub const DRAG_MIN_PX: f32 = 4.0;

/// Classify a press/release pair.
pub fn is_drag(a: [f32; 2], b: [f32; 2]) -> bool {
    (b[0] - a[0]).abs() >= DRAG_MIN_PX || (b[1] - a[1]).abs() >= DRAG_MIN_PX
}
```

`pick_at` body, exactly:

```rust
let p = view.unproject(screen[0], screen[1]);
if !p[0].is_finite() || !p[1].is_finite() { return Pick::Nothing; }
let r = world.scenario().collision_radius_cells() * UNIT_PICK_RADIUS_SCALE;
let r2 = r * r;
// 1. nearest own unit inside r
let mut best: Option<(f32, EntityId)> = None;
for &slot in live_slots {
    if !matches!(world.entities().kind(slot), EntityKind::Unit(_)) { continue; }
    if world.entities().owner(slot) != OWNER_PLAYER { continue; }
    let q = world.entities().position(slot);
    let d2 = (q[0] - p[0]).powi(2) + (q[1] - p[1]).powi(2);
    if d2 <= r2 && best.is_none_or(|(bd, _)| d2 < bd) {
        best = Some((d2, world.entities().id_at(slot).expect("live")));
    }
}
if let Some((_, id)) = best { return Pick::Unit(id); }
// 2. own building footprint
let Some(cell) = view.cell_at(screen[0], screen[1], w, h) else { return Pick::Nothing };
for &slot in live_slots {
    let EntityKind::Building(b) = world.entities().kind(slot) else { continue };
    if world.entities().owner(slot) != OWNER_PLAYER { continue; }
    if footprint_contains(world.entities().position(slot), b.footprint_cells(), cell) {
        return Pick::Building(world.entities().id_at(slot).expect("live"));
    }
}
// 3. node on that exact cell
for &slot in live_slots {
    let EntityKind::Node(_) = world.entities().kind(slot) else { continue };
    let q = world.entities().position(slot);
    if q[0].floor() as u32 == cell.x && q[1].floor() as u32 == cell.y {
        return Pick::Node(world.entities().id_at(slot).expect("live"));
    }
}
Pick::Nothing
```

with

```rust
/// Whether `cell` lies in the footprint of edge `edge` centred at `center`.
///
/// The footprint's minimum corner is `center - edge/2`, matching how
/// `RtsWorld` seeds the HQ (`hq_cell` is the min corner, position is the centre).
pub fn footprint_contains(center: [f32; 2], edge: u32, cell: Cell) -> bool {
    let half = edge as f32 * 0.5;
    let x0 = (center[0] - half).round() as i64;
    let y0 = (center[1] - half).round() as i64;
    let cx = cell.x as i64;
    let cy = cell.y as i64;
    cx >= x0 && cx < x0 + edge as i64 && cy >= y0 && cy < y0 + edge as i64
}

/// The footprint's minimum corner, as cell coordinates.
pub fn footprint_min(center: [f32; 2], edge: u32) -> Cell;
```

### `RtsWorld` additions

```rust
impl RtsWorld {
    pub fn selection(&self) -> &Selection;
    pub fn selection_mut(&mut self) -> &mut Selection;

    /// Apply a plain click: replace the selection with what was picked, or
    /// clear it when nothing was.
    pub fn click_select(&mut self, view: &IsoView, screen: [f32; 2]) -> Pick;

    /// Apply an additive (shift) click: toggle what was picked. A click on
    /// nothing leaves the selection alone — a modifier click is a refinement,
    /// and clearing on a near-miss is the single most annoying selection bug in
    /// the genre.
    pub fn shift_click_select(&mut self, view: &IsoView, screen: [f32; 2]) -> Pick;

    /// Apply a drag rectangle: replace the selection with every own unit inside.
    /// An empty box clears the selection.
    pub fn box_select_into_selection(&mut self, view: &IsoView, a: [f32; 2], b: [f32; 2]) -> usize;
}
```

`state_hash` gains `selection.hash_into(h)` after the order table.
`tick()` calls `self.selection.retain_live(&self.entities)` as the **last**
step, after the supply recount, so a unit that died this tick is out of the
selection before anything reads it next tick.

`RtsWorld` gains a private `pick_scratch: Vec<EntityId>` and reuses
`live_scratch` (both capacity `MAX_ENTITIES`) so no selection operation allocates.

### Exports

`crates/mmd-engine/src/rts/mod.rs`: `mod selection;` plus
`pub use selection::{DRAG_MIN_PX, MAX_SELECTION, Pick, Selection, UNIT_PICK_RADIUS_SCALE, box_select, footprint_contains, footprint_min, is_drag, normalise_rect, pick_at};`

## TDD

1. **Red** — write every test below in a new
   `crates/mmd-engine/tests/rts_selection.rs`. Watch them fail.
2. **Green** — implement.
3. **Refactor** — none expected. Keep green.

## Test plan

Unless stated otherwise, the harness is `RtsHarness::scene()` and the view is
`Camera::new(320, 320, 4.0, [1920.0, 1080.0], [166.0, 172.0]).iso_view()`.

| Test | Input | Expect |
| ---- | ----- | ------ |
| `selection_iterates_in_slot_order` | insert workers in reverse slot order | `ids()` ascending by `index` |
| `insert_is_idempotent` | insert the same id twice | `len() == 1`, second returns `false` |
| `toggle_round_trips` | toggle twice | back to absent, `len() == 0` |
| `replace_deduplicates` | `replace(&[a, a, b])` | `len() == 2` |
| `retain_live_drops_a_despawned_id` | select 3, despawn one | `retain_live` returns `1`, `len() == 2` |
| `retain_live_drops_a_stale_generation` | select a, despawn a, spawn b into the same slot | the selection is empty, and does **not** contain `b` |
| `primary_is_the_lowest_slot` | select workers 13, 11, 15 | `primary()` resolves to slot 11 |
| `normalise_rect_handles_every_drag_direction` | four corner orders of the same rect | identical `(min, max)` |
| `is_drag_needs_four_pixels` | deltas `3.9`, `4.0` on each axis | `false`, `true` |
| `clicking_a_worker_selects_it` | project worker slot 11's position, click it | `Pick::Unit(id_of(11))`, selection is exactly that id |
| `clicking_between_two_workers_picks_the_nearer` | place two workers 2 cells apart, click 0.4 cells from one | the nearer id |
| `the_pick_radius_is_the_body_radius` | click at exactly `6.0` cells and at `6.1` cells from a worker's centre, along +x in cell space | hit, then miss |
| `clicking_the_hq_selects_it_when_no_unit_is_near` | project cell `(166, 166)` | `Pick::Building(hq)` |
| `a_worker_standing_on_the_hq_wins_the_click` | move worker slot 11 to `[166.0, 166.0]`, click there | `Pick::Unit`, not `Pick::Building` |
| `clicking_a_node_selects_it` | project crystal node `(140, 150)`'s cell centre | `Pick::Node(..)` |
| `clicking_a_node_one_cell_off_misses_it` | project `(141, 150)` | `Pick::Nothing` |
| `clicking_empty_ground_clears_the_selection` | select 3, click far empty ground | `Pick::Nothing`, `selection().is_empty()` |
| `shift_click_adds` | select worker A, shift-click worker B | both in selection |
| `shift_click_on_a_selected_unit_removes_it` | select A and B, shift-click A | only B |
| `shift_click_on_nothing_keeps_the_selection` | select 3, shift-click empty ground | still 3 |
| `box_selects_every_own_unit_inside` | box the six spawn cells' screen extent | 6 ids, ascending slots 11..=16 |
| `box_excludes_units_outside` | box only the first three workers' extent | exactly those 3 |
| `box_never_selects_buildings` | box the whole HQ footprint's screen extent with no unit inside | `0`, selection empty |
| `box_never_selects_nodes` | box a crystal node's screen extent | `0` |
| `box_ignores_a_degenerate_rectangle` | `a == b` | `0`, and the pre-existing selection is cleared (documented behaviour) |
| `box_works_dragged_in_any_direction` | the six-worker box, dragged from each of the 4 corners | 6 every time, same ids |
| `box_respects_the_camera` | pan the camera 40 cells right, redo the six-worker box using the **old** screen rect | `0` — the box is screen space, so the units moved out of it |
| `footprint_contains_matches_the_seeded_hq` | HQ centre `[166.0, 166.0]`, edge 12 | true for `(160,160)` and `(171,171)`, false for `(159,160)` and `(172,171)` |
| `footprint_min_recovers_the_scenario_corner` | same | `Cell { x: 160, y: 160 }` |
| `selection_survives_a_tick` | select 3, `step_exact(60)` | still 3 |
| `state_hash_sees_the_selection` | hash before vs after a click that selects | different |
| `selection_operations_allocate_nothing` | in `frame_allocations.rs`, `MeasureGuard` around 100 box-selects of the full scene | zero allocations |

**Mutation verification (mandatory).** Inject, confirm red, revert, confirm green:
1. `pick_at` returns the first unit in radius instead of the nearest → kills `clicking_between_two_workers_picks_the_nearer`.
2. Buildings checked before units → kills `a_worker_standing_on_the_hq_wins_the_click`.
3. `UNIT_PICK_RADIUS_SCALE = 2.0` → kills `the_pick_radius_is_the_body_radius`.
4. `box_select` includes buildings → kills `box_never_selects_buildings`.
5. `shift_click_select` clears on `Pick::Nothing` → kills `shift_click_on_nothing_keeps_the_selection`.
6. `retain_live` checks `store.alive(slot)` instead of `store.contains(id)` → kills `retain_live_drops_a_stale_generation`.
7. `footprint_contains` uses `<=` on the far edge → kills `footprint_contains_matches_the_seeded_hq`.
8. `Selection::insert` appends without sorting → kills `selection_iterates_in_slot_order` and `primary_is_the_lowest_slot`.

## Impl steps

- [x] 1. Create `crates/mmd-engine/src/rts/selection.rs` with the constants, `Selection`, `Pick`.
- [x] 2. Add `mod selection;` and the `pub use` line to `crates/mmd-engine/src/rts/mod.rs`.
- [x] 3. Create `crates/mmd-engine/tests/rts_selection.rs` and write every test from the table. Watch them fail.
- [x] 4. Implement `Selection` (sorted insert by `(index, generation)`, `remove`, `toggle`, `replace`, `retain_live`, `primary`, `hash_into`).
- [x] 5. Implement `normalise_rect`, `is_drag`, `footprint_contains`, `footprint_min`.
- [x] 6. Implement `pick_at` with the body quoted above.
- [x] 7. Implement `box_select` (project each live own-unit position, test containment in the normalised rect, append `EntityId`).
- [x] 8. Add `selection`, `pick_scratch` fields to `RtsWorld`; reserve both at `MAX_ENTITIES`.
- [x] 9. Add `selection`, `selection_mut`, `click_select`, `shift_click_select`, `box_select_into_selection` to `RtsWorld`.
- [x] 10. Add `self.selection.retain_live(&self.entities)` as the last step of `RtsWorld::tick`.
- [x] 11. Extend `RtsWorld::state_hash` with the selection.
- [x] 12. Add `selection_operations_allocate_nothing` to `crates/mmd-engine/tests/frame_allocations.rs`.
- [x] 13. Run the mutation list; record kills in the commit body.
- [x] 14. Run the full validation block.

## Outputs

- **Files created**
  - `crates/mmd-engine/src/rts/selection.rs`
  - `crates/mmd-engine/tests/rts_selection.rs`
- **Files edited**
  - `crates/mmd-engine/src/rts/{mod,world}.rs`
  - `crates/mmd-engine/tests/frame_allocations.rs`
- **Public API added:** `rts::{Selection, Pick, MAX_SELECTION, UNIT_PICK_RADIUS_SCALE, DRAG_MIN_PX, pick_at, box_select, normalise_rect, is_drag, footprint_contains, footprint_min}`, `RtsWorld::{selection, selection_mut, click_select, shift_click_select, box_select_into_selection}`.
- **Behaviour change:** none for any existing command; nothing drives selection yet.
- **Migration / config:** none.

## Validation

- [x] `cargo fmt --all -- --check`
- [x] `cargo test -p mmd-engine --test rts_selection` — all green
- [x] `cargo test -p mmd-engine --test frame_allocations` — all green
- [x] `MMD_REQUIRE_GPU=1 cargo test --workspace --locked`
- [x] `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- [x] `nix flake check`
- [x] `cargo run -- run --agents 5000 --frames 300` — exit 0, exit-line `hash=` unchanged
- [x] app functional — no broken path from this slice
- [x] commit msg draft: `feat(rts): select units by click and drag rectangle`
