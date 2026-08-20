# T5: Turret

**Plan:** `./artifacts/PLAN_2026_08_17_combat-prototype.md`
**Depends:** T3
**Commit outcome:** `BuildingKind::Turret` is worker-built for 75 crystal under the existing placement rules and auto-fires the nearest enemy in range once finished.

## Context (self-contained)

- Goal: Phase 2 Combat Prototype — weapons, damage, turrets, enemy AI. Success = scripted combat run + exit tokens.
- This slice: the static weapon. First attacking building; rides every existing building pipeline (placement, construction, footprint stamp, pick, card).
- Out of scope here: player unit commands (T4, parallel — do not touch `Order` issue paths or T4's command tokens), HP bars (T6), gate scene/script (T7). No tech prerequisite, no ammo/decay, no power/territory rule. `sim/` frozen. Docs untouched (T8 owns the close doc).
- Assumptions in force: Turret stats 150 HP / 1 armor, damage 10, cooldown 20 ticks, range 36 cells, footprint 6 × 6 cells, cost 75 crystal, grants no supply, is not a drop-off; placement uses the existing four ordered rules unchanged (in bounds, terrain, building overlap, resource nodes); an unfinished turret site never fires; finished footprint stamps solid exactly like Depot/Barracks (atomic evacuation preplan included, `world.rs::finish_site` — nothing new to write there).
- **Baseline caveat:** every line number below was inspected on the pre-combat branch tip (`b130bfc`, before T1–T4 land). T1/T2/T3 shift lines; anchors are given as *symbol + pre-combat line*. Re-locate by symbol, never by raw number.

## Decisions (made while detailing — do not reopen, do not re-derive)

- **D1 — build time is a literal 180.** Depot's constant is `DEPOT_BUILD_TICKS: u32 = 180` (`build.rs:49`, house style: every duration is its own literal). `TURRET_BUILD_TICKS: u32 = 180` — same value, own constant, and the test pins `TURRET_BUILD_TICKS == DEPOT_BUILD_TICKS` so a later Depot retune forces a deliberate choice here.
- **D2 — building-firer geometry.** Effective distance from a turret to a unit target = `rect_distance(target_pos, turret_pos, TURRET_FOOTPRINT_CELLS) − target_kind.body_radius_cells()` (`rect_distance` is `pub(crate)` at `orders.rs:261`, point-to-rect, `0.0` inside). In range iff `≤ range_cells`. Building firers scan **enemy units only**: the enemy faction cannot own buildings (T2's schema spawns only Ghouls), so a building-vs-building distance rule would be dead code. **Unit-firer math is untouched** — do not reroute units through `rect_distance` with edge 1; that shifts their numbers by up to 0.5 cells and breaks T3's pins.
- **D3 — card wiring is purely positional; zero input-table change.** `KEY_BINDINGS` (`src/rts_input.rs:55-63`) already maps `A → RtsCommand::ExecuteSlot(3)` and `execute_slot` dispatches whatever `command_slots` put in slot 3. Turret takes slot 3 (row-major next free after HQ/Depot/Barracks at 0/1/2), key `A` follows automatically. `BUILD_MENU` (`hud.rs:594`) grows to 4 entries with `(b'A', BuildingKind::Turret)` so the display-order table stays truthful.
- **D4 — placeholder art is generated, not authored.** The tracked sheets under `assets/sprites/generated/rts/` come from `xtask/src/placeholder_art.rs` and are hash-pinned by `manifest.json` (`atlases --check` is on the merge gate). Turret gets buildings-sheet cells `(row 0, col 3)` finished / `(row 1, col 3)` site (`building_uv` uses `kind as u32` as the column — col 3 is free, sheet is 4 × 8) and props cell 16 = `(row 4, col 0)` for the card icon (`prop_uv(i) = (i/4, i%4)`; cells 0–15 are taken). Regenerate with `cargo run -p xtask -- atlases` and commit the changed PNGs + manifest. Two xtask test pins move (steps 5.4/5.5).
- **D5 — pinned test names are kept.** `worker_card_uses_stable_three_build_slots` (`tests/rts_hud.rs:551`) is named in both functional-close docs, and `validation_contract.rs` resolves those names against the binaries. The *body* is extended to slot 3; the name stays. Same rule everywhere: extend bodies, never rename.
- **D6 — turret fire is silent and unanimated this phase.** No SFX asset, no muzzle frame. Recorded as a known gap for T8's close doc (see Outputs). Build accept/reject receipts are untouched — `handle_hud_click`/`execute_slot` emit `UiCue::CommandGrid` generically.
- **D7 — the finish-tick boundary is pinned with a testkit fast-forward.** A naturally-attended site finishes at a walk-dependent tick; the test instead confirms silence while building, then `set_progress(slot, TURRET_BUILD_TICKS − 1, TURRET_BUILD_TICKS)` so exactly one attended tick remains, and asserts the first shot lands at (or one tick after — intra-tick scan order is T3's pin) the finish.
- **D8 — single-tick geometry assertions are safe under the enemy AI.** T3's tick order is AI → combat → movement, so the first combat scan reads a Ghoul exactly where the test spawned it; drift (18 c/s = 0.3 c/tick) only matters in multi-tick windows, which the tests bound generously.
- **D9 — `building_weapon` lives in `crates/mmd-engine/src/rts/combat.rs`,** beside T3's `weapon(kind: UnitKind)` — T3's ticket Outputs name that module. If T3 landed the weapon table elsewhere, put `building_weapon` beside it and adjust step 3's paths; the signature is the contract, not the file.

## Requirements

- `BuildingKind::Turret = 3` appended (`entity.rs:49-53` — discriminants appended, never inserted; `EntityKind::tag()` then yields `0x20 | 3 = 0x23` with no edit). `footprint_cells()` → new `pub const TURRET_FOOTPRINT_CELLS: u32 = 6` in `scenario.rs` beside HQ 12 / Depot 8 / Barracks 10 (lines 96-102). `is_drop_off()` needs **no edit** — it is `matches!(self, Self::Hq)` (`entity.rs:104-106`).
- Stats into T1's kind tables (`entity.rs`): `max_hp(Building(Turret)) = 150`, `armor = 1`, via new consts `TURRET_MAX_HP` / `TURRET_ARMOR` in T1's const block, exported from `rts/mod.rs`.
- Cost + build time in `build.rs` (costs live there, not `economy.rs`): `TURRET_COST = Resources { crystal: 75, gas: 0 }`, `TURRET_BUILD_TICKS = 180` (D1), `TURRET_SUPPLY_GRANT = 0`; arms in `building_cost` / `build_ticks` / `supply_grant`. `can_produce` (`production.rs:53`) already rejects every `(Turret, unit)` pair via its tuple `matches!` — no edit.
- Building weapon in `combat.rs`: `pub fn building_weapon(kind: BuildingKind) -> Option<Weapon>` — Turret `Some(Weapon { damage: 10, cooldown_ticks: 20, range_cells: 36.0 })`, others `None`. Cooldown column already exists for all entities (T3).
- Combat system (T3's) includes finished buildings with a weapon as firers: static, no order gate (buildings have no orders); target = nearest enemy unit by D2's effective distance, lowest-target-slot tie-break, fires on cooldown 0, resets to `cooldown_ticks`, damage through the one `apply_damage` seam (kills / `first_combat_tick` counters ride along for free). A site (`progress_target != 0`) never target-scans. Deterministic: the turret joins the existing single ascending-slot firer pass — no second loop, no allocation.
- Enemies target turrets already via T3's rules (a turret is a player building — valid melee objective and in-range target); T1's `apply_damage` covers its death (un-stamp + mask replace + no grant to revoke since `TURRET_SUPPLY_GRANT = 0` + losses). **No new death code.**
- Worker build card: `CommandId::BuildTurret` at slot 3 for a worker selection; icon `Prop::IconBuildTurret`; `execute_command` arm calls `begin_placement(BuildingKind::Turret)`; key `A` positional (D3). Ghost, assisted `placement_candidate`, preview→commit sharing: all inherited, only data.
- New exhaustive-match arms (a new `BuildingKind` variant breaks these compiles): `entity.rs::footprint_cells`, `build.rs::{building_cost, build_ticks, supply_grant}`, T1's `max_hp`/`armor`, `hud.rs::kind_label` → `"TURRET"`, `hud.rs::command_icon` (new `CommandId` variant), `src/rts_overlay.rs::ghost_name` → `"turret"`, `src/rts_ui.rs::execute_command`. Generic paths needing **nothing**: `building_uv` (col = `kind as u32`), `portrait_source`, `push_detail_text` building arm, `building_quad_px`, minimap, pick/selection, `placement_valid`, `confirm_placement`, `finish_site`, supply recount.
- State hash: no new columns (kind tag `0x23` flows through `EntityStore::hash_into`; cooldown column is T3's). No hash-composition edit.

## Inputs (inspected, pre-combat baseline `b130bfc`)

- **From T3 (verbatim):** `pub struct Weapon { pub damage: u32, pub cooldown_ticks: u32, pub range_cells: f32 }` and `weapon(kind: UnitKind) -> Option<Weapon>` in new `crates/mmd-engine/src/rts/combat.rs`; combat system between orders and movement in `RtsWorld::tick`, ascending slot order, cooldown column in `EntityStore`, `RtsWorld::{kills(), losses(), first_combat_tick()}`; enemy AI: Idle enemy → `AttackMove` at objective (HQ; HQ dead → nearest player building; none → Idle), halt-and-fire when its own target is in range.
- **From T2 (verbatim):** `OWNER_ENEMY: u8 = 1`; `UnitKind::Ghoul` (tag `0x12`, radius 3.0, 30 HP / 0 armor, speed 18 c/s from T3); `kind_tags_are_distinct` now asserts 8.
- **From T1 (verbatim):** `RtsWorld::apply_damage(&mut self, target: EntityId, damage: u32) -> DamageResult`; death routing (un-stamp exact inverse, `replace_blocked_mask` all-or-nothing, queue clear, no refund); `EntityStore::hp(slot)`; `max_hp`/`armor` exhaustive tables + const block in `entity.rs`; test file `tests/rts_combat.rs` whose `build_and_finish` pattern this ticket's helper copies.
- `crates/mmd-engine/src/rts/entity.rs` — `BuildingKind` 49-53 (`Hq = 0, Depot = 1, Barracks = 2`); `EntityKind::tag` 72-78 (`0x20 | k` for buildings — automatic); `footprint_cells` impl 94-100; `is_drop_off` 103-106 (`matches!(self, Self::Hq)` — untouched).
- `crates/mmd-engine/src/scenario.rs` — footprint consts 96-102; `Cell` 113. Consts are `pub` directly on `mmd_engine::scenario`.
- `crates/mmd-engine/src/rts/build.rs` (286 lines) — costs `HQ_COST/DEPOT_COST/BARRACKS_COST` 25-38 + `building_cost` 41-47; `HQ/DEPOT/BARRACKS_BUILD_TICKS` 48-51 + `build_ticks` 54-60; supply grants 62-75; `PlacementError` 85-104; `placement_valid` 121-207 (four ordered rules, all keyed on `kind.footprint_cells()` — **zero edits in this file beyond the constants and arms**); `placement_candidate` 216-273.
- `crates/mmd-engine/src/rts/orders.rs` — `rect_distance(p, center, edge)` 260-268 (`pub(crate)`, `0.0` inside); `interaction_reach` 41-43 (the geometry family the plan cites); `dist2` 254.
- `crates/mmd-engine/src/rts/world.rs` — `begin_placement` 1367 (affordability only); `confirm_placement` 1387-1440 (spawns building **centred** at `min + edge/2` → a turret at min (180,176) sits at `[183.0, 179.0]`; sets `progress(0, build_ticks(kind))`; debits); `is_site` 1474; `finish_site` 1909-1969 (evacuation preplan + stamp + `rebuild_center_blocked` + `replace_blocked_mask` + `grant_cap` — `grant_cap(0)` is a no-op, so Turret rides it unchanged); testkit `entities_mut` 859 (arms overlap-repair), `force_position_for_test` 873; `start_hq` 958; `order_gather`; `tick` 1730.
- `crates/mmd-engine/src/rts/hud.rs` (1933 lines) — `COMMAND_SLOT_KEYS: [u8; 9] = *b"QWEASDZXC"` 468; `BUILD_MENU: [(u8, BuildingKind); 3]` 594-598; `kind_label` 651-662; `CommandId` 665-672; `command_icon` 689-697; `command_slots` 711-762 (worker branch fills `out[0..=2]`); `command_slot_rect` 1233; the grid renderer draws all 9 cells + positional key letters generically 1244-1277.
- `crates/mmd-engine/src/rts/pack.rs` — `Prop` enum 57-74 (`IconSetRally = 15` last; sheet 4 cols × 8 rows, `FRAMES_X = 4` / `FRAMES_Y = 8` in `render/atlas.rs:22-24`); `prop_uv` 77-80; `building_uv` 83-90 (col = `kind as u32` — col 3 free); `unit_slot` 104.
- `crates/mmd-engine/src/rts/mod.rs` — `pub use build::{…}` 17-22; `pub use entity::{…}` 31-34; T3 adds a `pub use combat::{…}` block.
- `src/rts_ui.rs` — `execute_command` 450-477 (`CommandId::BuildBarracks` arm 458-460 is the template); `execute_slot` 483-492; tests module: `test_world()` 1840 (loads the tracked scenario without testkit), `RtsSession::for_test()`, `select_hq` 1735.
- `src/rts_input.rs` — `KEY_BINDINGS` 51-64: `A → ExecuteSlot(3)` already. **No edit.**
- `src/rts_overlay.rs` — `ghost_name` 31-37.
- `xtask/src/placeholder_art.rs` — icon colour consts 64-71; `buildings_table` 184-197 (rows 0/1 cols 0-2 used, row 2 = nodes; `(0,3)`/`(1,3)` free); `draw_props_cell` 248-321 (icons at `(2,2)…(3,3)`; `(4,0)` free); `draw_hud_icon` 324-326; tests: `unused_table_rows_are_fully_transparent` ~908 (buildings rows `3..`, props rows `4..` — props loop must skip `(4,0)`), `hud_icon_cells_are_opaque_and_distinct` ~934 (rows `2..=3`; must gain `(4,0)`), `extract_cell(&px, w, col, row)` argument order.
- Tracked scene `assets/scenarios/rts_prototype_v1.ron` — 320 × 320, `start_crystal: 300`, HQ min (160,160) edge 12, first crystal node (140,150), workers seeded near (162-167, 178). **Verified against the obstacle list:** 6×6 footprints from (180,176), (200,178), (138,148), (130,148) cover no obstacle; footprint from (0,0) covers obstacle (0,0); cells (210,179), (217,179), (155,179), (220,179), (224,179), (225,179) and the swarm column x=201, y=149..227 are all unblocked.
- `mmd_engine::testkit::RtsHarness` (`testkit/rts.rs`) — `scene()`, `step_exact`, `world()/world_mut()`, `ids_of_kind`, `state_hash`.
- Merge gate (`docs/05-testing.md:95-113`): fmt, `cargo test --workspace --locked`, clippy `-D warnings`, `nix flake check`, xtask `bootstrap/shaders/atlases/audio --check`, three `run` smokes, `rts --frames 1600 --inject-input-file assets/scenarios/rts_acceptance_v1.script`.

## TDD

1. **Red** — step 1 lands every test edit first. Red = the workspace fails to compile the new test crate (missing `BuildingKind::Turret`, `TURRET_*`, `building_weapon`, `CommandId::BuildTurret`, `Prop::IconBuildTurret`) plus the extended pins failing. Do not weaken a test to dodge it.
2. **Green** — steps 2-5, minimal code.
3. **Refactor** — keep green; fmt + clippy are the gate, not polish.

## Test plan

New file `crates/mmd-engine/tests/rts_turret.rs` (exact content in step 1.1). Run: `cargo test -p mmd-engine --test rts_turret`. Sibling pins: `--test rts_world --test rts_hud --test rts_pack` and `cargo test -p xtask`. App-side: `cargo test --bin mmd` (or the workspace run).

| Test (exact name) | File | Input | Expect |
| ---- | ---- | ----- | ------ |
| `turret_stats_cost_and_footprint_are_published` | `rts_turret.rs` | tables/consts | tag `0x23`; footprint 6; 150 HP / 1 armor; cost 75c/0g; build ticks == `DEPOT_BUILD_TICKS` == 180; grant 0; `is_drop_off()` false |
| `building_weapon_arms_only_the_turret` | `rts_turret.rs` | table | Turret `Some(10, 20, 36.0)` by field; Hq/Depot/Barracks `None` |
| `turret_placeable_under_four_rules` | `rts_turret.rs` | 5 probes on tracked scene | `Ok` at (180,176); `OutOfBounds` at (315,315); `BlockedTerrain{0,0}` at (0,0); `OverlapsBuilding` vs a raw Depot *site* at (200,178); `CoversNode{140,150}` at (138,148) |
| `turret_costs_75_and_builds` | `rts_turret.rs` | worker builds at (180,176) | crystal −75 at confirm, gas unchanged; site → finished ≤ 2000 ticks; centre cell stamped in `placement_solids` **and** pool `blocked`; supply cap unchanged |
| `unfinished_turret_never_fires` | `rts_turret.rs` | Ghoul at 31.5 eff cells of an attended site | HP 30 across 10 ticks; after fast-forward to last attended tick → finished, first hit → HP 20 (D7) |
| `turret_auto_fires_nearest` | `rts_turret.rs` | Ghouls at 21.5 / 28.5 eff cells | tick 1: near 20, far 30; ≤ tick 20: near still 20; ≤ tick 25: near 10; ≤ tick 50: near dead; ≤ tick 90: far damaged or dead |
| `turret_target_ties_break_to_lowest_slot` | `rts_turret.rs` | two Ghouls at exactly 24.5 rect cells (east spawned first) | tick 1: east 20, west 30 |
| `turret_range_measured_from_footprint` | `rts_turret.rs` | Ghoul at 35.9 / 36.1 eff cells (x = 224.9 / 225.1) | 35.9 → hit (a centre-measured turret would read 38.9 and miss — the pinned 3-cell claim); 36.1 → no hit |
| `ghouls_kill_turret` | `rts_turret.rs` | HQ killed, 12-Ghoul swarm, ≤ 3000 ticks | turret dead, centre cell un-stamped in both masks, `losses()` ≥ baseline + 1 |
| `turret_grants_no_supply_not_dropoff` | `rts_turret.rs` | turret at (130,148), 4.5 cells from node vs HQ's 21.7 | cap unchanged; gather round-trip `Returning { drop_off }` == the HQ |
| `turret_card_button_positional` | `rts_turret.rs` | worker selected | `slots[3] == BuildTurret` enabled; `COMMAND_SLOT_KEYS[3] == b'A'`; `BUILD_MENU[3] == (b'A', Turret)`, len 4 |
| `kind_tags_are_distinct` (extend) | `tests/rts_world.rs:175` | + Turret | 9 distinct tags |
| `footprints_come_from_the_scenario_constants` (extend) | `tests/rts_world.rs:153` | + Turret | 6 |
| `only_the_hq_takes_a_drop_off` (extend) | `tests/rts_world.rs:168` | + Turret | false |
| `worker_card_uses_stable_three_build_slots` (extend, name kept — D5) | `tests/rts_hud.rs:551` | worker selection | slot 3 = BuildTurret enabled; slots 4.. empty |
| `prop_uv_maps_to_the_published_cells` (extend) | `tests/rts_pack.rs:103` | + `IconBuildTurret` | cell (4,0) |
| `building_uv_switches_row_on_construction` (extend) | `tests/rts_pack.rs:139` | + Turret | rows 0/1, col 3 |
| `unused_table_rows_are_fully_transparent` (adjust) | `xtask/src/placeholder_art.rs` ~908 | props sheet | rows 4+ transparent **except** (4,0) |
| `hud_icon_cells_are_opaque_and_distinct` (extend) | `xtask/src/placeholder_art.rs` ~934 | + cell (4,0) | 9 opaque, pairwise distinct |
| `a_executes_turret_placement` (new) | `src/rts_ui.rs` tests | worker selected, `execute_slot(3)` | dispatched; `placement() == Pending { Turret }` |

## Impl steps

- [ ] 1. Red — land every test first
  - [ ] 1.1 Create `crates/mmd-engine/tests/rts_turret.rs` with exactly this content:

    ```rust
    //! Combat T5 — the Turret: the first armed building.
    //!
    //! Headless CPU checks through `testkit::RtsHarness` on the tracked scene
    //! (320×320, HQ min corner (160,160), 300 starting crystal). Enemies are
    //! raw testkit spawns; T3's enemy AI marches every Idle Ghoul at the HQ,
    //! so single-tick assertions read positions *as spawned* (the combat scan
    //! runs before movement) and longer windows tolerate the 0.3 cells/tick
    //! drift with generous bounds.

    use mmd_engine::rts::{
        BUILD_MENU, BuildingKind, COMMAND_SLOT_KEYS, CommandId, DEPOT_BUILD_TICKS, DamageResult,
        EntityId, EntityKind, GatherPhase, OWNER_ENEMY, OWNER_PLAYER, Order, PlacementError,
        ResourceKind, Resources, TURRET_ARMOR, TURRET_BUILD_TICKS, TURRET_COST, TURRET_MAX_HP,
        TURRET_SUPPLY_GRANT, UnitKind, armor, build_ticks, building_cost, building_weapon,
        command_slots, max_hp, placement_valid, supply_grant,
    };
    use mmd_engine::scenario::{Cell, TURRET_FOOTPRINT_CELLS};
    use mmd_engine::testkit::RtsHarness;

    /// Obstacle-free 6×6 corner of the tracked scene (verified against the
    /// RON's obstacle list) — the corner family `rts_production.rs` builds at.
    const TURRET_CORNER: Cell = Cell { x: 180, y: 176 };
    /// A building spawns centred at `min + edge/2`, so this turret's centre.
    /// Its footprint rectangle spans x 180..186, y 176..182.
    const TURRET_CENTER_CELL: (u32, u32) = (183, 179);
    /// Obstacle-free 6×6 corner whose finished turret sits 4.5 cells from
    /// the first crystal node (140,150) — the HQ is ~21.7 away, so a
    /// drop-off bug would pick the turret.
    const NODE_SIDE_CORNER: Cell = Cell { x: 130, y: 148 };

    fn first_worker(h: &RtsHarness) -> EntityId {
        h.ids_of_kind(EntityKind::Unit(UnitKind::Worker))[0]
    }

    fn hp_of(h: &RtsHarness, id: EntityId) -> u32 {
        let slot = h.world().entities().slot(id).expect("live entity");
        h.world().entities().hp(slot)
    }

    /// Raw testkit Ghoul spawn. Arms the overlap-repair pass; every position
    /// in this file is ≥ 7 cells from any other body, so the pass finds
    /// nothing to repair.
    fn spawn_ghoul(h: &mut RtsHarness, pos: [f32; 2]) -> EntityId {
        h.world_mut()
            .entities_mut()
            .spawn(EntityKind::Unit(UnitKind::Ghoul), OWNER_ENEMY, pos)
            .expect("store has room")
    }

    /// Place and fully attend a turret until it finishes. Generous bound:
    /// the builder walks in first (2000 ≫ walk + 180 attended ticks).
    fn build_and_finish_turret(h: &mut RtsHarness, corner: Cell, builder: EntityId) -> EntityId {
        assert!(h.world_mut().begin_placement(BuildingKind::Turret));
        let site = h
            .world_mut()
            .confirm_placement(corner, builder)
            .expect("confirm turret placement");
        h.step_exact(2_000);
        assert!(
            !h.world().is_site(site),
            "turret must finish within 2000 ticks"
        );
        site
    }

    /// Fresh scene, finished turret at [`TURRET_CORNER`], one Ghoul at
    /// `pos`, one tick: the Ghoul's HP after the turret's first scan (which
    /// reads the spawn position — combat runs before movement).
    fn fire_probe(pos: [f32; 2]) -> u32 {
        let mut h = RtsHarness::scene().build().expect("rts scene harness");
        let w0 = first_worker(&h);
        build_and_finish_turret(&mut h, TURRET_CORNER, w0);
        let g = spawn_ghoul(&mut h, pos);
        h.step_exact(1);
        hp_of(&h, g)
    }

    // --- published surface ---------------------------------------------------

    #[test]
    fn turret_stats_cost_and_footprint_are_published() {
        assert_eq!(EntityKind::Building(BuildingKind::Turret).tag(), 0x23);
        assert_eq!(TURRET_FOOTPRINT_CELLS, 6);
        assert_eq!(
            BuildingKind::Turret.footprint_cells(),
            TURRET_FOOTPRINT_CELLS
        );
        assert_eq!(
            max_hp(EntityKind::Building(BuildingKind::Turret)),
            TURRET_MAX_HP
        );
        assert_eq!(TURRET_MAX_HP, 150);
        assert_eq!(
            armor(EntityKind::Building(BuildingKind::Turret)),
            TURRET_ARMOR
        );
        assert_eq!(TURRET_ARMOR, 1);
        assert_eq!(building_cost(BuildingKind::Turret), TURRET_COST);
        assert_eq!(TURRET_COST, Resources { crystal: 75, gas: 0 });
        assert_eq!(build_ticks(BuildingKind::Turret), TURRET_BUILD_TICKS);
        assert_eq!(
            TURRET_BUILD_TICKS, DEPOT_BUILD_TICKS,
            "deliberately the Depot's duration; retuning one must be a choice"
        );
        assert_eq!(TURRET_BUILD_TICKS, 180);
        assert_eq!(supply_grant(BuildingKind::Turret), TURRET_SUPPLY_GRANT);
        assert_eq!(TURRET_SUPPLY_GRANT, 0);
        assert!(!BuildingKind::Turret.is_drop_off());
    }

    #[test]
    fn building_weapon_arms_only_the_turret() {
        let w = building_weapon(BuildingKind::Turret).expect("turret is armed");
        assert_eq!(w.damage, 10);
        assert_eq!(w.cooldown_ticks, 20);
        assert_eq!(w.range_cells, 36.0);
        assert!(building_weapon(BuildingKind::Hq).is_none());
        assert!(building_weapon(BuildingKind::Depot).is_none());
        assert!(building_weapon(BuildingKind::Barracks).is_none());
    }

    // --- placement and construction ------------------------------------------

    #[test]
    fn turret_placeable_under_four_rules() {
        let mut h = RtsHarness::scene().build().expect("rts scene harness");
        // Rule order is the existing one; each probe trips exactly one rule.
        assert!(placement_valid(h.world(), BuildingKind::Turret, TURRET_CORNER).is_ok());
        assert_eq!(
            placement_valid(h.world(), BuildingKind::Turret, Cell { x: 315, y: 315 }),
            Err(PlacementError::OutOfBounds),
            "315 + 6 leaves the 320 grid"
        );
        assert_eq!(
            placement_valid(h.world(), BuildingKind::Turret, Cell { x: 0, y: 0 }),
            Err(PlacementError::BlockedTerrain { x: 0, y: 0 }),
            "the tracked scene's obstacle at flat index 0"
        );
        // A *site* is not in the stamped mask, so rule 2 passes and rule 3
        // must catch the overlap. Raw-spawn a Depot site (centre = its
        // min (198,176) + 4) on terrain-verified clear ground; no tick runs,
        // so arming the repair pass is inert.
        let site = h
            .world_mut()
            .entities_mut()
            .spawn(
                EntityKind::Building(BuildingKind::Depot),
                OWNER_PLAYER,
                [202.0, 180.0],
            )
            .expect("store has room");
        let s = h.world().entities().slot(site).expect("live site");
        h.world_mut()
            .entities_mut()
            .set_progress(s, 10, DEPOT_BUILD_TICKS);
        assert_eq!(
            placement_valid(h.world(), BuildingKind::Turret, Cell { x: 200, y: 178 }),
            Err(PlacementError::OverlapsBuilding)
        );
        assert_eq!(
            placement_valid(h.world(), BuildingKind::Turret, Cell { x: 138, y: 148 }),
            Err(PlacementError::CoversNode { x: 140, y: 150 }),
            "the first crystal node sits inside the probe footprint"
        );
    }

    #[test]
    fn turret_costs_75_and_builds() {
        let mut h = RtsHarness::scene().build().expect("rts scene harness");
        let w0 = first_worker(&h);
        let crystal_before = h.world().resources().crystal;
        let gas_before = h.world().resources().gas;
        let cap_before = h.world().supply().cap();

        assert!(h.world_mut().begin_placement(BuildingKind::Turret));
        let site = h
            .world_mut()
            .confirm_placement(TURRET_CORNER, w0)
            .expect("confirm turret placement");
        assert_eq!(h.world().resources().crystal, crystal_before - 75);
        assert_eq!(h.world().resources().gas, gas_before);
        assert!(h.world().is_site(site));

        h.step_exact(2_000);
        assert!(!h.world().is_site(site), "must finish within 2000 ticks");
        let w = h.world().static_nav().width();
        let (cx, cy) = TURRET_CENTER_CELL;
        let idx = (cx + cy * w) as usize;
        assert!(
            h.world().static_nav().placement_solids()[idx],
            "finished footprint is stamped"
        );
        assert!(
            h.world().nav().blocked()[idx],
            "the pool mask followed the stamp (fields invalidated)"
        );
        assert_eq!(h.world().supply().cap(), cap_before, "no supply grant");
    }

    // --- firing --------------------------------------------------------------

    #[test]
    fn unfinished_turret_never_fires() {
        let mut h = RtsHarness::scene().build().expect("rts scene harness");
        let w0 = first_worker(&h);
        assert!(h.world_mut().begin_placement(BuildingKind::Turret));
        let site = h
            .world_mut()
            .confirm_placement(TURRET_CORNER, w0)
            .expect("confirm turret placement");
        // Builder walks in and starts attending.
        let mut attending = false;
        for _ in 0..600 {
            h.step_exact(1);
            let slot = h.world().entities().slot(site).expect("site alive");
            if h.world().entities().progress(slot) > 0 {
                attending = true;
                break;
            }
        }
        assert!(attending, "builder must start the site within 600 ticks");

        // 31.5 effective cells: rect face x=186, minus body radius 3. Far
        // from the builder, well inside would-be weapon range.
        let g = spawn_ghoul(&mut h, [220.5, 179.0]);
        h.step_exact(10);
        assert_eq!(hp_of(&h, g), 30, "a site never target-scans");

        // Fast-forward to the finish boundary: exactly one attended tick
        // remains (the builder is still attending).
        let slot = h.world().entities().slot(site).expect("site alive");
        h.world_mut()
            .entities_mut()
            .set_progress(slot, TURRET_BUILD_TICKS - 1, TURRET_BUILD_TICKS);
        let mut finished = false;
        for _ in 0..5 {
            h.step_exact(1);
            if !h.world().is_site(site) {
                finished = true;
                break;
            }
        }
        assert!(finished, "one attended tick finishes the site");
        // First shot lands on the finish tick or the one after — the
        // intra-tick scan order is T3's pin, not this one's. Never earlier.
        if hp_of(&h, g) == 30 {
            h.step_exact(1);
        }
        assert_eq!(hp_of(&h, g), 20, "the finished turret opens fire");
    }

    #[test]
    fn turret_auto_fires_nearest() {
        let mut h = RtsHarness::scene().build().expect("rts scene harness");
        let w0 = first_worker(&h);
        build_and_finish_turret(&mut h, TURRET_CORNER, w0);
        let near = spawn_ghoul(&mut h, [210.5, 179.0]); // 21.5 effective
        let far = spawn_ghoul(&mut h, [217.5, 179.0]); // 28.5 effective
        h.step_exact(1);
        assert_eq!(hp_of(&h, near), 20, "nearest ghoul takes the first 10");
        assert_eq!(hp_of(&h, far), 30, "one shot per cooldown, one target");
        // No second shot inside the 20-tick cooldown…
        h.step_exact(19);
        assert_eq!(hp_of(&h, near), 20);
        // …and the second lands within a couple of ticks of it (the exact
        // decrement phase is T3's `cooldown_gates_fire_rate` pin).
        h.step_exact(5);
        assert_eq!(hp_of(&h, near), 10);
        // Three hits kill a 30 HP Ghoul; by tick 50 the turret retargets.
        h.step_exact(25);
        assert!(!h.world().entities().contains(near), "3 hits by tick 50");
        h.step_exact(40);
        assert!(
            !h.world().entities().contains(far) || hp_of(&h, far) < 30,
            "after the kill the turret moves to the next ghoul"
        );
    }

    #[test]
    fn turret_target_ties_break_to_lowest_slot() {
        let mut h = RtsHarness::scene().build().expect("rts scene harness");
        let w0 = first_worker(&h);
        build_and_finish_turret(&mut h, TURRET_CORNER, w0);
        // Both exactly 24.5 cells from the footprint rectangle on the
        // centre row: x = 186 + 24.5 east, x = 180 − 24.5 west. Exact f32
        // equality — no rounding enters a subtraction of these literals.
        let east = spawn_ghoul(&mut h, [210.5, 179.0]); // spawned first → lower slot
        let west = spawn_ghoul(&mut h, [155.5, 179.0]);
        h.step_exact(1);
        assert_eq!(hp_of(&h, east), 20, "equal distance → lower slot");
        assert_eq!(hp_of(&h, west), 30);
    }

    #[test]
    fn turret_range_measured_from_footprint() {
        // Effective distance = distance from the footprint *rectangle* to
        // the target's hull. The rect's +x face is at x = 186: a ghoul on
        // the centre row at x = 224.9 reads 38.9 to the rect = 35.9
        // effective (in range); x = 225.1 reads 36.1 (out). Measured from
        // the *centre* the first case would read 224.9 − 183 − 3 = 38.9 > 36
        // and never fire — the "6-cell footprint must not lose 3 cells of
        // range" claim, pinned.
        assert_eq!(fire_probe([224.9, 179.0]), 20, "35.9 effective: in range");
        assert_eq!(fire_probe([225.1, 179.0]), 30, "36.1 effective: out");
    }

    // --- death and economy ---------------------------------------------------

    #[test]
    fn ghouls_kill_turret() {
        let mut h = RtsHarness::scene().build().expect("rts scene harness");
        let w0 = first_worker(&h);
        let turret = build_and_finish_turret(&mut h, TURRET_CORNER, w0);
        // Point the horde at the turret: with the HQ gone it is the only
        // remaining player building, so it becomes the march objective.
        let hq = h.world().start_hq().expect("hq");
        assert_eq!(h.world_mut().apply_damage(hq, 100_000), DamageResult::Killed);
        let losses_before = h.world().losses();
        // A column east of the turret, 7 cells apart (bodies never touch).
        for k in 0..12u32 {
            spawn_ghoul(&mut h, [201.0, 149.5 + 7.0 * k as f32]);
        }
        let mut dead = false;
        for _ in 0..3_000 {
            h.step_exact(1);
            if !h.world().entities().contains(turret) {
                dead = true;
                break;
            }
        }
        assert!(dead, "12 ghouls must grind 150 HP down within 3000 ticks");
        let w = h.world().static_nav().width();
        let (cx, cy) = TURRET_CENTER_CELL;
        let idx = (cx + cy * w) as usize;
        assert!(
            !h.world().static_nav().placement_solids()[idx],
            "turret death un-stamps its footprint"
        );
        assert!(!h.world().nav().blocked()[idx], "pool mask followed");
        assert!(
            h.world().losses() >= losses_before + 1,
            "the turret's death is counted as a loss"
        );
    }

    #[test]
    fn turret_grants_no_supply_not_dropoff() {
        let mut h = RtsHarness::scene().build().expect("rts scene harness");
        let w0 = first_worker(&h);
        let cap_before = h.world().supply().cap();
        build_and_finish_turret(&mut h, NODE_SIDE_CORNER, w0);
        assert_eq!(h.world().supply().cap(), cap_before, "no grant");
        // The turret hugs the node (4.5 cells to its rect; the HQ is ~21.7
        // away): a drop-off bug would send the hauler here.
        let node = h.ids_of_kind(EntityKind::Node(ResourceKind::Crystal))[0];
        assert!(h.world_mut().order_gather(w0, node));
        let mut returning = false;
        for _ in 0..4_000 {
            if let Some(Order::Gather {
                phase: GatherPhase::Returning { drop_off, .. },
                ..
            }) = h.world().order_of(w0)
            {
                let hq = h.world().start_hq().expect("hq");
                assert_eq!(drop_off, hq, "cargo returns to the HQ, never the turret");
                returning = true;
                break;
            }
            h.step_exact(1);
        }
        assert!(returning, "worker must turn for home within 4000 ticks");
    }

    // --- build card ----------------------------------------------------------

    #[test]
    fn turret_card_button_positional() {
        let mut h = RtsHarness::scene().build().expect("rts scene harness");
        let w0 = first_worker(&h);
        assert!(h.world_mut().select_only(w0));
        let slots = command_slots(h.world());
        assert_eq!(slots[3].command, Some(CommandId::BuildTurret));
        assert!(slots[3].enabled);
        assert_eq!(COMMAND_SLOT_KEYS[3], b'A', "positional key follows the slot");
        assert_eq!(BUILD_MENU.len(), 4);
        assert_eq!(BUILD_MENU[3], (b'A', BuildingKind::Turret));
    }
    ```
  - [ ] 1.2 `crates/mmd-engine/tests/rts_world.rs`, `kind_tags_are_distinct` (pre-combat 175; T2 made it 8 kinds): add `EntityKind::Building(BuildingKind::Turret),` after the Barracks line in the `kinds` array and bump the assertion to `assert_eq!(tags.len(), 9, "every kind must have a distinct tag byte");`.
  - [ ] 1.3 Same file, `footprints_come_from_the_scenario_constants` (pre-combat 153): after the Barracks assertion add
    ```rust
    assert_eq!(
        EntityKind::Building(BuildingKind::Turret).footprint_cells(),
        6
    );
    ```
  - [ ] 1.4 Same file, `only_the_hq_takes_a_drop_off` (pre-combat 168): add `assert!(!BuildingKind::Turret.is_drop_off());` after the Barracks line.
  - [ ] 1.5 `crates/mmd-engine/tests/rts_hud.rs`, `worker_card_uses_stable_three_build_slots` (551; keep the name — D5): after the `slots[2]` assertion add
    ```rust
    assert_eq!(slots[3].command, Some(CommandId::BuildTurret));
    ```
    change the enabled line to `assert!(slots[0].enabled && slots[1].enabled && slots[2].enabled && slots[3].enabled);` and the empty-slot guard from `if !(0..=2).contains(&i)` to `if !(0..=3).contains(&i)`.
  - [ ] 1.6 `crates/mmd-engine/tests/rts_pack.rs`, `prop_uv_maps_to_the_published_cells` (103): append `Prop::IconBuildTurret,` to the `all` array (the enumerate loop derives (4,0) itself) and, after the three trailing spot-checks, add `assert_eq!(prop_uv(Prop::IconBuildTurret), frame_uv_rect(4, 0));`. The stale comment `// …and the published cells really are the first two rows.` stays truthful for the three checks above it — leave it.
  - [ ] 1.7 Same file, `building_uv_switches_row_on_construction` (139): append
    ```rust
    assert_eq!(building_uv(BuildingKind::Turret, false), frame_uv_rect(0, 3));
    assert_eq!(building_uv(BuildingKind::Turret, true), frame_uv_rect(1, 3));
    ```
  - [ ] 1.8 Run `cargo test -p mmd-engine --test rts_turret` — expect a **compile failure** naming `Turret`, `TURRET_*`, `building_weapon`, `CommandId::BuildTurret`, `Prop::IconBuildTurret`. That is red; proceed.
- [ ] 2. Kind, constants and exhaustive-match arms
  - [ ] 2.1 `crates/mmd-engine/src/scenario.rs` — after `pub const BARRACKS_FOOTPRINT_CELLS: u32 = 10;` (102), add:
    ```rust
    /// Footprint edge of the Turret, in cells.
    pub const TURRET_FOOTPRINT_CELLS: u32 = 6;
    ```
  - [ ] 2.2 `crates/mmd-engine/src/rts/entity.rs`, `enum BuildingKind` (49-53) — append after `Barracks = 2,`:
    ```rust
    /// Phase-2 static defence. The first armed building; fires on its own,
    /// takes no orders.
    Turret = 3,
    ```
    (Discriminants appended, never inserted; `EntityKind::tag()` yields `0x23` with no further edit.)
  - [ ] 2.3 Same file, `impl BuildingKind::footprint_cells` (94-100) — append arm `Self::Turret => scenario::TURRET_FOOTPRINT_CELLS,` after the Barracks arm. (`is_drop_off` is a `matches!(self, Self::Hq)` — no edit.)
  - [ ] 2.4 Same file, T1's stat block — in the max-HP const run add `/// See [\`WORKER_MAX_HP\`].` + `pub const TURRET_MAX_HP: u32 = 150;` after `BARRACKS_MAX_HP`; in the armor run add `/// See [\`WORKER_ARMOR\`].` + `pub const TURRET_ARMOR: u32 = 1;` after `BARRACKS_ARMOR`; append `EntityKind::Building(BuildingKind::Turret) => TURRET_MAX_HP,` to `max_hp` and `EntityKind::Building(BuildingKind::Turret) => TURRET_ARMOR,` to `armor` (both matches are exhaustive — the compile error names them).
  - [ ] 2.5 `crates/mmd-engine/src/rts/build.rs` — after `BARRACKS_COST` (34-38) add:
    ```rust
    pub const TURRET_COST: Resources = Resources {
        crystal: 75,
        gas: 0,
    };
    ```
    and append `BuildingKind::Turret => TURRET_COST,` to `building_cost` (41-47).
  - [ ] 2.6 Same file — after `pub const BARRACKS_BUILD_TICKS: u32 = 300;` (50) add:
    ```rust
    /// Deliberately the Depot's duration: the turret is phase-2 placeholder
    /// pacing, not balance (see `DEPOT_BUILD_TICKS`).
    pub const TURRET_BUILD_TICKS: u32 = 180;
    ```
    and append `BuildingKind::Turret => TURRET_BUILD_TICKS,` to `build_ticks` (54-60).
  - [ ] 2.7 Same file — after `pub const BARRACKS_SUPPLY_GRANT: u32 = 0;` (64) add `pub const TURRET_SUPPLY_GRANT: u32 = 0;` and append `BuildingKind::Turret => TURRET_SUPPLY_GRANT,` to `supply_grant` (67-73). (`finish_site` calls `grant_cap(supply_grant(kind))` — a zero grant rides through; building death revokes zero. No world edit.)
  - [ ] 2.8 `crates/mmd-engine/src/rts/hud.rs`, `kind_label` (651-662) — after the Barracks arm add `EntityKind::Building(BuildingKind::Turret) => "TURRET",`.
  - [ ] 2.9 `src/rts_overlay.rs`, `ghost_name` (31-37) — append arm `BuildingKind::Turret => "turret",`.
  - [ ] 2.10 `crates/mmd-engine/src/rts/mod.rs` — in `pub use build::{…}` (17-22) insert `TURRET_BUILD_TICKS, TURRET_COST, TURRET_SUPPLY_GRANT,` after `HQ_SUPPLY_GRANT,`; in `pub use entity::{…}` (T1's extended list) insert `TURRET_ARMOR, TURRET_MAX_HP,` in const position (after `SOLDIER_MAX_HP,`). rustfmt settles wrapping.
- [ ] 3. Building weapon + combat-system inclusion (`crates/mmd-engine/src/rts/combat.rs`, T3's module — D9)
  - [ ] 3.1 Directly after T3's `pub fn weapon(kind: UnitKind) -> Option<Weapon>`, add:
    ```rust
    /// Weapon of a building kind. Only the Turret is armed.
    ///
    /// A `None` building never enters the combat scan as a firer, and an
    /// armed building fires only once **finished** (`progress_target == 0`)
    /// — a construction site has no working weapon. Buildings are static
    /// firers: no order, no chase; the scan alone decides.
    pub fn building_weapon(kind: BuildingKind) -> Option<Weapon> {
        match kind {
            BuildingKind::Turret => Some(Weapon {
                damage: 10,
                cooldown_ticks: 20,
                range_cells: 36.0,
            }),
            BuildingKind::Hq | BuildingKind::Depot | BuildingKind::Barracks => None,
        }
    }
    ```
    Add `BuildingKind` to the module's `use super::entity::{…}` import.
  - [ ] 3.2 `crates/mmd-engine/src/rts/mod.rs` — add `building_weapon,` to T3's `pub use combat::{…}` block (beside `weapon`).
  - [ ] 3.3 Extend the combat system's **firer eligibility**. Locate T3's per-slot firer pass (the ascending-slot loop that reads `weapon(kind)`, decrements the cooldown column, scans for the nearest enemy and calls `apply_damage`) — search `combat.rs`/`world.rs` for the call to `weapon(`. Where it derives the firer's weapon from `EntityKind::Unit(k)`, generalise to:
    ```rust
    let weapon = match kind {
        EntityKind::Unit(k) => weapon(k),
        // A finished building may be armed; a site never scans — its
        // weapon does not exist until construction completes.
        EntityKind::Building(b) if store.progress_target(slot) == 0 => building_weapon(b),
        _ => None,
    };
    ```
    (Adapt the receiver names to T3's code; if T3 collects armed *units* into a scratch list before the pass, widen that collection with the same predicate — same tick position, same single pass, no allocation.)
  - [ ] 3.4 In the same pass, branch the **firer-side geometry** (D2): a building firer's effective distance to an enemy **unit** target is
    ```rust
    rect_distance(store.position(t), store.position(slot), b.footprint_cells())
        - target_kind.body_radius_cells()
    ```
    with `rect_distance` imported from `super::orders` (`pub(crate)`, `orders.rs:261`). In-range iff `<= weapon.range_cells`; nearest by this distance; ties to the lowest target slot — the same comparator units use. Building firers consider enemy **units only** (the enemy faction cannot own buildings — T2's schema spawns Ghouls; a building-target arm would be dead code, documented at the branch). Unit-firer math stays byte-identical (do **not** route units through `rect_distance` — edge-1 rect math shifts unit numbers by up to 0.5 cells and breaks T3's pins).
  - [ ] 3.5 Confirm by reading (no edit expected): building firers need **no order gate** — T3's per-order firing rules (`Idle`/`AttackMove` fire, `Move`/`Gather`/`Build` never) apply to *unit* firers; a building slot's order row is never set and must not be consulted. Cooldown, `apply_damage`, kills/`first_combat_tick` bookkeeping: shared path, zero turret-specific code. If T3's structure forces an order read for every firer, gate it on `EntityKind::Unit(_)` and leave buildings unconditional.
- [ ] 4. Build card + command plumbing
  - [ ] 4.1 `crates/mmd-engine/src/rts/pack.rs`, `enum Prop` (57-74) — append after `IconSetRally = 15,`:
    ```rust
    IconBuildTurret = 16,
    ```
    (`prop_uv` derives cell (4,0); the 4×8 sheet has rows 4-7 free.)
  - [ ] 4.2 `crates/mmd-engine/src/rts/hud.rs`, `enum CommandId` (665-672) — append `BuildTurret,` after `BuildBarracks,`.
  - [ ] 4.3 Same file, `command_icon` (689-697) — append arm `CommandId::BuildTurret => Prop::IconBuildTurret,`.
  - [ ] 4.4 Same file, `command_slots` (711-762) — in the worker branch, after the `out[2]` assignment add:
    ```rust
    out[3] = CommandSlot {
        command: Some(CommandId::BuildTurret),
        enabled: true,
    };
    ```
    and update the fn doc's first bullet: `build commands at slots 0/1/2 (HQ, Depot, Barracks)` → `build commands at slots 0/1/2/3 (HQ, Depot, Barracks, Turret)`.
  - [ ] 4.5 Same file, `BUILD_MENU` (594-598) — grow to four entries:
    ```rust
    pub const BUILD_MENU: [(u8, BuildingKind); 4] = [
        (b'Q', BuildingKind::Hq),
        (b'W', BuildingKind::Depot),
        (b'E', BuildingKind::Barracks),
        (b'A', BuildingKind::Turret),
    ];
    ```
    (`A` because the key is positional: `KEY_BINDINGS` maps `A → ExecuteSlot(3)` already — `src/rts_input.rs:58`. **No input-table edit.**)
  - [ ] 4.6 `src/rts_ui.rs`, `execute_command` (450-477) — after the `CommandId::BuildBarracks` arm add:
    ```rust
    CommandId::BuildTurret => {
        let _ = world.begin_placement(BuildingKind::Turret);
    }
    ```
  - [ ] 4.7 Same file, tests module (near `hq_q_queues_worker`, 1750) — add:
    ```rust
    #[test]
    fn a_executes_turret_placement() {
        let mut world = test_world();
        let (mut session, _handle) = RtsSession::for_test();
        // Any seeded worker: the tracked scene starts with six.
        let worker = (0..world.entities().slot_count())
            .find_map(|slot| {
                (world.entities().alive(slot)
                    && world.entities().kind(slot)
                        == mmd_engine::rts::EntityKind::Unit(UnitKind::Worker))
                .then(|| world.entities().id_at(slot).expect("live slot"))
            })
            .expect("the tracked scene seeds workers");
        world.select_only(worker);
        let dispatched = execute_slot(&mut world, &mut session, 3); // A → slot 3
        assert!(dispatched, "worker slot 3 must dispatch BuildTurret");
        assert_eq!(
            world.placement(),
            mmd_engine::rts::Placement::Pending {
                kind: mmd_engine::rts::BuildingKind::Turret
            }
        );
    }
    ```
    (`UnitKind` is already imported at `rts_ui.rs:18`; `Placement`/`BuildingKind`/`EntityKind` are path-qualified like the module's other tests.)
- [ ] 5. Placeholder art (generated + hash-pinned — D4)
  - [ ] 5.1 `xtask/src/placeholder_art.rs` — after `const ICON_SET_RALLY_COLOR: …` (71) add:
    ```rust
    const ICON_BUILD_TURRET_COLOR: [u8; 4] = [120, 90, 160, 255];
    ```
    (Matches the turret building fill below, the same colour pairing every other build icon uses; distinct from all eight existing icon colours — `hud_icon_cells_are_opaque_and_distinct` proves it.)
  - [ ] 5.2 Same file, `buildings_table` (184-197) — add the two Turret cells after `(1, 2) => …` (fill/border pattern of the finished and hatched-site rows):
    ```rust
    (0, 3) => Some(([120, 90, 160, 255], [230, 235, 245, 255], false)),
    (1, 3) => Some(([60, 45, 80, 255], [120, 125, 130, 255], true)),
    ```
  - [ ] 5.3 Same file, `draw_props_cell` (248-321) — after the `(3, 3) => …` arm add:
    ```rust
    (4, 0) => draw_hud_icon(tile, ICON_BUILD_TURRET_COLOR),
    ```
  - [ ] 5.4 Same file, test `unused_table_rows_are_fully_transparent` (~908) — the props loop starts at row 4 and now hits the icon; skip exactly that cell. Inside `for row in 4..RTS_FRAMES_Y { for col in 0..RTS_FRAMES_X {`, first statement:
    ```rust
    if (row, col) == (4, 0) {
        continue; // IconBuildTurret
    }
    ```
    (The buildings loop starts at row 3 and is untouched — Turret sits in rows 0/1.)
  - [ ] 5.5 Same file, test `hud_icon_cells_are_opaque_and_distinct` (~934) — after the `for row in 2..=3` collection loop, add the ninth cell with the same shape:
    ```rust
    let cell = extract_cell(&px, w, 0, 4);
    assert!(
        cell.chunks_exact(4).any(|p| p[3] > 0),
        "row=4 col=0 must draw something"
    );
    cells.push(cell);
    ```
    (`extract_cell` takes `(px, w, col, row)`.)
  - [ ] 5.6 Regenerate the tracked sheets, then verify:
    ```sh
    cargo run -p xtask -- atlases
    cargo run -p xtask -- atlases --check
    git status --porcelain -- assets/sprites/generated
    ```
    Expect the second command clean and the status to show exactly `assets/sprites/generated/rts/buildings.png`, `assets/sprites/generated/rts/props.png` and `assets/sprites/generated/rts/manifest.json` modified (worker/soldier/ui bytes are untouched by these table edits — if more files moved, stop and inspect). Commit the three with the code.
- [ ] 6. Green + gate
  - [ ] 6.1 `cargo test -p mmd-engine --test rts_turret` — 11 tests green.
  - [ ] 6.2 `cargo test -p mmd-engine --test rts_world --test rts_hud --test rts_pack && cargo test -p xtask` — extended pins green.
  - [ ] 6.3 `cargo test --workspace --locked` — everything else unchanged-green (includes `a_executes_turret_placement`, both tracked scripts via `rts_acceptance`, and the frame-allocation guard — the new card slot allocates nothing).
  - [ ] 6.4 `cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets --all-features -- -D warnings` — clean.
  - [ ] 6.5 `MMD_WINDOW_HIDDEN=1 cargo run -- rts --frames 1600 --inject-input-file assets/scenarios/rts_acceptance_v1.script` — clean exit line, exit 0 (the script never presses `A` with a pure worker selection, so behaviour is byte-identical).
  - [ ] 6.6 Manual: `cargo run -- rts`, select a worker, press `A`, place — ghost previews at the cursor, site builds, finished turret stamps solid (units path around it). One-off sanity, not a gate.
  - [ ] 6.7 Commit as `feat(rts): turret as the first armed building` (sign-off required — the DCO gate walks every commit).

## Outputs

- Files: `crates/mmd-engine/src/rts/{entity.rs, build.rs, combat.rs, hud.rs, pack.rs, mod.rs}`, `crates/mmd-engine/src/scenario.rs`, `src/{rts_ui.rs, rts_overlay.rs}`, `xtask/src/placeholder_art.rs`; regenerated `assets/sprites/generated/rts/{buildings.png, props.png, manifest.json}`; tests: new `crates/mmd-engine/tests/rts_turret.rs`, edits in `crates/mmd-engine/tests/{rts_world.rs, rts_hud.rs, rts_pack.rs}` and `src/rts_ui.rs`. **No edit** to `src/rts_input.rs`, `world.rs`, `production.rs`, selection/minimap, any `assets/scenarios/*`, any doc.
- Public API T6/T7 consume verbatim: `BuildingKind::Turret`; `building_weapon(kind: BuildingKind) -> Option<Weapon>`; `TURRET_FOOTPRINT_CELLS: u32 = 6` (`mmd_engine::scenario`); plus `TURRET_MAX_HP/ARMOR/COST/BUILD_TICKS/SUPPLY_GRANT`, `CommandId::BuildTurret` at card slot 3 / key `A` (T7's script presses `A` with a worker selected).
- Known gaps to hand T8's close doc: turret fire has **no SFX asset and no firing animation** this phase (D6); enemy faction still renders from the soldier sheet (T2's gap, unchanged).

## Validation

- [ ] `cargo test -p mmd-engine --test rts_turret` → `test result: ok. 11 passed`
- [ ] `cargo test -p mmd-engine --test rts_world --test rts_hud --test rts_pack` → green; `cargo test -p xtask` → green
- [ ] `cargo test --workspace --locked` → all green, zero new skips
- [ ] `cargo fmt --all -- --check` → clean; `cargo clippy --workspace --all-targets --all-features -- -D warnings` → clean
- [ ] `cargo run -p xtask -- atlases --check` → clean (regenerated sheets committed)
- [ ] `MMD_WINDOW_HIDDEN=1 cargo run -- rts --frames 1600 --inject-input-file assets/scenarios/rts_acceptance_v1.script` → `rts: clean exit … frames=1600 …`, exit 0
- [ ] `git status --porcelain -- assets/scenarios` → empty (this ticket touches no scene)
- [ ] commit msg: `feat(rts): turret as the first armed building`
