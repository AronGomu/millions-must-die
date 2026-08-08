//! Tests for agent-agent soft separation: `mmd_engine::sim::SpatialGrid`, the
//! deterministic zero-alloc neighbour index it scans, and the steering blend
//! the tick applies on top of the flow field.
//!
//! Separation is *steering*, not resolution. Nothing here asserts that agents
//! stop overlapping — only that they push, that the push is reproducible, and
//! that bending a heading never changes how far an agent walks.

use mmd_engine::scenario::Cell;
use mmd_engine::sim::{
    CollisionParams, MAX_SEPARATION_NEIGHBORS, SEPARATION_DIR16, SPEED_CELLS_PER_SEC, SpatialGrid,
    TICK_DT, accumulate_separation,
};
use mmd_engine::testkit::{FIXTURE_DENSE_V1, GridSpec, Harness};

#[test]
fn spatial_bins_hold_every_agent_exactly_once() {
    let mut grid = SpatialGrid::new(8, 8, 1.0, 8);
    let xs = [0.5f32, 0.5, 7.5, 3.5, 7.5];
    let ys = [0.5f32, 0.5, 0.5, 3.5, 7.5];
    grid.rebuild(&xs, &ys);

    assert_eq!(grid.len(), 5);

    let mut seen: Vec<u32> = Vec::new();
    for by in 0..grid.rows() {
        for bx in 0..grid.cols() {
            seen.extend_from_slice(grid.agents_in_bin(bx, by));
        }
    }
    seen.sort_unstable();
    assert_eq!(seen, vec![0, 1, 2, 3, 4]);

    assert_eq!(grid.agents_in_bin(0, 0), &[0, 1]);
}

#[test]
fn spatial_bucket_order_is_ascending_agent_index() {
    let mut grid = SpatialGrid::new(4, 4, 1.0, 32);
    let xs = [1.5f32; 32];
    let ys = [1.5f32; 32];
    grid.rebuild(&xs, &ys);

    let expected: Vec<u32> = (0..32).collect();
    assert_eq!(grid.agents_in_bin(1, 1), expected.as_slice());
}

#[test]
fn spatial_bin_size_covers_two_radii() {
    let grid = SpatialGrid::new(480, 270, 7.5, 16);
    assert_eq!(grid.bin_size_cells(), 7.5);
    assert_eq!(grid.cols(), 64);
    assert_eq!(grid.rows(), 36);
}

#[test]
fn spatial_bin_size_never_drops_below_one_cell() {
    let grid = SpatialGrid::new(16, 16, 0.25, 4);
    assert_eq!(grid.bin_size_cells(), 1.0);
    assert_eq!(grid.cols(), 16);
    assert_eq!(grid.rows(), 16);
}

#[test]
fn spatial_clamps_positions_outside_the_world() {
    let mut grid = SpatialGrid::new(4, 4, 1.0, 3);
    let xs = [-3.0f32, 99.0, f32::NAN];
    let ys = [-3.0f32, 99.0, 0.5];
    grid.rebuild(&xs, &ys);

    assert_eq!(grid.bin_of(-3.0, -3.0), (0, 0));
    assert_eq!(grid.bin_of(99.0, 99.0), (3, 3));
    assert_eq!(grid.bin_of(f32::NAN, 0.5), (0, 0));

    let mut seen: Vec<u32> = Vec::new();
    for by in 0..grid.rows() {
        for bx in 0..grid.cols() {
            seen.extend_from_slice(grid.agents_in_bin(bx, by));
        }
    }
    seen.sort_unstable();
    assert_eq!(seen, vec![0, 1, 2]);
}

#[test]
fn spatial_rebuild_is_repeatable() {
    let mut grid = SpatialGrid::new(8, 8, 1.0, 8);
    let xs = [0.5f32, 0.5, 7.5, 3.5, 7.5];
    let ys = [0.5f32, 0.5, 0.5, 3.5, 7.5];

    grid.rebuild(&xs, &ys);
    let mut first: Vec<u32> = Vec::new();
    for by in 0..grid.rows() {
        for bx in 0..grid.cols() {
            first.extend_from_slice(grid.agents_in_bin(bx, by));
        }
    }

    grid.rebuild(&xs, &ys);
    let mut second: Vec<u32> = Vec::new();
    for by in 0..grid.rows() {
        for bx in 0..grid.cols() {
            second.extend_from_slice(grid.agents_in_bin(bx, by));
        }
    }

    assert_eq!(first, second);
}

// --- the repulsion accumulator -------------------------------------------

