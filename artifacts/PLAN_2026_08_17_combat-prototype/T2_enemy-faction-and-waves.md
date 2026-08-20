# T2: Enemy faction and waves

**Plan:** `./artifacts/PLAN_2026_08_17_combat-prototype.md`
**Depends:** T1
**Commit outcome:** `UnitKind::Ghoul` + `OWNER_ENEMY` exist; scenario schema carries optional enemy block (pre-placed, spawn points, timed waves); wave spawner system spawns them deterministically; every existing scene byte-identical and gate green.

## Context (self-contained)

- Goal: Phase 2 Combat Prototype — weapons, damage, turrets, enemy AI. Success = scripted combat run + exit tokens.
- This slice: enemies exist and appear. They do NOT move or fight yet (march AI + weapons = T3). Spawned enemies stand `Idle`.
- Out of scope here: targeting/attack (T3), commands (T4), turret (T5), bars (T6), editing `assets/scenarios/rts_prototype_v1.ron` (T7 — schema here must keep old files valid + byte-identical). `sim/` frozen.
- Assumptions in force: `OWNER_ENEMY = 1`; enemies no supply/cost, never gather/build/produce, never player-producible; Ghoul body radius 3.0, speed 18 c/s (speed lands in T3 with movement — this ticket compiles the `unit_speed` arm as `0.0`); finite wave list; validator caps total enemies ≤ 1200; spawn deferred bounded when store full.

### Assumptions updated by detailer (codebase inspected 2026-08-17, branch `plan/combat-prototype`)

1. **Fixture version string.** `validate_rts_block` (`crates/mmd-engine/src/scenario.rs:646`) enforces *rts block present iff `version == "rts_prototype_v1"`*, and the `fixture_*` version family therefore **rejects** an rts block. The new tracked fixture keeps the agreed **filename** `assets/scenarios/fixtures/fixture_rts_combat_v1.ron` but its `version:` field is `"rts_prototype_v1"`. It is **not** added to `testkit::ALL_FIXTURES` — that list feeds the phase-0 flow-field `Harness` suites (`crates/mmd-engine/tests/flow_field.rs:248`, `simulation.rs:499`, `harness.rs:419`), which need `hard_agent_count > 0`.
2. **Drag filter already exists.** `box_select` (`crates/mmd-engine/src/rts/selection.rs`, last fn in file) already skips `owner(slot) != OWNER_PLAYER`, and `pick_at` already gates units/buildings on `owner == OWNER_PLAYER`. This ticket adds **no selection code**, only the regression test `drag_box_excludes_enemies`.
3. **No duplicated enemy vecs in `RtsWorld`.** The world already owns its `Scenario` (`self.scenario`); `WaveSpec` and `Cell` are `Copy`, so the spawner reads wave data straight out of `self.scenario.rts().and_then(|r| r.enemies.as_ref())` and copies scalars out before mutating. Only three scalar fields are added (`enemy_wave_cursor`, `enemy_wave_pending`, `enemies_spawned`). Zero per-tick allocation holds trivially; no "preallocated fixed vecs" copy is made (the earlier ticket wording is superseded — the data already lives in the world).
4. **Adding `UnitKind::Ghoul` breaks eight exhaustive matches** outside this ticket's obvious files. Decided arms (all in Impl step 4): `supply_cost` → `0` (true: enemies cost no supply); `unit_speed` → `0.0` (T3 owns the real 18 c/s; an Idle Ghoul never moves); `unit_cost` / `produce_ticks` → `unreachable!("Ghoul is not producible")` (`can_produce` already rejects every `(building, Ghoul)` pair, so a reachable value here would be a lie); `pack::unit_slot` → `SLOT_RTS_SOLDIER` (enemy art is out of scope; soldier sheet renders it); `hud::kind_label` → `"GHOUL"`; `hud::portrait_source` → soldier sheet; HUD detail line → `"IDLE"`; HUD queue letter → `"G"` (render code stays total, never panics).
5. **`RtsSpec` struct literals.** `enemies` is a new struct field; `#[serde(default)]` only covers deserialization, so all **9** `RtsSpec { … }` literal sites in tests/helpers gain `enemies: None,` (exact list in Impl step 1.4).
6. **Wave semantics.** `at_tick` compares against the post-increment tick counter (`tick_index` is incremented at the top of `RtsWorld::tick`, so the first tick after construction is tick 1; `RtsHarness::step_exact(50)` lands exactly on `at_tick: 50`). Waves drain strictly in list order: a deferred wave delays later waves (bounded FIFO, no queue allocation). Equal `at_tick` on consecutive waves is legal (sorted = non-decreasing); `count: 0` is rejected by the validator.
7. **Validator additions beyond the brief:** enemy cells must also be *reachable* from `destination` (same flood rule resource nodes already obey at `scenario.rs:701`) — prevents T3's march from starting inside a sealed pocket; and `count >= 1` per wave.
8. **`enemies_spawned`** counts cumulative Ghouls ever spawned (pre-placed + waves), never decremented on death. It is **not** fed into `state_hash` — every live enemy already hashes through the entity store (`entity.rs::hash_into` covers kind tag + owner byte), and the repo hashes observable frame endpoints only (commit `a82a91e`). It is the observability seam T7's exit token reads.
9. **Pre-placed spawn failure is fatal at construction** (`RtsWorldError::StoreFull` / `NoFreeUnitPosition`), matching worker seeding at `world.rs:668-683`. Bounded deferral is a **wave-spawner-only** behaviour.

## Requirements

- `UnitKind::Ghoul = 2` appended (discriminants appended, never inserted — `EntityKind::tag()` gives `0x10 | 2 = 0x12`). `body_radius_cells()` → 3.0 (exhaustive match forces it). Max HP 30, armor 0 in T1's kind tables.
- `pub const OWNER_ENEMY: u8 = 1;` in `entity.rs` beside `OWNER_PLAYER = 0` / `OWNER_NEUTRAL = 255` (`entity.rs:17-20`), re-exported from `rts/mod.rs`.
- Scenario schema (`crates/mmd-engine/src/scenario.rs`, inside `RtsSpec` at line 121): `#[serde(default)] pub enemies: Option<EnemySpec>` with
  ```rust
  pub struct EnemySpec {
      pub pre_placed: Vec<Cell>,          // Ghoul positions at tick 0
      pub spawn_points: Vec<Cell>,        // wave origins
      pub waves: Vec<WaveSpec>,
  }
  pub struct WaveSpec { pub at_tick: u32, pub count: u32, pub spawn_point: u8 }
  ```
  Old RON files without the field parse identically → sha256 sidecars untouched.
