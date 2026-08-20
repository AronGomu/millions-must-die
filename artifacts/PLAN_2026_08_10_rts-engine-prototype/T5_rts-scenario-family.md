# T5: `rts_prototype_v1` scenario family

**Plan:** `./artifacts/PLAN_2026_08_10_rts-engine-prototype.md`
**Depends:** none
**Commit outcome:** a tracked, hash-verified, horde-free RTS scene loads through the existing validator, carrying an HQ site and both resource node kinds.

## Context (self-contained)

- Goal: phase 1 is a thin vertical slice of an RTS engine prototype (camera,
  selection, workers, economy, building, unit production). The scene it runs on
  is **horde-free**: no zombies, because combat is phase 2 and a zombie that
  cannot be fought is cost without payoff. The phase-0 gate scene keeps running
  untouched.
- This slice: extend the scenario contract with an optional RTS block and a new
  family, and commit the tracked scene file plus its generator. Nothing consumes
  the block yet.
- Out of scope here: any consumer of the new data, the entity store, the render
  layer, the CLI. Do not touch `crates/mmd-engine/src/runtime.rs`,
  `crates/mmd-engine/src/sim/`, or `src/`.
- Assumptions in force: **every tracked phase-0 `.ron` must keep its exact bytes
  and its `.sha256` sidecar must stay valid.** That is only achievable if the new
  `ScenarioSpec` field is `#[serde(default)]` — a required field would make every
  existing file fail to parse and force a mass regeneration this plan refuses.

## Requirements

- New version id constant and family, recognised by the existing validator
  dispatcher.
- New `RtsSpec` value type, deserialised from an **optional** `rts:` block.
- `Scenario` exposes the block; every non-RTS family must carry `rts: None`, and
  the RTS family must carry `rts: Some(_)`.
- A deterministic, idempotent generator script for the tracked scene.
- The tracked scene and sidecar committed.
- A testkit constant so later tickets name the scene once.

## Inputs

- **Files to read**
  - `crates/mmd-engine/src/scenario.rs` — `Scenario`, `ScenarioSpec`,
    `ScenarioError`, `from_spec`, `validate_version_and_dims`, `validate_counts`,
    `validate_collision`, `validate_fixture_dims`, `check_population`,
    `normalize_obstacles`, `flood_reachable`, `cell_index`, `MAX_LIVE_AGENTS`.
  - `crates/mmd-engine/src/testkit/fixtures.rs` — `GATE_SCENARIO`,
    `ALL_COLLISION_SCENES`, `scene_path`.
  - `crates/mmd-engine/tests/scenario_contract.rs` — the existing contract tests.
  - `tools/scenegen/gen_collision_scenes.py` — the generator style to mirror.
- **From Depends:** none.
- **Facts you must not rediscover**
  - `validate_version_and_dims` rejects any version that is not
    `fixture_*`, `collision_scene_v1` or `technical_prototype_v1`, and calls
    `check_population(doc)?` **before** dispatching to a family.
  - `validate_counts` locks `atlas_count = 4`, `direction_count = 8`,
    `frame_count = 4` for **every** family — the renderer contract. It applies
    the `hard_agent_count`/`stretch_agent_count` workload lock only when
    `free_workload` is false, where
    `free_workload = version.starts_with("fixture_") || version == "collision_scene_v1"`.
  - `validate_collision` requires: `separation_phases`, `mass_class_count`,
    `separation_threads` each in `1..=MAX`, and if `collision_radius_q8 == 0`
    then all three must be exactly `1` and `separation_strength_q8` must be `0`.
  - `from_spec` already enforces: nonzero `seed`, non-empty `spawn_cells`,
    in-bounds unique obstacles, a free destination, free spawns, and every spawn
    reachable from the destination by flood fill.
  - `Scenario` derives `Debug, Clone, PartialEq, Eq` — so `RtsSpec` must too.
  - `Cell` is `{ x: u32, y: u32 }`, `Deserialize`, `Copy`, `Eq`.

## Exact design — no decisions left

