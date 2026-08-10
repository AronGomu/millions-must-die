# T12: World render packing

**Plan:** `./ai-artifacts/PLAN_2026_08_10_rts-engine-prototype.md`
**Depends:** T2, T4, T8, T11
**Commit outcome:** the RTS world packs into draw groups and an overlay — units, buildings, sites, nodes, selection rings, the placement ghost and the drag box — and the camera lives in the world.

## Context (self-contained)

- Goal: phase 1 is a thin vertical slice of an RTS engine prototype (camera,
  selection, workers, economy, building, unit production) on a horde-free scene.
- This slice: turning world state into instances. Pure CPU, no device — the same
  split phase 0 uses (`runtime::pack_instance_groups` packs; the renderer draws).
  It also gives `RtsWorld` its camera, so the projection everything packs
  through is one value owned in one place.
- Out of scope here: the HUD text and panels (T13), SDL input (T14), the
  renderer itself (T2 already provides the scene pass).
- Assumptions in force: no per-frame allocation. Every buffer is reserved once
  and cleared per frame, exactly like `Runtime`'s `groups` and `ring_instances`.

## Requirements

- `RtsWorld` owns a `Camera`; the projection is derived from it once per frame.
- A reusable `RtsFrame` holding world groups, a world-space overlay and a UI
  group list.
- Named UV accessors for the static building / node / prop tables, so no packer
  hardcodes a `(row, col)`.
- Off-screen instances culled on the same rect test the horde packer uses.

## Inputs

- **Files to read**
  - `crates/mmd-engine/src/runtime.rs` — `pack_instance_groups`,
    `pack_ring_instances`, `RING_INNER`, `RING_OUTER`, `ring_quad_size_px`.
    That is the pattern to mirror; do **not** edit that file.
  - `crates/mmd-engine/src/render/{instance,atlas,renderer,camera}.rs`.
  - `crates/mmd-engine/src/rts/*`.
- **From Depends (T2) — spell out:**
  - Texture slots: `ATLAS_SLOT_COUNT = 9`; `SLOT_RTS_WORKER = 4`,
    `SLOT_RTS_SOLDIER = 5`, `SLOT_RTS_BUILDINGS = 6`, `SLOT_RTS_PROPS = 7`,
    `SLOT_UI_FONT = 8`. `DrawGroup { atlas_id: u32, instances: Vec<SpriteInstance> }`
    where `atlas_id` is a slot.
  - ```rust
    pub struct ScenePass<'a> {
        pub world: &'a [DrawGroup],      // depth-tested
        pub overlay: &'a [SpriteInstance], // depth off, slot 0 bound, ring branch only
        pub ui: &'a [DrawGroup],         // depth off, drawn last, grouped by slot
    }
    ```
    drawn via `draw_offscreen_scene` / `draw_to_swapchain_scene` /
    `draw_offscreen_readback_scene`.
  - **The `overlay` layer binds texture slot 0 and is only honest for
    procedural ring instances** (`SpriteInstance::ring`, sentinel
    `uv_rect.x < 0`). Any *textured* depth-off content must go in `ui`.
  - `assets/sprites/generated/rts/buildings.png` and `props.png` are 128 × 256,
    a 4-column × 8-row grid of 32 px cells addressed by
    `render::frame_uv_rect(row, col)`. Their contents:
    - buildings: `(0,0)` HQ, `(0,1)` Depot, `(0,2)` Barracks,
      `(1,0)` HQ site, `(1,1)` Depot site, `(1,2)` Barracks site,
      `(2,0)` Crystal node, `(2,1)` Gas node,
      `(2,2)` Crystal depleted, `(2,3)` Gas depleted. All other cells transparent.
    - props: `(0,0)` selection ring art, `(0,1)` placement OK tile,
      `(0,2)` placement BAD tile, `(0,3)` rally flag, `(1,0)` crystal icon,
      `(1,1)` gas icon, `(1,2)` supply icon, `(1,3)` opaque panel fill.
  - `worker.png` / `soldier.png` are `(dir, frame)` sheets addressed by
    `frame_uv_rect(dir, frame)`, 8 dirs × 4 frames.
