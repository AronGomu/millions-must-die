# T11: Production + supply

**Plan:** `./artifacts/PLAN_2026_08_10_rts-engine-prototype.md`
**Depends:** T10
**Commit outcome:** the HQ produces Workers and a finished Barracks produces Soldiers, both bounded by a supply cap that queueing cannot cheat, with a rally point.

## Context (self-contained)

- Goal: phase 1 is a thin vertical slice of an RTS engine prototype (camera,
  selection, workers, economy, building, unit production) on a horde-free scene.
- This slice: the last gameplay system. A building holds a small production
  queue; each entry costs resources and reserves supply at **enqueue** time;
  finished units appear beside the building and walk to its rally point.
- Out of scope here: combat (the Soldier has no weapon — that is phase 2),
  rendering (T12), the HUD (T13), the CLI (T14).
- Assumptions in force: the 500-population cap is a stated design pillar. Supply
  is what makes it a mechanic; `scenario::MAX_SUPPLY_CAP = 500` is its ceiling.

## Requirements

- A bounded production queue per building, hashed like everything else.
- Resources debited and supply **reserved** at enqueue; both refunded on cancel.
- Only legal building → unit pairs are producible.
- `Supply::used` is **recomputed** every tick from live units plus reservations,
  never incrementally maintained, so it cannot drift.
- Rally points, defaulting to none.

## Inputs

- **Files to read**
  - `crates/mmd-engine/src/rts/{entity,economy,orders,build,world}.rs`.
- **From Depends (T10) — spell out, the worker cannot read T10:**
  - `mmd_engine::rts` exports `EntityStore`, `EntityId { index, generation }`,
    `EntityKind::{Unit(UnitKind), Building(BuildingKind), Node(ResourceKind)}`,
    `UnitKind::{Worker, Soldier}`, `BuildingKind::{Hq, Depot, Barracks}`,
    `ResourceKind::{Crystal, Gas}`, `MAX_ENTITIES = 2_048`, `OWNER_PLAYER = 0`,
    `OWNER_NEUTRAL = 255`, `CARRY_NONE = 0xFF`.
  - `Resources { crystal: u32, gas: u32 }` with `covers`, `try_debit`
    (all-or-nothing), `credit` (saturating), `Resources::ZERO`.
  - `Supply` with `used()`, `cap()`, `free()`, `fits(cost)`, `add_used`,
    `remove_used` (saturating), `grant_cap` (clamped to
    `scenario::MAX_SUPPLY_CAP = 500`), `revoke_cap`;
    `WORKER_SUPPLY_COST = 1`, `SOLDIER_SUPPLY_COST = 2`, `supply_cost(kind)`.
  - `BuildingKind::footprint_cells()` = `12 / 8 / 10`; `is_drop_off()` true only
    for `Hq`. A building's `position` is its footprint **centre** in cell space.
  - `EntityStore` columns: `alive`, `kind`, `owner`, `position`, `dir`, `frame`,
    `progress`, `progress_target`, `amount`, `carry_kind`, `carry_amount`, with
    `spawn(kind, owner, pos) -> Option<EntityId>`, `despawn`, `slot`, `id_at`,
    `contains`, `set_progress(slot, progress, target)`, `set_carry`,
    `collect_live(&self, out: &mut Vec<usize>)`, `hash_into`.
  - Orders:
    ```rust
    pub enum GatherPhase { ToNode { field_slot: u8 }, Mining { ticks_left: u32 }, Returning { drop_off: EntityId, field_slot: u8 } }
    pub enum Order { Idle, Move { dest: Cell, field_slot: u8 }, Gather { node: EntityId, phase: GatherPhase }, Build { site: EntityId, field_slot: u8 } }
    pub struct OrderTable;   // get / set / clear / hash_into
    pub(crate) fn rect_distance(p: [f32; 2], center: [f32; 2], edge: u32) -> f32;
    pub(crate) fn building_approach_cell(store: &EntityStore, blocked: &[bool], width: u32, height: u32, id: EntityId) -> Cell;
    ```
  - Build system: `build::{building_cost, build_ticks, supply_grant, footprint_cells,
    placement_valid, Placement, PlacementError, BUILD_REACH_CELLS,
    HQ_COST, DEPOT_COST, BARRACKS_COST, HQ_BUILD_TICKS, DEPOT_BUILD_TICKS,
    BARRACKS_BUILD_TICKS, HQ_SUPPLY_GRANT = 10, DEPOT_SUPPLY_GRANT = 10,
    BARRACKS_SUPPLY_GRANT = 0}`. A building under construction has
    `progress_target > 0` (`RtsWorld::is_site(id)`); a finished one has
    `progress == 0 && progress_target == 0` and its footprint is **blocked** in
    `nav().blocked()`.
  - `RtsWorld`: `scenario()`, `entities()`, `entities_mut()`, `resources()`,
    `supply()`, `tick_index()`, `start_hq()`, `nav()`, `order_move`,
    `order_move_group`, `order_gather`, `order_gather_group`, `order_build`,
    `nearest_drop_off`, `order_of`, `placement`, `begin_placement`,
    `cancel_placement`, `confirm_placement(min, builder)`,
    `cancel_construction(site)`, `is_site(id)`, `selection()` (T8), `tick()`,
    `state_hash()`.
  - `tick()`'s reserved system order: *1. commands, 2. camera, 3. construction,
    4. production, 5. orders, 6. movement, 7. supply recount*, then selection
    self-heal. Step 4 and step 7 are the two this ticket fills in.
  - Tracked scene: 320 × 320, HQ min corner `(160, 160)` edge 12 (centre
    `[166.0, 166.0]`), six workers at `(162..=167, 178)`, start stock
    `crystal 300 / gas 100`, supply cap `10`, used `6`.
  - `testkit::RtsHarness::scene()` / `step_exact` / `world` / `world_mut` /
    `state_hash` / `ids_of_kind`.

