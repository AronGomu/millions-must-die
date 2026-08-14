//! T10 — building placement and construction: cost, footprint validity,
//! progress, cancellation, and the navigation consequence of a finished
//! building.
//!
//! Pure logic, no GPU, no clock: every case here is a headless CPU check of
//! `mmd_engine::rts` and `testkit::RtsHarness`.

use std::collections::HashSet;

use mmd_engine::rts::{
    BARRACKS_BUILD_TICKS, BARRACKS_COST, BARRACKS_SUPPLY_GRANT, BuildingKind, DEPOT_BUILD_TICKS,
    DEPOT_COST, DEPOT_SUPPLY_GRANT, EntityId, EntityKind, HQ_BUILD_TICKS, HQ_COST, HQ_SUPPLY_GRANT,
    IssuedOrder, OWNER_PLAYER, Order, OrderReceiptBuffer, Placement, PlacementError, ResourceKind,
    Resources, Supply, UnitKind, UnitOrderReceipt, build_ticks, building_cost, placement_valid,
    supply_grant,
};
use mmd_engine::scenario::{Cell, MAX_SUPPLY_CAP};
use mmd_engine::testkit::RtsHarness;

fn first_worker(h: &RtsHarness) -> EntityId {
    h.ids_of_kind(EntityKind::Unit(UnitKind::Worker))[0]
}

fn crystal_node(h: &RtsHarness) -> EntityId {
    h.ids_of_kind(EntityKind::Node(ResourceKind::Crystal))[0]
}

/// A clear, obstacle-free, node-free, HQ-free corner within easy walking
/// distance of the six spawn workers.
const CLEAR_CORNER: Cell = Cell { x: 180, y: 176 };

// --- constants ---------------------------------------------------------------

#[test]
fn costs_and_times_are_the_published_constants() {
    assert_eq!(building_cost(BuildingKind::Hq), HQ_COST);
    assert_eq!(
        HQ_COST,
        Resources {
            crystal: 400,
            gas: 0
        }
    );
    assert_eq!(building_cost(BuildingKind::Depot), DEPOT_COST);
    assert_eq!(
        DEPOT_COST,
        Resources {
            crystal: 100,
            gas: 0
        }
    );
    assert_eq!(building_cost(BuildingKind::Barracks), BARRACKS_COST);
    assert_eq!(
        BARRACKS_COST,
        Resources {
            crystal: 150,
            gas: 25
        }
    );

    assert_eq!(build_ticks(BuildingKind::Hq), HQ_BUILD_TICKS);
    assert_eq!(HQ_BUILD_TICKS, 600);
    assert_eq!(build_ticks(BuildingKind::Depot), DEPOT_BUILD_TICKS);
    assert_eq!(DEPOT_BUILD_TICKS, 180);
    assert_eq!(build_ticks(BuildingKind::Barracks), BARRACKS_BUILD_TICKS);
    assert_eq!(BARRACKS_BUILD_TICKS, 300);
}

#[test]
fn only_hq_and_depot_grant_supply() {
    assert_eq!(supply_grant(BuildingKind::Hq), HQ_SUPPLY_GRANT);
    assert_eq!(HQ_SUPPLY_GRANT, 10);
    assert_eq!(supply_grant(BuildingKind::Depot), DEPOT_SUPPLY_GRANT);
    assert_eq!(DEPOT_SUPPLY_GRANT, 10);
    assert_eq!(supply_grant(BuildingKind::Barracks), BARRACKS_SUPPLY_GRANT);
    assert_eq!(BARRACKS_SUPPLY_GRANT, 0);
}

// --- placement_valid -----------------------------------------------------------

#[test]
fn placement_rejects_an_out_of_bounds_footprint() {
    let h = RtsHarness::scene().build().expect("rts scene harness");
    let min = Cell { x: 316, y: 316 };
    assert_eq!(
        placement_valid(h.world(), BuildingKind::Depot, min),
        Err(PlacementError::OutOfBounds)
    );
}

#[test]
fn placement_rejects_terrain() {
    let h = RtsHarness::scene().build().expect("rts scene harness");
    // Obstacle index 0 of the tracked scene is cell (0, 0).
    let min = Cell { x: 0, y: 0 };
    assert_eq!(
        placement_valid(h.world(), BuildingKind::Depot, min),
        Err(PlacementError::BlockedTerrain { x: 0, y: 0 })
    );
}