- **From Depends (T4) — spell out:**
  - `IsoView { tile_w, tile_h, origin, map_height_px, depth_scale, depth_bias, view_size }`,
    `IsoView::{project, unproject, cell_at, with_center_cell, depth, frame_uniforms}`.
  - `render::Camera::{new(width, height, cell_size_px, view_size, start), iso_view, center, pan_cells, pan_tick, look_at_cell}`,
    `render::{edge_pan_dir, screen_dir_to_cells, CAMERA_PAN_CELLS_PER_SEC = 24.0, EDGE_PAN_MARGIN_PX = 12.0}`.
  - `render::{quad_is_visible, VIEW_WIDTH = 1920, VIEW_HEIGHT = 1080}`.
- **From Depends (T8) — spell out:**
  - `rts::{Selection, Pick, pick_at, box_select, normalise_rect, is_drag, DRAG_MIN_PX = 4.0, footprint_contains, footprint_min}`.
    `Selection::ids() -> &[EntityId]` ascending by slot; `RtsWorld::selection()`.
- **From Depends (T11) — spell out:**
  - `rts::{EntityStore, EntityId, EntityKind::{Unit, Building, Node}, UnitKind::{Worker, Soldier}, BuildingKind::{Hq, Depot, Barracks}, ResourceKind::{Crystal, Gas}, MAX_ENTITIES = 2_048, OWNER_PLAYER, OWNER_NEUTRAL}`.
  - `EntityStore`: `position(slot) -> [f32; 2]` (cell space, buildings hold their
    footprint **centre**), `dir`, `frame`, `progress`, `progress_target`,
    `amount`, `kind`, `owner`, `collect_live(&self, out: &mut Vec<usize>)`,
    `id_at`, `slot`.
  - `BuildingKind::footprint_cells()` = `12 / 8 / 10`.
    `RtsWorld::is_site(id)` is `progress_target > 0`.
  - `rts::{Placement::{None, Pending { kind }}, placement_valid(world, kind, min) -> Result<(), PlacementError>}`.
  - `RtsWorld::{scenario, entities, selection, placement, rally(building), production_queue, resources, supply, tick_index, nav, tick, state_hash}`.
  - `tick()`'s system order: *1. commands, 2. camera, 3. construction,
    4. production, 5. orders, 6. movement, 7. supply recount*, then selection
    self-heal. **Step 2 is empty and this ticket fills it.**
  - Tracked scene: 320 × 320 cells, `cell_size_px: 4`, `sprite_size_px: 48`.

## Exact design — no decisions left

### Camera in the world

`RtsWorld` gains:

```rust
/// Private fields.
camera: Camera,
/// Screen-space pan direction applied every tick, set by the input layer.
/// Each component in `-1.0..=1.0`.
pan_dir: [f32; 2],
```

```rust
impl RtsWorld {
    pub fn camera(&self) -> &Camera;
    pub fn camera_mut(&mut self) -> &mut Camera;
    /// The projection this frame packs through.
    pub fn iso_view(&self) -> IsoView;      // == self.camera.iso_view()
    /// Set the per-tick pan direction, in **screen** space.
    pub fn set_pan_dir(&mut self, dir: [f32; 2]);
    pub fn pan_dir(&self) -> [f32; 2];
}
```

`from_scenario` builds
`Camera::new(width, height, cell_size_px as f32, [VIEW_WIDTH as f32, VIEW_HEIGHT as f32], hq_center)`
where `hq_center` is the HQ footprint centre — the base is what a player wants to
see on frame 1.

**Step 2 of `tick()`** becomes exactly:

```rust
let v = self.camera.iso_view();
let d = screen_dir_to_cells(self.pan_dir, v.tile_w, v.tile_h);
self.camera.pan_tick(d, TICK_DT);
```

`state_hash` gains the camera centre (both `f32`s as raw bits) after the
production table. The camera is world state: a replay that ends looking
somewhere else did not reproduce.

### `crates/mmd-engine/src/rts/pack.rs`

```rust
/// Named cells of the props sheet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Prop {
    SelectionRing = 0, PlacementOk = 1, PlacementBad = 2, RallyFlag = 3,
    CrystalIcon = 4, GasIcon = 5, SupplyIcon = 6, PanelFill = 7,
}

/// UV rect of a prop cell. `(row, col) = (i / 4, i % 4)`.
pub fn prop_uv(prop: Prop) -> [f32; 4];

/// UV rect of a building's sprite. Row 0 finished, row 1 under construction.
pub fn building_uv(kind: BuildingKind, under_construction: bool) -> [f32; 4];

/// UV rect of a resource node. Row 2; columns 0/1 full, 2/3 depleted.
pub fn node_uv(kind: ResourceKind, depleted: bool) -> [f32; 4];

/// Texture slot a unit kind draws from.
pub fn unit_slot(kind: UnitKind) -> u32;   // Worker -> 4, Soldier -> 5

/// The screen quad of a building of `edge` cells under a given tile.
///
/// Width is the footprint diamond's full width (`edge * tile_w`); height is
/// twice the diamond's height (`edge * tile_h * 2`), which is what makes a
/// building read as a solid standing on its plot rather than as a flat decal on
/// it. For the tracked scene (`tile 8 x 4`) that is 96x96 (HQ), 64x64 (Depot)
/// and 80x80 (Barracks) px.
pub fn building_quad_px(edge_cells: u32, tile_w: f32, tile_h: f32) -> [f32; 2];

/// Selection-ring tint, premultiplied. Green, deliberately not the cyan the
/// hitbox overlay uses: two rings that meant different things in the same
/// colour would be worse than no ring.
pub const SELECTION_TINT: [f32; 4] = [0.0, 0.60, 0.24, 0.60];
/// Selection-ring radii in normalised quad units, matching
/// `runtime::{RING_INNER, RING_OUTER}`'s convention (`0.5` is the quad edge).
pub const SELECTION_RING_OUTER: f32 = 0.5;
pub const SELECTION_RING_INNER: f32 = SELECTION_RING_OUTER - 1.0 / 24.0;

/// Drag-rectangle edge thickness in screen pixels.
pub const DRAG_BOX_THICKNESS_PX: f32 = 2.0;
/// Drag-rectangle tint, premultiplied.
pub const DRAG_BOX_TINT: [f32; 4] = [0.16, 0.80, 0.32, 0.80];
/// Placement-ghost tint, premultiplied white — the sheet already carries the
/// colour and the alpha.
pub const GHOST_TINT: [f32; 4] = [1.0, 1.0, 1.0, 1.0];

/// Reusable per-frame instance buffers.
///
/// Owned by the caller (the app, or a test), not by `RtsWorld`: packing is a
/// projection of world state and must never be able to mutate it.
#[derive(Debug)]
pub struct RtsFrame {
    /// Depth-tested world groups, in slot order 4, 5, 6.
    pub world: Vec<DrawGroup>,
    /// Procedural rings only — selection rings.
    pub overlay: Vec<SpriteInstance>,
    /// Depth-off textured groups: the placement ghost, the drag box, and
    /// (from T13) the HUD. Slot 7 first, then slot 8.
    pub ui: Vec<DrawGroup>,
}

impl RtsFrame {
    /// Reserve every buffer at the ceilings it can reach, so a frame never grows one.
    pub fn new() -> Self;
    /// Clear every buffer, keeping capacity.
    pub fn clear(&mut self);
    /// A `ScenePass` borrowing this frame.
    pub fn scene(&self) -> ScenePass<'_>;
    /// Total instances across all three layers.
    pub fn instance_count(&self) -> usize;
}

/// The drag rectangle currently being dragged, in screen pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DragBox { pub a: [f32; 2], pub b: [f32; 2] }

/// Pack one frame of `world` into `frame`.
///
/// `cursor` is the pointer position in screen pixels, used to place the
/// placement ghost. `drag` is the live drag rectangle, if any.
///
/// Allocates nothing: every push lands in a buffer `RtsFrame::new` reserved.
pub fn pack_frame(
    world: &RtsWorld,
    cursor: [f32; 2],
    drag: Option<DragBox>,
    frame: &mut RtsFrame,
);
```