## Exact design — no decisions left

### `crates/mmd-engine/src/rts/production.rs`

```rust
/// Entries a building may hold, including the one in progress.
///
/// Five, as in the genre this one clones. It is small enough that the whole
/// queue fits in the state hash as five bytes and large enough that a player
/// can bank a build order or two.
pub const PRODUCTION_QUEUE_CAP: usize = 5;

/// Cost of each producible unit.
pub const WORKER_COST: Resources = Resources { crystal: 50, gas: 0 };
pub const SOLDIER_COST: Resources = Resources { crystal: 50, gas: 25 };
pub fn unit_cost(kind: UnitKind) -> Resources;

/// Ticks each unit takes to build. 60 ticks = 1 second.
pub const WORKER_PRODUCE_TICKS: u32 = 300;
pub const SOLDIER_PRODUCE_TICKS: u32 = 360;
pub fn produce_ticks(kind: UnitKind) -> u32;

/// Which building produces which unit. The whole build tree of this slice.
///
/// A Depot produces nothing — it exists to raise the supply cap, and a building
/// that both raises supply and produces would make the supply mechanic
/// unobservable.
pub fn can_produce(building: BuildingKind, unit: UnitKind) -> bool {
    matches!((building, unit), (BuildingKind::Hq, UnitKind::Worker)
                             | (BuildingKind::Barracks, UnitKind::Soldier))
}

/// Why an enqueue was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ProduceError {
    #[error("no such building")]
    NoBuilding,
    #[error("this building cannot produce that unit")]
    WrongBuilding,
    #[error("the building is still under construction")]
    UnderConstruction,
    #[error("the production queue is full")]
    QueueFull,
    #[error("not enough resources")]
    Unaffordable,
    #[error("not enough supply")]
    SupplyBlocked,
    #[error("the entity store is full")]
    StoreFull,
}

/// One building's queue: a fixed ring plus the head's elapsed ticks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct ProductionQueue { /* private: [Option<UnitKind>; CAP], len, progress */ }

impl ProductionQueue {
    pub fn len(&self) -> usize;
    pub fn is_empty(&self) -> bool;
    pub fn is_full(&self) -> bool;
    /// The unit currently being produced.
    pub fn head(&self) -> Option<UnitKind>;
    /// Ticks elapsed on the head.
    pub fn progress(&self) -> u32;
    /// Queue contents, oldest first.
    pub fn entries(&self) -> &[UnitKind];
    /// Push at the back. `false` when full.
    pub fn push(&mut self, kind: UnitKind) -> bool;
    /// Remove entry `index` (0 = the one in progress). Returns what was removed.
    /// Removing the head resets `progress` to zero — a partly built unit is not
    /// carried over to the next entry.
    pub fn cancel(&mut self, index: usize) -> Option<UnitKind>;
    /// Advance the head by one tick; returns the finished unit when it completes.
    pub fn advance(&mut self) -> Option<UnitKind>;
    pub fn hash_into(&self, h: &mut sha2::Sha256);
}

/// One queue and one rally point per entity slot, preallocated to `MAX_ENTITIES`.
#[derive(Debug, Clone)]
pub struct ProductionTable { /* private */ }

impl ProductionTable {
    pub fn new() -> Self;
    pub fn queue(&self, slot: usize) -> &ProductionQueue;
    pub fn queue_mut(&mut self, slot: usize) -> &mut ProductionQueue;
    pub fn rally(&self, slot: usize) -> Option<Cell>;
    pub fn set_rally(&mut self, slot: usize, cell: Option<Cell>);
    /// Reset a slot — called when a building is despawned so a reused slot does
    /// not inherit a queue.
    pub fn clear(&mut self, slot: usize);
    pub fn hash_into(&self, h: &mut sha2::Sha256, live: &[usize]);
}
```

