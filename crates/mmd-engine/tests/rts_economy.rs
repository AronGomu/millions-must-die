//! T9 — the gather loop: two resources, one worker round trip.
//!
//! Pure logic, no GPU, no clock: every case here is a headless CPU check of
//! `mmd_engine::rts` and `testkit::RtsHarness`.

use mmd_engine::rts::{
    BuildingKind, EntityId, EntityKind, EntityStore, GATHER_TICKS, GatherPhase, MAX_ENTITIES,
    OWNER_PLAYER, Order, ResourceKind, UnitKind, WORKER_CARRY_CAPACITY,
};
use mmd_engine::testkit::RtsHarness;

fn first_worker(h: &RtsHarness) -> EntityId {
    h.ids_of_kind(EntityKind::Unit(UnitKind::Worker))[0]
}

fn crystal_node(h: &RtsHarness) -> EntityId {
    h.ids_of_kind(EntityKind::Node(ResourceKind::Crystal))[0]
}

fn gas_node(h: &RtsHarness) -> EntityId {
    h.ids_of_kind(EntityKind::Node(ResourceKind::Gas))[0]
}

fn position_of(h: &RtsHarness, id: EntityId) -> [f32; 2] {
    let slot = h.world().entities().slot(id).expect("live entity");
    h.world().entities().position(slot)
}

/// Teleport `w0` onto `node`'s cell and order it gathering — skips the walk
/// so a test can drive the mining/return machinery directly.
fn park_and_order_gather(h: &mut RtsHarness, w0: EntityId, node: EntityId) -> usize {
    let node_slot = h.world().entities().slot(node).expect("node slot");
    let node_pos = h.world().entities().position(node_slot);
    let w0_slot = h.world().entities().slot(w0).expect("worker slot");
    h.world_mut().entities_mut().set_position(w0_slot, node_pos);
    assert!(h.world_mut().order_gather(w0, node));
    w0_slot
}

// --- carry columns -----------------------------------------------------------

#[test]
fn carry_starts_empty() {
    let h = RtsHarness::scene().build().expect("rts scene harness");
    let mut slots = Vec::new();
    h.world().entities().collect_live(&mut slots);
    let mut checked = 0;
    for slot in slots {
        if matches!(h.world().entities().kind(slot), EntityKind::Unit(_)) {
            assert_eq!(h.world().entities().carry(slot), None);
            checked += 1;
        }
    }
    assert!(checked > 0, "the scene must seed at least one unit");
}

#[test]
fn set_carry_round_trips() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    h.world_mut()
        .entities_mut()
        .set_carry(11, Some((ResourceKind::Gas, 5)));
    assert_eq!(h.world().entities().carry(11), Some((ResourceKind::Gas, 5)));
    h.world_mut().entities_mut().set_carry(11, None);
    assert_eq!(h.world().entities().carry(11), None);
}

#[test]
fn carry_columns_are_reserved() {
    let mut store = EntityStore::new();
    for _ in 0..512 {
        store
            .spawn(EntityKind::Unit(UnitKind::Worker), OWNER_PLAYER, [0.0, 0.0])
            .expect("spawn");
    }
    let caps = store.column_capacities();
    assert_eq!(caps.len(), 13);
    for cap in caps {
        assert_eq!(cap, MAX_ENTITIES, "no column may grow past its reservation");
    }
}

// --- order_gather --------------------------------------------------------------

#[test]
fn order_gather_accepts_a_worker_and_a_node() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let w0 = first_worker(&h);
    let n_crystal = crystal_node(&h);
    assert!(h.world_mut().order_gather(w0, n_crystal));
    assert!(matches!(
        h.world().order_of(w0),
        Some(Order::Gather {
            node,
            phase: GatherPhase::ToNode { .. }
        }) if node == n_crystal
    ));
}

#[test]
fn order_gather_rejects_a_soldier() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let n_crystal = crystal_node(&h);
    let soldier = h
        .world_mut()
        .entities_mut()
        .spawn(
            EntityKind::Unit(UnitKind::Soldier),
            OWNER_PLAYER,
            [5.0, 5.0],
        )
        .expect("spawn a soldier");
    assert!(!h.world_mut().order_gather(soldier, n_crystal));
}

