# T1: Combat data model

**Plan:** `./artifacts/PLAN_2026_08_17_combat-prototype.md`
**Depends:** none
**Commit outcome:** Every RTS entity has HP + armor; damage application + death (unit despawn, building un-stamp + queue cancel + gather fallback) work engine-side; state hash covers new columns; full existing gate untouched.

## Context (self-contained)

- Goal: Phase 2 Combat Prototype — weapons, damage, turrets, enemy AI on existing RTS world (`crates/mmd-engine/src/rts/`). Success = scripted combat run through shipped binary + exit tokens.
- This slice: foundation — data model + death rules. No weapons, no enemies, no UI yet. Damage enters only via a `pub` engine API tests call.
- Out of scope here: enemy kind/owner (T2), targeting/cooldowns (T3), commands/UI (T4), turret (T5), bars (T6), scene/script changes (T7). Do NOT edit `assets/scenarios/*`, `src/rts_*.rs` behavior, or `sim/` (frozen).
- Assumptions in force: stats are placeholders; damage per hit `max(1, damage - armor)`; building death cancels its production queue with no refund; HQ death sends returning gatherers to `Idle`; no corpse state.
- **Conflict vs plan, codebase wins:** the plan asked for "HP + armor *columns*". Armor is a pure function of kind with no per-entity state, so it is a kind table (`armor(kind)`), not an `EntityStore` column — the cross-ticket contract only ever required the tables, and a redundant column would be dead state the hash would have to carry. HP is the only new column.
- No golden state-hash literal exists anywhere for `RtsWorld::state_hash` (all RTS hash tests compare run-vs-run; `tests/rts_cli_contract.rs:679` documents that a pasted literal is banned), so extending the hash composition breaks nothing.

## Decisions (made while detailing — do not reopen, do not re-derive)

- **D1 — armor is a kind table, not a column.** See conflict note above.
- **D2 — HP column type is `u32`**, matching the store's `progress`/`amount` column style (`crates/mmd-engine/src/rts/entity.rs:135-140`), hashed as 4 LE bytes.
- **D3 — nodes carry `hp == 0` as a sentinel** (house style: `progress == 0` = "nothing in progress", `CARRY_NONE` byte). `apply_damage` refuses nodes **by kind**, never by reading hp. `max_hp(Node(_)) == 0`.
- **D4 — a construction site spawns with the full finished-kind HP** (placeholder; HP scaling with build progress is balance, out of scope). Sites are killable.
- **D5 — site death refunds nothing.** Destruction is not `RtsWorld::cancel_construction` (which refunds, `world.rs:1445`). Resources stay spent.
- **D6 — finished-building death revokes its supply grant**: `supply.revoke_cap(supply_grant(kind))`. `Supply::revoke_cap` already exists and its `free()` doc names exactly this case ("a Depot destroyed later", `crates/mmd-engine/src/rts/economy.rs:80-84`). A site death revokes nothing (a site never granted).
- **D7 — HQ death clears `RtsWorld::start_hq` to `None`** when the dead id is the start HQ — the accessor's doc already promises "`None` only after it is destroyed" (`world.rs:958-962`).
- **D8 — selection is NOT pruned inside `apply_damage`.** The tick's existing last step (`self.selection.retain_live(&self.entities)`, `world.rs:1739`) handles it, same contract as every other death-adjacent cleanup. Tests tick once after a kill before asserting the selection.
- **D9 — the gather-collision pair table needs no death hook.** `GatherCollisionState::sync_slot` (`crates/mmd-engine/src/rts/collision.rs:148`) already clears a recycled slot's pair row by generation compare at the head of every movement pass, and its `hash_into` frames only live unit slots with `(index, generation)`. Only a stale doc sentence is updated (step 6).
- **D10 — eager order cleanup on building death** covers exactly two order shapes: `Order::Build { site }` naming the dead building, and `Order::Gather { phase: Returning { drop_off, .. } }` naming it. `ToNode`/`Mining` gatherers are untouched — they hold no drop-off yet, and the existing gather system already handles "no drop-off exists" when their load fills (`nearest_drop_off` → `None` → order cleared, `world.rs:2192-2199`).
- **D11 — `DamageResult` variants**: `Damaged { remaining_hp: u32 }`, `Killed`, `Indestructible` (node), `NoTarget` (stale/dead id, no-op).
- **D12 — u32 formula**: `damage.saturating_sub(armor(kind)).max(1)` — exactly `max(1, damage - armor)` without underflow. Death when `dealt >= hp`, resolved in the same `apply_damage` call.
- **In-tick caller hazard (for T3, documented on the API now):** `RtsWorld::tick` collects `live_scratch` once at its top; store accessors assert liveness. A future in-tick damage caller must run before systems that consume `live_scratch`, or re-collect. Recorded in `apply_damage`'s doc comment (step 5.3); nothing in T1 calls it inside a tick.

## Requirements

