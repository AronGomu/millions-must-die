# T6: Entity store + `RtsWorld`

**Plan:** `./ai-artifacts/PLAN_2026_08_10_rts-engine-prototype.md`
**Depends:** T5
**Commit outcome:** loading the RTS scene builds a world holding an HQ, six workers and ten resource nodes with stable generational ids, and `tick()` advances it deterministically.

## Context (self-contained)

- Goal: phase 1 is a thin vertical slice of an RTS engine prototype (camera,
  selection, workers, economy, building, unit production) on a horde-free scene.
- This slice: the data spine. A preallocated SoA entity store with generational
  ids, and an `RtsWorld` that seeds itself from the scenario and ticks. No
  movement, no orders, no economy mutation, no rendering — those are T7–T11.
- Out of scope here: `crates/mmd-engine/src/sim/` (the horde simulation is not
  touched), `crates/mmd-engine/src/runtime.rs`, the renderer, `src/`.
- Assumptions in force: no per-frame allocation. Every column is reserved at
  construction to `MAX_ENTITIES`; spawn and despawn reuse slots and never grow a
  `Vec`. Determinism is same-host, same-binary, seed-driven — as phase 0.

## Requirements

- New module tree `crates/mmd-engine/src/rts/` with `entity.rs`, `economy.rs`
  and `world.rs`, plus `mod.rs`.
- Generational `EntityId`: a despawned entity's slot may be reused, and a stale
  id must resolve to `None` rather than to whatever now occupies the slot.
- `RtsWorld::from_scenario` seeds HQ, workers and resource nodes from the
  scenario's RTS block.
- `RtsWorld::tick()` advances `tick_index` and nothing else yet.
- `RtsWorld::state_hash()` — a SHA-256 digest of the world, so every later
  ticket can pin behaviour the way `Simulation::state_hash` does for the horde.
- `testkit::RtsHarness` — the seeded, clock-free entry point every later RTS
  test drives.

## Inputs

- **Files to read**
  - `crates/mmd-engine/src/sim/agents.rs` — the SoA + `state_hash` style to
    mirror (`Sha256`, `to_bits().to_le_bytes()` for `f32`, length prefix).
  - `crates/mmd-engine/src/testkit/mod.rs` — `Harness`, `HarnessBuilder`,
    `SplitMix64`, `HarnessError`.
  - `crates/mmd-engine/src/testkit/rng.rs` — `SplitMix64::new`, `derive`,
    `next_bounded`.
  - `crates/mmd-engine/src/lib.rs` — the module list to extend.
  - `crates/mmd-engine/src/render/camera.rs` — `Camera` (may not exist yet if T4
    has not landed; if absent, store the camera centre as a plain `[f32; 2]`
    field named `camera_center` and leave a `// T4` note. Do **not** implement a
    second camera).
- **From Depends (T5) — spell out, the worker cannot read T5:**
  - New scenario family constant `mmd_engine::scenario::RTS_PROTOTYPE_V1`
    (`"rts_prototype_v1"`).
  - `Scenario::rts() -> Option<&RtsSpec>` where
    ```rust
    pub struct RtsSpec {
        pub start_crystal: u32,
        pub start_gas: u32,
        pub start_supply_cap: u32,
        pub hq_cell: Cell,          // MINIMUM corner of the HQ footprint
        pub crystal_nodes: Vec<Cell>,
        pub gas_nodes: Vec<Cell>,
    }
    ```
  - Footprint constants: `HQ_FOOTPRINT_CELLS = 12`, `DEPOT_FOOTPRINT_CELLS = 8`,
    `BARRACKS_FOOTPRINT_CELLS = 10`. `MAX_SUPPLY_CAP = 500`.
  - Tracked scene `assets/scenarios/rts_prototype_v1.ron`, reachable as
    `mmd_engine::testkit::rts_scene_path()` (constant `RTS_SCENE`).
    It is 320 × 320 cells at `cell_size_px: 4`, `sprite_size_px: 48`,
    `hard_agent_count: 0`, `stretch_agent_count: 0`, six spawn cells at
    `(162..=167, 178)`, `hq_cell (160, 160)`, 8 crystal nodes, 2 gas nodes,
    `start_crystal 300`, `start_gas 100`, `start_supply_cap 10`.
  - The validator guarantees: HQ footprint fully in bounds and unblocked, every
    node cell unblocked and reachable, no node inside the HQ footprint, no spawn
    inside the HQ footprint.

