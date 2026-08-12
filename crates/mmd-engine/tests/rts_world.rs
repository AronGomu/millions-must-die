//! T6 — entity store + `RtsWorld`.
//!
//! Pure logic, no GPU, no clock: every case here is a headless CPU check of
//! `mmd_engine::rts` and `testkit::RtsHarness`.

use std::collections::HashSet;

use mmd_engine::rts::{
    BuildingKind, EntityKind, EntityStore, FORMATION_ARRIVAL_CELLS, MAX_ENTITIES, OWNER_PLAYER,
    Order, ResourceKind, Resources, RtsWorldError, Supply, UnitKind, unit_speed,
};
use mmd_engine::scenario::{Cell, MAX_SUPPLY_CAP, RtsSpec, ScenarioSpec};
use mmd_engine::testkit::{HarnessError, RtsHarness, gate_scenario_path};

// --- EntityStore ------------------------------------------------------------

#[test]
fn a_fresh_store_is_empty() {
    let store = EntityStore::new();
    assert_eq!(store.len(), 0);
    assert_eq!(store.slot_count(), 0);
    assert!(store.is_empty());
}

#[test]
fn spawn_returns_a_resolvable_id() {
    let mut store = EntityStore::new();
    let id = store
        .spawn(EntityKind::Unit(UnitKind::Worker), OWNER_PLAYER, [1.0, 2.0])
        .expect("spawn");
    assert!(store.contains(id));
    assert_eq!(store.slot(id), Some(0));
    assert_eq!(store.kind(0), EntityKind::Unit(UnitKind::Worker));
}

#[test]
fn despawn_frees_the_slot() {
    let mut store = EntityStore::new();
    let id = store
        .spawn(EntityKind::Unit(UnitKind::Worker), OWNER_PLAYER, [0.0, 0.0])
        .expect("spawn");
    assert!(store.despawn(id));
    assert_eq!(store.len(), 0);
    assert!(!store.contains(id));
    assert_eq!(store.id_at(0), None);
}

#[test]
fn a_stale_id_does_not_resolve_to_its_replacement() {
    let mut store = EntityStore::new();
    let a = store
        .spawn(EntityKind::Unit(UnitKind::Worker), OWNER_PLAYER, [0.0, 0.0])
        .expect("spawn a");
    store.despawn(a);
    let b = store
        .spawn(
            EntityKind::Unit(UnitKind::Soldier),
            OWNER_PLAYER,
            [0.0, 0.0],
        )
        .expect("spawn b");

    assert!(!store.contains(a));
    assert_eq!(store.slot(a), None);
    assert!(store.contains(b));
    assert_eq!(b.index, a.index);
    assert_ne!(b.generation, a.generation);
}

#[test]
fn double_despawn_is_rejected() {
    let mut store = EntityStore::new();
    let id = store
        .spawn(EntityKind::Unit(UnitKind::Worker), OWNER_PLAYER, [0.0, 0.0])
        .expect("spawn");
    assert!(store.despawn(id));
    assert!(!store.despawn(id));
    assert_eq!(store.len(), 0);
}

#[test]
fn the_free_list_is_lifo() {
    let mut store = EntityStore::new();
    let ids: Vec<_> = (0..3)
        .map(|_| {
            store
                .spawn(EntityKind::Unit(UnitKind::Worker), OWNER_PLAYER, [0.0, 0.0])
                .expect("spawn")
        })
        .collect();
    store.despawn(ids[0]);
    store.despawn(ids[2]);

    let next_a = store
        .spawn(EntityKind::Unit(UnitKind::Worker), OWNER_PLAYER, [0.0, 0.0])
        .expect("spawn a");
    let next_b = store
        .spawn(EntityKind::Unit(UnitKind::Worker), OWNER_PLAYER, [0.0, 0.0])
        .expect("spawn b");
    assert_eq!(
        next_a.index, 2,
        "most recently freed slot must be reused first"
    );
    assert_eq!(next_b.index, 0);
}