- `hp: Vec<u32>` column in `rts::EntityStore` (SoA, preallocated `MAX_ENTITIES = 2048`, never grown; the capacity test hook `column_capacities` grows `[usize; 13]` → `[usize; 14]`).
- Kind stat tables in `entity.rs`, exported from `rts/mod.rs`: `max_hp(EntityKind) -> u32` = Worker 25, Soldier 40, Hq 400, Depot 150, Barracks 200, Node 0; `armor(EntityKind) -> u32` = Worker 0, Soldier 0, Hq 2, Depot 1, Barracks 1, Node 0. Backing named consts (`WORKER_MAX_HP`, … `BARRACKS_ARMOR`) in house style (cf. `build.rs` costs).
- `pub fn apply_damage(&mut self, target: EntityId, damage: u32) -> DamageResult` on `RtsWorld`: subtract `damage.saturating_sub(armor).max(1)`; at 0 HP → death in the same call. Node target → `DamageResult::Indestructible` no-op; stale id → `DamageResult::NoTarget` no-op.
- Unit death: `orders.clear(slot)` (slot hygiene — an order table row is never auto-cleared at despawn), then despawn (generational free). Selection prune stays the tick's last step (D8).
- Finished-building death: un-stamp footprint from `StaticNav` via new exact-inverse `unstamp_finished_building`, `rebuild_center_blocked(RTS_UNIT_BODY_RADIUS_CELLS)`, `nav.replace_blocked_mask(static_nav.center_blocked())` (invalidates EVERY cached `FieldPool` field — same all-or-nothing rule stamping obeys, `world.rs:1874-1882` / `field_pool.rs:158-172`), `supply.revoke_cap(supply_grant(kind))`, `production.clear(slot)` (queue dies, NO refund), eager order cleanup per D10, `start_hq = None` if it was the start HQ, despawn.
- Site death (progress_target != 0): no un-stamp (never stamped), no cap revoke, no refund; `production.clear(slot)`, builders' `Order::Build` → `Idle` (D10 covers it), despawn.
- Supply `used` needs no death code at all: `supply_recount` recomputes it from live units + live queues every tick (`world.rs:2068-2079`); a dead building's reservations vanish from `reserved_supply()` immediately because it iterates live slots only (`world.rs:1702-1716`).
- State hash: hp appended to the per-live-slot block of `EntityStore::hash_into` (`entity.rs:431-448`) — fixed width (4 LE bytes), ascending slot order, same discipline as `OrderTable::hash_into` (`orders.rs:180`).
- Zero behavior change on existing runs: no caller applies damage yet; all existing tests + both tracked scripts pass with only one test edit (the column-count pin, `tests/rts_economy.rs:116`).

## Inputs (inspected, line refs from current `main`-based branch)

- `crates/mmd-engine/src/rts/entity.rs` — struct `EntityStore` columns 127-145; `spawn_impl` 214-259 (growth pushes then reset writes); accessors `amount` 343 / `set_amount` 384; `hash_into` 431-448; `column_capacities` 452-470 (`#[cfg(feature = "testkit")]`, returns `[usize; 13]`); `CARRY_NONE` 112; `impl BuildingKind::is_drop_off` ends ~106.
- `crates/mmd-engine/src/rts/world.rs` — entity import block 22-25; `TickError` 169-195; struct `RtsWorld` 197+; `entities_mut`/`force_position_for_test` testkit hooks 850-880; `start_hq` accessor 958-962; `cancel_construction` 1445-1471 (the despawn-hygiene pattern: despawn + `production.clear` + scan-and-clear `Order::Build`); `reserved_supply` 1702; `tick` 1730-1740 (order: commands, camera, construction, production, gather, movement, supply recount, selection prune last); `finish_site` commit block 1962-1969 (`set_progress(0,0)`, `stamp_finished_building`, `rebuild_center_blocked`, `grant_cap`) and the batch `replace_blocked_mask` + `debug_assert` 1874-1882; gather `Returning` dead-drop-off self-heal 2164-2170; `supply_recount` 2068; `state_hash` 3292-3318 with composition doc 3279-3291. `supply_grant`, `footprint_min`, `GatherPhase` already imported.
- `crates/mmd-engine/src/rts/static_nav.rs` — `solids`/`placement_solids`/`center_blocked` fields 33-55; `stamp_finished_building` 230-243; `rebuild_center_blocked` 269 (reuses preallocated `component_stack`, allocation-free after load).
- `crates/mmd-engine/src/rts/economy.rs` — `Supply::grant_cap` 106 / `revoke_cap` 110; `free()` saturation doc 80-84.
- `crates/mmd-engine/src/rts/build.rs` — `supply_grant` 66-73 (HQ 10, Depot 10, Barracks 0); `building_cost`; placement rules guarantee a footprint never covers terrain, another building, or a node → un-stamp-to-`false` is an exact inverse.
- `crates/mmd-engine/src/rts/collision.rs` — `sync_slot` 148-157 + its doc 130-147 (one stale sentence to update).
- `crates/mmd-engine/src/rts/mod.rs` — export lists: entity 31-34, world 91-95.
- `crates/mmd-engine/src/nav/field_pool.rs` — `replace_blocked_mask` 162-172 (copies in place, drops every key → all cached fields stale; movers re-acquire, documented at `world.rs` movement doc "re-checks that the field it cached is still the field it asked for").
- `crates/mmd-engine/src/testkit/rts.rs` — `RtsHarness`: `scene()` (tracked scene, hash-verified), `step_exact`, `world()`/`world_mut()`, `state_hash()`/`state_hash_hex()`, `ids_of_kind`. Seed 0 default.
- `crates/mmd-engine/tests/rts_production.rs:19-48` — `first_worker`, `DEPOT_CORNER {180,176}`, `BARRACKS_CORNER {198,176}`, `build_and_finish` helper this ticket's test file copies.
- `crates/mmd-engine/tests/rts_economy.rs:108-121` — `carry_columns_are_reserved` pins `caps.len() == 13` at line 116 → becomes 14.
- **From Depends:** none.

## TDD

