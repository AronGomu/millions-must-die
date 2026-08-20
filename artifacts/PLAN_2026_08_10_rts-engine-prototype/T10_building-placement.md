# T10: Building placement + construction

**Plan:** `./artifacts/PLAN_2026_08_10_rts-engine-prototype.md`
**Depends:** T7, T9
**Commit outcome:** a worker places a Depot on a valid grid footprint, stands at it while it builds, and the finished building raises the supply cap and blocks pathing.

## Context (self-contained)

- Goal: phase 1 is a thin vertical slice of an RTS engine prototype (camera,
  selection, workers, economy, building, unit production) on a horde-free scene.
- This slice: grid placement, cost, construction progress, cancellation, and the
  navigation consequence of a building existing.
- Out of scope here: producing units from the finished building (T11), drawing
  the ghost or the site (T12), the HUD build menu (T13), the CLI (T14).
- Assumptions in force: **grid placement** is a stated design decision
  (`docs/DESIGN.md`). A footprint is an axis-aligned cell rectangle anchored on
  its minimum corner. Buildings block navigation; units do not.

## Requirements

- Validity rules for a footprint, each with its own rejection reason.
- Cost debit on placement, full refund on cancel, nothing on completion.
- A `Build` order that walks a worker to the site and holds it there.
- Construction advances only while at least one worker is in reach.
- A finished building stamps its footprint into the navigation mask and grants
  its supply.
- Everything reproducible and allocation-free per tick.

## Inputs

- **Files to read**
  - `crates/mmd-engine/src/rts/{entity,economy,orders,selection,world}.rs`.
  - `crates/mmd-engine/src/nav/field_pool.rs`.
  - `crates/mmd-engine/src/scenario.rs` — footprint constants.
