//! What an RTS entity is doing, and the rules a moving one obeys.

use sha2::{Digest, Sha256};

use crate::nav::field_pool::FieldRef;
use crate::scenario::Cell;

use super::entity::{EntityId, EntityStore, MAX_ENTITIES, UnitKind};
use super::static_nav::StaticNav;

/// Walk speed in cells per second, per unit kind.
///
/// Phase-1.1 triples both from their phase-1 values (10.0/8.0) so movement
/// reads at RTS pace on the larger maps this slice unlocks; the worker still
/// outruns the soldier so a base can be re-tasked faster than it can be
/// walked across.
pub const WORKER_SPEED_CELLS_PER_SEC: f32 = 30.0;
/// See [`WORKER_SPEED_CELLS_PER_SEC`].
pub const SOLDIER_SPEED_CELLS_PER_SEC: f32 = 24.0;

/// Walk speed of a unit kind, in cells per second.
pub fn unit_speed(kind: UnitKind) -> f32 {
    match kind {
        UnitKind::Worker => WORKER_SPEED_CELLS_PER_SEC,
        UnitKind::Soldier => SOLDIER_SPEED_CELLS_PER_SEC,
    }
}

/// How close a unit's centre must get to its destination cell's centre to be
/// finished, in cells.
///
/// Larger than the horde's `ARRIVAL_RADIUS` (0.5) because a *group* is sent to
/// one cell and only one of them can stand on it; the rest stop adjacent and the
/// order still completes. Arrival is checked against the destination cell, not
/// against a per-unit goal, so the whole group clears its order together.
pub const ARRIVAL_RADIUS_CELLS: f32 = 1.5;

/// Slack added to a unit's own body radius to get its interaction reach: a
/// unit standing at a legal approach cell (whose centre already sits at least
/// one body radius clear of the target) needs this much more play to close
/// the last gap to the target's footprint rectangle without another step.
pub const NAV_CENTER_TOLERANCE_CELLS: f32 = 0.5;

/// How close a unit of `kind` must get — to a target's footprint rectangle —
/// to gather, build or drop off. Replaces the old independent
/// `BUILD_REACH_CELLS` / `GATHER_REACH_CELLS` / `DROP_OFF_REACH_CELLS`
/// constants: those were tuned for a point-sized unit, and a 3-cell-radius
/// body needs a reach that scales with its own hull.
pub const fn interaction_reach(kind: UnitKind) -> f32 {
    kind.body_radius_cells() + NAV_CENTER_TOLERANCE_CELLS
}

/// The reach an order actually completing against a specific approach cell
/// needs: [`interaction_reach`] is a floor, not a ceiling. Dense inflated
/// terrain can push every legal ring cell [`entity_approach_cell`] can find
/// past the flat-ground `interaction_reach` distance — a unit standing at the
/// approach cell it was actually routed to must still be able to finish its
/// order, so the reach widens to cover that cell's own distance (plus the same
/// tolerance) whenever it is the larger of the two.
pub(crate) fn adaptive_reach(kind: UnitKind, chosen_cell_dist: f32) -> f32 {
    interaction_reach(kind).max(chosen_cell_dist + NAV_CENTER_TOLERANCE_CELLS)
}

/// Where a gathering worker is in its round trip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GatherPhase {
    /// Walking to the node.
    ToNode { field: FieldRef },
    /// Standing at the node, filling up. `ticks_left` counts down to zero.
    Mining { ticks_left: u32 },
    /// Walking back to `drop_off` with a full load.
    Returning { drop_off: EntityId, field: FieldRef },
}

impl GatherPhase {
    /// One stable byte per phase, for the state hash.
    fn tag(self) -> u8 {
        match self {
            Self::ToNode { .. } => 0,
            Self::Mining { .. } => 1,
            Self::Returning { .. } => 2,
        }
    }
}