1. **Red** — step 1 lands the whole test file + the column-count pin first. Red here = the workspace fails to compile the new test crate (missing `hp`, `max_hp`, `armor`, `apply_damage`, `DamageResult`) and `carry_columns_are_reserved` fails on 13≠14 once the column lands. That is the intended red; do not weaken the tests to dodge it.
2. **Green** — steps 2-6, minimal code, no extra surface.
3. **Refactor** — keep green; `cargo fmt` + clippy are part of the gate, not optional polish.

## Test plan

All in the new file `crates/mmd-engine/tests/rts_combat.rs` (exact content in step 1.1). Run: `cargo test -p mmd-engine --test rts_combat`.

| Test (exact name) | Input | Expect |
| ---- | ----- | ------ |
| `stats_are_the_published_constants` | tables | 25/40/400/150/200 HP, 0/0/2/1/1 armor, node 0/0 |
| `every_entity_spawns_at_full_hp` | tracked scene, all live slots | `hp(slot) == max_hp(kind)` |
| `damage_reduces_hp_by_damage_minus_armor` | 10 dmg to scene HQ (armor 2) | `Damaged { remaining_hp: 392 }`, `hp()` 392 |
| `damage_floors_at_one` | 1 dmg then 2 dmg to HQ | 399 then 398 (both floored to 1) |
| `damage_to_a_node_is_a_no_op` | overkill to crystal node | `Indestructible`; node live, `amount` + `state_hash` unchanged |
| `damage_to_a_stale_id_is_refused` | hit an already-killed worker | `NoTarget` |
| `a_unit_dies_at_zero_hp_and_its_slot_frees` | exactly 25 dmg to selected worker | `Killed`; `contains` false, `len` −1, `order_of` `None`; selection empty after 1 tick |
| `building_death_unstamps_its_footprint` | kill finished Depot | `Killed`; all 8×8 `placement_solids` cells false; pool `blocked()` centre cell false (mask replaced ⇒ every cached field invalidated) |
| `building_death_cancels_its_queue_without_refund` | kill Barracks with queued Soldier | resources exactly post-enqueue value; `production_queue` `None`; `reserved_supply` 0; `supply().used()` back to pre-enqueue after 1 tick |
| `building_death_revokes_its_supply_grant` | kill finished Depot | `supply().cap()` drops by `DEPOT_SUPPLY_GRANT` (10) |
| `hq_death_idles_its_returning_gatherers` | kill HQ while a worker is `Returning` to it | worker order `Idle` same call; `start_hq()` `None` |
| `site_death_idles_its_builders_without_refund` | kill a fresh Depot site | site gone; builder `Idle` same call; resources unchanged (no refund) |
| `hp_enters_the_state_hash` | twin scene worlds, damage one HQ | hashes equal before, differ after |

Undamaged-world stability is not a new test: it is the entire existing suite plus the two tracked script runs passing unmodified (Validation), including `rts_world.rs::state_hash_is_reproducible_across_worlds` and `rts_acceptance.rs`.

## Impl steps