/// The seeded HQ is stamped into the mask like any other finished building,
/// so rule 2 (terrain, which carries every finished building) refuses this
/// before rule 3 (overlap) ever looks — exactly the ordering
/// `placement_valid` documents. Rule 3 still owns *sites*, which are not
/// stamped: see `placement_rejects_overlap_with_a_site`.
#[test]
fn placement_rejects_overlap_with_the_hq() {
    let h = RtsHarness::scene().build().expect("rts scene harness");
    let min = Cell { x: 165, y: 165 };
    assert_eq!(
        placement_valid(h.world(), BuildingKind::Depot, min),
        Err(PlacementError::BlockedTerrain { x: 165, y: 165 })
    );
}

#[test]
fn placement_rejects_overlap_with_a_site() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let w0 = first_worker(&h);
    assert!(h.world_mut().begin_placement(BuildingKind::Depot));
    assert!(h.world_mut().confirm_placement(CLEAR_CORNER, w0).is_ok());

    let overlapping = Cell { x: 183, y: 176 };
    assert_eq!(
        placement_valid(h.world(), BuildingKind::Depot, overlapping),
        Err(PlacementError::OverlapsBuilding)
    );
}

#[test]
fn placement_rejects_a_resource_node() {
    let h = RtsHarness::scene().build().expect("rts scene harness");
    let min = Cell { x: 140, y: 150 };
    assert_eq!(
        placement_valid(h.world(), BuildingKind::Depot, min),
        Err(PlacementError::CoversNode { x: 140, y: 150 })
    );
}

#[test]
fn placement_accepts_a_clear_corner() {
    let h = RtsHarness::scene().build().expect("rts scene harness");
    assert_eq!(
        placement_valid(h.world(), BuildingKind::Depot, CLEAR_CORNER),
        Ok(())
    );
}

#[test]
fn a_unit_standing_there_does_not_block_placement() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let w0 = first_worker(&h);
    let w0_slot = h.world().entities().slot(w0).expect("worker slot");
    h.world_mut()
        .entities_mut()
        .set_position(w0_slot, [184.0, 180.0]);
    assert_eq!(
        placement_valid(h.world(), BuildingKind::Depot, CLEAR_CORNER),
        Ok(())
    );
}

// --- begin/cancel placement -----------------------------------------------------

#[test]
fn begin_placement_refuses_what_you_cannot_afford() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    h.world_mut().resources_mut().crystal = 50;
    h.world_mut().resources_mut().gas = 0;
    assert!(!h.world_mut().begin_placement(BuildingKind::Depot));
    assert_eq!(h.world().placement(), Placement::None);
}

#[test]
fn begin_placement_sets_the_ghost() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    assert_eq!(h.world().resources().crystal, 300);
    assert!(h.world_mut().begin_placement(BuildingKind::Depot));
    assert_eq!(
        h.world().placement(),
        Placement::Pending {
            kind: BuildingKind::Depot
        }
    );
}

#[test]
fn cancel_placement_is_idempotent() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    assert!(h.world_mut().begin_placement(BuildingKind::Depot));
    h.world_mut().cancel_placement();
    assert_eq!(h.world().placement(), Placement::None);
    h.world_mut().cancel_placement();
    assert_eq!(h.world().placement(), Placement::None);
}

// --- confirm_placement -----------------------------------------------------------

#[test]
fn confirm_debits_the_cost() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let w0 = first_worker(&h);
    assert!(h.world_mut().begin_placement(BuildingKind::Depot));
    assert!(h.world_mut().confirm_placement(CLEAR_CORNER, w0).is_ok());
    assert_eq!(h.world().resources().crystal, 200);
}

#[test]
fn confirm_rejects_a_stale_builder() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let w0 = first_worker(&h);
    assert!(h.world_mut().entities_mut().despawn(w0));
    assert!(h.world_mut().begin_placement(BuildingKind::Depot));
    let before = h.world().resources();
    assert_eq!(
        h.world_mut().confirm_placement(CLEAR_CORNER, w0),
        Err(PlacementError::NoBuilder)
    );
    assert_eq!(h.world().resources(), before);
    assert_eq!(
        h.world().placement(),
        Placement::Pending {
            kind: BuildingKind::Depot
        }
    );
}

#[test]
fn confirm_rejects_a_soldier_builder() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let soldier = h
        .world_mut()
        .entities_mut()
        .spawn(
            EntityKind::Unit(UnitKind::Soldier),
            OWNER_PLAYER,
            [184.0, 180.0],
        )
        .expect("spawn a soldier");
    assert!(h.world_mut().begin_placement(BuildingKind::Depot));
    assert_eq!(
        h.world_mut().confirm_placement(CLEAR_CORNER, soldier),
        Err(PlacementError::NoBuilder)
    );
}

