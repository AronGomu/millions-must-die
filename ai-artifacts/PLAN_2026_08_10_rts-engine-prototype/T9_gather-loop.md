# T9: Gather loop, two resources

**Plan:** `./ai-artifacts/PLAN_2026_08_10_rts-engine-prototype.md`
**Depends:** T7
**Commit outcome:** a worker ordered onto a node walks there, mines a load, hauls it to the HQ, banks it, and repeats until the node is empty.

## Context (self-contained)

- Goal: phase 1 is a thin vertical slice of an RTS engine prototype (camera,
  selection, workers, economy, building, unit production) on a horde-free scene.
- This slice: the economy's beating heart — the worker round trip. Two
  resources, Crystal and Gas, both harvested by the same worker with the same
  loop; the only difference is which counter goes up and how much a node holds.
- Out of scope here: selection (T8 provides it), building placement (T10),
  production (T11), rendering (T12), the CLI (T14). No combat, no worker death.
- Assumptions in force: the round trip must be allocation-free and reproducible.
  A worker's whole state lives in its order plus two carry columns, so a state
  hash pins the loop exactly.

## Requirements

- Two new columns on `EntityStore`: what a unit is carrying and how much.
- A `Gather` order with an explicit three-state machine, hashed like everything
  else.
- Node depletion: a node's `amount` falls, reaches zero, and stops paying out.
- Drop-off resolution: the nearest live player building that accepts cargo.
- The order survives the round trip; only a depleted node or an
  unreachable/destroyed drop-off ends it.

## Inputs

- **Files to read**
  - `crates/mmd-engine/src/rts/{entity,economy,orders,world}.rs`.
  - `crates/mmd-engine/src/nav/field_pool.rs`.
- **From Depends (T7) — spell out, the worker cannot read T7:**
  - `mmd_engine::rts` exports `EntityStore`, `EntityId { index, generation }`,
    `EntityKind::{Unit(UnitKind), Building(BuildingKind), Node(ResourceKind)}`,
    `UnitKind::{Worker, Soldier}`, `BuildingKind::{Hq, Depot, Barracks}`,
    `ResourceKind::{Crystal, Gas}`, `Resources { crystal, gas }` with
    `covers` / `try_debit` / `credit`, `Supply`, `MAX_ENTITIES = 2_048`,
    `OWNER_PLAYER = 0`, `OWNER_NEUTRAL = 255`,
    `NODE_CRYSTAL_AMOUNT = 1_500`, `NODE_GAS_AMOUNT = 2_500`.
  - `EntityStore` columns: `alive`, `kind`, `owner`, `position` (cell space),
    `dir`, `frame`, `progress`, `progress_target`, `amount`, plus
    `set_position` / `set_dir` / `set_frame` / `set_progress` / `set_amount`,
    `collect_live(&self, out: &mut Vec<usize>)`, `slot(id)`, `id_at(slot)`,
    `contains(id)`, `spawn`, `despawn`, and a `#[cfg(feature = "testkit")]
    column_capacities()` returning one entry per column.
  - `BuildingKind::footprint_cells()` = `12` (Hq), `8` (Depot), `10` (Barracks);
    `BuildingKind::is_drop_off()` is true only for `Hq`. A building's
    `position` is its footprint **centre** in cell space.
  - Orders live in `rts::orders`:
    ```rust
    pub enum Order { Idle, Move { dest: Cell, field_slot: u8 } }
    pub struct OrderTable { /* MAX_ENTITIES entries */ }
    // get / set / clear / hash_into
    pub const ARRIVAL_RADIUS_CELLS: f32 = 1.5;
    pub const WORKER_SPEED_CELLS_PER_SEC: f32 = 10.0;
    pub const SOLDIER_SPEED_CELLS_PER_SEC: f32 = 8.0;
    pub fn unit_speed(kind: UnitKind) -> f32;
    pub(crate) fn step_admissible(cx: i32, cy: i32, nx: f32, ny: f32, width: u32, height: u32, blocked: &[bool]) -> bool;
    ```
  - `nav::field_pool::{FieldPool, FieldPoolError, NAV_FIELD_SLOTS}` with
    `acquire(dest) -> Result<u8, FieldPoolError>`, `field(slot)`, `key(slot)`,
    `rebuild_count()`, `blocked()`, `set_blocked(cell, bool)`,
    `scratch_capacity()`. `NAV_FIELD_SLOTS == 8`, LRU eviction, lowest slot wins
    a tie.
  - `RtsWorld` has `scenario()`, `entities()`, `entities_mut()`, `resources()`,
    `supply()`, `tick_index()`, `start_hq()`, `nav()`, `order_move(id, dest)`,
    `order_move_group(ids, dest)`, `order_of(id)`, `tick()`, `state_hash()`.
    Private fields include `nav: FieldPool`, `orders: OrderTable`,
    `live_scratch: Vec<usize>`.
  - `tick()`'s reserved system order is: *1. commands, 2. camera,
    3. construction, 4. production, 5. orders, 6. movement, 7. supply recount*,
    then selection self-heal if T8 has landed. The movement system today handles
    `Order::Move` only, samples the field at the unit's own cell, steps at
    `unit_speed(kind) * TICK_DT`, applies `step_admissible`, sets `dir` from
    `sim::dir_from_vector(vx, vy)` and advances `frame` modulo 4, and clears the
    order on arrival within `ARRIVAL_RADIUS_CELLS` of the destination cell centre.
  - `testkit::RtsHarness::scene()` runs the tracked 320 × 320 scene: HQ min
    corner `(160, 160)` edge 12 (centre `[166.0, 166.0]`), crystal nodes at
    `(140,150) (146,146) (152,142) (180,142) (186,146) (192,150) (150,190) (182,190)`,
    gas nodes at `(136,168) (196,168)`, six workers at `(162..=167, 178)`,
    starting stock `crystal 300 / gas 100`, supply cap `10`, used `6`.
  - `TICK_DT` is `1.0 / 60.0`.