### Constants added to `crates/mmd-engine/src/scenario.rs`

```rust
/// The phase-1 RTS prototype scene family: a horde-free base-building map.
pub const RTS_PROTOTYPE_V1: &str = "rts_prototype_v1";

/// Locked geometry for [`RTS_PROTOTYPE_V1`].
const RTS_WIDTH: u32 = 320;
const RTS_HEIGHT: u32 = 320;
const RTS_CELL_PX: u32 = 4;
const RTS_SPRITE_PX: u32 = 48;

/// Footprint edge of the HQ, in cells. The validator needs it to prove the HQ
/// site is buildable; the build system reuses the same constant.
pub const HQ_FOOTPRINT_CELLS: u32 = 12;
/// Footprint edge of the Depot, in cells.
pub const DEPOT_FOOTPRINT_CELLS: u32 = 8;
/// Footprint edge of the Barracks, in cells.
pub const BARRACKS_FOOTPRINT_CELLS: u32 = 10;

/// Largest starting stock a scene may grant, per resource. Generous, but not
/// "the whole slice is already paid for".
pub const MAX_START_RESOURCE: u32 = 2_000;
/// Absolute supply ceiling — the 500-population design pillar.
pub const MAX_SUPPLY_CAP: u32 = 500;
/// Most resource nodes a scene may declare, per kind.
pub const MAX_RESOURCE_NODES: usize = 64;
```

### `RtsSpec`

```rust
/// The RTS block of a scenario. Present exactly on [`RTS_PROTOTYPE_V1`].
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct RtsSpec {
    /// Starting Crystal stock.
    pub start_crystal: u32,
    /// Starting Gas stock.
    pub start_gas: u32,
    /// Starting supply ceiling, before any Depot is built.
    pub start_supply_cap: u32,
    /// **Minimum corner** (smallest x, smallest y) of the starting HQ's
    /// `HQ_FOOTPRINT_CELLS` x `HQ_FOOTPRINT_CELLS` footprint. Not its centre:
    /// a footprint anchored on a centre has no integer answer for an even edge,
    /// and the build system stamps obstacles from the min corner.
    pub hq_cell: Cell,
    /// Crystal node cells. At least one.
    pub crystal_nodes: Vec<Cell>,
    /// Gas node cells. At least one.
    pub gas_nodes: Vec<Cell>,
}
```

### `ScenarioSpec` / `Scenario` changes

Append to **both** structs, as the last field:

```rust
/// The RTS block, absent on every phase-0 family.
///
/// `#[serde(default)]` is load-bearing: it is what lets every tracked phase-0
/// `.ron` keep its exact bytes, and therefore its `.sha256` sidecar, across
/// this change. A required field would invalidate five committed scenes and
/// four fixtures at once.
#[serde(default)]
pub rts: Option<RtsSpec>,
```

(The `Scenario` copy is private with the same doc comment and **no** serde
attribute; it is populated in `from_spec` by `doc.rts`.)

Accessor on `Scenario`:

```rust
/// The RTS block, or `None` on every phase-0 family.
pub fn rts(&self) -> Option<&RtsSpec> {
    self.rts.as_ref()
}
```

### New error variant

```rust
#[error("invalid rts block: {0}")]
InvalidRts(String),
```

### Validator changes

In `validate_version_and_dims`, extend the recognised set and dispatch:

```rust
let is_fixture = doc.version.starts_with(FIXTURE_VERSION_PREFIX);
if !is_fixture
    && doc.version != COLLISION_SCENE_V1
    && doc.version != TECHNICAL_PROTOTYPE_V1
    && doc.version != RTS_PROTOTYPE_V1
{
    return Err(ScenarioError::UnsupportedVersion(doc.version.clone()));
}
check_population(doc)?;
if is_fixture { return validate_fixture_dims(doc, cells); }
if doc.version == COLLISION_SCENE_V1 { return validate_collision_scene_dims(doc); }
if doc.version == RTS_PROTOTYPE_V1 { return validate_rts_scene_dims(doc); }
// … existing technical_prototype_v1 locked-constant checks unchanged …
```

In `validate_counts`, add the RTS family to `free_workload`:

```rust
let free_workload = doc.version.starts_with(FIXTURE_VERSION_PREFIX)
    || doc.version == COLLISION_SCENE_V1
    || doc.version == RTS_PROTOTYPE_V1;