- Validation (`validate_rts_block`, `scenario.rs:640`): every enemy cell in bounds + unblocked + reachable + off resource-node cells + off the HQ footprint; `spawn_point` index < `spawn_points.len()`; `count >= 1`; `pre_placed.len() + Σ waves.count ≤ MAX_ENEMIES = 1_200`; waves sorted non-decreasing by `at_tick` (reject unsorted — determinism readability). All errors are `ScenarioError::InvalidRts(String)` like the rest of the block.
- Wave spawner system in `RtsWorld::tick` after camera, before construction (doc-comment order renumbered — construction becomes System 4 etc.): at `tick_index == at_tick`, spawn `count` Ghouls owner `OWNER_ENEMY` around the spawn point — deterministic placement via `nearest_free_body_center` (`world.rs:469`), the one planner behind every body placement, ascending-tie-break to lower flat cell index. Store full / no legal cell → remainder deferred to next tick (bounded: carries `enemy_wave_pending`, no allocation).
- Pre-placed enemies spawn at world construction (`from_scenario`, after worker seeding), same legality search, failure fatal.
- Supply recount (`world.rs:2068`) filters `owner == OWNER_PLAYER` — enemies never enter `Supply::used`. Belt-and-braces: `supply_cost(Ghoul) = 0`.
- Drag-box selection excludes `OWNER_ENEMY` — already true in code (see Assumption 2); pinned by test `drag_box_excludes_enemies`.
- Spawn counter `enemies_spawned: u32` on `RtsWorld` + `pub fn enemies_spawned(&self) -> u32` — T7's exit token reads it.
- No per-frame allocation: spawner reuses `body_scratch` / `live_scratch` (both reserved to `MAX_ENTITIES` at construction) and copies `Copy` scalars out of the owned scenario.

## Inputs

- **From T1 (verbatim, T1 lands first):** `RtsWorld::apply_damage(&mut self, target: EntityId, damage: u32) -> DamageResult`; HP column + kind tables `max_hp(kind)`, `armor(kind)` in `entity.rs` — this ticket adds the Ghoul 30/0 arms. (T2 never calls `apply_damage`; it only extends the tables. If T1's landed table signature differs from `pub fn max_hp(kind: EntityKind) -> u32`, add the Ghoul 30/0 entries to whatever exhaustive kind table T1 landed — the arm value, not the signature, is this ticket's contract.)
- `crates/mmd-engine/src/scenario.rs` — `Cell` (line 118, `Copy`), `RtsSpec` (121: `start_crystal`, `start_gas`, `start_supply_cap`, `hq_cell`, `crystal_nodes`, `gas_nodes`; derives `Debug, Clone, PartialEq, Eq, Deserialize`), `ScenarioSpec.rts` with the load-bearing `#[serde(default)]` comment (≈207), `ScenarioError::InvalidRts` (≈232), `Scenario::load_verified` sidecar verify (≈254, reads `<name>.sha256`, `expected.trim()`), `validate_rts_block(doc, blocked, reachable)` (≈640; already computes `all_nodes: Vec<&Cell>` and the `in_hq_footprint` closure this ticket reuses), `MAX_RESOURCE_NODES` (115, neighbour for the new `MAX_ENEMIES`), `cell_index` helper (≈905).
- `crates/mmd-engine/src/rts/entity.rs` — owner consts (17-20), `UnitKind` (29: `Worker = 0`, `Soldier = 1`), `body_radius_cells` exhaustive match (36-45), `EntityKind::tag` (`0x10 | k`, ≈72-78), `EntityStore::spawn` (testkit/`pub(crate)` twin pair, ≈180), `hash_into` covers kind tag + owner byte (≈420).
- `crates/mmd-engine/src/rts/world.rs` — `nearest_free_body_center(static_nav, placed, ignore, exclude, preferred)` free fn (469: legality = `center_blocked` + body-diameter clearance vs `placed` + same connected region, ties to lower flat cell index); `from_scenario` (625: HQ → nodes → `StaticNav::new` → worker seeding loop with `placed: Vec<[f32;2]>` at 668-683 → resources/supply → `FieldPool` → `Ok(Self{…}` at 726); `RtsWorld` struct fields (193-292, scratch buffers all reserved to `MAX_ENTITIES`); getters block (≈950); `tick` (1730-1741) + system doc labels (`camera_system` 1742/"System 2", `construction` 1757/"System 3", `production_system` 1972/"System 4", `supply_recount` 2065/"System 7", `gather` 2080/"System 5", `movement` 2220/"System 6"); production's spawn pattern incl. `self.live_scratch.push(id.index as usize)` (≈2030-2045); `collect_unit_bodies_into_scratch` (2052); `supply_recount` (2068); `RtsWorldError::{StoreFull, NoFreeUnitPosition}`; imports block (22-25 for entity consts). Idle units are already hard bodies (`collect_unit_bodies` takes every `Unit(_)`) and `step_one_unit` moves only `Move`/`Gather` orders — Ghouls need no movement code.
- Exhaustive `UnitKind` matches to extend: `crates/mmd-engine/src/rts/economy.rs:120-124` (`supply_cost`), `orders.rs:23-29` (`unit_speed`), `production.rs:28-33` (`unit_cost`) + `:40-45` (`produce_ticks`) + `can_produce` (:52, needs **no** change — tuple `matches!` already excludes Ghoul), `pack.rs:104-109` (`unit_slot`), `hud.rs:652-662` (`kind_label`), `:905-925` (`portrait_source`), `:977-981` (Soldier "IDLE" detail arm), `:1024-1029` (queue letter). `hud.rs:724` selection tally has a `_ => disqualified = true` catch-all — no change.
- `crates/mmd-engine/src/rts/mod.rs` — `pub use entity::{…}` block to gain `OWNER_ENEMY`.
- `crates/mmd-engine/src/testkit/fixtures.rs` — `FIXTURE_DIR = "assets/scenarios/fixtures"`, `fixture_path(name)`, `ALL_FIXTURES` (do **not** extend); `testkit/mod.rs` re-export list. `testkit/rts.rs` — `RtsHarness::{path, spec, step_exact, world, world_mut, state_hash, ids_of_kind, tick_index}`.
- Sidecar format: 64 lowercase hex chars + `\n` (verified via `xxd` on `fixture_small_v1.sha256`); no xtask subcommand exists for scenario sidecars — generate with `sha256sum … | cut -c1-64`.
- Test seams: `crates/mmd-engine/tests/scenario_contract.rs` — `rts_spec()` helper (1024), `expect_invalid_rts(spec, needle)` (1056); `rts_world.rs` — `kind_tags_are_distinct` (174-187, asserts 7 distinct tags — becomes 8); `rts_selection.rs:19` — `view()`/`Camera::new(w, h, 4.0, [1920.0, 1080.0], center).iso_view()` pattern for box tests. `RtsSpec { … }` literal sites needing `enemies: None,`: see Impl step 1.4.

## TDD

1. **Red** — write the failing tests of step 7 first (they compile only after steps 1-3 add the types, so in practice: land step 1-4 skeletons, then tests red against missing world behaviour of step 5-6).
2. **Green** — min code.
3. **Refactor** — keep green.