## Exact design — no decisions left

### Constants — `crates/mmd-engine/src/rts/economy.rs`

```rust
/// Units of resource a worker carries per trip.
pub const WORKER_CARRY_CAPACITY: u32 = 8;

/// Ticks a worker spends mining before its load is full. One second at 60 Hz.
///
/// The whole round trip is therefore mine-time plus walk-time, and walk-time is
/// what a player shortens by putting a drop-off closer — which is the macro
/// decision the economy exists to pose.
pub const GATHER_TICKS: u32 = 60;

/// How close a worker's centre must come to a node's centre to start mining.
pub const GATHER_REACH_CELLS: f32 = 2.0;

/// How close a worker's centre must come to a drop-off building's **footprint
/// rectangle** to bank its load.
///
/// Measured to the rectangle, not to the centre: an HQ is 12 cells across, and
/// a centre-distance rule would make a worker walk into the middle of its own
/// base to deliver.
pub const DROP_OFF_REACH_CELLS: f32 = 1.0;

/// Starting amount of each node kind, by kind.
pub fn node_amount(kind: ResourceKind) -> u32;
```

### New columns — `crates/mmd-engine/src/rts/entity.rs`

```rust
/// Sentinel in the `carry_kind` column meaning "carrying nothing".
///
/// A separate byte rather than `Option<ResourceKind>` so the column stays a
/// plain `Vec<u8>` the state hash can feed in one `update`.
pub const CARRY_NONE: u8 = 0xFF;

impl EntityStore {
    /// What this unit is carrying, and how much. `None` when empty-handed.
    pub fn carry(&self, slot: usize) -> Option<(ResourceKind, u32)>;
    /// Set the carried cargo. `None` clears both columns.
    pub fn set_carry(&mut self, slot: usize, cargo: Option<(ResourceKind, u32)>);
}
```

Two new columns, `carry_kind: Vec<u8>` (initialised `CARRY_NONE`) and
`carry_amount: Vec<u32>`, both `Vec::with_capacity(MAX_ENTITIES)` and both fed
into `hash_into` after `amount`. `column_capacities()` grows from 11 entries to
13; update its test.

### The order — `crates/mmd-engine/src/rts/orders.rs`

```rust
/// Where a gathering worker is in its round trip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GatherPhase {
    /// Walking to the node.
    ToNode { field_slot: u8 },
    /// Standing at the node, filling up. `ticks_left` counts down to zero.
    Mining { ticks_left: u32 },
    /// Walking back to `drop_off` with a full load.
    Returning { drop_off: EntityId, field_slot: u8 },
}

pub enum Order {
    Idle,
    Move { dest: Cell, field_slot: u8 },
    /// Mine `node` and haul to the nearest drop-off, forever.
    Gather { node: EntityId, phase: GatherPhase },
}
```

`Order::tag()` gains `2` for `Gather`; `GatherPhase` contributes a second byte
(`0` ToNode, `1` Mining, `2` Returning) plus `ticks_left` to `hash_into`.

### `RtsWorld` API