#[test]
fn confirm_rejects_when_the_stock_fell_under_the_ghost() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let w0 = first_worker(&h);
    assert!(h.world_mut().begin_placement(BuildingKind::Depot));
    h.world_mut().resources_mut().crystal = 50;
    assert_eq!(
        h.world_mut().confirm_placement(CLEAR_CORNER, w0),
        Err(PlacementError::Unaffordable)
    );
    assert_eq!(
        h.world().placement(),
        Placement::Pending {
            kind: BuildingKind::Depot
        }
    );
}

#[test]
fn confirm_spawns_an_unfinished_site() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let w0 = first_worker(&h);
    assert!(h.world_mut().begin_placement(BuildingKind::Depot));
    let id = h
        .world_mut()
        .confirm_placement(CLEAR_CORNER, w0)
        .expect("confirm");
    assert!(h.world().is_site(id));
    let slot = h.world().entities().slot(id).expect("site slot");
    assert_eq!(h.world().entities().progress(slot), 0);
    assert_eq!(h.world().entities().progress_target(slot), 180);
}

#[test]
fn confirm_orders_the_builder() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let w0 = first_worker(&h);
    assert!(h.world_mut().begin_placement(BuildingKind::Depot));
    let site = h
        .world_mut()
        .confirm_placement(CLEAR_CORNER, w0)
        .expect("confirm");
    assert!(matches!(
        h.world().order_of(w0),
        Some(Order::Build { site: s, .. }) if s == site
    ));
}

#[test]
fn confirm_clears_the_ghost() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let w0 = first_worker(&h);
    assert!(h.world_mut().begin_placement(BuildingKind::Depot));
    assert!(h.world_mut().confirm_placement(CLEAR_CORNER, w0).is_ok());
    assert_eq!(h.world().placement(), Placement::None);
}

#[test]
fn a_site_is_walkable() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let w0 = first_worker(&h);
    assert!(h.world_mut().begin_placement(BuildingKind::Depot));
    assert!(h.world_mut().confirm_placement(CLEAR_CORNER, w0).is_ok());

    let width = h.world().scenario().width();
    let blocked = h.world().nav().blocked();
    for y in CLEAR_CORNER.y..CLEAR_CORNER.y + 8 {
        for x in CLEAR_CORNER.x..CLEAR_CORNER.x + 8 {
            assert!(
                !blocked[(x + y * width) as usize],
                "cell ({x},{y}) must be walkable while the site is unfinished"
            );
        }
    }
}

// --- construction progress --------------------------------------------------------

#[test]
fn construction_does_not_advance_without_a_worker() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let w0 = first_worker(&h);
    assert!(h.world_mut().begin_placement(BuildingKind::Depot));
    let site = h
        .world_mut()
        .confirm_placement(CLEAR_CORNER, w0)
        .expect("confirm");

    // Send the worker far away instead of letting it attend. (50, 50) is a
    // legal (radius-clear) cell in the tracked scene's inflated navigation
    // mask, unlike a bare unblocked-terrain cell such as (20, 20).
    assert!(h.world_mut().order_move(w0, Cell { x: 50, y: 50 }));
    h.step_exact(600);

    assert!(
        h.world().is_site(site),
        "an unattended site must not finish"
    );
    let slot = h.world().entities().slot(site).expect("site slot");
    assert_eq!(
        h.world().entities().progress(slot),
        0,
        "an unattended site must never gain progress"
    );
}

#[test]
fn construction_advances_one_tick_per_tick() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let w0 = first_worker(&h);
    assert!(h.world_mut().begin_placement(BuildingKind::Depot));
    let site = h
        .world_mut()
        .confirm_placement(CLEAR_CORNER, w0)
        .expect("confirm");
    let slot = h.world().entities().slot(site).expect("site slot");

    // Let the worker walk over and start attending.
    let mut progress_before = 0;
    for _ in 0..400 {
        h.step_exact(1);
        let p = h.world().entities().progress(slot);
        if p > 0 {
            progress_before = p;
            break;
        }
    }
    assert!(progress_before > 0, "worker never started attending");

    h.step_exact(100);
    let progress_after = h.world().entities().progress(slot);
    assert_eq!(progress_after, progress_before + 100);
}