## Exact design — no decisions left

### `crates/mmd-engine/src/rts/entity.rs`

```rust
/// Hard ceiling on simultaneous RTS entities: units + buildings + nodes.
///
/// Sized from the design pillar, not guessed: the supply cap tops out at
/// `scenario::MAX_SUPPLY_CAP` (500) and the cheapest unit costs one supply, so
/// at most 500 units can exist, plus a bounded number of buildings and the
/// scene's nodes. 2 048 leaves room for all of it and for a construction site
/// per queued building without ever reallocating a column.
pub const MAX_ENTITIES: usize = 2_048;

/// Owner id of the human player.
pub const OWNER_PLAYER: u8 = 0;
/// Owner id of unowned world objects (resource nodes).
pub const OWNER_NEUTRAL: u8 = 255;

/// Producible unit kinds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum UnitKind { Worker = 0, Soldier = 1 }

/// Placeable building kinds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum BuildingKind { Hq = 0, Depot = 1, Barracks = 2 }

/// Harvestable resource kinds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum ResourceKind { Crystal = 0, Gas = 1 }

/// What an entity is. Discriminants are appended, never inserted — a recorded
/// state hash names a kind by value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EntityKind {
    Unit(UnitKind),
    Building(BuildingKind),
    Node(ResourceKind),
}

impl EntityKind {
    /// One stable byte per kind, for the state hash. `0x1_`=unit, `0x2_`=building,
    /// `0x3_`=node, low nibble the inner discriminant.
    pub fn tag(self) -> u8;
    /// Footprint edge in cells. Units and nodes occupy a single cell (`1`).
    pub fn footprint_cells(self) -> u32;
}

impl BuildingKind {
    /// Footprint edge in cells, from the scenario contract's constants.
    pub fn footprint_cells(self) -> u32 {
        match self {
            Self::Hq => scenario::HQ_FOOTPRINT_CELLS,
            Self::Depot => scenario::DEPOT_FOOTPRINT_CELLS,
            Self::Barracks => scenario::BARRACKS_FOOTPRINT_CELLS,
        }
    }
    /// Whether workers may return cargo here. Only the HQ, in this slice.
    pub fn is_drop_off(self) -> bool { matches!(self, Self::Hq) }
}

/// A stable handle into [`EntityStore`].
///
/// The generation is what makes a handle safe to hold across a despawn: a slot
/// reused by a new entity gets a higher generation, so the old handle resolves
/// to `None` instead of silently naming the newcomer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct EntityId { pub index: u32, pub generation: u32 }

/// Preallocated SoA store. Every column has `MAX_ENTITIES` capacity from
/// construction and is never grown.
#[derive(Debug, Clone)]
pub struct EntityStore { /* private */ }

impl EntityStore {
    pub fn new() -> Self;

    /// Live entity count.
    pub fn len(&self) -> usize;
    pub fn is_empty(&self) -> bool;
    /// Slots ever allocated — the iteration bound. Includes dead slots.
    pub fn slot_count(&self) -> usize;

    /// Allocate an entity at cell-space position `pos`.
    ///
    /// Returns `None` when the store is full: a caller that cannot spawn must
    /// see it, not silently drop a unit it charged the player for.
    pub fn spawn(&mut self, kind: EntityKind, owner: u8, pos: [f32; 2]) -> Option<EntityId>;

    /// Free a slot. Returns `false` for a stale or already-dead id.
    pub fn despawn(&mut self, id: EntityId) -> bool;

    /// Whether `id` names a live entity.
    pub fn contains(&self, id: EntityId) -> bool;

    /// Slot index of a live id.
    pub fn slot(&self, id: EntityId) -> Option<usize>;
    /// The id currently occupying `slot`, if it is live.
    pub fn id_at(&self, slot: usize) -> Option<EntityId>;

    // Column readers — all take a live slot index, panic on a dead one.
    pub fn alive(&self, slot: usize) -> bool;
    pub fn kind(&self, slot: usize) -> EntityKind;
    pub fn owner(&self, slot: usize) -> u8;
    pub fn position(&self, slot: usize) -> [f32; 2];
    pub fn dir(&self, slot: usize) -> u8;
    pub fn frame(&self, slot: usize) -> u8;
    /// Construction/production progress in ticks; `0` when nothing is in progress.
    pub fn progress(&self, slot: usize) -> u32;
    /// Ticks the current progress must reach; `0` when nothing is in progress.
    pub fn progress_target(&self, slot: usize) -> u32;
    /// Remaining amount in a resource node; `0` for every other kind.
    pub fn amount(&self, slot: usize) -> u32;

    // Column writers.
    pub fn set_position(&mut self, slot: usize, pos: [f32; 2]);
    pub fn set_dir(&mut self, slot: usize, dir: u8);
    pub fn set_frame(&mut self, slot: usize, frame: u8);
    pub fn set_progress(&mut self, slot: usize, progress: u32, target: u32);
    pub fn set_amount(&mut self, slot: usize, amount: u32);

    /// Live slot indices in ascending order, into a caller-owned buffer.
    ///
    /// Takes an `&mut Vec` rather than returning one so a per-tick sweep costs
    /// no allocation. The buffer is cleared first.
    pub fn collect_live(&self, out: &mut Vec<usize>);

    /// Feed every live slot's state into `h`, in ascending slot order.
    pub fn hash_into(&self, h: &mut sha2::Sha256);
}
```