- [ ] 1. Red — pin the new surface with failing tests
  - [ ] 1.1 Create `crates/mmd-engine/tests/rts_combat.rs` with exactly this content:

    ```rust
    //! Combat T1 — HP, armor and death: the combat data core.
    //!
    //! Pure logic, no GPU, no clock: every case is a headless CPU check of
    //! `mmd_engine::rts` through `testkit::RtsHarness`. Damage enters only via
    //! `RtsWorld::apply_damage` — nothing in these worlds can attack yet, so an
    //! undamaged world is untouched by this slice.

    use mmd_engine::rts::{
        BARRACKS_ARMOR, BARRACKS_MAX_HP, BuildingKind, DEPOT_ARMOR, DEPOT_MAX_HP,
        DEPOT_SUPPLY_GRANT, DamageResult, EntityId, EntityKind, GatherPhase, HQ_ARMOR, HQ_MAX_HP,
        Order, ResourceKind, SOLDIER_ARMOR, SOLDIER_MAX_HP, UnitKind, WORKER_ARMOR, WORKER_MAX_HP,
        armor, max_hp,
    };
    use mmd_engine::scenario::Cell;
    use mmd_engine::testkit::RtsHarness;

    fn first_worker(h: &RtsHarness) -> EntityId {
        h.ids_of_kind(EntityKind::Unit(UnitKind::Worker))[0]
    }

    fn crystal_node(h: &RtsHarness) -> EntityId {
        h.ids_of_kind(EntityKind::Node(ResourceKind::Crystal))[0]
    }

    /// Clear, obstacle-free, node-free, HQ-free corners of the tracked scene —
    /// the same corners `rts_production.rs` builds at.
    const DEPOT_CORNER: Cell = Cell { x: 180, y: 176 };
    const BARRACKS_CORNER: Cell = Cell { x: 198, y: 176 };

    /// A damage amount no current kind survives through its armor.
    const OVERKILL: u32 = 100_000;

    /// Place, confirm and fully attend a building until it finishes. Generous
    /// on ticks (2 000, well past any build time): the builder walks in first.
    fn build_and_finish(
        h: &mut RtsHarness,
        kind: BuildingKind,
        corner: Cell,
        builder: EntityId,
    ) -> EntityId {
        assert!(h.world_mut().begin_placement(kind));
        let site = h
            .world_mut()
            .confirm_placement(corner, builder)
            .expect("confirm placement");
        h.step_exact(2_000);
        assert!(
            !h.world().is_site(site),
            "building must have finished within 2000 ticks"
        );
        site
    }

    // --- stats and spawn state -----------------------------------------------

    #[test]
    fn stats_are_the_published_constants() {
        assert_eq!(max_hp(EntityKind::Unit(UnitKind::Worker)), WORKER_MAX_HP);
        assert_eq!(WORKER_MAX_HP, 25);
        assert_eq!(max_hp(EntityKind::Unit(UnitKind::Soldier)), SOLDIER_MAX_HP);
        assert_eq!(SOLDIER_MAX_HP, 40);
        assert_eq!(max_hp(EntityKind::Building(BuildingKind::Hq)), HQ_MAX_HP);
        assert_eq!(HQ_MAX_HP, 400);
        assert_eq!(
            max_hp(EntityKind::Building(BuildingKind::Depot)),
            DEPOT_MAX_HP
        );
        assert_eq!(DEPOT_MAX_HP, 150);
        assert_eq!(
            max_hp(EntityKind::Building(BuildingKind::Barracks)),
            BARRACKS_MAX_HP
        );
        assert_eq!(BARRACKS_MAX_HP, 200);
        assert_eq!(max_hp(EntityKind::Node(ResourceKind::Crystal)), 0);
        assert_eq!(max_hp(EntityKind::Node(ResourceKind::Gas)), 0);

        assert_eq!(armor(EntityKind::Unit(UnitKind::Worker)), WORKER_ARMOR);
        assert_eq!(WORKER_ARMOR, 0);
        assert_eq!(armor(EntityKind::Unit(UnitKind::Soldier)), SOLDIER_ARMOR);
        assert_eq!(SOLDIER_ARMOR, 0);
        assert_eq!(armor(EntityKind::Building(BuildingKind::Hq)), HQ_ARMOR);
        assert_eq!(HQ_ARMOR, 2);
        assert_eq!(armor(EntityKind::Building(BuildingKind::Depot)), DEPOT_ARMOR);
        assert_eq!(DEPOT_ARMOR, 1);
        assert_eq!(
            armor(EntityKind::Building(BuildingKind::Barracks)),
            BARRACKS_ARMOR
        );
        assert_eq!(BARRACKS_ARMOR, 1);
        assert_eq!(armor(EntityKind::Node(ResourceKind::Crystal)), 0);
    }

    #[test]
    fn every_entity_spawns_at_full_hp() {
        let h = RtsHarness::scene().build().expect("rts scene harness");
        let mut slots = Vec::new();
        h.world().entities().collect_live(&mut slots);
        assert!(!slots.is_empty());
        for slot in slots {
            let kind = h.world().entities().kind(slot);
            assert_eq!(h.world().entities().hp(slot), max_hp(kind), "slot {slot}");
        }
    }

    // --- apply_damage arithmetic ---------------------------------------------

    #[test]
    fn damage_reduces_hp_by_damage_minus_armor() {
        let mut h = RtsHarness::scene().build().expect("rts scene harness");
        let hq = h.world().start_hq().expect("hq");
        let slot = h.world().entities().slot(hq).expect("live hq");
        assert_eq!(h.world().entities().hp(slot), HQ_MAX_HP);
        assert_eq!(
            h.world_mut().apply_damage(hq, 10),
            DamageResult::Damaged {
                remaining_hp: HQ_MAX_HP - 8
            },
            "10 damage through 2 armor must deal 8"
        );
        assert_eq!(h.world().entities().hp(slot), HQ_MAX_HP - 8);
    }

    #[test]
    fn damage_floors_at_one() {
        let mut h = RtsHarness::scene().build().expect("rts scene harness");
        let hq = h.world().start_hq().expect("hq");
        let slot = h.world().entities().slot(hq).expect("live hq");
        // 1 damage through 2 armor: saturates to 0, floors to 1.
        assert_eq!(
            h.world_mut().apply_damage(hq, 1),
            DamageResult::Damaged {
                remaining_hp: HQ_MAX_HP - 1
            }
        );
        // 2 damage through 2 armor: exactly 0, floors to 1.
        assert_eq!(
            h.world_mut().apply_damage(hq, 2),
            DamageResult::Damaged {
                remaining_hp: HQ_MAX_HP - 2
            }
        );
        assert_eq!(h.world().entities().hp(slot), HQ_MAX_HP - 2);
    }

    #[test]
    fn damage_to_a_node_is_a_no_op() {
        let mut h = RtsHarness::scene().build().expect("rts scene harness");
        let node = crystal_node(&h);
        let slot = h.world().entities().slot(node).expect("live node");
        let amount_before = h.world().entities().amount(slot);
        let hash_before = h.state_hash();
        assert_eq!(
            h.world_mut().apply_damage(node, OVERKILL),
            DamageResult::Indestructible
        );
        assert!(h.world().entities().contains(node));
        assert_eq!(h.world().entities().amount(slot), amount_before);
        assert_eq!(h.state_hash(), hash_before, "a refused hit must change nothing");
    }

    #[test]
    fn damage_to_a_stale_id_is_refused() {
        let mut h = RtsHarness::scene().build().expect("rts scene harness");
        let w0 = first_worker(&h);
        assert_eq!(h.world_mut().apply_damage(w0, OVERKILL), DamageResult::Killed);
        assert_eq!(h.world_mut().apply_damage(w0, 5), DamageResult::NoTarget);
    }

    // --- unit death ----------------------------------------------------------

    #[test]
    fn a_unit_dies_at_zero_hp_and_its_slot_frees() {
        let mut h = RtsHarness::scene().build().expect("rts scene harness");
        let w0 = first_worker(&h);
        let n_before = h.world().entities().len();
        assert!(h.world_mut().select_only(w0));
        // 25 damage through 0 armor: exactly lethal for a full-health Worker.
        assert_eq!(
            h.world_mut().apply_damage(w0, WORKER_MAX_HP),
            DamageResult::Killed
        );
        assert!(!h.world().entities().contains(w0), "stale id must not resolve");
        assert_eq!(h.world().entities().len(), n_before - 1);
        assert_eq!(h.world().order_of(w0), None);
        // Selection pruning stays where it has always been: the tick's last step.
        h.step_exact(1);
        assert!(h.world().selection().ids().is_empty());
    }

    // --- building death ------------------------------------------------------

    #[test]
    fn building_death_unstamps_its_footprint() {
        let mut h = RtsHarness::scene().build().expect("rts scene harness");
        let w0 = first_worker(&h);
        let depot = build_and_finish(&mut h, BuildingKind::Depot, DEPOT_CORNER, w0);
        let edge = BuildingKind::Depot.footprint_cells();
        let w = h.world().static_nav().width();
        let centre_idx =
            (DEPOT_CORNER.x + edge / 2 + (DEPOT_CORNER.y + edge / 2) * w) as usize;
        assert!(h.world().static_nav().placement_solids()[centre_idx]);
        assert!(h.world().nav().blocked()[centre_idx]);

        assert_eq!(h.world_mut().apply_damage(depot, OVERKILL), DamageResult::Killed);
        assert!(!h.world().entities().contains(depot));
        for dy in 0..edge {
            for dx in 0..edge {
                let idx = (DEPOT_CORNER.x + dx + (DEPOT_CORNER.y + dy) * w) as usize;
                assert!(
                    !h.world().static_nav().placement_solids()[idx],
                    "footprint cell (+{dx},+{dy}) must be clear again"
                );
            }
        }
        // The pool's mask followed the static one — the whole-mask replacement
        // is what invalidates every cached field, same rule as stamping.
        assert!(!h.world().nav().blocked()[centre_idx]);
    }

    #[test]
    fn building_death_cancels_its_queue_without_refund() {
        let mut h = RtsHarness::scene().build().expect("rts scene harness");
        let w0 = first_worker(&h);
        let barracks = build_and_finish(&mut h, BuildingKind::Barracks, BARRACKS_CORNER, w0);
        let used_before = h.world().supply().used();
        assert!(h.world_mut().enqueue_unit(barracks, UnitKind::Soldier).is_ok());
        let resources_after_enqueue = h.world().resources();
        assert_eq!(h.world().reserved_supply(), 2, "one queued Soldier reserves 2");

        assert_eq!(
            h.world_mut().apply_damage(barracks, OVERKILL),
            DamageResult::Killed
        );
        // No refund: the stock is exactly what it was after paying.
        assert_eq!(h.world().resources(), resources_after_enqueue);
        assert!(h.world().production_queue(barracks).is_none());
        assert_eq!(h.world().reserved_supply(), 0);
        // used self-heals on the next tick's recount.
        h.step_exact(1);
        assert_eq!(h.world().supply().used(), used_before);
    }

    #[test]
    fn building_death_revokes_its_supply_grant() {
        let mut h = RtsHarness::scene().build().expect("rts scene harness");
        let w0 = first_worker(&h);
        let depot = build_and_finish(&mut h, BuildingKind::Depot, DEPOT_CORNER, w0);
        let cap_with_depot = h.world().supply().cap();
        assert_eq!(h.world_mut().apply_damage(depot, OVERKILL), DamageResult::Killed);
        assert_eq!(
            h.world().supply().cap(),
            cap_with_depot - DEPOT_SUPPLY_GRANT,
            "a dead Depot's grant must be revoked"
        );
    }

    #[test]
    fn hq_death_idles_its_returning_gatherers() {
        let mut h = RtsHarness::scene().build().expect("rts scene harness");
        let w0 = first_worker(&h);
        let node = crystal_node(&h);
        assert!(h.world_mut().order_gather(w0, node));
        // Walk in, mine a load, turn for home — bounded, deterministic.
        let mut returning = false;
        for _ in 0..4_000 {
            if matches!(
                h.world().order_of(w0),
                Some(Order::Gather {
                    phase: GatherPhase::Returning { .. },
                    ..
                })
            ) {
                returning = true;
                break;
            }
            h.step_exact(1);
        }
        assert!(returning, "worker must turn for home within 4000 ticks");
        let hq = h.world().start_hq().expect("hq");
        assert!(matches!(
            h.world().order_of(w0),
            Some(Order::Gather {
                phase: GatherPhase::Returning { drop_off, .. },
                ..
            }) if drop_off == hq
        ));

        assert_eq!(h.world_mut().apply_damage(hq, OVERKILL), DamageResult::Killed);
        assert_eq!(
            h.world().order_of(w0),
            Some(Order::Idle),
            "a hauler bound for a dead drop-off must go idle in the same call"
        );
        assert_eq!(h.world().start_hq(), None);
    }

    #[test]
    fn site_death_idles_its_builders_without_refund() {
        let mut h = RtsHarness::scene().build().expect("rts scene harness");
        let w0 = first_worker(&h);
        assert!(h.world_mut().begin_placement(BuildingKind::Depot));
        let site = h
            .world_mut()
            .confirm_placement(DEPOT_CORNER, w0)
            .expect("confirm placement");
        assert!(matches!(
            h.world().order_of(w0),
            Some(Order::Build { site: s, .. }) if s == site
        ));
        let resources_after_confirm = h.world().resources();

        assert_eq!(h.world_mut().apply_damage(site, OVERKILL), DamageResult::Killed);
        assert!(!h.world().entities().contains(site));
        assert_eq!(h.world().order_of(w0), Some(Order::Idle));
        // Destruction is not a cancel: the cost stays spent.
        assert_eq!(h.world().resources(), resources_after_confirm);
    }

    // --- state hash ----------------------------------------------------------

    #[test]
    fn hp_enters_the_state_hash() {
        let mut a = RtsHarness::scene().build().expect("rts scene harness");
        let b = RtsHarness::scene().build().expect("rts scene harness");
        assert_eq!(a.state_hash(), b.state_hash());
        let hq = a.world().start_hq().expect("hq");
        a.world_mut().apply_damage(hq, 10);
        assert_ne!(
            a.state_hash(),
            b.state_hash(),
            "a damaged HQ must change the digest"
        );
    }
    ```
  - [ ] 1.2 In `crates/mmd-engine/tests/rts_economy.rs` line 116, change `assert_eq!(caps.len(), 13);` → `assert_eq!(caps.len(), 14);`.
  - [ ] 1.3 Run `cargo test -p mmd-engine --test rts_combat` — expect a **compile failure** naming the missing symbols (`hp`, `max_hp`, `armor`, `apply_damage`, `DamageResult`). That is red; proceed.
