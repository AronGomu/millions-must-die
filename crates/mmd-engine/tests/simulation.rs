//! SoA movement + recycle contract (T5), driven through the T29 harness.
//!
//! Every test here goes through `mmd_engine::testkit::Harness` — the single
//! seeded, headless, clock-free entry point — rather than assembling a
//! scenario, flow field and simulation by hand. Unit-scale cases use the
//! harness's synthetic `GridSpec` source so they keep their surgical
//! assertions while still sharing that one entry point.

mod common;

use common::Tracker;
use mmd_engine::scenario::Cell;
use mmd_engine::sim::{ARRIVAL_RADIUS, AgentsView, SPEED_CELLS_PER_SEC, TICK_DT, quantize_cell};
use mmd_engine::testkit::{ALL_FIXTURES, FIXTURE_CORRIDOR_V1, FIXTURE_DENSE_V1, GridSpec, Harness};

fn assert_even(counts: &[u32], n: usize) {
    let sum: u32 = counts.iter().sum();
    assert_eq!(sum, n as u32);
    let base = (n / counts.len()) as u32;
    let rem = (n % counts.len()) as u32;
    // Perfect split, or floor/ceil when n not divisible.
    for &c in counts {
        assert!(
            c == base || c == base + 1,
            "count {c} base {base} rem {rem} counts={counts:?}"
        );
    }
    if rem == 0 {
        assert!(counts.iter().all(|&c| c == base));
    }
}

#[test]
fn tick_moves_eight_cells_per_second() {
    // Open corridor: dest east → unit vector (1,0).
    let dest = Cell { x: 15, y: 1 };
    let spec = GridSpec::new(16, 3, dest).with_spawns(vec![Cell { x: 1, y: 1 }]);
    let mut h = Harness::grid(spec).agents(1).build().expect("harness");

    let view = h.agents();
    let x0 = view.x[0];
    let y0 = view.y[0];
    assert!((x0 - 1.5).abs() < 1e-6);
    assert!((y0 - 1.5).abs() < 1e-6);

    let (vx, vy) = h.flow_field().vector_at(1, 1);
    assert!((vx - 1.0).abs() < 1e-5, "vx={vx}");
    assert!(vy.abs() < 1e-5, "vy={vy}");

    h.step_exact(1);

    let view = h.agents();
    let expect_x = x0 + SPEED_CELLS_PER_SEC * TICK_DT;
    assert!(
        (view.x[0] - expect_x).abs() < 1e-5,
        "x {} want {}",
        view.x[0],
        expect_x
    );
    assert!((view.y[0] - y0).abs() < 1e-5);
    assert!((SPEED_CELLS_PER_SEC * TICK_DT - 8.0 / 60.0).abs() < 1e-9);
}

#[test]
fn blocked_step_holds_position() {
    // Open grid; inject vector that steps into obstacle; position must hold.
    let w = 4u32;
    let dest = Cell { x: 3, y: 1 };
    let spec = GridSpec::new(w, 3, dest)
        .with_obstacles(vec![1 + w]) // (1,1)
        .with_spawns(vec![Cell { x: 0, y: 1 }]);
    let mut h = Harness::grid(spec).agents(1).build().expect("harness");

    // Sit in free cell (0,1); force +x unit vector into obstacle (1,1).
    h.sim_mut().set_position(0, 0.9, 1.5);
    h.sim_mut().set_vector_for_test(0, 1, 1.0, 0.0);

    let before = h.agents();
    let bx = before.x[0];
    let by = before.y[0];
    // next x = 0.9 + 8/60 ≈ 1.033 → nearest cell (1,1) blocked.
    h.step_exact(1);
    let after = h.agents();
    assert_eq!(after.x[0], bx, "blocked step must retain x");
    assert_eq!(after.y[0], by, "blocked step must retain y");
}

#[test]
fn arrival_radius_recycles() {
    let dest = Cell { x: 2, y: 2 };
    let spec =
        GridSpec::new(5, 5, dest).with_spawns(vec![Cell { x: 0, y: 0 }, Cell { x: 4, y: 4 }]);
    let mut h = Harness::grid(spec).agents(1).build().expect("harness");

    let cx = dest.x as f32 + 0.5;
    let cy = dest.y as f32 + 0.5;
    h.sim_mut().set_position(0, cx + 0.1, cy); // dist 0.1 ≤ 0.5
    assert!((ARRIVAL_RADIUS - 0.5).abs() < f32::EPSILON);

    h.step_exact(1);

    let view = h.agents();
    // recycle_cursor starts at agent_count (=1) → spawn[1 % 2] = (4,4)
    assert!(
        (view.x[0] - 4.5).abs() < 1e-5 && (view.y[0] - 4.5).abs() < 1e-5,
        "recycled to second spawn, got ({}, {})",
        view.x[0],
        view.y[0]
    );
    assert_eq!(h.recycled_count(), 1, "arrival must count as one recycle");
}