`RtsFrame::new` reserves:
- `world`: exactly 3 groups — `atlas_id` `4`, `5`, `6` — each
  `Vec::with_capacity(MAX_ENTITIES)`.
- `overlay`: `Vec::with_capacity(MAX_ENTITIES)`.
- `ui`: exactly 2 groups — `atlas_id` `7` then `8` — the first with capacity
  `MAX_ENTITIES` (ghost tiles are one per footprint cell, max 144, plus 4 drag
  edges; `MAX_ENTITIES` is generous and costs 96 KB once), the second with
  capacity `4_096` (HUD glyphs, T13).

`RtsFrame::clear` clears each group's `instances` and `overlay` but never
changes `atlas_id` or drops capacity.

### `pack_frame` body — exact order

```
frame.clear();
let iso = world.iso_view();
let store = world.entities();
store.collect_live(&mut scratch);          // a caller-free local Vec on RtsFrame

// 1. World: nodes, then buildings, then units.
//    Order does not affect the picture (the depth test sorts it) but it does
//    affect the packed byte order, and the frame is hashed in tests.
for &slot in &scratch {
    match store.kind(slot) {
        EntityKind::Node(res) => {
            let ground = iso.project(pos[0], pos[1]);
            let size = [SPRITE_PX, SPRITE_PX];               // 48.0, from scenario.sprite_size_px()
            let p = [ground[0] - size[0] * 0.5, ground[1] - size[1]];
            if !quad_is_visible(p, size, iso.view_size) { continue; }
            group(6).push(SpriteInstance::new(p, size, node_uv(res, store.amount(slot) == 0), WHITE));
        }
        EntityKind::Building(b) => {
            let ground = iso.project(pos[0], pos[1]);
            let size = building_quad_px(b.footprint_cells(), iso.tile_w, iso.tile_h);
            let p = [ground[0] - size[0] * 0.5, ground[1] - size[1]];
            if !quad_is_visible(p, size, iso.view_size) { continue; }
            let uc = store.progress_target(slot) > 0;
            group(6).push(SpriteInstance::new(p, size, building_uv(b, uc), WHITE));
        }
        EntityKind::Unit(u) => {
            let ground = iso.project(pos[0], pos[1]);
            let size = [SPRITE_PX, SPRITE_PX];
            let p = [ground[0] - size[0] * 0.5, ground[1] - size[1]];
            if !quad_is_visible(p, size, iso.view_size) { continue; }
            let uv = frame_uv_rect(store.dir(slot) as u32, store.frame(slot) as u32);
            group(unit_slot(u)).push(SpriteInstance::new(p, size, uv, WHITE));
        }
    }
}

// 2. Overlay: one procedural ring per selected entity.
//    Quad size is `runtime::ring_quad_size_px(tile_w, tile_h, r)` where `r` is
//    the body radius for a unit and `edge * 0.5` cells for a building — the same
//    expression the hitbox overlay uses, so "the ring shows the shape the world
//    uses" stays one derivation.
for id in world.selection().ids() {
    // centred ON the ground point, like `runtime::pack_ring_instances`
    frame.overlay.push(SpriteInstance::ring(p, size, SELECTION_RING_INNER, SELECTION_RING_OUTER, SELECTION_TINT));
}

// 3. UI slot 7: rally flags for selected producing buildings, then the
//    placement ghost, then the drag box.
for id in world.selection().ids() {
    if let Some(cell) = world.rally(id) { push a RallyFlag quad at that cell's ground point }
}
if let Placement::Pending { kind } = world.placement() {
    let Some(cell) = iso.cell_at(cursor[0], cursor[1], w, h) else { skip };
    let min = ghost_min_corner(cell, kind.footprint_cells());
    let ok = placement_valid(world, kind, min).is_ok();
    // one tile per footprint cell
    for c in footprint_cells_from_min(min, kind.footprint_cells()) {
        push a tile quad of size [tile_w, tile_h * 2.0] centred on that cell's ground point,
        uv = prop_uv(if ok { Prop::PlacementOk } else { Prop::PlacementBad });
    }
    // plus one silhouette of the building itself, at the footprint centre
    push building_uv(kind, true) at building_quad_px(...) with GHOST_TINT;
}
if let Some(d) = drag {
    let (min, max) = normalise_rect(d.a, d.b);
    push 4 PanelFill quads forming the rectangle's edges, DRAG_BOX_THICKNESS_PX thick,
    tinted DRAG_BOX_TINT;
}
```

