//! Deterministic 8-neighbor reverse-Dijkstra flow field.

use std::cmp::Ordering;
use std::collections::BinaryHeap;

use crate::scenario::{Cell, Scenario};

/// Cardinal step integration cost.
pub const CARDINAL_COST: u32 = 1000;
/// Diagonal step integration cost (≈1000√2).
pub const DIAGONAL_COST: u32 = 1414;
/// Obstacle cell sentinel cost.
pub const COST_OBSTACLE: u32 = u32::MAX;
/// Reachable-free but never visited (disconnected) sentinel.
pub const COST_UNREACHABLE: u32 = u32::MAX - 1;

/// Neighbor deltas in stable order: N, NE, E, SE, S, SW, W, NW.
const NEIGHBORS: [(i32, i32, u32); 8] = [
    (0, -1, CARDINAL_COST),  // N
    (1, -1, DIAGONAL_COST),  // NE
    (1, 0, CARDINAL_COST),   // E
    (1, 1, DIAGONAL_COST),   // SE
    (0, 1, CARDINAL_COST),   // S
    (-1, 1, DIAGONAL_COST),  // SW
    (-1, 0, CARDINAL_COST),  // W
    (-1, -1, DIAGONAL_COST), // NW
];

/// Immutable shared flow field. Build once before timed run.
#[derive(Debug, Clone)]
pub struct FlowField {
    width: u32,
    height: u32,
    /// Integration cost per cell (`x + y * width`).
    costs: Vec<u32>,
    /// Normalized descent vectors; (0,0) if none.
    vx: Vec<f32>,
    vy: Vec<f32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FlowFieldError {
    EmptyGrid,
    DestinationOutOfBounds,
    DestinationBlocked,
    InvalidObstacleIndex(u32),
}

/// Min-heap entry: lower cost first; ties → lower cell index.
#[derive(Copy, Clone, Eq, PartialEq)]
struct HeapEntry {
    cost: u32,
    index: u32,
}

impl Ord for HeapEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        // BinaryHeap is max-heap → reverse for min-cost, then min-index.
        other
            .cost
            .cmp(&self.cost)
            .then_with(|| other.index.cmp(&self.index))
    }
}

impl PartialOrd for HeapEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl FlowField {
    /// Build from verified scenario (obstacles + destination).
    pub fn from_scenario(scenario: &Scenario) -> Self {
        Self::build(
            scenario.width(),
            scenario.height(),
            scenario.destination(),
            scenario.obstacle_cells(),
        )
        .expect("validated scenario must yield flow field")
    }

    /// Build integration + vector field for grid.
    pub fn build(
        width: u32,
        height: u32,
        destination: Cell,
        obstacle_indices: &[u32],
    ) -> Result<Self, FlowFieldError> {
        if width == 0 || height == 0 {
            return Err(FlowFieldError::EmptyGrid);
        }
        let n = (width as usize)
            .checked_mul(height as usize)
            .ok_or(FlowFieldError::EmptyGrid)?;
        let dest_idx = cell_index(destination, width, height)
            .ok_or(FlowFieldError::DestinationOutOfBounds)? as u32;

        let mut blocked = vec![false; n];
        for &oi in obstacle_indices {
            if oi as usize >= n {
                return Err(FlowFieldError::InvalidObstacleIndex(oi));
            }
            blocked[oi as usize] = true;
        }
        if blocked[dest_idx as usize] {
            return Err(FlowFieldError::DestinationBlocked);
        }

        let mut costs = vec![COST_UNREACHABLE; n];
        for (i, b) in blocked.iter().enumerate() {
            if *b {
                costs[i] = COST_OBSTACLE;
            }
        }

        // Reverse Dijkstra from destination.
        let mut heap = BinaryHeap::with_capacity(n / 4 + 8);
        costs[dest_idx as usize] = 0;
        heap.push(HeapEntry {
            cost: 0,
            index: dest_idx,
        });

        while let Some(HeapEntry { cost, index: u_idx }) = heap.pop() {
            if cost != costs[u_idx as usize] {
                continue; // stale
            }
            let ux = (u_idx % width) as i32;
            let uy = (u_idx / width) as i32;

            for &(dx, dy, step) in &NEIGHBORS {
                let nx = ux + dx;
                let ny = uy + dy;
                if nx < 0 || ny < 0 || nx >= width as i32 || ny >= height as i32 {
                    continue;
                }
                let v_idx = (nx as u32) + (ny as u32) * width;
                let v = v_idx as usize;
                if blocked[v] {
                    continue;
                }
                // No diagonal corner-cut when either adjacent cardinal blocked.
                if dx != 0 && dy != 0 && !diagonal_clear(ux, uy, dx, dy, width, height, &blocked) {
                    continue;
                }
                let new_cost = match cost.checked_add(step) {
                    Some(c) => c,
                    None => continue,
                };
                if new_cost < costs[v] {
                    costs[v] = new_cost;
                    heap.push(HeapEntry {
                        cost: new_cost,
                        index: v_idx,
                    });
                }
            }
        }

        let (vx, vy) = derive_vectors(width, height, &costs, &blocked);

        Ok(Self {
            width,
            height,
            costs,
            vx,
            vy,
        })
    }