#[test]
fn order_gather_rejects_a_building_as_the_node() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let w0 = first_worker(&h);
    let hq = h.world().start_hq().expect("start hq");
    assert!(!h.world_mut().order_gather(w0, hq));
}

#[test]
fn order_gather_rejects_an_empty_node() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let w0 = first_worker(&h);
    let n_crystal = crystal_node(&h);
    let node_slot = h.world().entities().slot(n_crystal).expect("node slot");
    h.world_mut().entities_mut().set_amount(node_slot, 0);
    assert!(!h.world_mut().order_gather(w0, n_crystal));
}

#[test]
fn order_gather_rejects_a_stale_worker() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let w0 = first_worker(&h);
    let n_crystal = crystal_node(&h);
    assert!(h.world_mut().entities_mut().despawn(w0));
    assert!(!h.world_mut().order_gather(w0, n_crystal));
}

#[test]
fn order_gather_group_acquires_once() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let workers = h.ids_of_kind(EntityKind::Unit(UnitKind::Worker));
    assert_eq!(workers.len(), 6);
    let n_crystal = crystal_node(&h);
    let before = h.world().nav().rebuild_count();
    let ordered = h.world_mut().order_gather_group(&workers, n_crystal);
    assert_eq!(ordered, 6);
    assert_eq!(
        h.world().nav().rebuild_count(),
        before + 1,
        "a group gather order must rebuild its field exactly once"
    );
}

// --- the walk and the mine ------------------------------------------------------

#[test]
fn a_worker_walks_to_its_node() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let w0 = first_worker(&h);
    let n_crystal = crystal_node(&h);
    assert!(h.world_mut().order_gather(w0, n_crystal));

    let mut saw_mining = false;
    for _ in 0..400 {
        h.step_exact(1);
        if matches!(
            h.world().order_of(w0),
            Some(Order::Gather {
                phase: GatherPhase::Mining { .. },
                ..
            })
        ) {
            saw_mining = true;
            break;
        }
    }
    assert!(saw_mining, "worker never started mining within 400 ticks");
}

#[test]
fn mining_takes_the_documented_time() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let w0 = first_worker(&h);
    let n_crystal = crystal_node(&h);
    let w0_slot = park_and_order_gather(&mut h, w0, n_crystal);

    h.step_exact(1);
    assert!(
        matches!(
            h.world().order_of(w0),
            Some(Order::Gather {
                phase: GatherPhase::Mining { .. },
                ..
            })
        ),
        "one tick at the node must enter Mining"
    );

    // One tick short of the documented time: still mining, cargo still empty.
    h.step_exact(GATHER_TICKS as u64 - 1);
    assert!(
        matches!(
            h.world().order_of(w0),
            Some(Order::Gather {
                phase: GatherPhase::Mining { .. },
                ..
            })
        ),
        "mining must not finish before GATHER_TICKS"
    );
    assert_eq!(h.world().entities().carry(w0_slot), None);

    // The documented tick: loaded, and walking home.
    h.step_exact(1);
    assert_eq!(
        h.world().entities().carry(w0_slot),
        Some((ResourceKind::Crystal, WORKER_CARRY_CAPACITY))
    );
    assert!(matches!(
        h.world().order_of(w0),
        Some(Order::Gather {
            phase: GatherPhase::Returning { .. },
            ..
        })
    ));
}

#[test]
fn a_mining_worker_does_not_move() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let w0 = first_worker(&h);
    let n_crystal = crystal_node(&h);
    let w0_slot = park_and_order_gather(&mut h, w0, n_crystal);

    h.step_exact(1); // enters Mining
    let pos_at_mining_start = h.world().entities().position(w0_slot);

    for _ in 0..(GATHER_TICKS - 1) {
        h.step_exact(1);
        assert!(matches!(
            h.world().order_of(w0),
            Some(Order::Gather {
                phase: GatherPhase::Mining { .. },
                ..
            })
        ));
        assert_eq!(h.world().entities().position(w0_slot), pos_at_mining_start);
    }
}

