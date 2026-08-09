//! Tests for agent-agent soft separation: `mmd_engine::sim::SpatialGrid`, the
//! deterministic zero-alloc neighbour index it scans, and the steering blend
//! the tick applies on top of the flow field.
//!
//! Separation is *steering*, not resolution. Nothing here asserts that agents
//! stop overlapping — only that they push, that the push is reproducible, and
//! that bending a heading never changes how far an agent walks.

mod common;

use common::Tracker;
use mmd_engine::nav::flow_field::COST_UNREACHABLE;
use mmd_engine::scenario::Cell;
use mmd_engine::sim::{
    CollisionParams, MAX_SEPARATION_NEIGHBORS, SEPARATION_DIR16, SPEED_CELLS_PER_SEC, SpatialGrid,
    TICK_DT, accumulate_separation,
};
use mmd_engine::testkit::{
    COLLISION_MID_SCENE, COLLISION_SPRITE_SCENE, FIXTURE_DENSE_V1, GridSpec, Harness,
    ScenarioSource, scene_path,
};

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

#[test]
fn spatial_bin_counts_match_the_bucket_lengths() {
    let mut grid = SpatialGrid::new(8, 8, 1.0, 8);
    let xs = [0.5f32, 0.5, 7.5, 3.5, 7.5];
    let ys = [0.5f32, 0.5, 0.5, 3.5, 7.5];
    grid.rebuild(&xs, &ys);

    for by in 0..grid.rows() {
        for bx in 0..grid.cols() {
            assert_eq!(
                grid.bin_count(bx, by) as usize,
                grid.agents_in_bin(bx, by).len(),
                "bin ({bx}, {by}) mismatch"
            );
        }
    }
}

#[test]
fn bin_row_matches_bin_by_bin_order() {
    let mut grid = SpatialGrid::new(8, 8, 1.0, 16);
    let xs = [
        0.5f32, 1.5, 2.5, 3.5, 4.5, 5.5, 6.5, 7.5, 0.5, 1.5, 2.5, 3.5, 4.5, 5.5, 6.5, 7.5,
    ];
    let ys = [
        2.5f32, 2.5, 2.5, 2.5, 2.5, 2.5, 2.5, 2.5, 5.5, 5.5, 5.5, 5.5, 5.5, 5.5, 5.5, 5.5,
    ];
    grid.rebuild(&xs, &ys);

    let expected: Vec<u32> = [1u32, 2, 3]
        .iter()
        .flat_map(|cx| grid.agents_in_bin(*cx, 2))
        .copied()
        .collect();
    assert_eq!(grid.agents_in_bin_row(1, 3, 2).to_vec(), expected);
}

#[test]
fn bin_row_clamps_to_the_last_column() {
    let mut grid = SpatialGrid::new(8, 8, 1.0, 16);
    let xs = [
        0.5f32, 1.5, 2.5, 3.5, 4.5, 5.5, 6.5, 7.5, 0.5, 1.5, 2.5, 3.5, 4.5, 5.5, 6.5, 7.5,
    ];
    let ys = [
        2.5f32, 2.5, 2.5, 2.5, 2.5, 2.5, 2.5, 2.5, 5.5, 5.5, 5.5, 5.5, 5.5, 5.5, 5.5, 5.5,
    ];
    grid.rebuild(&xs, &ys);

    let mut expected: Vec<u32> = grid.agents_in_bin(6, 0).to_vec();
    expected.extend_from_slice(grid.agents_in_bin(7, 0));
    assert_eq!(grid.agents_in_bin_row(6, 99, 0).to_vec(), expected);
}

#[test]
fn bin_row_is_empty_off_the_grid() {
    let mut grid = SpatialGrid::new(8, 8, 1.0, 16);
    let xs = [
        0.5f32, 1.5, 2.5, 3.5, 4.5, 5.5, 6.5, 7.5, 0.5, 1.5, 2.5, 3.5, 4.5, 5.5, 6.5, 7.5,
    ];
    let ys = [
        2.5f32, 2.5, 2.5, 2.5, 2.5, 2.5, 2.5, 2.5, 5.5, 5.5, 5.5, 5.5, 5.5, 5.5, 5.5, 5.5,
    ];
    grid.rebuild(&xs, &ys);

    assert_eq!(grid.agents_in_bin_row(0, 2, 99), &[] as &[u32]);
    assert_eq!(grid.agents_in_bin_row(99, 100, 0), &[] as &[u32]);
}

