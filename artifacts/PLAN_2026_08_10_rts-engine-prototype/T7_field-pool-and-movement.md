# T7: Field pool + unit movement

**Plan:** `./artifacts/PLAN_2026_08_10_rts-engine-prototype.md`
**Depends:** T6
**Commit outcome:** a move order routes units down a pooled flow field, they walk around obstacles, and they stop on arrival.

## Context (self-contained)

- Goal: phase 1 is a thin vertical slice of an RTS engine prototype (camera,
  selection, workers, economy, building, unit production) on a horde-free scene.
- This slice: how a player unit gets from A to B. The engine rule is flow fields,
  never per-entity pathfinding. A player issues orders to arbitrary destinations,
  so the answer is a **small preallocated pool of flow fields keyed by
  destination cell, LRU-evicted** — units sharing a destination share one field,
  which also gives group cohesion for free.
- Out of scope here: selection (T8), gathering (T9), building (T10), production
  (T11), rendering (T12), the CLI (T14). The horde `Simulation` in
  `crates/mmd-engine/src/sim/` is not touched — this is a separate mover for a
  separate entity store.
- Assumptions in force: no allocation in the movement sweep. A flow-field
  **miss** rebuilds into reused scratch and may grow that scratch once; that is
  the single documented, tested exception in this plan.

## Requirements

- `FlowField` can rebuild in place, reusing its own buffers and a caller-owned
  scratch heap.
- A `FieldPool` of 8 fields with exact LRU eviction and a hit counter.
- An order table keyed by entity slot, with `Idle` and `Move`.
- A movement system in `RtsWorld::tick` obeying the same admissibility rules the
  horde walk obeys, so a unit can never end up in a walkable-but-unreachable
  pocket.

## Inputs

- **Files to read**
  - `crates/mmd-engine/src/nav/flow_field.rs` — `FlowField`, `build`,
    `HeapEntry`, `derive_vectors`, `diagonal_clear`, `COST_OBSTACLE`,
    `COST_UNREACHABLE`, `CARDINAL_COST`, `DIAGONAL_COST`, `NEIGHBORS`.
  - `crates/mmd-engine/src/sim/tick.rs` — `step_admissible`, `nearest_cell`,
    `dir_from_vector`, `advance_frame`, `TICK_DT`, `SPEED_CELLS_PER_SEC`.
    These are the rules to mirror; do **not** edit that file.
  - `crates/mmd-engine/src/rts/world.rs`, `entity.rs` (from T6).
- **From Depends (T6) — spell out, the worker cannot read T6:**
  - `mmd_engine::rts` exports `EntityStore`, `EntityId { index: u32, generation: u32 }`,
    `EntityKind::{Unit(UnitKind), Building(BuildingKind), Node(ResourceKind)}`,
    `UnitKind::{Worker, Soldier}`, `BuildingKind::{Hq, Depot, Barracks}`,
    `ResourceKind::{Crystal, Gas}`, `MAX_ENTITIES = 2_048`,
    `OWNER_PLAYER = 0`, `OWNER_NEUTRAL = 255`.
  - `EntityStore` API: `len`, `slot_count`, `spawn(kind, owner, pos) -> Option<EntityId>`,
    `despawn`, `contains`, `slot(id) -> Option<usize>`, `id_at(slot)`,
    `alive(slot)`, `kind(slot)`, `owner(slot)`, `position(slot) -> [f32; 2]`,
    `dir(slot)`, `frame(slot)`, `progress(slot)`, `progress_target(slot)`,
    `amount(slot)`, `set_position`, `set_dir`, `set_frame`, `set_progress`,
    `set_amount`, `collect_live(&self, out: &mut Vec<usize>)`, `hash_into`.
    Slots are dense-ish indices; dead slots are reused LIFO.
  - `RtsWorld` has `scenario()`, `entities()`, `entities_mut()`, `resources()`,
    `supply()`, `tick_index()`, `start_hq()`, `tick()`, `state_hash()`.
    Its `tick()` today advances only the tick counter, and its doc comment
    already reserves this system order:
    *1. commands, 2. camera, 3. construction, 4. production, 5. orders,
    6. movement, 7. supply recount.*
  - `RtsWorld::from_scenario` seeds, in this exact order: the HQ (slot 0),
    then crystal nodes then gas nodes (slots 1..=10 on the tracked scene), then
    one worker per scenario spawn cell (slots 11..=16).
  - `testkit::RtsHarness::scene()` builds a harness over the tracked scene;
    `step_exact(n)`, `world()`, `world_mut()`, `state_hash()`, `ids_of_kind(kind)`.
  - The tracked scene is 320 × 320 cells, `cell_size_px: 4`, HQ min corner
    `(160, 160)` with a 12-cell footprint, six workers spawned at
    `(162..=167, 178)`.
