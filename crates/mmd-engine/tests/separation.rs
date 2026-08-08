//! Tests for `mmd_engine::sim::SpatialGrid`, the deterministic zero-alloc
//! neighbour index. Nothing calls it yet (T4 wires it in) — this file pins
//! its standalone contract.

use mmd_engine::sim::SpatialGrid;

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
