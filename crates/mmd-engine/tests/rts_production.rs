//! T11 — production queues bounded by supply, and rally points.
//!
//! Pure logic, no GPU, no clock: every case here is a headless CPU check of
//! `mmd_engine::rts` and `testkit::RtsHarness`.

use mmd_engine::rts::{
    BuildingKind, EntityId, EntityKind, FORMATION_ARRIVAL_CELLS, MAX_ENTITIES, OWNER_ENEMY,
    OWNER_NEUTRAL, OWNER_PLAYER, Order, PRODUCTION_QUEUE_CAP, ProduceError, ProductionQueue,
    RTS_UNIT_BODY_DIAMETER_CELLS, RTS_UNIT_BODY_RADIUS_CELLS, RallyTarget, ResourceKind,
    SOLDIER_COST, SOLDIER_PRODUCE_TICKS, UnitKind, WORKER_COST, WORKER_PRODUCE_TICKS, can_produce,
    produce_ticks, unit_cost, units_overlap,
};
use mmd_engine::scenario::Cell;
use mmd_engine::testkit::{RtsHarness, fixture_path};

/// The enemy-free twin of the tracked scene, for horizons that cross its
/// first wave tick (3000): `fixture_rts_baseline_v1.ron` is a byte-identical
/// copy taken immediately before the combat gate scripted enemies into the
/// scene, so every pinned number below keeps its meaning.
fn baseline_scene() -> RtsHarness {
    RtsHarness::path(fixture_path("fixture_rts_baseline_v1"))
        .build()
        .expect("baseline scene harness")
}

mod common;
use common::{ONE_FREE_CENTRE, one_free_centre_spec};

fn first_worker(h: &RtsHarness) -> EntityId {
    h.ids_of_kind(EntityKind::Unit(UnitKind::Worker))[0]
}

/// Clear, obstacle-free, node-free, HQ-free build-square corners, far enough
/// apart that a Depot (edge 8) and a Barracks (edge 16) placed at each never
/// overlap.
const DEPOT_CORNER: Cell = Cell { x: 144, y: 176 };
const BARRACKS_CORNER: Cell = Cell { x: 144, y: 152 };

/// Place, confirm and fully attend a building until it finishes. Generous on
/// ticks (4 000, well past any of this slice's build times) because the
/// builder must first walk in — and since T7 both corners sit west of the
/// enlarged HQ, which is a longer walk out of the spawn cluster than the old
/// east-side plots were.
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
    h.step_exact(4_000);
    assert!(
        !h.world().is_site(site),
        "building must have finished within 4000 ticks"
    );
    site
}

// --- constants and the build tree -------------------------------------------

#[test]
fn costs_and_times_are_the_published_constants() {
    assert_eq!(unit_cost(UnitKind::Worker), WORKER_COST);
    assert_eq!(
        WORKER_COST,
        mmd_engine::rts::Resources {
            crystal: 50,
            gas: 0
        }
    );
    assert_eq!(unit_cost(UnitKind::Soldier), SOLDIER_COST);
    assert_eq!(
        SOLDIER_COST,
        mmd_engine::rts::Resources {
            crystal: 50,
            gas: 25
        }
    );

    assert_eq!(produce_ticks(UnitKind::Worker), WORKER_PRODUCE_TICKS);
    assert_eq!(WORKER_PRODUCE_TICKS, 300);
    assert_eq!(produce_ticks(UnitKind::Soldier), SOLDIER_PRODUCE_TICKS);
    assert_eq!(SOLDIER_PRODUCE_TICKS, 360);
}

#[test]
fn the_build_tree_is_hq_worker_and_barracks_soldier() {
    use BuildingKind::*;
    use UnitKind::*;
    let pairs = [
        (Hq, Worker, true),
        (Hq, Soldier, false),
        (Depot, Worker, false),
        (Depot, Soldier, false),
        (Barracks, Worker, false),
        (Barracks, Soldier, true),
    ];
    for (b, u, expect) in pairs {
        assert_eq!(can_produce(b, u), expect, "{b:?} -> {u:?}");
    }
}

// --- ProductionQueue, in isolation -------------------------------------------

#[test]
fn queue_push_respects_the_cap() {
    let mut q = ProductionQueue::default();
    for _ in 0..PRODUCTION_QUEUE_CAP {
        assert!(q.push(UnitKind::Worker));
    }
    assert!(q.is_full());
    assert!(!q.push(UnitKind::Worker));
    assert_eq!(q.len(), PRODUCTION_QUEUE_CAP);
}

#[test]
fn queue_cancel_of_the_head_resets_progress() {
    let mut q = ProductionQueue::default();
    assert!(q.push(UnitKind::Worker));
    assert!(q.push(UnitKind::Soldier));
    for _ in 0..100 {
        q.tick_head();
        assert!(!q.head_ready());
    }
    assert_eq!(q.progress(), 100);

    assert_eq!(q.cancel(0), Some(UnitKind::Worker));
    assert_eq!(q.progress(), 0);
    assert_eq!(q.head(), Some(UnitKind::Soldier));
}

#[test]
fn queue_cancel_of_a_tail_entry_keeps_progress() {
    let mut q = ProductionQueue::default();
    assert!(q.push(UnitKind::Worker));
    assert!(q.push(UnitKind::Soldier));
    assert!(q.push(UnitKind::Worker));
    for _ in 0..100 {
        q.tick_head();
        assert!(!q.head_ready());
    }
    assert_eq!(q.progress(), 100);
    let head_before = q.head();

    assert_eq!(q.cancel(2), Some(UnitKind::Worker));
    assert_eq!(q.progress(), 100);
    assert_eq!(q.head(), head_before);
}