- [ ] 2. HP column + kind stat tables in `crates/mmd-engine/src/rts/entity.rs`
  - [ ] 2.1 After the `impl BuildingKind { … is_drop_off … }` block (ends ~line 106), before the `CARRY_NONE` doc comment, insert the stat tables:

    ```rust
    /// Full hit points per kind — phase-2 placeholder stats, not balance.
    pub const WORKER_MAX_HP: u32 = 25;
    /// See [`WORKER_MAX_HP`].
    pub const SOLDIER_MAX_HP: u32 = 40;
    /// See [`WORKER_MAX_HP`].
    pub const HQ_MAX_HP: u32 = 400;
    /// See [`WORKER_MAX_HP`].
    pub const DEPOT_MAX_HP: u32 = 150;
    /// See [`WORKER_MAX_HP`].
    pub const BARRACKS_MAX_HP: u32 = 200;

    /// Flat damage reduction per kind — a hit deals `max(1, damage - armor)`.
    pub const WORKER_ARMOR: u32 = 0;
    /// See [`WORKER_ARMOR`].
    pub const SOLDIER_ARMOR: u32 = 0;
    /// See [`WORKER_ARMOR`].
    pub const HQ_ARMOR: u32 = 2;
    /// See [`WORKER_ARMOR`].
    pub const DEPOT_ARMOR: u32 = 1;
    /// See [`WORKER_ARMOR`].
    pub const BARRACKS_ARMOR: u32 = 1;

    /// Hit points a full-health entity of `kind` spawns with.
    ///
    /// `0` for a resource node: nodes are indestructible and carry no HP
    /// semantics at all — damage refuses them by kind, never by reading this.
    /// The exhaustive match forces a future kind to decide its own value
    /// instead of silently inheriting one.
    pub fn max_hp(kind: EntityKind) -> u32 {
        match kind {
            EntityKind::Unit(UnitKind::Worker) => WORKER_MAX_HP,
            EntityKind::Unit(UnitKind::Soldier) => SOLDIER_MAX_HP,
            EntityKind::Building(BuildingKind::Hq) => HQ_MAX_HP,
            EntityKind::Building(BuildingKind::Depot) => DEPOT_MAX_HP,
            EntityKind::Building(BuildingKind::Barracks) => BARRACKS_MAX_HP,
            EntityKind::Node(_) => 0,
        }
    }

    /// Flat damage reduction of `kind`: one hit deals `max(1, damage - armor)`.
    pub fn armor(kind: EntityKind) -> u32 {
        match kind {
            EntityKind::Unit(UnitKind::Worker) => WORKER_ARMOR,
            EntityKind::Unit(UnitKind::Soldier) => SOLDIER_ARMOR,
            EntityKind::Building(BuildingKind::Hq) => HQ_ARMOR,
            EntityKind::Building(BuildingKind::Depot) => DEPOT_ARMOR,
            EntityKind::Building(BuildingKind::Barracks) => BARRACKS_ARMOR,
            EntityKind::Node(_) => 0,
        }
    }
    ```
  - [ ] 2.2 In `struct EntityStore` (line ~141), directly after `carry_amount: Vec<u32>,`, add:

    ```rust
    /// Remaining hit points. `0` for a resource node — indestructible, no HP
    /// semantics (see [`max_hp`]).
    hp: Vec<u32>,
    ```
  - [ ] 2.3 In `EntityStore::new()`, after `carry_amount: Vec::with_capacity(MAX_ENTITIES),`, add `hp: Vec::with_capacity(MAX_ENTITIES),`.
  - [ ] 2.4 In `spawn_impl` (line ~214), growth branch: after `self.carry_amount.push(0);` add `self.hp.push(0);`.
  - [ ] 2.5 In `spawn_impl`, reset section: after `self.carry_amount[idx] = 0;` (line ~248) add `self.hp[idx] = max_hp(kind);`.
  - [ ] 2.6 After the `amount` accessor (line ~343-346), add the getter:

    ```rust
    /// Remaining hit points; `0` for a resource node, which has no HP
    /// semantics.
    pub fn hp(&self, slot: usize) -> u32 {
        self.assert_live(slot);
        self.hp[slot]
    }
    ```
  - [ ] 2.7 After `set_amount` (line ~384-387), add the setter:

    ```rust
    pub fn set_hp(&mut self, slot: usize, hp: u32) {
        self.assert_live(slot);
        self.hp[slot] = hp;
    }
    ```
  - [ ] 2.8 In `EntityStore::hash_into` (line ~431), after `h.update(self.carry_amount[i].to_le_bytes());`, add `h.update(self.hp[i].to_le_bytes());`.
  - [ ] 2.9 In `column_capacities` (line ~454): change return type `[usize; 13]` → `[usize; 14]` and append `self.hp.capacity(),` after `self.carry_amount.capacity(),`.