### `RtsWorld` API

```rust
impl RtsWorld {
    /// Queue a unit at a building.
    ///
    /// Charges the **resources and the supply at enqueue time**, not at
    /// completion. Reserving supply up front is what makes the cap a real
    /// bound: charging on completion would let a player queue five Soldiers
    /// into two free supply and get all five.
    pub fn enqueue_unit(&mut self, building: EntityId, unit: UnitKind) -> Result<(), ProduceError>;

    /// Cancel queue entry `index` at `building`, refunding its cost and
    /// releasing its supply reservation. `false` when there is no such entry.
    pub fn cancel_queued(&mut self, building: EntityId, index: usize) -> bool;

    pub fn production_queue(&self, building: EntityId) -> Option<&ProductionQueue>;

    /// Where units produced here walk after they appear. `None` leaves them idle.
    pub fn rally(&self, building: EntityId) -> Option<Cell>;
    /// Set or clear a rally point. `false` for a stale id or a non-building.
    /// A rally cell that is out of bounds or blocked is rejected.
    pub fn set_rally(&mut self, building: EntityId, cell: Option<Cell>) -> bool;

    /// Supply reserved by every live production queue.
    pub fn reserved_supply(&self) -> u32;
}
```

### Production system — step 4 of `tick()`

Runs after construction, so a Barracks that finished this tick can already hold
a queue, and before orders, so a unit produced this tick can be given its rally
order in the same tick.

```
for &slot in &live_scratch {
    let EntityKind::Building(_) = entities.kind(slot) else { continue };
    if entities.progress_target(slot) != 0 { continue; }          // still a site
    let Some(kind) = production.queue(slot).head() else { continue };
    let Some(done) = production.queue_mut(slot).advance() else { continue };

    // Spawn beside the building, on its approach cell.
    let cell = building_approach_cell(entities, nav.blocked(), w, h, id_at(slot));
    let pos = [cell.x as f32 + 0.5, cell.y as f32 + 0.5];
    let Some(id) = entities.spawn(EntityKind::Unit(done), OWNER_PLAYER, pos) else {
        // Store full: put the entry back at the FRONT and stop. The player keeps
        // what they paid for rather than losing it to a silent drop.
        production.queue_mut(slot).push_front(done);
        continue;
    };
    if let Some(rally) = production.rally(slot) {
        self.order_move(id, rally);      // ignores its own return; a blocked rally is a no-op
    }
    let _ = kind;   // `head()` before `advance()` is only for the guard above
}
```