#[test]
fn a_second_worker_does_not_speed_it_up() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let workers = h.ids_of_kind(EntityKind::Unit(UnitKind::Worker));
    let w0 = workers[0];
    let w1 = workers[1];
    assert!(h.world_mut().begin_placement(BuildingKind::Depot));
    let site = h
        .world_mut()
        .confirm_placement(CLEAR_CORNER, w0)
        .expect("confirm");
    assert!(h.world_mut().order_build(w1, site));
    let slot = h.world().entities().slot(site).expect("site slot");

    let mut progress_before = 0;
    for _ in 0..400 {
        h.step_exact(1);
        let p = h.world().entities().progress(slot);
        if p > 0 {
            progress_before = p;
            break;
        }
    }
    assert!(progress_before > 0, "workers never started attending");

    h.step_exact(100);
    let progress_after = h.world().entities().progress(slot);
    assert_eq!(
        progress_after,
        progress_before + 100,
        "two attending workers must advance exactly as fast as one"
    );
}

#[test]
fn a_depot_finishes_in_its_documented_time() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let w0 = first_worker(&h);
    assert!(h.world_mut().begin_placement(BuildingKind::Depot));
    let site = h
        .world_mut()
        .confirm_placement(CLEAR_CORNER, w0)
        .expect("confirm");

    h.step_exact(2_000);

    assert!(!h.world().is_site(site));
    let slot = h
        .world()
        .entities()
        .slot(site)
        .expect("finished building slot");
    assert_eq!(h.world().entities().progress_target(slot), 0);
}

#[test]
fn a_finished_depot_blocks_navigation() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let w0 = first_worker(&h);
    assert!(h.world_mut().begin_placement(BuildingKind::Depot));
    let site = h
        .world_mut()
        .confirm_placement(CLEAR_CORNER, w0)
        .expect("confirm");
    h.step_exact(2_000);
    assert!(!h.world().is_site(site));

    let width = h.world().scenario().width();
    let blocked = h.world().nav().blocked();
    for y in CLEAR_CORNER.y..CLEAR_CORNER.y + 8 {
        for x in CLEAR_CORNER.x..CLEAR_CORNER.x + 8 {
            assert!(
                blocked[(x + y * width) as usize],
                "cell ({x},{y}) must be blocked once the depot finishes"
            );
        }
    }
}

#[test]
fn finishing_invalidates_the_cached_fields() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let w0 = first_worker(&h);

    // Warm a field to some unrelated destination.
    let dest = Cell { x: 200, y: 200 };
    assert!(h.world_mut().order_move(w0, dest));
    h.step_exact(1);
    assert!(h.world_mut().order_move(w0, dest)); // re-acquire: must be a hit

    let before = h.world().nav().rebuild_count();

    assert!(h.world_mut().begin_placement(BuildingKind::Depot));
    let workers = h.ids_of_kind(EntityKind::Unit(UnitKind::Worker));
    let builder = workers[1];
    let _site = h
        .world_mut()
        .confirm_placement(CLEAR_CORNER, builder)
        .expect("confirm");
    h.step_exact(2_000);

    assert!(h.world_mut().order_move(w0, dest)); // same destination: must now be a miss
    assert!(
        h.world().nav().rebuild_count() > before,
        "the finished depot must have invalidated every cached field"
    );
}

#[test]
fn a_finished_depot_raises_the_supply_cap() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let w0 = first_worker(&h);
    assert_eq!(h.world().supply().cap(), 10);
    assert!(h.world_mut().begin_placement(BuildingKind::Depot));
    assert!(h.world_mut().confirm_placement(CLEAR_CORNER, w0).is_ok());
    h.step_exact(2_000);
    assert_eq!(h.world().supply().cap(), 20);
}

#[test]
fn the_supply_cap_is_clamped_at_the_pillar() {
    let mut supply = Supply::new(10);
    for _ in 0..60 {
        supply.grant_cap(DEPOT_SUPPLY_GRANT);
    }
    assert_eq!(supply.cap(), MAX_SUPPLY_CAP);
}

#[test]
fn finishing_clears_the_builder_order() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let w0 = first_worker(&h);
    assert!(h.world_mut().begin_placement(BuildingKind::Depot));
    assert!(h.world_mut().confirm_placement(CLEAR_CORNER, w0).is_ok());
    h.step_exact(2_000);
    assert_eq!(h.world().order_of(w0), Some(Order::Idle));
}

// --- cancellation --------------------------------------------------------------