- [ ] 3. Exact-inverse un-stamp in `crates/mmd-engine/src/rts/static_nav.rs`
  - [ ] 3.1 Directly after `stamp_finished_building` (ends ~line 243), add:

    ```rust
    /// Clear a destroyed building's footprint from the solid masks — the
    /// exact inverse of [`Self::stamp_finished_building`]. Placement validity
    /// keeps a footprint clear of terrain, resource nodes and other
    /// buildings, so clearing these cells cannot erase anyone else's solid.
    /// Does not recompute [`Self::center_blocked`] — call
    /// [`Self::rebuild_center_blocked`] afterward, same contract as stamping.
    pub fn unstamp_finished_building(&mut self, min: Cell, edge: u32) {
        for dy in 0..edge {
            for dx in 0..edge {
                let x = min.x + dx;
                let y = min.y + dy;
                if x < self.width && y < self.height {
                    let idx = (x + y * self.width) as usize;
                    self.solids[idx] = false;
                    self.placement_solids[idx] = false;
                }
            }
        }
    }
    ```
- [ ] 4. Damage + death in `crates/mmd-engine/src/rts/world.rs`
  - [ ] 4.1 In the `use super::entity::{ … }` block (lines 22-25), add `armor` to the list (alphabetically first: `armor, BuildingKind, EntityId, …` — rustfmt will settle the order).
  - [ ] 4.2 Directly after the `TickError` enum (ends ~line 195 with `UnrepairableOverlap,` `}`), before `/// The phase-1 RTS game state.`, insert:

    ```rust
    /// What [`RtsWorld::apply_damage`] did to its target.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum DamageResult {
        /// The hit landed and the target survives with this much HP left.
        Damaged { remaining_hp: u32 },
        /// The hit reduced the target to 0 HP; it died and was despawned
        /// inside this call.
        Killed,
        /// The target is a resource node. Nodes are indestructible; nothing
        /// changed.
        Indestructible,
        /// The id names no live entity; nothing changed.
        NoTarget,
    }
    ```
  - [ ] 4.3 Directly before the `/// Advance one fixed 1/60 s step.` doc of `pub fn tick` (line ~1718), insert the damage seam and the death routine:

    ```rust
    /// Deal one hit to `target`: subtract `max(1, damage - armor(kind))`
    /// from its HP, resolving death inside this same call at 0.
    ///
    /// The one damage seam of the combat slice — turrets, unit attacks and
    /// scripted tests all enter here, so the death rules cannot diverge per
    /// caller:
    ///
    /// - a **unit** despawns; its order row is cleared for slot reuse. The
    ///   selection is pruned by the tick's existing last step, not here.
    /// - a **finished building** is un-stamped from [`StaticNav`], the
    ///   pooled blocked mask is replaced (invalidating every cached field —
    ///   the same all-or-nothing rule stamping obeys), its supply grant is
    ///   revoked, its production queue dies with **no refund**, and workers
    ///   hauling cargo back to it go [`Order::Idle`] in this call.
    /// - a **site** despawns and its attending builders go idle. No refund:
    ///   destruction is not [`Self::cancel_construction`].
    /// - a **resource node** is indestructible: the call is a no-op.
    ///
    /// A caller inside [`Self::tick`] must run before any system that
    /// consumes `live_scratch`, or re-collect it: a slot despawned here
    /// stays in that buffer until the next collect, and the store's
    /// accessors assert liveness. Nothing calls this from inside a tick in
    /// this slice.
    pub fn apply_damage(&mut self, target: EntityId, damage: u32) -> DamageResult {
        let Some(slot) = self.entities.slot(target) else {
            return DamageResult::NoTarget;
        };
        let kind = self.entities.kind(slot);
        if matches!(kind, EntityKind::Node(_)) {
            return DamageResult::Indestructible;
        }
        let dealt = damage.saturating_sub(armor(kind)).max(1);
        let hp = self.entities.hp(slot);
        if dealt < hp {
            self.entities.set_hp(slot, hp - dealt);
            return DamageResult::Damaged {
                remaining_hp: hp - dealt,
            };
        }
        self.apply_death(target, slot, kind);
        DamageResult::Killed
    }

    /// Resolve a death [`Self::apply_damage`] decided. `kind` is `slot`'s
    /// kind and is never a node.
    fn apply_death(&mut self, id: EntityId, slot: usize, kind: EntityKind) {
        match kind {
            EntityKind::Unit(_) => {
                self.orders.clear(slot);
            }
            EntityKind::Building(b) => {
                // Only a finished building was ever stamped or granted
                // supply; a site was neither.
                if self.entities.progress_target(slot) == 0 {
                    let edge = b.footprint_cells();
                    let min = footprint_min(self.entities.position(slot), edge);
                    self.static_nav.unstamp_finished_building(min, edge);
                    self.static_nav
                        .rebuild_center_blocked(RTS_UNIT_BODY_RADIUS_CELLS);
                    let replaced = self
                        .nav
                        .replace_blocked_mask(self.static_nav.center_blocked());
                    debug_assert!(
                        replaced.is_ok(),
                        "pool and static_nav grids must agree in size"
                    );
                    self.supply.revoke_cap(supply_grant(b));
                }
                self.production.clear(slot);
                self.orders.clear(slot);
                // Orders that named this building die with it, in this same
                // call: a hauler bound for a dead drop-off and a builder
                // attending a dead site go idle now rather than walking at a
                // ghost until their own system notices next tick.
                for s in 0..self.entities.slot_count() {
                    if s == slot || !self.entities.alive(s) {
                        continue;
                    }
                    match self.orders.get(s) {
                        Order::Build { site, .. } if site == id => self.orders.clear(s),
                        Order::Gather {
                            phase: GatherPhase::Returning { drop_off, .. },
                            ..
                        } if drop_off == id => self.orders.clear(s),
                        _ => {}
                    }
                }
                if self.start_hq == Some(id) {
                    self.start_hq = None;
                }
            }
            EntityKind::Node(_) => unreachable!("apply_damage refuses nodes before death"),
        }
        self.entities.despawn(id);
    }
    ```
  - [ ] 4.4 In the `state_hash` doc comment (line ~3283), extend the per-slot field list: `/// progress_target, amount, carry kind, carry amount), then every live` → `/// progress_target, amount, carry kind, carry amount, hp), then every live`.
  - [ ] 4.5 Update the stale `start_hq` doc (line ~958): `/// The starting HQ. \`None\` only after it is destroyed, which nothing in` + `/// phase 1 can do.` → `/// The starting HQ. \`None\` only after it is destroyed` + `/// ([\`Self::apply_damage\`]).`