#[test]
fn separation_of_a_pair_is_equal_and_opposite() {
    // No direction exists between two identical points, so the pair falls to
    // the tie-break table. It must hand the two members opposite vectors, or a
    // stack would drift as a body instead of coming apart.
    let xs = [2.5f32, 2.5];
    let ys = [2.5f32, 2.5];
    let mut grid = SpatialGrid::new(8, 8, 1.0, 2);
    grid.rebuild(&xs, &ys);

    let mut sep_x = [0.0f32; 2];
    let mut sep_y = [0.0f32; 2];
    accumulate_separation(&xs, &ys, &grid, 0.5, &mut sep_x, &mut sep_y);

    assert_eq!(sep_x[0], -sep_x[1], "x push must be exactly opposite");
    assert_eq!(sep_y[0], -sep_y[1], "y push must be exactly opposite");
    assert!(
        sep_x[0] != 0.0 || sep_y[0] != 0.0,
        "coincident agents produced no push at all ({}, {})",
        sep_x[0],
        sep_y[0]
    );
}

#[test]
fn separation_is_capped_at_eight_neighbours() {
    // 64 agents on one coordinate is the spawn-stack shape in miniature. Every
    // contribution is a unit vector, so the cap is what bounds the sum — and
    // the sum must still be a real direction for every member, not a set of
    // pushes that cancel to nothing.
    const N: usize = 64;
    let xs = [2.5f32; N];
    let ys = [2.5f32; N];
    let mut grid = SpatialGrid::new(8, 8, 1.0, N);
    grid.rebuild(&xs, &ys);

    let mut sep_x = [0.0f32; N];
    let mut sep_y = [0.0f32; N];
    accumulate_separation(&xs, &ys, &grid, 0.5, &mut sep_x, &mut sep_y);

    let bound = MAX_SEPARATION_NEIGHBORS as f32 + 1e-4;
    for i in 0..N {
        let mag = sep_x[i].hypot(sep_y[i]);
        assert!(
            mag > 0.0 && mag <= bound,
            "agent {i}: push magnitude {mag} outside (0, {bound}]"
        );
    }

    // The bound above is theoretical and loose enough that a cap of 9, 10 or 24
    // would also satisfy it. Pin the count exactly on an agent whose neighbour
    // set is unambiguous: for any `i >= MAX_SEPARATION_NEIGHBORS` the first
    // agents scanned are `j = 0..cap`, every one of them below `i`, so every
    // sign is negative. Exact `==` is right here — identical addends in
    // identical order, no reassociation possible.
    let last = N - 1;
    let (mut ex, mut ey) = (0.0f32, 0.0f32);
    for j in 0..MAX_SEPARATION_NEIGHBORS {
        let (ux, uy) = SEPARATION_DIR16[(last ^ j) & 15];
        ex -= ux;
        ey -= uy;
    }
    assert_eq!(
        (sep_x[last], sep_y[last]),
        (ex, ey),
        "agent {last} must sum exactly {MAX_SEPARATION_NEIGHBORS} neighbours"
    );
}

#[test]
fn separation_ignores_agents_beyond_contact() {
    // Contact is two radii = 1.0 cell. Agent 0 stands alone; agents 1 and 2 are
    // 0.4 apart and therefore touching. One call covers both branches, so the
    // test can tell "correctly ignores the distant one" apart from "never
    // pushes at all".
    //
    // The buffers are pre-seeded with 1.0: the accumulator must *write* zero
    // rather than leave the caller's slot alone, or a push from a previous tick
    // would survive into this one.
    let xs = [1.5f32, 5.5, 5.9];
    let ys = [1.5f32, 1.5, 1.5];
    let mut grid = SpatialGrid::new(8, 8, 1.0, 3);
    grid.rebuild(&xs, &ys);

    let mut sep_x = [1.0f32; 3];
    let mut sep_y = [1.0f32; 3];
    accumulate_separation(&xs, &ys, &grid, 0.5, &mut sep_x, &mut sep_y);

    assert_eq!(
        (sep_x[0], sep_y[0]),
        (0.0, 0.0),
        "an agent 4 cells from anyone must be pushed by nobody"
    );
    assert!(
        sep_x[1] != 0.0,
        "agents inside contact must push: got {}",
        sep_x[1]
    );
    assert_eq!(sep_x[1], -sep_x[2], "and they must push each other apart");
}

// --- the blend, through the tick -----------------------------------------

/// A 32x32 open grid whose agents all spawn on one cell with a half-cell body
/// (`128` in 1/256 units), so every run starts as a perfectly stacked column.
fn stacked_collision_grid(agents: u32) -> GridSpec {
    GridSpec::new(32, 32, Cell { x: 31, y: 16 })
        .with_spawns(vec![Cell { x: 1, y: 16 }])
        .with_collision(128, 256)
        .with_agents(agents)
}

