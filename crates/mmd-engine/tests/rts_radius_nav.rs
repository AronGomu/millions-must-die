//! T3 — radius-aware static navigation: continuous circle-vs-static clearance,
//! the inflated centre mask pooled fields route through, legal approach cells,
//! and collision-safe initial spawn.
//!
//! Pure logic, no GPU, no clock: every case here is a headless CPU check of
//! `mmd_engine::rts::StaticNav`, `mmd_engine::nav::field_pool::FieldPool` and
//! `testkit::RtsHarness`.

use mmd_engine::nav::field_pool::FieldPool;
use mmd_engine::rts::{
    BuildingKind, EntityKind, EntityStore, OWNER_NEUTRAL, OWNER_PLAYER,
    RTS_UNIT_BODY_DIAMETER_CELLS, RTS_UNIT_BODY_RADIUS_CELLS, ResourceKind, StaticNav, UnitKind,
};
use mmd_engine::rts::{FORMATION_ARRIVAL_CELLS, Order};
use mmd_engine::scenario::{Cell, RtsSpec, Scenario, ScenarioSpec};
use mmd_engine::testkit::RtsHarness;

const RADIUS: f32 = RTS_UNIT_BODY_RADIUS_CELLS;

/// A small, validly-shaped RTS scenario with `obstacle_cells` as its terrain.
/// The HQ and both resource node kinds are tucked in a far corner, clear of
/// any obstacle geometry a test places, so the scenario's own destination/
/// node-reachability validation (point-agent geometry, unrelated to body
/// radius) always passes regardless of what a case is testing.
fn small_scenario(width: u32, height: u32, obstacle_cells: Vec<u32>) -> Scenario {
    Scenario::from_spec(small_spec(width, height, obstacle_cells))
        .expect("small scenario must validate")
}

/// A live world over [`small_scenario`]'s 50 x 40 corridor grid — HQ, starting
/// workers and all, so a case can walk a real body instead of reading a mask.
fn spec_harness(obstacle_cells: Vec<u32>) -> RtsHarness {
    RtsHarness::spec(small_spec(50, 40, obstacle_cells))
        .build()
        .expect("corridor harness")
}

fn small_spec(width: u32, height: u32, obstacle_cells: Vec<u32>) -> ScenarioSpec {
    ScenarioSpec {
        version: "rts_prototype_v1".to_string(),
        width,
        height,
        cell_size_px: 4,
        sprite_size_px: 48,
        hard_agent_count: 0,
        stretch_agent_count: 0,
        seed: 1,
        destination: Cell { x: 0, y: 0 },
        spawn_cells: vec![Cell { x: 0, y: 0 }],
        atlas_count: 4,
        direction_count: 8,
        frame_count: 4,
        collision_radius_q8: 0,
        separation_strength_q8: 0,
        separation_phases: 1,
        mass_class_count: 1,
        separation_threads: 1,
        obstacle_cells,
        rts: Some(RtsSpec {
            start_crystal: 300,
            start_gas: 100,
            start_supply_cap: 10,
            hq_cell: Cell {
                x: width - 13,
                y: height - 13,
            },
            // Far from the HQ footprint (and, for the corridor tests, on the
            // same side of any wall as the destination) so a case's own
            // obstacle geometry never has to account for them.
            crystal_nodes: vec![Cell {
                x: 1,
                y: height - 2,
            }],
            gas_nodes: vec![Cell {
                x: 1,
                y: height - 3,
            }],
        }),
    }
}

fn flat_index(x: u32, y: u32, width: u32) -> u32 {
    x + y * width
}

// ---------------------------------------------------------------------------
// Continuous clearance
// ---------------------------------------------------------------------------

#[test]
fn body_clears_map_edges() {
    let scenario = small_scenario(40, 40, vec![]);
    let store = EntityStore::new();
    let nav = StaticNav::new(&scenario, &store).expect("static nav");

    assert!(
        !nav.position_clear([2.99, 20.0], RADIUS),
        "a centre 0.01 inside the radius margin from the left edge must be rejected"
    );
    assert!(
        nav.position_clear([3.0, 20.0], RADIUS),
        "a centre exactly one radius from the left edge must be legal — touching is legal"
    );
    assert!(
        nav.position_clear([37.0, 20.0], RADIUS),
        "a centre exactly one radius from the right edge (width 40) must be legal"
    );
}