#[test]
fn queue_advance_completes_at_the_documented_tick() {
    let mut q = ProductionQueue::default();
    assert!(q.push(UnitKind::Worker));
    for i in 0..299 {
        q.tick_head();
        assert!(!q.head_ready(), "tick {i}");
        assert_eq!(q.pop_ready(), None, "tick {i}");
    }
    q.tick_head();
    assert!(q.head_ready());
    assert_eq!(q.pop_ready(), Some(UnitKind::Worker));
    assert_eq!(q.progress(), 0);
    assert!(q.is_empty());
}

/// A ready head that nothing pops must not run its progress on: it saturates
/// at the unit's build time and stays there, tick after tick, so the queue
/// state a blocked producer hashes is stable.
#[test]
fn queue_progress_saturates_on_a_ready_head() {
    let mut q = ProductionQueue::default();
    assert!(q.push(UnitKind::Worker));
    for _ in 0..(WORKER_PRODUCE_TICKS + 50) {
        q.tick_head();
    }
    assert_eq!(q.progress(), WORKER_PRODUCE_TICKS);
    assert!(q.head_ready());
    assert_eq!(q.len(), 1, "a ready head is not popped by ticking");
}

// --- enqueue_unit rejections --------------------------------------------------

#[test]
fn enqueue_rejects_a_stale_building() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let hq = h.world().start_hq().expect("hq");
    assert!(h.world_mut().entities_mut().despawn(hq));
    assert_eq!(
        h.world_mut().enqueue_unit(hq, UnitKind::Worker),
        Err(ProduceError::NoBuilding)
    );
}

#[test]
fn enqueue_rejects_a_worker_as_the_producer() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let w0 = first_worker(&h);
    assert_eq!(
        h.world_mut().enqueue_unit(w0, UnitKind::Worker),
        Err(ProduceError::NoBuilding)
    );
}

#[test]
fn enqueue_rejects_a_neutral_building() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let barracks = h
        .world_mut()
        .entities_mut()
        .spawn(
            EntityKind::Building(BuildingKind::Barracks),
            OWNER_NEUTRAL,
            [203.0, 181.0],
        )
        .expect("spawn neutral Barracks");
    assert_eq!(
        h.world_mut().enqueue_unit(barracks, UnitKind::Soldier),
        Err(ProduceError::NoBuilding)
    );
}

#[test]
fn enqueue_rejects_a_soldier_at_the_hq() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let hq = h.world().start_hq().expect("hq");
    assert_eq!(
        h.world_mut().enqueue_unit(hq, UnitKind::Soldier),
        Err(ProduceError::WrongBuilding)
    );
}

#[test]
fn enqueue_rejects_a_worker_at_a_barracks() {
    let mut h = baseline_scene();
    let w0 = first_worker(&h);
    h.world_mut().resources_mut().crystal = 10_000;
    h.world_mut().resources_mut().gas = 10_000;
    let barracks = build_and_finish(&mut h, BuildingKind::Barracks, BARRACKS_CORNER, w0);
    assert_eq!(
        h.world_mut().enqueue_unit(barracks, UnitKind::Worker),
        Err(ProduceError::WrongBuilding)
    );
}

#[test]
fn enqueue_rejects_a_site() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let w0 = first_worker(&h);
    h.world_mut().resources_mut().crystal = 10_000;
    h.world_mut().resources_mut().gas = 10_000;
    assert!(h.world_mut().begin_placement(BuildingKind::Barracks));
    let site = h
        .world_mut()
        .confirm_placement(BARRACKS_CORNER, w0)
        .expect("confirm");
    assert!(h.world().is_site(site));
    assert_eq!(
        h.world_mut().enqueue_unit(site, UnitKind::Soldier),
        Err(ProduceError::UnderConstruction)
    );
}

#[test]
fn enqueue_rejects_a_full_queue() {
    let mut h = baseline_scene();
    let hq = h.world().start_hq().expect("hq");
    let workers = h.ids_of_kind(EntityKind::Unit(UnitKind::Worker));
    // Raise the supply cap first: the default cap 10 with 6 already used only
    // has 4 free, not enough for 5 Workers. Building a Depot elsewhere grants
    // +10, which is not this test's subject and is only setup.
    h.world_mut().resources_mut().crystal = 10_000;
    build_and_finish(&mut h, BuildingKind::Depot, DEPOT_CORNER, workers[0]);
    h.world_mut().resources_mut().crystal = 10_000;

    for _ in 0..PRODUCTION_QUEUE_CAP {
        assert!(h.world_mut().enqueue_unit(hq, UnitKind::Worker).is_ok());
    }
    assert_eq!(
        h.world_mut().enqueue_unit(hq, UnitKind::Worker),
        Err(ProduceError::QueueFull)
    );
}

fn queue_four_workers_at_the_supply_edge(h: &mut RtsHarness) -> EntityId {
    let hq = h.world().start_hq().expect("hq");
    assert_eq!(h.world().supply().cap(), 10);
    assert_eq!(h.world().supply().used(), 6);
    for _ in 0..4 {
        assert!(h.world_mut().enqueue_unit(hq, UnitKind::Worker).is_ok());
    }
    assert_eq!(h.world().supply().used(), 10);
    hq
}

#[test]
fn enqueue_rejects_over_supply() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let hq = queue_four_workers_at_the_supply_edge(&mut h);
    assert_eq!(
        h.world_mut().enqueue_unit(hq, UnitKind::Worker),
        Err(ProduceError::SupplyBlocked)
    );
}

#[test]
fn a_supply_blocked_enqueue_does_not_charge() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let hq = queue_four_workers_at_the_supply_edge(&mut h);
    let before = h.world().resources();
    assert_eq!(
        h.world_mut().enqueue_unit(hq, UnitKind::Worker),
        Err(ProduceError::SupplyBlocked)
    );
    assert_eq!(h.world().resources(), before);
}