```rust
impl RtsWorld {
    /// Order one worker to gather from `node`.
    ///
    /// `false` when: `id` is stale, is not a `UnitKind::Worker`, is not
    /// `OWNER_PLAYER`; `node` is stale or is not an `EntityKind::Node`; the node
    /// is already empty; or no field can be built to the node's cell.
    /// A Soldier cannot gather — refusing is what makes the HUD's "no valid
    /// order" state real rather than cosmetic.
    pub fn order_gather(&mut self, id: EntityId, node: EntityId) -> bool;

    /// Order several workers onto one node, acquiring the field once.
    pub fn order_gather_group(&mut self, ids: &[EntityId], node: EntityId) -> usize;

    /// The nearest live drop-off building owned by the player, by distance from
    /// `pos` to its footprint rectangle. Ties go to the lower entity slot.
    pub fn nearest_drop_off(&self, pos: [f32; 2]) -> Option<EntityId>;
}
```

### Gather system — inside step 5 ("orders") of `tick()`

Runs **before** movement, so a phase change takes effect on the same tick.

```
for &slot in &live_scratch {
    let Order::Gather { node, phase } = orders.get(slot) else { continue };

    // The node may have been removed; the order dies with it.
    let Some(node_slot) = entities.slot(node) else { orders.clear(slot); continue };
    let EntityKind::Node(res) = entities.kind(node_slot) else { orders.clear(slot); continue };
    let node_pos = entities.position(node_slot);
    let p = entities.position(slot);

    match phase {
        GatherPhase::ToNode { .. } => {
            if entities.amount(node_slot) == 0 { orders.clear(slot); continue; }
            if dist2(p, node_pos) <= GATHER_REACH_CELLS * GATHER_REACH_CELLS {
                orders.set(slot, Order::Gather { node, phase: GatherPhase::Mining { ticks_left: GATHER_TICKS } });
            }
            // else: leave it; the movement system walks it down field_slot toward the node cell.
        }
        GatherPhase::Mining { ticks_left } => {
            if ticks_left > 1 {
                orders.set(slot, Order::Gather { node, phase: GatherPhase::Mining { ticks_left: ticks_left - 1 } });
            } else {
                // Load up: take min(capacity, remaining).
                let take = WORKER_CARRY_CAPACITY.min(entities.amount(node_slot));
                if take == 0 { orders.clear(slot); continue; }
                entities.set_amount(node_slot, entities.amount(node_slot) - take);
                entities.set_carry(slot, Some((res, take)));
                match self.nearest_drop_off(p) {
                    Some(d) => {
                        let cell = drop_off_approach_cell(entities, d);
                        match nav.acquire(cell) {
                            Ok(fs) => orders.set(slot, Order::Gather { node, phase: GatherPhase::Returning { drop_off: d, field_slot: fs } }),
                            Err(_) => orders.clear(slot),
                        }
                    }
                    None => orders.clear(slot),   // nowhere to deliver: stop, holding the cargo
                }
            }
        }
        GatherPhase::Returning { drop_off, field_slot: _ } => {
            let Some(d_slot) = entities.slot(drop_off) else { orders.clear(slot); continue };
            let EntityKind::Building(b) = entities.kind(d_slot) else { orders.clear(slot); continue };
            if rect_distance(p, entities.position(d_slot), b.footprint_cells()) <= DROP_OFF_REACH_CELLS {
                if let Some((kind, amount)) = entities.carry(slot) {
                    match kind {
                        ResourceKind::Crystal => resources.credit(Resources { crystal: amount, gas: 0 }),
                        ResourceKind::Gas => resources.credit(Resources { crystal: 0, gas: amount }),
                    }
                    entities.set_carry(slot, None);
                }
                if entities.amount(node_slot) == 0 { orders.clear(slot); continue; }
                match nav.acquire(node_cell(node_pos)) {
                    Ok(fs) => orders.set(slot, Order::Gather { node, phase: GatherPhase::ToNode { field_slot: fs } }),
                    Err(_) => orders.clear(slot),
                }
            }
        }
    }
}
```

with helpers in `rts/orders.rs`:

```rust
/// Squared cell-space distance.
pub(crate) fn dist2(a: [f32; 2], b: [f32; 2]) -> f32;

/// Distance from a point to a footprint rectangle, `0.0` when inside.
pub(crate) fn rect_distance(p: [f32; 2], center: [f32; 2], edge: u32) -> f32;

/// The cell a worker should be routed to when heading for a building.
///
/// The footprint's **centre cell**. The flow field's destination must be
/// unblocked, and T10 stamps a finished building's footprint as blocked — so
/// this returns the centre cell *before* T10 lands and T10 changes it to the
/// nearest free cell adjacent to the footprint. Recorded here so the change is a
/// deliberate edit, not a surprise.
pub(crate) fn drop_off_approach_cell(store: &EntityStore, id: EntityId) -> Cell;

/// The cell a node occupies.
pub(crate) fn node_cell(pos: [f32; 2]) -> Cell;
```

### Movement system change

`Order::Gather` now also drives movement. Extend step 6's match to derive a
`(dest, field_slot)` from the order:

```rust
let (dest, field_slot) = match orders.get(slot) {
    Order::Move { dest, field_slot } => (dest, field_slot),
    Order::Gather { node, phase: GatherPhase::ToNode { field_slot } } =>
        (node_cell(entities.position(entities.slot(node)?)), field_slot),
    Order::Gather { node: _, phase: GatherPhase::Returning { drop_off, field_slot } } =>
        (drop_off_approach_cell(entities, drop_off), field_slot),
    _ => continue,           // Idle, and Mining (a mining worker stands still)
};
```

**Arrival clears the order only for `Order::Move`.** A gathering worker that
reaches its destination is handled by the gather system's reach tests, not by
the mover — otherwise the round trip would cancel itself on arrival.

## TDD

1. **Red** — write every test below in a new
   `crates/mmd-engine/tests/rts_economy.rs`. Watch them fail.
2. **Green** — implement.
3. **Refactor** — none expected. Keep green.

## Test plan

Harness is `RtsHarness::scene()` unless stated. `w0` = the first worker
(entity slot 11), `n_crystal` = the crystal node at `(140, 150)`,
`n_gas` = the gas node at `(136, 168)`, `hq` = `world().start_hq().unwrap()`.

| Test | Input | Expect |
| ---- | ----- | ------ |
| `carry_starts_empty` | fresh world | `carry(slot) == None` for every unit |
| `set_carry_round_trips` | `set_carry(11, Some((Gas, 5)))` then read | `Some((Gas, 5))`; `set_carry(11, None)` → `None` |
| `carry_columns_are_reserved` | 512 spawns | `column_capacities()` still `MAX_ENTITIES` on all 13 |
| `order_gather_accepts_a_worker_and_a_node` | `order_gather(w0, n_crystal)` | `true`, order is `Gather { node: n_crystal, phase: ToNode { .. } }` |
| `order_gather_rejects_a_soldier` | spawn a Soldier, order it | `false` |
| `order_gather_rejects_a_building_as_the_node` | `order_gather(w0, hq)` | `false` |
| `order_gather_rejects_an_empty_node` | `set_amount(node_slot, 0)` then order | `false` |
| `order_gather_rejects_a_stale_worker` | despawned id | `false` |
| `order_gather_group_acquires_once` | 6 workers, one node | `nav().rebuild_count()` up by exactly 1, returns `6` |
| `a_worker_walks_to_its_node` | `order_gather(w0, n_crystal)`, `step_exact(400)` | at some tick the phase became `Mining` |
| `mining_takes_the_documented_time` | place `w0` at the node, order, step 1 tick to enter `Mining`, then step `GATHER_TICKS` | `carry(w0) == Some((Crystal, 8))` and the phase is `Returning` |
| `a_mining_worker_does_not_move` | as above, sample position each tick during `Mining` | unchanged |
| `the_node_loses_exactly_the_carried_amount` | as above | node `amount` fell from `1500` to `1492` |
| `a_partial_node_pays_out_what_is_left` | `set_amount(node_slot, 3)`, mine once | `carry == Some((Crystal, 3))`, node `amount == 0` |
| `an_emptied_node_ends_the_order` | node at `3`, run a full trip, then one more approach | after banking, the order is `Idle` |
| `a_full_round_trip_banks_crystal` | `order_gather(w0, n_crystal)`, `step_exact(3000)` | `resources().crystal > 300`, `resources().gas == 100` |
| `a_full_round_trip_banks_gas` | `order_gather(w0, n_gas)`, `step_exact(3000)` | `resources().gas > 100`, `resources().crystal == 300` |
| `the_worker_keeps_cycling` | `step_exact(6000)` on one worker | at least 3 deliveries (`crystal >= 300 + 24`) |
| `cargo_is_cleared_on_delivery` | sample `carry` right after a bank | `None` |
| `delivery_uses_the_footprint_not_the_centre` | worker parked exactly `DROP_OFF_REACH_CELLS` outside the HQ's footprint edge, carrying 8 | banks on the next tick without moving |
| `delivery_one_cell_further_does_not_fire` | worker at `DROP_OFF_REACH_CELLS + 0.1` outside | does not bank |
| `nearest_drop_off_prefers_the_closer_building` | spawn a second `Hq`-kind drop-off nearer the worker | `nearest_drop_off` returns it |
| `nearest_drop_off_ignores_non_drop_off_buildings` | spawn a `Barracks` right next to the worker | returns the HQ |
| `nearest_drop_off_ties_go_to_the_lower_slot` | two drop-offs at equal distance | the lower `index` |
| `no_drop_off_stops_the_worker_holding_cargo` | despawn the HQ mid-trip | order becomes `Idle`, `carry` still `Some(..)` |
| `six_workers_on_one_node_all_deliver` | `order_gather_group(all six, n_crystal)`, `step_exact(4000)` | `resources().crystal >= 300 + 6 * 8` |
| `the_economy_is_reproducible` | two harnesses, same orders, 4000 ticks | equal `state_hash()` |
| `state_hash_sees_the_carried_load` | `set_carry(11, Some((Gas, 1)))` | hash differs |
| `state_hash_sees_the_gather_phase` | flip `Mining { ticks_left }` by 1 | hash differs |
| `the_gather_loop_allocates_nothing` | in `frame_allocations.rs`, `MeasureGuard` around 2 000 ticks of six workers on two nodes, with all fields warm | zero allocations |

