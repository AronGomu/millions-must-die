//! T15 — the phase-1 acceptance run, asserted milestone by milestone.
//!
//! This file owns the *meaning* of the run: select six workers, put five of
//! them on a crystal node and one on a gas node, build a Depot, produce a
//! Worker, build a Barracks, produce a Soldier, and pan the camera. `tests/rts_acceptance.rs` in the app crate owns the separate fact
//! that the shipped binary, driven by the tracked script, reproduces it. Two
//! failures in different places mean different things, which is the whole
//! reason both exist.
//!
//! Deliberately not driven from the script file: the run here is made of world
//! calls, so a failure names the system that broke rather than the pixel that
//! moved. The one thing this file *does* read from the script is its
//! coordinates — see `the_script_coordinates_hit_what_they_name`, which is what
//! stops the script and the scene from drifting apart.
//!
//! Pure logic: no GPU and no clock — every case is a headless CPU check of
//! `mmd_engine::rts` and `testkit::RtsHarness`.

use std::path::PathBuf;

use mmd_engine::rts::{
    BuildingKind, EntityId, EntityKind, Order, ProduceError, ResourceKind, UnitKind,
    footprint_cells,
};
use mmd_engine::scenario::Cell;
use mmd_engine::testkit::RtsHarness;

/// The tracked acceptance script, also driven by the app-crate CLI test.
const SCRIPT_REL: &str = "assets/scenarios/rts_acceptance_v1.script";

/// The scene is 320 x 320 cells.
const GRID: u32 = 320;

// --- the run's timing ------------------------------------------------------
//
// Ticks, not game constants. A milestone that does not land is retimed here;
// no number below is a balance value, and none of them is asserted against.

/// Gathering long enough for the first loads to be banked.
const TICKS_GATHER: u64 = 900;
/// Walking to the Depot site plus `DEPOT_BUILD_TICKS` of attended work.
const TICKS_DEPOT_BUILD: u64 = 900;
/// `WORKER_PRODUCE_TICKS` at the HQ.
const TICKS_WORKER_PRODUCE: u64 = 300;
/// Walking to the Barracks site plus `BARRACKS_BUILD_TICKS`.
const TICKS_BARRACKS_BUILD: u64 = 900;
/// `SOLDIER_PRODUCE_TICKS` at the Barracks.
const TICKS_SOLDIER_PRODUCE: u64 = 400;
/// Held-pan ticks at the end of the run, mirroring the tracked script's held
/// arrow key.
const TICKS_PAN: u64 = 120;

/// The scene's starting stock, from `assets/scenarios/rts_prototype_v1.ron`.
const START_CRYSTAL: u32 = 300;
const START_GAS: u32 = 100;

// --- the run's geometry, shared with the tracked script ---------------------

/// Box-select corners, in screen pixels at the starting camera.
///
/// T3's radius-aware initial spawn scatters the scene's six workers across a
/// wider area than the old one-cell-apart spawn row, so this box is wider
/// than the pre-T3 one — it must still enclose every relocated worker's
/// ground point.
const DRAG_A: [f32; 2] = [850.0, 520.0];
const DRAG_B: [f32; 2] = [1000.0, 600.0];
/// Depot footprint min corner (ghost cell `(184, 180)`, edge 8).
const DEPOT_MIN: Cell = Cell { x: 180, y: 176 };
/// Barracks footprint min corner (ghost cell `(151, 181)`, edge 10).
const BARRACKS_MIN: Cell = Cell { x: 146, y: 176 };

/// Every screen coordinate the tracked script names, with the cell it claims.
///
/// The script is written in pixels because that is what a mouse produces; this
/// table is the only place those pixels are given a meaning, and
/// `the_script_coordinates_hit_what_they_name` proves the meaning is the one
/// the live projection agrees with.
const SCRIPT_COORDS: &[([f32; 2], Cell, &str)] = &[
    ([960.0, 540.0], Cell { x: 166, y: 166 }, "HQ centre"),
    (
        [850.0, 520.0],
        Cell { x: 147, y: 174 },
        "worker box, top-left",
    ),
    (
        [1000.0, 600.0],
        Cell { x: 186, y: 176 },
        "worker box, bottom-right",
    ),
    (
        [920.0, 458.0],
        Cell { x: 140, y: 150 },
        "crystal node (140,150)",
    ),
    (
        [916.0, 568.0],
        Cell { x: 167, y: 178 },
        "worker spawn cell (167,178)",
    ),
    (
        [1072.0, 606.0],
        Cell { x: 196, y: 168 },
        "gas node (196,168)",
    ),
    (
        [896.0, 558.0],
        Cell { x: 162, y: 178 },
        "worker spawn cell (162,178)",
    ),
    ([976.0, 606.0], Cell { x: 184, y: 180 }, "Depot ghost cell"),
    (
        [906.0, 563.0],
        Cell { x: 165, y: 178 },
        "worker spawn cell (165,178)",
    ),
    (
        [840.0, 542.0],
        Cell { x: 151, y: 181 },
        "Barracks ghost cell",
    ),
];