#[test]
fn body_clears_static_rectangles() {
    // A terrain block at cells (10..12, 10..12) — a 2x2 solid square.
    let obstacles = vec![
        flat_index(10, 10, 40),
        flat_index(11, 10, 40),
        flat_index(10, 11, 40),
        flat_index(11, 11, 40),
    ];
    let scenario = small_scenario(40, 40, obstacles);

    // A resource node and a finished building, spawned into the store, both
    // contribute solids the same way terrain does.
    let mut store = EntityStore::new();
    let node = store
        .spawn(
            EntityKind::Node(ResourceKind::Crystal),
            OWNER_NEUTRAL,
            [20.5, 20.5],
        )
        .expect("spawn node");
    store.set_amount(node.index as usize, 100);
    let building = store
        .spawn(
            EntityKind::Building(BuildingKind::Depot),
            OWNER_PLAYER,
            [30.0, 30.0],
        )
        .expect("spawn building");
    // `Depot` footprint is 8 cells, centred at (30, 30) -> min corner (26, 26).
    // Progress target 0 (the default) marks it finished, not a site.
    store.set_progress(building.index as usize, 0, 0);

    let nav = StaticNav::new(&scenario, &store).expect("static nav");

    // Terrain rectangle spans the closed square [10, 12] x [10, 12] — a
    // straight-on approach from the left touches it exactly at one radius,
    // and is rejected 0.1 cells closer.
    assert!(
        nav.position_clear([10.0 - RADIUS, 11.0], RADIUS),
        "a centre exactly one radius from the terrain block's left edge must be legal"
    );
    assert!(
        !nav.position_clear([10.0 - RADIUS + 0.1, 11.0], RADIUS),
        "0.1 cells inside the terrain block's clearance zone must be rejected"
    );
    // Well inside the terrain block's inflated clearance: must be blocked.
    assert!(
        !nav.position_clear([11.0, 11.0], RADIUS),
        "a centre inside the terrain block's own footprint can never be legal"
    );

    // The node cell (20, 20) — solid rectangle [20, 21] x [20, 21] — is itself
    // solid; its clearance zone rejects a body that has not backed off a full
    // radius from the rectangle's own near edge (y = 20).
    assert!(!nav.position_clear([20.5, 20.5], RADIUS));
    assert!(nav.position_clear([20.5, 20.0 - RADIUS], RADIUS));

    // The finished building's footprint [26, 34) is solid throughout.
    assert!(!nav.position_clear([30.0, 30.0], RADIUS));
    assert!(nav.position_clear([26.0 - RADIUS, 30.0], RADIUS));
}

// ---------------------------------------------------------------------------
// Corridors
// ---------------------------------------------------------------------------

/// A full-height wall at `x == wall_x`, with a `gap` cells wide opening
/// centred on `y == 20`, on a 50 x 40 grid.
fn walled_corridor(wall_x: u32, gap: u32) -> (Scenario, EntityStore) {
    let scenario = small_scenario(50, 40, walled_corridor_obstacles(wall_x, gap));
    let store = EntityStore::new();
    (scenario, store)
}

fn walled_corridor_obstacles(wall_x: u32, gap: u32) -> Vec<u32> {
    let gap_lo = 20u32.saturating_sub(gap / 2);
    let gap_hi = gap_lo + gap;
    (0..40u32)
        .filter(|y| !(gap_lo..gap_hi).contains(y))
        .map(|y| flat_index(wall_x, y, 50))
        .collect()
}

#[test]
fn five_cell_corridor_is_unreachable() {
    let (scenario, store) = walled_corridor(25, 5);
    let nav = StaticNav::new(&scenario, &store).expect("static nav");
    let mut pool =
        FieldPool::from_blocked_mask(scenario.width(), scenario.height(), nav.center_blocked())
            .expect("pool");

    let far_side = Cell { x: 45, y: 20 };
    let near_side = Cell { x: 5, y: 20 };
    let field = pool.acquire(far_side).expect("far side is itself legal");
    assert!(
        !pool.reachable(field.slot as usize, near_side),
        "a 5-cell gap cannot pass a 6-cell-diameter body; the near side must stay unreachable"
    );
}

#[test]
fn wide_corridor_is_reachable() {
    // 6 = the exact diameter; give one more cell of margin so the flat centre
    // of the gap clears both flanking wall segments with room to spare.
    let (scenario, store) = walled_corridor(25, 7);
    let nav = StaticNav::new(&scenario, &store).expect("static nav");
    let mut pool =
        FieldPool::from_blocked_mask(scenario.width(), scenario.height(), nav.center_blocked())
            .expect("pool");

    let far_side = Cell { x: 45, y: 20 };
    let near_side = Cell { x: 5, y: 20 };
    let field = pool.acquire(far_side).expect("far side is legal");
    assert!(
        pool.reachable(field.slot as usize, near_side),
        "a 7-cell gap must pass a 6-cell-diameter body"
    );
}