#[test]
fn the_store_refuses_to_overfill() {
    let mut store = EntityStore::new();
    let mut last = None;
    for _ in 0..(MAX_ENTITIES + 1) {
        last = Some(store.spawn(EntityKind::Unit(UnitKind::Worker), OWNER_PLAYER, [0.0, 0.0]));
    }
    assert_eq!(
        last,
        Some(None),
        "the store must refuse the one past capacity"
    );
    assert_eq!(store.len(), MAX_ENTITIES);
}

#[test]
fn every_column_is_reserved_at_construction() {
    let mut store = EntityStore::new();
    for _ in 0..512 {
        store
            .spawn(EntityKind::Unit(UnitKind::Worker), OWNER_PLAYER, [0.0, 0.0])
            .expect("spawn");
    }
    for cap in store.column_capacities() {
        assert_eq!(cap, MAX_ENTITIES, "no column may grow past its reservation");
    }
}

#[test]
fn collect_live_is_ascending_and_excludes_the_dead() {
    let mut store = EntityStore::new();
    let ids: Vec<_> = (0..5)
        .map(|_| {
            store
                .spawn(EntityKind::Unit(UnitKind::Worker), OWNER_PLAYER, [0.0, 0.0])
                .expect("spawn")
        })
        .collect();
    store.despawn(ids[2]);

    let mut out = Vec::new();
    store.collect_live(&mut out);
    assert_eq!(out, vec![0, 1, 3, 4]);
}

#[test]
fn footprints_come_from_the_scenario_constants() {
    assert_eq!(EntityKind::Building(BuildingKind::Hq).footprint_cells(), 12);
    assert_eq!(
        EntityKind::Building(BuildingKind::Depot).footprint_cells(),
        8
    );
    assert_eq!(
        EntityKind::Building(BuildingKind::Barracks).footprint_cells(),
        10
    );
    assert_eq!(EntityKind::Unit(UnitKind::Worker).footprint_cells(), 1);
    assert_eq!(EntityKind::Node(ResourceKind::Crystal).footprint_cells(), 1);
}

#[test]
fn only_the_hq_takes_a_drop_off() {
    assert!(BuildingKind::Hq.is_drop_off());
    assert!(!BuildingKind::Depot.is_drop_off());
    assert!(!BuildingKind::Barracks.is_drop_off());
}

#[test]
fn kind_tags_are_distinct() {
    let kinds = [
        EntityKind::Unit(UnitKind::Worker),
        EntityKind::Unit(UnitKind::Soldier),
        EntityKind::Building(BuildingKind::Hq),
        EntityKind::Building(BuildingKind::Depot),
        EntityKind::Building(BuildingKind::Barracks),
        EntityKind::Node(ResourceKind::Crystal),
        EntityKind::Node(ResourceKind::Gas),
    ];
    let tags: HashSet<u8> = kinds.iter().map(|k| k.tag()).collect();
    assert_eq!(tags.len(), 7, "every kind must have a distinct tag byte");
}

#[test]
fn rts_unit_body_radius_is_three_cells() {
    assert_eq!(UnitKind::Worker.body_radius_cells(), 3.0);
    assert_eq!(UnitKind::Soldier.body_radius_cells(), 3.0);
    assert_eq!(mmd_engine::rts::RTS_UNIT_BODY_RADIUS_CELLS, 3.0);
    assert_eq!(mmd_engine::rts::RTS_UNIT_BODY_DIAMETER_CELLS, 6.0);
}

#[test]
fn rts_unit_speeds_are_tripled() {
    assert_eq!(unit_speed(UnitKind::Worker), 30.0);
    assert_eq!(unit_speed(UnitKind::Soldier), 24.0);
}

// --- economy -----------------------------------------------------------------

#[test]
fn resources_debit_is_all_or_nothing() {
    let mut stock = Resources {
        crystal: 50,
        gas: 0,
    };
    let cost = Resources {
        crystal: 50,
        gas: 25,
    };
    assert!(!stock.try_debit(cost));
    assert_eq!(
        stock,
        Resources {
            crystal: 50,
            gas: 0
        }
    );
}

#[test]
fn resources_credit_saturates() {
    let mut stock = Resources {
        crystal: u32::MAX,
        gas: 0,
    };
    stock.credit(Resources {
        crystal: 10,
        gas: 0,
    });
    assert_eq!(stock.crystal, u32::MAX);
}