Storage: parallel `Vec`s `alive: Vec<bool>`, `generation: Vec<u32>`,
`kind: Vec<EntityKind>`, `owner: Vec<u8>`, `x: Vec<f32>`, `y: Vec<f32>`,
`dir: Vec<u8>`, `frame: Vec<u8>`, `progress: Vec<u32>`,
`progress_target: Vec<u32>`, `amount: Vec<u32>`, plus
`free: Vec<u32>` (a LIFO free list) and `live: usize`. Every one is
`Vec::with_capacity(MAX_ENTITIES)` in `new()`.

`spawn` pops the free list; if empty and `slot_count() < MAX_ENTITIES`, pushes a
new slot; otherwise returns `None`. `despawn` sets `alive[i] = false`,
increments `generation[i]` (wrapping), pushes `i` to `free`.

**Free-list order is part of the contract**: LIFO. Two runs that spawn and
despawn the same sequence must produce the same slot assignment, or the state
hash is not reproducible. Assert it.

### `crates/mmd-engine/src/rts/economy.rs`

```rust
/// Player stock of both resources.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Resources { pub crystal: u32, pub gas: u32 }

impl Resources {
    pub const ZERO: Self = Self { crystal: 0, gas: 0 };
    /// Whether this stock covers `cost`.
    pub fn covers(&self, cost: Resources) -> bool;
    /// Subtract `cost`, or leave the stock untouched and return `false`.
    pub fn try_debit(&mut self, cost: Resources) -> bool;
    /// Add `amount`, saturating.
    pub fn credit(&mut self, amount: Resources);
}

/// Supply usage and ceiling. `cap` is clamped to `scenario::MAX_SUPPLY_CAP`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Supply { used: u32, cap: u32 }

impl Supply {
    pub fn new(cap: u32) -> Self;                    // clamps to MAX_SUPPLY_CAP
    pub fn used(&self) -> u32;
    pub fn cap(&self) -> u32;
    /// Free supply headroom, saturating at zero — a cap that *fell* below usage
    /// (a Depot destroyed later) must read as 0 free, not underflow.
    pub fn free(&self) -> u32;
    /// Whether `cost` fits under the cap right now.
    pub fn fits(&self, cost: u32) -> bool;
    pub fn add_used(&mut self, cost: u32);
    pub fn remove_used(&mut self, cost: u32);        // saturating
    pub fn grant_cap(&mut self, amount: u32);        // clamps to MAX_SUPPLY_CAP
    pub fn revoke_cap(&mut self, amount: u32);       // saturating
}

/// Supply a unit kind costs.
pub const WORKER_SUPPLY_COST: u32 = 1;
pub const SOLDIER_SUPPLY_COST: u32 = 2;
pub fn supply_cost(kind: UnitKind) -> u32;
```