fn script_path() -> PathBuf {
    mmd_engine::workspace_root().join(SCRIPT_REL)
}

/// What the run observed, milestone by milestone. Recorded rather than asserted
/// in place so `the_acceptance_run_is_reproducible` can drive the identical
/// sequence without duplicating it.
#[derive(Debug)]
struct Run {
    selected: usize,
    gather_orders: usize,
    gas_gather_orders: usize,
    crystal_after_gather: u32,
    gas_after_gather: u32,
    gas_end: u32,
    camera_before_pan: [f32; 2],
    camera_after_pan: [f32; 2],
    crystal_spent_on_depot: i64,
    depot_is_site_at_placement: bool,
    depot_is_site_after_build: bool,
    cap_after_depot: u32,
    depot_cells_total: usize,
    depot_cells_blocked: usize,
    crystal_spent_on_worker: i64,
    supply_used_after_worker_queued: u32,
    workers_after_produce: usize,
    crystal_spent_on_barracks: i64,
    gas_spent_on_barracks: i64,
    barracks_is_site_after_build: bool,
    soldier_enqueue: Result<(), ProduceError>,
    supply_used_rise_for_soldier: i64,
    soldiers_at_end: usize,
    supply_used_end: u32,
    supply_cap_end: u32,
    ticks_stepped: u64,
    tick_index_end: u64,
    state_hash: String,
}