#[test]
fn cancel_refunds_the_full_cost() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let w0 = first_worker(&h);
    assert!(h.world_mut().begin_placement(BuildingKind::Depot));
    let site = h
        .world_mut()
        .confirm_placement(CLEAR_CORNER, w0)
        .expect("confirm");
    h.step_exact(60);

    assert!(h.world_mut().cancel_construction(site));
    assert_eq!(h.world().resources().crystal, 300);
    assert!(h.world().entities().slot(site).is_none());
}

#[test]
fn cancel_clears_the_builder_order() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let w0 = first_worker(&h);
    assert!(h.world_mut().begin_placement(BuildingKind::Depot));
    let site = h
        .world_mut()
        .confirm_placement(CLEAR_CORNER, w0)
        .expect("confirm");
    h.step_exact(60);

    assert!(h.world_mut().cancel_construction(site));
    assert_eq!(h.world().order_of(w0), Some(Order::Idle));
}

#[test]
fn cancel_of_a_finished_building_is_refused() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let w0 = first_worker(&h);
    assert!(h.world_mut().begin_placement(BuildingKind::Depot));
    let site = h
        .world_mut()
        .confirm_placement(CLEAR_CORNER, w0)
        .expect("confirm");
    h.step_exact(2_000);
    assert!(!h.world().is_site(site));
    let before = h.world().resources();

    assert!(!h.world_mut().cancel_construction(site));
    assert!(h.world().entities().slot(site).is_some());
    assert_eq!(h.world().resources(), before);
}

#[test]
fn cancel_of_a_stale_id_is_refused() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let w0 = first_worker(&h);
    assert!(h.world_mut().begin_placement(BuildingKind::Depot));
    let site = h
        .world_mut()
        .confirm_placement(CLEAR_CORNER, w0)
        .expect("confirm");
    assert!(h.world_mut().entities_mut().despawn(site));

    assert!(!h.world_mut().cancel_construction(site));
}

// --- the long walk -----------------------------------------------------------------

#[test]
fn a_builder_walks_to_a_far_site() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    // (298.5, 298.5), not the raw (300.5, 300.5): the direct entity-store
    // spawn below bypasses `RtsWorld`'s own collision-safe placement search,
    // so it must land on a cell that is itself legal in the inflated
    // navigation mask, or the mover never takes its first step — its own
    // cell would sample as blocked, which `FieldPool::reachable` correctly
    // reports as unreachable.
    let far = h
        .world_mut()
        .entities_mut()
        .spawn(
            EntityKind::Unit(UnitKind::Worker),
            OWNER_PLAYER,
            [298.5, 298.5],
        )
        .expect("spawn a far worker");
    assert!(h.world_mut().begin_placement(BuildingKind::Depot));
    let site = h
        .world_mut()
        .confirm_placement(CLEAR_CORNER, far)
        .expect("confirm");

    h.step_exact(3_000);

    assert!(!h.world().is_site(site), "the far site must have finished");
}

#[test]
fn a_gatherer_still_delivers_after_the_hq_is_stamped() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let w0 = first_worker(&h);
    let n_crystal = crystal_node(&h);

    // The tracked scene's HQ footprint is [160, 172) x [160, 172) (min corner
    // (160, 160), edge 12).
    for y in 160..172u32 {
        for x in 160..172u32 {
            h.world_mut().nav_mut().set_blocked(Cell { x, y }, true);
        }
    }

    assert!(h.world_mut().order_gather(w0, n_crystal));
    h.step_exact(3_000);

    assert!(
        h.world().resources().crystal > 300,
        "a worker must still be able to deliver to a stamped HQ"
    );
}

// --- reproducibility ---------------------------------------------------------------

#[test]
fn construction_is_reproducible() {
    let mut a = RtsHarness::scene().build().expect("a");
    let mut b = RtsHarness::scene().build().expect("b");
    for h in [&mut a, &mut b] {
        let w0 = first_worker(h);
        assert!(h.world_mut().begin_placement(BuildingKind::Depot));
        assert!(h.world_mut().confirm_placement(CLEAR_CORNER, w0).is_ok());
        h.step_exact(3_000);
    }
    assert_eq!(a.state_hash(), b.state_hash());
}

#[test]
fn state_hash_sees_construction_progress() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let w0 = first_worker(&h);
    assert!(h.world_mut().begin_placement(BuildingKind::Depot));
    let site = h
        .world_mut()
        .confirm_placement(CLEAR_CORNER, w0)
        .expect("confirm");
    let slot = h.world().entities().slot(site).expect("site slot");

    // Walk the builder in, then take a hash right as attendance starts.
    let mut started = false;
    for _ in 0..400 {
        h.step_exact(1);
        if h.world().entities().progress(slot) > 0 {
            started = true;
            break;
        }
    }
    assert!(started, "builder never started attending");
    let before = h.state_hash();
    h.step_exact(1);
    assert_ne!(h.state_hash(), before);
}