#[test]
fn the_node_loses_exactly_the_carried_amount() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let w0 = first_worker(&h);
    let n_crystal = crystal_node(&h);
    let node_slot = h.world().entities().slot(n_crystal).expect("node slot");
    park_and_order_gather(&mut h, w0, n_crystal);

    h.step_exact(1 + GATHER_TICKS as u64);
    assert_eq!(
        h.world().entities().amount(node_slot),
        1_500 - WORKER_CARRY_CAPACITY
    );
    assert_eq!(h.world().entities().amount(node_slot), 1_492);
}

#[test]
fn a_partial_node_pays_out_what_is_left() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let w0 = first_worker(&h);
    let n_crystal = crystal_node(&h);
    let node_slot = h.world().entities().slot(n_crystal).expect("node slot");
    h.world_mut().entities_mut().set_amount(node_slot, 3);
    let w0_slot = park_and_order_gather(&mut h, w0, n_crystal);

    h.step_exact(1 + GATHER_TICKS as u64);
    assert_eq!(
        h.world().entities().carry(w0_slot),
        Some((ResourceKind::Crystal, 3))
    );
    assert_eq!(h.world().entities().amount(node_slot), 0);
}

#[test]
fn an_emptied_node_ends_the_order() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let w0 = first_worker(&h);
    let n_crystal = crystal_node(&h);
    let node_slot = h.world().entities().slot(n_crystal).expect("node slot");
    h.world_mut().entities_mut().set_amount(node_slot, 3);
    assert!(h.world_mut().order_gather(w0, n_crystal));

    h.step_exact(3_000);
    assert_eq!(
        h.world().order_of(w0),
        Some(Order::Idle),
        "an emptied node must end the order after the last delivery"
    );
    assert_eq!(h.world().entities().amount(node_slot), 0);
}

// --- the round trip --------------------------------------------------------------

#[test]
fn a_full_round_trip_banks_crystal() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let w0 = first_worker(&h);
    let n_crystal = crystal_node(&h);
    assert!(h.world_mut().order_gather(w0, n_crystal));
    h.step_exact(3_000);
    assert!(h.world().resources().crystal > 300);
    assert_eq!(h.world().resources().gas, 100);
}

#[test]
fn a_full_round_trip_banks_gas() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let w0 = first_worker(&h);
    let n_gas = gas_node(&h);
    assert!(h.world_mut().order_gather(w0, n_gas));
    h.step_exact(3_000);
    assert!(h.world().resources().gas > 100);
    assert_eq!(h.world().resources().crystal, 300);
}

#[test]
fn the_worker_keeps_cycling() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let w0 = first_worker(&h);
    let n_crystal = crystal_node(&h);
    assert!(h.world_mut().order_gather(w0, n_crystal));
    h.step_exact(6_000);
    assert!(
        h.world().resources().crystal >= 300 + 3 * WORKER_CARRY_CAPACITY,
        "expected at least 3 deliveries, got {}",
        h.world().resources().crystal
    );
}

#[test]
fn cargo_is_cleared_on_delivery() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let w0 = first_worker(&h);
    let n_crystal = crystal_node(&h);
    let w0_slot = h.world().entities().slot(w0).expect("worker slot");
    assert!(h.world_mut().order_gather(w0, n_crystal));

    let mut delivered = false;
    let mut prev_crystal = h.world().resources().crystal;
    for _ in 0..3_000 {
        h.step_exact(1);
        let now = h.world().resources().crystal;
        if now > prev_crystal {
            assert_eq!(h.world().entities().carry(w0_slot), None);
            delivered = true;
            break;
        }
        prev_crystal = now;
    }
    assert!(delivered, "worker never delivered within 3000 ticks");
}

// --- drop-off resolution ---------------------------------------------------------