- **Facts you must not rediscover** — from `sim/tick.rs`:
  - `pub const TICK_DT: f32 = 1.0 / 60.0;`
  - `nearest_cell(p) == p.floor() as i32` — cell centres sit at `n + 0.5`.
  - A step is admissible when its target cell is in bounds and unblocked **and**,
    if it crosses both cell boundaries at once, both shared cardinal neighbours
    are clear (`nav::flow_field::diagonal_clear`, which is `pub(crate)`).
  - `dir_from_vector(vx, vy)` maps a velocity to the 8-way sprite direction
    `0=E,1=NE,2=N,3=NW,4=W,5=SW,6=S,7=SE`. It is private to `sim::tick`; this
    ticket needs it, so **promote it** to `pub fn` in `sim/tick.rs` and re-export
    it as `mmd_engine::sim::dir_from_vector`. That is the only edit permitted to
    `sim/`.

## Exact design — no decisions left

### `crates/mmd-engine/src/nav/flow_field.rs` additions

```rust
/// Reusable working memory for [`FlowField::rebuild_in_place`].
///
/// Owning the heap outside the field is what lets a pool rebuild without
/// allocating: `BinaryHeap::clear` keeps capacity, so after the first rebuild at
/// a given grid size the heap never grows again in practice.
#[derive(Debug, Default)]
pub struct FieldScratch { heap: BinaryHeap<HeapEntry> }

impl FieldScratch {
    /// Reserve for a grid of `cells`. Mirrors `FlowField::build`'s own
    /// `n / 4 + 8` heuristic so a pool warms to the same shape.
    pub fn with_capacity(cells: usize) -> Self;
    /// Heap capacity, for the allocation-invariant test.
    pub fn capacity(&self) -> usize;
}

impl FlowField {
    /// Recompute this field for a new destination over `blocked`, reusing every
    /// buffer this field already owns plus `scratch`.
    ///
    /// `blocked` must be exactly `width * height` long — a caller-owned mask, so
    /// a building stamped into the world becomes an obstacle without rebuilding
    /// the obstacle set from the scenario every time.
    ///
    /// Produces bit-identical output to `FlowField::build` for the same inputs;
    /// that equivalence is the test that keeps the two from drifting.
    pub fn rebuild_in_place(
        &mut self,
        destination: Cell,
        blocked: &[bool],
        scratch: &mut FieldScratch,
    ) -> Result<(), FlowFieldError>;

    /// An empty field of the right shape, for preallocating a pool.
    ///
    /// Every cost is `COST_UNREACHABLE` and every vector is zero, so a slot that
    /// is somehow read before its first rebuild moves nobody rather than moving
    /// everybody to cell zero.
    pub fn blank(width: u32, height: u32) -> Result<Self, FlowFieldError>;
}
```

`rebuild_in_place` is `build`'s body with three changes: it writes into
`self.costs` / `self.vx` / `self.vy` via `fill` + indexed writes instead of
`vec![]`, it uses `scratch.heap` (cleared first) instead of a fresh
`BinaryHeap`, and it takes `blocked` as a parameter instead of deriving it from
an obstacle index list. `derive_vectors` is refactored to
`derive_vectors_into(width, height, &costs, blocked, &mut vx, &mut vy)`; the
existing `build` calls it too, so both paths share one implementation.

### New module `crates/mmd-engine/src/nav/field_pool.rs`