#[test]
fn enqueue_rejects_when_unaffordable() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let hq = h.world().start_hq().expect("hq");
    h.world_mut().resources_mut().crystal = 10;
    h.world_mut().resources_mut().gas = 0;
    let supply_before = h.world().supply();
    assert_eq!(
        h.world_mut().enqueue_unit(hq, UnitKind::Worker),
        Err(ProduceError::Unaffordable)
    );
    assert_eq!(h.world().supply(), supply_before);
}

// --- enqueue_unit success ------------------------------------------------------

#[test]
fn enqueue_debits_immediately() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let hq = h.world().start_hq().expect("hq");
    assert_eq!(h.world().resources().crystal, 300);
    assert!(h.world_mut().enqueue_unit(hq, UnitKind::Worker).is_ok());
    assert_eq!(h.world().resources().crystal, 250);
}

#[test]
fn enqueue_reserves_supply_immediately() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let hq = h.world().start_hq().expect("hq");
    assert!(h.world_mut().enqueue_unit(hq, UnitKind::Worker).is_ok());
    assert_eq!(h.world().supply().used(), 7);
}

#[test]
fn queueing_cannot_exceed_the_cap() {
    let mut h = baseline_scene();
    let w0 = first_worker(&h);
    assert_eq!(h.world().supply().cap(), 10);
    assert_eq!(h.world().supply().used(), 6);
    let barracks = build_and_finish(&mut h, BuildingKind::Barracks, BARRACKS_CORNER, w0);

    let mut accepted = 0;
    for _ in 0..8 {
        if h.world_mut()
            .enqueue_unit(barracks, UnitKind::Soldier)
            .is_ok()
        {
            accepted += 1;
        }
    }
    assert_eq!(accepted, 2, "2 x 2 == 4, the free supply at cap 10 used 6");
}

#[test]
fn cancel_queued_refunds_and_releases() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let hq = h.world().start_hq().expect("hq");
    assert!(h.world_mut().enqueue_unit(hq, UnitKind::Worker).is_ok());
    assert!(h.world_mut().cancel_queued(hq, 0));
    assert_eq!(h.world().resources().crystal, 300);
    assert_eq!(h.world().supply().used(), 6);
}

#[test]
fn cancel_queued_rejects_a_bad_index() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let hq = h.world().start_hq().expect("hq");
    assert!(!h.world_mut().cancel_queued(hq, 0));
}

// --- the production system ----------------------------------------------------

#[test]
fn a_worker_appears_after_its_build_time() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let hq = h.world().start_hq().expect("hq");
    let before = h.ids_of_kind(EntityKind::Unit(UnitKind::Worker)).len();
    assert!(h.world_mut().enqueue_unit(hq, UnitKind::Worker).is_ok());
    h.step_exact(WORKER_PRODUCE_TICKS as u64);
    let after = h.ids_of_kind(EntityKind::Unit(UnitKind::Worker)).len();
    assert_eq!(after, before + 1);
    assert_eq!(
        h.world().supply().used(),
        7,
        "completion must replace one reservation with one live Worker in the same recount"
    );
}

#[test]
fn a_produced_unit_spawns_beside_its_building() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let hq = h.world().start_hq().expect("hq");
    let before = h.ids_of_kind(EntityKind::Unit(UnitKind::Worker));
    assert!(h.world_mut().enqueue_unit(hq, UnitKind::Worker).is_ok());
    h.step_exact(WORKER_PRODUCE_TICKS as u64);
    let after = h.ids_of_kind(EntityKind::Unit(UnitKind::Worker));
    let new_id = *after
        .iter()
        .find(|id| !before.contains(id))
        .expect("a new worker exists");

    let slot = h.world().entities().slot(new_id).expect("live");
    let pos = h.world().entities().position(slot);
    let cell = Cell {
        x: pos[0].floor() as u32,
        y: pos[1].floor() as u32,
    };

    // The tracked scene's HQ footprint is [160, 172) x [160, 172).
    let inside = cell.x >= 160 && cell.x < 172 && cell.y >= 160 && cell.y < 172;
    assert!(
        !inside,
        "must not spawn inside the HQ footprint, got {cell:?}"
    );
    // T3: the approach cell is a *legal* (radius-clear) centre, not
    // necessarily one cell off the footprint — dense terrain can push it
    // several cells out. Generously bounded rather than pinned to a single
    // ring.
    let near = cell.x + 10 >= 160 && cell.x <= 172 + 10 && cell.y + 10 >= 160 && cell.y <= 172 + 10;
    assert!(near, "must spawn near the HQ footprint, got {cell:?}");

    let width = h.world().scenario().width();
    let blocked = h.world().nav().blocked();
    assert!(!blocked[(cell.x + cell.y * width) as usize]);
}

#[test]
fn a_produced_unit_is_idle_without_a_rally() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let hq = h.world().start_hq().expect("hq");
    let before = h.ids_of_kind(EntityKind::Unit(UnitKind::Worker));
    assert!(h.world_mut().enqueue_unit(hq, UnitKind::Worker).is_ok());
    h.step_exact(WORKER_PRODUCE_TICKS as u64);
    let after = h.ids_of_kind(EntityKind::Unit(UnitKind::Worker));
    let new_id = *after
        .iter()
        .find(|id| !before.contains(id))
        .expect("a new worker exists");
    assert_eq!(h.world().order_of(new_id), Some(Order::Idle));
}

