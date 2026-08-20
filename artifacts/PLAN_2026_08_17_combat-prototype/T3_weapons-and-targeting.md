# T3: Weapons and targeting

**Plan:** `./artifacts/PLAN_2026_08_17_combat-prototype.md`
**Depends:** T2
**Commit outcome:** Instant-hit combat system runs in the tick; enemies march on the HQ and melee whatever enters range; soldiers auto-acquire; deaths flow through T1's rules; all engine-level, no player commands yet.

## Context (self-contained)

- Goal: Phase 2 Combat Prototype — weapons, damage, turrets, enemy AI. Success = scripted combat run + exit tokens.
- This slice: the fight itself, engine-side. Enemies stop standing idle (T2) and march/attack; soldiers defend without orders. Player cannot issue Attack yet (T4), turret does not exist yet (T5).
- Out of scope here: command card/input/audio (T4), `BuildingKind::Turret` (T5), HP bars (T6), any `assets/scenarios/*` edit except test fixtures (gate scene = T7). `sim/` frozen.
- Assumptions in force: instant-hit on cooldown, no projectile entities; damage `max(1, damage - armor)` via T1 API; nearest-target lowest-slot tie-break; enemy march = permanent `Order::AttackMove` at the enemy objective; HQ dead → nearest remaining player building, none → Idle; plain `Order::Move` NEVER fires; no per-enemy pathfinding — enemies descend shared pooled fields (`NAV_FIELD_SLOTS = 8`, `crates/mmd-engine/src/nav/field_pool.rs:15`).
- **Detailer decisions (recorded, do not reopen):**
  - **Objective cell is the approach cell, not the building's own cell** (conflict vs plan brief, codebase wins): a finished building's footprint is blocked in the inflated centre mask, and `FieldPool::acquire` on a blocked cell fails cleanly (see the doc of `entity_approach_cell`, `crates/mmd-engine/src/rts/orders.rs` — "Returns the footprint's own cell when no ring … which then makes `FieldPool::acquire` fail cleanly"). So the objective is `entity_approach_cell(&static_nav, &entities, building_id, UnitKind::Ghoul).0` — the same geometry family a hauler's drop-off leg already targets.
  - **"Nearest remaining player building" is measured from the starting HQ centre**, captured at world construction into a new field `enemy_objective_origin: [f32; 2]` (the plan left the reference point unstated). Deterministic, survives the HQ's death, ties go to the lower slot via an ascending scan with strict `<`.
  - **Cooldown semantics**: decrement first (`saturating` at 0), then fire when the column reads 0, then reset to `cooldown_ticks` — this makes the firing period *exactly* `cooldown_ticks` (fire at t, next at t + cooldown_ticks). A fresh spawn starts at 0 (fires the first tick it has a target).
  - **Death detection for the counters**: T1's `DamageResult` variant shape is not part of the consumed contract, so the combat system detects a kill by `!self.entities.contains(target)` after `apply_damage` — correct regardless of T1's enum. Read the target's owner *before* the call.
  - **Counters are hashed**: `kills`, `losses`, `first_combat_tick` enter `state_hash` fixed-width (they are world state T7's exit tokens read; the camera is already hashed on the same argument).
  - **Kills/losses are counted in the combat system**, not in T1's death routing: every present and future damage source (T4 player attack, T5 turret) flows through this one system, and `first_combat_tick` is defined against it anyway.
  - **A dead `Attack` target clears the order to `Idle` for both owners, in the combat pass itself, and the unit falls through to the `Idle` auto-acquire rule the same tick.** An enemy that went Idle this way is re-marched by the enemy AI next tick ("resumes march").
  - **`combat_hold` scratch (`Vec<bool>`, `MAX_ENTITIES`) is the halt-in-range channel**: combat (which runs before movement) marks every armed unit whose target is in range this tick; the movement arms for `Attack`/`AttackMove` return early when marked.
  - **Construction sites count** as objective candidates and as valid targets (T1 allows site death); resource nodes are never targets (owner `OWNER_NEUTRAL` excludes them; `surface_distance` returns `None` for nodes as a second fence).
  - **Ghoul speed gets its own const + compile-time push-chain assert** in `world.rs`, same pattern as the worker/soldier asserts at `crates/mmd-engine/src/rts/world.rs:351-361`.
  - **`hp` accessor name**: T1's ticket adds the HP column + accessors but the exact getter name is not in the consumed contract. Tests below route every HP read through one local helper `hp_of`; if T1 landed a different name than `hp(slot)`, fix that one helper line only.
  - **Test file**: new `crates/mmd-engine/tests/rts_combat.rs` (if T2 already created a file of that name, append to it). One allocation test goes into the existing `crates/mmd-engine/tests/frame_allocations.rs` because the counting allocator lives there.
  - `counters_and_first_combat_tick` uses a controlled in-memory spec rather than the T2 fixture — the fixture's exact enemy counts are T2's detailer's choice and exact-number assertions must not depend on them. The fixture is exercised by `combat_determinism` instead.

## Requirements

- Weapon table keyed on `UnitKind` (buildings join in T5): `pub struct Weapon { pub damage: u32, pub cooldown_ticks: u32, pub range_cells: f32 }`; `pub fn weapon(kind: UnitKind) -> Option<Weapon>` — Worker `None`; Soldier `Some(6, 15, 24.0)`; Ghoul `Some(5, 30, 8.0)`.
- Cooldown column in `EntityStore` (u32 ticks remaining, decrement in combat system, fires only at 0, reset to `cooldown_ticks` on fire). Enters state hash fixed-width.
- New `Order` variants **appended** (`orders.rs`, tags after `Build`=3):
  ```rust
  Attack { target: EntityId, field: FieldRef },      // tag 4
  AttackMove { goal: FormationGoal, field: FieldRef }, // tag 5
  ```
  `Order::tag()`, `with_field()`, `hash_into()` extended fixed-width (per-variant constant frame; the tag byte self-frames, exactly as `Gather` already differs in width from `Move`).
- Combat system inserted in `RtsWorld::tick` between orders (`gather`) and movement (doc list renumbered): for each live armed unit, pick target = nearest enemy-of-owner entity whose **surface distance** ≤ `range_cells` (units: centre distance − target body radius; buildings: `rect_distance` to footprint — same geometry family as `interaction_reach`, `orders.rs:41`), lowest slot tie-break; if cooldown 0 → `apply_damage`, reset cooldown. Deaths resolve immediately (T1 routing). Deterministic iteration: ascending slot order (`live_scratch` order with a re-check of `alive`, since combat itself despawns mid-pass).
- Firing rules: unit with `Order::Idle` or `Order::AttackMove` → auto-acquire + fire in place (no chasing while Idle). `Order::Attack { target }` → walk toward target while out of range (field to the target's current cell for a unit, to its approach cell for a building; reuse the pooled-field re-acquire discipline already in `step_one_unit` step 3), fire when in range; target dead → Idle (player) / resume march via enemy AI (enemy). `Order::Move`/`Gather`/`Build` → never fires. Enemies and player units use identical rules — owner only flips who is a valid target (`owner != mine && owner != OWNER_NEUTRAL`).
- `AttackMove` movement: descend shared field toward goal; when an armed unit's combat check finds a target in range it halts and fires (`combat_hold`); target dies → resume descent. `FormationGoal` shape reused so player A-move (T4) shares the path.
- Enemy AI system (immediately before combat in the same insertion block): every `OWNER_ENEMY` unit with `Order::Idle` — or with an `AttackMove` whose `goal.anchor` no longer equals the objective — gets `Order::AttackMove` at the current enemy objective. Objective = approach cell of the live player building nearest `enemy_objective_origin` (HQ while it lives, since its distance is 0); none alive → objective `None`, marching enemies cleared to Idle. Objective recomputed only when `enemy_objective_dirty` is set (at load, and by a player building's death in T1's routing — never a per-tick scan). All enemies share the pooled field for the objective cell — hundreds of enemies, one field, acquired at most once per tick.
- Ghoul speed: `unit_speed` arm 18.0 c/s via new `pub const GHOUL_SPEED_CELLS_PER_SEC: f32 = 18.0;` (`orders.rs` — exhaustive match; T2 may have landed a placeholder arm to compile, replace it). Compile-time assert `GHOUL_SPEED_CELLS_PER_SEC < MAX_PUSH_SAFE_UNIT_SPEED_CELLS_PER_SEC` in `world.rs` (ceiling is 45.0 c/s, see `world.rs:346`).
- Enemies use existing hard-body movement (proposal/commit, push chains `MAX_PUSH_DEPTH = 3`, `MAX_PUSHED_BODIES = 8`; ADR 017/021 invariant untouched — enemies are ordinary hard pairs, never the gather exception; `collect_unit_bodies` already sweeps every `EntityKind::Unit(_)` regardless of owner, so no collision change is needed at all).
- Kill/loss counters on `RtsWorld`: `kills: u32` (enemy deaths by combat fire), `losses: u32` (player unit + building deaths by combat fire), `first_combat_tick: Option<u32>` (tick of the first combat-system `apply_damage`). Pub getters — T7 exit tokens read them. All three hashed.
- No per-frame allocation anywhere: `combat_hold` is `vec![false; MAX_ENTITIES]` at construction; the targeting scan iterates `live_scratch` (already filled at tick start) with no push; the only allocation-adjacent path is `FieldPool::acquire` on a **miss**, which is the pool's documented bounded exception.

## Inputs

- **From T1 (verbatim, lands before this ticket):** `RtsWorld::apply_damage(&mut self, target: EntityId, damage: u32) -> DamageResult`; death routing (building un-stamp invalidates ALL pooled fields, production-queue cancel, HQ-death gather fallback). Damage per hit `max(1, damage - armor)`; armor: Hq 2, everything else in play here 0. Max HP: Worker 25, Soldier 40, Hq 400, Ghoul 30 (T2 adds the Ghoul row).
- **From T2 (verbatim, lands before this ticket):** `OWNER_ENEMY: u8 = 1` (in `entity.rs` beside `OWNER_PLAYER = 0` / `OWNER_NEUTRAL = 255`); `UnitKind::Ghoul` (= 2, body radius 3.0); fixture scene `assets/scenarios/fixtures/fixture_rts_combat_v1.ron` + `.sha256` with pre-placed Ghouls + waves; `RtsWorld::enemies_spawned() -> u32`; `RtsSpec` gains `enemies: Option<EnemySpec>` with `EnemySpec { pre_placed: Vec<Cell>, spawn_points: Vec<Cell>, waves: Vec<WaveSpec> }`, `WaveSpec { at_tick: u32, count: u32, spawn_point: u8 }` (every `RtsSpec` literal in new tests must carry the `enemies` field).
- `crates/mmd-engine/src/rts/orders.rs` — `Order` enum at line 85 (`Idle`=0, `Move`=1, `Gather`=2, `Build`=3), `tag()` line 109, `with_field()` line 124, `OrderTable::hash_into` line 182 (fixed-width discipline incl. the Idle zeroed-payload trick and the `hash_field`/`hash_goal` helpers below it), `unit_speed` line 23 (`WORKER…=30.0` line 18, `SOLDIER…=24.0` line 20), `interaction_reach` line 41, `dist2` line 254, `rect_distance` line 261, `node_cell` line 380, `entity_approach_cell` (pub(crate), returns `(Cell, f32)`). *(Line numbers are pre-T1/T2; re-anchor by symbol.)*
- `crates/mmd-engine/src/rts/entity.rs` — SoA columns end with `carry_amount: Vec<u32>` (line 141), `spawn_impl` line 214 (push-fresh branch + reset block), `set_carry` line 403 (insert cooldown accessors after it), `hash_into` line 431, `column_capacities() -> [usize; 13]` line 454 (T1 grows it; grow again).
- `crates/mmd-engine/src/rts/world.rs` — `tick` line 1730 (current body: `collect_live` → `camera_system` → `construction` → `production_system` → `gather` → `movement` → `supply_recount` → `selection.retain_live`; T2 inserts its wave system after `camera_system`), speed asserts lines 351–361, `gather` line 2085, `movement` line 2289, `step_one_unit` line 3059 (the `let (goal, field) = match order` block and its re-path step 3 / zero-vector step 4), `state_hash` line 3292, `from_scenario` line 625 (struct init block near line 726), struct fields near line 200 (`build_attend: Vec<bool>` is the pattern for `combat_hold`), testkit seams `entities_mut` (line 859, arms overlap repair), `force_position_for_test` (line 873), `force_order_for_test` (line ~885), `nav_mut`, `resources_mut`; imports: `use crate::nav::field_pool::{FieldPool, FieldPoolError};` line 5 (add `FieldRef`), `use super::orders::{…}` line 30 (add `GHOUL_SPEED_CELLS_PER_SEC`, `node_cell`).
- `crates/mmd-engine/src/rts/mod.rs` — module list lines 3–15 (`mod collision;` line 4, `mod economy;` line 5), re-export blocks (`pub use collision::` line 23).
- `crates/mmd-engine/src/nav/field_pool.rs` — `FieldRef { slot: u8, epoch: u64 }`, `NAV_FIELD_SLOTS = 8` (line 15), exact-LRU ties-to-lowest, `acquire`/`is_current`/`reachable`, stats seams `rebuild_count()` / `acquire_count()`.
- `crates/mmd-engine/src/rts/formation.rs` — `FormationGoal { anchor: Cell, slot: Cell }` (pub fields — integration tests build literals), `FormationGoal::at` (pub(crate)), `capture_radius2`, `FORMATION_CAPTURE_MARGIN_CELLS = 6.0`.
- `mmd_engine::testkit` — `RtsHarness::{scene, path, spec}` builders, `step_exact`, `world`/`world_mut`, `state_hash`, `ids_of_kind`; `fixture_path(name)` (`testkit/fixtures.rs:40`, workspace-root-anchored, appends `.ron`).
- `crates/mmd-engine/tests/frame_allocations.rs` — `lock_alloc_tests()`, `reset_count()`, `MeasureGuard::enter()` pattern (see `movement_allocates_nothing`, line ~733).
- Merge gate: `docs/05-testing.md` lines 95–114.

## TDD

1. **Red** — failing tests below (harness, seeded, clock-free).
2. **Green** — min code.
3. **Refactor** — keep green.

## Test plan

All in `crates/mmd-engine/tests/rts_combat.rs` unless noted. Shared setup (write once at the top of the file):

```rust
//! T3 — instant-hit combat, enemy march AI and auto-acquire.
//!
//! Headless, seeded, clock-free: every case drives `RtsWorld::tick` through
//! `testkit::RtsHarness`. Player commands do not exist yet (T4), so orders
//! are placed through the testkit seams.

use mmd_engine::rts::{
    BuildingKind, EntityId, EntityKind, FormationGoal, OWNER_ENEMY, OWNER_PLAYER, Order, UnitKind,
    weapon,
};
use mmd_engine::scenario::{Cell, RtsSpec, ScenarioSpec};
use mmd_engine::testkit::RtsHarness;

const W: u32 = 96;
const H: u32 = 96;

/// A combat sandbox: HQ (12-cell footprint, centre [88.0, 88.0]) in the
/// south-east corner, nodes in the north-east corner, the one mandatory
/// seeded worker parked in the north-west corner far from every fight.
/// Each case spawns and places its own combatants.
fn combat_spec() -> ScenarioSpec {
    ScenarioSpec {
        version: "rts_prototype_v1".to_string(),
        width: W,
        height: H,
        cell_size_px: 4,
        sprite_size_px: 48,
        hard_agent_count: 0,
        stretch_agent_count: 0,
        seed: 1,
        destination: Cell { x: 0, y: 0 },
        spawn_cells: vec![Cell { x: 4, y: 4 }],
        atlas_count: 4,
        direction_count: 8,
        frame_count: 4,
        collision_radius_q8: 0,
        separation_strength_q8: 0,
        separation_phases: 1,
        mass_class_count: 1,
        separation_threads: 1,
        obstacle_cells: vec![],
        rts: Some(RtsSpec {
            start_crystal: 300,
            start_gas: 100,
            start_supply_cap: 10,
            hq_cell: Cell { x: 82, y: 82 },
            crystal_nodes: vec![Cell { x: 94, y: 1 }],
            gas_nodes: vec![Cell { x: 93, y: 1 }],
            enemies: None,
        }),
    }
}

fn harness() -> RtsHarness {
    RtsHarness::spec(combat_spec()).build().expect("combat harness")
}

fn spawn_unit(h: &mut RtsHarness, kind: UnitKind, owner: u8, pos: [f32; 2]) -> EntityId {
    h.world_mut()
        .entities_mut()
        .spawn(EntityKind::Unit(kind), owner, pos)
        .expect("store has room")
}

/// The one place HP is read; if T1 named its accessor differently, fix here.
fn hp_of(h: &RtsHarness, id: EntityId) -> u32 {
    let slot = h.world().entities().slot(id).expect("live entity");
    h.world().entities().hp(slot)
}

/// One `apply_damage` call the HQ cannot survive: 402 - armor 2 = 400 = max HP.
/// Killing the only player building parks the enemy AI (objective `None`),
/// so a case controls exactly who moves and who fires.
fn kill_hq(h: &mut RtsHarness) {
    let hq = h.world().start_hq().expect("seeded hq");
    let _ = h.world_mut().apply_damage(hq, 402);
    assert!(!h.world().entities().contains(hq), "the hq must be dead");
}
```

Geometry crib for the cases (unit body radius 3.0; surface distance = centre distance − 3.0 for units, `rect_distance` for buildings; Soldier range 24.0 / damage 6 / cooldown 15; Ghoul range 8.0 / damage 5 / cooldown 30; Ghoul walks 18.0/60 = 0.3 cells per tick; HQ rect is `[82, 94) × [82, 94)`, armor 2 → ghoul hit = 3):

| Test | Setup (exact) | Expect (exact) |
| ---- | ----- | ------ |
| `soldier_auto_acquires_idle` | `kill_hq`; Soldier P at `[30.5, 30.5]` (Idle); Ghoul E at `[60.5, 30.5]` (surface 27.0 > 24 → out of range) forced `Order::Move { goal: FormationGoal { anchor: Cell{x:40,y:30}, slot: Cell{x:40,y:30} }, field }` with `field = h.world_mut().nav_mut().acquire(Cell{x:40,y:30}).expect("field")`. Loop 80 single ticks recording `(tick_index, hp_drop)` for the ghoul, breaking when `!contains(ghoul)` | ≥ 2 hits recorded; every drop == 6; consecutive hit ticks differ by exactly 15; first hit strictly after tick 1 (it had to walk into range) |
| `plain_move_never_fires` | `kill_hq`; Soldier at `[20.5, 30.5]`, Ghoul at `[44.5, 36.5]` (Idle); `assert!(h.world_mut().order_move(soldier, Cell { x: 60, y: 30 }))`; `h.step_exact(140)` | `hp_of(ghoul) == 30` (Move never fires); `hp_of(soldier) < 40` (the ghoul *did* see the soldier in range — anti-vacuity); soldier still alive |
| `nearest_target_lowest_slot_tie` | `kill_hq`; Soldier `[30.5, 30.5]`; Ghoul A `[20.5, 30.5]` then Ghoul B `[40.5, 30.5]` (spawn order ⇒ A gets the lower slot; both at surface 7.0 exactly — an exact f32 tie); `h.step_exact(1)` | `hp_of(A) == 24`, `hp_of(B) == 30` (lower slot hit first); `hp_of(soldier) == 30` (both ghouls fired back under the Idle rule: 40 − 2·5) |
| `cooldown_gates_fire_rate` | HQ alive; Ghoul at `[88.5, 74.5]` (rect distance 7.5 ≤ 8 → in range from tick 1; enemy AI gives it `AttackMove`, hold keeps it planted); `h.step_exact(90)` | HQ hp == 400 − 9 (hits on ticks 1, 31, 61 at 3 each); then `h.step_exact(1)` → hp == 400 − 12 (tick 91); `first_combat_tick() == Some(1)` |
| `ghouls_march_on_hq` | HQ alive; 6 Ghouls at `[10.5 + 9.0 * i, 60.5]` for `i in 0..6`; `h.step_exact(2)`; snapshot `rebuild_count()` and each ghoul's distance to `[88.0, 88.0]`; `h.step_exact(200)` | every ghoul strictly closer to `[88.0, 88.0]`; every ghoul order is `AttackMove` and all six `goal.anchor` are equal (one shared objective); `rebuild_count()` unchanged over the 200 ticks (one pooled field) |
| `ghoul_attacks_first_thing_in_range` | HQ alive; Ghoul at `[40.5, 88.5]` (due west of the HQ), Worker (player) at `[58.5, 88.5]` on the march line; loop up to 900 single ticks | worker hp drops in steps of exactly 5, 30 ticks apart, until it dies; `losses() == 1`, `kills() == 0`; after the worker's death the ghoul's x strictly increases again (march resumed); within 400 further ticks HQ hp < 400 |
| `ghouls_besiege_and_kill_hq` | HQ alive, undefended; 10 Ghouls in two columns: `[64.5, 62.5 + 6.0 * i]` and `[70.5, 62.5 + 6.0 * i]` for `i in 0..5`; `h.step_exact(1400)` (10 ghouls × 3 dmg / 30 ticks = 1 hp/tick sustained; 400 hp + approach ≪ 1400) | HQ dead (`contains == false`); `losses() == 1`, `kills() == 0`, `first_combat_tick().is_some()`; footprint un-stamped: `!h.world().nav().blocked()[(88 + 88 * W) as usize]`; after 2 more ticks every ghoul order is `Idle` (objective `None`) |
| `objective_retargets_on_hq_death` | HQ alive; raw-spawn a Depot: `h.world_mut().entities_mut().spawn(EntityKind::Building(BuildingKind::Depot), OWNER_PLAYER, [30.0, 30.0]).expect("room")`; Ghoul at `[10.5, 88.5]`; `h.step_exact(1)`, capture `g1` from its `AttackMove`; `kill_hq`; `h.step_exact(1)`, capture `g2` | `g2.anchor != g1.anchor`; `g2.anchor`'s centre is within 1.0 of the Depot's footprint rect (edge 8, centre `[30.0, 30.0]`: `max(0, |cx − 30| − 4)² + max(0, |cy − 30| − 4)²` ≤ 1.0²) |
| `no_player_buildings_enemies_idle` | 3 Ghouls at `[20.5, 20.5 + 8.0 * i]`; `h.step_exact(1)` (marching); `kill_hq`; `h.step_exact(1)` | every ghoul order == `Order::Idle`; positions after 60 further ticks identical to positions now; no panic (the seeded worker being alive changes nothing — the objective is buildings-only) |
| `counters_and_first_combat_tick` | `kill_hq`; Soldier `[30.5, 30.5]`, Ghoul `[53.5, 30.5]` (soldier surface 20 ≤ 24; ghoul surface 20 > 8 → one-sided); `h.step_exact(61)` (hits on ticks 1, 16, 31, 46, 61 = 5 × 6 ≥ 30) | ghoul dead; `kills() == 1`, `losses() == 0`, `first_combat_tick() == Some(1)` |
| `combat_determinism` | two `RtsHarness::path(mmd_engine::testkit::fixture_path("fixture_rts_combat_v1"))` harnesses; 600 ticks stepped in lockstep | `a.state_hash() == b.state_hash()` after **every** tick |
| `cooldown_enters_state_hash` | two `harness()` worlds, identical Soldier spawn at `[30.5, 30.5]` in each; assert hashes equal; then `set_cooldown(slot, 5)` via `entities_mut` in one | hashes now differ |
| `field_pool_not_churned` | own spec, `width = height = 160`, `hq_cell = Cell { x: 144, y: 144 }`, `enemies: Some(EnemySpec { pre_placed: <300 cells: x = 4 + 7*(k % 15), y = 4 + 7*(k / 15), k in 0..300>, spawn_points: vec![], waves: vec![] })`; `h.step_exact(5)`; snapshot `rebuild_count()`; `h.step_exact(150)` | `enemies_spawned() == 300`; `rebuild_count()` delta over the 150 ticks == 0 (hundreds marching, one field); `acquire_count()` strictly grew (the hit path is what is being reused — anti-vacuity) |
| `combat_march_allocates_nothing` (**in `frame_allocations.rs`**) | `lock_alloc_tests()` + `reset_count()`; `RtsHarness::spec` over `combat_spec()`-shaped scene; 6 Ghouls two columns at `[64.5/70.5, 62.5 + 6.0*i]` and 2 player Workers at `[50.5, 85.5]`, `[50.5, 91.5]` in the march path; warm `h.step_exact(20)` outside the guard (field misses settle); `MeasureGuard::enter()`; `h.step_exact(300)`; assert 0 allocations, `guard.assert_zero()` | zero allocations across march + fire + deaths; after the guard `h.world().losses() == 2` (both workers died inside the window — the measured ticks really contained combat and despawns) |

Run: `cargo test -p mmd-engine --test rts_combat --locked` and `cargo test -p mmd-engine --test frame_allocations combat_march_allocates_nothing --locked -- --test-threads=1` (alloc tests serialize via `lock_alloc_tests` anyway).

## Impl steps

- [ ] 1. **Weapon table module** — new file + registration.
  - [ ] 1.1 Create `crates/mmd-engine/src/rts/combat.rs` with exactly:
    ```rust
    //! Weapons and the shared range geometry of instant-hit combat.
    //!
    //! Data only: the combat *system* lives in `world.rs` beside the other
    //! tick systems because it mutates `RtsWorld` internals. What lives here
    //! is the per-kind weapon table and the surface-distance rule both the
    //! system and its tests share.

    use super::entity::{EntityKind, EntityStore, UnitKind};
    use super::orders::{dist2, rect_distance};

    /// An instant-hit weapon: no projectile entity, the damage lands on the
    /// tick the shot fires.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct Weapon {
        /// Raw damage per shot, before the defender's armor — the world
        /// applies `max(1, damage - armor)`.
        pub damage: u32,
        /// Ticks between shots. A unit fires when its cooldown column reads
        /// 0 and the column resets to this on every shot, so the firing
        /// period is exactly this many ticks.
        pub cooldown_ticks: u32,
        /// Reach in cells, measured to the target's *surface* — see
        /// [`surface_distance`].
        pub range_cells: f32,
    }

    /// The weapon a unit kind carries. `None` never fires. Buildings join
    /// this table in the turret slice; until then only units are armed.
    pub fn weapon(kind: UnitKind) -> Option<Weapon> {
        match kind {
            UnitKind::Worker => None,
            UnitKind::Soldier => Some(Weapon {
                damage: 6,
                cooldown_ticks: 15,
                range_cells: 24.0,
            }),
            UnitKind::Ghoul => Some(Weapon {
                damage: 5,
                cooldown_ticks: 30,
                range_cells: 8.0,
            }),
        }
    }

    /// Distance from an attacker standing at `p` to the *surface* of the
    /// entity in store slot `target_slot`: centre distance minus body radius
    /// for a unit, distance to the footprint rectangle for a building — the
    /// same geometry family as `interaction_reach`. `None` for a node:
    /// nodes are indestructible and never a combat target.
    pub(crate) fn surface_distance(
        store: &EntityStore,
        p: [f32; 2],
        target_slot: usize,
    ) -> Option<f32> {
        let q = store.position(target_slot);
        match store.kind(target_slot) {
            EntityKind::Unit(k) => Some(dist2(p, q).sqrt() - k.body_radius_cells()),
            EntityKind::Building(b) => Some(rect_distance(p, q, b.footprint_cells())),
            EntityKind::Node(_) => None,
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// The design numbers, pinned: a balance edit must be a visible diff
        /// here, not only in behaviour.
        #[test]
        fn the_weapon_table_matches_the_design_numbers() {
            assert_eq!(weapon(UnitKind::Worker), None);
            assert_eq!(
                weapon(UnitKind::Soldier),
                Some(Weapon { damage: 6, cooldown_ticks: 15, range_cells: 24.0 })
            );
            assert_eq!(
                weapon(UnitKind::Ghoul),
                Some(Weapon { damage: 5, cooldown_ticks: 30, range_cells: 8.0 })
            );
        }
    }
    ```
  - [ ] 1.2 `crates/mmd-engine/src/rts/mod.rs`: in the module list, insert `mod combat;` between `mod collision;` (line 4) and `mod economy;` (line 5).
  - [ ] 1.3 `crates/mmd-engine/src/rts/mod.rs`: after the `pub use collision::{…};` block (starts line 23), insert:
    ```rust
    pub use combat::{Weapon, weapon};
    ```
- [ ] 2. **Cooldown column** in `crates/mmd-engine/src/rts/entity.rs`.
  - [ ] 2.1 In `struct EntityStore` after `carry_amount: Vec<u32>,` (line 141; T1 may have appended `hp`/`armor` after it — append after whatever is now the last data column, before the `free` field):
    ```rust
    /// Ticks until this entity may fire again; `0` means ready, and stays
    /// `0` for anything unarmed.
    cooldown: Vec<u32>,
    ```
  - [ ] 2.2 In `EntityStore::new()`, append `cooldown: Vec::with_capacity(MAX_ENTITIES),` beside the other column initialisers (before `free:`).
  - [ ] 2.3 In `spawn_impl` (line 214): add `self.cooldown.push(0);` at the end of the fresh-slot push block (after the `carry_amount` push and any T1 pushes), and `self.cooldown[idx] = 0;` at the end of the reset block (after `self.carry_amount[idx] = 0;` and any T1 resets).
  - [ ] 2.4 After `set_carry` (line 403), add:
    ```rust
    /// Ticks until this entity may fire again. `0` means ready.
    pub fn cooldown(&self, slot: usize) -> u32 {
        self.assert_live(slot);
        self.cooldown[slot]
    }

    pub fn set_cooldown(&mut self, slot: usize, ticks: u32) {
        self.assert_live(slot);
        self.cooldown[slot] = ticks;
    }
    ```
  - [ ] 2.5 In `hash_into` (line 431): append `h.update(self.cooldown[i].to_le_bytes());` as the **last** per-slot update (after `carry_amount` and after T1's lines).
  - [ ] 2.6 In `column_capacities` (line 454): bump the array length literal by one (13 → 14 pre-T1; whatever T1 left, plus one) and append `self.cooldown.capacity(),` as the last entry.
- [ ] 3. **Order variants + Ghoul speed** in `crates/mmd-engine/src/rts/orders.rs`.
  - [ ] 3.1 In `enum Order` (line 85), append after the `Build { … }` variant:
    ```rust
    /// Close on `target` until it is inside weapon range, then stand and
    /// fire until it dies. The field tracks the target's current cell.
    Attack {
        target: EntityId,
        field: FieldRef,
    },
    /// Walk toward `goal`, but stop and fire on anything hostile that comes
    /// into range on the way; resume the walk when nothing is.
    AttackMove {
        goal: FormationGoal,
        field: FieldRef,
    },
    ```
  - [ ] 3.2 In `Order::tag()` (line 109), append arms:
    ```rust
    Self::Attack { .. } => 4,
    Self::AttackMove { .. } => 5,
    ```
  - [ ] 3.3 In `Order::with_field()` (line 124), insert before the `other => other,` arm:
    ```rust
    Self::Attack { target, .. } => Self::Attack { target, field },
    Self::AttackMove { goal, .. } => Self::AttackMove { goal, field },
    ```
  - [ ] 3.4 In `OrderTable::hash_into` (line 182), append match arms after the `Order::Build { … }` arm (per-variant constant frame; the tag byte written above the match self-frames the variants, exactly as `Gather`'s wider frame already coexists with `Move`'s):
    ```rust
    Order::Attack { target, field } => {
        h.update(target.index.to_le_bytes());
        h.update(target.generation.to_le_bytes());
        hash_field(h, field);
    }
    Order::AttackMove { goal, field } => {
        hash_goal(h, goal);
        hash_field(h, field);
    }
    ```
  - [ ] 3.5 After `SOLDIER_SPEED_CELLS_PER_SEC` (line 20), add:
    ```rust
    /// See [`WORKER_SPEED_CELLS_PER_SEC`]. Slower than both player units:
    /// a ghoul is walked away from, never outrun by accident.
    pub const GHOUL_SPEED_CELLS_PER_SEC: f32 = 18.0;
    ```
    and in `unit_speed` (line 23) make the Ghoul arm `UnitKind::Ghoul => GHOUL_SPEED_CELLS_PER_SEC,` (T2 may have landed a placeholder arm to keep the exhaustive match compiling — replace it; if it landed none, add it).
  - [ ] 3.6 `crates/mmd-engine/src/rts/mod.rs`: add `GHOUL_SPEED_CELLS_PER_SEC` to the `pub use orders::{…}` list (alphabetical slot: after `GatherPhase,`).
- [ ] 4. **World fields, getters, hash** in `crates/mmd-engine/src/rts/world.rs`.
  - [ ] 4.1 Imports: line 5 → `use crate::nav::field_pool::{FieldPool, FieldPoolError, FieldRef};`; the `use super::orders::{…}` block (line 30) → add `GHOUL_SPEED_CELLS_PER_SEC` and `node_cell`; the `use super::entity::{…}` block → `OWNER_ENEMY` is already needed here (T2 exported it; add if absent); add `use super::combat::{surface_distance, weapon};`.
  - [ ] 4.2 After the soldier speed assert (lines 356–361), add:
    ```rust
    const _: () = assert!(
        GHOUL_SPEED_CELLS_PER_SEC < MAX_PUSH_SAFE_UNIT_SPEED_CELLS_PER_SEC,
        "the ghoul outruns the push chain's endpoint check: raise MAX_PUSH_DEPTH's \
         cost or lower the speed — see MAX_PUSH_SAFE_UNIT_SPEED_CELLS_PER_SEC"
    );
    ```
  - [ ] 4.3 In `struct RtsWorld`, after `production: ProductionTable,`, add:
    ```rust
    /// Per-slot "this unit has a live target in weapon range this tick",
    /// written by the combat system and read by the movement arms for
    /// `Attack`/`AttackMove` — a unit that can shoot stands still. Reserved
    /// to [`MAX_ENTITIES`] so combat never allocates.
    combat_hold: Vec<bool>,
    /// Where the enemy faction marches: the approach cell of the current
    /// objective building. `None` when no player building is left.
    enemy_objective: Option<Cell>,
    /// Recompute [`Self::enemy_objective`] before the next enemy-AI pass.
    /// Set at load and by a player building's death — never per tick.
    enemy_objective_dirty: bool,
    /// The starting HQ's centre, kept after its death: the fixed point
    /// "nearest remaining player building" is measured from.
    enemy_objective_origin: [f32; 2],
    /// Enemy entities destroyed by combat fire.
    kills: u32,
    /// Player units and buildings destroyed by combat fire.
    losses: u32,
    /// Tick index of the first combat shot ever applied; `None` while the
    /// run is bloodless.
    first_combat_tick: Option<u32>,
    ```
  - [ ] 4.4 In `from_scenario`'s `Ok(Self { … })` block (near line 726), after `production: ProductionTable::new(),`, add:
    ```rust
    combat_hold: vec![false; MAX_ENTITIES],
    enemy_objective: None,
    enemy_objective_dirty: true,
    enemy_objective_origin: hq_pos,
    kills: 0,
    losses: 0,
    first_combat_tick: None,
    ```
  - [ ] 4.5 Next to `pub fn tick_index` (line ~950), add the contract getters:
    ```rust
    /// Enemy entities destroyed by combat fire. An exit token reads this.
    pub fn kills(&self) -> u32 {
        self.kills
    }

    /// Player units and buildings destroyed by combat fire.
    pub fn losses(&self) -> u32 {
        self.losses
    }

    /// Tick index of the first combat shot, `None` while nothing has fired.
    pub fn first_combat_tick(&self) -> Option<u32> {
        self.first_combat_tick
    }
    ```
  - [ ] 4.6 In `state_hash` (line 3292), before `h.finalize().into()`, append:
    ```rust
    h.update(self.kills.to_le_bytes());
    h.update(self.losses.to_le_bytes());
    match self.first_combat_tick {
        None => {
            h.update([0u8]);
            h.update(0u32.to_le_bytes());
        }
        Some(t) => {
            h.update([1u8]);
            h.update(t.to_le_bytes());
        }
    }
    ```
    and extend the doc comment's coverage list: `…then resources and supply, then the combat counters (kills, losses, first-combat tick as a tag byte plus fixed-width payload).` The cooldown column needs no mention here — `EntityStore::hash_into` already carries it per slot.
- [ ] 5. **Enemy objective + enemy AI system** in `world.rs` (place both methods directly after `gather`, line 2085 region).
  - [ ] 5.1 Add:
    ```rust
    /// Re-derive the enemy faction's objective from the live world: the
    /// approach cell of the live player building nearest
    /// [`Self::enemy_objective_origin`] (ascending scan with a strict `<`,
    /// so a tie goes to the lower slot), or `None` when no player building
    /// is left.
    ///
    /// The approach cell, not the building's own cell: a finished
    /// building's footprint is blocked in the inflated centre mask, so a
    /// pooled field to its own cell cannot be built — the objective must be
    /// a cell a body can stand on, exactly as a hauler's drop-off leg
    /// targets one.
    fn recompute_enemy_objective(&mut self) {
        self.enemy_objective_dirty = false;
        let mut best: Option<(f32, usize)> = None;
        for slot in 0..self.entities.slot_count() {
            if !self.entities.alive(slot)
                || self.entities.owner(slot) != OWNER_PLAYER
                || !matches!(self.entities.kind(slot), EntityKind::Building(_))
            {
                continue;
            }
            let d = dist2(self.entities.position(slot), self.enemy_objective_origin);
            if best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, slot));
            }
        }
        self.enemy_objective = best.map(|(_, slot)| {
            let id = self.entities.id_at(slot).expect("live building");
            entity_approach_cell(&self.static_nav, &self.entities, id, UnitKind::Ghoul).0
        });
    }

    /// Enemy-AI system: every idle enemy unit is sent marching at the
    /// faction objective, and one already marching somewhere stale is
    /// re-aimed. Runs immediately before combat so a fresh order can still
    /// fire this tick.
    fn enemy_ai(&mut self) {
        if self.enemy_objective_dirty {
            self.recompute_enemy_objective();
        }
        let Some(obj) = self.enemy_objective else {
            // Nothing left to march on: marchers stop. A forced `Attack`
            // (test seam) keeps its target; combat clears it on death.
            for i in 0..self.live_scratch.len() {
                let slot = self.live_scratch[i];
                if self.entities.alive(slot)
                    && self.entities.owner(slot) == OWNER_ENEMY
                    && matches!(self.orders.get(slot), Order::AttackMove { .. })
                {
                    self.orders.clear(slot);
                }
            }
            return;
        };
        // One pooled field for the whole faction, acquired at most once per
        // tick — hundreds of enemies, one field.
        let mut field: Option<FieldRef> = None;
        for i in 0..self.live_scratch.len() {
            let slot = self.live_scratch[i];
            if !self.entities.alive(slot)
                || self.entities.owner(slot) != OWNER_ENEMY
                || !matches!(self.entities.kind(slot), EntityKind::Unit(_))
            {
                continue;
            }
            let stale = match self.orders.get(slot) {
                Order::Idle => true,
                Order::AttackMove { goal, .. } => goal.anchor != obj,
                _ => false,
            };
            if !stale {
                continue;
            }
            let f = match field {
                Some(f) => f,
                None => match self.nav.acquire(obj) {
                    Ok(f) => {
                        field = Some(f);
                        f
                    }
                    // No field to the objective right now: leave the
                    // faction idle and retry next tick, rather than order
                    // half of it.
                    Err(_) => return,
                },
            };
            self.orders.set(
                slot,
                Order::AttackMove {
                    goal: FormationGoal::at(obj),
                    field: f,
                },
            );
        }
    }
    ```
  - [ ] 5.2 Hook the dirty flag into T1's death routing: locate the branch of `apply_damage`'s death path that handles `EntityKind::Building(_)` targets (`grep -n "fn apply_damage" crates/mmd-engine/src/rts/world.rs`, follow the building arm; T1 landed it). The owner is read before the despawn there (T1 needs it too for its own routing — if not, read it first). Immediately after the building's despawn, insert:
    ```rust
    if owner == OWNER_PLAYER {
        // The enemy faction may have just lost its objective.
        self.enemy_objective_dirty = true;
    }
    ```
    (Adapt the local variable name to T1's code; the condition and placement — after the building despawn, any owner check on T1's routing left intact — are the contract.)
- [ ] 6. **Combat system** in `world.rs` (place directly after `enemy_ai`).
  - [ ] 6.1 Add the two targeting helpers:
    ```rust
    /// The nearest live hostile of `slot`'s owner whose surface is within
    /// `range` cells — units by centre distance minus body radius,
    /// buildings by footprint distance, nodes never. Ascending slot scan
    /// with a strict `<`, so an exact tie goes to the lower slot.
    fn nearest_hostile_in_range(&self, slot: usize, range: f32) -> Option<EntityId> {
        let p = self.entities.position(slot);
        let own = self.entities.owner(slot);
        let mut best: Option<(f32, usize)> = None;
        for i in 0..self.live_scratch.len() {
            let t = self.live_scratch[i];
            if t == slot || !self.entities.alive(t) {
                continue;
            }
            let owner = self.entities.owner(t);
            if owner == own || owner == OWNER_NEUTRAL {
                continue;
            }
            let Some(d) = surface_distance(&self.entities, p, t) else {
                continue;
            };
            if d <= range && best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, t));
            }
        }
        best.map(|(_, t)| self.entities.id_at(t).expect("live target"))
    }

    /// Whether `target` is live, hostile to `slot`'s owner and within
    /// `range` of it, by the same surface rule the acquire scan uses.
    fn target_in_range(&self, slot: usize, target: EntityId, range: f32) -> bool {
        let Some(t) = self.entities.slot(target) else {
            return false;
        };
        let owner = self.entities.owner(t);
        if owner == self.entities.owner(slot) || owner == OWNER_NEUTRAL {
            return false;
        }
        surface_distance(&self.entities, self.entities.position(slot), t)
            .is_some_and(|d| d <= range)
    }
    ```
  - [ ] 6.2 Add the system:
    ```rust
    /// Combat system: instant-hit fire. For every live armed unit, in
    /// ascending slot order: tick the cooldown down, pick a target under
    /// the firing rules, and — at cooldown 0 — apply the damage and reset
    /// the cooldown, so the firing period is exactly `cooldown_ticks` and a
    /// fresh spawn (cooldown 0) fires the first tick it has a target.
    ///
    /// Firing rules: `Idle` and `AttackMove` auto-acquire the nearest
    /// hostile in range and fire in place; `Attack` fires only at its own
    /// target and walks while out of range; a dead `Attack` target clears
    /// the order to `Idle` (the enemy AI re-marches an enemy next tick) and
    /// the unit defends itself under the `Idle` rule the same tick.
    /// `Move`, `Gather` and `Build` never fire.
    ///
    /// Deaths resolve immediately through `apply_damage`, so a later slot
    /// never shoots a corpse. [`Self::combat_hold`] records who has a live
    /// target in range; the movement system holds those units in place.
    fn combat(&mut self) {
        self.combat_hold.fill(false);
        for i in 0..self.live_scratch.len() {
            let slot = self.live_scratch[i];
            if !self.entities.alive(slot) {
                continue;
            }
            let EntityKind::Unit(kind) = self.entities.kind(slot) else {
                continue;
            };
            let Some(w) = weapon(kind) else {
                continue;
            };
            let cd = self.entities.cooldown(slot);
            if cd > 0 {
                self.entities.set_cooldown(slot, cd - 1);
            }
            let target = match self.orders.get(slot) {
                Order::Idle | Order::AttackMove { .. } => {
                    self.nearest_hostile_in_range(slot, w.range_cells)
                }
                Order::Attack { target, .. } => {
                    if self.entities.contains(target) {
                        self.target_in_range(slot, target, w.range_cells)
                            .then_some(target)
                    } else {
                        self.orders.clear(slot);
                        self.nearest_hostile_in_range(slot, w.range_cells)
                    }
                }
                Order::Move { .. } | Order::Gather { .. } | Order::Build { .. } => continue,
            };
            let Some(target) = target else {
                continue;
            };
            self.combat_hold[slot] = true;
            if self.entities.cooldown(slot) != 0 {
                continue;
            }
            let target_owner = {
                let t = self.entities.slot(target).expect("live target");
                self.entities.owner(t)
            };
            let _ = self.apply_damage(target, w.damage);
            if self.first_combat_tick.is_none() {
                self.first_combat_tick = Some(self.tick_index as u32);
            }
            if !self.entities.contains(target) {
                if target_owner == OWNER_ENEMY {
                    self.kills += 1;
                } else if target_owner == OWNER_PLAYER {
                    self.losses += 1;
                }
            }
            self.entities.set_cooldown(slot, w.cooldown_ticks);
        }
    }
    ```
  - [ ] 6.3 In `tick` (line 1730): insert the two calls between `self.gather();` and `self.movement();`:
    ```rust
    self.gather();
    self.enemy_ai();
    self.combat();
    self.movement();
    ```
    Rewrite the doc comment's two lists so enemy AI and combat sit between orders and movement, renumbering everything after — with T2's wave system in place the target list is: `1. commands, 2. camera, 3. waves, 4. construction, 5. production, 6. orders, 7. enemy AI, 8. combat, 9. movement, 10. supply recount.` (keep whatever wording T2 landed for waves; mirror it in the "Today … run" prose sentence).
- [ ] 7. **Movement integration** in `step_one_unit` (`world.rs:3059`): in the `let (goal, field) = match order` block, insert after the `Order::Build { … }` arm and before the catch-all `_ => return,`:
  ```rust
  Order::Attack { target, field } => {
      let Some(t_slot) = self.entities.slot(target) else {
          // Only armed units get their stale Attack cleared by the
          // combat system; an unarmed one under the test seam stops here.
          self.orders.clear(slot);
          return;
      };
      if self.combat_hold[slot] {
          // In range: combat is firing, the walk pauses.
          return;
      }
      let cell = match self.entities.kind(t_slot) {
          EntityKind::Unit(_) => node_cell(self.entities.position(t_slot)),
          EntityKind::Building(_) => {
              entity_approach_cell(&self.static_nav, &self.entities, target, kind).0
          }
          // A node is never a combat target.
          EntityKind::Node(_) => {
              self.orders.clear(slot);
              return;
          }
      };
      (FormationGoal::at(cell), field)
  }
  Order::AttackMove { goal, field } => {
      if self.combat_hold[slot] {
          // In range: hold and let combat fire; descent resumes when the
          // target dies.
          return;
      }
      (goal, field)
  }
  ```
  Nothing else in `step_one_unit` changes: the existing step-3 re-path (`is_current`/`acquire`) is what chases a moving `Attack` target across cell boundaries (a changed dest is a stale handle), and the existing step-4 zero-vector/`reachable` rule already handles an `AttackMove` unit standing on its goal (sink → stands, keeps the order — the march is permanent by design). Arrival-clearing stays `Move`-only (`is_move_order`).
- [ ] 8. **Tests** — new file `crates/mmd-engine/tests/rts_combat.rs` with the header/helpers block from *Test plan* verbatim, then one `#[test]` per row, exactly as specified in the table (setups, tick counts and assertions are all given there):
  - [ ] 8.1 `soldier_auto_acquires_idle`
  - [ ] 8.2 `plain_move_never_fires`
  - [ ] 8.3 `nearest_target_lowest_slot_tie`
  - [ ] 8.4 `cooldown_gates_fire_rate`
  - [ ] 8.5 `ghouls_march_on_hq`
  - [ ] 8.6 `ghoul_attacks_first_thing_in_range`
  - [ ] 8.7 `ghouls_besiege_and_kill_hq`
  - [ ] 8.8 `objective_retargets_on_hq_death`
  - [ ] 8.9 `no_player_buildings_enemies_idle`
  - [ ] 8.10 `counters_and_first_combat_tick`
  - [ ] 8.11 `combat_determinism` (fixture, 600 lockstep ticks, per-tick hash equality)
  - [ ] 8.12 `cooldown_enters_state_hash`
  - [ ] 8.13 `field_pool_not_churned` (own 160×160 spec with 300 pre-placed Ghouls via T2's `EnemySpec`; pool stats seams `rebuild_count()`/`acquire_count()`)
- [ ] 9. **Allocation test** — `crates/mmd-engine/tests/frame_allocations.rs`: add `combat_march_allocates_nothing` per the last Test-plan row, copying the `lock_alloc_tests()` / `reset_count()` / warm-up-outside-guard / `MeasureGuard::enter()` / `guard.assert_zero()` shape of `movement_allocates_nothing` (line ~733), with the doc comment noting the warm-up absorbs the pool's bounded miss exception and the guarded window contains real fire and two despawns.

## Outputs

- Files: `crates/mmd-engine/src/rts/combat.rs` (new), `orders.rs`, `entity.rs`, `world.rs`, `mod.rs`, `crates/mmd-engine/tests/rts_combat.rs` (new), `crates/mmd-engine/tests/frame_allocations.rs`.
- Public API next tickets consume verbatim: `Order::Attack { target: EntityId, field: FieldRef }` (tag 4); `Order::AttackMove { goal: FormationGoal, field: FieldRef }` (tag 5); `weapon(kind: UnitKind) -> Option<Weapon>` with `Weapon { damage: u32, cooldown_ticks: u32, range_cells: f32 }`; `RtsWorld::kills() -> u32`, `RtsWorld::losses() -> u32`, `RtsWorld::first_combat_tick() -> Option<u32>`.
- No config/migration; no tracked-scene edit (the fixture is T2's; the gate scene is untouched until T7).

## Validation

- [ ] `cargo test -p mmd-engine --test rts_combat --locked` — every test above green.
- [ ] `cargo test --workspace --locked` — full suite green (in particular `frame_allocations`, `rts_acceptance`, and the in-crate `orders`/`world` unit tests: `the_worker_is_the_faster_unit` still holds at Ghoul 18.0).
- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- [ ] `cargo run -- rts --frames 1600 --inject-input-file assets/scenarios/rts_acceptance_v1.script` — exits 0; the gate scene has no enemies, so behaviour is byte-identical (hash *values* may differ from pre-T1 recordings since the hash composition grew, but this run pins tokens, not a stored hash).
- [ ] `git status --porcelain -- assets/scenarios/rts_prototype_v1.ron assets/scenarios/rts_acceptance_v1.script` — empty (no tracked-scene edit).
- [ ] commit msg draft: `feat(rts): instant-hit combat, enemy march ai and auto-acquire`