```rust
/// Flow fields held simultaneously. Eight is the working set an RTS actually
/// needs: a player rarely has more than a handful of distinct live
/// destinations, and each field costs `width * height * 12` bytes.
pub const NAV_FIELD_SLOTS: usize = 8;

/// Pool errors.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum FieldPoolError {
    #[error("flow field: {0:?}")]
    Field(FlowFieldError),
    #[error("blocked mask length {got}, expected {expected}")]
    MaskLength { got: usize, expected: usize },
}

/// A fixed set of flow fields keyed by destination cell, LRU-evicted.
#[derive(Debug)]
pub struct FieldPool { /* private */ }

impl FieldPool {
    /// Build a pool over a grid, with `obstacle_cells` as the initial mask.
    ///
    /// Every slot is allocated here — this is the only place the pool
    /// allocates for fields — and the scratch heap is reserved to the grid's
    /// size, so a later rebuild reuses both.
    pub fn new(width: u32, height: u32, obstacle_cells: &[u32]) -> Result<Self, FieldPoolError>;

    /// Slot holding a field to `dest`, rebuilding into the least-recently-used
    /// slot on a miss. Every call counts as a use, so a destination in constant
    /// use is never evicted.
    pub fn acquire(&mut self, dest: Cell) -> Result<u8, FieldPoolError>;

    /// Read a slot's field. Panics on an out-of-range slot — slots come from
    /// [`Self::acquire`] and a fabricated one is a caller bug.
    pub fn field(&self, slot: u8) -> &FlowField;

    /// The destination a slot currently holds, if any.
    pub fn key(&self, slot: u8) -> Option<Cell>;

    /// Rebuilds performed since construction. A test needs to see a hit not
    /// rebuild.
    pub fn rebuild_count(&self) -> u64;

    /// The blocked mask, `width * height` long, indexed `x + y * width`.
    pub fn blocked(&self) -> &[bool];

    /// Mark a cell blocked or free and invalidate **every** cached field.
    ///
    /// Invalidating all of them rather than the ones that "look affected" is
    /// deliberate: a single new obstacle can change the descent vector anywhere
    /// downstream of it, and a partial invalidation is a bug that only shows up
    /// as units walking into a wall built ten seconds ago.
    pub fn set_blocked(&mut self, cell: Cell, blocked: bool);

    /// Scratch heap capacity, for the allocation-invariant test.
    pub fn scratch_capacity(&self) -> usize;
}
```

LRU: `last_used: [u64; NAV_FIELD_SLOTS]`, a monotonic `clock: u64` bumped on
every `acquire`. A hit updates `last_used[slot]`. A miss picks
`argmin(last_used)`, ties broken by **lowest slot index**, rebuilds it and sets
its key. An empty slot has `last_used == 0` and therefore always wins eviction
before any used slot does.

Export from `crates/mmd-engine/src/nav/mod.rs`:
`pub mod field_pool;` and, from `flow_field`, `FieldScratch`.

### `crates/mmd-engine/src/rts/orders.rs`

```rust
/// Walk speed in cells per second, per unit kind.
///
/// The worker outruns the horde's 8.0 so a base can be re-tasked faster than it
/// can be walked across; the soldier matches the horde exactly, because a phase-2
/// fight between the two must not be decided by a speed nobody chose.
pub const WORKER_SPEED_CELLS_PER_SEC: f32 = 10.0;
pub const SOLDIER_SPEED_CELLS_PER_SEC: f32 = 8.0;
pub fn unit_speed(kind: UnitKind) -> f32;

/// How close a unit's centre must get to its destination cell's centre to be
/// finished, in cells.
///
/// Larger than the horde's `ARRIVAL_RADIUS` (0.5) because a *group* is sent to
/// one cell and only one of them can stand on it; the rest stop adjacent and the
/// order still completes. Arrival is checked against the destination cell, not
/// against a per-unit goal, so the whole group clears its order together.
pub const ARRIVAL_RADIUS_CELLS: f32 = 1.5;

/// What an entity is currently doing.
///
/// Discriminants are appended, never inserted — later tickets add `Gather` and
/// `Build` after `Move`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Order {
    Idle,
    /// Walk to `dest` down the field in `field_slot`.
    Move { dest: Cell, field_slot: u8 },
}

/// One order per entity slot, preallocated to `MAX_ENTITIES`.
#[derive(Debug, Clone)]
pub struct OrderTable { /* private: Vec<Order> */ }

impl OrderTable {
    pub fn new() -> Self;                        // MAX_ENTITIES entries of Idle
    pub fn get(&self, slot: usize) -> Order;
    pub fn set(&mut self, slot: usize, order: Order);
    pub fn clear(&mut self, slot: usize);        // -> Idle
    /// Feed every slot's order into the state hash, ascending.
    pub fn hash_into(&self, h: &mut sha2::Sha256, live: &[usize]);
}
```

`Order::tag() -> u8` (`0` Idle, `1` Move) for the hash.

