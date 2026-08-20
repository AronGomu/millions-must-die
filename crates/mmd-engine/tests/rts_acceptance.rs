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
    BuildingKind, EntityId, EntityKind, FramePackOptions, HudHit, HudLayout, ModalHit, ModalPage,
    Order, PAN_MIN, Pick, ProduceError, ResourceKind, RtsFrame, SETTINGS_SCROLL_STEP_PX, UnitKind,
    footprint_cells, hud_hit_test, minimap_projection, modal_hit_test, pack_frame_with_options,
    pick_at, sprite_screen_rect,
};
use mmd_engine::scenario::Cell;
use mmd_engine::testkit::RtsHarness;

/// The tracked acceptance script, also driven by the app-crate CLI test.
const SCRIPT_REL: &str = "assets/scenarios/rts_acceptance_v1.script";
/// The focused feedback-polish script (`T15`), driven by the same CLI test.
const FOCUSED_SCRIPT_REL: &str = "assets/scenarios/rts_feedback_polish_v1.script";

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
const DRAG_A: [f32; 2] = [800.0, 520.0];
const DRAG_B: [f32; 2] = [940.0, 620.0];
/// Depot footprint min corner (ghost cell `(188, 196)`, edge 8).
const DEPOT_MIN: Cell = Cell { x: 184, y: 192 };
/// Barracks footprint min corner (ghost cell `(152, 168)`, edge 16).
const BARRACKS_MIN: Cell = Cell { x: 144, y: 160 };

/// The minimap point the tracked script clicks, in screen pixels.
///
/// Derived, not guessed: `acceptance_minimap_click_moves_camera` runs it back
/// through the live [`mmd_engine::rts::MinimapProjection`], proves it falls in
/// the map's own diamond and in its right half, and then proves the camera
/// actually goes there and stays frontier-safe.
const MINIMAP_CLICK: [f32; 2] = [360.0, 960.0];

/// The keyboard-pan speed the script's slider click must snap to.
const SLIDER_PAN_SPEED: u32 = 78;

/// What one script coordinate means — and therefore what must be true of it.
///
/// A phase-1.1 script no longer names only world cells: it clicks node
/// sprites, HUD chrome, an open modal, and one point deliberately off the
/// map. Each of those is a *different* claim, so each gets its own variant
/// rather than one `Cell` the HUD points would have to lie about.
#[derive(Debug)]
enum ScriptPoint {
    /// A world click that projects to this map cell.
    Cell(Cell),
    /// A world click one pixel inside a resource node's **visible quad**, at
    /// the named corner: it picks the node standing on this ground cell, and
    /// deliberately does *not* project to it.
    NodeQuad(Cell),
    /// A world click with no map cell under it at all — the run's one
    /// deliberate invalid order.
    OffMap,
    /// Owned by the HUD before the world ever sees it.
    Hud(HudHit),
    /// Owned by an open settings modal.
    Modal(ModalHit),
    /// A world click that resolves to the building whose footprint centre
    /// stands on this cell — not merely "projects onto a cell inside it".
    Building(Cell),
}