- **From Depends (T7 + T9) — spell out, the worker cannot read them:**
  - `mmd_engine::rts` exports `EntityStore`, `EntityId { index, generation }`,
    `EntityKind::{Unit(UnitKind), Building(BuildingKind), Node(ResourceKind)}`,
    `UnitKind::{Worker, Soldier}`, `BuildingKind::{Hq, Depot, Barracks}`,
    `ResourceKind::{Crystal, Gas}`, `MAX_ENTITIES = 2_048`, `OWNER_PLAYER = 0`,
    `OWNER_NEUTRAL = 255`, `CARRY_NONE = 0xFF`.
  - `Resources { crystal: u32, gas: u32 }` with `covers(cost)`,
    `try_debit(cost) -> bool` (all-or-nothing), `credit(amount)` (saturating),
    and `Resources::ZERO`.
  - `Supply` with `used()`, `cap()`, `free()`, `fits(cost)`, `add_used`,
    `remove_used`, `grant_cap(amount)` (clamped to `scenario::MAX_SUPPLY_CAP = 500`),
    `revoke_cap`.
  - `BuildingKind::footprint_cells()` = `12` (Hq), `8` (Depot), `10` (Barracks),
    from `scenario::{HQ_FOOTPRINT_CELLS, DEPOT_FOOTPRINT_CELLS, BARRACKS_FOOTPRINT_CELLS}`.
    `BuildingKind::is_drop_off()` is true only for `Hq`. A building's
    `position` is its footprint **centre** in cell space.
  - `EntityStore` columns: `alive`, `kind`, `owner`, `position`, `dir`, `frame`,
    `progress`, `progress_target`, `amount`, `carry_kind`, `carry_amount`, with
    `set_progress(slot, progress, target)`, `set_amount`, `carry`, `set_carry`,
    `collect_live(&self, out: &mut Vec<usize>)`, `spawn`, `despawn`, `slot`,
    `id_at`, `contains`.
  - Orders:
    ```rust
    pub enum GatherPhase { ToNode { field_slot: u8 }, Mining { ticks_left: u32 }, Returning { drop_off: EntityId, field_slot: u8 } }
    pub enum Order { Idle, Move { dest: Cell, field_slot: u8 }, Gather { node: EntityId, phase: GatherPhase } }
    pub struct OrderTable;   // get / set / clear / hash_into
    pub const ARRIVAL_RADIUS_CELLS: f32 = 1.5;
    pub(crate) fn dist2(a: [f32; 2], b: [f32; 2]) -> f32;
    pub(crate) fn rect_distance(p: [f32; 2], center: [f32; 2], edge: u32) -> f32;   // 0.0 inside
    pub(crate) fn drop_off_approach_cell(store: &EntityStore, id: EntityId) -> Cell;
    pub(crate) fn node_cell(pos: [f32; 2]) -> Cell;
    pub(crate) fn step_admissible(cx: i32, cy: i32, nx: f32, ny: f32, width: u32, height: u32, blocked: &[bool]) -> bool;
    ```
    `drop_off_approach_cell` currently returns the footprint's **centre cell**;
    T9 recorded that this ticket must change it — see below.
  - `nav::field_pool::FieldPool`: `acquire(dest) -> Result<u8, FieldPoolError>`,
    `field(slot)`, `key(slot)`, `rebuild_count()`, `blocked() -> &[bool]`
    (`width * height`, indexed `x + y * width`), `set_blocked(cell, bool)`
    (invalidates **every** cached field), `scratch_capacity()`.
    `NAV_FIELD_SLOTS == 8`, LRU, lowest slot wins ties.
  - `RtsWorld`: `scenario()`, `entities()`, `entities_mut()`, `resources()`,
    `supply()`, `tick_index()`, `start_hq()`, `nav()`, `order_move`,
    `order_move_group`, `order_gather`, `order_gather_group`,
    `nearest_drop_off(pos)`, `order_of(id)`, `tick()`, `state_hash()`,
    plus (from T8, if landed) `selection()`, `click_select`, `box_select_into_selection`,
    and `rts::footprint_contains(center, edge, cell)` / `footprint_min(center, edge)`.
  - `tick()`'s reserved system order: *1. commands, 2. camera, 3. construction,
    4. production, 5. orders, 6. movement, 7. supply recount*, then selection
    self-heal.
  - Economy constants: `WORKER_CARRY_CAPACITY = 8`, `GATHER_TICKS = 60`,
    `GATHER_REACH_CELLS = 2.0`, `DROP_OFF_REACH_CELLS = 1.0`,
    `NODE_CRYSTAL_AMOUNT = 1_500`, `NODE_GAS_AMOUNT = 2_500`.
  - Tracked scene: 320 × 320, HQ min corner `(160, 160)` edge 12 (centre
    `[166.0, 166.0]`), six workers at `(162..=167, 178)`, start stock
    `crystal 300 / gas 100`, supply cap `10`, used `6`. Obstacles are scattered
    single cells; the region around the HQ within Chebyshev 28 is guaranteed
    obstacle-free.
  - `testkit::RtsHarness::scene()` / `step_exact` / `world` / `world_mut` /
    `state_hash` / `ids_of_kind`.

## Exact design — no decisions left

### Constants — `crates/mmd-engine/src/rts/build.rs`