```rust
/// The minimum corner of a footprint of `edge` cells centred on `cell`.
///
/// `cell - edge/2`, saturating at zero. The ghost follows the cursor's cell as
/// its **centre**, which is what every RTS does; anchoring the min corner to the
/// cursor makes a 12-cell building appear down-right of the pointer.
pub fn ghost_min_corner(cell: Cell, edge: u32) -> Cell;
```

`RtsFrame` carries a private `scratch: Vec<usize>` with capacity `MAX_ENTITIES`
so `collect_live` never allocates.

## TDD

1. **Red** — write every test below: headless ones in a new
   `crates/mmd-engine/tests/rts_pack.rs`, the two GPU ones appended to
   `crates/mmd-engine/tests/gpu_smoke.rs`. Watch them fail.
2. **Green** — implement.
3. **Refactor** — none expected. Keep green.

## Test plan

Harness `RtsHarness::scene()` unless stated; `frame = RtsFrame::new()`;
`cursor = [960.0, 540.0]`.

| Test | Input | Expect |
| ---- | ----- | ------ |
| `frame_new_reserves_the_documented_groups` | fresh frame | `world` has 3 groups with `atlas_id` `4,5,6`; `ui` has 2 with `7,8` |
| `prop_uv_maps_to_the_published_cells` | all 8 props | `(row, col)` of `(0,0)…(1,3)` in order |
| `building_uv_switches_row_on_construction` | Depot finished vs site | row 0 vs row 1, same column |
| `node_uv_switches_column_on_depletion` | Crystal full vs depleted | col 0 vs col 2 |
| `unit_slot_separates_the_two_kinds` | both | `4` and `5` |
| `building_quad_matches_the_documented_sizes` | edges 12/8/10 at tile `8×4` | `[96,96] / [64,64] / [80,80]` |
| `every_entity_packs_exactly_once` | camera centred on the base, all 17 entities on screen | `world` instance total `== 17` |
| `nodes_and_buildings_share_the_building_slot` | same | group `6` holds `1 + 10 == 11` |
| `workers_land_in_the_worker_slot` | same | group `4` holds `6`, group `5` holds `0` |
| `a_soldier_lands_in_the_soldier_slot` | spawn one | group `5` holds `1` |
| `a_units_uv_is_its_dir_and_frame` | set `dir = 3`, `frame = 2` | that instance's `uv_rect == frame_uv_rect(3, 2)` |
| `a_site_draws_the_construction_sprite` | place a Depot | its instance's `uv_rect == building_uv(Depot, true)` |
| `a_depleted_node_draws_the_depleted_sprite` | `set_amount(node_slot, 0)` | `uv_rect == node_uv(Crystal, true)` |
| `sprites_stand_on_their_ground_point` | one worker | `pos[1] + size[1] == iso.project(p).1` exactly |
| `offscreen_entities_are_culled` | pan the camera 200 cells away | `instance_count()` for the world layer is `0` |
| `culling_uses_the_same_rect_as_the_horde` | an entity straddling the left edge | packed when any pixel is on screen, dropped when `pos.x + size.x == 0` |
| `packing_follows_the_camera` | pack, pan 10 cells, pack again | every world instance's `pos` moved by the same screen delta |
| `selection_rings_are_procedural` | select 3 units, pack | `overlay.len() == 3`, and every instance's `is_ring()` is true |
| `nothing_selected_means_no_rings` | clear selection | `overlay.is_empty()` |
| `a_selected_building_gets_a_ring_sized_to_its_footprint` | select the HQ | its ring's `size` equals `ring_quad_size_px(tile_w, tile_h, 6.0)` |
| `the_ghost_only_appears_while_pending` | no placement | `ui[0].instances.is_empty()` |
| `the_ghost_follows_the_cursor_cell` | `begin_placement(Depot)`, two different cursors | the ghost's tile positions differ by the projected cell delta |
| `a_valid_ghost_is_green` | Depot over `(180, 176)` | every tile's `uv_rect == prop_uv(Prop::PlacementOk)` |
| `an_invalid_ghost_is_red` | Depot over the HQ | every tile's `uv_rect == prop_uv(Prop::PlacementBad)` |
| `the_ghost_covers_the_whole_footprint` | Depot (edge 8) | `64` tile instances plus one silhouette |
| `ghost_min_corner_centres_the_footprint` | `cell (100, 100)`, edge 8 | `(96, 96)` |
| `the_drag_box_is_four_edges` | `Some(DragBox { a: [10,10], b: [110,60] })` | 4 more instances in `ui[0]`, each of thickness `2.0` on one axis |
| `the_drag_box_normalises_its_corners` | drag from bottom-right to top-left | identical instances to the forward drag |
| `no_drag_means_no_box` | `None` | no drag instances |
| `a_rally_flag_draws_for_a_selected_building` | `set_rally(hq, Some(c))`, select the HQ | one `Prop::RallyFlag` instance at `c`'s ground point |
| `an_unselected_buildings_rally_is_not_drawn` | rally set, nothing selected | none |
| `pack_frame_allocates_nothing` | in `frame_allocations.rs`, `MeasureGuard` around 600 `pack_frame` calls on the full scene with a pending ghost and a drag | zero allocations |
| `pack_frame_does_not_mutate_the_world` | hash before and after 100 packs | equal `state_hash()` |
| `the_camera_starts_on_the_base` | fresh world | `iso_view().project(166.0, 166.0)` is `[960.0, 540.0]` within `1e-3` |
| `pan_dir_moves_the_camera_each_tick` | `set_pan_dir([1.0, 0.0])`, `step_exact(60)` | the camera centre moved by `screen_dir_to_cells([1,0], 8, 4) * 24.0` |
| `zero_pan_dir_holds_the_camera` | `step_exact(600)` | centre unchanged |
| `state_hash_sees_the_camera` | pan one tick | hash differs |
| `the_frame_renders` (GPU) | pack the base scene, `draw_offscreen_readback_scene(frame.scene())` | more than 1 000 pixels have `a > 0` |
| `the_ghost_draws_over_the_world` (GPU) | pending Depot over the HQ | at least one pixel inside the HQ's screen rect carries the placement-BAD colour |

