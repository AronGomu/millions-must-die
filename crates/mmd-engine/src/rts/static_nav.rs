//! Radius-aware static world geometry: circle-vs-terrain clearance and the
//! inflated centre mask that feeds the pooled flow fields.
//!
//! An RTS body is not a point — every current unit kind shares a 3-cell body
//! radius (see `entity::RTS_UNIT_BODY_RADIUS_CELLS`). Before this module,
//! navigation treated a unit as a point sampling a per-cell obstacle mask,
//! which let a 3-cell-wide body's centre walk within inches of a wall its own
//! hull would have clipped. `StaticNav` answers "is this circle clear of the
//! static world" exactly (closed-shape distance, never a sampled step), and
//! [`Self::center_blocked`] is the per-cell precomputation of that answer for
//! a unit's own body radius — the mask [`super::world::RtsWorld`] feeds its
//! pooled fields.

use crate::scenario::{Cell, Scenario};

use super::build::footprint_cells as building_footprint_cells;
use super::entity::{EntityKind, EntityStore};

/// Failures building a [`StaticNav`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum StaticNavError {
    #[error("empty grid")]
    EmptyGrid,
    #[error("obstacle index {0} out of bounds")]
    InvalidObstacleIndex(u32),
}

/// Static (never-moving) world geometry: terrain, resource nodes and
/// finished buildings — a construction site stays walkable until it
/// finishes, so it is deliberately excluded.
#[derive(Debug, Clone)]
pub struct StaticNav {
    width: u32,
    height: u32,
    /// Raw per-cell solid mask: terrain, resource-node footprints, finished
    /// building footprints. What a circle's clearance test is measured
    /// against.
    solids: Vec<bool>,
    /// Terrain and finished buildings only — no resource nodes. Building
    /// placement keeps its own, non-inflated exclusion rule (a node blocks
    /// placement through its own, more specific `CoversNode` check), so this
    /// is the mask placement validity reads instead of [`Self::solids`] or
    /// [`Self::center_blocked`].
    placement_solids: Vec<bool>,
    /// Per-cell: is this cell's centre a legal position for a body of the
    /// radius last passed to [`Self::rebuild_center_blocked`]?
    center_blocked: Vec<bool>,
    /// Connected-component id per cell over [`Self::center_blocked`],
    /// [`NO_COMPONENT`] for a blocked cell.
    ///
    /// Connectivity is the *same* rule a pooled flow field integrates with —
    /// 8 neighbours, no diagonal corner cut — so "same component" is exactly
    /// "a field to one would resolve a finite cost at the other". That is
    /// what makes this a legitimate substitute for building a field just to
    /// ask a reachability question, which every body-relocation site needs
    /// and none of them can afford.
    components: Vec<u32>,
    /// Reused flood-fill stack, so [`Self::rebuild_center_blocked`] allocates
    /// nothing after construction.
    component_stack: Vec<u32>,
}

/// [`StaticNav::component_at`]'s answer for a blocked cell: no component.
pub const NO_COMPONENT: u32 = u32::MAX;