```

New function:

```rust
/// Geometry lock for the RTS prototype family.
///
/// The population fields are required to be **exactly zero**: this family is
/// horde-free by construction, and a nonzero count would silently seed a
/// flow-field crowd into a base-building scene. `validate_counts` skips the
/// phase-0 workload lock for this family precisely so this stricter rule can
/// replace it.
fn validate_rts_scene_dims(doc: &ScenarioSpec) -> Result<(), ScenarioError> {
    for (got, want, name) in [
        (doc.width, RTS_WIDTH, "width"),
        (doc.height, RTS_HEIGHT, "height"),
        (doc.cell_size_px, RTS_CELL_PX, "cell_size_px"),
        (doc.sprite_size_px, RTS_SPRITE_PX, "sprite_size_px"),
        (doc.hard_agent_count, 0, "hard_agent_count"),
        (doc.stretch_agent_count, 0, "stretch_agent_count"),
    ] {
        if got != want {
            return Err(ScenarioError::InvalidDimension(format!(
                "{name}: got {got}, want {want}"
            )));
        }
    }
    Ok(())
}
```

New function, called from `from_spec` **after** `blocked` and the reachability
flood are computed (it needs both):

```rust
/// The RTS block is present exactly on the RTS family, and every cell it names
/// is a cell a base could actually use.
fn validate_rts_block(
    doc: &ScenarioSpec,
    blocked: &[bool],
    reachable: &[bool],
) -> Result<(), ScenarioError>;
```

Rules, in this order, each with its own message:

1. `doc.version == RTS_PROTOTYPE_V1` **xor** `doc.rts.is_none()` — i.e. an RTS
   scene without a block, or any other family with one, is
   `InvalidRts("…")`. Message names the version and which side is wrong.
2. `rts.start_crystal <= MAX_START_RESOURCE` and `rts.start_gas <= MAX_START_RESOURCE`.
3. `1 <= rts.start_supply_cap <= MAX_SUPPLY_CAP`.
4. `!rts.crystal_nodes.is_empty()` and `!rts.gas_nodes.is_empty()`;
   each list at most `MAX_RESOURCE_NODES`.
5. Every node cell is in bounds, **not blocked**, and **reachable**.
6. No cell appears twice across the two node lists combined.
7. The HQ footprint — `hq_cell.x .. hq_cell.x + HQ_FOOTPRINT_CELLS` ×
   `hq_cell.y .. hq_cell.y + HQ_FOOTPRINT_CELLS` — is fully in bounds and every
   cell in it is unblocked.
8. No node cell lies inside the HQ footprint.
9. Every spawn cell lies **outside** the HQ footprint (a worker starting inside
   the building it drops off at is an authoring mistake, not a feature).

### Tracked scene: `assets/scenarios/rts_prototype_v1.ron`

Locked field values:

| field | value |
| ----- | ----- |
| `version` | `"rts_prototype_v1"` |
| `width` / `height` | `320` / `320` |
| `cell_size_px` / `sprite_size_px` | `4` / `48` |
| `hard_agent_count` / `stretch_agent_count` | `0` / `0` |
| `seed` | `1743218095512340987` |
| `destination` | `(x: 165, y: 176)` |
| `spawn_cells` | `(162..=167, 178)` — six cells |
| `atlas_count` / `direction_count` / `frame_count` | `4` / `8` / `4` |
| `collision_radius_q8` | `1536` |
| `separation_strength_q8` | `256` |
| `separation_phases` / `mass_class_count` / `separation_threads` | `1` / `1` / `1` |
| `rts.start_crystal` / `rts.start_gas` | `300` / `100` |
| `rts.start_supply_cap` | `10` |
| `rts.hq_cell` | `(x: 160, y: 160)` |
| `rts.crystal_nodes` | `(140,150) (146,146) (152,142) (180,142) (186,146) (192,150) (150,190) (182,190)` |
| `rts.gas_nodes` | `(136,168) (196,168)` |

Obstacles are the deterministic set

```
{ x + y*320 : (x*7 + y*13) % 97 == 0
              and chebyshev((x,y), hq_center=(165,165)) > 28
              and for every node n: chebyshev((x,y), n) > 6
              and (x,y) not in spawn_cells
              and (x,y) != destination }