/// Mean distance over every unordered pair — 0.0 for a perfect stack, and it
/// grows as the stack opens. A *statistical* measure on purpose: separation
/// bounds overlap, it does not forbid it.
fn mean_pairwise_distance(h: &Harness) -> f32 {
    let v = h.agents();
    let n = v.x.len();
    let mut sum = 0.0f32;
    let mut pairs = 0u32;
    for i in 0..n {
        for j in (i + 1)..n {
            sum += (v.x[i] - v.x[j]).hypot(v.y[i] - v.y[j]);
            pairs += 1;
        }
    }
    assert!(pairs > 0, "need at least two agents to measure a spread");
    sum / pairs as f32
}

#[test]
fn coincident_agents_separate_on_the_first_tick() {
    let mut h = Harness::grid(stacked_collision_grid(2))
        .build()
        .expect("collision grid");

    {
        let v = h.agents();
        assert_eq!(
            (v.x[0], v.y[0]),
            (v.x[1], v.y[1]),
            "both agents must start on the same point"
        );
    }

    h.step_exact(1);

    let v = h.agents();
    let apart = (v.x[0] - v.x[1]).abs() + (v.y[0] - v.y[1]).abs();
    assert!(
        apart > 1e-4,
        "two coincident agents walked as one (L1 gap {apart})"
    );
}

#[test]
fn separation_is_reproducible() {
    // Three spawn cells, because a seeded harness only redistributes agents
    // *across* spawn cells — with one cell the seeded run would be identical by
    // construction and the inequality below would prove nothing.
    let spec = GridSpec::new(32, 32, Cell { x: 31, y: 16 })
        .with_spawns(vec![
            Cell { x: 1, y: 10 },
            Cell { x: 1, y: 16 },
            Cell { x: 1, y: 22 },
        ])
        .with_collision(128, 256)
        .with_agents(32);

    let run = |seed: u64| {
        let mut h = Harness::grid(spec.clone())
            .seed(seed)
            .build()
            .expect("collision grid");
        h.step_exact(120);
        h.state_hash()
    };

    assert_eq!(
        run(0),
        run(0),
        "the same inputs must produce the same state: separation reads no clock \
         and no RNG"
    );
    assert_ne!(
        run(0),
        run(7),
        "a different spawn placement must produce a different state"
    );

    // Both assertions above hold with separation switched off, so neither shows
    // the pass is running. Pin that it is.
    let flow_only = {
        let mut h = Harness::grid(spec.clone().with_collision(0, 0))
            .seed(0)
            .build()
            .expect("bodyless grid");
        h.step_exact(120);
        h.state_hash()
    };
    assert_ne!(
        run(0),
        flow_only,
        "separation must change the run it is enabled on"
    );
}

#[test]
fn separation_keeps_the_step_length() {
    // The blend bends the heading; it must not touch the speed.
    let mut h = Harness::grid(stacked_collision_grid(8))
        .build()
        .expect("collision grid");

    let before: Vec<(f32, f32)> = {
        let v = h.agents();
        (0..v.x.len()).map(|i| (v.x[i], v.y[i])).collect()
    };

    h.step_exact(1);

    let step_len = SPEED_CELLS_PER_SEC * TICK_DT;
    let v = h.agents();
    let mut walked = 0usize;
    for (i, &(bx, by)) in before.iter().enumerate() {
        let d = (v.x[i] - bx).hypot(v.y[i] - by);
        // A jump no walk could produce is a recycle, not a step.
        if d > step_len * 2.0 {
            continue;
        }
        assert!(
            (d - step_len).abs() < 1e-4,
            "agent {i} covered {d} cells, expected {step_len}"
        );
        walked += 1;
    }
    assert_eq!(
        walked,
        before.len(),
        "a one-tick run cannot recycle; every agent must have been measured"
    );

    // Speed being preserved is only interesting if a heading was actually bent.
    // These 8 agents are coincident, so they share one flow vector — distinct
    // displacements can only come from separation.
    let deltas: Vec<(i32, i32)> = before
        .iter()
        .enumerate()
        .map(|(i, &(bx, by))| (((v.x[i] - bx) * 1e4) as i32, ((v.y[i] - by) * 1e4) as i32))
        .collect();
    assert!(
        deltas.iter().any(|d| *d != deltas[0]),
        "every agent walked an identical heading, so no bend was measured"
    );
}

#[test]
fn a_released_stack_spreads_apart() {
    let mut h = Harness::grid(stacked_collision_grid(16))
        .build()
        .expect("collision grid");

    assert_eq!(
        mean_pairwise_distance(&h),
        0.0,
        "the stack must start perfectly coincident"
    );

    h.step_exact(90);

    {
        let v = h.agents();
        for i in 0..v.x.len() {
            assert!(
                v.x[i].is_finite() && v.y[i].is_finite(),
                "agent {i} left the numeric domain at ({}, {})",
                v.x[i],
                v.y[i]
            );
        }
    }

    let spread = mean_pairwise_distance(&h);
    assert!(
        spread > 0.5,
        "the stack barely opened: mean pairwise distance {spread}"
    );
}