### `crates/mmd-engine/src/rts/world.rs`

```rust
/// Failures building an [`RtsWorld`].
#[derive(Debug, thiserror::Error)]
pub enum RtsWorldError {
    #[error(transparent)]
    Scenario(#[from] crate::scenario::ScenarioError),
    #[error("scenario {version} carries no rts block; the rts world needs one")]
    NotAnRtsScene { version: String },
    #[error("entity store full while seeding: {what}")]
    StoreFull { what: String },
}

/// The phase-1 RTS game state.
#[derive(Debug)]
pub struct RtsWorld { /* private */ }

impl RtsWorld {
    /// Load a hash-verified scenario and seed the world from its RTS block.
    pub fn load(path: impl AsRef<std::path::Path>) -> Result<Self, RtsWorldError>;

    /// Seed from an already-validated scenario.
    pub fn from_scenario(scenario: Scenario) -> Result<Self, RtsWorldError>;

    pub fn scenario(&self) -> &Scenario;
    pub fn entities(&self) -> &EntityStore;
    pub fn entities_mut(&mut self) -> &mut EntityStore;
    pub fn resources(&self) -> Resources;
    pub fn supply(&self) -> Supply;
    pub fn tick_index(&self) -> u64;

    /// The starting HQ. `None` only after it is destroyed, which nothing in
    /// phase 1 can do.
    pub fn start_hq(&self) -> Option<EntityId>;

    /// Advance one fixed 1/60 s step.
    ///
    /// Systems are added by later tickets and each one runs at a fixed point in
    /// this order, so a reordering is a visible diff rather than an accident:
    /// 1. commands, 2. camera, 3. construction, 4. production, 5. orders,
    /// 6. movement, 7. supply recount. Today only the tick counter advances.
    pub fn tick(&mut self);

    /// Exact same-host state digest.
    ///
    /// Covers `tick_index`, live entity count, then every live slot in
    /// ascending order (kind tag, owner, x bits, y bits, dir, frame, progress,
    /// progress_target, amount), then resources and supply. `f32` goes in as
    /// raw bits, matching `Simulation::state_hash`.
    pub fn state_hash(&self) -> [u8; 32];
}
```

**Seeding order is part of the contract** — change it and every recorded hash
moves:

1. The HQ: `spawn(EntityKind::Building(BuildingKind::Hq), OWNER_PLAYER, pos)`
   where `pos` is the footprint's **centre** in cell space,
   `[hq_cell.x as f32 + HQ_FOOTPRINT_CELLS as f32 * 0.5,
     hq_cell.y as f32 + HQ_FOOTPRINT_CELLS as f32 * 0.5]`.
   Progress is set to `(0, 0)` — the starting HQ is finished, not a site.
2. Crystal nodes, in `crystal_nodes` order, then gas nodes in `gas_nodes` order,
   each at its cell centre `[c.x as f32 + 0.5, c.y as f32 + 0.5]`, owner
   `OWNER_NEUTRAL`, `amount` set to `NODE_CRYSTAL_AMOUNT` / `NODE_GAS_AMOUNT`
   (declared here, consumed in T9):
   ```rust
   pub const NODE_CRYSTAL_AMOUNT: u32 = 1_500;
   pub const NODE_GAS_AMOUNT: u32 = 2_500;
   ```
3. One `UnitKind::Worker` per `scenario.spawn_cells()` entry, in order, at each
   cell's centre, owner `OWNER_PLAYER`, `dir = 0`, `frame = 0`.