## Test plan

| Test (exact name) | File | Input | Expect |
| ---- | ---- | ----- | ------ |
| `ghoul_kind_tags_stably` | `crates/mmd-engine/tests/rts_enemy.rs` | `EntityKind::Unit(UnitKind::Ghoul).tag()` | `== 0x12`; `UnitKind::Ghoul.body_radius_cells() == 3.0` |
| `kind_tags_are_distinct` (update) | `crates/mmd-engine/tests/rts_world.rs:174` | 8-kind array incl. Ghoul | `tags.len() == 8` |
| `enemy_block_optional_old_scenes_parse` | `crates/mmd-engine/tests/scenario_contract.rs` | `Scenario::load_verified(rts_scene_path())` | loads (sidecar verify passes ⇒ bytes untouched), `scene.rts().unwrap().enemies.is_none()` |
| `enemy_baseline_block_is_valid` | `scenario_contract.rs` | `combat_rts_spec()` | `Scenario::from_spec` Ok (guards the negatives) |
| `enemy_spec_validates_cells_and_totals` | `scenario_contract.rs` | 9 single-field mutations | each `Err(ScenarioError::InvalidRts(msg))` with needles: `"out of bounds"`, `"blocked"`, `"unreachable"`, `"resource node"`, `"HQ footprint"`, `"spawn_point"`, `"count"`, `"sorted"`, `"exceeds"` |
| `pre_placed_ghouls_spawn_at_start` | `rts_enemy.rs` | tracked fixture, tick 0 | 2 live Ghouls, owner `OWNER_ENEMY`, order `Idle`, positions exactly `[20.5, 20.5]` / `[26.5, 20.5]` (ascending slot), `enemies_spawned() == 2` |
| `wave_spawns_at_exact_tick` | `rts_enemy.rs` | fixture wave `at_tick=50, count=8` | after `step_exact(49)`: 2 Ghouls; after 1 more tick: 10 Ghouls, all owner 1, `enemies_spawned() == 10` |
| `wave_defers_when_store_full` | `rts_enemy.rs` | in-memory spec, store filled to `MAX_ENTITIES - 3` with neutral nodes | wave tick spawns 3, `enemies_spawned() == 3`; after freeing 5 slots + 1 tick: 8, none lost, no panic |
| `enemies_never_consume_supply` | `rts_enemy.rs` | fixture through tick 60 | `Supply::used` stays 2 (the two seeded workers) |
| `drag_box_excludes_enemies` | `rts_enemy.rs` | worker + ghoul parked in one screen box | selection = the 2 workers only |
| `spawn_determinism` | `rts_enemy.rs` | two fresh fixture runs, `step_exact(200)` | equal `state_hash()`, both `enemies_spawned() == 14` |

Run: `cargo test -p mmd-engine --test rts_enemy`, `cargo test -p mmd-engine --test scenario_contract`, `cargo test -p mmd-engine --test rts_world`.

## Impl steps