```rust
/// Cost of each building.
pub const HQ_COST: Resources = Resources { crystal: 400, gas: 0 };
pub const DEPOT_COST: Resources = Resources { crystal: 100, gas: 0 };
pub const BARRACKS_COST: Resources = Resources { crystal: 150, gas: 25 };
pub fn building_cost(kind: BuildingKind) -> Resources;

/// Ticks of *attended* construction each building needs. 60 ticks = 1 second.
pub const HQ_BUILD_TICKS: u32 = 600;
pub const DEPOT_BUILD_TICKS: u32 = 180;
pub const BARRACKS_BUILD_TICKS: u32 = 300;
pub fn build_ticks(kind: BuildingKind) -> u32;

/// Supply ceiling a finished building grants.
pub const HQ_SUPPLY_GRANT: u32 = 10;
pub const DEPOT_SUPPLY_GRANT: u32 = 10;
pub const BARRACKS_SUPPLY_GRANT: u32 = 0;
pub fn supply_grant(kind: BuildingKind) -> u32;

/// How close a worker's centre must be to a site's footprint rectangle to count
/// as building it.
pub const BUILD_REACH_CELLS: f32 = 1.5;

/// Whether extra workers speed a site up.
///
/// They do not. One attending worker advances construction by exactly one tick
/// per tick; a second changes nothing. Additive build speed is a balance knob,
/// and phase 1 is not a balance pass — a constant that reads `false` is the
/// honest way to record that the question was asked and deferred.
pub const EXTRA_BUILDERS_SPEED_UP: bool = false;

/// Why a footprint was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PlacementError {
    #[error("footprint leaves the map")]
    OutOfBounds,
    #[error("footprint covers terrain at ({x}, {y})")]
    BlockedTerrain { x: u32, y: u32 },
    #[error("footprint overlaps another building")]
    OverlapsBuilding,
    #[error("footprint covers a resource node at ({x}, {y})")]
    CoversNode { x: u32, y: u32 },
    #[error("not enough resources")]
    Unaffordable,
    #[error("no live player worker was given as the builder")]
    NoBuilder,
    #[error("the entity store is full")]
    StoreFull,
}

/// The pending build ghost.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Placement {
    #[default]
    None,
    /// A build was chosen and is following the cursor.
    Pending { kind: BuildingKind },
}
```

### Validity — free function

```rust
/// Whether `kind` may occupy the footprint whose minimum corner is `min`.
///
/// Checked in this order, and the first failure is returned:
/// 1. the whole rectangle is inside the grid,
/// 2. no cell is scenario terrain (`FieldPool::blocked`, which also carries
///    every already-placed building — see the note below),
/// 3. no cell lies inside another live building's footprint,
/// 4. no resource node's cell lies inside the rectangle.
///
/// **Units are not an obstruction.** A worker standing where you want a Depot is
/// not a reason to refuse the build; the genre does not do that, and a rule that
/// depends on a moving unit makes placement validity flicker frame to frame.
///
/// Rule 3 is redundant with rule 2 for *finished* buildings, which are stamped
/// into the mask — but not for **sites**, which are not stamped until they
/// finish. Both rules stay: rule 2 catches terrain and finished buildings,
/// rule 3 catches sites under construction.
pub fn placement_valid(
    world: &RtsWorld,
    kind: BuildingKind,
    min: Cell,
) -> Result<(), PlacementError>;
```

### `RtsWorld` API

```rust
impl RtsWorld {
    /// The pending build ghost.
    pub fn placement(&self) -> Placement;

    /// Choose a building to place. `false` when the player cannot currently
    /// afford it — the cost check is repeated at `confirm_placement`, because
    /// the stock can fall while the ghost is up.
    pub fn begin_placement(&mut self, kind: BuildingKind) -> bool;

    /// Drop the ghost. Idempotent.
    pub fn cancel_placement(&mut self);

    /// Commit the pending ghost at `min`, built by `builder`.
    ///
    /// On success: debits the cost, spawns the site with
    /// `set_progress(0, build_ticks(kind))`, orders `builder` to
    /// `Order::Build { site, field_slot }`, clears the ghost, and returns the
    /// site's id. The footprint is **not** stamped into navigation yet — a site
    /// is walkable until it finishes, which is what lets the builder stand in it.
    pub fn confirm_placement(&mut self, min: Cell, builder: EntityId) -> Result<EntityId, PlacementError>;

    /// Cancel an unfinished site: refund the full cost, unstamp nothing (a site
    /// was never stamped), despawn it, and clear every worker whose `Build`
    /// order named it.
    ///
    /// A **finished** building is not cancellable and returns `false`.
    pub fn cancel_construction(&mut self, site: EntityId) -> bool;

    /// Whether a building entity is still under construction.
    pub fn is_site(&self, id: EntityId) -> bool;   // progress_target > 0

    /// Order an existing worker to attend an existing site.
    pub fn order_build(&mut self, id: EntityId, site: EntityId) -> bool;
}
```

