//! T6 — entity store + `RtsWorld`.
//!
//! Pure logic, no GPU, no clock: every case here is a headless CPU check of
//! `mmd_engine::rts` and `testkit::RtsHarness`.

use std::collections::HashSet;

use mmd_engine::rts::{
    BuildingKind, EntityKind, EntityStore, MAX_ENTITIES, OWNER_PLAYER, ResourceKind, Resources,
    RtsWorldError, Supply, UnitKind,
};
use mmd_engine::scenario::MAX_SUPPLY_CAP;
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
    let expected: Vec<[f32; 2]> = h
        .world()
        .scenario()
        .spawn_cells()
        .iter()
        .map(|c| [c.x as f32 + 0.5, c.y as f32 + 0.5])
        .collect();
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