### `RtsWorld` additions

```rust
impl RtsWorld {
    /// The navigation pool. Buildings stamp obstacles into it (T10).
    pub fn nav(&self) -> &FieldPool;

    /// Order one unit to walk to `dest`.
    ///
    /// Returns `false` when `id` is stale, is not a unit, is not owned by
    /// `OWNER_PLAYER`, or `dest` is out of bounds or blocked — a right-click on
    /// a rock must be a no-op, not an order nobody can finish.
    pub fn order_move(&mut self, id: EntityId, dest: Cell) -> bool;

    /// Order several units to one destination, acquiring the field **once**.
    ///
    /// This is the API the input layer uses. Issuing N single orders would
    /// acquire N times, and on a full pool that is N rebuilds of the same field.
    pub fn order_move_group(&mut self, ids: &[EntityId], dest: Cell) -> usize;

    /// The current order of a live entity.
    pub fn order_of(&self, id: EntityId) -> Option<Order>;
}
```

`RtsWorld` gains private fields `nav: FieldPool`, `orders: OrderTable`,
`live_scratch: Vec<usize>` (capacity `MAX_ENTITIES`, reused by every sweep).
`from_scenario` builds the pool from
`FieldPool::new(scenario.width(), scenario.height(), scenario.obstacle_cells())`.

### Movement system — step 6 of `tick()`

```
self.entities.collect_live(&mut self.live_scratch);
for &slot in &self.live_scratch {
    let EntityKind::Unit(kind) = self.entities.kind(slot) else { continue };
    let Order::Move { dest, field_slot } = self.orders.get(slot) else { continue };

    let p = self.entities.position(slot);
    // 1. Arrival, against the destination cell CENTRE.
    let dx = p[0] - (dest.x as f32 + 0.5);
    let dy = p[1] - (dest.y as f32 + 0.5);
    if dx * dx + dy * dy <= ARRIVAL_RADIUS_CELLS * ARRIVAL_RADIUS_CELLS {
        self.orders.clear(slot);
        continue;
    }

    // 2. Sample the field at the unit's own cell.
    let cx = p[0].floor() as i32;
    let cy = p[1].floor() as i32;
    if out of bounds { continue; }
    let (vx, vy) = self.nav.field(field_slot).vector_at(cx as u32, cy as u32);
    if vx == 0.0 && vy == 0.0 {
        // Unreachable or already on the destination cell: stop, do not spin.
        self.orders.clear(slot);
        continue;
    }

    // 3. Step, with the horde's admissibility rule.
    let step = unit_speed(kind) * TICK_DT;
    let nx = p[0] + vx * step;
    let ny = p[1] + vy * step;
    if step_admissible(cx, cy, nx, ny, width, height, self.nav.blocked()) {
        self.entities.set_position(slot, [nx, ny]);
    }
    self.entities.set_dir(slot, dir_from_vector(vx, vy));
    let f = (self.entities.frame(slot) + 1) % 4;
    self.entities.set_frame(slot, f);
}
```

`step_admissible` is duplicated into `rts/orders.rs` as
`pub(crate) fn step_admissible(...)` with the **identical** body to
`sim::tick::step_admissible`, including its `diagonal_clear` call. A test asserts
the two agree on a 5 × 5 exhaustive grid, so the duplication cannot drift.

`state_hash` gains the order table, hashed after the entity columns via
`OrderTable::hash_into(h, &live)`.

## TDD

1. **Red** — write every test below: field-pool and flow-field cases in a new
   `crates/mmd-engine/tests/nav_pool.rs`, movement cases appended to
   `crates/mmd-engine/tests/rts_world.rs`. Watch them fail.
2. **Green** — implement.
3. **Refactor** — extract `derive_vectors_into` so `build` and
   `rebuild_in_place` share it. Keep green.

## Test plan