#[test]
fn supply_new_clamps_to_the_pillar() {
    let s = Supply::new(9_999);
    assert_eq!(s.cap(), MAX_SUPPLY_CAP);
    assert_eq!(s.cap(), 500);
}

#[test]
fn supply_free_saturates_when_the_cap_drops() {
    let mut s = Supply::new(10);
    s.add_used(10);
    s.revoke_cap(5);
    assert_eq!(s.free(), 0);
}

#[test]
fn supply_fits_is_exact_at_the_boundary() {
    let mut s = Supply::new(10);
    s.add_used(8);
    assert!(s.fits(2));
    assert!(!s.fits(3));
}

// --- RtsWorld seeding ----------------------------------------------------------

#[test]
fn world_seeds_the_scene() {
    let h = RtsHarness::scene().build().expect("rts scene harness");
    assert_eq!(
        h.ids_of_kind(EntityKind::Building(BuildingKind::Hq)).len(),
        1
    );
    assert_eq!(h.ids_of_kind(EntityKind::Unit(UnitKind::Worker)).len(), 6);
    assert_eq!(
        h.ids_of_kind(EntityKind::Node(ResourceKind::Crystal)).len(),
        8
    );
    assert_eq!(h.ids_of_kind(EntityKind::Node(ResourceKind::Gas)).len(), 2);
    assert_eq!(h.world().entities().len(), 17);
}

#[test]
fn world_seeds_the_starting_stock() {
    let h = RtsHarness::scene().build().expect("rts scene harness");
    assert_eq!(
        h.world().resources(),
        Resources {
            crystal: 300,
            gas: 100
        }
    );
    assert_eq!(h.world().supply().cap(), 10);
    assert_eq!(h.world().supply().used(), 6);
}

#[test]
fn the_hq_sits_at_its_footprint_centre() {
    let h = RtsHarness::scene().build().expect("rts scene harness");
    let hq = h.world().start_hq().expect("start hq");
    let slot = h.world().entities().slot(hq).expect("hq slot");
    assert_eq!(h.world().entities().position(slot), [166.0, 166.0]);
}

/// The seeded HQ is a *finished* building, and a finished building blocks
/// navigation. Stamped at seed time, not by whoever remembers to.
#[test]
fn the_seeded_hq_is_stamped_into_navigation() {
    let h = RtsHarness::scene().build().expect("rts scene harness");
    let width = h.world().scenario().width();
    // The *raw* solid mask (what placement validity reads) is exact-footprint:
    // no more, no less.
    let solids = h.world().static_nav().placement_solids();
    // The tracked scene's HQ footprint is [160, 172) x [160, 172).
    for y in 160..172u32 {
        for x in 160..172u32 {
            assert!(
                solids[(x + y * width) as usize],
                "HQ footprint cell ({x}, {y}) is not solid: a field routes straight \
                 through the base"
            );
        }
    }
    // ...and only the footprint: the raw mask's own ring around it must stay
    // clear, or placement would refuse a building next to a finished one for
    // no reason.
    for x in 159..173u32 {
        assert!(
            !solids[(x + 159 * width) as usize],
            "cell ({x}, 159) is solid; the stamp spilled past the footprint"
        );
    }
    // The *navigation* mask a pooled field reads is deliberately wider: a
    // 3-cell-radius body's own clearance from the footprint blocks its centre
    // well outside the footprint's own cells.
    let nav_blocked = h.world().nav().blocked();
    assert!(
        nav_blocked[(159 + 159 * width) as usize],
        "a body's own clearance from the HQ footprint must reach (159, 159)"
    );
}

#[test]
fn nodes_carry_their_starting_amount() {
    let h = RtsHarness::scene().build().expect("rts scene harness");
    for id in h.ids_of_kind(EntityKind::Node(ResourceKind::Crystal)) {
        let slot = h.world().entities().slot(id).expect("crystal slot");
        assert_eq!(h.world().entities().amount(slot), 1_500);
    }
    for id in h.ids_of_kind(EntityKind::Node(ResourceKind::Gas)) {
        let slot = h.world().entities().slot(id).expect("gas slot");
        assert_eq!(h.world().entities().amount(slot), 2_500);
    }
}

