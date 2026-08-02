//! SoA movement + recycle contract (T5).

use std::path::PathBuf;

use mmd_engine::nav::flow_field::FlowField;
use mmd_engine::scenario::{Cell, Scenario};
use mmd_engine::sim::{
    ARRIVAL_RADIUS, AgentsView, SPEED_CELLS_PER_SEC, Simulation, TICK_DT, quantize_cell,
};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root")
}

fn load_v1() -> (Scenario, FlowField) {
    let path = workspace_root().join("assets/scenarios/technical_prototype_v1.ron");
    let scenario = Scenario::load_verified(&path).expect("load scenario");
    let field = FlowField::from_scenario(&scenario);
    (scenario, field)
}

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
    let w = 16u32;
    let h = 3u32;
    let dest = Cell { x: 15, y: 1 };
    let field = FlowField::build(w, h, dest, &[]).expect("field");
    let spawns = [Cell { x: 1, y: 1 }];
    let mut sim = Simulation::new_custom(&field, dest, &spawns, 1, 4, 8, 4);

    let view = sim.agents();
    let x0 = view.x[0];
    let y0 = view.y[0];
    assert!((x0 - 1.5).abs() < 1e-6);
    assert!((y0 - 1.5).abs() < 1e-6);

    let (vx, vy) = field.vector_at(1, 1);
    assert!((vx - 1.0).abs() < 1e-5, "vx={vx}");
    assert!(vy.abs() < 1e-5, "vy={vy}");

    sim.tick();

    let view = sim.agents();
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
    let h = 3u32;
    let dest = Cell { x: 3, y: 1 };
    let obstacles = [1 + w]; // (1,1)
    let field = FlowField::build(w, h, dest, &obstacles).expect("field");
    let spawns = [Cell { x: 0, y: 1 }];
    let mut sim = Simulation::new_custom(&field, dest, &spawns, 1, 4, 8, 4);

    // Sit in free cell (0,1); force +x unit vector into obstacle (1,1).
    sim.set_position(0, 0.9, 1.5);
    sim.set_vector_for_test(0, 1, 1.0, 0.0);

    let before = sim.agents();
    let bx = before.x[0];
    let by = before.y[0];
    // next x = 0.9 + 8/60 ≈ 1.033 → nearest cell (1,1) blocked.
    sim.tick();
    let after = sim.agents();
    assert_eq!(after.x[0], bx, "blocked step must retain x");
    assert_eq!(after.y[0], by, "blocked step must retain y");
}

#[test]
fn arrival_radius_recycles() {
    let w = 5u32;
    let h = 5u32;
    let dest = Cell { x: 2, y: 2 };
    let field = FlowField::build(w, h, dest, &[]).expect("field");
    let spawns = [Cell { x: 0, y: 0 }, Cell { x: 4, y: 4 }];
    let mut sim = Simulation::new_custom(&field, dest, &spawns, 1, 4, 8, 4);

    let cx = dest.x as f32 + 0.5;
    let cy = dest.y as f32 + 0.5;
    sim.set_position(0, cx + 0.1, cy); // dist 0.1 ≤ 0.5
    assert!((ARRIVAL_RADIUS - 0.5).abs() < f32::EPSILON);

    sim.tick();

    let view = sim.agents();
    // recycle_cursor starts at agent_count (=1) → spawn[1 % 2] = (4,4)
    assert!(
        (view.x[0] - 4.5).abs() < 1e-5 && (view.y[0] - 4.5).abs() < 1e-5,
        "recycled to second spawn, got ({}, {})",
        view.x[0],
        view.y[0]
    );
}

#[test]
fn population_stays_50000() {
    let (scenario, field) = load_v1();
    let mut sim = Simulation::new(&scenario, &field);
    assert_eq!(sim.agent_count(), 50_000);

    for _ in 0..10_000 {
        sim.tick();
    }
    assert_eq!(sim.agent_count(), 50_000);
    assert_eq!(sim.agents().x.len(), 50_000);
}

#[test]
fn animation_uses_tick() {
    let w = 16u32;
    let h = 3u32;
    let dest = Cell { x: 15, y: 1 };
    let field = FlowField::build(w, h, dest, &[]).expect("field");
    let spawns = [Cell { x: 1, y: 1 }];
    let mut sim = Simulation::new_custom(&field, dest, &spawns, 8, 4, 8, 4);

    let before: Vec<(u8, u8)> = {
        let v = sim.agents();
        (0..v.dir.len()).map(|i| (v.dir[i], v.frame[i])).collect()
    };

    sim.tick();

    let after = sim.agents();
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
    sim.tick();
    let f2 = sim.agents().frame[0];
    assert_ne!(f1, f2, "frame advances each tick while active");
}

#[test]
fn cross_platform_quantized_drift_is_bounded() {
    let (scenario, field) = load_v1();
    let mut sim = Simulation::new(&scenario, &field);
    for _ in 0..600 {
        sim.tick();
    }

    let view = sim.agents();
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

    let mut a = Simulation::new(&scenario, &field);
    let mut b = Simulation::new(&scenario, &field);
    for _ in 0..120 {
        a.tick();
        b.tick();
    }
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
    let (scenario, field) = load_v1();
    let sim = Simulation::new(&scenario, &field);
    let AgentsView {
        atlas, dir, frame, ..
    } = sim.agents();
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