/// Drive the acceptance run once. Every step is a world call, and every
/// milestone is *recorded* rather than judged — the judging lives in
/// `the_full_economy_loop_runs_end_to_end`, so both tests below drive exactly
/// the same sequence. The few `assert!`/`expect` calls here are setup failures
/// (the run could not even start), not milestones.
fn drive() -> Run {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let mut ticks = 0u64;

    // --- 1. box-select the six starting workers ---------------------------
    let view = h.world().iso_view();
    let selected = h
        .world_mut()
        .box_select_into_selection(&view, DRAG_A, DRAG_B);
    let group: Vec<EntityId> = h.world().selection().ids().to_vec();

    // --- 2. send them all to the nearest crystal node ---------------------
    let node = h.ids_of_kind(EntityKind::Node(ResourceKind::Crystal))[0];
    h.world_mut().order_gather_group(&group, node);
    let gather_orders = group
        .iter()
        .filter(|&&id| matches!(h.world().order_of(id), Some(Order::Gather { .. })))
        .count();

    // --- 2b. ...except the last of them, which works the gas node ---------
    //
    // Both resources or the economy is only half proven: an engine where gas
    // gathering is entirely broken still buys everything this run buys, from
    // the 100 gas the scene starts with.
    let gas_node = h.ids_of_kind(EntityKind::Node(ResourceKind::Gas))[1];
    let gas_worker = *group.last().expect("the box selected nobody");
    let gas_gather_orders = h.world_mut().order_gather_group(&[gas_worker], gas_node);

    // --- 3. gather ---------------------------------------------------------
    h.step_exact(TICKS_GATHER);
    ticks += TICKS_GATHER;
    let crystal_after_gather = h.world().resources().crystal;
    let gas_after_gather = h.world().resources().gas;

    // --- 4. place the Depot ------------------------------------------------
    let workers = h.ids_of_kind(EntityKind::Unit(UnitKind::Worker));
    let depot_builder = workers[0];
    let before = h.world().resources();
    assert!(
        h.world_mut().begin_placement(BuildingKind::Depot),
        "the Depot ghost would not open: {before:?}"
    );
    let depot = h
        .world_mut()
        .confirm_placement(DEPOT_MIN, depot_builder)
        .expect("Depot placement refused");
    let crystal_spent_on_depot = before.crystal as i64 - h.world().resources().crystal as i64;
    let depot_is_site_at_placement = h.world().is_site(depot);

    // --- 5 + 6. build it ---------------------------------------------------
    h.step_exact(TICKS_DEPOT_BUILD);
    ticks += TICKS_DEPOT_BUILD;
    let depot_is_site_after_build = h.world().is_site(depot);
    let cap_after_depot = h.world().supply().cap();
    let depot_slot = h.world().entities().slot(depot).expect("Depot alive");
    let depot_center = h.world().entities().position(depot_slot);
    let blocked = h.world().nav().blocked();
    let mut depot_cells_total = 0usize;
    let mut depot_cells_blocked = 0usize;
    for cell in footprint_cells(depot_center, BuildingKind::Depot.footprint_cells()) {
        depot_cells_total += 1;
        if blocked[(cell.x + cell.y * GRID) as usize] {
            depot_cells_blocked += 1;
        }
    }

    // --- 7. queue a Worker at the HQ ---------------------------------------
    let hq = h.world().start_hq().expect("the scene's HQ");
    let before = h.world().resources();
    h.world_mut()
        .enqueue_unit(hq, UnitKind::Worker)
        .expect("the HQ refused a Worker");
    let crystal_spent_on_worker = before.crystal as i64 - h.world().resources().crystal as i64;
    let supply_used_after_worker_queued = h.world().supply().used();

    // --- 8. produce it -----------------------------------------------------
    h.step_exact(TICKS_WORKER_PRODUCE);
    ticks += TICKS_WORKER_PRODUCE;
    let workers_after_produce = h.ids_of_kind(EntityKind::Unit(UnitKind::Worker)).len();

    // --- 9. place the Barracks ---------------------------------------------
    let barracks_builder = workers[1];
    let before = h.world().resources();
    assert!(
        h.world_mut().begin_placement(BuildingKind::Barracks),
        "the Barracks ghost would not open: {before:?}"
    );
    let barracks = h
        .world_mut()
        .confirm_placement(BARRACKS_MIN, barracks_builder)
        .expect("Barracks placement refused");
    let after = h.world().resources();
    let crystal_spent_on_barracks = before.crystal as i64 - after.crystal as i64;
    let gas_spent_on_barracks = before.gas as i64 - after.gas as i64;

    // --- 10. build it -------------------------------------------------------
    h.step_exact(TICKS_BARRACKS_BUILD);
    ticks += TICKS_BARRACKS_BUILD;
    let barracks_is_site_after_build = h.world().is_site(barracks);

    // --- 11. queue a Soldier ------------------------------------------------
    let supply_before = h.world().supply().used();
    let soldier_enqueue = h.world_mut().enqueue_unit(barracks, UnitKind::Soldier);
    let supply_used_rise_for_soldier = h.world().supply().used() as i64 - supply_before as i64;

    // --- 12. produce it -----------------------------------------------------
    h.step_exact(TICKS_SOLDIER_PRODUCE);
    ticks += TICKS_SOLDIER_PRODUCE;
    let soldiers_at_end = h.ids_of_kind(EntityKind::Unit(UnitKind::Soldier)).len();

    // --- 13. pan the camera off the base ------------------------------------
    //
    // The direction is what the tracked script's held `right` arrow produces:
    // screen space, converted through the live projection by the camera
    // system. A player who cannot look away from their own base cannot play.
    let camera_before_pan = h.world().camera().center();
    h.world_mut().set_pan_dir([1.0, 0.0]);
    h.step_exact(TICKS_PAN);
    ticks += TICKS_PAN;
    h.world_mut().set_pan_dir([0.0, 0.0]);
    let camera_after_pan = h.world().camera().center();

    Run {
        selected,
        gather_orders,
        gas_gather_orders,
        crystal_after_gather,
        gas_after_gather,
        gas_end: h.world().resources().gas,
        camera_before_pan,
        camera_after_pan,
        crystal_spent_on_depot,
        depot_is_site_at_placement,
        depot_is_site_after_build,
        cap_after_depot,
        depot_cells_total,
        depot_cells_blocked,
        crystal_spent_on_worker,
        supply_used_after_worker_queued,
        workers_after_produce,
        crystal_spent_on_barracks,
        gas_spent_on_barracks,
        barracks_is_site_after_build,
        soldier_enqueue,
        supply_used_rise_for_soldier,
        soldiers_at_end,
        supply_used_end: h.world().supply().used(),
        supply_cap_end: h.world().supply().cap(),
        ticks_stepped: ticks,
        tick_index_end: h.tick_index(),
        state_hash: h.state_hash_hex(),
    }
}