impl StaticNav {
    /// Build from a validated scenario and the entity store at the moment
    /// every static object (HQ, resource nodes) exists but before any unit
    /// has been placed — unit placement is scored *against* this, not folded
    /// into it.
    pub fn new(scenario: &Scenario, store: &EntityStore) -> Result<Self, StaticNavError> {
        let width = scenario.width();
        let height = scenario.height();
        if width == 0 || height == 0 {
            return Err(StaticNavError::EmptyGrid);
        }
        let n = (width as usize)
            .checked_mul(height as usize)
            .ok_or(StaticNavError::EmptyGrid)?;

        let mut solids = vec![false; n];
        let mut placement_solids = vec![false; n];
        for &oi in scenario.obstacle_cells() {
            if oi as usize >= n {
                return Err(StaticNavError::InvalidObstacleIndex(oi));
            }
            solids[oi as usize] = true;
            placement_solids[oi as usize] = true;
        }

        for slot in 0..store.slot_count() {
            if !store.alive(slot) {
                continue;
            }
            match store.kind(slot) {
                EntityKind::Node(_) => {
                    let pos = store.position(slot);
                    let cell = Cell {
                        x: pos[0].floor() as u32,
                        y: pos[1].floor() as u32,
                    };
                    if cell.x < width && cell.y < height {
                        solids[(cell.x + cell.y * width) as usize] = true;
                    }
                }
                EntityKind::Building(b) => {
                    // Only finished buildings; a site stays walkable.
                    if store.progress_target(slot) != 0 {
                        continue;
                    }
                    let pos = store.position(slot);
                    for cell in building_footprint_cells(pos, b.footprint_cells()) {
                        if cell.x < width && cell.y < height {
                            let idx = (cell.x + cell.y * width) as usize;
                            solids[idx] = true;
                            placement_solids[idx] = true;
                        }
                    }
                }
                EntityKind::Unit(_) => {}
            }
        }

        let mut nav = Self {
            width,
            height,
            solids,
            placement_solids,
            center_blocked: vec![false; n],
            components: vec![NO_COMPONENT; n],
            component_stack: Vec::with_capacity(n),
        };
        nav.rebuild_center_blocked(super::entity::RTS_UNIT_BODY_RADIUS_CELLS);
        Ok(nav)
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    /// Per-cell: is this cell's centre a legal position for the body radius
    /// [`Self::rebuild_center_blocked`] was last called with?
    pub fn center_blocked(&self) -> &[bool] {
        &self.center_blocked
    }

    /// Terrain and finished buildings only — no resource nodes. The mask
    /// building placement validity reads.
    pub fn placement_solids(&self) -> &[bool] {
        &self.placement_solids
    }

    /// Whether a circle of `radius` cells centred at `p` is clear of every
    /// static solid and stays inside the map.
    ///
    /// Touching is legal: a circle whose edge exactly meets a solid's edge,
    /// or the map boundary, is clear. Penetration is strict `<`.
    pub fn position_clear(&self, p: [f32; 2], radius: f32) -> bool {
        let w = self.width as f32;
        let h = self.height as f32;
        if p[0] < radius || p[0] > w - radius || p[1] < radius || p[1] > h - radius {
            return false;
        }
        !self.circle_hits_solids(p, radius)
    }

    fn circle_hits_solids(&self, p: [f32; 2], radius: f32) -> bool {
        let x0 = (p[0] - radius).floor().max(0.0) as i64;
        let x1 = ((p[0] + radius).ceil() as i64).min(self.width as i64);
        let y0 = (p[1] - radius).floor().max(0.0) as i64;
        let y1 = ((p[1] + radius).ceil() as i64).min(self.height as i64);
        let r2 = radius * radius;
        for y in y0..y1 {
            for x in x0..x1 {
                let idx = (x as u32 + y as u32 * self.width) as usize;
                if !self.solids[idx] {
                    continue;
                }
                if dist2_point_rect(p, x as f32, x as f32 + 1.0, y as f32, y as f32 + 1.0) < r2 {
                    return true;
                }
            }
        }
        false
    }

    /// Whether a circle of `radius` cells can sweep the straight segment
    /// `from -> to` without ever penetrating a static solid or leaving the
    /// map — an exact continuous test, not a sampled walk, so it cannot miss
    /// a thin obstacle a step-sampled sweep would tunnel through.
    pub fn sweep_clear(&self, from: [f32; 2], to: [f32; 2], radius: f32) -> bool {
        let w = self.width as f32;
        let h = self.height as f32;
        for p in [from, to] {
            if p[0] < radius || p[0] > w - radius || p[1] < radius || p[1] > h - radius {
                return false;
            }
        }

        let min_x = (from[0].min(to[0]) - radius).floor().max(0.0) as i64;
        let max_x = ((from[0].max(to[0]) + radius).ceil() as i64).min(self.width as i64);
        let min_y = (from[1].min(to[1]) - radius).floor().max(0.0) as i64;
        let max_y = ((from[1].max(to[1]) + radius).ceil() as i64).min(self.height as i64);
        let r2 = radius * radius;

        for y in min_y..max_y {
            for x in min_x..max_x {
                let idx = (x as u32 + y as u32 * self.width) as usize;
                if !self.solids[idx] {
                    continue;
                }
                let d2 =
                    dist2_seg_rect(from, to, x as f32, x as f32 + 1.0, y as f32, y as f32 + 1.0);
                if d2 < r2 {
                    return false;
                }
            }
        }
        true
    }

    /// Stamp a finished building's footprint into the solid masks. Does not
    /// recompute [`Self::center_blocked`] — call
    /// [`Self::rebuild_center_blocked`] afterward, which is the caller's
    /// choice of when to pay for it.
    pub fn stamp_finished_building(&mut self, min: Cell, edge: u32) {
        for dy in 0..edge {
            for dx in 0..edge {
                let x = min.x + dx;
                let y = min.y + dy;
                if x < self.width && y < self.height {
                    let idx = (x + y * self.width) as usize;
                    self.solids[idx] = true;
                    self.placement_solids[idx] = true;
                }
            }
        }
    }

    /// Clear a destroyed building's footprint from the solid masks — the
    /// exact inverse of [`Self::stamp_finished_building`]. Placement validity
    /// keeps a footprint clear of terrain, resource nodes and other
    /// buildings, so clearing these cells cannot erase anyone else's solid.
    /// Does not recompute [`Self::center_blocked`] — call
    /// [`Self::rebuild_center_blocked`] afterward, same contract as stamping.
    pub fn unstamp_finished_building(&mut self, min: Cell, edge: u32) {
        for dy in 0..edge {
            for dx in 0..edge {
                let x = min.x + dx;
                let y = min.y + dy;
                if x < self.width && y < self.height {
                    let idx = (x + y * self.width) as usize;
                    self.solids[idx] = false;
                    self.placement_solids[idx] = false;
                }
            }
        }
    }

    /// Test-only: build directly from a raw solid mask, skipping the
    /// scenario/entity-store plumbing [`Self::new`] needs. `radius` seeds
    /// [`Self::center_blocked`] the same way [`Self::new`] does.
    #[cfg(test)]
    pub(crate) fn from_raw(width: u32, height: u32, solids: Vec<bool>, radius: f32) -> Self {
        let placement_solids = solids.clone();
        let n = solids.len();
        let mut nav = Self {
            width,
            height,
            solids,
            placement_solids,
            center_blocked: vec![false; n],
            components: vec![NO_COMPONENT; n],
            component_stack: Vec::with_capacity(n),
        };
        nav.rebuild_center_blocked(radius);
        nav
    }

    /// Recompute [`Self::center_blocked`] for a body of `radius` cells, from
    /// the current [`Self::solids`], and relabel [`Self::components`] with it.
    pub fn rebuild_center_blocked(&mut self, radius: f32) {
        for y in 0..self.height {
            for x in 0..self.width {
                let idx = (x + y * self.width) as usize;
                let p = [x as f32 + 0.5, y as f32 + 0.5];
                self.center_blocked[idx] = !self.position_clear(p, radius);
            }
        }
        self.rebuild_components();
    }

    /// Which connected region of legal body centres `cell` belongs to, or
    /// `None` when it is blocked (or off the grid).
    pub fn component_at(&self, cell: Cell) -> Option<u32> {
        if cell.x >= self.width || cell.y >= self.height {
            return None;
        }
        match self.components[(cell.x + cell.y * self.width) as usize] {
            NO_COMPONENT => None,
            c => Some(c),
        }
    }

    /// Whether a body standing at `a` could walk to `b` — same component, by
    /// the field's own connectivity rule. Blocked or off-grid on either side
    /// is `false`.
    pub fn connected(&self, a: Cell, b: Cell) -> bool {
        match (self.component_at(a), self.component_at(b)) {
            (Some(ca), Some(cb)) => ca == cb,
            _ => false,
        }
    }

    /// Flood-fill [`Self::components`] over [`Self::center_blocked`].
    ///
    /// Ascending cell index seeds the labels, so a given mask always produces
    /// the same ids — nothing here depends on scan order beyond that.
    fn rebuild_components(&mut self) {
        self.components.fill(NO_COMPONENT);
        let width = self.width as i32;
        let height = self.height as i32;
        let mut next = 0u32;
        for seed in 0..self.center_blocked.len() {
            if self.center_blocked[seed] || self.components[seed] != NO_COMPONENT {
                continue;
            }
            let id = next;
            next += 1;
            self.components[seed] = id;
            self.component_stack.clear();
            self.component_stack.push(seed as u32);
            while let Some(idx) = self.component_stack.pop() {
                let cx = (idx % self.width) as i32;
                let cy = (idx / self.width) as i32;
                for (dx, dy) in [
                    (0, -1),
                    (1, -1),
                    (1, 0),
                    (1, 1),
                    (0, 1),
                    (-1, 1),
                    (-1, 0),
                    (-1, -1),
                ] {
                    let nx = cx + dx;
                    let ny = cy + dy;
                    if nx < 0 || ny < 0 || nx >= width || ny >= height {
                        continue;
                    }
                    let n_idx = (nx as u32 + ny as u32 * self.width) as usize;
                    if self.center_blocked[n_idx] || self.components[n_idx] != NO_COMPONENT {
                        continue;
                    }
                    if dx != 0
                        && dy != 0
                        && !crate::nav::flow_field::diagonal_clear(
                            cx,
                            cy,
                            dx,
                            dy,
                            self.width,
                            self.height,
                            &self.center_blocked,
                        )
                    {
                        continue;
                    }
                    self.components[n_idx] = id;
                    self.component_stack.push(n_idx as u32);
                }
            }
        }
    }
}

/// Whether a circle of `radius` centred at `p` stays clear of the closed cell
/// rectangle whose minimum corner is `min` and whose edge is `edge` cells.
///
/// The rectangle a building is *about to* occupy is not in [`StaticNav`]'s
/// masks yet — a site is walkable until the tick it finishes — so a completion
/// that must first plan where every body it covers will stand needs to treat
/// that rectangle as solid without stamping it: the plan may still be
/// discarded. Same rule as everywhere else here: touching is legal,
/// penetration is strict `<`.
pub(crate) fn circle_clear_of_cell_rect(p: [f32; 2], radius: f32, min: Cell, edge: u32) -> bool {
    dist2_point_rect(
        p,
        min.x as f32,
        (min.x + edge) as f32,
        min.y as f32,
        (min.y + edge) as f32,
    ) >= radius * radius
}

/// Squared distance from a point to a closed interval `[a, b]`.
fn dist2_point_interval(p: f32, a: f32, b: f32) -> f32 {
    let d = (a - p).max(p - b).max(0.0);
    d * d
}

/// Squared distance from a point to a closed axis-aligned rectangle.
fn dist2_point_rect(p: [f32; 2], rx0: f32, rx1: f32, ry0: f32, ry1: f32) -> f32 {
    dist2_point_interval(p[0], rx0, rx1) + dist2_point_interval(p[1], ry0, ry1)
}

/// Squared distance from a point to a closed segment.
fn dist2_point_seg(p: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
    let abx = b[0] - a[0];
    let aby = b[1] - a[1];
    let apx = p[0] - a[0];
    let apy = p[1] - a[1];
    let len2 = abx * abx + aby * aby;
    let t = if len2 > 0.0 {
        ((apx * abx + apy * aby) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let cx = a[0] + abx * t;
    let cy = a[1] + aby * t;
    let dx = p[0] - cx;
    let dy = p[1] - cy;
    dx * dx + dy * dy
}

fn orient(a: [f32; 2], b: [f32; 2], c: [f32; 2]) -> f32 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}

fn on_seg(a: [f32; 2], b: [f32; 2], c: [f32; 2]) -> bool {
    c[0] <= a[0].max(b[0])
        && c[0] >= a[0].min(b[0])
        && c[1] <= a[1].max(b[1])
        && c[1] >= a[1].min(b[1])
}

/// Whether two closed segments intersect (including touching), by the
/// standard orientation test.
fn segs_intersect(a1: [f32; 2], a2: [f32; 2], b1: [f32; 2], b2: [f32; 2]) -> bool {
    let o1 = orient(a1, a2, b1);
    let o2 = orient(a1, a2, b2);
    let o3 = orient(b1, b2, a1);
    let o4 = orient(b1, b2, a2);
    if (o1 > 0.0) != (o2 > 0.0) && (o3 > 0.0) != (o4 > 0.0) {
        return true;
    }
    (o1 == 0.0 && on_seg(a1, a2, b1))
        || (o2 == 0.0 && on_seg(a1, a2, b2))
        || (o3 == 0.0 && on_seg(b1, b2, a1))
        || (o4 == 0.0 && on_seg(b1, b2, a2))
}

/// Exact squared distance between two closed segments: zero when they
/// intersect, otherwise the minimum over the four endpoint-to-opposite-segment
/// distances — the well-known closed form for segment/segment distance.
fn dist2_seg_seg(a1: [f32; 2], a2: [f32; 2], b1: [f32; 2], b2: [f32; 2]) -> f32 {
    if segs_intersect(a1, a2, b1, b2) {
        return 0.0;
    }
    let d1 = dist2_point_seg(a1, b1, b2);
    let d2 = dist2_point_seg(a2, b1, b2);
    let d3 = dist2_point_seg(b1, a1, a2);
    let d4 = dist2_point_seg(b2, a1, a2);
    d1.min(d2).min(d3).min(d4)
}

/// Exact squared distance between a closed segment and a closed axis-aligned
/// rectangle: zero when either endpoint lies inside the rectangle or the
/// segment crosses one of its four edges, otherwise the minimum of the two
/// endpoint-to-rectangle distances and the four segment-to-edge distances.
fn dist2_seg_rect(a: [f32; 2], b: [f32; 2], rx0: f32, rx1: f32, ry0: f32, ry1: f32) -> f32 {
    let inside = |p: [f32; 2]| p[0] >= rx0 && p[0] <= rx1 && p[1] >= ry0 && p[1] <= ry1;
    if inside(a) || inside(b) {
        return 0.0;
    }
    let c1 = [rx0, ry0];
    let c2 = [rx1, ry0];
    let c3 = [rx1, ry1];
    let c4 = [rx0, ry1];
    let mut best =
        dist2_point_rect(a, rx0, rx1, ry0, ry1).min(dist2_point_rect(b, rx0, rx1, ry0, ry1));
    for &(e1, e2) in &[(c1, c2), (c2, c3), (c3, c4), (c4, c1)] {
        best = best.min(dist2_seg_seg(a, b, e1, e2));
    }
    best
}