/// What an entity is currently doing.
///
/// Discriminants are appended, never inserted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Order {
    Idle,
    /// Walk to `dest` down the field `field` names.
    Move {
        dest: Cell,
        field: FieldRef,
    },
    /// Mine `node` and haul to the nearest drop-off, forever.
    Gather {
        node: EntityId,
        phase: GatherPhase,
    },
    /// Walk to `site` and attend it until it finishes.
    Build {
        site: EntityId,
        field: FieldRef,
    },
}

impl Order {
    /// One stable byte per variant, for the state hash.
    pub fn tag(self) -> u8 {
        match self {
            Self::Idle => 0,
            Self::Move { .. } => 1,
            Self::Gather { .. } => 2,
            Self::Build { .. } => 3,
        }
    }

    /// The same order with its cached field handle replaced.
    ///
    /// [`Self::Idle`] and [`GatherPhase::Mining`] hold no handle — nothing is
    /// walking — and come back unchanged. This is how the mover writes a
    /// re-acquired field back into a live order without having to restate the
    /// order's own payload at each of the four places that carry one.
    pub fn with_field(self, field: FieldRef) -> Self {
        match self {
            Self::Move { dest, .. } => Self::Move { dest, field },
            Self::Build { site, .. } => Self::Build { site, field },
            Self::Gather {
                node,
                phase: GatherPhase::ToNode { .. },
            } => Self::Gather {
                node,
                phase: GatherPhase::ToNode { field },
            },
            Self::Gather {
                node,
                phase: GatherPhase::Returning { drop_off, .. },
            } => Self::Gather {
                node,
                phase: GatherPhase::Returning { drop_off, field },
            },
            other => other,
        }
    }
}

/// One order per entity slot, preallocated to [`MAX_ENTITIES`].
#[derive(Debug, Clone)]
pub struct OrderTable {
    orders: Vec<Order>,
}

impl Default for OrderTable {
    fn default() -> Self {
        Self::new()
    }
}

impl OrderTable {
    pub fn new() -> Self {
        Self {
            orders: vec![Order::Idle; MAX_ENTITIES],
        }
    }

    pub fn get(&self, slot: usize) -> Order {
        self.orders[slot]
    }

    pub fn set(&mut self, slot: usize, order: Order) {
        self.orders[slot] = order;
    }

    pub fn clear(&mut self, slot: usize) {
        self.orders[slot] = Order::Idle;
    }

    /// Feed every live slot's order into the state hash, ascending.
    ///
    /// Fixed width per slot — an idle order hashes its zeroed payload — so the
    /// digest cannot alias a different order table by variable-length framing.
    pub fn hash_into(&self, h: &mut Sha256, live: &[usize]) {
        for &slot in live {
            let order = self.orders[slot];
            h.update([order.tag()]);
            match order {
                Order::Idle => {
                    h.update(0u32.to_le_bytes());
                    h.update(0u32.to_le_bytes());
                    h.update([0u8]);
                    h.update(0u64.to_le_bytes());
                }
                Order::Move { dest, field } => {
                    h.update(dest.x.to_le_bytes());
                    h.update(dest.y.to_le_bytes());
                    hash_field(h, field);
                }
                Order::Gather { node, phase } => {
                    h.update(node.index.to_le_bytes());
                    h.update(node.generation.to_le_bytes());
                    h.update([phase.tag()]);
                    match phase {
                        GatherPhase::ToNode { field } => {
                            hash_field(h, field);
                            h.update(0u32.to_le_bytes());
                        }
                        GatherPhase::Mining { ticks_left } => {
                            h.update([0u8]);
                            h.update(0u64.to_le_bytes());
                            h.update(ticks_left.to_le_bytes());
                        }
                        GatherPhase::Returning { drop_off, field } => {
                            hash_field(h, field);
                            h.update(drop_off.index.to_le_bytes());
                            h.update(drop_off.generation.to_le_bytes());
                        }
                    }
                }
                Order::Build { site, field } => {
                    h.update(site.index.to_le_bytes());
                    h.update(site.generation.to_le_bytes());
                    hash_field(h, field);
                }
            }
        }
    }
}