/// The acceptance run, asserted milestone by milestone.
#[test]
fn the_full_economy_loop_runs_end_to_end() {
    let r = drive();

    // 1
    assert_eq!(
        r.selected, 6,
        "milestone 1: the box did not select the six starting workers"
    );
    // 2
    assert_eq!(
        r.gather_orders, 6,
        "milestone 2: not every selected worker took a Gather order"
    );
    // 3
    assert!(
        r.crystal_after_gather > START_CRYSTAL,
        "milestone 3: crystal is {} after {TICKS_GATHER} ticks of gathering — \
         the round trip banked nothing over the starting {START_CRYSTAL}",
        r.crystal_after_gather
    );
    assert_eq!(
        r.gas_gather_orders, 1,
        "milestone 3: no worker took the gas order, so the gas below proves nothing"
    );
    assert!(
        r.gas_after_gather > START_GAS,
        "milestone 3: gas is {} after {TICKS_GATHER} ticks — the gas round trip \
         banked nothing over the starting {START_GAS}",
        r.gas_after_gather
    );
    // 4
    assert_eq!(
        r.crystal_spent_on_depot, 100,
        "milestone 4: placing the Depot debited {} crystal, not DEPOT_COST",
        r.crystal_spent_on_depot
    );
    assert!(
        r.depot_is_site_at_placement,
        "milestone 4: the Depot was not a construction site when it was placed"
    );
    // 5
    assert!(
        !r.depot_is_site_after_build,
        "milestone 5: the Depot was still a site after {TICKS_DEPOT_BUILD} ticks"
    );
    assert_eq!(
        r.cap_after_depot, 20,
        "milestone 5: the supply cap is {} — a finished Depot must raise it to 20",
        r.cap_after_depot
    );
    // 6
    assert_eq!(
        r.depot_cells_total, 64,
        "milestone 6: a Depot footprint is 8 x 8 cells"
    );
    assert_eq!(
        r.depot_cells_blocked, r.depot_cells_total,
        "milestone 6: {} of {} Depot footprint cells are blocked in navigation — \
         a finished building must be stamped whole",
        r.depot_cells_blocked, r.depot_cells_total
    );
    // 7
    assert_eq!(
        r.crystal_spent_on_worker, 50,
        "milestone 7: queueing a Worker debited {} crystal, not WORKER_COST",
        r.crystal_spent_on_worker
    );
    assert_eq!(
        r.supply_used_after_worker_queued, 7,
        "milestone 7: supply used is {} — six live workers plus one reserved by \
         the queue is 7",
        r.supply_used_after_worker_queued
    );
    // 8
    assert_eq!(
        r.workers_after_produce, 7,
        "milestone 8: {} workers after {TICKS_WORKER_PRODUCE} ticks — the HQ owed a seventh",
        r.workers_after_produce
    );
    // 9
    assert_eq!(
        r.crystal_spent_on_barracks, 150,
        "milestone 9: placing the Barracks debited {} crystal, not BARRACKS_COST",
        r.crystal_spent_on_barracks
    );
    assert_eq!(
        r.gas_spent_on_barracks, 25,
        "milestone 9: placing the Barracks debited {} gas, not BARRACKS_COST",
        r.gas_spent_on_barracks
    );
    // 10
    assert!(
        !r.barracks_is_site_after_build,
        "milestone 10: the Barracks was still a site after {TICKS_BARRACKS_BUILD} ticks"
    );
    // 11
    assert_eq!(
        r.soldier_enqueue,
        Ok(()),
        "milestone 11: the finished Barracks refused a Soldier"
    );
    assert_eq!(
        r.supply_used_rise_for_soldier, 2,
        "milestone 11: queueing a Soldier moved supply used by {}, not \
         SOLDIER_SUPPLY_COST",
        r.supply_used_rise_for_soldier
    );
    // 12
    assert_eq!(
        r.soldiers_at_end, 1,
        "milestone 12: {} soldiers after {TICKS_SOLDIER_PRODUCE} ticks — the Barracks owed one",
        r.soldiers_at_end
    );
    // 13
    assert!(
        r.supply_used_end <= r.supply_cap_end,
        "milestone 13: supply {}/{} — the run overspent its own cap",
        r.supply_used_end,
        r.supply_cap_end
    );
    // 14: the run ends with more gas than it started with, having also *spent*
    // 25 of it on the Barracks — only gathering can do that.
    assert!(
        r.gas_end > START_GAS,
        "milestone 14: gas is {} at the end of a run that spent 25 on the \
         Barracks and started with {START_GAS}",
        r.gas_end
    );
    // 15: and the camera actually moved when it was panned.
    assert!(
        r.camera_after_pan[0] > r.camera_before_pan[0],
        "milestone 15: the camera centre went {:?} -> {:?} under {TICKS_PAN} \
         ticks of held right-pan; the view never left the base",
        r.camera_before_pan,
        r.camera_after_pan
    );
    assert!(
        r.camera_after_pan[1] < r.camera_before_pan[1],
        "milestone 15: a screen-right pan must move the centre toward a larger \
         `cell.x - cell.y`, got {:?} -> {:?}",
        r.camera_before_pan,
        r.camera_after_pan
    );
    // 16
    assert_eq!(
        r.tick_index_end, r.ticks_stepped,
        "milestone 16: the world is at tick {} after {} stepped ticks — something \
         silently did not advance",
        r.tick_index_end, r.ticks_stepped
    );
}