| Test | Input | Expect |
| ---- | ----- | ------ |
| `rebuild_in_place_matches_build` | 40 × 40 grid, 30 random-but-seeded obstacle sets, 5 destinations each | `costs()` and every `vector_at` equal `FlowField::build`'s, exactly |
| `rebuild_in_place_rejects_a_wrong_mask` | mask of length `n - 1` | `Err(..)`, field untouched |
| `blank_field_moves_nobody` | `FlowField::blank(8, 8)` | every `vector_at` is `(0.0, 0.0)`, every `cost_at` is `COST_UNREACHABLE` |
| `pool_hit_does_not_rebuild` | acquire `(5,5)` twice | `rebuild_count() == 1`, same slot both times |
| `pool_holds_eight_distinct_destinations` | 8 distinct acquires | 8 distinct slots, `rebuild_count() == 8` |
| `pool_evicts_the_least_recently_used` | acquire A..H, re-acquire A, acquire I | I lands in B's slot; A's slot still keys A |
| `eviction_ties_prefer_the_lowest_slot` | fresh pool, acquire I on an all-empty pool | slot 0 |
| `pool_rejects_an_out_of_bounds_destination` | `Cell { x: 400, y: 0 }` on a 320 grid | `Err(FieldPoolError::Field(_))` |
| `pool_rejects_a_blocked_destination` | a known obstacle cell | `Err(FieldPoolError::Field(_))` |
| `set_blocked_invalidates_every_slot` | fill 8 slots, `set_blocked(c, true)`, re-acquire the first key | `rebuild_count()` increases |
| `set_blocked_changes_the_walk` | 8 × 8 open grid, field to `(7,7)`, then block the whole column `x == 4` except one gap | the descent path from `(0,0)` passes through the gap cell |
| `a_cached_acquire_allocates_nothing` | warm all 8 slots twice, record `scratch_capacity()`, acquire a cached key 100 times | `scratch_capacity()` unchanged, `rebuild_count()` unchanged |
| `a_miss_may_grow_scratch_only_once` | warm 8, then cycle 32 fresh destinations | `scratch_capacity()` after the first 8 equals the capacity after all 40 |
| `rts_step_admissible_agrees_with_the_sim` | exhaustive 5 × 5 grid, every obstacle subset of a 3 × 3 core, every 8-neighbour step | `rts::orders::step_admissible` equals `sim`'s for all cases |
| `order_move_sets_a_move_order` | worker id, `dest (200, 200)` | returns `true`, `order_of(id) == Some(Order::Move { dest, .. })` |
| `order_move_rejects_a_stale_id` | despawned id | `false` |
| `order_move_rejects_a_building` | the HQ id | `false` |
| `order_move_rejects_a_neutral_node` | a crystal node id | `false` |
| `order_move_rejects_a_blocked_destination` | a scenario obstacle cell | `false`, order stays `Idle` |
| `order_move_group_acquires_once` | 6 workers, one destination | `nav().rebuild_count()` increases by exactly 1; returns `6` |
| `a_unit_reaches_its_destination` | worker at `(165, 178)`, `dest (200, 200)`, `step_exact(1200)` | final position within `ARRIVAL_RADIUS_CELLS` of `(200.5, 200.5)`, order back to `Idle` |
| `a_unit_walks_around_an_obstacle` | 32 × 32 inline scene, wall `x == 16` with a gap at `y == 8`, unit at `(2, 20)`, dest `(30, 20)` | arrives, and at some tick its cell `y` was within 2 of the gap |
| `a_unit_never_enters_a_blocked_cell` | the walk above, sampled every tick | no sampled position's cell is blocked |
| `an_unreachable_destination_clears_the_order` | destination walled off completely | order returns to `Idle` within 2 ticks, unit did not move |
| `arrival_is_measured_from_the_cell_centre` | unit placed at exactly `dest + [1.4, 0.0]` | order clears on the first tick, position unchanged |
| `a_group_sharing_a_destination_shares_a_field` | 6 workers, one dest | all six `Order::Move` carry the same `field_slot` |
| `movement_is_reproducible` | two harnesses, same orders, 600 ticks | equal `state_hash()` |
| `state_hash_sees_an_order` | hash before vs after `order_move` | different |
| `an_idle_unit_does_not_move_or_animate` | no order, 600 ticks | position, `dir` and `frame` unchanged |
| `movement_allocates_nothing` | in `crates/mmd-engine/tests/frame_allocations.rs`, `MeasureGuard` around 600 ticks with 6 units under orders to cached destinations | zero allocations |