#[test]
fn spatial_reuses_bins_without_clearing_them() {
    let mut grid = SpatialGrid::new(8, 8, 1.0, 4);
    let xs = [0.5f32, 0.5, 0.5, 0.5];
    let ys = [0.5f32, 0.5, 0.5, 0.5];
    grid.rebuild(&xs, &ys);
    assert_eq!(grid.bin_count(0, 0), 4);

    let xs2 = [7.5f32, 7.5, 7.5, 7.5];
    let ys2 = [7.5f32, 7.5, 7.5, 7.5];
    grid.rebuild(&xs2, &ys2);

    assert_eq!(grid.bin_count(0, 0), 0);
    assert_eq!(grid.bin_count(7, 7), 4);

    let mut total = 0u32;
    for by in 0..grid.rows() {
        for bx in 0..grid.cols() {
            total += grid.bin_count(bx, by);
        }
    }
    assert_eq!(total as usize, grid.len());
}

#[test]
fn spatial_survives_a_stamp_wrap() {
    let mut grid = SpatialGrid::new(8, 8, 1.0, 5);
    grid.set_stamp_for_test(u32::MAX);

    let xs = [0.5f32, 0.5, 7.5, 3.5, 7.5];
    let ys = [0.5f32, 0.5, 0.5, 3.5, 7.5];
    grid.rebuild(&xs, &ys);
    let mut total = 0u32;
    for by in 0..grid.rows() {
        for bx in 0..grid.cols() {
            total += grid.bin_count(bx, by);
        }
    }
    assert_eq!(total as usize, grid.len());

    let xs2 = [1.5f32, 1.5, 1.5, 1.5, 1.5];
    let ys2 = [1.5f32, 1.5, 1.5, 1.5, 1.5];
    grid.rebuild(&xs2, &ys2);
    assert_eq!(grid.bin_count(1, 1), 5);
    let mut total2 = 0u32;
    for by in 0..grid.rows() {
        for bx in 0..grid.cols() {
            total2 += grid.bin_count(bx, by);
        }
    }
    assert_eq!(total2 as usize, grid.len());
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
    let mass = [1u8; 2];
    let inv_mass = [1.0f32; 2];
    accumulate_separation(
        &xs, &ys, &grid, 0.5, &mass, &inv_mass, &mut sep_x, &mut sep_y,
    );

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
    let mass = [1u8; N];
    let inv_mass = [1.0f32; N];
    accumulate_separation(
        &xs, &ys, &grid, 0.5, &mass, &inv_mass, &mut sep_x, &mut sep_y,
    );

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
    // Contact is two radii = 1.0 cell, and the bins are one cell wide, so the
    // 3x3 scan window around agent 0's bin spans x in [0, 3).
    //
    // Agent 1 is deliberately placed at 1.1 from agent 0 — *inside* that window,
    // in the adjacent bin, and just past contact. That is the only placement
    // that tests the cutoff rather than the bin geometry: park the far agent
    // four bins away and it is never a candidate, so the cutoff could be deleted
    // and nothing would notice. It cannot be deleted quietly, either — without
    // it `w = (contact - d) * inv_contact` goes negative past contact and the
    // pair *attracts* at medium range.
    //
    // Agents 1 and 2 are 0.4 apart and therefore touching, so one call covers
    // both branches and the test can tell "correctly ignores the distant one"
    // apart from "never pushes at all". Agent 2 sits 1.5 from agent 0, in bin 3
    // — outside agent 0's window and past contact either way.
    //
    // The buffers are pre-seeded with 1.0: the accumulator must *write* zero
    // rather than leave the caller's slot alone, or a push from a previous tick
    // would survive into this one.
    let xs = [1.5f32, 2.6, 3.0];
    let ys = [1.5f32, 1.5, 1.5];
    let mut grid = SpatialGrid::new(8, 8, 1.0, 3);
    grid.rebuild(&xs, &ys);

    // The placement claims above, checked rather than asserted in prose.
    assert_eq!(grid.bin_of(xs[0], ys[0]), (1, 1));
    assert_eq!(
        grid.bin_of(xs[1], ys[1]),
        (2, 1),
        "the out-of-contact agent must sit inside agent 0's 3x3 scan window"
    );

    let mut sep_x = [1.0f32; 3];
    let mut sep_y = [1.0f32; 3];
    let mass = [1u8; 3];
    let inv_mass = [1.0f32; 3];
    accumulate_separation(
        &xs, &ys, &grid, 0.5, &mass, &inv_mass, &mut sep_x, &mut sep_y,
    );

    assert_eq!(
        (sep_x[0], sep_y[0]),
        (0.0, 0.0),
        "a scanned neighbour 1.1 cells away is past the 1.0-cell contact \
         distance and must contribute nothing"
    );
    assert!(
        sep_x[1] != 0.0,
        "agents inside contact must push: got {}",
        sep_x[1]
    );
    assert_eq!(sep_x[1], -sep_x[2], "and they must push each other apart");
}