#[test]
fn population_stays_50000() {
    let mut h = Harness::gate_scene().build().expect("gate scene");
    assert_eq!(h.alive_count(), 50_000);

    h.step_exact(10_000);
    assert_eq!(h.alive_count(), 50_000);
    assert_eq!(h.agents().x.len(), 50_000);
}

#[test]
fn animation_uses_tick() {
    let dest = Cell { x: 15, y: 1 };
    let spec = GridSpec::new(16, 3, dest).with_spawns(vec![Cell { x: 1, y: 1 }]);
    let mut h = Harness::grid(spec).agents(8).build().expect("harness");

    let before: Vec<(u8, u8)> = {
        let v = h.agents();
        (0..v.dir.len()).map(|i| (v.dir[i], v.frame[i])).collect()
    };

    h.step_exact(1);

    let after = h.agents();
    let mut frame_changed = false;
    let mut moved = false;
    for (i, &(_bd, bf)) in before.iter().enumerate() {
        if after.frame[i] != bf {
            frame_changed = true;
        }
        if after.x[i] > 1.5 {
            moved = true;
        }
    }
    assert!(frame_changed, "frame must advance with tick");
    assert!(moved, "agents should have moved");
    let f1 = after.frame[0];
    h.step_exact(1);
    let f2 = h.agents().frame[0];
    assert_ne!(f1, f2, "frame advances each tick while active");
}

#[test]
fn cross_platform_quantized_drift_is_bounded() {
    let mut h = Harness::gate_scene().build().expect("gate scene");
    h.step_exact(600);

    let view = h.agents();
    for i in 0..view.x.len() {
        let qx = quantize_cell(view.x[i]);
        let qy = quantize_cell(view.y[i]);
        // Identity: same quantize twice → exact.
        assert_eq!(qx, quantize_cell(view.x[i]));
        assert_eq!(qy, quantize_cell(view.y[i]));
        // Reconstruct quantize: drift ≤1 quantum vs raw.
        let rx = qx as f32 * (1.0 / 256.0);
        let ry = qy as f32 * (1.0 / 256.0);
        let drift_x = (quantize_cell(view.x[i]) - quantize_cell(rx)).abs();
        let drift_y = (quantize_cell(view.y[i]) - quantize_cell(ry)).abs();
        assert!(drift_x <= 1, "x drift {drift_x}");
        assert!(drift_y <= 1, "y drift {drift_y}");
        assert!(view.dir[i] < 8);
        assert!(view.frame[i] < 4);
        assert!(view.atlas[i] < 4);
    }

    let mut a = Harness::gate_scene().build().expect("a");
    let mut b = Harness::gate_scene().build().expect("b");
    a.step_exact(120);
    b.step_exact(120);
    assert_eq!(a.state_hash(), b.state_hash());
    assert_eq!(a.quantized_state_hash(), b.quantized_state_hash());

    let va = a.agents();
    let vb = b.agents();
    for i in 0..va.x.len() {
        let dx = (quantize_cell(va.x[i]) - quantize_cell(vb.x[i])).abs();
        let dy = (quantize_cell(va.y[i]) - quantize_cell(vb.y[i])).abs();
        assert!(dx <= 1 && dy <= 1);
        assert_eq!(va.dir[i], vb.dir[i]);
        assert_eq!(va.frame[i], vb.frame[i]);
        assert_eq!(va.atlas[i], vb.atlas[i]);
    }
}

#[test]
fn atlas_dir_frame_evenly_distributed() {
    let h = Harness::gate_scene().build().expect("gate scene");
    let AgentsView {
        atlas, dir, frame, ..
    } = h.agents();
    let n = atlas.len();
    let mut ac = [0u32; 4];
    let mut dc = [0u32; 8];
    let mut fc = [0u32; 4];
    for i in 0..n {
        ac[atlas[i] as usize] += 1;
        dc[dir[i] as usize] += 1;
        fc[frame[i] as usize] += 1;
    }
    assert_even(&ac, n);
    assert_even(&dc, n);
    assert_even(&fc, n);
}