/// A cached field handle's contribution to the state hash: slot then epoch.
///
/// The epoch is in there because it is state — two orders on the same slot,
/// one of which has noticed the slot was rebuilt under it and one of which has
/// not, are not the same world.
fn hash_field(h: &mut Sha256, field: FieldRef) {
    h.update([field.slot]);
    h.update(field.epoch.to_le_bytes());
}

/// Squared cell-space distance.
pub(crate) fn dist2(a: [f32; 2], b: [f32; 2]) -> f32 {
    let dx = a[0] - b[0];
    let dy = a[1] - b[1];
    dx * dx + dy * dy
}

/// Distance from a point to a footprint rectangle, `0.0` when inside.
pub(crate) fn rect_distance(p: [f32; 2], center: [f32; 2], edge: u32) -> f32 {
    let half = edge as f32 * 0.5;
    let dx = (p[0] - center[0]).abs() - half;
    let dy = (p[1] - center[1]).abs() - half;
    let cx = dx.max(0.0);
    let cy = dy.max(0.0);
    (cx * cx + cy * cy).sqrt()
}

/// The cell a unit of `mover_kind` is routed to when heading for `target` (a
/// building or a resource node) — a legal centre (per
/// [`StaticNav::center_blocked`]) for the mover's own body, scored by
/// distance to `target`'s footprint rectangle.
///
/// The footprint's own cells (and everything within one body radius of it)
/// are blocked in the centre mask once a building finishes, so a field
/// cannot target them and a mover cannot stand inside another body's
/// clearance either. This expands a 4-connected ring around the footprint —
/// the cells directly north, east, south and west of it, offset outward one
/// grid step at a time, never a diagonal corner cell, matching the no-corner-
/// cut rule the field descent itself obeys — and, at the first ring radius
/// that contains at least one legal cell, returns the one with the least
/// [`rect_distance`] to the footprint, ties broken by the lower flat cell
/// index. A legal cell at ring radius `r` is never farther from the
/// footprint than one at `r + 1`, so the first non-empty ring already holds
/// the answer.
///
/// Returns the footprint's own cell when no ring out to the grid's own size
/// ever finds one, which then makes `FieldPool::acquire` fail cleanly rather
/// than silently routing somewhere else.
///
/// Returns the cell and its own [`rect_distance`] to `target`'s footprint —
/// the second half is what lets a caller compute [`adaptive_reach`] instead of
/// trusting a flat-ground constant that dense terrain can push the chosen
/// cell past.
pub(crate) fn entity_approach_cell(
    static_nav: &StaticNav,
    store: &EntityStore,
    target: EntityId,
    mover_kind: UnitKind,
) -> (Cell, f32) {
    let slot = store.slot(target).expect("live target");
    let pos = store.position(slot);
    let edge = store.kind(slot).footprint_cells();
    let min = super::selection::footprint_min(pos, edge);
    let width = static_nav.width();
    let height = static_nav.height();
    let cb = static_nav.center_blocked();
    let _ = mover_kind; // every current unit kind shares one body radius

    let min_x = min.x as i64;
    let min_y = min.y as i64;
    let e = edge as i64;
    let max_radius = width.max(height);

    for r in 1..=max_radius {
        let rr = r as i64;
        let mut best: Option<(f32, u32, u32)> = None;
        let consider = |x: i64, y: i64, best: &mut Option<(f32, u32, u32)>| {
            if x < 0 || y < 0 || x >= width as i64 || y >= height as i64 {
                return;
            }
            let idx = (x as u32 + y as u32 * width) as usize;
            if cb[idx] {
                return;
            }
            let p = [x as f32 + 0.5, y as f32 + 0.5];
            let d = rect_distance(p, pos, edge);
            if best.is_none_or(|(bd, _, _)| d < bd) {
                *best = Some((d, x as u32, y as u32));
            }
        };
        for x in min_x..min_x + e {
            consider(x, min_y - rr, &mut best);
        }
        for y in min_y..min_y + e {
            consider(min_x + e - 1 + rr, y, &mut best);
        }
        for x in min_x..min_x + e {
            consider(x, min_y + e - 1 + rr, &mut best);
        }
        for y in min_y..min_y + e {
            consider(min_x - rr, y, &mut best);
        }
        if let Some((d, x, y)) = best {
            return (Cell { x, y }, d);
        }
    }

    (node_cell(pos), 0.0)
}