#[test]
fn a_lone_agent_accumulates_no_repulsion() {
    // Two agents 20 cells apart in a 32x32 grid: neither is ever in the
    // other's 3x3 window, so each agent's window holds only itself and the
    // early-out must fire, writing zero without touching `items`.
    let params = CollisionParams::from_q8(128, 256);
    let xs = [1.5f32, 21.5];
    let ys = [1.5f32, 1.5];
    let mut grid = SpatialGrid::new(32, 32, params.bin_size_cells(), 2);
    grid.rebuild(&xs, &ys);

    let mut sep_x = [1.0f32; 2];
    let mut sep_y = [1.0f32; 2];
    let mass = [1u8; 2];
    let inv_mass = [1.0f32; 2];
    accumulate_separation(
        &xs,
        &ys,
        &grid,
        params.radius_cells,
        &mass,
        &inv_mass,
        &mut sep_x,
        &mut sep_y,
    );

    assert_eq!(sep_x, [0.0, 0.0]);
    assert_eq!(sep_y, [0.0, 0.0]);
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

#[test]
fn separation_phases_of_one_is_the_identity() {
    let mut default_h = Harness::grid(stacked_collision_grid(32))
        .build()
        .expect("default-phase stack");
    let mut explicit_h = Harness::grid(stacked_collision_grid(32).with_separation_phases(1))
        .build()
        .expect("explicit-phase-1 stack");

    default_h.step_exact(200);
    explicit_h.step_exact(200);

    assert_eq!(
        default_h.state_hash(),
        explicit_h.state_hash(),
        "declaring separation_phases = 1 explicitly must match the default"
    );
    assert_eq!(
        default_h.state_hash_hex(),
        BODIED_STACK_HASH,
        "separation_phases = 1 must stay bit-identical to the pinned digest"
    );
}

#[test]
fn an_amortised_agent_keeps_its_repulsion_between_phases() {
    let mut h = Harness::grid(stacked_collision_grid(32).with_separation_phases(4))
        .build()
        .expect("4-phase stack");

    // Agent 1 is in phase 1 (`i % phases == 1`), so it recomputes on ticks
    // where `tick_index % 4 == 1` — the second tick, not the first.
    h.step_exact(1);
    assert_eq!(
        h.sim().separation_of(1),
        (0.0, 0.0),
        "agent 1 must not have recomputed yet on tick 0"
    );

    h.step_exact(1);
    let recomputed = h.sim().separation_of(1);
    assert!(
        recomputed.0 != 0.0 || recomputed.1 != 0.0,
        "agent 1 must have recomputed by its phase tick, got {recomputed:?}"
    );

    // Tick 2 is phase 2 for a 4-phase spread, so agent 1 (phase 1) sits idle
    // again. Its stored repulsion must be untouched, not zeroed — the whole
    // point of amortisation is that a skipped agent keeps exactly what it
    // last computed.
    h.step_exact(1);
    assert_eq!(
        h.sim().separation_of(1),
        recomputed,
        "agent 1's repulsion must be left exactly as it was on a tick outside its phase"
    );
}

#[test]
fn the_grid_rebuilds_once_per_phase_cycle() {
    let mut h = Harness::grid(stacked_collision_grid(32).with_separation_phases(4))
        .build()
        .expect("4-phase stack");

    h.step_exact(8);

    assert_eq!(
        h.sim().grid_rebuild_count(),
        2,
        "8 ticks at 4 phases must rebuild the grid exactly twice"
    );
}

#[test]
fn an_amortised_stack_still_spreads() {
    let mut h = Harness::grid(stacked_collision_grid(64).with_separation_phases(4))
        .build()
        .expect("4-phase stack");

    let before = mean_pairwise_distance(&h);
    h.step_exact(200);
    let after = mean_pairwise_distance(&h);

    // `> before` alone would also pass for a crowd 95% still stacked; hold
    // amortisation to the same bar the unamortised sibling
    // (`a_released_stack_spreads_apart`) clears in fewer ticks.
    assert!(
        after > 0.5,
        "an amortised stack barely opened: before={before}, after={after}"
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

/// State hash of the bodied stack below after 200 ticks, measured on this tree.
///
/// The mirror of [`BODYLESS_GRID_PRE_SEPARATION_HASH`], for the path that
/// actually runs the separation pass. Every other test here compares the engine
/// to *itself*: `separation_is_reproducible` runs the same build twice in one
/// process, so any change that moves both runs together is invisible to it —
/// rotating `SEPARATION_DIR16` by one position, or raising
/// `MAX_SEPARATION_NEIGHBORS` from 8 to 16, left the whole file green.
///
/// Unlike the bodyless digest this one is not a compatibility claim against an
/// older engine; it is a change detector. If a deliberate change to the
/// separation model moves it, re-measure and update it in the same commit — but
/// do not update it to make an unexplained move go away.
const BODIED_STACK_HASH: &str = "81f958301624ff2033e797032d7cff8fd59344036263fdc5c6e13f00b5c80e8b";

#[test]
fn a_bodied_scenario_is_pinned_to_a_golden_digest() {
    // A single spawn cell, so every agent starts coincident and the tie-break
    // table is exercised from tick 0; 32 agents, so the 8-neighbour cap actually
    // truncates. Both of the constants above therefore reach the digest.
    let mut h = Harness::grid(stacked_collision_grid(32))
        .build()
        .expect("bodied stack");
    assert!(h.sim().collision().enabled());

    h.step_exact(200);

    assert_eq!(
        h.state_hash_hex(),
        BODIED_STACK_HASH,
        "the bodied separation path drifted; if the change was deliberate, \
         re-measure this digest in the same commit that caused it"
    );
}

#[test]
fn one_mass_class_leaves_every_agent_equal() {
    let mut h = Harness::grid(stacked_collision_grid(32))
        .build()
        .expect("default-class stack");

    for i in 0..32 {
        assert_eq!(h.sim().mass_of(i), 1, "agent {i} must be class 1");
    }

    h.step_exact(200);

    assert_eq!(
        h.state_hash_hex(),
        BODIED_STACK_HASH,
        "one mass class must stay bit-identical to the pinned digest"
    );
}

#[test]
fn mass_is_assigned_round_robin_by_index() {
    let h = Harness::grid(stacked_collision_grid(9).with_mass_classes(3))
        .build()
        .expect("3-class stack");

    let expected = [1u8, 2, 3, 1, 2, 3, 1, 2, 3];
    for (i, want) in expected.into_iter().enumerate() {
        assert_eq!(h.sim().mass_of(i), want, "agent {i} class mismatch");
    }
}

#[test]
fn a_heavier_neighbour_pushes_a_lighter_one_harder() {
    let mut h = Harness::grid(
        GridSpec::new(32, 32, Cell { x: 31, y: 16 })
            .with_collision(128, 256)
            .with_mass_classes(2)
            .with_agents(2),
    )
    .build()
    .expect("2-class pair");
    h.sim_mut().set_position(0, 1.5, 1.5);
    h.sim_mut().set_position(1, 1.6, 1.5);

    h.step_exact(1);

    let (sx0, sy0) = h.sim().separation_of(0);
    let (sx1, sy1) = h.sim().separation_of(1);
    assert!(
        sx0.hypot(sy0) > sx1.hypot(sy1),
        "the lighter agent (0) must be pushed harder than the heavier one (1): \
         {} vs {}",
        sx0.hypot(sy0),
        sx1.hypot(sy1)
    );
}

#[test]
fn mass_classes_change_the_bodied_digest() {
    let mut h = Harness::grid(stacked_collision_grid(32).with_mass_classes(2))
        .build()
        .expect("2-class stack");

    h.step_exact(200);

    assert_ne!(
        h.state_hash_hex(),
        BODIED_STACK_HASH,
        "two mass classes must not be a no-op on the digest"
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

    // Every free cell here is reachable through the gap, so every agent always
    // has an admissible descent step. The fallback therefore owes a *per tick*
    // guarantee, not an eventual one: no agent may ever sit out a tick.
    //
    // Measuring "did it move at all over 200 ticks" instead would be no test:
    // the crowd's own churn carries a wedged agent tens of cells, so deleting
    // the fallback outright leaves such an assertion green. Counting the ticks
    // an agent stood still is what the fallback actually controls.
    const TICKS: u64 = 200;
    let mut prev: Vec<(f32, f32)> = {
        let v = h.agents();
        (0..v.x.len()).map(|i| (v.x[i], v.y[i])).collect()
    };
    let mut stalled: Vec<(usize, u64)> = Vec::new();

    for tick in 1..=TICKS {
        h.step_exact(1);
        let v = h.agents();
        for (i, was) in prev.iter_mut().enumerate() {
            assert!(
                v.x[i].is_finite() && v.y[i].is_finite(),
                "agent {i} left the numeric domain at tick {tick}"
            );
            let now = (v.x[i], v.y[i]);
            if now == *was {
                stalled.push((i, tick));
            }
            *was = now;
        }
    }

    assert!(
        stalled.is_empty(),
        "{} agent-ticks spent motionless against the wall (first few: {:?}) — \
         the blended step was refused and no descent step was taken in its place",
        stalled.len(),
        &stalled[..stalled.len().min(8)]
    );

    // A stall count of zero is only meaningful if the wall was in play at all.
    // Agents must have crossed it, i.e. got past x = 3 through the single gap.
    let v = h.agents();
    assert!(
        (0..v.x.len()).any(|i| v.x[i] > 4.0),
        "no agent ever reached the far side of the wall, so the wedging \
         configuration was never exercised"
    );
}

#[test]
fn separation_never_steers_an_agent_into_a_corner_pocket() {
    // The blend turns the step into an arbitrary unit vector, so it can aim the
    // centre diagonally between two cells the flow field would never cut across.
    // Land in a walkable-but-unreachable pocket that way and the agent is stuck
    // forever: its descent vector is `(0, 0)`, so it never moves, never arrives
    // and never recycles — and the wall fallback cannot save it, because the
    // blended step *was* walkable.
    //
    // `(0,1)` and `(1,0)` blocked leaves `(0,0)` walkable but corner-locked.
    const W: u32 = 8;
    const OBSTACLE_1_0: u32 = 1; // (1,0)
    const OBSTACLE_0_1: u32 = W; // (0,1)

    let spec = GridSpec::new(W, 8, Cell { x: 7, y: 7 })
        .with_obstacles(vec![OBSTACLE_1_0, OBSTACLE_0_1])
        .with_spawns(vec![Cell { x: 1, y: 1 }])
        .with_collision(256, 2560)
        .with_agents(2);
    let mut h = Harness::grid(spec).build().expect("corner-pocket grid");
    assert!(h.sim().collision().enabled());

    // The pocket, stated as the field sees it: walkable, unreachable, no descent.
    assert_eq!(
        h.flow_field().vector_at(0, 0),
        (0.0, 0.0),
        "the pocket must have no descent vector, or it is not a pocket"
    );
    assert_eq!(
        h.flow_field().cost_at(0, 0),
        COST_UNREACHABLE,
        "the pocket must be unreachable under the no-corner-cut rule"
    );

    // Agent 0 sits just inside cell (1,1) with agent 1 up-field of it, so the
    // repulsion overwhelms the descent vector and points at the pocket corner.
    h.sim_mut().set_position(0, 1.02, 1.02);
    h.sim_mut().set_position(1, 1.60, 1.60);

    h.step_exact(1);

    let after_one = {
        let v = h.agents();
        (v.x[0], v.y[0])
    };
    let cell = (after_one.0.floor() as i32, after_one.1.floor() as i32);
    assert_ne!(
        cell,
        (0, 0),
        "the blend cut the corner into the pocket: agent 0 at {after_one:?}"
    );

    h.step_exact(500);

    let after_many = {
        let v = h.agents();
        (v.x[0], v.y[0])
    };
    assert_ne!(
        after_many, after_one,
        "agent 0 has not moved in 500 ticks — it is wedged at {after_one:?}"
    );
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

// --- collision_scene_v1 demo scenes (T5) ------------------------------------

/// Count unordered agent pairs whose centre distance is under `radius_cells`
/// (deep overlap — half contact or closer). Statistical, not a zero-overlap
/// claim: separation bounds overlap, it does not forbid it.
fn count_deep_pairs(h: &Harness, radius_cells: f32) -> usize {
    let v = h.agents();
    let n = v.x.len();
    let mut count = 0;
    for i in 0..n {
        for j in (i + 1)..n {
            let d = (v.x[i] - v.x[j]).hypot(v.y[i] - v.y[j]);
            if d < radius_cells {
                count += 1;
            }
        }
    }
    count
}

/// Every agent position must stay in the numeric domain at every sample.
fn assert_positions_finite(h: &Harness, tick: u64) {
    let v = h.agents();
    for i in 0..v.x.len() {
        assert!(
            v.x[i].is_finite() && v.y[i].is_finite(),
            "agent {i} left the numeric domain at tick {tick}"
        );
    }
}

/// Ticks at which deep overlap is sampled. The first is the stacked baseline
/// taken one tick after spawn; the rest track the decay across the 300-tick
/// window this scene is specified for.
const DEEP_SAMPLE_TICKS: [u64; 4] = [1, 100, 200, 300];

#[test]
fn sprite_scene_pulls_agents_out_of_deep_overlap() {
    let mut h = Harness::builder(ScenarioSource::path(scene_path(COLLISION_SPRITE_SCENE)))
        .build()
        .expect("sprite collision scene");
    let radius_cells = h.scenario().collision_radius_cells();

    let mut samples: Vec<(u64, usize)> = Vec::new();
    let mut tick = 0;
    for want in DEEP_SAMPLE_TICKS {
        h.step_exact(want - tick);
        tick = want;
        assert_positions_finite(&h, tick);
        samples.push((tick, count_deep_pairs(&h, radius_cells)));
    }
    eprintln!("deep pairs by tick: {samples:?}");

    let deep_before = samples[0].1;
    assert!(
        deep_before > 0,
        "agents start stacked ~9 per spawn cell; expected deep overlap at tick 1"
    );

    // Separation must not merely fail to make things worse: no later sample may
    // climb back above the stacked baseline. This is what catches agents
    // recycling to the spawn cells and restacking inside the window.
    for &(t, deep) in &samples[1..] {
        assert!(
            deep <= deep_before,
            "deep overlap rose above its tick-1 baseline at tick {t}: \
             {deep} > {deep_before}; samples={samples:?}"
        );
    }

    // The material claim: deep overlap at least halves over the window.
    let deep_at_300 = samples[samples.len() - 1].1;
    assert!(
        deep_at_300 * 2 <= deep_before,
        "deep overlap did not at least halve: before={deep_before}, \
         at_300={deep_at_300}; samples={samples:?}"
    );
}

#[test]
fn mid_scene_reports_its_tuning() {
    let h = Harness::builder(ScenarioSource::path(scene_path(COLLISION_MID_SCENE)))
        .build()
        .expect("mid collision scene");
    assert_eq!(h.alive_count(), 5_000);
    let c = h.sim().collision();
    assert!(c.enabled());
    assert!(
        (c.radius_cells - 6.0).abs() < 1e-6,
        "expected 6-cell radius, got {}",
        c.radius_cells
    );
    assert!(
        (c.strength - 1.0).abs() < 1e-6,
        "expected strength 1.0, got {}",
        c.strength
    );
    assert_eq!(
        h.scenario().separation_phases(),
        4,
        "expected the mid scene amortised over 4 phases"
    );
}

#[test]
fn collision_scene_agents_never_enter_an_obstacle() {
    let mut h = Harness::builder(ScenarioSource::path(scene_path(COLLISION_MID_SCENE)))
        .build()
        .expect("mid collision scene");
    let mut t = Tracker::new(&h);
    t.run(&mut h, 200);
    assert_eq!(t.obstacle_samples(), 0);
    assert_eq!(t.bounds_violations(), 0);
}