**Mutation verification (mandatory).** Inject, confirm red, revert, confirm green:
1. LRU picks the *most* recently used → kills `pool_evicts_the_least_recently_used`.
2. `set_blocked` invalidates only the touched cell's slot → kills `set_blocked_invalidates_every_slot`.
3. `rebuild_in_place` forgets to clear `scratch.heap` → kills `rebuild_in_place_matches_build`.
4. Drop the `diagonal_clear` arm from `rts::orders::step_admissible` → kills `rts_step_admissible_agrees_with_the_sim`.
5. Arrival measured from `dest.x as f32` instead of `+ 0.5` → kills `arrival_is_measured_from_the_cell_centre`.
6. `order_move_group` acquires per id → kills `order_move_group_acquires_once`.
7. Zero-vector case falls through instead of clearing → kills `an_unreachable_destination_clears_the_order` (it spins forever; the test must assert the order, not just the position).
8. `unit_speed` returns `SPEED_CELLS_PER_SEC` for both kinds → confirm a test distinguishes them; if none does, add `worker_outruns_soldier` (equal orders, 60 ticks, worker is strictly further along).

## Impl steps

- [x] 1. Promote `dir_from_vector` to `pub fn` in `crates/mmd-engine/src/sim/tick.rs` and add it to `crates/mmd-engine/src/sim/mod.rs`'s `pub use tick::{...}` list.
  - **Deviation (parent-authorised).** `sim::tick::step_admissible` was also
    promoted, to `pub(crate)` only. The ticket's mandatory test
    `rts_step_admissible_agrees_with_the_sim` cannot see a module-private fn
    from any test site, so mutation 4 would otherwise be unkillable. Visibility
    keyword only: signature, body and doc semantics untouched, and the phase-0
    gate hash is unchanged (see Validation).
- [x] 2. Add `FieldScratch` and `derive_vectors_into` to `crates/mmd-engine/src/nav/flow_field.rs`; rewrite `derive_vectors` as a thin caller.
- [x] 3. Add `FlowField::rebuild_in_place` and `FlowField::blank`.
  - **Plan defect (resolved).** `FlowFieldError` had no variant for a
    wrong-length mask, so `MaskLength { got, expected }` was added to it,
    matching `FieldPoolError::MaskLength`'s shape.
- [x] 4. Create `crates/mmd-engine/tests/nav_pool.rs` with the flow-field and pool tests. Watch them fail.
  - Red confirmed: `error[E0432]: unresolved import mmd_engine::nav::field_pool`.
- [x] 5. Create `crates/mmd-engine/src/nav/field_pool.rs` with `NAV_FIELD_SLOTS`, `FieldPoolError`, `FieldPool`.
- [x] 6. Implement `new`, `acquire` with exact LRU, `field`, `key`, `rebuild_count`, `blocked`, `set_blocked`, `scratch_capacity`.
- [x] 7. Add `pub mod field_pool;` to `crates/mmd-engine/src/nav/mod.rs`; export `FieldScratch` from `flow_field`.
  - `nav::flow_field` is already a `pub mod`, so `FieldScratch` is public at
    `nav::flow_field::FieldScratch` with no re-export line needed.
  - Steps 2–7 green: `cargo test -p mmd-engine --test nav_pool` → 13 passed.
- [x] 8. Create `crates/mmd-engine/src/rts/orders.rs` with the speed constants, `ARRIVAL_RADIUS_CELLS`, `Order`, `OrderTable`, `pub(crate) step_admissible`.
  - `rts_step_admissible_agrees_with_the_sim` lives in that file's unit-test
    module (36 864 cases) — it is the only site that can see both fns.
- [x] 9. Add `mod orders;` and the re-exports to `crates/mmd-engine/src/rts/mod.rs`.
- [x] 10. Add `nav`, `orders`, `live_scratch` fields to `RtsWorld`; build the pool in `from_scenario`.
- [x] 11. Add `order_move`, `order_move_group`, `order_of`, `nav` to `RtsWorld`.
- [x] 12. Implement the movement system as step 6 of `tick()`, body exactly as quoted.
- [x] 13. Extend `RtsWorld::state_hash` with the order table.
- [x] 14. Append the movement tests to `crates/mmd-engine/tests/rts_world.rs`.
  - **Plan defect (resolved).** `a_unit_walks_around_an_obstacle` /
    `a_unit_never_enters_a_blocked_cell` cannot use a "32 x 32 inline scene":
    `scenario::validate_rts_scene_dims` locks the `rts_prototype_v1` family to
    320 x 320 and the rts block is required exactly on that family, so a 32 x 32
    RTS scenario cannot validate. Built at 320 x 320 with the ticket's literal
    geometry — wall `x == 16` with the gap at `y == 8`, unit at `(2, 20)`,
    destination `(30, 20)` — unscaled, so each case proves what it was written
    to prove.
  - Green: `cargo test -p mmd-engine --test rts_world` → 48 passed.