// --- T5: group build orders ---------------------------------------------------

/// Every eligible worker sent to one site gets its own legal approach slot —
/// distinct cells, one shared anchor field, receipts ascending by entity slot —
/// and a Soldier in the same group is rejected outright.
#[test]
fn builders_get_distinct_site_approaches() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let workers = h.ids_of_kind(EntityKind::Unit(UnitKind::Worker));
    assert_eq!(workers.len(), 6);
    assert!(h.world_mut().begin_placement(BuildingKind::Depot));
    let site = h
        .world_mut()
        .confirm_placement(CLEAR_CORNER, workers[0])
        .expect("confirm");

    let soldier = h
        .world_mut()
        .entities_mut()
        .spawn(
            EntityKind::Unit(UnitKind::Soldier),
            OWNER_PLAYER,
            [140.0, 200.0],
        )
        .expect("spawn a soldier");
    let mut group = workers.clone();
    group.push(soldier);

    let acquires = h.world().nav().acquire_count();
    let mut receipts = OrderReceiptBuffer::new();
    assert_eq!(
        h.world_mut().order_build_group(&group, site, &mut receipts),
        Ok(workers.len()),
        "every worker must be ordered and the Soldier rejected"
    );
    assert_eq!(
        h.world().nav().acquire_count(),
        acquires + 1,
        "a group build order must acquire exactly one anchor field"
    );

    // Receipts: one Build per worker, ascending by entity slot, no Soldier.
    assert_eq!(
        receipts.as_slice(),
        workers
            .iter()
            .map(|&id| UnitOrderReceipt {
                id,
                order: IssuedOrder::Build
            })
            .collect::<Vec<_>>()
            .as_slice()
    );
    assert_eq!(
        h.world().order_of(soldier),
        Some(Order::Idle),
        "a Soldier cannot build and must keep its own order"
    );

    // Distinct legal slots around one shared anchor.
    let mut slots = Vec::new();
    let mut anchors = Vec::new();
    for id in &workers {
        match h.world().order_of(*id) {
            Some(Order::Build { site: s, goal, .. }) if s == site => {
                assert!(
                    !h.world().static_nav().center_blocked()
                        [(goal.slot.x + goal.slot.y * 320) as usize],
                    "slot {:?} is not a legal body position",
                    goal.slot
                );
                slots.push((goal.slot.x, goal.slot.y));
                anchors.push((goal.anchor.x, goal.anchor.y));
            }
            other => panic!("{id:?} is not building the site: {other:?}"),
        }
    }
    let unique: HashSet<(u32, u32)> = slots.iter().copied().collect();
    assert_eq!(
        unique.len(),
        slots.len(),
        "two builders share a slot: {slots:?}"
    );
    assert_eq!(
        anchors.iter().copied().collect::<HashSet<_>>().len(),
        1,
        "one site must mean one shared anchor"
    );
}

/// The group build order really builds: six workers walk to their own slots
/// and the site finishes.
#[test]
fn a_group_of_builders_finishes_the_site() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let workers = h.ids_of_kind(EntityKind::Unit(UnitKind::Worker));
    assert!(h.world_mut().begin_placement(BuildingKind::Depot));
    let site = h
        .world_mut()
        .confirm_placement(CLEAR_CORNER, workers[0])
        .expect("confirm");
    let mut receipts = OrderReceiptBuffer::new();
    assert_eq!(
        h.world_mut()
            .order_build_group(&workers, site, &mut receipts),
        Ok(6)
    );

    for _ in 0..2_000 {
        h.step_exact(1);
        if !h.world().is_site(site) {
            break;
        }
    }
    assert!(
        !h.world().is_site(site),
        "the group never finished the site"
    );
}

// --- T6: atomic completion evacuation -------------------------------------------

mod common;
use common::{SEALED_SITE_MIN, sealed_site_spec};

use mmd_engine::rts::{RTS_UNIT_BODY_RADIUS_CELLS, units_overlap};

/// Every live unit's slot and body position, ascending slot.
fn unit_bodies(h: &RtsHarness) -> Vec<(usize, [f32; 2])> {
    let store = h.world().entities();
    (0..store.slot_count())
        .filter(|&slot| store.alive(slot) && matches!(store.kind(slot), EntityKind::Unit(_)))
        .map(|slot| (slot, store.position(slot)))
        .collect()
}