#[test]
fn delivery_uses_the_footprint_not_the_centre() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let w0 = first_worker(&h);
    let n_crystal = crystal_node(&h);
    let node_slot = h.world().entities().slot(n_crystal).expect("node slot");
    // Exactly one load, so the node is empty the moment mining completes and
    // delivery ends the order outright — no second leg to account for.
    h.world_mut()
        .entities_mut()
        .set_amount(node_slot, WORKER_CARRY_CAPACITY);
    let w0_slot = park_and_order_gather(&mut h, w0, n_crystal);

    h.step_exact(1 + GATHER_TICKS as u64);
    assert!(matches!(
        h.world().order_of(w0),
        Some(Order::Gather {
            phase: GatherPhase::Returning { .. },
            ..
        })
    ));
    assert_eq!(h.world().entities().amount(node_slot), 0);

    // HQ centre is [166.0, 166.0], footprint edge 12: the footprint spans
    // 160..172 on each axis. One cell outside the edge on the x axis is
    // (172 + 1.0, 166.0) — 7.0 from the centre, well past a centre-distance
    // rule, but exactly `DROP_OFF_REACH_CELLS` from the footprint rectangle.
    let boundary = [173.0, 166.0];
    h.world_mut().entities_mut().set_position(w0_slot, boundary);
    let before_crystal = h.world().resources().crystal;

    h.step_exact(1);

    assert_eq!(
        h.world().entities().position(w0_slot),
        boundary,
        "the node was already empty, so delivery must end the order without a step"
    );
    assert_eq!(h.world().entities().carry(w0_slot), None);
    assert_eq!(
        h.world().resources().crystal,
        before_crystal + WORKER_CARRY_CAPACITY
    );
    assert_eq!(h.world().order_of(w0), Some(Order::Idle));
}

#[test]
fn delivery_one_cell_further_does_not_fire() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let w0 = first_worker(&h);
    let n_crystal = crystal_node(&h);
    let w0_slot = park_and_order_gather(&mut h, w0, n_crystal);

    h.step_exact(1 + GATHER_TICKS as u64);
    assert!(matches!(
        h.world().order_of(w0),
        Some(Order::Gather {
            phase: GatherPhase::Returning { .. },
            ..
        })
    ));

    // 1.1 cells outside the footprint edge: past `DROP_OFF_REACH_CELLS`.
    let just_outside = [173.1, 166.0];
    h.world_mut()
        .entities_mut()
        .set_position(w0_slot, just_outside);
    let before_crystal = h.world().resources().crystal;

    h.step_exact(1);

    assert_eq!(
        h.world().entities().carry(w0_slot),
        Some((ResourceKind::Crystal, WORKER_CARRY_CAPACITY)),
        "cargo must not bank one cell past the reach"
    );
    assert_eq!(h.world().resources().crystal, before_crystal);
    assert!(matches!(
        h.world().order_of(w0),
        Some(Order::Gather {
            phase: GatherPhase::Returning { .. },
            ..
        })
    ));
}

#[test]
fn nearest_drop_off_prefers_the_closer_building() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let w0 = first_worker(&h);
    let pos = position_of(&h, w0);
    let closer = h
        .world_mut()
        .entities_mut()
        .spawn(
            EntityKind::Building(BuildingKind::Hq),
            OWNER_PLAYER,
            [pos[0] + 3.0, pos[1]],
        )
        .expect("spawn a second hq");

    assert_eq!(h.world().nearest_drop_off(pos), Some(closer));
}

#[test]
fn nearest_drop_off_ignores_non_drop_off_buildings() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let w0 = first_worker(&h);
    let pos = position_of(&h, w0);
    let hq = h.world().start_hq().expect("start hq");
    h.world_mut()
        .entities_mut()
        .spawn(
            EntityKind::Building(BuildingKind::Barracks),
            OWNER_PLAYER,
            [pos[0] + 1.0, pos[1]],
        )
        .expect("spawn a barracks");

    assert_eq!(h.world().nearest_drop_off(pos), Some(hq));
}

#[test]
fn nearest_drop_off_ties_go_to_the_lower_slot() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let hq = h.world().start_hq().expect("start hq");
    let hq_pos = position_of(&h, hq);
    let second = h
        .world_mut()
        .entities_mut()
        .spawn(EntityKind::Building(BuildingKind::Hq), OWNER_PLAYER, hq_pos)
        .expect("spawn a tied hq");
    assert!(hq.index < second.index);

    let query = [hq_pos[0] + 50.0, hq_pos[1]];
    assert_eq!(h.world().nearest_drop_off(query), Some(hq));
}