**Mutation verification (mandatory).** Inject, confirm red, revert, confirm green:
1. Anchor sprites centred instead of bottom-edge → kills `sprites_stand_on_their_ground_point`.
2. `building_quad_px` drops the `* 2.0` on height → kills `building_quad_matches_the_documented_sizes`.
3. `building_uv` ignores `under_construction` → kills `a_site_draws_the_construction_sprite`.
4. `node_uv` ignores depletion → kills `a_depleted_node_draws_the_depleted_sprite`.
5. Remove the `quad_is_visible` cull → kills `offscreen_entities_are_culled`.
6. `ghost_min_corner` returns `cell` unchanged → kills `ghost_min_corner_centres_the_footprint` and `the_ghost_follows_the_cursor_cell` stays green (it only checks the delta) — note that in the commit body.
7. Push selection rings as textured instances → kills `selection_rings_are_procedural`.
8. `pack_frame` forgets `frame.clear()` → kills `every_entity_packs_exactly_once` on the second call; make sure that test packs **twice**.
9. Camera pan applied before `screen_dir_to_cells` → kills `pan_dir_moves_the_camera_each_tick`.

## Impl steps

- [ ] 1. Add `camera`, `pan_dir` fields to `RtsWorld` and the five accessors; build the camera in `from_scenario` centred on the HQ.
- [ ] 2. Implement step 2 of `tick()` exactly as quoted; extend `state_hash` with the camera centre.
- [ ] 3. Create `crates/mmd-engine/src/rts/pack.rs` with the constants, `Prop`, `DragBox`, `RtsFrame`.
- [ ] 4. Add `mod pack;` and the re-exports to `crates/mmd-engine/src/rts/mod.rs`.
- [ ] 5. Create `crates/mmd-engine/tests/rts_pack.rs` and write every headless test from the table. Watch them fail.
- [ ] 6. Implement `prop_uv`, `building_uv`, `node_uv`, `unit_slot`, `building_quad_px`, `ghost_min_corner`.
- [ ] 7. Implement `RtsFrame::{new, clear, scene, instance_count}` with the exact reservations listed.
- [ ] 8. Implement `pack_frame`'s three phases in the exact order quoted.
- [ ] 9. Append the two GPU tests to `crates/mmd-engine/tests/gpu_smoke.rs`.
- [ ] 10. Add `pack_frame_allocates_nothing` to `crates/mmd-engine/tests/frame_allocations.rs`.
- [ ] 11. Run the mutation list; record kills in the commit body.
- [ ] 12. Run the full validation block.