#[test]
fn a_produced_unit_walks_to_the_rally() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let hq = h.world().start_hq().expect("hq");
    let before = h.ids_of_kind(EntityKind::Unit(UnitKind::Worker));
    assert!(
        h.world_mut()
            .set_rally(hq, Some(RallyTarget::Cell(Cell { x: 200, y: 200 })))
    );
    assert!(h.world_mut().enqueue_unit(hq, UnitKind::Worker).is_ok());
    h.step_exact(1_200);
    let after = h.ids_of_kind(EntityKind::Unit(UnitKind::Worker));
    let new_id = *after
        .iter()
        .find(|id| !before.contains(id))
        .expect("a new worker exists");
    let slot = h.world().entities().slot(new_id).expect("live");
    let pos = h.world().entities().position(slot);
    let dx = pos[0] - 200.5;
    let dy = pos[1] - 200.5;
    assert!(
        (dx * dx + dy * dy).sqrt() <= FORMATION_ARRIVAL_CELLS,
        "unit ended at {pos:?}, expected near (200.5, 200.5)"
    );
}

#[test]
fn rally_defaults_to_none_and_can_be_cleared() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let hq = h.world().start_hq().expect("hq");
    assert_eq!(h.world().rally(hq), None);
    assert!(
        h.world_mut()
            .set_rally(hq, Some(RallyTarget::Cell(Cell { x: 200, y: 200 })))
    );
    assert_eq!(
        h.world().rally(hq),
        Some(RallyTarget::Cell(Cell { x: 200, y: 200 }))
    );
    assert!(h.world_mut().set_rally(hq, None));
    assert_eq!(h.world().rally(hq), None);
}

#[test]
fn set_rally_rejects_a_blocked_cell() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let hq = h.world().start_hq().expect("hq");
    let valid = Cell { x: 200, y: 200 };
    assert!(h.world_mut().set_rally(hq, Some(RallyTarget::Cell(valid))));
    // Obstacle index 0 of the tracked scene is cell (0, 0) (see rts_build.rs).
    assert!(
        !h.world_mut()
            .set_rally(hq, Some(RallyTarget::Cell(Cell { x: 0, y: 0 })))
    );
    assert_eq!(h.world().rally(hq), Some(RallyTarget::Cell(valid)));
}

#[test]
fn set_rally_rejects_an_out_of_bounds_cell() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let hq = h.world().start_hq().expect("hq");
    let width = h.world().scenario().width();
    assert!(
        !h.world_mut()
            .set_rally(hq, Some(RallyTarget::Cell(Cell { x: width, y: 0 })))
    );
    assert_eq!(h.world().rally(hq), None);
}

#[test]
fn set_rally_rejects_a_non_building() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let w0 = first_worker(&h);
    assert!(
        !h.world_mut()
            .set_rally(w0, Some(RallyTarget::Cell(Cell { x: 200, y: 200 })))
    );
}

// --- supply recount -------------------------------------------------------------

#[test]
fn supply_used_is_recomputed_not_incremented() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    assert_eq!(h.world().supply().used(), 6);
    let w0 = first_worker(&h);
    assert!(h.world_mut().entities_mut().despawn(w0));
    h.step_exact(1);
    assert_eq!(h.world().supply().used(), 5);
}

#[test]
fn supply_used_counts_reservations() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let hq = h.world().start_hq().expect("hq");
    assert!(h.world_mut().enqueue_unit(hq, UnitKind::Worker).is_ok());
    assert!(h.world_mut().enqueue_unit(hq, UnitKind::Worker).is_ok());
    h.step_exact(1);
    assert_eq!(h.world().supply().used(), 6 + 2);
}

#[test]
fn reserved_supply_reports_the_queues() {
    let mut h = baseline_scene();
    let w2 = h.ids_of_kind(EntityKind::Unit(UnitKind::Worker))[0];
    let barracks = build_and_finish(&mut h, BuildingKind::Barracks, BARRACKS_CORNER, w2);
    let hq = h.world().start_hq().expect("hq");
    assert!(h.world_mut().enqueue_unit(hq, UnitKind::Worker).is_ok());
    assert!(h.world_mut().enqueue_unit(hq, UnitKind::Worker).is_ok());
    assert!(
        h.world_mut()
            .enqueue_unit(barracks, UnitKind::Soldier)
            .is_ok()
    );
    assert_eq!(h.world().reserved_supply(), 1 + 1 + 2);
}

#[test]
fn a_barracks_produces_a_soldier() {
    let mut h = baseline_scene();
    let w0 = first_worker(&h);
    let barracks = build_and_finish(&mut h, BuildingKind::Barracks, BARRACKS_CORNER, w0);
    // Reset gas after the Barracks's own construction cost so the assertion
    // below isolates the Soldier's cost alone.
    h.world_mut().resources_mut().gas = 100;
    assert!(
        h.world_mut()
            .enqueue_unit(barracks, UnitKind::Soldier)
            .is_ok()
    );
    let before = h.ids_of_kind(EntityKind::Unit(UnitKind::Soldier)).len();
    h.step_exact(SOLDIER_PRODUCE_TICKS as u64);
    let after = h.ids_of_kind(EntityKind::Unit(UnitKind::Soldier)).len();
    assert_eq!(after, before + 1);
    assert_eq!(h.world().resources().gas, 75);
}

#[test]
fn production_stops_when_the_store_is_full() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let hq = h.world().start_hq().expect("hq");
    h.world_mut().resources_mut().crystal = 10_000;
    assert!(h.world_mut().enqueue_unit(hq, UnitKind::Worker).is_ok());
    let spent = h.world().resources().crystal;

    while h.world().entities().len() < MAX_ENTITIES {
        let ok = h
            .world_mut()
            .entities_mut()
            .spawn(
                EntityKind::Node(ResourceKind::Crystal),
                OWNER_NEUTRAL,
                [0.5, 0.5],
            )
            .is_some();
        if !ok {
            break;
        }
    }
    assert_eq!(h.world().entities().len(), MAX_ENTITIES);

    h.step_exact(WORKER_PRODUCE_TICKS as u64);

    let q = h.world().production_queue(hq).expect("hq queue");
    assert_eq!(q.len(), 1, "the entry must be put back, not lost");
    assert_eq!(
        h.world().resources().crystal,
        spent,
        "the debit is not refunded on a store-full stall"
    );
}