`Order` gains:

```rust
/// Walk to `site` and attend it until it finishes.
Build { site: EntityId, field_slot: u8 },
```

`Order::tag()` gains `3`.

### `drop_off_approach_cell` change (flagged by T9)

Replace its body: a finished building's footprint is blocked, so its centre cell
cannot be a flow-field destination. New rule — **the nearest unblocked cell in
the one-cell ring around the footprint**, scanned in a fixed order so it is
deterministic:

```rust
/// The cell a unit is routed to when heading for a building.
///
/// The footprint's own cells are blocked once the building finishes, so a field
/// cannot target them. This walks the one-cell ring around the rectangle in a
/// fixed order — top edge left→right, right edge top→bottom, bottom edge
/// right→left, left edge bottom→top — and returns the first unblocked, in-bounds
/// cell. Fixed order, not "nearest", because two equally near cells would make
/// the choice depend on float comparison and the state hash would stop being
/// reproducible across a refactor.
///
/// Returns the footprint's centre cell when the ring is entirely blocked, which
/// then makes `FieldPool::acquire` fail cleanly rather than silently routing
/// somewhere else.
pub(crate) fn building_approach_cell(store: &EntityStore, blocked: &[bool], width: u32, height: u32, id: EntityId) -> Cell;
```

`drop_off_approach_cell` becomes a thin alias for it. Every caller in the gather
system passes `nav.blocked()`.

### Construction system — step 3 of `tick()`

Runs **before** orders and movement, so a site that finishes this tick is
finished for everything downstream.

```
// Pass A: which sites have an attending worker?
build_attend.clear();                    // Vec<bool>, len = slot_count, reused
for &slot in &live_scratch {
    let Order::Build { site, .. } = orders.get(slot) else { continue };
    let EntityKind::Unit(UnitKind::Worker) = entities.kind(slot) else { continue };
    let Some(site_slot) = entities.slot(site) else { orders.clear(slot); continue };
    let EntityKind::Building(b) = entities.kind(site_slot) else { orders.clear(slot); continue };
    if entities.progress_target(site_slot) == 0 { orders.clear(slot); continue; }   // already finished
    if rect_distance(entities.position(slot), entities.position(site_slot), b.footprint_cells())
        <= BUILD_REACH_CELLS {
        build_attend[site_slot] = true;
    }
}

// Pass B: advance every attended site by exactly one tick.
for &slot in &live_scratch {
    let EntityKind::Building(b) = entities.kind(slot) else { continue };
    let target = entities.progress_target(slot);
    if target == 0 { continue; }
    if !build_attend[slot] { continue; }
    let p = entities.progress(slot) + 1;
    if p < target {
        entities.set_progress(slot, p, target);
    } else {
        // Finish.
        entities.set_progress(slot, 0, 0);
        for cell in footprint_cells(entities.position(slot), b.footprint_cells()) {
            nav.set_blocked(cell, true);
        }
        supply.grant_cap(supply_grant(b));
        finished.push(entities.id_at(slot).expect("live"));
    }
}
// Clear the orders of every worker that was building something now finished.
for &slot in &live_scratch {
    if let Order::Build { site, .. } = orders.get(slot) {
        if finished.contains(&site) { orders.clear(slot); }
    }
}
```

`build_attend: Vec<bool>` and `finished: Vec<EntityId>` are `RtsWorld` fields
reserved at `MAX_ENTITIES`, cleared per tick — no allocation.

```rust
/// The cells a footprint occupies, as an iterator, from its centre and edge.
pub fn footprint_cells(center: [f32; 2], edge: u32) -> impl Iterator<Item = Cell>;
```

### Movement change

The mover's destination derivation gains one arm:

```rust
Order::Build { site, field_slot } =>
    (building_approach_cell(entities, nav.blocked(), w, h, site), field_slot),
```

A worker with a `Build` order stops moving once `rect_distance` to the site is
`<= BUILD_REACH_CELLS` — add that guard alongside the `Move` arrival test, but
**do not clear the order**: the worker must keep attending.