/// One cell of [`entity_approach_cell`]'s ring scan: `None` when out of
/// bounds or blocked, `Some` otherwise.
fn ring_try(x: i64, y: i64, width: u32, height: u32, blocked: &[bool]) -> Option<Cell> {
    if x < 0 || y < 0 || x >= width as i64 || y >= height as i64 {
        return None;
    }
    let idx = (x as u32 + y as u32 * width) as usize;
    if blocked[idx] {
        None
    } else {
        Some(Cell {
            x: x as u32,
            y: y as u32,
        })
    }
}

/// The nearest unblocked, in-bounds cell to `from`, or `None` when the grid
/// has no unblocked cell.
///
/// Scanned as squares of growing Chebyshev radius, and inside a ring in a
/// fixed row-major order — "nearest" by float distance would make the choice
/// depend on a float comparison and stop the state hash reproducing across a
/// refactor, exactly as [`entity_approach_cell`] argues.
///
/// This is how a unit gets out of a cell that was stamped blocked underneath
/// it: every field's descent vector at a blocked cell is zero, so a unit left
/// standing in one could never walk out again.
pub(crate) fn nearest_unblocked_cell(
    blocked: &[bool],
    width: u32,
    height: u32,
    from: Cell,
) -> Option<Cell> {
    // Every in-bounds cell is within this Chebyshev radius. Each ring is
    // visited in row-major order: top row, left/right sides, bottom row.
    let max_radius = width.max(height);
    for r in 0..=max_radius {
        let min_x = from.x as i64 - r as i64;
        let max_x = from.x as i64 + r as i64;
        let min_y = from.y as i64 - r as i64;
        let max_y = from.y as i64 + r as i64;

        for x in min_x..=max_x {
            if let Some(c) = ring_try(x, min_y, width, height, blocked) {
                return Some(c);
            }
        }
        for y in (min_y + 1)..max_y {
            if let Some(c) = ring_try(min_x, y, width, height, blocked) {
                return Some(c);
            }
            if max_x != min_x
                && let Some(c) = ring_try(max_x, y, width, height, blocked)
            {
                return Some(c);
            }
        }
        if max_y != min_y {
            for x in min_x..=max_x {
                if let Some(c) = ring_try(x, max_y, width, height, blocked) {
                    return Some(c);
                }
            }
        }
    }
    None
}

/// The cell a node occupies.
pub(crate) fn node_cell(pos: [f32; 2]) -> Cell {
    Cell {
        x: pos[0].floor() as u32,
        y: pos[1].floor() as u32,
    }
}