/// Every screen coordinate the tracked script names, with what it claims.
///
/// The script is written in pixels because that is what a mouse produces; this
/// table is the only place those pixels are given a meaning, and
/// `the_script_coordinates_hit_what_they_name` proves the meaning is the one
/// the live projection, the live HUD hit test and the live modal hit test all
/// agree with.
const SCRIPT_COORDS: &[([f32; 2], ScriptPoint, &str)] = &[
    (
        [960.0, 540.0],
        ScriptPoint::Cell(Cell { x: 172, y: 172 }),
        "the SETTINGS button with the menu open (a world click on HQ centre otherwise)",
    ),
    (
        [960.0, 494.0],
        ScriptPoint::Building(Cell { x: 172, y: 172 }),
        "the HQ's own footprint corner (160.5, 160.5), which picks the HQ",
    ),
    (
        [800.0, 520.0],
        ScriptPoint::Cell(Cell { x: 147, y: 187 }),
        "worker box, top-left",
    ),
    (
        [940.0, 620.0],
        ScriptPoint::Cell(Cell { x: 189, y: 194 }),
        "worker box, bottom-right",
    ),
    (
        [897.0, 387.0],
        ScriptPoint::NodeQuad(Cell { x: 140, y: 150 }),
        "crystal node quad, one pixel inside its top-left corner",
    ),
    (
        [872.0, 567.0],
        ScriptPoint::Cell(Cell { x: 167, y: 189 }),
        "an already-selected worker's ground point at frame 12",
    ),
    (
        [1049.0, 535.0],
        ScriptPoint::NodeQuad(Cell { x: 196, y: 168 }),
        "gas node quad, one pixel inside its top-left corner",
    ),
    (
        [1900.0, 100.0],
        ScriptPoint::OffMap,
        "the invalid order: logical content, no HUD, no map cell",
    ),
    (
        [780.0, 520.0],
        ScriptPoint::Cell(Cell { x: 144, y: 189 }),
        "the Depot builder's box, top-left",
    ),
    (
        [830.0, 552.0],
        ScriptPoint::Cell(Cell { x: 158, y: 191 }),
        "the Depot builder's box, bottom-right",
    ),
    (
        [1800.0, 888.0],
        ScriptPoint::Hud(HudHit::CommandSlot(1)),
        "command slot 1 centre (Depot)",
    ),
    (
        [928.0, 622.0],
        ScriptPoint::Cell(Cell { x: 188, y: 196 }),
        "Depot ghost cell",
    ),
    (
        [834.0, 500.0],
        ScriptPoint::Cell(Cell { x: 146, y: 177 }),
        "the Barracks builder's box, top-left",
    ),
    (
        [880.0, 524.0],
        ScriptPoint::Cell(Cell { x: 158, y: 178 }),
        "the Barracks builder's box, bottom-right",
    ),
    (
        [1872.0, 888.0],
        ScriptPoint::Hud(HudHit::CommandSlot(2)),
        "command slot 2 centre (Barracks)",
    ),
    (
        [896.0, 494.0],
        ScriptPoint::Cell(Cell { x: 152, y: 168 }),
        "Barracks ghost cell, and the finished Barracks' own footprint",
    ),
    (
        [1728.0, 888.0],
        ScriptPoint::Hud(HudHit::CommandSlot(0)),
        "command slot 0 centre (Worker / Soldier)",
    ),
    (
        MINIMAP_CLICK,
        ScriptPoint::Hud(HudHit::Minimap(MINIMAP_CLICK)),
        "minimap, inside the diamond's right half",
    ),
    (
        [1888.0, 24.0],
        ScriptPoint::Hud(HudHit::Menu),
        "settings menu centre",
    ),
    (
        [1170.0, 288.0],
        ScriptPoint::Modal(ModalHit::KeyboardPan(SLIDER_PAN_SPEED)),
        "keyboard-pan slider, at the 78 cells/s step",
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
    /// Body-penetration scans, `(milestone, policy violations, raw pairs)`,
    /// taken before the first order and after every order/build/produce
    /// phase. Every policy entry must read 0 — see
    /// `acceptance_never_has_body_penetration` — while the raw column is what
    /// makes those zeros mean something (`canonical_gather_overlap_is_non_vacuous`).
    body_scans: Vec<(&'static str, u32, u32)>,
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
    let mut body_scans: Vec<(&'static str, u32, u32)> = Vec::new();
    macro_rules! scan {
        ($h:expr, $what:literal) => {
            body_scans.push((
                $what,
                $h.world().body_overlap_count(),
                $h.world().raw_body_overlap_count(),
            ))
        };
    }
    scan!(h, "initial spawn");

    // --- 1. box-select the six starting workers ---------------------------
    let view = h.world().iso_view();
    let selected = h
        .world_mut()
        .box_select_into_selection(&view, DRAG_A, DRAG_B);
    let group: Vec<EntityId> = h.world().selection().ids().to_vec();

    // --- 2. send them all to the nearest crystal node ---------------------
    let node = h.ids_of_kind(EntityKind::Node(ResourceKind::Crystal))[0];
    assert!(h.world_mut().order_gather_group(&group, node).is_ok());
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
    let gas_gather_orders = h
        .world_mut()
        .order_gather_group(&[gas_worker], gas_node)
        .unwrap_or(0);

    scan!(h, "after the gather orders");

    // --- 3. gather ---------------------------------------------------------
    h.step_exact(TICKS_GATHER);
    ticks += TICKS_GATHER;
    scan!(h, "after gathering");
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
    scan!(h, "after the Depot was placed");

    // --- 5 + 6. build it ---------------------------------------------------
    h.step_exact(TICKS_DEPOT_BUILD);
    ticks += TICKS_DEPOT_BUILD;
    let depot_is_site_after_build = h.world().is_site(depot);
    scan!(h, "after the Depot finished");
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
    scan!(h, "after the HQ produced a Worker");

    // --- 9. place the Barracks ---------------------------------------------
    // workers[3], not workers[1]: since T7 the six seeded bodies are packed
    // tighter under the enlarged HQ, and the middle of that cluster cannot
    // push its way west to the Barracks plot inside this run's tick budget
    // (ADR 017's bounded push chain). The tracked script boxes the same
    // outer worker for the same reason.
    let barracks_builder = workers[3];
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
    scan!(h, "after the Barracks finished");

    // --- 11. queue a Soldier ------------------------------------------------
    let supply_before = h.world().supply().used();
    let soldier_enqueue = h.world_mut().enqueue_unit(barracks, UnitKind::Soldier);
    let supply_used_rise_for_soldier = h.world().supply().used() as i64 - supply_before as i64;

    // --- 12. produce it -----------------------------------------------------
    h.step_exact(TICKS_SOLDIER_PRODUCE);
    ticks += TICKS_SOLDIER_PRODUCE;
    let soldiers_at_end = h.ids_of_kind(EntityKind::Unit(UnitKind::Soldier)).len();
    scan!(h, "after the Barracks produced a Soldier");

    // --- 13. pan the camera off the base ------------------------------------
    //
    // The direction is what the tracked script's held `right` arrow produces:
    // screen space, converted through the live projection by the camera
    // system. A player who cannot look away from their own base cannot play.
    let camera_before_pan = h.world().camera().center();
    h.world_mut().set_keyboard_pan_dir([1.0, 0.0]);
    h.step_exact(TICKS_PAN);
    ticks += TICKS_PAN;
    h.world_mut().set_keyboard_pan_dir([0.0, 0.0]);
    let camera_after_pan = h.world().camera().center();
    scan!(h, "final");

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
        body_scans,
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

/// Hard bodies, asserted as an invariant rather than at one lucky moment:
/// every milestone of the acceptance run scans every unordered pair of live
/// units and demands `d^2 >= (r1 + r2)^2`.
///
/// The same oracle the shipped `rts` exit line reports as `body_overlaps`,
/// so a scripted run and this world-call run cannot disagree about what
/// "hard bodies" means.
#[test]
fn acceptance_never_has_body_penetration() {
    let r = drive();
    assert!(
        r.body_scans.len() >= 9,
        "the run must scan for penetration at every milestone, not once: {:?}",
        r.body_scans
    );
    for (milestone, overlaps, _raw) in &r.body_scans {
        assert_eq!(
            *overlaps, 0,
            "{milestone}: {overlaps} pair(s) of live units penetrate each other"
        );
    }
}

/// Anti-vacuity for the run above, at run scale rather than in a two-body
/// fixture: T13 lets an *active gather pair* pass through its partner, so
/// somewhere in the acceptance run bodies really do interpenetrate. The
/// policy oracle reading 0 at every milestone only means "no violation"
/// because the raw oracle reads more than 0 at some of them; if gathering
/// ever stopped producing geometric overlap, `acceptance_never_has_body_
/// penetration` would be proving nothing and this fails first.
#[test]
fn canonical_gather_overlap_is_non_vacuous() {
    let r = drive();
    let raw_total: u32 = r.body_scans.iter().map(|(_, _, raw)| *raw).sum();
    assert!(
        raw_total > 0,
        "no milestone of the acceptance run has a single geometrically overlapping \
         pair, so the policy oracle's zeros are vacuous: {:?}",
        r.body_scans
    );
    // ...and every one of those raw overlaps was an exempt pair, never a
    // violation: the two oracles differ only by the exemption.
    for (milestone, policy, raw) in &r.body_scans {
        assert!(
            *policy <= *raw,
            "{milestone}: {policy} policy violation(s) but only {raw} overlapping pair(s) \
             — the policy oracle cannot see more than the raw one"
        );
    }
}

/// Anti-vacuity for the scan above: the oracle must actually be able to see
/// a penetration, or reading 0 everywhere proves nothing.
#[test]
fn the_body_oracle_sees_a_forced_penetration() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    assert_eq!(h.world().body_overlap_count(), 0, "the scene starts clean");
    let workers = h.ids_of_kind(EntityKind::Unit(UnitKind::Worker));
    let target = {
        let store = h.world().entities();
        store.position(store.slot(workers[0]).expect("live worker"))
    };
    assert!(
        h.world_mut().force_position_for_test(workers[1], target),
        "the test hook must accept a live id"
    );
    assert_eq!(
        h.world().body_overlap_count(),
        1,
        "two bodies at the same point are one penetrating pair"
    );
}

/// The script's resource clicks are one pixel inside the *visible quad*'s
/// top-left corner, not on the node's ground point: a player clicks the
/// sprite they can see, so the sprite is what must be pickable.
#[test]
fn resource_click_uses_visible_quad_corner() {
    let h = RtsHarness::scene().build().expect("rts scene harness");
    let view = h.world().iso_view();
    let store = h.world().entities();

    for (screen, claim, what) in SCRIPT_COORDS {
        let ScriptPoint::NodeQuad(ground) = claim else {
            continue;
        };
        let node = node_standing_on(&h, *ground);
        let slot = store.slot(node).expect("live node");
        let quad = sprite_screen_rect(&view, store.position(slot));
        assert_eq!(
            *screen,
            [quad[0] + 1.0, quad[1] + 1.0],
            "{what}: {screen:?} is not one pixel inside the quad {quad:?}'s top-left corner"
        );
        // ...and the corner is genuinely *not* the ground point's own cell:
        // a click that happened to land on the node's cell anyway would
        // prove nothing about picking the sprite.
        assert_ne!(
            view.cell_at(screen[0], screen[1], GRID, GRID),
            Some(*ground),
            "{what}: the quad corner still projects onto the node's own cell, so this \
             coordinate does not exercise sprite picking at all"
        );
        assert_eq!(
            pick_at(h.world(), &view, *screen),
            Pick::Node(node),
            "{what}: the visible quad corner does not pick the node"
        );
    }
}

/// The script's minimap click: derived from the live projection, inside the
/// map's own diamond and in its right half, and it moves the camera to a
/// frontier-legal centre.
#[test]
fn acceptance_minimap_click_moves_camera() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let projection = minimap_projection(h.world());
    let origin = [HudLayout::MINIMAP_MAP[0], HudLayout::MINIMAP_MAP[1]];
    let local = [MINIMAP_CLICK[0] - origin[0], MINIMAP_CLICK[1] - origin[1]];
    let map_point = projection
        .minimap_to_map(local)
        .unwrap_or_else(|| panic!("{MINIMAP_CLICK:?} is outside the minimap's map diamond"));
    assert!(
        map_point[0] > GRID as f32 * 0.5,
        "the click must be in the diamond's right half, got {map_point:?}"
    );

    let before = h.world().camera().center();
    h.world_mut().look_at_map_point(map_point);
    let after = h.world().camera().center();
    assert_ne!(
        before, after,
        "a minimap click on the far side of the map did not move the camera"
    );
    // Frontier-safe: the clamp is what keeps a minimap jump from showing
    // the void past the map's own edge.
    let frontier = h.world().camera().frontier();
    let view = h.world().camera().iso_view();
    let p =
        mmd_engine::render::iso_project(after[0], after[1], view.tile_w, view.tile_h, [0.0, 0.0]);
    assert!(
        p[0] >= frontier.x[0] - 1e-3 && p[0] <= frontier.x[1] + 1e-3,
        "camera centre {after:?} projects to {p:?}, outside the frontier {frontier:?}"
    );
    assert!(
        p[1] >= frontier.y[0] - 1e-3 && p[1] <= frontier.y[1] + 1e-3,
        "camera centre {after:?} projects to {p:?}, outside the frontier {frontier:?}"
    );
}

/// The live building whose centre stands on `centre`, by its floored
/// cell-space position.
fn building_standing_on(h: &RtsHarness, centre: Cell) -> EntityId {
    let store = h.world().entities();
    for slot in 0..store.slot_count() {
        if !store.alive(slot) || !matches!(store.kind(slot), EntityKind::Building(_)) {
            continue;
        }
        let p = store.position(slot);
        if p[0].floor() as u32 == centre.x && p[1].floor() as u32 == centre.y {
            return store.id_at(slot).expect("live building");
        }
    }
    panic!("no building stands on {centre:?}")
}

/// The live node standing on `ground`, by its floored cell-space position.
fn node_standing_on(h: &RtsHarness, ground: Cell) -> EntityId {
    let store = h.world().entities();
    for kind in [
        EntityKind::Node(ResourceKind::Crystal),
        EntityKind::Node(ResourceKind::Gas),
    ] {
        for id in h.ids_of_kind(kind) {
            let slot = store.slot(id).expect("live node");
            let p = store.position(slot);
            if p[0].floor() as u32 == ground.x && p[1].floor() as u32 == ground.y {
                return id;
            }
        }
    }
    panic!("no resource node stands on {ground:?}")
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

    for (screen, claim, what) in SCRIPT_COORDS {
        let cell = view.cell_at(screen[0], screen[1], GRID, GRID);
        match claim {
            ScriptPoint::Cell(expected) => {
                assert_eq!(
                    cell,
                    Some(*expected),
                    "the script's {what} coordinate {screen:?} no longer lands on {expected:?}"
                );
                assert_eq!(
                    hud_hit_test(h.world(), *screen),
                    None,
                    "the script's {what} coordinate {screen:?} is a world click, but the \
                     HUD now owns it"
                );
            }
            ScriptPoint::NodeQuad(ground) => {
                let node = node_standing_on(&h, *ground);
                assert_eq!(
                    pick_at(h.world(), &view, *screen),
                    Pick::Node(node),
                    "the script's {what} coordinate {screen:?} no longer picks the node on \
                     {ground:?}"
                );
            }
            ScriptPoint::OffMap => {
                assert_eq!(
                    cell, None,
                    "the script's {what} coordinate {screen:?} now has a map cell under it, \
                     so the order it makes would no longer be refused"
                );
                assert_eq!(
                    hud_hit_test(h.world(), *screen),
                    None,
                    "the script's {what} coordinate {screen:?} must reach the world to be \
                     refused by it; the HUD now consumes it instead"
                );
            }
            ScriptPoint::Hud(expected) => {
                assert_eq!(
                    hud_hit_test(h.world(), *screen),
                    Some(*expected),
                    "the script's {what} coordinate {screen:?} no longer hits {expected:?}"
                );
            }
            ScriptPoint::Modal(expected) => {
                assert_eq!(
                    modal_hit_test(ModalPage::Settings, *screen, 0.0),
                    *expected,
                    "the script's {what} coordinate {screen:?} no longer hits {expected:?}"
                );
            }
            ScriptPoint::Building(centre) => {
                let building = building_standing_on(&h, *centre);
                assert_eq!(
                    hud_hit_test(h.world(), *screen),
                    None,
                    "the script's {what} coordinate {screen:?} is a world click, but the \
                     HUD now owns it"
                );
                assert_eq!(
                    pick_at(h.world(), &view, *screen),
                    Pick::Building(building),
                    "the script's {what} coordinate {screen:?} no longer picks the building \
                     standing on {centre:?}"
                );
            }
        }
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
            "{SCRIPT_REL} uses coordinate {screen:?}, which this test documents no meaning for"
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

// ---------------------------------------------------------------------------
// T15: the focused feedback-polish script
// ---------------------------------------------------------------------------

/// The scroll offset one `wheel:...,-1` notch leaves the settings body at.
const FOCUSED_SCROLL_PX: f32 = SETTINGS_SCROLL_STEP_PX;

/// What one coordinate of the focused script claims, on the page it is
/// clicked on.
///
/// A separate table from [`SCRIPT_COORDS`] rather than a shared one: the two
/// scripts click the *same pixel* (`960,540`) on two different pages — a world
/// cell in the canonical run, the SETTINGS button here — and one table that
/// had to hold both meanings for one pixel would have to stop asserting.
#[derive(Debug)]
enum FocusedPoint {
    /// A world click that picks the building whose centre stands on this cell.
    Building(Cell),
    /// Owned by the HUD chrome, before any modal is open.
    Hud(HudHit),
    /// Owned by the paused Escape menu.
    PauseMenu(ModalHit),
    /// Owned by the settings panel, at the scroll offset the script clicks it
    /// at.
    Settings(ModalHit, f32),
}

/// Every screen coordinate the focused script names, with what it claims.
const FOCUSED_COORDS: &[([f32; 2], FocusedPoint, &str)] = &[
    (
        [960.0, 494.0],
        FocusedPoint::Building(Cell { x: 172, y: 172 }),
        "the HQ's own footprint corner (160.5, 160.5)",
    ),
    (
        [1888.0, 24.0],
        FocusedPoint::Hud(HudHit::Menu),
        "MENU_RECT centre",
    ),
    (
        [960.0, 540.0],
        FocusedPoint::PauseMenu(ModalHit::OpenSettings),
        "PAUSE_MENU_SETTINGS_BTN centre",
    ),
    (
        [568.0, 288.0],
        FocusedPoint::Settings(ModalHit::KeyboardPan(PAN_MIN), 0.0),
        "keyboard-pan track, its own left edge: where the live drag starts",
    ),
    (
        [1170.0, 288.0],
        FocusedPoint::Settings(ModalHit::KeyboardPan(SLIDER_PAN_SPEED), 0.0),
        "keyboard-pan track, at the 78 cells/s step: where the live drag ends",
    ),
    (
        [960.0, 580.0],
        FocusedPoint::Settings(ModalHit::Grid, 0.0),
        "GRID_CONTROL_RECT centre",
    ),
    (
        [552.0, 928.0],
        FocusedPoint::Settings(ModalHit::Back, FOCUSED_SCROLL_PX),
        "SETTINGS_BACK_BTN centre, clicked after the wheel notch",
    ),
    (
        [960.0, 620.0],
        FocusedPoint::PauseMenu(ModalHit::CloseMenu),
        "PAUSE_MENU_CLOSE_BTN centre",
    ),
];

fn focused_script_path() -> PathBuf {
    mmd_engine::workspace_root().join(FOCUSED_SCRIPT_REL)
}

/// Step 1 of the focused script, asserted as the *pick* it claims to be.
///
/// `find_builder` falls back to "any live worker" and `enqueue_unit` is given
/// a building id, so an accepted Worker enqueue alone would not prove the
/// click resolved to the HQ. This is that proof.
///
/// Before T7 the second half of this case also pinned *why* the script uses
/// the footprint corner rather than the HQ's screen centre: a T3-relocated
/// worker's sprite quad reached back over the centre and outranked the HQ on
/// pick depth. T7 moved the spawn row out of the enlarged footprint, so that
/// occlusion is gone and both points now pick the HQ — which this case says
/// outright rather than leaving the corner click unexplained.
#[test]
fn focused_script_hq_click_picks_the_building_not_a_worker() {
    let h = RtsHarness::scene().build().expect("rts scene harness");
    let view = h.world().iso_view();
    let hq = h.world().start_hq().expect("the scene's HQ");

    assert_eq!(
        pick_at(h.world(), &view, [960.0, 494.0]),
        Pick::Building(hq),
        "the focused script's step-1 coordinate does not pick the HQ"
    );
    assert_eq!(
        pick_at(h.world(), &view, [960.0, 540.0]),
        Pick::Building(hq),
        "since T7 no worker's sprite quad reaches back over the HQ's screen \
         centre, so it must pick the HQ too"
    );
}

/// The settings Back control is a *fixed footer*: it sits outside the
/// scrollable body viewport, so the wheel notch the script fires just before
/// clicking it must not move it.
#[test]
fn focused_script_back_button_is_fixed_under_scroll() {
    let back = [552.0, 928.0];
    assert!(
        back[1] >= HudLayout::SETTINGS_BODY_VIEWPORT[1] + HudLayout::SETTINGS_BODY_VIEWPORT[3],
        "the Back control is inside the scrollable body viewport, so it is not fixed"
    );
    for offset in [
        0.0,
        FOCUSED_SCROLL_PX,
        mmd_engine::rts::settings_max_scroll(),
    ] {
        assert_eq!(
            modal_hit_test(ModalPage::Settings, back, offset),
            ModalHit::Back,
            "Back stopped being hittable at scroll offset {offset}"
        );
    }
    // ...and the body really did move under it, or "fixed" is untested.
    assert_ne!(
        modal_hit_test(ModalPage::Settings, [960.0, 580.0], FOCUSED_SCROLL_PX),
        ModalHit::Grid,
        "one wheel notch did not move the settings body at all, so the fixed-footer \
         claim above is vacuous"
    );
    assert!(
        FOCUSED_SCROLL_PX > 0.0 && FOCUSED_SCROLL_PX <= mmd_engine::rts::settings_max_scroll(),
        "one wheel notch ({FOCUSED_SCROLL_PX}) does not land inside the scrollable range"
    );
}

/// Every screen coordinate the focused script uses must hit what it claims,
/// on the page it is clicked on — and the table and the script must cover each
/// other, so a coordinate cannot drift by being added rather than edited.
#[test]
fn the_focused_script_coordinates_hit_what_they_name() {
    let h = RtsHarness::scene().build().expect("rts scene harness");
    let view = h.world().iso_view();

    for (screen, claim, what) in FOCUSED_COORDS {
        match claim {
            FocusedPoint::Building(centre) => {
                let building = building_standing_on(&h, *centre);
                assert_eq!(
                    hud_hit_test(h.world(), *screen),
                    None,
                    "the focused script's {what} coordinate {screen:?} is a world click, \
                     but the HUD now owns it"
                );
                assert_eq!(
                    pick_at(h.world(), &view, *screen),
                    Pick::Building(building),
                    "the focused script's {what} coordinate {screen:?} no longer picks the \
                     building standing on {centre:?}"
                );
            }
            FocusedPoint::Hud(expected) => {
                assert_eq!(
                    hud_hit_test(h.world(), *screen),
                    Some(*expected),
                    "the focused script's {what} coordinate {screen:?} no longer hits \
                     {expected:?}"
                );
            }
            FocusedPoint::PauseMenu(expected) => {
                assert_eq!(
                    modal_hit_test(ModalPage::PauseMenu, *screen, 0.0),
                    *expected,
                    "the focused script's {what} coordinate {screen:?} no longer hits \
                     {expected:?} on the paused menu"
                );
            }
            FocusedPoint::Settings(expected, offset) => {
                assert_eq!(
                    modal_hit_test(ModalPage::Settings, *screen, *offset),
                    *expected,
                    "the focused script's {what} coordinate {screen:?} no longer hits \
                     {expected:?} at scroll offset {offset}"
                );
            }
        }
    }

    // The wheel target is not a control: it only has to be inside the body
    // viewport, which is the whole condition the app scrolls on.
    let wheel = [960.0, 540.0];
    let vp = HudLayout::SETTINGS_BODY_VIEWPORT;
    assert!(
        wheel[0] >= vp[0]
            && wheel[0] < vp[0] + vp[2]
            && wheel[1] >= vp[1]
            && wheel[1] < vp[1] + vp[3],
        "the focused script's wheel point {wheel:?} is outside SETTINGS_BODY_VIEWPORT {vp:?}, \
         so the notch would be ignored"
    );

    let text = std::fs::read_to_string(focused_script_path())
        .unwrap_or_else(|e| panic!("read {}: {e}", focused_script_path().display()));
    let used = coords_in(&text);
    assert!(
        !used.is_empty(),
        "{FOCUSED_SCRIPT_REL} names no coordinates at all"
    );
    for screen in &used {
        assert!(
            FOCUSED_COORDS.iter().any(|(s, _, _)| s == screen),
            "{FOCUSED_SCRIPT_REL} uses coordinate {screen:?}, which this test documents no \
             meaning for"
        );
    }
    for (screen, _, what) in FOCUSED_COORDS {
        assert!(
            used.contains(screen),
            "this test documents the {what} coordinate {screen:?}, which \
             {FOCUSED_SCRIPT_REL} no longer uses"
        );
    }
}

/// The grid is a *render* setting: toggling it changes what one frame packs
/// and nothing the world hashes.
///
/// `crates/mmd-engine/tests/rts_pack.rs` owns each half on its own (the exact
/// lattice, and that packing does not mutate the world). This is the joined
/// claim the focused script's `show_grid=false` exit token rests on: the same
/// world, packed both ways in one test, must differ in instance count and
/// agree on the hash.
#[test]
fn grid_toggle_changes_frame_not_world_hash() {
    let h = RtsHarness::scene().build().expect("rts scene harness");
    let before = h.state_hash_hex();
    let cursor = [960.0, 540.0];

    let mut frame = RtsFrame::new();
    pack_frame_with_options(
        h.world(),
        cursor,
        None,
        FramePackOptions { show_grid: true },
        &mut frame,
    );
    let with_grid = frame.instance_count();

    pack_frame_with_options(
        h.world(),
        cursor,
        None,
        FramePackOptions { show_grid: false },
        &mut frame,
    );
    let without_grid = frame.instance_count();

    assert!(
        with_grid > without_grid,
        "grid on packed {with_grid} instances and grid off {without_grid}: the setting \
         reaches no frame at all"
    );
    assert_eq!(
        h.state_hash_hex(),
        before,
        "packing a frame with the grid on or off moved hashed world state"
    );
}