#[test]
fn workers_start_on_the_scenario_spawn_cells() {
    let h = RtsHarness::scene().build().expect("rts scene harness");
    // The scenario's 6 spawn cells sit one cell apart (162..167 @ y=178), far
    // closer than two 3-cell-radius bodies can share, so every worker but the
    // first is relocated to the nearest legal, non-overlapping cell centre.
    // Deterministic and pinned here so a regression in the relocation search
    // shows up as a diff against a known-good layout, not a vague "positions
    // changed".
    let expected: Vec<[f32; 2]> = vec![
        [162.5, 178.5],
        [168.5, 178.5],
        [164.5, 184.5],
        [170.5, 184.5],
        [174.5, 178.5],
        [175.5, 172.5],
    ];
    let workers = h.ids_of_kind(EntityKind::Unit(UnitKind::Worker));
    assert_eq!(workers.len(), expected.len());
    for (id, want) in workers.into_iter().zip(expected) {
        let slot = h.world().entities().slot(id).expect("worker slot");
        assert_eq!(h.world().entities().position(slot), want);
    }
}

#[test]
fn seeding_order_is_hq_then_nodes_then_workers() {
    let h = RtsHarness::scene().build().expect("rts scene harness");
    let entities = h.world().entities();
    assert_eq!(entities.kind(0), EntityKind::Building(BuildingKind::Hq));
    for slot in 1..=10 {
        assert!(
            matches!(entities.kind(slot), EntityKind::Node(_)),
            "slot {slot} must be a node"
        );
    }
    for slot in 11..=16 {
        assert_eq!(
            entities.kind(slot),
            EntityKind::Unit(UnitKind::Worker),
            "slot {slot} must be a worker"
        );
    }
}

#[test]
fn a_phase0_scenario_is_refused() {
    let err = RtsHarness::path(gate_scenario_path())
        .build()
        .expect_err("phase-0 scenario must be refused");
    assert!(
        matches!(err, HarnessError::Rts(RtsWorldError::NotAnRtsScene { .. })),
        "expected NotAnRtsScene, got {err:?}"
    );
}

// --- tick + state hash ---------------------------------------------------------

#[test]
fn tick_advances_only_the_counter() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let mut before = Vec::new();
    h.world().entities().collect_live(&mut before);
    let snapshot: Vec<_> = before
        .iter()
        .map(|&slot| {
            let e = h.world().entities();
            (
                e.position(slot),
                e.dir(slot),
                e.frame(slot),
                e.progress(slot),
                e.progress_target(slot),
                e.amount(slot),
            )
        })
        .collect();

    h.step_exact(10);
    assert_eq!(h.tick_index(), 10);

    for (&slot, want) in before.iter().zip(snapshot.iter()) {
        let e = h.world().entities();
        let got = (
            e.position(slot),
            e.dir(slot),
            e.frame(slot),
            e.progress(slot),
            e.progress_target(slot),
            e.amount(slot),
        );
        assert_eq!(&got, want, "slot {slot} changed on a bare tick");
    }
}

#[test]
fn state_hash_moves_with_the_tick() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let before = h.state_hash();
    h.step_exact(1);
    assert_ne!(h.state_hash(), before);
}

#[test]
fn state_hash_is_reproducible_across_worlds() {
    let mut a = RtsHarness::scene().build().expect("a");
    let mut b = RtsHarness::scene().build().expect("b");
    a.step_exact(300);
    b.step_exact(300);
    assert_eq!(a.state_hash(), b.state_hash());
}

#[test]
fn state_hash_sees_a_moved_entity() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let before = h.state_hash();
    h.world_mut().entities_mut().set_position(11, [1.0, 1.0]);
    assert_ne!(h.state_hash(), before);
}

#[test]
fn state_hash_sees_a_spent_resource() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let before = h.state_hash();
    assert!(
        h.world_mut()
            .resources_mut()
            .try_debit(Resources { crystal: 1, gas: 0 })
    );
    assert_ne!(h.state_hash(), before);
}