**Mutation verification (mandatory).** Inject, confirm red, revert, confirm green:
1. `WORKER_CARRY_CAPACITY = 9` → kills `the_node_loses_exactly_the_carried_amount`.
2. `take = WORKER_CARRY_CAPACITY` without the `min` → kills `a_partial_node_pays_out_what_is_left` (node underflows / over-credits).
3. Bank without clearing `carry` → kills `cargo_is_cleared_on_delivery` **and** should double-credit; confirm `a_full_round_trip_banks_crystal` also reddens.
4. `rect_distance` measured centre-to-centre → kills `delivery_uses_the_footprint_not_the_centre`.
5. `Mining` decrements by 2 → kills `mining_takes_the_documented_time`.
6. Arrival in the mover clears a `Gather` order → kills `the_worker_keeps_cycling`.
7. `nearest_drop_off` ignores `is_drop_off()` → kills `nearest_drop_off_ignores_non_drop_off_buildings`.
8. Gather system runs *after* movement → confirm a test notices; if none does, add `a_worker_that_arrives_starts_mining_the_same_tick`.

## Impl steps

- [x] 1. Add `CARRY_NONE`, the two carry columns, `carry`, `set_carry` to `crates/mmd-engine/src/rts/entity.rs`; extend `hash_into` and `column_capacities`. Evidence: `cargo test -p mmd-engine --test rts_world` green (`every_column_is_reserved_at_construction`), `cargo test -p mmd-engine --test rts_economy` green (`carry_starts_empty`, `set_carry_round_trips`, `carry_columns_are_reserved`).
- [x] 2. Add the constants and `node_amount` to `crates/mmd-engine/src/rts/economy.rs`; make `RtsWorld::from_scenario` seed node amounts through `node_amount` instead of the two literals. Evidence: `cargo test -p mmd-engine --test rts_world` green (`nodes_carry_their_starting_amount`).
- [x] 3. Add `GatherPhase` and the `Order::Gather` variant to `crates/mmd-engine/src/rts/orders.rs`; extend `Order::tag` and `OrderTable::hash_into`. Evidence: `cargo test -p mmd-engine --test rts_economy` green (`state_hash_sees_the_gather_phase`).
- [x] 4. Add `dist2`, `rect_distance`, `drop_off_approach_cell`, `node_cell` to `orders.rs`. Evidence: builds and used by `order_gather`/gather system/movement; `cargo build -p mmd-engine --lib` clean.
- [x] 5. Create `crates/mmd-engine/tests/rts_economy.rs` and write every test from the table. Watch them fail. Evidence: 30 tests written (plus mutation-list test #8); observed initial red against pre-impl code (E0432/E0433 unresolved imports for `GatherPhase`/`GATHER_TICKS`/`WORKER_CARRY_CAPACITY`/`order_gather`), then green after impl — `cargo test -p mmd-engine --test rts_economy`: 30 passed.
- [x] 6. Implement `RtsWorld::nearest_drop_off`. Evidence: `nearest_drop_off_prefers_the_closer_building`, `nearest_drop_off_ignores_non_drop_off_buildings`, `nearest_drop_off_ties_go_to_the_lower_slot` pass.
- [x] 7. Implement `RtsWorld::order_gather` and `order_gather_group`. Evidence: `order_gather_*` and `order_gather_group_acquires_once` tests pass.
- [x] 8. Implement the gather system as step 5 of `tick()`, body exactly as quoted. Evidence: full round-trip tests (`a_full_round_trip_banks_crystal`, `a_full_round_trip_banks_gas`, `the_worker_keeps_cycling`, `six_workers_on_one_node_all_deliver`) pass.
- [x] 9. Extend the movement system's destination derivation to cover both `Gather` phases, and restrict arrival-clearing to `Order::Move`. Evidence: `a_worker_walks_to_its_node`, `a_mining_worker_does_not_move`, `a_worker_that_arrives_starts_mining_the_same_tick` pass; `cargo test -p mmd-engine --test rts_world` still 48/48 green (no Move-order regression).
- [x] 10. Extend `RtsWorld::state_hash` — no change needed; confirmed by running `state_hash_sees_the_carried_load`. Evidence: test passes without touching `state_hash` (it already delegates to `EntityStore::hash_into`/`OrderTable::hash_into`); doc comment updated to name the new columns.
- [x] 11. Add `the_gather_loop_allocates_nothing` to `crates/mmd-engine/tests/frame_allocations.rs`. Evidence: `cargo test -p mmd-engine --test frame_allocations`: 14 passed, including the new test.
- [x] 12. Update `rts::mod`'s re-exports with `GatherPhase`, `WORKER_CARRY_CAPACITY`, `GATHER_TICKS`, `GATHER_REACH_CELLS`, `DROP_OFF_REACH_CELLS`, `CARRY_NONE`, `node_amount`. Evidence: `crates/mmd-engine/src/rts/mod.rs` re-exports all seven; `cargo build -p mmd-engine --lib` clean.
- [x] 13. Run the mutation list; record kills in the commit body. Evidence: 7/8 confirmed inject→red→revert→green; #6 (arrival clears a Gather order) proven a genuine equivalent mutant under the current constants (`GATHER_REACH_CELLS=2.0 > ARRIVAL_RADIUS_CELLS=1.5`; HQ footprint half-width 6 cells `>>` `ARRIVAL_RADIUS_CELLS`), recorded in the commit body.
- [ ] 14. Run the full validation block.

## Outputs

- **Files created**
  - `crates/mmd-engine/tests/rts_economy.rs`
- **Files edited**
  - `crates/mmd-engine/src/rts/{entity,economy,orders,world,mod}.rs`
  - `crates/mmd-engine/tests/rts_world.rs` (the `column_capacities` count)
  - `crates/mmd-engine/tests/frame_allocations.rs`
- **Public API added:** `rts::{GatherPhase, CARRY_NONE, WORKER_CARRY_CAPACITY, GATHER_TICKS, GATHER_REACH_CELLS, DROP_OFF_REACH_CELLS, node_amount}`, `EntityStore::{carry, set_carry}`, `RtsWorld::{order_gather, order_gather_group, nearest_drop_off}`, `Order::Gather`.
- **Behaviour change:** workers gather. No existing command changes.
- **Migration / config:** none.

## Validation

- [x] `cargo fmt --all -- --check` — clean after `cargo fmt --all` (test files + `mod.rs`/`world.rs` import wrapping)
- [x] `cargo test -p mmd-engine --test rts_economy` — all green (30 passed)
- [x] `cargo test -p mmd-engine --test rts_world` — all green (48 passed)
- [x] `cargo test -p mmd-engine --test frame_allocations` — all green (14 passed)
- [x] `MMD_REQUIRE_GPU=1 cargo test --workspace --locked` — exit 0, every `test result: ok` block passed
- [x] `cargo clippy --workspace --all-targets --all-features -- -D warnings` — clean, exit 0
- [x] `nix flake check` — "all checks passed!"
- [x] `cargo run -- run --agents 5000 --frames 300` — exit 0, `hash=864147ca3a0e09f7ebc5762b778fce193e705a2bc943ceaf67acf087581ee881` (unchanged, matches the pinned T4–T8 value)
- [x] app functional — no broken path from this slice (CLI run above completes clean exit; full workspace suite green)
- [x] commit msg draft: `feat(rts): gather crystal and gas with worker hauling`