- [x] 15. Add `movement_allocates_nothing` to `crates/mmd-engine/tests/frame_allocations.rs`.
  - Green: `cargo test -p mmd-engine --test frame_allocations` → 12 passed.
- [x] 16. Run the mutation list; record kills in the commit body.
  - Killed 1, 2, 4, 5, 6, 7, 8 (inject → red, revert → green, real `cargo test`
    output captured).
  - **Plan defect — mutation 3 is an equivalent mutant.** Deleting
    `heap.clear()` from `rebuild_in_place` changes nothing observable:
    `while let Some(..) = heap.pop()` drains the heap, and every rejection
    returns before the first push, so the scratch is always empty on entry.
    `rebuild_in_place_matches_build` passes with the line deleted (real output
    captured). The clear is kept as a documented defensive restore of that
    invariant.
  - **Plan defect — mutation 6 was unkillable as specified.** `acquire` is
    idempotent for a cached key, so acquiring once per unit and once per group
    give the *same* `rebuild_count` (1 miss + 5 hits). Fixed by adding
    `FieldPool::acquire_count()` (the LRU clock, already maintained) and
    asserting it in `order_move_group_acquires_once`; the mutation then fails
    with `left: 6, right: 1`.
- [x] 17. Run the full validation block.

## Outputs

- **Files created**
  - `crates/mmd-engine/src/nav/field_pool.rs`
  - `crates/mmd-engine/src/rts/orders.rs`
  - `crates/mmd-engine/tests/nav_pool.rs`
- **Files edited**
  - `crates/mmd-engine/src/nav/flow_field.rs`, `crates/mmd-engine/src/nav/mod.rs`
  - `crates/mmd-engine/src/sim/tick.rs` (visibility of `dir_from_vector` only), `crates/mmd-engine/src/sim/mod.rs`
  - `crates/mmd-engine/src/rts/{mod,world}.rs`
  - `crates/mmd-engine/tests/rts_world.rs`, `crates/mmd-engine/tests/frame_allocations.rs`
- **Public API added:** `nav::flow_field::{FieldScratch, FlowField::rebuild_in_place, FlowField::blank}`, `nav::field_pool::{NAV_FIELD_SLOTS, FieldPool, FieldPoolError}`, `rts::{Order, OrderTable, ARRIVAL_RADIUS_CELLS, WORKER_SPEED_CELLS_PER_SEC, SOLDIER_SPEED_CELLS_PER_SEC, unit_speed}`, `RtsWorld::{nav, order_move, order_move_group, order_of}`, `sim::dir_from_vector`.
- **Behaviour change:** RTS units move. The horde is untouched — `sim/tick.rs` changes only a `pub` keyword.
- **Migration / config:** none.

## Validation

- [x] `cargo fmt --all -- --check` — clean
- [x] `cargo test -p mmd-engine --test nav_pool` — 13 passed
- [x] `cargo test -p mmd-engine --test rts_world` — 48 passed
- [x] `cargo test -p mmd-engine --test frame_allocations` — 12 passed
- [x] `MMD_REQUIRE_GPU=1 cargo test --workspace --locked` — every binary `0 failed`;
  `golden_frame_matches` passed without regeneration (`MMD_UPDATE_GOLDEN` unset)
- [x] `cargo clippy --workspace --all-targets --all-features -- -D warnings` — clean
- [x] `nix flake check` — `all checks passed!`
- [x] `cargo run -- run --agents 5000 --frames 300` — exit 0, exit-line
  `hash=864147ca3a0e09f7ebc5762b778fce193e705a2bc943ceaf67acf087581ee881`,
  unchanged from T4/T5/T6 (proof the horde walk did not move)
- [x] `cargo run -- run --scenario assets/scenarios/collision_mid_v1.ron --frames 300` — exit 0,
  `hash=3df604770021490eb416b38bcd9bc4a25bb417638010c458d5563a17578d268a`
- [x] app functional — both scenes render and exit cleanly; `xtask bootstrap/shaders/atlases --check` all ok;
  `git diff --stat main` over `lab/goldens/`, `assets/scenarios/fixtures/` and the tracked atlases is empty
- [x] commit msg draft: `feat(rts): move units on pooled flow fields`