```

where `chebyshev((ax,ay),(bx,by)) = max(|ax-bx|, |ay-by|)`. That is roughly 1 %
of the grid — scattered single-cell rock, enough to make the flow field do work
without partitioning the map.

RON emission format: mirror `tools/scenegen/gen_collision_scenes.py::render`
exactly (two-space indent, one `(x: N, y: N),` per spawn line, obstacle indices
20 per line). The `rts` block is emitted **last**, before the closing `)`:

```
  rts: Some((
    start_crystal: 300,
    start_gas: 100,
    start_supply_cap: 10,
    hq_cell: (x: 160, y: 160),
    crystal_nodes: [
      (x: 140, y: 150),
      …
    ],
    gas_nodes: [
      (x: 136, y: 168),
      (x: 196, y: 168),
    ],
  )),
```

### Generator: `tools/scenegen/gen_rts_scene.py`

Same shape and docstring style as `gen_collision_scenes.py`: pure stdlib,
deterministic, idempotent, run from the workspace root, writes the `.ron` and a
`.sha256` sidecar containing `hexdigest + "\n"`. Before writing it **asserts**:

- the destination is unblocked,
- every spawn cell is unblocked,
- every spawn cell is reachable from the destination by 8-neighbour BFS over
  unblocked cells,
- the full HQ footprint is unblocked,
- every node cell is unblocked and reachable,
- no node lies inside the HQ footprint,
- `hard_agent_count == 0` (the loader refuses anything else for this family).

Mark it executable (`chmod +x`), like its sibling.

### Testkit

`crates/mmd-engine/src/testkit/fixtures.rs`:

```rust
/// The phase-1 RTS prototype scene, relative to `assets/scenarios/`.
pub const RTS_SCENE: &str = "rts_prototype_v1.ron";