/// Reordering pin: construction must finish a building before production sees
/// it in the same tick. Testkit re-marks a finished, queued Barracks as one tick
/// from completion because public enqueue correctly rejects ordinary sites.
#[test]
fn a_barracks_can_be_queued_the_tick_it_finishes() {
    let mut h = baseline_scene();
    let w0 = first_worker(&h);
    h.world_mut().resources_mut().crystal = 10_000;
    h.world_mut().resources_mut().gas = 10_000;
    let barracks = build_and_finish(&mut h, BuildingKind::Barracks, BARRACKS_CORNER, w0);
    assert!(
        h.world_mut()
            .enqueue_unit(barracks, UnitKind::Soldier)
            .is_ok()
    );

    let slot = h.world().entities().slot(barracks).expect("barracks slot");
    h.world_mut().entities_mut().set_progress(slot, 0, 1);
    // `w0`'s resting position from the original walk satisfied the *site's*
    // adaptive reach at the moment it finished — computed against the
    // pre-finish mask, one tick before the Barracks's own footprint became a
    // solid that mask reads. Re-marking it a site here does not un-stamp
    // that footprint (a real completion never un-stamps), so every
    // recomputation of its approach cell from now on scores against the
    // wider post-finish mask, and `w0`'s old resting spot is not guaranteed
    // to still be inside it. Standing `w0` on the footprint boundary itself
    // (`rect_distance == 0`) sidesteps that mask-timing gap outright, which
    // is what this test needs to isolate — the reordering pin between
    // construction and production, not the approach-cell search.
    let worker_slot = h.world().entities().slot(w0).expect("worker slot");
    h.world_mut()
        .entities_mut()
        .set_position(worker_slot, [141.0, 160.0]);
    assert!(h.world_mut().order_build(w0, barracks));
    h.step_exact(1);

    assert!(!h.world().is_site(barracks));
    assert_eq!(
        h.world()
            .production_queue(barracks)
            .expect("barracks queue")
            .progress(),
        1,
        "production must advance after construction finishes the Barracks"
    );
}

// --- despawn hook ---------------------------------------------------------------

#[test]
fn cancelling_a_site_clears_its_queue_slot() {
    // `enqueue_unit` refuses an unfinished site (`UnderConstruction`), so a
    // site's production *queue* can never actually hold an entry to lose —
    // but its rally point can be set pre-finish (`set_rally`'s contract only
    // checks "is a building", not "is finished"), so that is the populated
    // side of `ProductionTable` this test can legitimately exercise. It still
    // exercises exactly the call under test: `production.clear(slot)` inside
    // `cancel_construction`, which resets both the queue and the rally
    // together.
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let w0 = first_worker(&h);
    h.world_mut().resources_mut().crystal = 10_000;
    h.world_mut().resources_mut().gas = 10_000;
    assert!(h.world_mut().begin_placement(BuildingKind::Barracks));
    let site = h
        .world_mut()
        .confirm_placement(BARRACKS_CORNER, w0)
        .expect("confirm");
    assert!(h.world().is_site(site));
    assert_eq!(
        h.world_mut().enqueue_unit(site, UnitKind::Soldier),
        Err(ProduceError::UnderConstruction)
    );
    assert!(
        h.world_mut()
            .set_rally(site, Some(RallyTarget::Cell(Cell { x: 200, y: 200 })))
    );
    assert_eq!(
        h.world().rally(site),
        Some(RallyTarget::Cell(Cell { x: 200, y: 200 }))
    );

    assert!(h.world_mut().cancel_construction(site));

    // The free list is LIFO: the very next building spawned lands in the slot
    // `site` just vacated.
    let w1 = h.ids_of_kind(EntityKind::Unit(UnitKind::Worker))[1];
    assert!(h.world_mut().begin_placement(BuildingKind::Depot));
    let new_building = h
        .world_mut()
        .confirm_placement(DEPOT_CORNER, w1)
        .expect("confirm the replacement building");
    assert_eq!(
        new_building.index, site.index,
        "must land in the reused slot"
    );

    let q = h.world().production_queue(new_building).expect("queue");
    assert!(q.is_empty());
    assert_eq!(
        h.world().rally(new_building),
        None,
        "a reused slot must not inherit the cancelled site's rally point"
    );
}

// --- reproducibility --------------------------------------------------------------

#[test]
fn production_is_reproducible() {
    let mut a = RtsHarness::scene().build().expect("a");
    let mut b = RtsHarness::scene().build().expect("b");
    for h in [&mut a, &mut b] {
        let hq = h.world().start_hq().expect("hq");
        assert!(
            h.world_mut()
                .set_rally(hq, Some(RallyTarget::Cell(Cell { x: 200, y: 200 })))
        );
        assert!(h.world_mut().enqueue_unit(hq, UnitKind::Worker).is_ok());
        h.step_exact(3_000);
    }
    assert_eq!(a.state_hash(), b.state_hash());
}

#[test]
fn state_hash_sees_a_queue_entry() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let hq = h.world().start_hq().expect("hq");
    let before = h.state_hash();
    assert!(h.world_mut().enqueue_unit(hq, UnitKind::Worker).is_ok());
    assert_ne!(h.state_hash(), before);
}

#[test]
fn state_hash_sees_a_rally_point() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let hq = h.world().start_hq().expect("hq");
    let before = h.state_hash();
    assert!(
        h.world_mut()
            .set_rally(hq, Some(RallyTarget::Cell(Cell { x: 200, y: 200 })))
    );
    assert_ne!(h.state_hash(), before);
}