- [ ] 1. Scenario schema: `EnemySpec`/`WaveSpec` + optional field, old bytes untouched
  - [ ] 1.1 `crates/mmd-engine/src/scenario.rs` — directly **after** the `RtsSpec` struct (after its closing `}` near line 143), insert:
    ```rust
    /// Scripted enemy content of an RTS scene: pre-placed Ghouls, wave
    /// origins and a finite timed wave list. Validated by
    /// [`validate_rts_block`]; capped by [`MAX_ENEMIES`].
    #[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
    pub struct EnemySpec {
        /// Ghoul positions seeded at world construction (tick 0).
        pub pre_placed: Vec<Cell>,
        /// Wave origins, indexed by [`WaveSpec::spawn_point`].
        pub spawn_points: Vec<Cell>,
        /// Timed waves, sorted non-decreasing by `at_tick`.
        pub waves: Vec<WaveSpec>,
    }

    /// One timed enemy wave.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
    pub struct WaveSpec {
        /// Tick the wave fires on, compared against the post-increment tick
        /// counter: the first tick after construction is tick 1.
        pub at_tick: u32,
        /// Ghouls in the wave. At least 1.
        pub count: u32,
        /// Index into [`EnemySpec::spawn_points`].
        pub spawn_point: u8,
    }
    ```
  - [ ] 1.2 Same file — inside `RtsSpec`, after the `pub gas_nodes: Vec<Cell>,` field, add:
    ```rust
    /// Scripted enemy content. Absent on every scene shipped before combat.
    ///
    /// `#[serde(default)]` is load-bearing: it is what keeps every tracked
    /// RTS `.ron` byte-identical, and therefore its `.sha256` sidecar valid,
    /// across this change.
    #[serde(default)]
    pub enemies: Option<EnemySpec>,
    ```
  - [ ] 1.3 Same file — directly after `pub const MAX_RESOURCE_NODES: usize = 64;` (line ≈115), add:
    ```rust
    /// Most enemies a scene may script in total: pre-placed plus every wave.
    ///
    /// Chosen under the 2 048-entity store with room left for the 500-supply
    /// player army, its buildings and the scene's nodes; the wave spawner's
    /// bounded deferral absorbs a store that is momentarily fuller.
    pub const MAX_ENEMIES: u32 = 1_200;
    ```
  - [ ] 1.4 Add `enemies: None,` as the last field of every existing `RtsSpec { … }` literal (9 sites, each is one line after its `gas_nodes: …,` line):
    - `crates/mmd-engine/src/rts/world.rs:3352`
    - `crates/mmd-engine/tests/common/mod.rs:417`
    - `crates/mmd-engine/tests/rts_formation.rs:45`
    - `crates/mmd-engine/tests/rts_radius_nav.rs:59`
    - `crates/mmd-engine/tests/rts_world.rs:552`
    - `crates/mmd-engine/tests/scenario_contract.rs:1045`
    - `crates/mmd-engine/tests/scenario_contract.rs:1282`
    - `crates/mmd-engine/tests/rts_collision.rs:62`
    - `crates/mmd-engine/tests/rts_collision.rs:1762`
    (Line numbers pre-T1; re-locate with `grep -rn "rts: Some(RtsSpec" crates` if T1 shifted them.)

- [ ] 2. Validation rules in `validate_rts_block`
  - [ ] 2.1 `crates/mmd-engine/src/scenario.rs`, in `validate_rts_block`, immediately **before** the final `Ok(())` (after the spawn-in-HQ-footprint loop, ≈line 738 — the `all_nodes` vec and `in_hq_footprint` closure defined above are still in scope), insert:
    ```rust
    // The enemy block: every cell it names must be ground an enemy could
    // actually stand on and walk out of, and the whole scripted invasion
    // must stay under the entity budget.
    if let Some(enemies) = rts.enemies.as_ref() {
        let enemy_cell_ok = |cell: &Cell, what: &str| -> Result<(), ScenarioError> {
            let idx = cell_index(*cell, doc.width, doc.height).ok_or_else(|| {
                ScenarioError::InvalidRts(format!(
                    "{what} ({}, {}) is out of bounds",
                    cell.x, cell.y
                ))
            })?;
            if blocked[idx] {
                return Err(ScenarioError::InvalidRts(format!(
                    "{what} ({}, {}) is blocked",
                    cell.x, cell.y
                )));
            }
            if !reachable[idx] {
                return Err(ScenarioError::InvalidRts(format!(
                    "{what} ({}, {}) is unreachable from the destination",
                    cell.x, cell.y
                )));
            }
            if in_hq_footprint(*cell) {
                return Err(ScenarioError::InvalidRts(format!(
                    "{what} ({}, {}) lies inside the HQ footprint",
                    cell.x, cell.y
                )));
            }
            if all_nodes.iter().any(|n| **n == *cell) {
                return Err(ScenarioError::InvalidRts(format!(
                    "{what} ({}, {}) sits on a resource node",
                    cell.x, cell.y
                )));
            }
            Ok(())
        };
        for c in &enemies.pre_placed {
            enemy_cell_ok(c, "enemy pre_placed cell")?;
        }
        for c in &enemies.spawn_points {
            enemy_cell_ok(c, "enemy spawn_point cell")?;
        }

        let mut total = enemies.pre_placed.len() as u64;
        let mut prev_tick: Option<u32> = None;
        for (i, w) in enemies.waves.iter().enumerate() {
            if w.count == 0 {
                return Err(ScenarioError::InvalidRts(format!(
                    "wave {i}: count must be >= 1"
                )));
            }
            if (w.spawn_point as usize) >= enemies.spawn_points.len() {
                return Err(ScenarioError::InvalidRts(format!(
                    "wave {i}: spawn_point {} out of range ({} spawn points)",
                    w.spawn_point,
                    enemies.spawn_points.len()
                )));
            }
            if prev_tick.is_some_and(|p| w.at_tick < p) {
                return Err(ScenarioError::InvalidRts(format!(
                    "wave {i}: at_tick {} is not sorted non-decreasing",
                    w.at_tick
                )));
            }
            prev_tick = Some(w.at_tick);
            total += u64::from(w.count);
        }
        if total > u64::from(MAX_ENEMIES) {
            return Err(ScenarioError::InvalidRts(format!(
                "enemy total {total} exceeds cap {MAX_ENEMIES}"
            )));
        }
    }
    ```
    (No presence rule needed: `enemies` nests inside `RtsSpec`, and the existing `is_rts != doc.rts.is_some()` check already confines it to the RTS family.)

- [ ] 3. Kind + owner + stat-table arms in `entity.rs`
  - [ ] 3.1 `crates/mmd-engine/src/rts/entity.rs` — between `pub const OWNER_PLAYER: u8 = 0;` and the `OWNER_NEUTRAL` doc line (17-19), insert:
    ```rust
    /// Owner id of the enemy faction.
    pub const OWNER_ENEMY: u8 = 1;
    ```
  - [ ] 3.2 Same file — in `enum UnitKind` (line 29), append after `Soldier = 1,`:
    ```rust
    /// Phase-2 melee enemy. Never player-producible, never supply-counted.
    Ghoul = 2,
    ```
    and in `body_radius_cells` (line 36) append the arm `Self::Ghoul => RTS_UNIT_BODY_RADIUS_CELLS,` after the `Soldier` arm.
  - [ ] 3.3 Same file — in T1's stat tables, add the Ghoul arms: in `max_hp` the arm for `UnitKind::Ghoul` → `30`, in `armor` → `0` (exact match-shape follows T1's landed tables; the values 30/0 are the contract).
  - [ ] 3.4 `crates/mmd-engine/src/rts/mod.rs` — in the `pub use entity::{…}` block, add `OWNER_ENEMY,` (keep alphabetical position: between `MAX_ENTITIES,` and `OWNER_NEUTRAL,`).

- [ ] 4. New exhaustive-match arms across the crate (each one edit, one file)
  - [ ] 4.1 `crates/mmd-engine/src/rts/economy.rs`, `supply_cost` (line 120) — append arm:
    ```rust
    // Enemies never enter Supply::used; the recount also filters by owner.
    UnitKind::Ghoul => 0,
    ```
  - [ ] 4.2 `crates/mmd-engine/src/rts/orders.rs`, `unit_speed` (line 24) — append arm:
    ```rust
    // T3's march AI owns the real Ghoul speed; until then a Ghoul only
    // ever stands Idle, and an Idle unit is never stepped.
    UnitKind::Ghoul => 0.0,
    ```
  - [ ] 4.3 `crates/mmd-engine/src/rts/production.rs` — append to `unit_cost` (line 28): `UnitKind::Ghoul => unreachable!("Ghoul is not producible; can_produce rejects it"),` and to `produce_ticks` (line 40): `UnitKind::Ghoul => unreachable!("Ghoul is not producible; can_produce rejects it"),`. (`can_produce` at line 52 needs no change — its tuple `matches!` already returns `false` for Ghoul.)
  - [ ] 4.4 `crates/mmd-engine/src/rts/pack.rs`, `unit_slot` (line 104) — append arm:
    ```rust
    // Enemy art is out of this slice's scope: the Ghoul draws from the
    // soldier sheet until a dedicated sheet exists.
    UnitKind::Ghoul => SLOT_RTS_SOLDIER,
    ```
  - [ ] 4.5 `crates/mmd-engine/src/rts/hud.rs`, `kind_label` (line 652) — after the Soldier arm add `EntityKind::Unit(UnitKind::Ghoul) => "GHOUL",`.
  - [ ] 4.6 Same file, `portrait_source` (line ≈905) — after the Soldier arm add:
    ```rust
    EntityKind::Unit(UnitKind::Ghoul) => (
        PortraitTarget::Soldier,
        frame_uv_rect(PORTRAIT_DIR, PORTRAIT_FRAME),
    ),
    ```
  - [ ] 4.7 Same file, `push_detail_text` (line ≈977) — after the `EntityKind::Unit(UnitKind::Soldier)` arm's closing `}` add:
    ```rust
    EntityKind::Unit(UnitKind::Ghoul) => {
        push_text(font, "IDLE", [x, y], PANEL_TEXT_SCALE, TEXT_TINT);
    }
    ```
    (Unreachable while selection excludes enemies; HUD code stays total anyway.)
  - [ ] 4.8 Same file, queue-letter match (line ≈1024, `let label = match kind {`) — append `UnitKind::Ghoul => "G",`.

- [ ] 5. World: construction seeding, wave system, counter, supply filter (`crates/mmd-engine/src/rts/world.rs`)
  - [ ] 5.1 Imports (line 22-25): in `use super::entity::{…}` add `OWNER_ENEMY,` (between `OWNER_NEUTRAL,` and `OWNER_PLAYER,` alphabetically: `MAX_ENTITIES, OWNER_ENEMY, OWNER_NEUTRAL, OWNER_PLAYER,`).
  - [ ] 5.2 `RtsWorld` struct (fields end ≈line 290) — after the `evac_to: Vec<[f32; 2]>,` field, insert:
    ```rust
    /// Index of the first scenario enemy wave not yet fully spawned.
    enemy_wave_cursor: usize,
    /// Ghouls the wave at [`Self::enemy_wave_cursor`] still owes the world —
    /// nonzero exactly while a wave is mid-deferral (store full, or no legal
    /// free centre). Bounded state, not a queue: waves drain strictly in
    /// list order, so one pending count is the whole backlog.
    enemy_wave_pending: u32,
    /// Cumulative Ghouls ever spawned (pre-placed + waves). Never
    /// decremented on death — the combat gate's exit token reads it as a
    /// spawn odometer, not a head-count.
    enemies_spawned: u32,
    ```
  - [ ] 5.3 `from_scenario` — after the worker-seeding `for c in scenario.spawn_cells() { … }` loop (ends ≈line 683, `placed` and `static_nav` still in scope) and **before** the `let resources = …` line, insert:
    ```rust
    // 5. Pre-placed Ghouls, in scenario order, through the same body-safe
    //    search the workers used, against the bodies already seeded.
    //    Enemies charge no supply. Failure is fatal here, exactly as it is
    //    for a worker: a scene that cannot seed its own script is broken,
    //    and the wave spawner's bounded deferral is a runtime behaviour,
    //    not a construction one.
    let mut enemies_spawned: u32 = 0;
    if let Some(enemies) = rts.enemies.as_ref() {
        for c in &enemies.pre_placed {
            let preferred = [c.x as f32 + 0.5, c.y as f32 + 0.5];
            let pos = nearest_free_body_center(&static_nav, &placed, None, None, preferred)
                .ok_or(RtsWorldError::NoFreeUnitPosition)?;
            entities
                .spawn(EntityKind::Unit(UnitKind::Ghoul), OWNER_ENEMY, pos)
                .ok_or_else(|| RtsWorldError::StoreFull {
                    what: "ghoul".to_string(),
                })?;
            placed.push(pos);
            enemies_spawned += 1;
        }
    }
    ```
    Then in the `Ok(Self { … })` literal (≈line 726) add, after `evac_to: Vec::with_capacity(MAX_ENTITIES),`:
    ```rust
    enemy_wave_cursor: 0,
    enemy_wave_pending: 0,
    enemies_spawned,
    ```
    Note: `placed` is currently declared `let mut placed` and stays so; if T1 left `worker_count` warnings-clean this compiles as-is.
  - [ ] 5.4 Getters block (after `pub fn tick_index` ≈line 954) — add:
    ```rust
    /// Cumulative Ghouls ever spawned (pre-placed + waves). Never
    /// decremented on death.
    pub fn enemies_spawned(&self) -> u32 {
        self.enemies_spawned
    }
    ```
  - [ ] 5.5 New system fn — insert between `camera_system` (ends ≈line 1756) and `construction`:
    ```rust
    /// System 3: spawn scheduled enemy waves at their exact tick.
    ///
    /// Waves drain strictly in list order: [`Self::enemy_wave_cursor`] names
    /// the first wave not fully spawned, [`Self::enemy_wave_pending`] how
    /// many of its Ghouls still owe the world a body. Placement is
    /// [`nearest_free_body_center`] around the wave's spawn point — the same
    /// deterministic planner every other body placement uses — with each
    /// spawn's centre added to the obstacle set before the next. A spawn
    /// that cannot be honoured this tick (no legal free centre anywhere, or
    /// the store is full) leaves the remainder pending and is retried next
    /// tick: a scheduled enemy is deferred, never dropped.
    fn enemy_wave_system(&mut self) {
        loop {
            // Copy the current wave and its origin out (`WaveSpec` and
            // `Cell` are `Copy`) so no borrow of the owned scenario
            // outlives the mutations below.
            let (wave, point) = {
                let Some(enemies) = self.scenario.rts().and_then(|r| r.enemies.as_ref())
                else {
                    return;
                };
                let Some(&wave) = enemies.waves.get(self.enemy_wave_cursor) else {
                    return;
                };
                // In range by validation: spawn_point < spawn_points.len().
                (wave, enemies.spawn_points[wave.spawn_point as usize])
            };
            if self.tick_index < u64::from(wave.at_tick) {
                return;
            }
            if self.enemy_wave_pending == 0 {
                self.enemy_wave_pending = wave.count;
            }
            let preferred = [point.x as f32 + 0.5, point.y as f32 + 0.5];
            self.collect_unit_bodies_into_scratch();
            while self.enemy_wave_pending > 0 {
                let Some(pos) = nearest_free_body_center(
                    &self.static_nav,
                    &self.body_scratch,
                    None,
                    None,
                    preferred,
                ) else {
                    // No legal free centre on the whole grid: the remainder
                    // waits for next tick.
                    return;
                };
                let Some(id) = self
                    .entities
                    .spawn(EntityKind::Unit(UnitKind::Ghoul), OWNER_ENEMY, pos)
                else {
                    // Store full: same wait, same reason.
                    return;
                };
                self.body_scratch.push(pos);
                // `live_scratch` was collected at the top of the tick; make
                // this Ghoul a body for every later system this same tick,
                // exactly as the production system does for its unit.
                self.live_scratch.push(id.index as usize);
                self.enemies_spawned += 1;
                self.enemy_wave_pending -= 1;
            }
            // Wave fully spawned; the next wave may share this very tick.
            self.enemy_wave_cursor += 1;
        }
    }
    ```
  - [ ] 5.6 `tick` (line 1730) — insert `self.enemy_wave_system();` between `self.camera_system();` and `self.construction();`, and rewrite the doc comment's two order paragraphs to:
    ```
    /// 1. commands, 2. camera, 3. enemy waves, 4. construction,
    /// 5. production, 6. orders, 7. movement, 8. supply recount.
    ///
    /// Today the tick counter, the camera pan (2), the enemy wave spawner
    /// (3), the construction system (4), the production system (5), the
    /// gather system (6), the movement system (7) and the supply recount (8)
    /// run, followed by pruning the selection of anything that died this
    /// tick — last, so a unit that died on this tick is out of the selection
    /// before anything reads it next tick.
    ```
    Renumber the system doc labels to match: `construction` "System 3:" → "System 4:" (line 1757) **and** its "Runs before orders (5) and movement (6)" → "(6)"/"(7)"; `production_system` "System 4:" → "System 5:" (1972); `gather` "System 5:" → "System 6:" (2080); `movement` "System 6:" → "System 7:" (2220); `supply_recount` "System 7:" → "System 8:" (2065).
  - [ ] 5.7 `supply_recount` (line 2068) — filter enemies out. Replace the loop body:
    ```rust
    for i in 0..self.live_scratch.len() {
        let slot = self.live_scratch[i];
        if let EntityKind::Unit(k) = self.entities.kind(slot) {
            used += supply_cost(k);
        }
    }
    ```
    with:
    ```rust
    for i in 0..self.live_scratch.len() {
        let slot = self.live_scratch[i];
        // Enemies never enter Supply::used — supply is a player economy.
        if self.entities.owner(slot) != OWNER_PLAYER {
            continue;
        }
        if let EntityKind::Unit(k) = self.entities.kind(slot) {
            used += supply_cost(k);
        }
    }
    ```

- [ ] 6. Tracked fixture scene + sidecar + testkit name
  - [ ] 6.1 Create `assets/scenarios/fixtures/fixture_rts_combat_v1.ron` with exactly:
    ```ron
    (
      version: "rts_prototype_v1",
      width: 96,
      height: 96,
      cell_size_px: 4,
      sprite_size_px: 48,
      hard_agent_count: 0,
      stretch_agent_count: 0,
      seed: 20260817,
      destination: (x: 46, y: 60),
      spawn_cells: [(x: 46, y: 56), (x: 50, y: 56)],
      atlas_count: 4,
      direction_count: 8,
      frame_count: 4,
      collision_radius_q8: 768,
      separation_strength_q8: 256,
      separation_phases: 1,
      mass_class_count: 1,
      separation_threads: 1,
      obstacle_cells: [],
      rts: Some((
        start_crystal: 100,
        start_gas: 50,
        start_supply_cap: 10,
        hq_cell: (x: 40, y: 40),
        crystal_nodes: [(x: 30, y: 46)],
        gas_nodes: [(x: 62, y: 46)],
        enemies: Some((
          pre_placed: [(x: 20, y: 20), (x: 26, y: 20)],
          spawn_points: [(x: 76, y: 76), (x: 20, y: 76)],
          waves: [
            (at_tick: 50, count: 8, spawn_point: 0),
            (at_tick: 120, count: 4, spawn_point: 1),
          ],
        )),
      )),
    )
    ```
    Geometry facts the tests rely on: version must be `"rts_prototype_v1"` (Assumption 1); the two pre-placed centres are exactly 6.0 cells apart — touching is legal, so both keep their preferred cells; the two worker spawn cells are 4 cells apart, so worker 2 is deterministically relocated by the seeding search (exact spot irrelevant to these tests); totals 2 + 8 + 4 = 14 ≤ 1200; both spawn points sit clear of the HQ footprint (40..52 × 40..52), both nodes, and both node-inflated masks.
  - [ ] 6.2 Generate the sidecar (from the repo root; format = 64 hex + newline, matching every existing sidecar):
    ```sh
    sha256sum assets/scenarios/fixtures/fixture_rts_combat_v1.ron | cut -c1-64 > assets/scenarios/fixtures/fixture_rts_combat_v1.sha256
    ```
  - [ ] 6.3 `crates/mmd-engine/src/testkit/fixtures.rs` — after the `FIXTURE_WALLED_V1` const, add:
    ```rust
    /// 96×96 RTS combat fixture: two pre-placed Ghouls, two spawn points,
    /// two timed waves (8 at tick 50, 4 at tick 120), two workers, one node
    /// of each kind.
    ///
    /// Deliberately **not** in [`ALL_FIXTURES`]: that list feeds the phase-0
    /// flow-field suites, and this is an RTS-family scene
    /// (`version: "rts_prototype_v1"` — the validator requires that version
    /// on any scene carrying an `rts:` block) with `hard_agent_count: 0`.
    pub const FIXTURE_RTS_COMBAT_V1: &str = "fixture_rts_combat_v1";
    ```
    (`ALL_FIXTURES` itself: unchanged.)
  - [ ] 6.4 `crates/mmd-engine/src/testkit/mod.rs` — in `pub use fixtures::{…}` add `FIXTURE_RTS_COMBAT_V1,` (alphabetically after `FIXTURE_DIR,`).

- [ ] 7. Tests
  - [ ] 7.1 `crates/mmd-engine/tests/scenario_contract.rs` — add `EnemySpec, WaveSpec` to the existing `use mmd_engine::scenario::{…}` import; below `rts_spec()` (line 1024, which step 1.4 gave `enemies: None,`) add the helper + baseline test:
    ```rust
    /// `rts_spec()` with a small, fully legal enemy block. 320×320, no
    /// obstacles, HQ at (200, 200), nodes at (5, 5)/(6, 6).
    fn combat_rts_spec() -> ScenarioSpec {
        let mut spec = rts_spec();
        let rts = spec.rts.as_mut().expect("rts block");
        rts.enemies = Some(EnemySpec {
            pre_placed: vec![Cell { x: 20, y: 20 }],
            spawn_points: vec![Cell { x: 100, y: 100 }],
            waves: vec![
                WaveSpec { at_tick: 10, count: 5, spawn_point: 0 },
                WaveSpec { at_tick: 20, count: 5, spawn_point: 0 },
            ],
        });
        spec
    }

    #[test]
    fn enemy_baseline_block_is_valid() {
        // Guards the negative cases below: each mutates exactly one thing.
        Scenario::from_spec(combat_rts_spec()).expect("baseline enemy block must validate");
    }
    ```
  - [ ] 7.2 Same file — old-scene byte-identity + optionality:
    ```rust
    #[test]
    fn enemy_block_optional_old_scenes_parse() {
        // `load_verified` re-checks the tracked sidecar, so passing at all
        // proves the scene's bytes did not move under the schema change.
        let scene = Scenario::load_verified(mmd_engine::testkit::rts_scene_path())
            .expect("tracked rts scene still loads hash-verified");
        assert!(
            scene.rts().expect("rts block").enemies.is_none(),
            "a scene written before combat must parse with no enemies"
        );
    }
    ```
  - [ ] 7.3 Same file — the negative battery (uses `expect_invalid_rts` from line 1056; each closure mutates one field of `combat_rts_spec()`):
    ```rust
    #[test]
    fn enemy_spec_validates_cells_and_totals() {
        let with = |f: fn(&mut EnemySpec)| {
            let mut spec = combat_rts_spec();
            f(spec.rts.as_mut().unwrap().enemies.as_mut().unwrap());
            spec
        };

        // Out of bounds (width is 320).
        expect_invalid_rts(
            with(|e| e.spawn_points = vec![Cell { x: 320, y: 10 }]),
            "out of bounds",
        );
        // Blocked: obstacle exactly under the pre-placed cell (20, 20).
        let mut spec = combat_rts_spec();
        spec.obstacle_cells = vec![20 + 20 * 320];
        expect_invalid_rts(spec, "blocked");
        // Unreachable: 4-connected ring seals (20, 20) off from the
        // destination; the cell itself stays unblocked.
        let mut spec = combat_rts_spec();
        spec.obstacle_cells = vec![
            19 + 19 * 320, 20 + 19 * 320, 21 + 19 * 320,
            19 + 20 * 320,                21 + 20 * 320,
            19 + 21 * 320, 20 + 21 * 320, 21 + 21 * 320,
        ];
        expect_invalid_rts(spec, "unreachable");
        // On a resource node (rts_spec puts crystal at (5, 5)).
        expect_invalid_rts(
            with(|e| e.pre_placed = vec![Cell { x: 5, y: 5 }]),
            "resource node",
        );
        // Inside the HQ footprint (min corner (200, 200), edge 12).
        expect_invalid_rts(
            with(|e| e.pre_placed = vec![Cell { x: 205, y: 205 }]),
            "HQ footprint",
        );
        // Wave names a spawn point that does not exist.
        expect_invalid_rts(
            with(|e| e.waves[0].spawn_point = 1),
            "spawn_point",
        );
        // A zero wave is authoring noise.
        expect_invalid_rts(with(|e| e.waves[0].count = 0), "count");
        // Waves must be sorted non-decreasing by at_tick.
        expect_invalid_rts(with(|e| e.waves[0].at_tick = 30), "sorted");
        // 1 pre-placed + 1200 in waves = 1201 > MAX_ENEMIES.
        expect_invalid_rts(with(|e| e.waves[0].count = 1_195), "exceeds");
    }
    ```
    (`1 + 1_195 + 5 = 1_201`.)
  - [ ] 7.4 `crates/mmd-engine/tests/rts_world.rs`, `kind_tags_are_distinct` (line 174) — add `EntityKind::Unit(UnitKind::Ghoul),` to the `kinds` array and change the assertion to `assert_eq!(tags.len(), 8, "every kind must have a distinct tag byte");`.
  - [ ] 7.5 New file `crates/mmd-engine/tests/rts_enemy.rs`, complete content:
    ```rust
    //! T2 — enemy faction and waves: the Ghoul kind, `OWNER_ENEMY`, the
    //! scenario enemy block and the deterministic wave spawner. Enemies in
    //! this slice stand `Idle`; marching and fighting are the next ticket's.

    use mmd_engine::render::{Camera, IsoView};
    use mmd_engine::rts::{
        EntityKind, MAX_ENTITIES, OWNER_ENEMY, Order, ResourceKind, UnitKind,
    };
    use mmd_engine::scenario::{Cell, EnemySpec, RtsSpec, ScenarioSpec, WaveSpec};
    use mmd_engine::testkit::{FIXTURE_RTS_COMBAT_V1, RtsHarness, fixture_path};

    /// The tracked combat fixture, hash-verified.
    fn combat() -> RtsHarness {
        RtsHarness::path(fixture_path(FIXTURE_RTS_COMBAT_V1))
            .build()
            .expect("combat fixture loads hash-verified")
    }

    /// The view the drag test projects through — same construction pattern
    /// as `rts_selection.rs`, sized to the fixture's 96×96 grid.
    fn combat_view() -> IsoView {
        Camera::new(96, 96, 4.0, [1920.0, 1080.0], [46.0, 46.0]).iso_view()
    }

    fn ghouls(h: &RtsHarness) -> Vec<mmd_engine::rts::EntityId> {
        h.ids_of_kind(EntityKind::Unit(UnitKind::Ghoul))
    }

    #[test]
    fn ghoul_kind_tags_stably() {
        assert_eq!(EntityKind::Unit(UnitKind::Ghoul).tag(), 0x12);
        assert_eq!(UnitKind::Ghoul.body_radius_cells(), 3.0);
    }

    #[test]
    fn pre_placed_ghouls_spawn_at_start() {
        let h = combat();
        assert_eq!(h.tick_index(), 0);
        let ids = ghouls(&h);
        assert_eq!(ids.len(), 2, "the fixture pre-places exactly two ghouls");
        // Both preferred cells are legal and 6.0 apart (touching is legal),
        // so placement is exact, not relocated.
        let expect = [[20.5_f32, 20.5], [26.5, 20.5]];
        for (&id, want) in ids.iter().zip(expect) {
            let slot = h.world().entities().slot(id).expect("live");
            assert_eq!(h.world().entities().owner(slot), OWNER_ENEMY);
            assert_eq!(h.world().entities().position(slot), want);
            assert_eq!(h.world().order_of(id), Some(Order::Idle));
        }
        assert_eq!(h.world().enemies_spawned(), 2);
    }

    #[test]
    fn wave_spawns_at_exact_tick() {
        let mut h = combat();
        h.step_exact(49);
        assert_eq!(ghouls(&h).len(), 2, "tick 49: pre-placed only");
        assert_eq!(h.world().enemies_spawned(), 2);
        h.step_exact(1);
        let ids = ghouls(&h);
        assert_eq!(ids.len(), 10, "tick 50: the 8-ghoul wave fires");
        assert_eq!(h.world().enemies_spawned(), 10);
        for &id in &ids {
            let slot = h.world().entities().slot(id).expect("live");
            assert_eq!(h.world().entities().owner(slot), OWNER_ENEMY);
        }
    }

    #[test]
    fn enemies_never_consume_supply() {
        let mut h = combat();
        let before = h.world().supply().used();
        assert_eq!(before, 2, "two seeded workers at 1 supply each");
        h.step_exact(60); // through the tick-50 wave
        assert_eq!(ghouls(&h).len(), 10);
        assert_eq!(h.world().supply().used(), before);
    }

    #[test]
    fn drag_box_excludes_enemies() {
        let mut h = combat();
        let workers = h.ids_of_kind(EntityKind::Unit(UnitKind::Worker));
        let enemies = ghouls(&h);
        assert_eq!(workers.len(), 2);
        assert_eq!(enemies.len(), 2);
        // Park all four in one row; box select reads ground points only, so
        // overlapping bodies are a valid fixture (same trick as
        // `rts_selection.rs::park_workers_in_a_row`).
        for (i, &id) in workers.iter().chain(enemies.iter()).enumerate() {
            let slot = h.world().entities().slot(id).expect("live");
            h.world_mut()
                .entities_mut()
                .set_position(slot, [30.5 + i as f32, 30.5]);
        }
        let view = combat_view();
        // The row lies on the screen-space diagonal between these corners.
        let a = view.project(29.5, 30.5);
        let b = view.project(34.5, 30.5);
        let n = h.world_mut().box_select_into_selection(&view, a, b);
        assert_eq!(n, 2, "only the player's workers are boxable");
        assert_eq!(h.world().selection().ids(), workers.as_slice());
    }

    #[test]
    fn spawn_determinism() {
        let run = || {
            let mut h = combat();
            h.step_exact(200); // both waves done: 2 + 8 + 4
            (h.world().enemies_spawned(), h.state_hash())
        };
        let (count_a, hash_a) = run();
        let (count_b, hash_b) = run();
        assert_eq!(count_a, 14);
        assert_eq!(count_b, 14);
        assert_eq!(hash_a, hash_b, "two fresh runs must agree bit-for-bit");
    }

    /// In-memory combat spec: the fixture's geometry, one worker, one wave
    /// of 8 at tick 10 — small enough to stage store-full quickly.
    fn deferral_spec() -> ScenarioSpec {
        ScenarioSpec {
            version: "rts_prototype_v1".to_string(),
            width: 96,
            height: 96,
            cell_size_px: 4,
            sprite_size_px: 48,
            hard_agent_count: 0,
            stretch_agent_count: 0,
            seed: 20260817,
            destination: Cell { x: 46, y: 60 },
            spawn_cells: vec![Cell { x: 46, y: 56 }],
            atlas_count: 4,
            direction_count: 8,
            frame_count: 4,
            collision_radius_q8: 768,
            separation_strength_q8: 256,
            separation_phases: 1,
            mass_class_count: 1,
            separation_threads: 1,
            obstacle_cells: vec![],
            rts: Some(RtsSpec {
                start_crystal: 100,
                start_gas: 50,
                start_supply_cap: 10,
                hq_cell: Cell { x: 40, y: 40 },
                crystal_nodes: vec![Cell { x: 30, y: 46 }],
                gas_nodes: vec![Cell { x: 62, y: 46 }],
                enemies: Some(EnemySpec {
                    pre_placed: vec![],
                    spawn_points: vec![Cell { x: 76, y: 76 }],
                    waves: vec![WaveSpec { at_tick: 10, count: 8, spawn_point: 0 }],
                }),
            }),
        }
    }

    #[test]
    fn wave_defers_when_store_full() {
        let mut h = RtsHarness::spec(deferral_spec())
            .build()
            .expect("deferral spec");
        // Fill the store to MAX_ENTITIES - 3 with neutral nodes: nodes are
        // neither unit bodies nor navigation solids (StaticNav was built at
        // construction), so they consume slots and nothing else.
        while h.world().entities().len() < MAX_ENTITIES - 3 {
            h.world_mut()
                .entities_mut()
                .spawn(
                    EntityKind::Node(ResourceKind::Crystal),
                    mmd_engine::rts::OWNER_NEUTRAL,
                    [0.5, 0.5],
                )
                .expect("store not yet full");
        }
        h.step_exact(10);
        assert_eq!(
            ghouls(&h).len(),
            3,
            "three free slots -> three spawns, five deferred"
        );
        assert_eq!(h.world().enemies_spawned(), 3);
        // Free five slots; the remainder arrives on the very next tick.
        let nodes = h.ids_of_kind(EntityKind::Node(ResourceKind::Crystal));
        for &id in nodes.iter().take(5) {
            assert!(h.world_mut().entities_mut().despawn(id));
        }
        h.step_exact(1);
        assert_eq!(ghouls(&h).len(), 8, "deferred remainder spawns, none lost");
        assert_eq!(h.world().enemies_spawned(), 8);
    }
    ```
    Notes for the impl worker: `entities_mut()` arms the testkit overlap-repair pass — harmless here (nodes are not unit bodies; the drag test never ticks after parking). `ids_of_kind(Node(Crystal))` includes the scenario's real node; despawning it is fine (nothing gathers in this test).

- [ ] 8. Gate
  - [ ] 8.1 `cargo fmt --all` then the targeted suites: `cargo test -p mmd-engine --test rts_enemy --test scenario_contract --test rts_world` — all green.
  - [ ] 8.2 Full gate per `docs/05-testing.md`: `cargo fmt --all -- --check && cargo test --workspace --locked && cargo clippy --workspace --all-targets --all-features -- -D warnings`, then the smoke `cargo run -- rts --frames 1600 --inject-input-file assets/scenarios/rts_acceptance_v1.script` (exit tokens unchanged — the tracked scene has no enemy block, so behaviour is byte-for-byte the old one).
  - [ ] 8.3 Byte-identity + tracked-asset proof:
    ```sh
    git diff --stat -- assets/scenarios/rts_prototype_v1.ron assets/scenarios/rts_prototype_v1.sha256   # expect empty
    git status --porcelain -- assets/scenarios   # expect only the two new fixture files
    test "$(sha256sum assets/scenarios/fixtures/fixture_rts_combat_v1.ron | cut -c1-64)" = "$(cat assets/scenarios/fixtures/fixture_rts_combat_v1.sha256)" && echo sidecar-ok
    ```

## Outputs

- Files: `crates/mmd-engine/src/scenario.rs`, `crates/mmd-engine/src/rts/{entity.rs, mod.rs, economy.rs, orders.rs, production.rs, pack.rs, hud.rs, world.rs}`, `crates/mmd-engine/src/testkit/{fixtures.rs, mod.rs}`, new `assets/scenarios/fixtures/fixture_rts_combat_v1.ron` + `.sha256`, new `crates/mmd-engine/tests/rts_enemy.rs`, edits in `crates/mmd-engine/tests/{scenario_contract.rs, rts_world.rs}` + `enemies: None,` one-liners in the step-1.4 list. **No change** to `selection.rs` (filter pre-exists) and none to `assets/scenarios/rts_prototype_v1.ron`.
- Public API next tickets consume verbatim: `OWNER_ENEMY: u8 = 1` (`mmd_engine::rts::OWNER_ENEMY`); `UnitKind::Ghoul` (tag `0x12`, radius 3.0, HP 30 / armor 0); `EnemySpec { pre_placed, spawn_points, waves }` + `WaveSpec { at_tick, count, spawn_point }` (`mmd_engine::scenario::{EnemySpec, WaveSpec}`); `RtsWorld::enemies_spawned() -> u32`; testkit `FIXTURE_RTS_COMBAT_V1`.
- Old scenes/sidecars byte-identical.

## Validation

- [ ] `cargo fmt --all -- --check` — clean
- [ ] `cargo test --workspace --locked` — green (incl. new `rts_enemy` suite: 7 tests; `scenario_contract`: +3; `rts_world::kind_tags_are_distinct` now asserts 8)
- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings` — clean
- [ ] `cargo run -- rts --frames 1600 --inject-input-file assets/scenarios/rts_acceptance_v1.script` — passes with unchanged exit tokens (`body_overlaps=0 …`)
- [ ] `git diff --stat -- assets/scenarios/rts_prototype_v1.ron` — empty; `git status --porcelain -- assets/scenarios` — only the two new fixture files
- [ ] `test "$(sha256sum assets/scenarios/fixtures/fixture_rts_combat_v1.ron | cut -c1-64)" = "$(cat assets/scenarios/fixtures/fixture_rts_combat_v1.sha256)"` — true
- [ ] commit msg draft: `feat(rts): enemy faction, scenario waves and deterministic spawner`