#[test]
fn state_hash_sees_a_drained_node() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let before = h.state_hash();
    h.world_mut().entities_mut().set_amount(1, 0);
    assert_ne!(h.state_hash(), before);
}

#[test]
fn state_hash_ignores_a_dead_slot() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let before = h.state_hash();
    let id = h
        .world_mut()
        .entities_mut()
        .spawn(EntityKind::Unit(UnitKind::Worker), OWNER_PLAYER, [5.0, 5.0])
        .expect("spawn extra worker");
    h.world_mut().entities_mut().despawn(id);
    assert_eq!(h.state_hash(), before);
}

#[test]
fn harness_rng_streams_are_independent() {
    let h = RtsHarness::scene()
        .seed(7)
        .build()
        .expect("rts scene harness");
    let a: Vec<u64> = {
        let mut r = h.rng("a");
        (0..8).map(|_| r.next_u64()).collect()
    };
    let b: Vec<u64> = {
        let mut r = h.rng("b");
        (0..8).map(|_| r.next_u64()).collect()
    };
    assert_ne!(a, b);
}

// --- T7: orders + movement -------------------------------------------------

/// A 320x320 RTS scene with hand-placed obstacles.
///
/// The RTS family's geometry is locked to 320x320 by
/// `scenario::validate_rts_scene_dims`, and the rts block is required exactly
/// on that family, so a small synthetic RTS grid is not expressible — the
/// obstacles below are placed at their literal coordinates on the full grid
/// instead of rescaled, so each case still proves what it was written to prove.
fn rts_spec(obstacles: Vec<u32>, spawns: Vec<Cell>, destination: Cell) -> ScenarioSpec {
    ScenarioSpec {
        version: "rts_prototype_v1".to_string(),
        width: 320,
        height: 320,
        cell_size_px: 4,
        sprite_size_px: 48,
        hard_agent_count: 0,
        stretch_agent_count: 0,
        seed: 0x5715_1234,
        destination,
        spawn_cells: spawns,
        atlas_count: 4,
        direction_count: 8,
        frame_count: 4,
        collision_radius_q8: 0,
        separation_strength_q8: 0,
        separation_phases: 1,
        mass_class_count: 1,
        separation_threads: 1,
        obstacle_cells: obstacles,
        rts: Some(RtsSpec {
            start_crystal: 300,
            start_gas: 100,
            start_supply_cap: 10,
            hq_cell: Cell { x: 100, y: 100 },
            crystal_nodes: vec![Cell { x: 100, y: 50 }],
            gas_nodes: vec![Cell { x: 101, y: 50 }],
        }),
    }
}

/// Full wall at `x == 16` with a gap centred on `y == 8`.
///
/// The gap must be wide enough for a 3-cell-radius body to clear both
/// flanking wall segments at once (each flanking segment must sit at least a
/// body radius from the gap's own centre): `y` in `2..=14` leaves the centre
/// cell `(16, 8)` six cells clear of the nearest wall cell on either side, well
/// past the 3-cell radius this fixture exists to exercise.
fn wall_with_a_gap() -> Vec<u32> {
    (0..320u32)
        .filter(|y| !(2..=14).contains(y))
        .map(|y| 16 + y * 320)
        .collect()
}

/// A sealed chamber of free ground, fully enclosed (no gap at all), large
/// enough that its own centre sits well past a 3-cell-radius body's own
/// clearance from every wall — unlike the destination itself, which must
/// stay legal, or `order_move` would refuse the order before the field ever
/// got a chance to prove it unreachable.
fn sealed_chamber() -> Vec<u32> {
    let mut out = Vec::new();
    for y in 193..=207u32 {
        for x in 193..=207u32 {
            let on_border = x == 193 || x == 207 || y == 193 || y == 207;
            if on_border {
                out.push(x + y * 320);
            }
        }
    }
    out
}

fn first_worker(h: &RtsHarness) -> mmd_engine::rts::EntityId {
    h.ids_of_kind(EntityKind::Unit(UnitKind::Worker))[0]
}

fn position_of(h: &RtsHarness, id: mmd_engine::rts::EntityId) -> [f32; 2] {
    let slot = h.world().entities().slot(id).expect("live entity");
    h.world().entities().position(slot)
}