// --- T6: body-safe production ---------------------------------------------------

/// Every live unit body on the grid, ascending slot.
fn unit_positions(h: &RtsHarness) -> Vec<[f32; 2]> {
    let mut out = Vec::new();
    let store = h.world().entities();
    for slot in 0..store.slot_count() {
        if store.alive(slot) && matches!(store.kind(slot), EntityKind::Unit(_)) {
            out.push(store.position(slot));
        }
    }
    out
}

/// Independent oracle for the spawn rule: the legal body centre nearest
/// `preferred` that no live body already covers, ties broken by the lower flat
/// cell index. Brute force over the whole grid, from public state only.
fn nearest_free_centre(h: &RtsHarness, preferred: [f32; 2]) -> Option<[f32; 2]> {
    let width = h.world().scenario().width();
    let height = h.world().scenario().height();
    let blocked = h.world().static_nav().center_blocked();
    let bodies = unit_positions(h);
    let diam2 = RTS_UNIT_BODY_DIAMETER_CELLS * RTS_UNIT_BODY_DIAMETER_CELLS;
    let mut best: Option<(f32, u32)> = None;
    for y in 0..height {
        for x in 0..width {
            let idx = x + y * width;
            if blocked[idx as usize] {
                continue;
            }
            let p = [x as f32 + 0.5, y as f32 + 0.5];
            if bodies.iter().any(|&q| {
                let dx = p[0] - q[0];
                let dy = p[1] - q[1];
                dx * dx + dy * dy < diam2
            }) {
                continue;
            }
            let d = {
                let dx = p[0] - preferred[0];
                let dy = p[1] - preferred[1];
                dx * dx + dy * dy
            };
            if best.is_none_or(|(bd, bi)| d < bd || (d == bd && idx < bi)) {
                best = Some((d, idx));
            }
        }
    }
    best.map(|(_, idx)| [(idx % width) as f32 + 0.5, (idx / width) as f32 + 0.5])
}

fn clear_the_starting_workers(h: &mut RtsHarness) {
    for w in h.ids_of_kind(EntityKind::Unit(UnitKind::Worker)) {
        assert!(h.world_mut().entities_mut().despawn(w));
    }
}

fn produce_one_worker(h: &mut RtsHarness) -> EntityId {
    let hq = h.world().start_hq().expect("hq");
    let before = h.ids_of_kind(EntityKind::Unit(UnitKind::Worker));
    assert!(h.world_mut().enqueue_unit(hq, UnitKind::Worker).is_ok());
    h.step_exact(WORKER_PRODUCE_TICKS as u64);
    let after = h.ids_of_kind(EntityKind::Unit(UnitKind::Worker));
    *after
        .iter()
        .find(|id| !before.contains(id))
        .expect("a new worker exists")
}

fn position_of(h: &RtsHarness, id: EntityId) -> [f32; 2] {
    let slot = h.world().entities().slot(id).expect("live entity");
    h.world().entities().position(slot)
}

/// With the base empty, a produced unit stands exactly on the HQ's preferred
/// approach centre. Park a body there instead and the next unit must take the
/// nearest free legal centre to it — the exact cell the brute-force oracle
/// names, not "somewhere near", and not on top of the body already standing
/// there.
#[test]
fn production_uses_nearest_free_body_position() {
    // 1. The preferred centre, observed on an empty base.
    let mut control = RtsHarness::scene().build().expect("rts scene harness");
    clear_the_starting_workers(&mut control);
    let first = produce_one_worker(&mut control);
    let preferred = position_of(&control, first);

    // 2. The same production, with that exact centre occupied.
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    clear_the_starting_workers(&mut h);
    let blocker = h
        .world_mut()
        .entities_mut()
        .spawn(EntityKind::Unit(UnitKind::Worker), OWNER_PLAYER, preferred)
        .expect("park a body on the preferred centre");
    let hq = h.world().start_hq().expect("hq");
    assert!(h.world_mut().enqueue_unit(hq, UnitKind::Worker).is_ok());
    h.step_exact(WORKER_PRODUCE_TICKS as u64 - 1);
    // The oracle is computed one tick before the spawn, from the same state
    // the production system will see.
    let expected = nearest_free_centre(&h, preferred).expect("the grid has a free legal centre");
    h.step_exact(1);

    let produced = *h
        .ids_of_kind(EntityKind::Unit(UnitKind::Worker))
        .iter()
        .find(|&&id| id != blocker)
        .expect("the queued worker was produced");
    assert_eq!(
        position_of(&h, produced),
        expected,
        "the produced unit did not take the nearest free legal centre"
    );
    assert_ne!(position_of(&h, produced), preferred);
    assert!(
        !units_overlap(
            position_of(&h, produced),
            RTS_UNIT_BODY_RADIUS_CELLS,
            preferred,
            RTS_UNIT_BODY_RADIUS_CELLS
        ),
        "the produced body merged into the parked one"
    );
}