`ProductionQueue::push_front` is a private helper used only by that recovery
path; it is `pub(crate)` and documented as such.

### Supply recount — step 7 of `tick()`

```
let mut used = 0;
for &slot in &live_scratch {
    if let EntityKind::Unit(k) = entities.kind(slot) { used += supply_cost(k); }
}
used += self.reserved_supply();
self.supply = Supply { used, ..self.supply };   // via a `set_used` method
```

`Supply` gains `pub(crate) fn set_used(&mut self, used: u32)`.

**Recomputed, never incremented.** An incremental counter has to be adjusted at
five call sites (spawn, despawn, enqueue, cancel, store-full recovery) and any
one of them missed is a slow drift the player only notices when production
mysteriously stops. Recounting 2 048 slots per tick is a linear pass over one
`Vec<EntityKind>`.

`reserved_supply()` sums `supply_cost(k)` over every live building slot's queue
entries.

### `enqueue_unit` body order — exactly

1. Resolve `building` → slot, or `NoBuilding`.
2. `EntityKind::Building(b)`, else `NoBuilding`.
3. `entities.owner(slot) == OWNER_PLAYER`, else `NoBuilding`.
4. `progress_target(slot) == 0`, else `UnderConstruction`.
5. `can_produce(b, unit)`, else `WrongBuilding`.
6. `!queue.is_full()`, else `QueueFull`.
7. `supply.free() >= supply_cost(unit)`, else `SupplyBlocked`.
   (`free()` already accounts for reservations, because step 7 folds them into
   `used` every tick.)
8. `resources.try_debit(unit_cost(unit))`, else `Unaffordable`.
9. `queue.push(unit)`, then `supply.add_used(supply_cost(unit))` so the
   reservation is visible **before** the next tick's recount.

Order matters: the supply check precedes the debit, so a supply-blocked enqueue
never takes the player's money.

### Despawn hook

`RtsWorld` must call `production.clear(slot)` whenever a building is despawned
(`cancel_construction`, and any future destruction). Add it to
`cancel_construction` and note it in `EntityStore::despawn`'s doc as a caller
responsibility — the store does not know about the table.

`state_hash` gains `production.hash_into(h, &live)` after the selection.

## TDD

1. **Red** — write every test below in a new
   `crates/mmd-engine/tests/rts_production.rs`. Watch them fail.
2. **Green** — implement.
3. **Refactor** — none expected. Keep green.

## Test plan

Harness `RtsHarness::scene()`; `hq = world().start_hq().unwrap()`;
`w0` = worker at entity slot 11. A clear buildable corner is `(180, 176)`.