#[test]
fn order_move_sets_a_move_order() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let worker = first_worker(&h);
    let dest = Cell { x: 200, y: 200 };
    assert!(h.world_mut().order_move(worker, dest));
    assert!(matches!(
        h.world().order_of(worker),
        Some(Order::Move { goal, .. }) if goal.anchor == dest && goal.slot == dest
    ));
}

#[test]
fn order_move_rejects_a_stale_id() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let worker = first_worker(&h);
    assert!(h.world_mut().entities_mut().despawn(worker));
    assert!(!h.world_mut().order_move(worker, Cell { x: 200, y: 200 }));
    assert_eq!(h.world().order_of(worker), None);
}

#[test]
fn order_move_rejects_a_building() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let hq = h.world().start_hq().expect("start hq");
    assert!(!h.world_mut().order_move(hq, Cell { x: 200, y: 200 }));
    assert_eq!(h.world().order_of(hq), Some(Order::Idle));
}

#[test]
fn order_move_rejects_a_neutral_node() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let node = h.ids_of_kind(EntityKind::Node(ResourceKind::Crystal))[0];
    assert!(!h.world_mut().order_move(node, Cell { x: 200, y: 200 }));
    assert_eq!(h.world().order_of(node), Some(Order::Idle));
}

/// A click on a rock is an order to stand *by* the rock, not a refusal: since
/// T5 the anchor snaps to the nearest cell a body may legally occupy. Only a
/// destination off the grid entirely has nothing to snap to.
#[test]
fn order_move_snaps_a_blocked_destination_to_a_legal_anchor() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let worker = first_worker(&h);
    // Cell (0, 0) is the tracked scene's first obstacle.
    let rock = Cell { x: 0, y: 0 };
    assert!(
        h.world()
            .scenario()
            .is_obstacle_index(rock.x + rock.y * 320)
    );

    assert!(h.world_mut().order_move(worker, rock));
    let goal = match h.world().order_of(worker) {
        Some(Order::Move { goal, .. }) => goal,
        other => panic!("the worker is not moving: {other:?}"),
    };
    assert_ne!(goal.anchor, rock, "the anchor must not be the rock itself");
    assert!(
        !h.world().static_nav().center_blocked()[(goal.anchor.x + goal.anchor.y * 320) as usize],
        "the snapped anchor {:?} is not a legal body position",
        goal.anchor
    );

    // Off the grid has no legal cell to snap to, and is still refused.
    let live_dest = Cell { x: 200, y: 200 };
    assert!(h.world_mut().order_move(worker, live_dest));
    let before = h.world().order_of(worker);
    assert!(!h.world_mut().order_move(worker, Cell { x: 320, y: 0 }));
    assert_eq!(
        h.world().order_of(worker),
        before,
        "a refused destination must not corrupt the live order"
    );
}

#[test]
fn order_move_group_acquires_once() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let workers = h.ids_of_kind(EntityKind::Unit(UnitKind::Worker));
    assert_eq!(workers.len(), 6);
    let before = h.world().nav().rebuild_count();
    let acquires = h.world().nav().acquire_count();
    let ordered = h
        .world_mut()
        .order_move_group(&workers, Cell { x: 200, y: 200 });
    assert_eq!(ordered, Ok(6));
    assert_eq!(
        h.world().nav().rebuild_count(),
        before + 1,
        "a group order must rebuild its field exactly once"
    );
    // The rebuild count alone cannot see this: six acquires of one cached
    // destination are five hits and one miss.
    assert_eq!(
        h.world().nav().acquire_count(),
        acquires + 1,
        "a group order must acquire its field exactly once, not once per unit"
    );
}

#[test]
fn a_group_sharing_a_destination_shares_a_field() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let workers = h.ids_of_kind(EntityKind::Unit(UnitKind::Worker));
    assert!(
        h.world_mut()
            .order_move_group(&workers, Cell { x: 200, y: 200 })
            .is_ok()
    );
    let slots: HashSet<u8> = workers
        .iter()
        .map(|id| match h.world().order_of(*id) {
            Some(Order::Move { field, .. }) => field.slot,
            other => panic!("worker is not moving: {other:?}"),
        })
        .collect();
    assert_eq!(slots.len(), 1, "one destination must mean one field");
}