fn position_of(h: &RtsHarness, id: EntityId) -> [f32; 2] {
    let slot = h.world().entities().slot(id).expect("live entity");
    h.world().entities().position(slot)
}

fn spawn_worker(h: &mut RtsHarness, pos: [f32; 2]) -> EntityId {
    h.world_mut()
        .entities_mut()
        .spawn(EntityKind::Unit(UnitKind::Worker), OWNER_PLAYER, pos)
        .expect("spawn a worker")
}

/// No live body may penetrate static geometry, and no two may be merged.
fn assert_every_body_is_legal(h: &RtsHarness) {
    let bodies = unit_bodies(h);
    for &(slot, p) in &bodies {
        assert!(
            h.world()
                .static_nav()
                .position_clear(p, RTS_UNIT_BODY_RADIUS_CELLS),
            "slot {slot} stands at {p:?}, inside static geometry"
        );
    }
    for (i, &(sa, a)) in bodies.iter().enumerate() {
        for &(sb, b) in &bodies[i + 1..] {
            assert!(
                !units_overlap(a, RTS_UNIT_BODY_RADIUS_CELLS, b, RTS_UNIT_BODY_RADIUS_CELLS),
                "slots {sa} and {sb} are merged at {a:?} and {b:?}"
            );
        }
    }
}

/// A building finishing on top of several bodies moves every one of them, to
/// distinct legal positions, in the same tick it becomes solid.
#[test]
fn completion_evacuates_every_overlapping_body() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let builder = first_worker(&h);
    // Two bodies inside the 8 x 8 footprint, one body diameter apart, plus the
    // builder standing against its east edge — all three penetrate the
    // footprint the finished Depot will occupy.
    let inside_a = spawn_worker(&mut h, [180.5, 176.5]);
    let inside_b = spawn_worker(&mut h, [186.5, 182.5]);
    let caught = [inside_a, inside_b];

    assert!(h.world_mut().begin_placement(BuildingKind::Depot));
    let site = h
        .world_mut()
        .confirm_placement(CLEAR_CORNER, builder)
        .expect("confirm");
    h.step_exact(2_000);
    assert!(!h.world().is_site(site), "the Depot never finished");

    let width = h.world().scenario().width();
    for y in CLEAR_CORNER.y..CLEAR_CORNER.y + 8 {
        for x in CLEAR_CORNER.x..CLEAR_CORNER.x + 8 {
            assert!(
                h.world().nav().blocked()[(x + y * width) as usize],
                "the finished Depot did not stamp cell ({x},{y})"
            );
        }
    }
    let mut seen = HashSet::new();
    for id in caught {
        let p = position_of(&h, id);
        assert!(
            h.world()
                .static_nav()
                .position_clear(p, RTS_UNIT_BODY_RADIUS_CELLS),
            "a caught body was left at {p:?}, inside the finished footprint"
        );
        assert!(
            seen.insert((p[0].to_bits(), p[1].to_bits())),
            "two evacuated bodies were given the same position {p:?}"
        );
    }
    assert_every_body_is_legal(&h);
}

/// Construction places bodies itself — the builder walks in under the movement
/// system, and completion evacuates whatever the finished footprint would
/// swallow — so a build run never needs the overlap-repair pass, which the
/// shipping build does not compile at all.
///
/// The sibling above owns the evacuation *behaviour*; it forces its extra
/// bodies in through the raw store hook, which is exactly what arms the repair
/// pass, so it cannot make this claim. This one drives the plain path: one
/// builder, one Depot, no test hook at all.
#[test]
fn a_completion_never_runs_overlap_repair() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let builder = first_worker(&h);
    assert_eq!(
        h.world().overlap_repair_runs(),
        0,
        "seeding must not need the repair pass"
    );

    assert!(h.world_mut().begin_placement(BuildingKind::Depot));
    let site = h
        .world_mut()
        .confirm_placement(CLEAR_CORNER, builder)
        .expect("confirm");

    for tick in 1..=2_000u64 {
        h.step_exact(1);
        assert_eq!(
            h.world().body_overlap_count(),
            0,
            "tick {tick} ended with a merged pair"
        );
        assert_eq!(
            h.world().overlap_repair_runs(),
            0,
            "tick {tick} ran the overlap repair pass on the construction path"
        );
    }

    assert!(!h.world().is_site(site), "the Depot never finished");
    assert_every_body_is_legal(&h);
}