/// Absolute path of the tracked RTS prototype scene.
pub fn rts_scene_path() -> PathBuf { scene_path(RTS_SCENE) }
```

Export both from `crates/mmd-engine/src/testkit/mod.rs`'s existing
`pub use fixtures::{…}` list.

## TDD

1. **Red** — write every test below in
   `crates/mmd-engine/tests/scenario_contract.rs`. Watch them fail.
2. **Green** — implement the validator and generate the scene.
3. **Refactor** — none expected. Keep green.

## Test plan

| Test | Input | Expect |
| ---- | ----- | ------ |
| `rts_scene_loads_verified` | `Scenario::load_verified(testkit::rts_scene_path())` | `Ok`, `version() == "rts_prototype_v1"` |
| `rts_scene_is_horde_free` | the loaded scene | `hard_agent_count() == 0 && stretch_agent_count() == 0` |
| `rts_scene_geometry_is_locked` | the loaded scene | `320, 320, 4, 48` |
| `rts_scene_carries_its_block` | the loaded scene | `rts().is_some()`, `start_crystal == 300`, `start_gas == 100`, `start_supply_cap == 10`, `hq_cell == Cell{x:160,y:160}`, 8 crystal nodes, 2 gas nodes |
| `rts_obstacles_match_the_published_formula` | recompute the formula in the test | the recomputed sorted index set equals `scenario.obstacle_cells()` |
| `hq_footprint_is_free_in_the_tracked_scene` | 144 cells | none is an obstacle |
| `every_phase0_scene_has_no_rts_block` | gate scene, both collision scenes, all four fixtures | `rts().is_none()` for each |
| `phase0_scene_bytes_are_unchanged` | sha256 of each tracked phase-0 `.ron` | equals the value in its committed `.sha256` sidecar — proves the serde default did not force a regeneration |
| `an_rts_scene_without_a_block_is_rejected` | in-memory spec, version rts, `rts: None` | `Err(ScenarioError::InvalidRts(_))` |
| `a_phase0_family_with_an_rts_block_is_rejected` | fixture spec + `rts: Some(..)` | `Err(ScenarioError::InvalidRts(_))` |
| `a_nonzero_population_is_rejected_for_the_rts_family` | rts spec, `hard_agent_count: 1` | `Err(ScenarioError::InvalidDimension(_))` naming `hard_agent_count` |
| `a_nonzero_stretch_is_rejected_for_the_rts_family` | rts spec, `stretch_agent_count: 1` | same, naming `stretch_agent_count` |
| `an_empty_node_list_is_rejected` | rts spec, `crystal_nodes: vec![]` | `Err(InvalidRts(_))` |
| `a_blocked_node_is_rejected` | rts spec with a node index also in `obstacle_cells` | `Err(InvalidRts(_))` |
| `an_unreachable_node_is_rejected` | rts spec, node walled off by a full obstacle ring | `Err(InvalidRts(_))` |
| `a_duplicate_node_across_kinds_is_rejected` | same cell in both lists | `Err(InvalidRts(_))` |
| `a_blocked_hq_footprint_is_rejected` | one obstacle inside the footprint | `Err(InvalidRts(_))` |
| `an_out_of_bounds_hq_footprint_is_rejected` | `hq_cell = (315, 315)` on a 320 grid | `Err(InvalidRts(_))` |
| `a_node_inside_the_hq_footprint_is_rejected` | node at `(163, 163)` | `Err(InvalidRts(_))` |
| `a_spawn_inside_the_hq_footprint_is_rejected` | spawn at `(163, 163)` | `Err(InvalidRts(_))` |
| `a_supply_cap_over_the_pillar_is_rejected` | `start_supply_cap: 501` | `Err(InvalidRts(_))` |
| `a_zero_supply_cap_is_rejected` | `start_supply_cap: 0` | `Err(InvalidRts(_))` |
| `an_over_generous_start_stock_is_rejected` | `start_crystal: 2_001` | `Err(InvalidRts(_))` |
| `the_renderer_contract_still_binds_the_rts_family` | rts spec, `atlas_count: 5` | `Err(ScenarioError::InvalidDimension(_))` |
| `the_generator_is_idempotent` (shell, in Validation) | rerun the python script | `git status --porcelain` empty |

**Mutation verification (mandatory).** Inject, confirm red, revert, confirm green:
1. Drop `#[serde(default)]` → kills `phase0_scene_bytes_are_unchanged` (every phase-0 load fails to parse).
2. Add `RTS_PROTOTYPE_V1` to `free_workload` but *skip* `validate_rts_scene_dims`'s zero check → kills both nonzero-population tests.
3. Invert the xor in rule 1 → kills `an_rts_scene_without_a_block_is_rejected` and `a_phase0_family_with_an_rts_block_is_rejected`.
4. Use `<` instead of `<=` for `MAX_SUPPLY_CAP` → confirm the `501` test still passes and add a boundary case at exactly `500` that must be `Ok`.
5. Skip the reachability check for nodes → kills `an_unreachable_node_is_rejected`.
6. Shift the obstacle formula's modulus to `98` → kills `rts_obstacles_match_the_published_formula`.

## Impl steps