#[test]
fn a_unit_reaches_its_destination() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let worker = h
        .ids_of_kind(EntityKind::Unit(UnitKind::Worker))
        .into_iter()
        .find(|id| position_of(&h, *id) == [174.5, 178.5])
        .expect("a worker spawned near (174, 178) after relocation");
    let dest = Cell { x: 200, y: 200 };
    assert!(h.world_mut().order_move(worker, dest));

    h.step_exact(1_200);

    let p = position_of(&h, worker);
    let dx = p[0] - 200.5;
    let dy = p[1] - 200.5;
    assert!(
        dx * dx + dy * dy <= FORMATION_ARRIVAL_CELLS * FORMATION_ARRIVAL_CELLS,
        "worker stopped at {p:?}, not within {FORMATION_ARRIVAL_CELLS} of (200.5, 200.5)"
    );
    assert_eq!(h.world().order_of(worker), Some(Order::Idle));
}

#[test]
fn a_unit_walks_around_an_obstacle() {
    let spec = rts_spec(
        wall_with_a_gap(),
        vec![Cell { x: 2, y: 20 }],
        Cell { x: 30, y: 20 },
    );
    let mut h = RtsHarness::spec(spec).build().expect("walled rts scene");
    let worker = first_worker(&h);
    let dest = Cell { x: 30, y: 20 };
    assert!(h.world_mut().order_move(worker, dest));

    let mut near_the_gap = false;
    let mut arrived = false;
    for _ in 0..1_200 {
        h.step_exact(1);
        let p = position_of(&h, worker);
        let cx = p[0].floor() as i32;
        // The wall is solid at every other cell of `x == 16`, so crossing at
        // `x == 16` at all proves the gap was used — the destination sits at
        // `y == 20`, well outside the `2..=14` gap window, so the walk's
        // actual crossing row is not pinned to the gap's own centre.
        if cx == 16 {
            near_the_gap = true;
        }
        if h.world().order_of(worker) == Some(Order::Idle) {
            arrived = true;
            break;
        }
    }
    assert!(arrived, "the worker never finished its order");
    let p = position_of(&h, worker);
    let dx = p[0] - 30.5;
    let dy = p[1] - 20.5;
    assert!(
        dx * dx + dy * dy <= FORMATION_ARRIVAL_CELLS * FORMATION_ARRIVAL_CELLS,
        "worker cleared its order at {p:?}, away from the destination"
    );
    assert!(
        near_the_gap,
        "the walk never passed the only gap in the wall"
    );
}

#[test]
fn a_unit_never_enters_a_blocked_cell() {
    let spec = rts_spec(
        wall_with_a_gap(),
        vec![Cell { x: 2, y: 20 }],
        Cell { x: 30, y: 20 },
    );
    let mut h = RtsHarness::spec(spec).build().expect("walled rts scene");
    let worker = first_worker(&h);
    assert!(h.world_mut().order_move(worker, Cell { x: 30, y: 20 }));

    for _ in 0..1_200 {
        h.step_exact(1);
        let p = position_of(&h, worker);
        let idx = (p[0].floor() as u32) + (p[1].floor() as u32) * 320;
        assert!(
            !h.world().scenario().is_obstacle_index(idx),
            "worker stood inside an obstacle at {p:?}"
        );
        if h.world().order_of(worker) == Some(Order::Idle) {
            break;
        }
    }
}

#[test]
fn an_unreachable_destination_clears_the_order() {
    let spec = rts_spec(
        sealed_chamber(),
        vec![Cell { x: 2, y: 20 }],
        Cell { x: 30, y: 20 },
    );
    let mut h = RtsHarness::spec(spec).build().expect("sealed rts scene");
    let worker = first_worker(&h);
    let before = position_of(&h, worker);
    // The chamber is free ground, so the order is accepted — and then found
    // impossible by the field, not by the order check.
    assert!(h.world_mut().order_move(worker, Cell { x: 201, y: 201 }));

    h.step_exact(2);
    assert_eq!(
        h.world().order_of(worker),
        Some(Order::Idle),
        "an unreachable destination must clear, not spin"
    );
    assert_eq!(position_of(&h, worker), before, "the worker moved anyway");
}