| Test | Input | Expect |
| ---- | ----- | ------ |
| `costs_and_times_are_the_published_constants` | both kinds | `(50,0)/(50,25)`, `300/360` |
| `the_build_tree_is_hq_worker_and_barracks_soldier` | all 6 pairs | only those two are `true` |
| `queue_push_respects_the_cap` | 6 pushes | 5 succeed, the 6th returns `false` |
| `queue_cancel_of_the_head_resets_progress` | advance 100 ticks, cancel index 0 | `progress() == 0`, `head()` is the old index 1 |
| `queue_cancel_of_a_tail_entry_keeps_progress` | advance 100, cancel index 2 | `progress() == 100`, head unchanged |
| `queue_advance_completes_at_the_documented_tick` | Worker head, advance 299 then once more | `None` × 299 then `Some(Worker)` |
| `enqueue_rejects_a_stale_building` | despawned id | `Err(NoBuilding)` |
| `enqueue_rejects_a_worker_as_the_producer` | `w0` | `Err(NoBuilding)` |
| `enqueue_rejects_a_soldier_at_the_hq` | `(hq, Soldier)` | `Err(WrongBuilding)` |
| `enqueue_rejects_a_worker_at_a_barracks` | build a Barracks, `(barracks, Worker)` | `Err(WrongBuilding)` |
| `enqueue_rejects_a_site` | place a Barracks, enqueue before it finishes | `Err(UnderConstruction)` |
| `enqueue_rejects_a_full_queue` | 5 Workers then a 6th | `Err(QueueFull)` |
| `enqueue_rejects_over_supply` | cap 10, used 6, queue 4 Workers, then a 5th | `Err(SupplyBlocked)` |
| `a_supply_blocked_enqueue_does_not_charge` | as above | `resources()` unchanged by the failed call |
| `enqueue_rejects_when_unaffordable` | drain to `(10, 0)` | `Err(Unaffordable)`, supply unchanged |
| `enqueue_debits_immediately` | one Worker | `resources().crystal == 250` |
| `enqueue_reserves_supply_immediately` | one Worker | `supply().used() == 7` on the same call, before any tick |
| `queueing_cannot_exceed_the_cap` | cap 10, used 6; queue as many Soldiers as accepted | at most 2 accepted (`2 × 2 == 4` free supply) |
| `cancel_queued_refunds_and_releases` | enqueue a Worker, cancel index 0 | `resources().crystal == 300`, `supply().used() == 6` |
| `cancel_queued_rejects_a_bad_index` | empty queue, index 0 | `false` |
| `a_worker_appears_after_its_build_time` | enqueue at the HQ, `step_exact(300)` | one more `EntityKind::Unit(Worker)` exists |
| `a_produced_unit_spawns_beside_its_building` | same | the new unit's cell is adjacent to the HQ footprint and unblocked |
| `a_produced_unit_is_idle_without_a_rally` | same | `order_of(new) == Some(Order::Idle)` |
| `a_produced_unit_walks_to_the_rally` | `set_rally(hq, Some(Cell{x:200,y:200}))`, produce, `step_exact(1200)` | the unit ends within `ARRIVAL_RADIUS_CELLS` of `(200.5, 200.5)` |
| `set_rally_rejects_a_blocked_cell` | a scenario obstacle cell | `false`, rally unchanged |
| `set_rally_rejects_a_non_building` | `w0` | `false` |
| `supply_used_is_recomputed_not_incremented` | despawn a worker directly via `entities_mut`, `step_exact(1)` | `supply().used()` fell by exactly `1` |
| `supply_used_counts_reservations` | enqueue 2 Workers, `step_exact(1)` | `used() == 6 + 2` |
| `reserved_supply_reports_the_queues` | 2 Workers + (after a Barracks) 1 Soldier | `reserved_supply() == 1 + 1 + 2` |
| `a_barracks_produces_a_soldier` | build a Barracks, enqueue a Soldier, `step_exact(360)` | one `Unit(Soldier)` exists, `resources().gas == 75` |
| `production_stops_when_the_store_is_full` | fill the store to `MAX_ENTITIES`, complete a queued unit | queue length unchanged, no panic, resources still spent (documented) |
| `cancelling_a_site_clears_its_queue_slot` | build a Barracks, queue a Soldier, cancel construction, spawn a new building into the reused slot | the new building's queue is empty |
| `production_is_reproducible` | two harnesses, identical actions, 3000 ticks | equal `state_hash()` |
| `state_hash_sees_a_queue_entry` | before/after one enqueue | different |
| `state_hash_sees_a_rally_point` | before/after `set_rally` | different |
| `production_allocates_nothing` | in `frame_allocations.rs`, `MeasureGuard` around 600 ticks with two producing buildings | zero allocations |

