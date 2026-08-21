//! What an RTS entity is doing, and the rules a moving one obeys.

use sha2::{Digest, Sha256};

use crate::nav::field_pool::FieldRef;
use crate::scenario::Cell;

use super::entity::{EntityId, EntityStore, MAX_ENTITIES, UnitKind};
use super::formation::FormationGoal;
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
/// See [`WORKER_SPEED_CELLS_PER_SEC`]. Slower than both player units:
/// a ghoul is walked away from, never outrun by accident.
pub const GHOUL_SPEED_CELLS_PER_SEC: f32 = 18.0;

/// Walk speed of a unit kind, in cells per second.
pub fn unit_speed(kind: UnitKind) -> f32 {
    match kind {
        UnitKind::Worker => WORKER_SPEED_CELLS_PER_SEC,
        UnitKind::Soldier => SOLDIER_SPEED_CELLS_PER_SEC,
        UnitKind::Ghoul => GHOUL_SPEED_CELLS_PER_SEC,
    }
}

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

/// A follower re-paths when its target has moved this many cells from `goal`.
pub const FOLLOW_REPATH_CELLS: f32 = 4.0;

/// Where a gathering worker is in its round trip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GatherPhase {
    /// Walking to this worker's own approach slot around the node.
    ToNode {
        goal: FormationGoal,
        field: FieldRef,
    },
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
    /// Walk to this unit's own slot in the group's formation, descending the
    /// group's shared field until the slot is in reach.
    Move {
        goal: FormationGoal,
        field: FieldRef,
    },
    /// Mine `node` and haul to the nearest drop-off, forever.
    Gather {
        node: EntityId,
        phase: GatherPhase,
    },
    /// Walk to this worker's own approach slot around `site` and attend it
    /// until it finishes.
    Build {
        site: EntityId,
        goal: FormationGoal,
        field: FieldRef,
    },
    /// Close on `target` until it is inside weapon range, then stand and
    /// fire until it dies. The field tracks the target's current cell.
    Attack {
        target: EntityId,
        field: FieldRef,
    },
    /// Walk toward `goal`, but stop and fire on anything hostile that comes
    /// into range on the way; resume the walk when nothing is.
    AttackMove {
        goal: FormationGoal,
        field: FieldRef,
    },
    /// Walk toward `target` and hold at interaction reach, re-pathing when it
    /// has moved more than [`FOLLOW_REPATH_CELLS`] from `goal`.
    Follow {
        target: EntityId,
        goal: FormationGoal,
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
            Self::Attack { .. } => 4,
            Self::AttackMove { .. } => 5,
            Self::Follow { .. } => 6,
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
            Self::Move { goal, .. } => Self::Move { goal, field },
            Self::Build { site, goal, .. } => Self::Build { site, goal, field },
            Self::Gather {
                node,
                phase: GatherPhase::ToNode { goal, .. },
            } => Self::Gather {
                node,
                phase: GatherPhase::ToNode { goal, field },
            },
            Self::Gather {
                node,
                phase: GatherPhase::Returning { drop_off, .. },
            } => Self::Gather {
                node,
                phase: GatherPhase::Returning { drop_off, field },
            },
            Self::Attack { target, .. } => Self::Attack { target, field },
            Self::AttackMove { goal, .. } => Self::AttackMove { goal, field },
            Self::Follow { target, goal, .. } => Self::Follow {
                target,
                goal,
                field,
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
                    hash_goal(h, FormationGoal::at(Cell { x: 0, y: 0 }));
                    h.update([0u8]);
                    h.update(0u64.to_le_bytes());
                }
                Order::Move { goal, field } => {
                    hash_goal(h, goal);
                    hash_field(h, field);
                }
                Order::Gather { node, phase } => {
                    h.update(node.index.to_le_bytes());
                    h.update(node.generation.to_le_bytes());
                    h.update([phase.tag()]);
                    match phase {
                        GatherPhase::ToNode { goal, field } => {
                            hash_goal(h, goal);
                            hash_field(h, field);
                            h.update(0u32.to_le_bytes());
                        }
                        GatherPhase::Mining { ticks_left } => {
                            hash_goal(h, FormationGoal::at(Cell { x: 0, y: 0 }));
                            h.update([0u8]);
                            h.update(0u64.to_le_bytes());
                            h.update(ticks_left.to_le_bytes());
                        }
                        GatherPhase::Returning { drop_off, field } => {
                            hash_goal(h, FormationGoal::at(Cell { x: 0, y: 0 }));
                            hash_field(h, field);
                            h.update(drop_off.index.to_le_bytes());
                            h.update(drop_off.generation.to_le_bytes());
                        }
                    }
                }
                Order::Build { site, goal, field } => {
                    h.update(site.index.to_le_bytes());
                    h.update(site.generation.to_le_bytes());
                    hash_goal(h, goal);
                    hash_field(h, field);
                }
                Order::Attack { target, field } => {
                    h.update(target.index.to_le_bytes());
                    h.update(target.generation.to_le_bytes());
                    hash_field(h, field);
                }
                Order::AttackMove { goal, field } => {
                    hash_goal(h, goal);
                    hash_field(h, field);
                }
                Order::Follow {
                    target,
                    goal,
                    field,
                } => {
                    h.update(target.index.to_le_bytes());
                    h.update(target.generation.to_le_bytes());
                    hash_goal(h, goal);
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

/// A formation goal's contribution to the state hash: anchor then slot.
///
/// Both halves are state. Two units of one group share an anchor and differ
/// only in their slot, so hashing the anchor alone would make a plan that
/// swapped two members' slots indistinguishable from the one that did not.
fn hash_goal(h: &mut Sha256, goal: FormationGoal) {
    h.update(goal.anchor.x.to_le_bytes());
    h.update(goal.anchor.y.to_le_bytes());
    h.update(goal.slot.x.to_le_bytes());
    h.update(goal.slot.y.to_le_bytes());
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
/// clearance either. This expands the **full** square ring around the
/// footprint — every cell on the border of the footprint box grown by `r`,
/// corners included — and returns the legal cell with the least
/// [`rect_distance`] to the footprint, ties broken by scan order (which is
/// fixed, so the answer is deterministic).
///
/// It cannot stop at the first non-empty ring: the closest cell on ring `r`
/// is the on-axis one at `r - 0.5`, but its corners sit at `√2 (r - 0.5)`, so
/// a corner found on ring `r` can be farther than an on-axis cell on ring
/// `r + 1`. `r - 0.5` is the least any cell on ring `r` can be, so the scan
/// stops as soon as that floor reaches the best distance found — at most one
/// ring past the first hit, and still allocation-free.
///
/// The ring used to be the four axis-aligned rays only — the cells directly
/// north, east, south and west of the footprint. That declared a target
/// unreachable whenever its only body-legal centres were off-axis, and on a
/// hauler's return leg [`super::world::RtsWorld`] then clears the order,
/// stranding a loaded worker for good. Widening it cannot change any case the
/// narrow scan already answered: an off-axis cell on ring `r` is strictly
/// farther from the footprint rectangle than any on-axis cell on the same
/// ring, so it can only win when the four rays hold nothing legal at all.
///
/// Allocation-free: two fixed loops per ring, no collection anywhere.
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

    let mut best: Option<(f32, u32, u32)> = None;
    for r in 1..=max_radius {
        // Nothing on this ring, or any ring beyond it, can beat what is
        // already held.
        if best.is_some_and(|(bd, _, _)| (r as f32 - 0.5) >= bd) {
            break;
        }
        let rr = r as i64;
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
        // The border of the footprint box grown by `rr`, each cell visited
        // exactly once: the two full horizontal runs, then the two vertical
        // runs with the corners already taken.
        let x0 = min_x - rr;
        let x1 = min_x + e - 1 + rr;
        let y0 = min_y - rr;
        let y1 = min_y + e - 1 + rr;
        for x in x0..=x1 {
            consider(x, y0, &mut best);
        }
        for y in (y0 + 1)..y1 {
            consider(x1, y, &mut best);
        }
        for x in x0..=x1 {
            consider(x, y1, &mut best);
        }
        for y in (y0 + 1)..y1 {
            consider(x0, y, &mut best);
        }
    }

    match best {
        Some((d, x, y)) => (Cell { x, y }, d),
        None => (node_cell(pos), 0.0),
    }
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
        BuildingKind, EntityKind, OWNER_NEUTRAL, OWNER_PLAYER, RTS_UNIT_BODY_RADIUS_CELLS,
        ResourceKind,
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

    /// A target whose only body-legal centres are **off-axis** must still get
    /// an approach cell, and a near one.
    ///
    /// Four small plugs, one on each cardinal side of a node at `(20, 20)`,
    /// inflate just far enough to blank every cell the old four-ray scan ever
    /// looked at, out to ring radius 10 — while the diagonal quadrants stay
    /// wide open one ring out. The narrow scan walked six rings past a legal
    /// centre 3.8 cells from the node to land on one 9.5 cells away; on a
    /// hauler's return leg an answer that far out (or none at all) is what
    /// strands a loaded worker.
    #[test]
    fn the_approach_cell_finds_an_off_axis_only_target() {
        let mut store = EntityStore::new();
        let id = store
            .spawn(
                EntityKind::Node(ResourceKind::Crystal),
                OWNER_NEUTRAL,
                [20.5, 20.5],
            )
            .expect("spawn node");
        let mut solids = vec![false; (W * H) as usize];
        // The node's own cell, as `StaticNav::new` would stamp it.
        solids[(20 + 20 * W) as usize] = true;
        // One plug per cardinal side, 7 cells out: a body's 3-cell clearance
        // then covers every on-axis ring cell between the node and the plug.
        for (x, y) in [(20u32, 13u32), (27, 20), (20, 27), (13, 20)] {
            solids[(x + y * W) as usize] = true;
        }
        let nav = StaticNav::from_raw(W, H, solids, RTS_UNIT_BODY_RADIUS_CELLS);

        // Every cell the four-ray scan would ever have considered, out past
        // the plugs, really is illegal — otherwise this case proves nothing.
        for r in 1..=10u32 {
            for (x, y) in [(20, 20 - r), (20 + r, 20), (20, 20 + r), (20 - r, 20)] {
                assert!(
                    nav.center_blocked()[(x + y * W) as usize],
                    "on-axis cell ({x}, {y}) at ring {r} must be illegal for this case"
                );
            }
        }

        let (cell, d) = entity_approach_cell(&nav, &store, id, UnitKind::Worker);
        let (again, _) = entity_approach_cell(&nav, &store, id, UnitKind::Worker);
        assert_eq!(cell, again, "the approach cell must be stable");
        assert!(
            cell.x != 20 && cell.y != 20,
            "the answer must be an off-axis cell, got {cell:?}"
        );
        assert!(
            !nav.center_blocked()[(cell.x + cell.y * W) as usize],
            "the approach cell {cell:?} must be a legal body centre"
        );
        assert!(
            d < 4.5,
            "an off-axis cell one ring out is {d} cells from the node; the \
             four-ray scan's own answer was 9.5"
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