/// State hash of the grid below after 200 ticks, **measured on the commit
/// before separation existed**. This is the whole content of the "a zero-radius
/// scenario is bit-identical to the old flow-only path" requirement: comparing
/// two bodyless configurations to each other proves nothing, because both walk
/// the same new code. Only a digest recorded before the change can fail when
/// that path drifts.
const BODYLESS_GRID_PRE_SEPARATION_HASH: &str =
    "e110a2bfba692f92ce6f2924991b881efa5075c6a72dce9b08fa9b984245ff85";

#[test]
fn a_bodyless_scenario_walks_the_flow_only_path() {
    let base = GridSpec::new(24, 24, Cell { x: 23, y: 12 })
        .with_spawns(vec![Cell { x: 1, y: 12 }])
        .with_agents(32);

    // Declaring `0 / 0` explicitly must be indistinguishable from not declaring
    // a body — note this pair alone is a weak claim, since `GridSpec` defaults
    // both fields to 0 and the two specs are equal by construction.
    let mut implicit = Harness::grid(base.clone()).build().expect("implicit");
    let mut explicit = Harness::grid(base.clone().with_collision(0, 0))
        .build()
        .expect("explicit");

    assert_eq!(implicit.sim().collision(), CollisionParams::NONE);
    assert_eq!(explicit.sim().collision(), CollisionParams::NONE);
    assert!(!implicit.sim().collision().enabled());
    assert!(!explicit.sim().collision().enabled());

    implicit.step_exact(200);
    explicit.step_exact(200);
    assert_eq!(implicit.state_hash(), explicit.state_hash());

    // The load-bearing assertion: unchanged against the pre-separation engine.
    assert_eq!(
        implicit.state_hash_hex(),
        BODYLESS_GRID_PRE_SEPARATION_HASH,
        "a bodyless scenario no longer walks the flow-only path it walked \
         before separation existed"
    );

    // ...and the switch is real: the same grid with a body must diverge, or the
    // assertion above would be satisfied by a feature that does nothing.
    let mut bodied = Harness::grid(base.with_collision(128, 256))
        .build()
        .expect("bodied");
    assert!(bodied.sim().collision().enabled());
    bodied.step_exact(200);
    assert_ne!(
        implicit.state_hash(),
        bodied.state_hash(),
        "declaring a body must change the walk"
    );
}

#[test]
fn separation_never_wedges_an_agent_against_a_wall() {
    // The requirement the flow-only fallback in `tick::step` exists for. Every
    // other collision test here runs on open ground, so without this the new
    // "blended step blocked -> retry the pure descent step" branch has no
    // coverage at all.
    //
    // A wall one cell wide with a single gap, a stack of bodied agents spawned
    // right against it: the crowd pushes members sideways into the wall while
    // the field points through the gap, which is exactly the configuration
    // where a blended step is unwalkable but the descent step is not.
    const W: u32 = 32;
    const GAP_Y: u32 = 16;
    let wall: Vec<u32> = (0..W).filter(|y| *y != GAP_Y).map(|y| 3 + y * W).collect();

    let spec = GridSpec::new(W, 32, Cell { x: 31, y: GAP_Y })
        .with_obstacles(wall)
        .with_spawns(vec![Cell { x: 1, y: GAP_Y }])
        .with_collision(128, 256)
        .with_agents(24);
    let mut h = Harness::grid(spec).build().expect("walled collision grid");
    assert!(h.sim().collision().enabled());

    let start: Vec<(f32, f32)> = {
        let v = h.agents();
        (0..v.x.len()).map(|i| (v.x[i], v.y[i])).collect()
    };

    h.step_exact(200);

    let v = h.agents();
    for (i, &(sx, sy)) in start.iter().enumerate() {
        assert!(
            v.x[i].is_finite() && v.y[i].is_finite(),
            "agent {i} left the numeric domain"
        );
        assert!(
            (v.x[i] - sx).abs() + (v.y[i] - sy).abs() > 1e-4,
            "agent {i} never left its spawn point ({sx}, {sy}) — the crowd \
             wedged it against the wall"
        );
    }
}

#[test]
fn a_fixture_scenario_reports_its_body() {
    let h = Harness::fixture(FIXTURE_DENSE_V1).build().expect("dense");
    let c = h.sim().collision();
    assert!(c.enabled(), "the tracked fixtures declare a body");
    assert!(
        (c.radius_cells - 0.125).abs() < 1e-6,
        "expected a 1/8-cell body radius, got {}",
        c.radius_cells
    );
}