// --- T30 behaviour ----------------------------------------------------------
//
// The tests below assert what the horde *does*, never how fast it does it.
//
// Every constant here was read off the current systems on 2026-08-06 (see the
// per-test comments) rather than invented, and every run is deterministic, so
// the observed value is reproducible and the margin between it and the asserted
// bound is the honest amount of headroom.
//
// Observed on `fixture_dense_v1` (256 agents, 33.3% of the grid blocked):
//   first arrival tick 230; every one of the 4 spawn groups has an arrival by
//   tick 263; 100% of agents have recycled at least once by tick 300; 0
//   obstacle entries, 0 out-of-bounds samples, 0 agents that never moved.
// `fixture_corridor_v1` is the slow one: first arrival 565, all 200 agents
// recycled by tick 700.

/// Tick budget for the fixture-scale behaviour runs.
///
/// 400 ticks is ~1.33x the tick 300 at which the last dense-fixture agent has
/// recycled — enough headroom that a benign fixture or field tweak does not
/// break the suite, short enough that a real movement or recycle regression
/// (which drops the recycled fraction to 0) cannot hide inside it.
const BEHAVIOUR_TICKS: u64 = 400;

#[test]
fn agents_reach_destination() {
    // Obstacle-dense fixture: no straight-line route exists, so arriving at all
    // means the flow field routed the agents around the pillars.
    let mut h = Harness::fixture(FIXTURE_DENSE_V1).build().expect("dense");
    let n = h.alive_count();
    let mut t = Tracker::new(&h);
    t.run(&mut h, BEHAVIOUR_TICKS);

    // Observed: 100% by tick 300. Asserting 90% keeps a margin for benign field
    // changes while still failing outright on a broken recycle or a horde that
    // stops short of the destination.
    let frac = t.recycled_fraction();
    assert!(
        frac >= 0.90,
        "only {:.1}% of {n} agents reached the destination in {BEHAVIOUR_TICKS} ticks \
         (observed 100% by tick 300); recycles={:?}..",
        100.0 * frac,
        &t.recycles()[..8.min(n)]
    );

    // The sim's own counters must agree with what the run was observed to do —
    // recycles are counted here geometrically (a jump no walk can produce), so
    // this is an independent check, not a restatement of the accessors.
    assert_eq!(
        h.recycled_count(),
        t.observed_recycles(),
        "recycle counter disagrees with observed respawns"
    );
    assert_eq!(
        h.spawned_count(),
        n as u64 + t.observed_recycles(),
        "spawn events must be the initial seeding plus every recycle"
    );
    assert_eq!(
        h.alive_count(),
        n,
        "recycling must not change the population"
    );
}

#[test]
fn obstacles_are_never_entered() {
    let mut h = Harness::fixture(FIXTURE_DENSE_V1).build().expect("dense");
    let scenario = h.scenario();
    let cells = scenario.width() * scenario.height();
    let ratio = f64::from(scenario.obstacle_count()) / f64::from(cells);
    // Guard against the test quietly becoming vacuous if the fixture is ever
    // edited: an "agents avoid walls" claim proves nothing on an open field.
    assert!(
        ratio >= 0.30,
        "fixture is only {:.1}% blocked — not an obstacle-dense case",
        100.0 * ratio
    );

    let mut t = Tracker::new(&h);
    t.run(&mut h, BEHAVIOUR_TICKS);
    assert_eq!(
        t.obstacle_samples(),
        0,
        "agents stood inside an obstacle cell during the run"
    );
    assert!(
        common::agents_in_obstacles(&h).is_empty(),
        "agents ended the run inside an obstacle cell"
    );

    // The run above only proves the *field* never aims at a wall. The movement
    // step owes its own guarantee, so drive the same fixture with a hostile
    // field: every cell pushes south, straight into the pillar rows (spawns sit
    // on the open rows y % 3 == 0, and the row below each is blocked wherever
    // x % 4 is 1 or 2). Agents must refuse the step instead of walking in.
    let mut hostile = Harness::fixture(FIXTURE_DENSE_V1).build().expect("dense");
    let (w, ht) = (hostile.scenario().width(), hostile.scenario().height());
    for y in 0..ht {
        for x in 0..w {
            hostile.sim_mut().set_vector_for_test(x, y, 0.0, 1.0);
        }
    }
    let mut ht_tracker = Tracker::new(&hostile);
    ht_tracker.run(&mut hostile, 100);
    assert_eq!(
        ht_tracker.obstacle_samples(),
        0,
        "a field pointing into the walls walked agents into obstacle cells"
    );
    assert_eq!(
        ht_tracker.bounds_violations(),
        0,
        "a field pointing off the map walked agents out of the world"
    );
}