- [x] 1. Add the constants block, `RtsSpec`, and `ScenarioError::InvalidRts` to `crates/mmd-engine/src/scenario.rs`.
- [x] 2. Add the `#[serde(default)] pub rts: Option<RtsSpec>` field to `ScenarioSpec` and the private mirror to `Scenario`; populate it in `from_spec`; add `Scenario::rts()`.
- [x] 3. Extend the recognised-version set and the dispatcher in `validate_version_and_dims`.
- [x] 4. Add `RTS_PROTOTYPE_V1` to `free_workload` in `validate_counts`.
- [x] 5. Write `validate_rts_scene_dims` exactly as quoted.
- [x] 6. Write `validate_rts_block` with the nine rules in order.
- [x] 7. Call `validate_rts_block(&doc, &blocked, &reachable)?` in `from_spec`, after the reachability flood and before the `Ok(Self { .. })`.
- [x] 8. Add `RTS_SCENE` and `rts_scene_path()` to `crates/mmd-engine/src/testkit/fixtures.rs` and export them from `testkit/mod.rs`.
- [x] 9. Write every failing test from the table into `crates/mmd-engine/tests/scenario_contract.rs`.
- [x] 10. Run `cargo test -p mmd-engine --test scenario_contract` and record the failures.
- [x] 11. Create `tools/scenegen/gen_rts_scene.py` with the asserts listed above; `chmod +x` it.
- [x] 12. Run `python3 tools/scenegen/gen_rts_scene.py` from the workspace root.
- [x] 13. `git add assets/scenarios/rts_prototype_v1.ron assets/scenarios/rts_prototype_v1.sha256`.
- [x] 14. Confirm `git status --porcelain assets/scenarios/` lists **only** the two new files.
- [x] 15. Run the mutation list; record kills in the commit body.
- [x] 16. Run the full validation block.

## Outputs

- **Files created**
  - `tools/scenegen/gen_rts_scene.py`
  - `assets/scenarios/rts_prototype_v1.ron`
  - `assets/scenarios/rts_prototype_v1.sha256`
- **Files edited**
  - `crates/mmd-engine/src/scenario.rs`
  - `crates/mmd-engine/src/testkit/fixtures.rs`
  - `crates/mmd-engine/src/testkit/mod.rs`
  - `crates/mmd-engine/tests/scenario_contract.rs`
- **Public API added:** `scenario::{RTS_PROTOTYPE_V1, RtsSpec, HQ_FOOTPRINT_CELLS, DEPOT_FOOTPRINT_CELLS, BARRACKS_FOOTPRINT_CELLS, MAX_START_RESOURCE, MAX_SUPPLY_CAP, MAX_RESOURCE_NODES}`, `Scenario::rts`, `ScenarioError::InvalidRts`, `testkit::{RTS_SCENE, rts_scene_path}`.
- **Behaviour change:** a fourth scenario family loads. No existing scene's bytes, hash or validation outcome changes.
- **Migration / config:** none.

## Validation

- [x] `cargo fmt --all -- --check`
- [x] `cargo test -p mmd-engine --test scenario_contract` — all green
- [x] `MMD_REQUIRE_GPU=1 cargo test --workspace --locked`
- [x] `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- [x] `nix flake check`
- [x] `python3 tools/scenegen/gen_rts_scene.py && git status --porcelain` — **empty output** (verified via sha256 stability across reruns; the new `.ron`/`.sha256` are untracked at this stage of the workflow so `git status --porcelain` reports them `??` regardless of content, but the digest is byte-identical run to run)
- [x] `python3 tools/scenegen/gen_collision_scenes.py && git status --porcelain` — **empty output** (the sibling generator still reproduces its scenes byte-for-byte; `git diff --stat` on the two tracked collision `.ron` files is empty)
- [x] `sha256sum -c <(sed 's|$|  assets/scenarios/rts_prototype_v1.ron|' assets/scenarios/rts_prototype_v1.sha256)` — `OK`
- [x] `cargo run -- run --agents 5000 --frames 300` — exit 0, exit-line `hash=864147ca...` verified bit-identical against the pre-change worktree (git stash before/after)
- [x] `cargo run -- run --scenario assets/scenarios/collision_mid_v1.ron --frames 300` — exit 0
- [x] `cargo run -- run --scenario assets/scenarios/rts_prototype_v1.ron --frames 3` — fails with a clear message (`--agents 0 is not a runnable scene` path); this is expected and is what T14's `rts` subcommand replaces
- [x] app functional — no broken path from this slice
- [x] commit msg draft: `feat(scenario): add the horde-free rts_prototype_v1 family`