/// A grid with exactly one legal body centre, and a worker standing on it: the
/// finished head has nowhere to go, so it stays ready, paid for and reserved,
/// and no unit appears.
#[test]
fn production_waits_when_no_spawn_is_free() {
    let mut h = RtsHarness::spec(one_free_centre_spec())
        .build()
        .expect("one-free-centre scene");
    assert_eq!(
        h.world()
            .static_nav()
            .center_blocked()
            .iter()
            .filter(|b| !**b)
            .count(),
        1,
        "the scene must hold exactly one legal body centre"
    );
    assert_eq!(h.ids_of_kind(EntityKind::Unit(UnitKind::Worker)).len(), 1);

    let hq = h.world().start_hq().expect("hq");
    assert!(h.world_mut().enqueue_unit(hq, UnitKind::Worker).is_ok());
    let paid = h.world().resources();
    let reserved = h.world().reserved_supply();

    h.step_exact(WORKER_PRODUCE_TICKS as u64 + 300);

    assert_eq!(
        h.ids_of_kind(EntityKind::Unit(UnitKind::Worker)).len(),
        1,
        "a unit was produced with nowhere legal to stand"
    );
    let q = h.world().production_queue(hq).expect("hq queue");
    assert_eq!(q.len(), 1, "the paid entry must stay in the queue");
    assert!(q.head_ready(), "the head must be ready and waiting");
    assert_eq!(q.progress(), WORKER_PRODUCE_TICKS, "progress must saturate");
    assert_eq!(h.world().resources(), paid, "a waiting head charged again");
    assert_eq!(
        h.world().reserved_supply(),
        reserved,
        "a waiting head lost or doubled its supply reservation"
    );
    assert_eq!(
        h.world().supply().used(),
        2,
        "one live worker plus one reservation"
    );
}

/// The same wait, released: free the one legal centre and the head spawns
/// exactly once, on the tick after it became free, without charging twice.
#[test]
fn waiting_production_resumes_once() {
    let mut h = RtsHarness::spec(one_free_centre_spec())
        .build()
        .expect("one-free-centre scene");
    let hq = h.world().start_hq().expect("hq");
    assert!(h.world_mut().enqueue_unit(hq, UnitKind::Worker).is_ok());
    let paid = h.world().resources();
    h.step_exact(WORKER_PRODUCE_TICKS as u64 + 60);
    assert_eq!(h.ids_of_kind(EntityKind::Unit(UnitKind::Worker)).len(), 1);

    let squatter = h.ids_of_kind(EntityKind::Unit(UnitKind::Worker))[0];
    assert!(h.world_mut().entities_mut().despawn(squatter));
    h.step_exact(1);

    let workers = h.ids_of_kind(EntityKind::Unit(UnitKind::Worker));
    assert_eq!(workers.len(), 1, "exactly one unit must be produced");
    assert_eq!(
        position_of(&h, workers[0]),
        ONE_FREE_CENTRE,
        "the released unit must take the freed centre"
    );
    assert!(
        h.world().production_queue(hq).expect("hq queue").is_empty(),
        "the head must be popped exactly once"
    );
    assert_eq!(h.world().resources(), paid, "the resume charged again");

    h.step_exact(120);
    assert_eq!(
        h.ids_of_kind(EntityKind::Unit(UnitKind::Worker)).len(),
        1,
        "a second unit appeared from a queue that held one entry"
    );
    assert_eq!(
        h.world().supply().used(),
        1,
        "one live worker, no reservation"
    );
}

/// A unit produced this tick is a body for the rest of this tick: the whole
/// base is ordered onto one cell, so the movement sweep is contended, and the
/// tick still ends with no two bodies merged.
#[test]
fn new_spawn_joins_same_tick_collision() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let hq = h.world().start_hq().expect("hq");
    let workers = h.ids_of_kind(EntityKind::Unit(UnitKind::Worker));
    assert_eq!(workers.len(), 6);
    assert!(
        h.world_mut()
            .set_rally(hq, Some(RallyTarget::Cell(Cell { x: 176, y: 192 })))
    );
    assert!(
        h.world_mut()
            .order_move_group(&workers, Cell { x: 176, y: 192 })
            .is_ok()
    );
    assert!(h.world_mut().enqueue_unit(hq, UnitKind::Worker).is_ok());

    for _ in 0..(WORKER_PRODUCE_TICKS as u64 + 60) {
        h.step_exact(1);
        let bodies = unit_positions(&h);
        for (i, a) in bodies.iter().enumerate() {
            for b in &bodies[i + 1..] {
                assert!(
                    !units_overlap(
                        *a,
                        RTS_UNIT_BODY_RADIUS_CELLS,
                        *b,
                        RTS_UNIT_BODY_RADIUS_CELLS
                    ),
                    "tick {} ended with merged bodies {a:?} and {b:?}",
                    h.tick_index()
                );
            }
        }
    }
    assert_eq!(
        h.ids_of_kind(EntityKind::Unit(UnitKind::Worker)).len(),
        7,
        "the queued worker never appeared"
    );
}

/// Production places a finished unit itself, on the nearest free legal body
/// centre, so a producing run never needs the overlap-repair pass — which the
/// shipping build does not compile at all.
#[test]
fn production_never_runs_overlap_repair() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let hq = h.world().start_hq().expect("hq");
    assert_eq!(h.world().body_overlap_count(), 0, "the scene starts clean");
    assert_eq!(
        h.world().overlap_repair_runs(),
        0,
        "seeding must not need the repair pass"
    );
    assert!(h.world_mut().enqueue_unit(hq, UnitKind::Worker).is_ok());
    assert!(h.world_mut().enqueue_unit(hq, UnitKind::Worker).is_ok());

    for tick in 1..=(2 * WORKER_PRODUCE_TICKS as u64 + 240) {
        h.step_exact(1);
        assert_eq!(
            h.world().body_overlap_count(),
            0,
            "tick {tick} ended with a merged pair"
        );
        assert_eq!(
            h.world().overlap_repair_runs(),
            0,
            "tick {tick} ran the overlap repair pass on the production path"
        );
    }
    assert_eq!(
        h.ids_of_kind(EntityKind::Unit(UnitKind::Worker)).len(),
        8,
        "both queued workers must have been produced, or this run proved nothing"
    );
}

// --- T11: rally onto an entity ----------------------------------------------

/// The one unit `before` did not hold — the unit this production run made.
fn newly_produced(h: &RtsHarness, kind: UnitKind, before: &[EntityId]) -> EntityId {
    *h.ids_of_kind(EntityKind::Unit(kind))
        .iter()
        .find(|id| !before.contains(id))
        .expect("production made a new unit")
}