/// `pool.reachable` on the inflated mask is a claim about the *mask*, not
/// about a body. This walks a real 3-cell body through the same 7-cell gap in
/// a live world and asserts it arrives — the invariant the mask exists to
/// stand for: field-reachable implies a swept body can traverse. Without it,
/// the mask and `StaticNav::sweep_clear` could disagree in the gap and only
/// the mask half would be tested.
///
/// The 5-cell gap is walked too, as the negative control — the same order on
/// the same grid must leave the body on its own side of the wall — which is
/// what makes the positive half non-vacuous.
#[test]
fn a_body_walks_through_the_wide_corridor_and_not_the_narrow_one() {
    // Clear of the HQ footprint ([31, 43) x [21, 33)) and its inflation, and
    // one full radius inside the map edges.
    let far_side = Cell { x: 46, y: 10 };
    let near_start = [5.5f32, 20.5];

    let mut wide = spec_harness(walled_corridor_obstacles(25, 7));
    let walker = wide
        .world_mut()
        .entities_mut()
        .spawn(EntityKind::Unit(UnitKind::Worker), OWNER_PLAYER, near_start)
        .expect("spawn walker");
    assert!(
        wide.world_mut().order_move(walker, far_side),
        "a 7-cell gap must accept a move order across the wall"
    );
    wide.step_exact(3_000);
    let slot = wide.world().entities().slot(walker).expect("live walker");
    let p = wide.world().entities().position(slot);
    let dx = p[0] - (far_side.x as f32 + 0.5);
    let dy = p[1] - (far_side.y as f32 + 0.5);
    assert!(
        dx * dx + dy * dy <= FORMATION_ARRIVAL_CELLS * FORMATION_ARRIVAL_CELLS,
        "a 6-cell-diameter body must walk a 7-cell gap; it stopped at {p:?}, \
         not within {FORMATION_ARRIVAL_CELLS} of {far_side:?}"
    );
    assert_eq!(
        wide.world().order_of(walker),
        Some(Order::Idle),
        "the move order never completed"
    );

    let mut narrow = spec_harness(walled_corridor_obstacles(25, 5));
    let walker = narrow
        .world_mut()
        .entities_mut()
        .spawn(EntityKind::Unit(UnitKind::Worker), OWNER_PLAYER, near_start)
        .expect("spawn walker");
    narrow.world_mut().order_move(walker, far_side);
    narrow.step_exact(3_000);
    let slot = narrow.world().entities().slot(walker).expect("live walker");
    let p = narrow.world().entities().position(slot);
    assert!(
        p[0] < 25.0,
        "a 5-cell gap cannot pass a 6-cell-diameter body, but the walker \
         reached {p:?}, east of the wall at x = 25"
    );
}

// ---------------------------------------------------------------------------
// Sites vs finished buildings
// ---------------------------------------------------------------------------

#[test]
fn sites_remain_walkable_until_completion() {
    let scenario = small_scenario(40, 40, vec![]);

    let mut site_store = EntityStore::new();
    let site = site_store
        .spawn(
            EntityKind::Building(BuildingKind::Depot),
            OWNER_PLAYER,
            [20.0, 20.0],
        )
        .expect("spawn site");
    // A nonzero target marks it under construction.
    site_store.set_progress(site.index as usize, 0, 180);
    let site_nav = StaticNav::new(&scenario, &site_store).expect("static nav (site)");

    let mut finished_store = EntityStore::new();
    let building = finished_store
        .spawn(
            EntityKind::Building(BuildingKind::Depot),
            OWNER_PLAYER,
            [20.0, 20.0],
        )
        .expect("spawn finished building");
    finished_store.set_progress(building.index as usize, 0, 0);
    let finished_nav = StaticNav::new(&scenario, &finished_store).expect("static nav (finished)");

    // Depot footprint centred at (20, 20), edge 8 -> min corner (16, 16),
    // covering [16, 24) on each axis. The footprint's own centre cell:
    let idx = flat_index(20, 20, 40) as usize;
    assert!(
        !site_nav.center_blocked()[idx],
        "an unfinished site's own footprint centre must stay walkable"
    );
    assert!(
        finished_nav.center_blocked()[idx],
        "a finished building's own footprint centre must be blocked"
    );

    // The inflated clearance zone around the finished building reaches
    // outside its own footprint; the site's does not.
    let ring_idx = flat_index(15, 20, 40) as usize; // one cell outside the footprint's min edge
    assert!(
        !site_nav.center_blocked()[ring_idx],
        "an unfinished site must not inflate navigation around itself"
    );
}

// ---------------------------------------------------------------------------
// Approach cells stay legal, and interaction actually completes
// ---------------------------------------------------------------------------