**Mutation verification (mandatory).** Inject, confirm red, revert, confirm green:
1. Charge supply at completion instead of enqueue → kills `queueing_cannot_exceed_the_cap` and `enqueue_reserves_supply_immediately`.
2. Debit before the supply check → kills `a_supply_blocked_enqueue_does_not_charge`.
3. `Supply::used` incremented on spawn instead of recomputed → kills `supply_used_is_recomputed_not_incremented`.
4. `reserved_supply` counts entries rather than their supply cost → kills `reserved_supply_reports_the_queues` (Soldier costs 2).
5. `cancel` of the head keeps `progress` → kills `queue_cancel_of_the_head_resets_progress`.
6. `can_produce` returns `true` for every pair → kills the two `WrongBuilding` tests.
7. Production runs before construction → kills `enqueue_rejects_a_site`? No — add `a_barracks_can_be_queued_the_tick_it_finishes` and confirm the reordering kills it.
8. `production.clear(slot)` dropped from `cancel_construction` → kills `cancelling_a_site_clears_its_queue_slot`.

## Impl steps

- [x] 1. Create `crates/mmd-engine/src/rts/production.rs` with the constants, `ProduceError`, `ProductionQueue`, `ProductionTable`.
- [x] 2. Add `mod production;` and the re-exports to `crates/mmd-engine/src/rts/mod.rs`.
- [x] 3. Add `pub(crate) fn set_used(&mut self, used: u32)` to `Supply` in `crates/mmd-engine/src/rts/economy.rs`.
- [x] 4. Create `crates/mmd-engine/tests/rts_production.rs` and write every test from the table. Watch them fail.
- [x] 5. Implement `ProductionQueue` (`push`, `push_front`, `cancel`, `advance`, `head`, `entries`, `hash_into`).
- [x] 6. Implement `ProductionTable` with `MAX_ENTITIES` entries, reserved at construction.
- [x] 7. Add a `production: ProductionTable` field to `RtsWorld`.
- [x] 8. Implement `enqueue_unit` with the nine steps in the exact order given.
- [x] 9. Implement `cancel_queued`, `production_queue`, `rally`, `set_rally`, `reserved_supply`.
- [x] 10. Implement the production system as step 4 of `tick()`, body exactly as quoted.
- [x] 11. Implement the supply recount as step 7 of `tick()`.
- [x] 12. Call `production.clear(slot)` in `cancel_construction`.
- [x] 13. Extend `RtsWorld::state_hash` with the production table.
- [x] 14. Add `production_allocates_nothing` to `crates/mmd-engine/tests/frame_allocations.rs`.
- [x] 15. Run the mutation list; record kills in the commit body.
- [x] 16. Run the full validation block.

## Outputs

- **Files created**
  - `crates/mmd-engine/src/rts/production.rs`
  - `crates/mmd-engine/tests/rts_production.rs`
- **Files edited**
  - `crates/mmd-engine/src/rts/{mod,economy,world}.rs`
  - `crates/mmd-engine/tests/frame_allocations.rs`
- **Public API added:** `rts::{ProductionQueue, ProductionTable, ProduceError, PRODUCTION_QUEUE_CAP, WORKER_COST, SOLDIER_COST, WORKER_PRODUCE_TICKS, SOLDIER_PRODUCE_TICKS, unit_cost, produce_ticks, can_produce}`, `RtsWorld::{enqueue_unit, cancel_queued, production_queue, rally, set_rally, reserved_supply}`.
- **Behaviour change:** buildings produce units; supply bounds them.
- **Migration / config:** none.

## Validation

- [x] `cargo fmt --all -- --check`
- [x] `cargo test -p mmd-engine --test rts_production` — all green
- [x] `cargo test -p mmd-engine --test rts_build` — all green
- [x] `cargo test -p mmd-engine --test rts_economy` — all green
- [x] `cargo test -p mmd-engine --test frame_allocations` — all green
- [x] `MMD_REQUIRE_GPU=1 cargo test --workspace --locked`
- [x] `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- [x] `nix flake check`
- [x] `cargo run -- run --agents 5000 --frames 300` — exit 0, exit-line `hash=` unchanged
- [x] app functional — no broken path from this slice
- [x] commit msg draft: `feat(rts): produce units from queues bounded by supply`