/// Is a step from cell `(cx, cy)` to the continuous position `(nx, ny)` one the
/// walk is allowed to take?
///
/// Deliberately identical to `sim::tick::step_admissible`, down to the
/// `diagonal_clear` call: the RTS mover and the horde walk must agree on what
/// "reachable" means, or an RTS unit can be steered across a blocked corner
/// into a cell that is walkable but unreachable — a cell whose descent vector
/// is zero forever, where the unit parks and never finishes its order.
/// `rts_step_admissible_agrees_with_the_sim` pins the two together so the
/// duplication cannot drift.
#[inline]
pub(crate) fn step_admissible(
    cx: i32,
    cy: i32,
    nx: f32,
    ny: f32,
    width: u32,
    height: u32,
    blocked: &[bool],
) -> bool {
    let tx = nearest_cell(nx);
    let ty = nearest_cell(ny);
    if tx < 0 || ty < 0 || tx >= width as i32 || ty >= height as i32 {
        return false;
    }
    let idx = (tx as u32 + ty as u32 * width) as usize;
    if blocked[idx] {
        return false;
    }
    // `signum`, not the raw delta: a step is shorter than a cell so the delta is
    // already in -1..=1, but the rule is about the corner being crossed and must
    // not silently index a cell two away if that ever stops holding.
    let dx = (tx - cx).signum();
    let dy = (ty - cy).signum();
    if dx != 0 && dy != 0 {
        return crate::nav::flow_field::diagonal_clear(cx, cy, dx, dy, width, height, blocked);
    }
    true
}