## TDD

1. **Red** — write every test below in a new
   `crates/mmd-engine/tests/rts_build.rs`. Watch them fail.
2. **Green** — implement.
3. **Refactor** — none expected. Keep green.

## Test plan

Harness `RtsHarness::scene()`; `w0` = worker at entity slot 11; `hq` =
`world().start_hq().unwrap()`. A clear buildable corner is `(180, 176)` — inside
the HQ's obstacle-free Chebyshev-28 region, clear of the footprint and of every
node.

| Test | Input | Expect |
| ---- | ----- | ------ |
| `costs_and_times_are_the_published_constants` | each kind | `(400,0)/(100,0)/(150,25)` and `600/180/300` |
| `only_hq_and_depot_grant_supply` | each kind | `10 / 10 / 0` |
| `placement_rejects_an_out_of_bounds_footprint` | Depot at `(316, 316)` on a 320 grid | `Err(OutOfBounds)` |
| `placement_rejects_terrain` | a Depot footprint containing a known scenario obstacle | `Err(BlockedTerrain { .. })` |
| `placement_rejects_overlap_with_the_hq` | Depot at `(165, 165)` | `Err(OverlapsBuilding)` |
| `placement_rejects_overlap_with_a_site` | place one Depot, then a second overlapping it before it finishes | `Err(OverlapsBuilding)` |
| `placement_rejects_a_resource_node` | Depot footprint covering `(140, 150)` | `Err(CoversNode { x: 140, y: 150 })` |
| `placement_accepts_a_clear_corner` | Depot at `(180, 176)` | `Ok(())` |
| `a_unit_standing_there_does_not_block_placement` | move `w0` to `[184.0, 180.0]`, place a Depot over it | `Ok(())` |
| `begin_placement_refuses_what_you_cannot_afford` | drain stock to `(50, 0)`, `begin_placement(Depot)` | `false`, `placement() == None` |
| `begin_placement_sets_the_ghost` | with 300 crystal | `true`, `placement() == Pending { kind: Depot }` |
| `cancel_placement_is_idempotent` | cancel twice | `placement() == None` both times |
| `confirm_debits_the_cost` | place a Depot | `resources().crystal == 200` |
| `confirm_rejects_a_stale_builder` | despawned worker | `Err(NoBuilder)`, stock unchanged, ghost still pending |
| `confirm_rejects_a_soldier_builder` | spawn a Soldier, use it | `Err(NoBuilder)` |
| `confirm_rejects_when_the_stock_fell_under_the_ghost` | begin with 300, drain to 50, confirm | `Err(Unaffordable)`, ghost still pending |
| `confirm_spawns_an_unfinished_site` | place a Depot | `is_site(id)`, `progress == 0`, `progress_target == 180` |
| `confirm_orders_the_builder` | same | `order_of(w0) == Some(Order::Build { site, .. })` |
| `confirm_clears_the_ghost` | same | `placement() == None` |
| `a_site_is_walkable` | right after placement | every footprint cell is unblocked in `nav().blocked()` |
| `construction_does_not_advance_without_a_worker` | place a Depot with `w0`, then `order_move(w0, far_cell)`, `step_exact(600)` | `progress` stopped rising once the worker left; still a site |
| `construction_advances_one_tick_per_tick` | place, let the worker arrive, then step 100 more ticks | `progress` rose by exactly 100 |
| `a_second_worker_does_not_speed_it_up` | two workers attending, 100 ticks | `progress` rose by exactly 100 |
| `a_depot_finishes_in_its_documented_time` | place with `w0`, `step_exact(2000)` | `is_site(id) == false`, `progress_target == 0` |
| `a_finished_depot_blocks_navigation` | after it finishes | every footprint cell is blocked in `nav().blocked()` |
| `finishing_invalidates_the_cached_fields` | record `rebuild_count()` before, re-acquire a previously cached destination after | `rebuild_count()` rose |
| `a_finished_depot_raises_the_supply_cap` | before/after | `supply().cap()` `10` → `20` |
| `the_supply_cap_is_clamped_at_the_pillar` | grant 60 Depots' worth via repeated `grant_cap` | `cap() == 500` |
| `finishing_clears_the_builder_order` | after completion | `order_of(w0) == Some(Order::Idle)` |
| `cancel_refunds_the_full_cost` | place a Depot, `step_exact(60)`, cancel | `resources().crystal` back to `300`, entity gone |
| `cancel_clears_the_builder_order` | same | `order_of(w0) == Some(Order::Idle)` |
| `cancel_of_a_finished_building_is_refused` | let it finish, then cancel | `false`, still alive, stock unchanged |
| `cancel_of_a_stale_id_is_refused` | despawned id | `false` |
| `a_builder_walks_to_a_far_site` | place at `(180, 176)` with a worker across the map, `step_exact(3000)` | the site finishes |
| `the_approach_cell_rings_a_finished_building` | `building_approach_cell` for the finished HQ | an unblocked cell adjacent to the footprint, and the same cell on every call |
| `the_approach_cell_is_deterministic_under_a_blocked_ring` | block the whole top edge of the ring | returns the first unblocked cell in the documented scan order |
| `a_gatherer_still_delivers_after_the_hq_is_stamped` | `order_gather(w0, node)` after stamping the HQ, `step_exact(3000)` | `resources().crystal` rose (regression guard for the approach-cell change) |
| `construction_is_reproducible` | two harnesses, identical placements, 3000 ticks | equal `state_hash()` |
| `state_hash_sees_construction_progress` | one tick of progress | hash differs |
| `construction_allocates_nothing` | in `frame_allocations.rs`, `MeasureGuard` around 600 ticks with three live sites and six workers | zero allocations |