- [ ] 5. Export the new API from `crates/mmd-engine/src/rts/mod.rs`
  - [ ] 5.1 In the `pub use entity::{ … }` block (lines 31-34), add: `BARRACKS_ARMOR, BARRACKS_MAX_HP, DEPOT_ARMOR, DEPOT_MAX_HP, HQ_ARMOR, HQ_MAX_HP, SOLDIER_ARMOR, SOLDIER_MAX_HP, WORKER_ARMOR, WORKER_MAX_HP, armor, max_hp` (merge into the existing sorted list; rustfmt settles wrapping).
  - [ ] 5.2 In the `pub use world::{ … }` block (lines 91-95), add `DamageResult,` after `ContextOrderResult,`.
- [ ] 6. Doc hygiene in `crates/mmd-engine/src/rts/collision.rs`
  - [ ] 6.1 In `sync_slot`'s doc (lines 138-147), replace the now-false claim. Old text:

    ```text
    /// That placement leaves exactly one window, and it is not reachable from
    /// the game: a *unit* slot has to be recycled and then read before the
    /// next tick syncs it. Nothing in a shipping build despawns a unit at all
    /// (the only despawn is a cancelled building site, whose slot never
    /// carried a pair byte — only unit pairs are ever written), so reaching it
    /// takes a raw `testkit` despawn/respawn followed by a `state_hash` or
    /// `body_overlap_count` call with no tick in between. Deterministic even
    /// then, and cleared by the next tick.
    ```

    New text:

    ```text
    /// That placement leaves exactly one window: a *unit* slot has to be
    /// recycled and then read before the next tick syncs it. Since combat,
    /// `RtsWorld::apply_damage` can despawn a unit, so the window is reached
    /// by killing a unit and spawning into its slot with a `state_hash` or
    /// `body_overlap_count` call and no tick in between. Deterministic even
    /// then, and cleared by the next tick's sync before any movement gate
    /// reads a pair byte.
    ```