Then `resources = Resources { crystal: rts.start_crystal, gas: rts.start_gas }`,
`supply = Supply::new(rts.start_supply_cap)` with
`add_used(WORKER_SUPPLY_COST * worker_count)`.

Any `spawn` returning `None` during seeding is `RtsWorldError::StoreFull`.

### `crates/mmd-engine/src/rts/mod.rs`

```rust
//! Phase-1 real-time-strategy world: entities, economy, orders, buildings.

mod economy;
mod entity;
mod world;

pub use economy::{
    Resources, SOLDIER_SUPPLY_COST, Supply, WORKER_SUPPLY_COST, supply_cost,
};
pub use entity::{
    BuildingKind, EntityId, EntityKind, EntityStore, MAX_ENTITIES, OWNER_NEUTRAL,
    OWNER_PLAYER, ResourceKind, UnitKind,
};
pub use world::{NODE_CRYSTAL_AMOUNT, NODE_GAS_AMOUNT, RtsWorld, RtsWorldError};
```

`crates/mmd-engine/src/lib.rs`: add `pub mod rts;` after `pub mod render;`.

### `crates/mmd-engine/src/testkit/rts.rs`

```rust
/// A seeded, clock-free RTS world runner — the RTS sibling of [`Harness`].
#[derive(Debug)]
pub struct RtsHarness { /* private */ }

impl RtsHarness {
    /// The tracked phase-1 scene.
    pub fn scene() -> RtsHarnessBuilder;
    /// An arbitrary scenario file, hash-verified.
    pub fn path(path: impl Into<PathBuf>) -> RtsHarnessBuilder;
    /// An in-memory spec, same validator, no hash.
    pub fn spec(spec: ScenarioSpec) -> RtsHarnessBuilder;

    /// Advance exactly `ticks` ticks.
    pub fn step_exact(&mut self, ticks: u64);
    pub fn tick_index(&self) -> u64;
    pub fn world(&self) -> &RtsWorld;
    pub fn world_mut(&mut self) -> &mut RtsWorld;
    pub fn state_hash(&self) -> [u8; 32];
    pub fn state_hash_hex(&self) -> String;
    /// A fresh, independent seed stream for the named subsystem.
    pub fn rng(&self, label: &str) -> SplitMix64;
    /// Live entities of one kind, ascending slot order.
    pub fn ids_of_kind(&self, kind: EntityKind) -> Vec<EntityId>;
}

pub struct RtsHarnessBuilder { /* source + seed */ }
impl RtsHarnessBuilder {
    pub fn seed(self, seed: u64) -> Self;
    pub fn build(self) -> Result<RtsHarness, HarnessError>;
}
```

`HarnessError` gains `#[error(transparent)] Rts(#[from] RtsWorldError)`.
Export `RtsHarness`, `RtsHarnessBuilder` from `crates/mmd-engine/src/testkit/mod.rs`.

## TDD

1. **Red** — write every test below in a new
   `crates/mmd-engine/tests/rts_world.rs`. Watch them fail.
2. **Green** — implement `entity.rs`, `economy.rs`, `world.rs`, `testkit/rts.rs`.
3. **Refactor** — none expected. Keep green.

## Test plan

