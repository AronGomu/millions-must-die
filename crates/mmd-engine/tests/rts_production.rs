//! T11 — production queues bounded by supply, and rally points.
//!
//! Pure logic, no GPU, no clock: every case here is a headless CPU check of
//! `mmd_engine::rts` and `testkit::RtsHarness`.

use mmd_engine::rts::{
    ARRIVAL_RADIUS_CELLS, BuildingKind, EntityId, EntityKind, MAX_ENTITIES, OWNER_NEUTRAL, Order,
    PRODUCTION_QUEUE_CAP, ProduceError, ProductionQueue, ResourceKind, SOLDIER_COST,
    SOLDIER_PRODUCE_TICKS, UnitKind, WORKER_COST, WORKER_PRODUCE_TICKS, can_produce, produce_ticks,
    unit_cost,
};
use mmd_engine::scenario::Cell;
use mmd_engine::testkit::RtsHarness;

fn first_worker(h: &RtsHarness) -> EntityId {
    h.ids_of_kind(EntityKind::Unit(UnitKind::Worker))[0]
}

/// Clear, obstacle-free, node-free, HQ-free corners, far enough apart that a
/// Depot (edge 8) and a Barracks (edge 10) placed at each never overlap.
const DEPOT_CORNER: Cell = Cell { x: 180, y: 176 };
const BARRACKS_CORNER: Cell = Cell { x: 198, y: 176 };

/// Place, confirm and fully attend a building until it finishes. Generous on
/// ticks (2 000, well past any of this slice's build times) because the
/// builder must first walk in.
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
        assert_eq!(q.advance(), None);
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
        assert_eq!(q.advance(), None);
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
        assert_eq!(q.advance(), None, "tick {i}");
    }
    assert_eq!(q.advance(), Some(UnitKind::Worker));
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
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
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
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let hq = h.world().start_hq().expect("hq");
    let workers = h.ids_of_kind(EntityKind::Unit(UnitKind::Worker));
    // Raise the supply cap first: the default cap 10 with 6 already used only
    // has 4 free, not enough for 5 Workers. Building a Depot elsewhere grants
    // +10, which is not this test's subject and is only setup.
    h.world_mut().resources_mut().crystal = 10_000;
    build_and_finish(&mut h, BuildingKind::Depot, DEPOT_CORNER, workers[1]);
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
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
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
    let adjacent = cell.x >= 159 && cell.x <= 172 && cell.y >= 159 && cell.y <= 172;
    assert!(adjacent, "must spawn beside the HQ footprint, got {cell:?}");

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
    assert!(h.world_mut().set_rally(hq, Some(Cell { x: 200, y: 200 })));
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
        (dx * dx + dy * dy).sqrt() <= ARRIVAL_RADIUS_CELLS,
        "unit ended at {pos:?}, expected near (200.5, 200.5)"
    );
}

#[test]
fn rally_defaults_to_none_and_can_be_cleared() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let hq = h.world().start_hq().expect("hq");
    assert_eq!(h.world().rally(hq), None);
    assert!(h.world_mut().set_rally(hq, Some(Cell { x: 200, y: 200 })));
    assert_eq!(h.world().rally(hq), Some(Cell { x: 200, y: 200 }));
    assert!(h.world_mut().set_rally(hq, None));
    assert_eq!(h.world().rally(hq), None);
}

#[test]
fn set_rally_rejects_a_blocked_cell() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let hq = h.world().start_hq().expect("hq");
    let valid = Cell { x: 200, y: 200 };
    assert!(h.world_mut().set_rally(hq, Some(valid)));
    // Obstacle index 0 of the tracked scene is cell (0, 0) (see rts_build.rs).
    assert!(!h.world_mut().set_rally(hq, Some(Cell { x: 0, y: 0 })));
    assert_eq!(h.world().rally(hq), Some(valid));
}

#[test]
fn set_rally_rejects_an_out_of_bounds_cell() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let hq = h.world().start_hq().expect("hq");
    let width = h.world().scenario().width();
    assert!(!h.world_mut().set_rally(hq, Some(Cell { x: width, y: 0 })));
    assert_eq!(h.world().rally(hq), None);
}

#[test]
fn set_rally_rejects_a_non_building() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let w0 = first_worker(&h);
    assert!(!h.world_mut().set_rally(w0, Some(Cell { x: 200, y: 200 })));
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
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let w2 = h.ids_of_kind(EntityKind::Unit(UnitKind::Worker))[2];
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
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
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
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
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
    assert!(h.world_mut().set_rally(site, Some(Cell { x: 200, y: 200 })));
    assert_eq!(h.world().rally(site), Some(Cell { x: 200, y: 200 }));

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
        assert!(h.world_mut().set_rally(hq, Some(Cell { x: 200, y: 200 })));
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
    assert!(h.world_mut().set_rally(hq, Some(Cell { x: 200, y: 200 })));
    assert_ne!(h.state_hash(), before);
}