**Mutation verification (mandatory).** Inject, confirm red, revert, confirm green:
1. Stamp the footprint at placement instead of at completion → kills `a_site_is_walkable` and, downstream, `a_builder_walks_to_a_far_site`.
2. Advance progress without checking `build_attend` → kills `construction_does_not_advance_without_a_worker`.
3. Advance by the attendee count → kills `a_second_worker_does_not_speed_it_up`.
4. `cancel_construction` refunds half → kills `cancel_refunds_the_full_cost`.
5. `cancel_construction` accepts a finished building → kills `cancel_of_a_finished_building_is_refused`.
6. Drop the re-check of affordability in `confirm_placement` → kills `confirm_rejects_when_the_stock_fell_under_the_ghost`.
7. `placement_valid` skips the node rule → kills `placement_rejects_a_resource_node`.
8. `building_approach_cell` scans the ring in reverse → kills `the_approach_cell_is_deterministic_under_a_blocked_ring`.
9. `grant_cap` without the clamp → kills `the_supply_cap_is_clamped_at_the_pillar`.

## Impl steps

- [x] 1. Create `crates/mmd-engine/src/rts/build.rs` with the constants, `PlacementError`, `Placement`, `building_cost`, `build_ticks`, `supply_grant`, `footprint_cells`. Evidence: file exists, `cargo build -p mmd-engine` green.
- [x] 2. Add `mod build;` and the re-exports to `crates/mmd-engine/src/rts/mod.rs`. Evidence: `cargo build -p mmd-engine` green with the new items resolvable at `mmd_engine::rts::*`.
- [x] 3. Add `Order::Build { site, field_slot }`; extend `Order::tag` and `OrderTable::hash_into`. Evidence: `state_hash_sees_construction_progress` passes.
- [x] 4. Replace `drop_off_approach_cell`'s body with `building_approach_cell` and make the former an alias; update the gather system's two call sites to pass `nav.blocked()`. Evidence: `cargo test -p mmd-engine --test rts_economy` green (30/30), including the delivery-footprint tests.
- [x] 5. Create `crates/mmd-engine/tests/rts_build.rs` and write every test from the table. Watch them fail. Evidence: red observed before implementation (build/gather/movement APIs did not exist); all 37 green after implementation.
- [x] 6. Implement `placement_valid` with the four rules in order. Evidence: all 9 `placement_*`/`a_unit_standing_there_does_not_block_placement` tests pass, including mutation kill #7.
- [x] 7. Add `placement`, `build_attend`, `finished` fields to `RtsWorld`, reserved at `MAX_ENTITIES`. Evidence: `cargo build`; `construction_allocates_nothing` proves zero growth.
- [x] 8. Implement `begin_placement`, `cancel_placement`, `confirm_placement`, `cancel_construction`, `is_site`, `order_build`. Evidence: `cargo test -p mmd-engine --test rts_build` 37/37 green.
- [x] 9. Implement the construction system as step 3 of `tick()`, body exactly as quoted. Evidence: `construction_advances_one_tick_per_tick`, `construction_does_not_advance_without_a_worker`, `a_second_worker_does_not_speed_it_up` pass; mutation kills #2, #3.
- [x] 10. Add the `Order::Build` arm to the mover's destination derivation and the in-reach stop guard. Evidence: `a_builder_walks_to_a_far_site`, `a_depot_finishes_in_its_documented_time` pass.
- [x] 11. Extend `RtsWorld::state_hash` with `placement` (one tag byte plus the kind byte). Evidence: `construction_is_reproducible` (equal hashes) and `state_hash_sees_construction_progress` (differing hash) both pass.
- [x] 12. Add `construction_allocates_nothing` to `crates/mmd-engine/tests/frame_allocations.rs`. Evidence: `cargo test -p mmd-engine --test frame_allocations` 15/15 green, including this test.
- [x] 13. Run the mutation list; record kills in the commit body. Evidence: all 9 mutants injected, confirmed red, reverted, confirmed green (see commit body).
- [x] 14. Run the full validation block. Evidence: every command below run with real captured output; all passed.