#[test]
fn rally_onto_a_cell_still_moves() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let hq = h.world().start_hq().expect("hq");
    let before = h.ids_of_kind(EntityKind::Unit(UnitKind::Worker));
    assert!(
        h.world_mut()
            .set_rally(hq, Some(RallyTarget::Cell(Cell { x: 200, y: 200 })))
    );
    assert!(h.world_mut().enqueue_unit(hq, UnitKind::Worker).is_ok());
    h.step_exact(WORKER_PRODUCE_TICKS as u64 + 1);

    let new_id = newly_produced(&h, UnitKind::Worker, &before);
    assert!(
        matches!(h.world().order_of(new_id), Some(Order::Move { .. })),
        "a cell rally is still a Move, got {:?}",
        h.world().order_of(new_id)
    );
}

#[test]
fn rally_onto_a_node_makes_produced_workers_gather() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let hq = h.world().start_hq().expect("hq");
    let node = h.ids_of_kind(EntityKind::Node(ResourceKind::Crystal))[0];
    let before = h.ids_of_kind(EntityKind::Unit(UnitKind::Worker));

    assert!(h.world_mut().set_rally(hq, Some(RallyTarget::Entity(node))));
    assert_eq!(h.world().rally(hq), Some(RallyTarget::Entity(node)));
    assert!(h.world_mut().enqueue_unit(hq, UnitKind::Worker).is_ok());
    h.step_exact(WORKER_PRODUCE_TICKS as u64 + 1);

    let new_id = newly_produced(&h, UnitKind::Worker, &before);
    assert!(
        matches!(h.world().order_of(new_id), Some(Order::Gather { node: n, .. }) if n == node),
        "rallying to a node must gather it, got {:?}",
        h.world().order_of(new_id)
    );
}

#[test]
fn rally_onto_a_unit_makes_produced_units_follow() {
    let mut h = baseline_scene();
    let w0 = first_worker(&h);
    let barracks = build_and_finish(&mut h, BuildingKind::Barracks, BARRACKS_CORNER, w0);
    h.world_mut().resources_mut().gas = 100;

    assert!(
        h.world_mut()
            .set_rally(barracks, Some(RallyTarget::Entity(w0)))
    );
    let before = h.ids_of_kind(EntityKind::Unit(UnitKind::Soldier));
    assert!(
        h.world_mut()
            .enqueue_unit(barracks, UnitKind::Soldier)
            .is_ok()
    );
    h.step_exact(SOLDIER_PRODUCE_TICKS as u64 + 1);

    let new_id = newly_produced(&h, UnitKind::Soldier, &before);
    assert!(
        matches!(h.world().order_of(new_id), Some(Order::Follow { target, .. }) if target == w0),
        "rallying to a unit must follow it, got {:?}",
        h.world().order_of(new_id)
    );
}

#[test]
fn rally_rejects_an_enemy_target() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let hq = h.world().start_hq().expect("hq");
    let valid = RallyTarget::Cell(Cell { x: 200, y: 200 });
    assert!(h.world_mut().set_rally(hq, Some(valid)));

    let ghoul = h
        .world_mut()
        .entities_mut()
        .spawn(
            EntityKind::Unit(UnitKind::Ghoul),
            OWNER_ENEMY,
            [200.5, 200.5],
        )
        .expect("spawn a ghoul");
    assert!(
        !h.world_mut()
            .set_rally(hq, Some(RallyTarget::Entity(ghoul))),
        "an enemy is not a rally point"
    );
    assert_eq!(
        h.world().rally(hq),
        Some(valid),
        "a rejected rally must leave the old one exactly as it was"
    );

    // A stale id is refused on the same rule.
    let worker = first_worker(&h);
    assert!(h.world_mut().entities_mut().despawn(worker));
    assert!(
        !h.world_mut()
            .set_rally(hq, Some(RallyTarget::Entity(worker)))
    );
    assert_eq!(h.world().rally(hq), Some(valid));
}

#[test]
fn a_stale_entity_rally_is_dropped() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let hq = h.world().start_hq().expect("hq");
    let target = h.ids_of_kind(EntityKind::Unit(UnitKind::Worker))[1];
    assert!(
        h.world_mut()
            .set_rally(hq, Some(RallyTarget::Entity(target)))
    );

    // The rally target dies after the rally was accepted.
    assert!(h.world_mut().entities_mut().despawn(target));
    let before = h.ids_of_kind(EntityKind::Unit(UnitKind::Worker));
    assert!(h.world_mut().enqueue_unit(hq, UnitKind::Worker).is_ok());
    h.step_exact(WORKER_PRODUCE_TICKS as u64 + 1);

    let new_id = newly_produced(&h, UnitKind::Worker, &before);
    assert_eq!(
        h.world().order_of(new_id),
        Some(Order::Idle),
        "a stale rally is a no-op hand-off, not a crash and not a stray order"
    );
    assert_eq!(
        h.world().rally(hq),
        Some(RallyTarget::Entity(target)),
        "the stored rally is untouched; only the hand-off declines it"
    );
}

#[test]
fn cell_and_entity_rallies_hash_differently() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let hq = h.world().start_hq().expect("hq");
    let worker = first_worker(&h);

    assert!(
        h.world_mut()
            .set_rally(hq, Some(RallyTarget::Cell(Cell { x: 200, y: 200 })))
    );
    let as_cell = h.state_hash();
    assert!(
        h.world_mut()
            .set_rally(hq, Some(RallyTarget::Entity(worker)))
    );
    let as_entity = h.state_hash();
    assert_ne!(
        as_cell, as_entity,
        "a cell rally and an entity rally must never hash alike"
    );
}