| Test | Input | Expect |
| ---- | ----- | ------ |
| `a_fresh_store_is_empty` | `EntityStore::new()` | `len() == 0`, `slot_count() == 0` |
| `spawn_returns_a_resolvable_id` | one worker | `contains(id)`, `slot(id) == Some(0)`, `kind(0) == Unit(Worker)` |
| `despawn_frees_the_slot` | spawn, despawn | `len() == 0`, `contains(id) == false`, `id_at(0) == None` |
| `a_stale_id_does_not_resolve_to_its_replacement` | spawn A, despawn A, spawn B into the same slot | `contains(a) == false`, `slot(a) == None`, `contains(b) == true`, `b.index == a.index`, `b.generation != a.generation` |
| `double_despawn_is_rejected` | despawn twice | second returns `false`, `len()` unchanged |
| `the_free_list_is_lifo` | spawn 3, despawn slots 0 then 2, spawn 2 more | the new ids land in slots 2 then 0 |
| `the_store_refuses_to_overfill` | `MAX_ENTITIES + 1` spawns | the last returns `None`, `len() == MAX_ENTITIES` |
| `every_column_is_reserved_at_construction` | `EntityStore::new()` then 512 spawns | reading capacity via a `#[cfg(feature = "testkit")] fn column_capacities()` shows every column still `MAX_ENTITIES` |
| `collect_live_is_ascending_and_excludes_the_dead` | spawn 5, despawn slot 2 | `[0, 1, 3, 4]` |
| `footprints_come_from_the_scenario_constants` | each `BuildingKind` | `12`, `8`, `10`; `Unit`/`Node` kinds give `1` |
| `only_the_hq_takes_a_drop_off` | each `BuildingKind` | `Hq` true, others false |
| `kind_tags_are_distinct` | all 7 `EntityKind` values | 7 distinct `tag()` bytes |
| `resources_debit_is_all_or_nothing` | stock `(50, 0)`, cost `(50, 25)` | `try_debit` returns `false` and the stock is still `(50, 0)` |
| `resources_credit_saturates` | stock `(u32::MAX, 0)`, credit `(10, 0)` | `crystal == u32::MAX`, no panic |
| `supply_new_clamps_to_the_pillar` | `Supply::new(9_999)` | `cap() == 500` |
| `supply_free_saturates_when_the_cap_drops` | used 10, cap 10, `revoke_cap(5)` | `free() == 0`, no underflow |
| `supply_fits_is_exact_at_the_boundary` | used 8, cap 10 | `fits(2) == true`, `fits(3) == false` |
| `world_seeds_the_scene` | `RtsHarness::scene().build()` | 1 HQ, 6 workers, 8 crystal nodes, 2 gas nodes; `entities().len() == 17` |
| `world_seeds_the_starting_stock` | same | `resources() == Resources { crystal: 300, gas: 100 }`, `supply().cap() == 10`, `supply().used() == 6` |
| `the_hq_sits_at_its_footprint_centre` | same | `position(slot_of_hq) == [166.0, 166.0]` |
| `nodes_carry_their_starting_amount` | same | every crystal node `amount == 1500`, every gas node `amount == 2500` |
| `workers_start_on_the_scenario_spawn_cells` | same | the six worker positions are exactly the six spawn-cell centres, in scenario order |
| `seeding_order_is_hq_then_nodes_then_workers` | same | slot 0 is the HQ, slots 1..=10 are nodes, slots 11..=16 are workers |
| `a_phase0_scenario_is_refused` | `RtsHarness::path(testkit::gate_scenario_path()).build()` | `Err(..NotAnRtsScene { .. })` |
| `tick_advances_only_the_counter` | `step_exact(10)` | `tick_index() == 10`; every entity's position, dir, frame, progress and amount unchanged |
| `state_hash_moves_with_the_tick` | hash at tick 0 vs tick 1 | different |
| `state_hash_is_reproducible_across_worlds` | two harnesses built the same way, both stepped 300 | equal hashes |
| `state_hash_sees_a_moved_entity` | `set_position(11, [1.0, 1.0])` | hash differs from before |
| `state_hash_sees_a_spent_resource` | `try_debit((1,0))` on the world's stock via `entities_mut`-adjacent test hook | hash differs |
| `state_hash_ignores_a_dead_slot` | spawn then despawn an extra worker | hash equals the pre-spawn hash |
| `harness_rng_streams_are_independent` | `rng("a")` vs `rng("b")`, 8 draws each | the two sequences differ |