    pub fn width(&self) -> u32 {
        self.width
    }
    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn cost_at(&self, x: u32, y: u32) -> u32 {
        self.costs[self.flat(x, y)]
    }

    pub fn vector_at(&self, x: u32, y: u32) -> (f32, f32) {
        let i = self.flat(x, y);
        (self.vx[i], self.vy[i])
    }

    /// True when cell has nonzero descent vector.
    pub fn has_vector(&self, x: u32, y: u32) -> bool {
        let i = self.flat(x, y);
        self.vx[i] != 0.0 || self.vy[i] != 0.0
    }

    pub fn costs(&self) -> &[u32] {
        &self.costs
    }

    fn flat(&self, x: u32, y: u32) -> usize {
        debug_assert!(x < self.width && y < self.height);
        (x + y * self.width) as usize
    }
}

fn cell_index(cell: Cell, width: u32, height: u32) -> Option<usize> {
    if cell.x >= width || cell.y >= height {
        None
    } else {
        Some((cell.x + cell.y * width) as usize)
    }
}

/// Diagonal from (ux,uy) along (dx,dy): both adjacent cardinals must be free + in-bounds.
///
/// Visible to the crate because the tick applies the *same* rule to a blended
/// step (see [`crate::sim::tick`]): there must be exactly one no-corner-cut
/// rule, or a steered agent can reach a cell the field considers unreachable.
pub(crate) fn diagonal_clear(
    ux: i32,
    uy: i32,
    dx: i32,
    dy: i32,
    width: u32,
    height: u32,
    blocked: &[bool],
) -> bool {
    let w = width as i32;
    let h = height as i32;
    // Cardinal A: (ux+dx, uy), B: (ux, uy+dy)
    let ax = ux + dx;
    let ay = uy;
    let bx = ux;
    let by = uy + dy;
    if ax < 0 || ay < 0 || ax >= w || ay >= h {
        return false;
    }
    if bx < 0 || by < 0 || bx >= w || by >= h {
        return false;
    }
    let ai = (ax as u32 + ay as u32 * width) as usize;
    let bi = (bx as u32 + by as u32 * width) as usize;
    !blocked[ai] && !blocked[bi]
}

fn derive_vectors(
    width: u32,
    height: u32,
    costs: &[u32],
    blocked: &[bool],
) -> (Vec<f32>, Vec<f32>) {
    let n = costs.len();
    let mut vx = vec![0.0f32; n];
    let mut vy = vec![0.0f32; n];
    let w = width as i32;
    let h = height as i32;

    for y in 0..height {
        for x in 0..width {
            let i = (x + y * width) as usize;
            if blocked[i] {
                continue;
            }
            let c = costs[i];
            if c == 0 || c >= COST_UNREACHABLE {
                continue; // dest or unreachable → zero vector
            }

            let mut best_cost = c; // must strictly descend
            let mut best_dx = 0i32;
            let mut best_dy = 0i32;
            let mut found = false;

            // Stable neighbor order; first lowest cost wins on ties.
            for &(dx, dy, _) in &NEIGHBORS {
                let nx = x as i32 + dx;
                let ny = y as i32 + dy;
                if nx < 0 || ny < 0 || nx >= w || ny >= h {
                    continue;
                }
                let ni = (nx as u32 + ny as u32 * width) as usize;
                if blocked[ni] {
                    continue;
                }
                // Match traversal: no corner cut on diagonal sample.
                if dx != 0
                    && dy != 0
                    && !diagonal_clear(x as i32, y as i32, dx, dy, width, height, blocked)
                {
                    continue;
                }
                let nc = costs[ni];
                if nc >= COST_UNREACHABLE {
                    continue;
                }
                if nc < best_cost {
                    best_cost = nc;
                    best_dx = dx;
                    best_dy = dy;
                    found = true;
                }
            }

            if found {
                let fx = best_dx as f32;
                let fy = best_dy as f32;
                let len = (fx * fx + fy * fy).sqrt();
                // len is 1 or √2 for grid steps; always > 0 when found.
                vx[i] = fx / len;
                vy[i] = fy / len;
            }
        }
    }
    (vx, vy)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heap_tie_prefers_lower_index() {
        let mut h = BinaryHeap::new();
        h.push(HeapEntry { cost: 10, index: 5 });
        h.push(HeapEntry { cost: 10, index: 2 });
        h.push(HeapEntry { cost: 9, index: 99 });
        assert_eq!(h.pop().unwrap().index, 99);
        assert_eq!(h.pop().unwrap().index, 2);
        assert_eq!(h.pop().unwrap().index, 5);
    }
}