/// Arrival is measured from the unit's own formation slot's *centre*, at
/// `FORMATION_ARRIVAL_CELLS`: a unit parked a fifth of a cell east of the
/// centre has arrived, one parked a third of a cell east has not.
#[test]
fn arrival_is_measured_from_the_slot_centre() {
    let park = |offset: f32| {
        let mut h = RtsHarness::scene().build().expect("rts scene harness");
        let worker = first_worker(&h);
        let dest = Cell { x: 200, y: 200 };
        assert!(h.world_mut().order_move(worker, dest));
        let goal = match h.world().order_of(worker) {
            Some(Order::Move { goal, .. }) => goal,
            other => panic!("the worker is not moving: {other:?}"),
        };
        assert_eq!(goal.slot, dest, "a lone unit forms up on the anchor itself");

        let placed = [goal.slot.x as f32 + 0.5 + offset, goal.slot.y as f32 + 0.5];
        let slot = h.world().entities().slot(worker).expect("worker slot");
        h.world_mut().entities_mut().set_position(slot, placed);
        h.step_exact(1);
        (h.world().order_of(worker), position_of(&h, worker), placed)
    };

    let (order, p, placed) = park(FORMATION_ARRIVAL_CELLS - 0.05);
    assert_eq!(
        order,
        Some(Order::Idle),
        "inside the arrival radius is arrival"
    );
    assert_eq!(p, placed, "an arrival must not step");

    let (order, _, _) = park(FORMATION_ARRIVAL_CELLS + 0.05);
    assert!(
        matches!(order, Some(Order::Move { .. })),
        "outside the arrival radius the order must still be live, got {order:?}"
    );
}

#[test]
fn worker_outruns_soldier() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let worker = first_worker(&h);
    let start = position_of(&h, worker);
    let soldier = h
        .world_mut()
        .entities_mut()
        .spawn(EntityKind::Unit(UnitKind::Soldier), OWNER_PLAYER, start)
        .expect("spawn a soldier");

    let dest = Cell { x: 200, y: 200 };
    assert_eq!(
        h.world_mut().order_move_group(&[worker, soldier], dest),
        Ok(2)
    );
    h.step_exact(60);

    let dist = |p: [f32; 2]| {
        let dx = p[0] - start[0];
        let dy = p[1] - start[1];
        (dx * dx + dy * dy).sqrt()
    };
    let walked_worker = dist(position_of(&h, worker));
    let walked_soldier = dist(position_of(&h, soldier));
    assert!(
        walked_worker > walked_soldier,
        "worker walked {walked_worker}, soldier {walked_soldier}; the kinds \
         must not share one speed"
    );
}

#[test]
fn an_idle_unit_does_not_move_or_animate() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let worker = first_worker(&h);
    let slot = h.world().entities().slot(worker).expect("worker slot");
    let before = (
        h.world().entities().position(slot),
        h.world().entities().dir(slot),
        h.world().entities().frame(slot),
    );

    h.step_exact(600);

    let after = (
        h.world().entities().position(slot),
        h.world().entities().dir(slot),
        h.world().entities().frame(slot),
    );
    assert_eq!(after, before, "an idle unit walked or animated");
}

#[test]
fn movement_is_reproducible() {
    let dest = Cell { x: 200, y: 200 };
    let mut a = RtsHarness::scene().build().expect("a");
    let mut b = RtsHarness::scene().build().expect("b");
    for h in [&mut a, &mut b] {
        let workers = h.ids_of_kind(EntityKind::Unit(UnitKind::Worker));
        assert_eq!(h.world_mut().order_move_group(&workers, dest), Ok(6));
        h.step_exact(600);
    }
    assert_eq!(a.state_hash(), b.state_hash());
}

#[test]
fn state_hash_sees_an_order() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let worker = first_worker(&h);
    let before = h.state_hash();
    assert!(h.world_mut().order_move(worker, Cell { x: 200, y: 200 }));
    assert_ne!(h.state_hash(), before, "an order must reach the state hash");
}