#[test]
fn no_agent_is_stuck_against_an_obstacle() {
    // The failure this guards: an agent wedged into a corner whose descent
    // vector points at a blocked cell holds position forever, because the field
    // is static and nothing perturbs it.
    let mut h = Harness::fixture(FIXTURE_DENSE_V1).build().expect("dense");
    let mut t = Tracker::new(&h);
    t.run(&mut h, BEHAVIOUR_TICKS);

    let stuck = t.never_moved();
    assert!(
        stuck.is_empty(),
        "{} of {} agents never moved in {BEHAVIOUR_TICKS} ticks: {:?}",
        stuck.len(),
        t.agent_count(),
        &stuck[..stuck.len().min(8)]
    );
}

#[test]
fn aggregate_progress_is_monotone() {
    // Per-agent progress is not monotone (an agent rounding a pillar can land
    // in a costlier cell for a tick), but the horde's mean routing cost must
    // fall every tick until the first arrival is teleported back to a spawn.
    // Per-fixture budgets: each must be long enough to contain that fixture's
    // first arrival (dense 230, corridor 565), so the "cost fell all the way to
    // an arrival" claim below is real rather than a truncated run.
    for (name, ticks) in [(FIXTURE_DENSE_V1, 400u64), (FIXTURE_CORRIDOR_V1, 700)] {
        let mut h = Harness::fixture(name).build().expect("fixture");
        let mut t = Tracker::new(&h);
        t.run(&mut h, ticks);

        let increases = t.mean_cost_increases();
        assert!(
            increases.is_empty(),
            "{name}: mean routing cost rose on {} ticks before the first arrival: {:?}",
            increases.len(),
            &increases[..increases.len().min(4)]
        );
        // Without this the assertion above passes trivially on a horde that
        // never moves (a flat series has no increases either).
        let series = t.mean_cost_series();
        let start = series[0];
        let lowest = series.iter().copied().fold(f64::INFINITY, f64::min);
        assert!(
            lowest < start,
            "{name}: mean routing cost never fell below its start ({start} → {lowest})"
        );
        assert!(
            t.first_recycle_tick().is_some(),
            "{name}: no agent arrived within {ticks} ticks"
        );
    }
}

#[test]
fn aggregate_progress_never_stalls() {
    // The weaker sibling of `aggregate_progress_is_monotone`, and deliberately
    // kept alongside it rather than replacing it. Monotonicity is a claim about
    // the tracked fixtures' current body radius (1/8 cell, so a spawn stack
    // opens without moving anyone into a costlier cell); this claim survives a
    // body large enough to push agents backwards for a few ticks. If a future
    // tuning breaks monotonicity, that test is supposed to fail loudly — and
    // this one is what still holds the floor afterwards.
    const STALL_BUDGET_TICKS: u64 = 60; // 1 s at 60 Hz
    for (name, ticks) in [(FIXTURE_DENSE_V1, 400u64), (FIXTURE_CORRIDOR_V1, 700)] {
        let mut h = Harness::fixture(name).build().expect("fixture");
        let mut t = Tracker::new(&h);
        t.run(&mut h, ticks);

        // The stall window is `first_recycle_tick - 1` ticks long. If a future
        // change collapses it, `longest_progress_stall` returns 0 and the
        // budget below would be satisfied by a horde that did nothing.
        let first = t
            .first_recycle_tick()
            .unwrap_or_else(|| panic!("{name}: no agent arrived within {ticks} ticks"));
        // `+ 2`, not `+ 0`: the helper scans `1..(first - 1)`, so the largest
        // run it can report is `first - 2`. A window of exactly the budget
        // would make the assertion below unfalsifiable rather than merely
        // tight.
        assert!(
            first > STALL_BUDGET_TICKS + 2,
            "{name}: only {first} ticks before the first arrival — a stall budget \
             of {STALL_BUDGET_TICKS} cannot be measured in that window"
        );

        let stall = t.longest_progress_stall();
        assert!(
            stall <= STALL_BUDGET_TICKS,
            "{name}: the horde closed no distance for {stall} consecutive ticks \
             (budget {STALL_BUDGET_TICKS})"
        );
        // Without this the assertion above passes trivially on a horde that
        // never moves. Scoped to the same pre-recycle window the stall is
        // measured over, so a late post-recycle dip cannot satisfy it.
        let series = &t.mean_cost_series()[..(first as usize - 1).min(t.mean_cost_series().len())];
        let start = series[0];
        let lowest = series.iter().copied().fold(f64::INFINITY, f64::min);
        assert!(
            lowest < start,
            "{name}: mean routing cost never fell below its start ({start} → {lowest})"
        );
    }
}