/// The same run, twice, must land on the same hash.
#[test]
fn the_acceptance_run_is_reproducible() {
    let a = drive();
    let b = drive();
    assert_eq!(
        a.state_hash, b.state_hash,
        "the acceptance run is not reproducible:\n{a:#?}\n{b:#?}"
    );
    assert_eq!(
        a.tick_index_end, b.tick_index_end,
        "the two runs did not even step the same number of ticks"
    );
}

/// Every screen coordinate the tracked script uses must project to the cell it
/// claims. Move a node, retune the camera start, or edit the script's pixels
/// and this fails — instead of the CLI run silently ordering nobody.
#[test]
fn the_script_coordinates_hit_what_they_name() {
    let h = RtsHarness::scene().build().expect("rts scene harness");
    let view = h.world().iso_view();
    assert_eq!(
        h.world().scenario().width(),
        GRID,
        "the scene is no longer {GRID} cells wide; the script's pixels were derived for it"
    );

    for (screen, cell, what) in SCRIPT_COORDS {
        assert_eq!(
            view.cell_at(screen[0], screen[1], GRID, GRID),
            Some(*cell),
            "the script's {what} coordinate {screen:?} no longer lands on {cell:?}"
        );
    }

    // ...and the table must cover the script, or a coordinate could drift by
    // being added rather than edited.
    let text = std::fs::read_to_string(script_path())
        .unwrap_or_else(|e| panic!("read {}: {e}", script_path().display()));
    let used = coords_in(&text);
    assert!(!used.is_empty(), "{SCRIPT_REL} names no coordinates at all",);
    for screen in &used {
        assert!(
            SCRIPT_COORDS.iter().any(|(s, _, _)| s == screen),
            "{SCRIPT_REL} uses coordinate {screen:?}, which this test documents no cell for"
        );
    }
    for (screen, _, what) in SCRIPT_COORDS {
        assert!(
            used.contains(screen),
            "this test documents the {what} coordinate {screen:?}, which {SCRIPT_REL} \
             no longer uses"
        );
    }
}

/// Every `X,Y` pair in a script's text, in order, comments stripped.
///
/// A deliberately separate, dumber reader than the app crate's `RtsScript`:
/// this test must fail when the *script* drifts, not agree with the parser
/// about a drift they share.
fn coords_in(text: &str) -> Vec<[f32; 2]> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("");
        for entry in line.split(';') {
            let entry = entry.trim();
            if entry.is_empty() {
                continue;
            }
            let mut parts = entry.splitn(3, ':');
            let (_frame, _kind) = (parts.next(), parts.next());
            let Some(args) = parts.next() else {
                continue;
            };
            let nums: Vec<f32> = args
                .split(',')
                .filter_map(|n| n.trim().parse::<f32>().ok())
                .collect();
            if !nums.len().is_multiple_of(2) {
                continue;
            }
            for pair in nums.chunks_exact(2) {
                out.push([pair[0], pair[1]]);
            }
        }
    }
    out
}