/// Cell under a continuous coordinate. Cell centres sit at `n + 0.5`, so the
/// Voronoi cell of the centres is exactly `floor`.
#[inline]
pub(crate) fn nearest_cell(p: f32) -> i32 {
    p.floor() as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The RTS admissibility rule and the horde's must agree everywhere.
    ///
    /// Exhaustive over every obstacle subset of a 3x3 core inside a 5x5 grid
    /// and every 8-neighbour step out of the core's centre ring, so dropping
    /// either arm of the rule — the blocked-target test or the no-corner-cut
    /// test — shows up here rather than as a unit parked in a pocket.
    #[test]
    fn rts_step_admissible_agrees_with_the_sim() {
        const W: u32 = 5;
        const H: u32 = 5;
        let core: [usize; 9] = [6, 7, 8, 11, 12, 13, 16, 17, 18];
        let step = 0.4f32;
        let mut checked = 0u32;

        for mask in 0..(1u32 << 9) {
            let mut blocked = vec![false; (W * H) as usize];
            for (bit, &idx) in core.iter().enumerate() {
                if mask & (1 << bit) != 0 {
                    blocked[idx] = true;
                }
            }
            for cy in 1..4i32 {
                for cx in 1..4i32 {
                    for (dx, dy) in [
                        (-1, -1),
                        (0, -1),
                        (1, -1),
                        (-1, 0),
                        (1, 0),
                        (-1, 1),
                        (0, 1),
                        (1, 1),
                    ] {
                        // Land just inside the neighbour cell, so the step is a
                        // real boundary crossing rather than a jump.
                        let nx = cx as f32 + 0.5 + dx as f32 * (0.5 + step);
                        let ny = cy as f32 + 0.5 + dy as f32 * (0.5 + step);
                        let ours = step_admissible(cx, cy, nx, ny, W, H, &blocked);
                        let theirs = crate::sim::step_admissible(cx, cy, nx, ny, W, H, &blocked);
                        assert_eq!(
                            ours, theirs,
                            "mask {mask:#b} step ({cx},{cy})->({nx},{ny}): rts {ours}, sim {theirs}"
                        );
                        checked += 1;
                    }
                }
            }
        }
        assert_eq!(checked, 512 * 9 * 8, "every case must have been exercised");
    }

    #[test]
    fn the_worker_is_the_faster_unit() {
        assert!(unit_speed(UnitKind::Worker) > unit_speed(UnitKind::Soldier));
    }

    #[test]
    fn an_order_table_starts_idle() {
        let t = OrderTable::new();
        assert_eq!(t.get(0), Order::Idle);
        assert_eq!(t.get(MAX_ENTITIES - 1), Order::Idle);
    }

    // --- entity_approach_cell ------------------------------------------------
    //
    // `entity_approach_cell` is `pub(crate)`, so these cases live here as unit
    // tests rather than in `tests/rts_build.rs` (an external integration test
    // cannot see a crate-private item) — the same reason
    // `rts_step_admissible_agrees_with_the_sim` above tests `step_admissible`
    // in-crate instead of from outside.

    use super::super::entity::{
        BuildingKind, EntityKind, OWNER_PLAYER, RTS_UNIT_BODY_RADIUS_CELLS,
    };
    use super::super::static_nav::StaticNav;

    const W: u32 = 320;
    const H: u32 = 320;

    fn stamp(solids: &mut [bool], min: Cell, edge: u32) {
        for dy in 0..edge {
            for dx in 0..edge {
                solids[((min.x + dx) + (min.y + dy) * W) as usize] = true;
            }
        }
    }

    #[test]
    fn the_approach_cell_rings_a_finished_building() {
        let mut store = EntityStore::new();
        let id = store
            .spawn(
                EntityKind::Building(BuildingKind::Hq),
                OWNER_PLAYER,
                [166.0, 166.0],
            )
            .expect("spawn hq");
        let mut solids = vec![false; (W * H) as usize];
        stamp(&mut solids, Cell { x: 160, y: 160 }, 12);
        let nav = StaticNav::from_raw(W, H, solids, RTS_UNIT_BODY_RADIUS_CELLS);

        let (a, _) = entity_approach_cell(&nav, &store, id, UnitKind::Worker);
        let (b, _) = entity_approach_cell(&nav, &store, id, UnitKind::Worker);
        assert_eq!(a, b, "the approach cell must be the same on every call");

        // The HQ footprint is [160, 172) x [160, 172); the approach cell must
        // lie outside it and outside the mask's inflated clearance.
        assert!(
            a.x < 160 || a.x >= 172 || a.y < 160 || a.y >= 172,
            "approach cell {a:?} must lie outside the footprint"
        );
        assert!(!nav.center_blocked()[(a.x + a.y * W) as usize]);
    }

    #[test]
    fn the_nearest_open_cell_uses_a_deterministic_row_major_tie_break() {
        let mut blocked = vec![true; 7 * 7];
        let first = Cell { x: 2, y: 2 };
        let tied_later = Cell { x: 4, y: 2 };
        blocked[(first.x + first.y * 7) as usize] = false;
        blocked[(tied_later.x + tied_later.y * 7) as usize] = false;

        assert_eq!(
            nearest_unblocked_cell(&blocked, 7, 7, Cell { x: 3, y: 3 }),
            Some(first)
        );
    }

    #[test]
    fn the_nearest_open_cell_searches_the_whole_grid() {
        let mut blocked = vec![true; 7 * 7];
        blocked[6 + 6 * 7] = false;
        assert_eq!(
            nearest_unblocked_cell(&blocked, 7, 7, Cell { x: 0, y: 0 }),
            Some(Cell { x: 6, y: 6 })
        );
    }

    #[test]
    fn the_approach_cell_is_deterministic_under_a_blocked_ring() {
        let mut store = EntityStore::new();
        // Depot, edge 8, centred at [20.0, 20.0] -> min corner (16, 16).
        let id = store
            .spawn(
                EntityKind::Building(BuildingKind::Depot),
                OWNER_PLAYER,
                [20.0, 20.0],
            )
            .expect("spawn depot");
        let mut solids = vec![false; (W * H) as usize];
        stamp(&mut solids, Cell { x: 16, y: 16 }, 8);

        // A 3-cell body needs its centre 3 cells clear of the footprint, so
        // the first ring radius with any legal cell is r=4 (dist = r - 0.5
        // >= 3). Blocking the top edge's r=4 candidates (y=12, x in 16..24)
        // forces the scan to the next edge scored at the same radius.
        for x in 16..24u32 {
            solids[(x + 12 * W) as usize] = true;
        }

        let nav = StaticNav::from_raw(W, H, solids, RTS_UNIT_BODY_RADIUS_CELLS);
        let (cell, _) = entity_approach_cell(&nav, &store, id, UnitKind::Worker);

        // The scan must then move to the right edge, top -> bottom, whose
        // first legal cell at r=4 is (27, 16).
        assert_eq!(cell, Cell { x: 27, y: 16 });
    }
}