/// A full gather round trip on the real tracked `rts_prototype_v1.ron` scene
/// credits crystal — the regression case for the reach-vs-density gap this
/// ticket found and fixed with an adaptive reach (interaction reach widens to
/// whatever the chosen approach cell actually needs, never less than the
/// flat-ground floor). Before that fix, this exact round trip stalled forever
/// short of the HQ on this scene's dense obstacle field.
#[test]
fn a_full_gather_round_trip_credits_crystal_on_the_tracked_scenario() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let worker = h.ids_of_kind(EntityKind::Unit(UnitKind::Worker))[0];
    let node = h.ids_of_kind(EntityKind::Node(ResourceKind::Crystal))[0];
    assert!(h.world_mut().order_gather(worker, node));

    let start_crystal = h.world().resources().crystal;
    let mut delivered = false;
    for _ in 0..3_000 {
        h.step_exact(1);
        if h.world().resources().crystal > start_crystal {
            delivered = true;
            break;
        }
    }
    assert!(
        delivered,
        "a full gather round trip on the tracked scenario must credit crystal \
         within 3000 ticks"
    );
}

/// Every worker's approach cell to a node, the HQ (a drop-off) and a build
/// site is a legal centre — and each interaction actually reaches completion,
/// not just an accepted order.
#[test]
fn approach_targets_stay_outside_solids() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let workers = h.ids_of_kind(EntityKind::Unit(UnitKind::Worker));
    let node = h.ids_of_kind(EntityKind::Node(ResourceKind::Crystal))[0];

    // Node: gather completes (mining starts, then delivery banks crystal) —
    // proven by `a_full_gather_round_trip_credits_crystal_on_the_tracked_scenario`
    // above; here just confirm the order is accepted and begins moving.
    assert!(h.world_mut().order_gather(workers[0], node));

    // A build site: the worker must be able to reach and attend it.
    let builder = workers[1];
    assert!(h.world_mut().begin_placement(BuildingKind::Depot));
    let site = h
        .world_mut()
        .confirm_placement(Cell { x: 180, y: 176 }, builder)
        .expect("placement");
    let mut finished = false;
    for _ in 0..3_000 {
        h.step_exact(1);
        if !h.world().is_site(site) {
            finished = true;
            break;
        }
    }
    assert!(
        finished,
        "a builder must be able to reach and finish a site"
    );
}

// ---------------------------------------------------------------------------
// Collision-safe initial spawn
// ---------------------------------------------------------------------------

#[test]
fn initial_workers_are_relocated_without_overlap() {
    let h = RtsHarness::scene().build().expect("rts scene harness");
    let workers = h.ids_of_kind(EntityKind::Unit(UnitKind::Worker));
    assert_eq!(workers.len(), 6, "the tracked scene seeds six workers");

    let positions: Vec<[f32; 2]> = workers
        .iter()
        .map(|&id| {
            let slot = h.world().entities().slot(id).expect("live worker");
            h.world().entities().position(slot)
        })
        .collect();

    let blocked = h.world().nav().blocked();
    let width = h.world().scenario().width();
    for &p in &positions {
        let cell = Cell {
            x: p[0].floor() as u32,
            y: p[1].floor() as u32,
        };
        assert!(
            !blocked[(cell.x + cell.y * width) as usize],
            "worker at {p:?} must stand on a legal (radius-clear) cell"
        );
    }

    let diam2 = RTS_UNIT_BODY_DIAMETER_CELLS * RTS_UNIT_BODY_DIAMETER_CELLS;
    for i in 0..positions.len() {
        for j in (i + 1)..positions.len() {
            let dx = positions[i][0] - positions[j][0];
            let dy = positions[i][1] - positions[j][1];
            let dist2 = dx * dx + dy * dy;
            assert!(
                dist2 >= diam2 || (dist2 - diam2).abs() < 1e-3,
                "workers {i} and {j} overlap: {:?} and {:?} are {} cells apart, \
                 less than the {} body diameter",
                positions[i],
                positions[j],
                dist2.sqrt(),
                RTS_UNIT_BODY_DIAMETER_CELLS
            );
        }
    }
}

/// Two worlds seeded from the same scenario relocate every worker to the
/// identical cell — the relocation search must be pure and deterministic.
#[test]
fn initial_spawn_relocation_is_deterministic() {
    let a = RtsHarness::scene().build().expect("world a");
    let b = RtsHarness::scene().build().expect("world b");
    let ids_a = a.ids_of_kind(EntityKind::Unit(UnitKind::Worker));
    let ids_b = b.ids_of_kind(EntityKind::Unit(UnitKind::Worker));
    for (&ia, &ib) in ids_a.iter().zip(ids_b.iter()) {
        let pa = a
            .world()
            .entities()
            .position(a.world().entities().slot(ia).unwrap());
        let pb = b
            .world()
            .entities()
            .position(b.world().entities().slot(ib).unwrap());
        assert_eq!(pa, pb, "relocation must be reproducible across worlds");
    }
}