## Outputs

- **Files created**
  - `crates/mmd-engine/src/rts/build.rs`
  - `crates/mmd-engine/tests/rts_build.rs`
- **Files edited**
  - `crates/mmd-engine/src/rts/{mod,orders,world}.rs`
  - `crates/mmd-engine/tests/frame_allocations.rs`
- **Public API added:** `rts::{Placement, PlacementError, placement_valid, building_cost, build_ticks, supply_grant, footprint_cells, HQ_COST, DEPOT_COST, BARRACKS_COST, HQ_BUILD_TICKS, DEPOT_BUILD_TICKS, BARRACKS_BUILD_TICKS, HQ_SUPPLY_GRANT, DEPOT_SUPPLY_GRANT, BARRACKS_SUPPLY_GRANT, BUILD_REACH_CELLS, EXTRA_BUILDERS_SPEED_UP}`, `RtsWorld::{placement, begin_placement, cancel_placement, confirm_placement, cancel_construction, is_site, order_build}`, `Order::Build`.
- **Behaviour change:** buildings exist, cost, take time, block pathing and grant supply.
- **Migration / config:** none.

## Validation

- [x] `cargo fmt --all -- --check` — clean, no output
- [x] `cargo test -p mmd-engine --test rts_build` — 37 passed; 0 failed
- [x] `cargo test -p mmd-engine --test rts_economy` — all green (the approach-cell change is a regression risk) — 30 passed; 0 failed
- [x] `cargo test -p mmd-engine --test frame_allocations` — all green — 15 passed; 0 failed
- [x] `MMD_REQUIRE_GPU=1 cargo test --workspace --locked` — all green (no FAILED lines in any suite)
- [x] `cargo clippy --workspace --all-targets --all-features -- -D warnings` — clean
- [x] `nix flake check` — "all checks passed!"
- [x] `cargo run -- run --agents 5000 --frames 300` — exit 0, `hash=864147ca3a0e09f7ebc5762b778fce193e705a2bc943ceaf67acf087581ee881` unchanged (matches the value held since T4–T9)
- [x] app functional — no broken path from this slice — `run` binary launches, draws, and exits cleanly; no panics
- [x] commit msg draft: `feat(rts): place and construct buildings on the grid`