**Mutation verification (mandatory).** Inject, confirm red, revert, confirm green:
1. `despawn` does not bump the generation → kills `a_stale_id_does_not_resolve_to_its_replacement`.
2. Free list becomes FIFO → kills `the_free_list_is_lifo`.
3. Seed workers before nodes → kills `seeding_order_is_hq_then_nodes_then_workers` and `state_hash_is_reproducible_across_worlds` stays green (both worlds move together) — this is why the order test exists separately; note that in the commit body.
4. `hash_into` skips `amount` → kills `nodes_carry_their_starting_amount`? No — it kills nothing. Add a dedicated `state_hash_sees_a_drained_node` case (`set_amount(1, 0)`) and confirm the mutation kills it.
5. HQ position uses `hq_cell` directly instead of the footprint centre → kills `the_hq_sits_at_its_footprint_centre`.
6. `Supply::new` clamps to `u32::MAX` → kills `supply_new_clamps_to_the_pillar`.
7. `try_debit` debits the affordable half → kills `resources_debit_is_all_or_nothing`.

## Impl steps

- [x] 1. Create `crates/mmd-engine/src/rts/mod.rs` with the module list and re-exports above.
- [x] 2. Add `pub mod rts;` to `crates/mmd-engine/src/lib.rs`.
- [x] 3. Create `crates/mmd-engine/src/rts/entity.rs` with the constants and enums.
- [x] 4. Create `crates/mmd-engine/src/rts/economy.rs` with `Resources` and `Supply`.
- [x] 5. Create `crates/mmd-engine/tests/rts_world.rs` and write every test from the table. Watch them fail.
- [x] 6. Implement `EntityStore` with the eleven columns, free list and `live` counter, all reserved at `MAX_ENTITIES`.
- [x] 7. Implement `collect_live` and `hash_into`.
- [x] 8. Add `#[cfg(feature = "testkit")] pub fn column_capacities(&self) -> [usize; 11]` to `EntityStore`.
- [x] 9. Create `crates/mmd-engine/src/rts/world.rs`: `RtsWorldError`, `RtsWorld`, `load`, `from_scenario`, the seeding order, accessors.
- [x] 10. Implement `RtsWorld::tick` (counter only) and `state_hash`.
- [x] 11. Create `crates/mmd-engine/src/testkit/rts.rs` with `RtsHarness` + builder; add `mod rts;` and the `pub use` to `testkit/mod.rs`; add the `Rts` variant to `HarnessError`.
- [x] 12. Run `cargo test -p mmd-engine --test rts_world` to green.
- [x] 13. Run the mutation list; record kills in the commit body.
- [x] 14. Run the full validation block. — re-run independently by the parent at
  the T6 checkpoint: `cargo fmt --all -- --check` clean, `cargo clippy --workspace
  --all-targets --all-features -- -D warnings` clean, `cargo test --workspace
  --locked` 38 result lines all `0 failed`, `nix flake check` "all checks
  passed!", all three `xtask --check` ok, `cargo run -- run --agents 5000
  --frames 300` exit 0 with `hash=864147ca3a0e09f7ebc5762b778fce193e705a2bc943ceaf67acf087581ee881`.

## Outputs

- **Files created**
  - `crates/mmd-engine/src/rts/{mod,entity,economy,world}.rs`
  - `crates/mmd-engine/src/testkit/rts.rs`
  - `crates/mmd-engine/tests/rts_world.rs`
- **Files edited**
  - `crates/mmd-engine/src/lib.rs`
  - `crates/mmd-engine/src/testkit/mod.rs`
- **Public API added:** the whole `mmd_engine::rts` module listed above, plus
  `testkit::{RtsHarness, RtsHarnessBuilder}`.
- **Behaviour change:** none for any existing command.
- **Migration / config:** none.

## Validation

- [x] `cargo fmt --all -- --check`
- [x] `cargo test -p mmd-engine --test rts_world` — all green
- [x] `MMD_REQUIRE_GPU=1 cargo test --workspace --locked`
- [x] `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- [x] `nix flake check`
- [x] `cargo tree -e features | grep -c testkit` — `0` (the shipping binary must not gain `testkit`)
- [x] `cargo build --no-default-features --features gpu -p mmd-engine` — compiles (the `#[cfg(feature = "testkit")]` hooks are genuinely gated)
- [x] `cargo run -- run --agents 5000 --frames 300` — exit 0, exit-line `hash=` unchanged
- [x] app functional — no broken path from this slice
- [x] commit msg draft: `feat(rts): add the entity store and the RTS world tick`