#[test]
fn no_group_is_starved() {
    // fixture_dense_v1 has exactly four west-edge spawn groups; each must both
    // close routing distance and land arrivals. Observed: every group's mean
    // progress is 1.000 (all members reached the destination) and every group
    // has its first arrival by tick 263.
    let mut h = Harness::fixture(FIXTURE_DENSE_V1).build().expect("dense");
    let mut t = Tracker::new(&h);
    assert_eq!(
        t.group_count(),
        4,
        "this test is about the four spawn groups"
    );
    t.run(&mut h, BEHAVIOUR_TICKS);

    for g in 0..t.group_count() {
        let progress = t.group_mean_progress(g);
        assert!(
            progress > 0.0,
            "group {g} at spawn {:?} made no progress toward the destination",
            t.spawn_cell(g)
        );
        assert!(
            t.group_recycles(g) > 0,
            "group {g} at spawn {:?} landed no arrival in {BEHAVIOUR_TICKS} ticks \
             (observed first arrival by tick 263)",
            t.spawn_cell(g)
        );
    }
}

#[test]
fn positions_finite_and_in_bounds() {
    // Every tracked fixture, sampled every tick, plus a bounded sweep of the
    // real 50k workload.
    for name in ALL_FIXTURES {
        let mut h = Harness::fixture(*name).build().expect("fixture");
        common::assert_positions_finite_and_in_bounds(&h, name);
        let mut t = Tracker::new(&h);
        t.run(&mut h, BEHAVIOUR_TICKS);
        assert_eq!(
            t.bounds_violations(),
            0,
            "{name}: agents left the world rect or went non-finite during the run"
        );
        common::assert_positions_finite_and_in_bounds(&h, name);
    }

    let mut h = Harness::gate_scene().build().expect("gate scene");
    for _ in 0..3 {
        h.step_exact(100);
        common::assert_positions_finite_and_in_bounds(&h, "gate scene");
    }
}

#[test]
fn alive_count_is_stable() {
    // 50k agents, 1000 ticks, checked every single tick — a population that
    // dipped for one frame would be invisible to an end-of-run assertion.
    let mut h = Harness::gate_scene().build().expect("gate scene");
    let configured = h.scenario().hard_agent_count() as usize;
    assert_eq!(h.alive_count(), configured);

    for tick in 1..=1_000u64 {
        h.step_exact(1);
        assert_eq!(
            h.alive_count(),
            configured,
            "population changed on tick {tick}"
        );
        assert_eq!(
            h.spawned_count(),
            configured as u64 + h.recycled_count(),
            "spawn/recycle counters stopped balancing on tick {tick}"
        );
    }

    // The gate scene spawns on the west edge (x=2) and its destination is at
    // x=240, so the shortest route is ~238 cells: at 8 cells/s that is ~1785
    // ticks. Within this 1000-tick budget no agent can have arrived yet, so a
    // nonzero recycle count would mean the arrival test is firing spuriously.
    assert_eq!(
        h.recycled_count(),
        0,
        "no agent can reach the destination within 1000 ticks of the gate scene"
    );
    assert_eq!(h.agents().x.len(), configured);
}

#[test]
fn determinism_holds_for_50k_agents() {
    // Bounded on purpose: 300 ticks is enough for 50k agents to spread out over
    // the field (they start stacked on 127 spawn cells and diverge immediately),
    // and keeps the whole-suite cost of this check to a few seconds.
    const TICKS: u64 = 300;

    let mut a = Harness::gate_scene().build().expect("a");
    let mut b = Harness::gate_scene().build().expect("b");
    assert_eq!(a.alive_count(), 50_000);
    a.step_exact(TICKS);
    b.step_exact(TICKS);
    assert_eq!(
        a.state_hash_hex(),
        b.state_hash_hex(),
        "two 50k runs of the same scenario diverged"
    );
    assert_eq!(a.quantized_state_hash(), b.quantized_state_hash());

    // And the hash actually tracks the run: a differently seeded 50k horde must
    // not land on the same digest, or the comparison above proves nothing.
    let mut seeded = Harness::gate_scene().seed(7).build().expect("seeded");
    seeded.step_exact(TICKS);
    assert_ne!(
        seeded.state_hash_hex(),
        a.state_hash_hex(),
        "a reseeded 50k run must not match the canonical one"
    );
}