- [ ] 7. Green + gate
  - [ ] 7.1 `cargo test -p mmd-engine --test rts_combat` — all 13 tests pass.
  - [ ] 7.2 `cargo test --workspace --locked` — everything else unchanged-green (includes both tracked scripts via `tests/rts_acceptance.rs`, the alloc guard `frame_allocations.rs`, and the updated `rts_economy.rs::carry_columns_are_reserved`).
  - [ ] 7.3 `cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets --all-features -- -D warnings` — clean.
  - [ ] 7.4 `cargo run -- rts --frames 1600 --inject-input-file assets/scenarios/rts_acceptance_v1.script` — clean exit line, exit code 0 (headless: prefix `MMD_WINDOW_HIDDEN=1`).
  - [ ] 7.5 Commit as `feat(rts): hp, armor and death as the combat data core` (sign-off required — the DCO gate walks every commit).

## Outputs

- Files touched: `crates/mmd-engine/src/rts/entity.rs`, `world.rs`, `static_nav.rs`, `collision.rs` (doc only), `mod.rs`; tests: new `crates/mmd-engine/tests/rts_combat.rs`, one-line pin update in `crates/mmd-engine/tests/rts_economy.rs`.
- Public API next tickets consume **verbatim**:
  - `RtsWorld::apply_damage(&mut self, target: EntityId, damage: u32) -> DamageResult`
  - `DamageResult::{Damaged { remaining_hp }, Killed, Indestructible, NoTarget}`
  - `EntityStore::hp(slot) -> u32`, `EntityStore::set_hp(slot, hp)`
  - `rts::max_hp(EntityKind) -> u32`, `rts::armor(EntityKind) -> u32` (+ the ten backing consts)
- No config, no migration, no scenario-asset change, no `sim/` change.

## Validation

- [ ] `cargo test -p mmd-engine --test rts_combat` → `test result: ok. 13 passed`
- [ ] `cargo test --workspace --locked` → all green, zero skips beyond the documented GPU skips
- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings` → clean
- [ ] `cargo fmt --all -- --check` → clean
- [ ] `cargo run -- rts --frames 1600 --inject-input-file assets/scenarios/rts_acceptance_v1.script` → `rts: clean exit … frames=1600 …`, exit 0
- [ ] commit msg: `feat(rts): hp, armor and death as the combat data core`