## Outputs

- **Files created**
  - `crates/mmd-engine/src/rts/pack.rs`
  - `crates/mmd-engine/tests/rts_pack.rs`
- **Files edited**
  - `crates/mmd-engine/src/rts/{mod,world}.rs`
  - `crates/mmd-engine/tests/{gpu_smoke,frame_allocations}.rs`
- **Public API added:** `rts::{Prop, DragBox, RtsFrame, pack_frame, prop_uv, building_uv, node_uv, unit_slot, building_quad_px, ghost_min_corner, SELECTION_TINT, SELECTION_RING_INNER, SELECTION_RING_OUTER, DRAG_BOX_THICKNESS_PX, DRAG_BOX_TINT, GHOST_TINT}`, `RtsWorld::{camera, camera_mut, iso_view, set_pan_dir, pan_dir}`.
- **Behaviour change:** the RTS world is drawable and its camera pans.
- **Migration / config:** none.

## Validation

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo test -p mmd-engine --test rts_pack` — all green
- [ ] `cargo test -p mmd-engine --test frame_allocations` — all green
- [ ] `MMD_REQUIRE_GPU=1 cargo test --workspace --locked`
- [ ] `VK_DRIVER_FILES=/nonexistent cargo test --workspace --locked` — GPU cases skip cleanly
- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- [ ] `nix flake check`
- [ ] `git diff --stat HEAD -- lab/goldens/` — **empty**
- [ ] `cargo run -- run --agents 5000 --frames 300` — exit 0, exit-line `hash=` unchanged
- [ ] app functional — no broken path from this slice
- [ ] commit msg draft: `feat(rts): pack the RTS world into draw groups`