#[test]
fn no_drop_off_stops_the_worker_holding_cargo() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let w0 = first_worker(&h);
    let n_crystal = crystal_node(&h);
    let w0_slot = park_and_order_gather(&mut h, w0, n_crystal);

    h.step_exact(1 + GATHER_TICKS as u64);
    assert!(matches!(
        h.world().order_of(w0),
        Some(Order::Gather {
            phase: GatherPhase::Returning { .. },
            ..
        })
    ));

    let hq = h.world().start_hq().expect("start hq");
    assert!(h.world_mut().entities_mut().despawn(hq));

    h.step_exact(1);
    assert_eq!(h.world().order_of(w0), Some(Order::Idle));
    assert!(h.world().entities().carry(w0_slot).is_some());
}

// --- many workers, one node -------------------------------------------------------

#[test]
fn six_workers_on_one_node_all_deliver() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let workers = h.ids_of_kind(EntityKind::Unit(UnitKind::Worker));
    let n_crystal = crystal_node(&h);
    assert_eq!(h.world_mut().order_gather_group(&workers, n_crystal), 6);

    h.step_exact(4_000);
    assert!(
        h.world().resources().crystal >= 300 + 6 * WORKER_CARRY_CAPACITY,
        "expected all six workers to deliver at least once, got {}",
        h.world().resources().crystal
    );
}

// --- reproducibility ---------------------------------------------------------------

#[test]
fn the_economy_is_reproducible() {
    let mut a = RtsHarness::scene().build().expect("a");
    let mut b = RtsHarness::scene().build().expect("b");
    for h in [&mut a, &mut b] {
        let w0 = first_worker(h);
        let n_crystal = crystal_node(h);
        assert!(h.world_mut().order_gather(w0, n_crystal));
        h.step_exact(4_000);
    }
    assert_eq!(a.state_hash(), b.state_hash());
}

#[test]
fn state_hash_sees_the_carried_load() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let before = h.state_hash();
    h.world_mut()
        .entities_mut()
        .set_carry(11, Some((ResourceKind::Gas, 1)));
    assert_ne!(h.state_hash(), before);
}

#[test]
fn state_hash_sees_the_gather_phase() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let w0 = first_worker(&h);
    let n_crystal = crystal_node(&h);
    park_and_order_gather(&mut h, w0, n_crystal);

    h.step_exact(1); // now Mining { ticks_left: GATHER_TICKS }
    let before = h.state_hash();
    h.step_exact(1); // ticks_left decrements by exactly one
    assert_ne!(h.state_hash(), before);
}

// --- system order (mutation #8) -----------------------------------------------------

/// The gather system must run before movement: a phase change decided this
/// tick must also govern movement this same tick, not one tick later.
///
/// Pinned directly: the tick a walking worker's phase flips from `ToNode` to
/// `Mining` must be a tick the worker does not also move on. A gather system
/// that instead ran after movement would use last tick's order to drive this
/// tick's step, so the worker would still take a step on the very tick its
/// phase flips.
#[test]
fn a_worker_that_arrives_starts_mining_the_same_tick() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let w0 = first_worker(&h);
    let n_crystal = crystal_node(&h);
    let w0_slot = h.world().entities().slot(w0).expect("worker slot");
    assert!(h.world_mut().order_gather(w0, n_crystal));

    for _ in 0..2_000 {
        let pos_before = h.world().entities().position(w0_slot);
        h.step_exact(1);
        let pos_after = h.world().entities().position(w0_slot);
        match h.world().order_of(w0) {
            Some(Order::Gather {
                phase: GatherPhase::Mining { .. },
                ..
            }) => {
                assert_eq!(
                    pos_after, pos_before,
                    "the tick a worker starts mining must not also move it"
                );
                return;
            }
            Some(Order::Gather {
                phase: GatherPhase::ToNode { .. },
                ..
            }) => continue,
            other => panic!("unexpected order during approach: {other:?}"),
        }
    }
    panic!("worker never reached the node within 2000 ticks");
}