/// A site whose completion would leave a body with nowhere legal to stand does
/// not finish: it holds at one tick short of complete, stays walkable, grants
/// no supply, and moves nobody.
#[test]
fn completion_waits_when_evacuation_impossible() {
    let mut h = RtsHarness::spec(sealed_site_spec())
        .build()
        .expect("sealed-site scene");
    let builder = first_worker(&h);
    let cap_before = h.world().supply().cap();

    assert!(h.world_mut().begin_placement(BuildingKind::Depot));
    let site = h
        .world_mut()
        .confirm_placement(SEALED_SITE_MIN, builder)
        .expect("confirm");
    h.step_exact(DEPOT_BUILD_TICKS as u64 + 600);

    assert!(h.world().is_site(site), "the sealed site must not finish");
    let slot = h.world().entities().slot(site).expect("live site");
    assert_eq!(
        h.world().entities().progress(slot),
        DEPOT_BUILD_TICKS - 1,
        "a blocked completion must hold at one tick short of complete"
    );
    assert_eq!(
        h.world().entities().progress_target(slot),
        DEPOT_BUILD_TICKS
    );
    assert_eq!(
        h.world().supply().cap(),
        cap_before,
        "a site that did not finish granted supply anyway"
    );

    // `nav().blocked()` is the radius-inflated centre mask, which a pocket's
    // own walls already fill; the honest "was this stamped" question is asked
    // of the raw solids a finished building writes into.
    let width = h.world().scenario().width();
    for y in SEALED_SITE_MIN.y..SEALED_SITE_MIN.y + 8 {
        for x in SEALED_SITE_MIN.x..SEALED_SITE_MIN.x + 8 {
            assert!(
                !h.world().static_nav().placement_solids()[(x + y * width) as usize],
                "an unfinished site stamped cell ({x},{y})"
            );
        }
    }
    assert!(
        h.world()
            .static_nav()
            .position_clear(position_of(&h, builder), RTS_UNIT_BODY_RADIUS_CELLS),
        "the builder was moved by a completion that never happened"
    );
    assert_every_body_is_legal(&h);
}

/// Two sites finishing on the same tick are processed ascending by slot, and
/// the later one plans its evacuation against the earlier one as solid ground:
/// either both finish legally, or the later waits — never a body left inside a
/// finished footprint.
#[test]
fn later_completion_sees_earlier_building() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    h.world_mut().resources_mut().crystal = 10_000;
    let workers = h.ids_of_kind(EntityKind::Unit(UnitKind::Worker));

    // Two Depots, one body diameter apart, each attended by a builder standing
    // on its own footprint edge (`rect_distance == 0`, so attendance needs no
    // walk-in) and therefore caught by its own completion.
    let first_min = Cell { x: 176, y: 176 };
    let second_min = Cell { x: 187, y: 176 };
    let mut sites = Vec::new();
    for (min, builder) in [(first_min, workers[0]), (second_min, workers[1])] {
        let slot = h.world().entities().slot(builder).expect("live worker");
        h.world_mut()
            .entities_mut()
            .set_position(slot, [min.x as f32 + 4.5, min.y as f32 + 8.5]);
        assert!(h.world_mut().begin_placement(BuildingKind::Depot));
        let site = h
            .world_mut()
            .confirm_placement(min, builder)
            .expect("confirm");
        sites.push(site);
    }

    // Drive both to exactly one tick short of complete, then let the single
    // tick that finishes them both run.
    for &site in &sites {
        let slot = h.world().entities().slot(site).expect("live site");
        h.world_mut()
            .entities_mut()
            .set_progress(slot, DEPOT_BUILD_TICKS - 1, DEPOT_BUILD_TICKS);
    }
    h.step_exact(1);

    let width = h.world().scenario().width();
    let finished: Vec<Cell> = [first_min, second_min]
        .into_iter()
        .zip(sites.iter())
        .filter(|&(_, &site)| !h.world().is_site(site))
        .map(|(min, _)| min)
        .collect();
    assert!(
        finished.contains(&first_min),
        "the lower-slot site must finish: nothing blocks its own evacuation"
    );
    for min in &finished {
        for y in min.y..min.y + 8 {
            for x in min.x..min.x + 8 {
                assert!(
                    h.world().static_nav().placement_solids()[(x + y * width) as usize],
                    "a finished Depot did not stamp cell ({x},{y})"
                );
            }
        }
    }
    assert_every_body_is_legal(&h);
}
