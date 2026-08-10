//! What an RTS entity is doing, and the rules a moving one obeys.

use sha2::{Digest, Sha256};

use crate::scenario::Cell;

use super::entity::{EntityId, EntityStore, MAX_ENTITIES, UnitKind};

/// Walk speed in cells per second, per unit kind.
///
/// The worker outruns the horde's 8.0 so a base can be re-tasked faster than it
/// can be walked across; the soldier matches the horde exactly, because a phase-2
/// fight between the two must not be decided by a speed nobody chose.
pub const WORKER_SPEED_CELLS_PER_SEC: f32 = 10.0;
/// See [`WORKER_SPEED_CELLS_PER_SEC`].
pub const SOLDIER_SPEED_CELLS_PER_SEC: f32 = 8.0;

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

/// Where a gathering worker is in its round trip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GatherPhase {
    /// Walking to the node.
    ToNode { field_slot: u8 },
    /// Standing at the node, filling up. `ticks_left` counts down to zero.
    Mining { ticks_left: u32 },
    /// Walking back to `drop_off` with a full load.
    Returning { drop_off: EntityId, field_slot: u8 },
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
/// Discriminants are appended, never inserted — later tickets add `Build`
/// after `Gather`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Order {
    Idle,
    /// Walk to `dest` down the field in `field_slot`.
    Move {
        dest: Cell,
        field_slot: u8,
    },
    /// Mine `node` and haul to the nearest drop-off, forever.
    Gather {
        node: EntityId,
        phase: GatherPhase,
    },
}

impl Order {
    /// One stable byte per variant, for the state hash.
    pub fn tag(self) -> u8 {
        match self {
            Self::Idle => 0,
            Self::Move { .. } => 1,
            Self::Gather { .. } => 2,
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
                }
                Order::Move { dest, field_slot } => {
                    h.update(dest.x.to_le_bytes());
                    h.update(dest.y.to_le_bytes());
                    h.update([field_slot]);
                }
                Order::Gather { node, phase } => {
                    h.update(node.index.to_le_bytes());
                    h.update(node.generation.to_le_bytes());
                    h.update([phase.tag()]);
                    match phase {
                        GatherPhase::ToNode { field_slot } => {
                            h.update([field_slot]);
                            h.update(0u32.to_le_bytes());
                        }
                        GatherPhase::Mining { ticks_left } => {
                            h.update([0u8]);
                            h.update(ticks_left.to_le_bytes());
                        }
                        GatherPhase::Returning {
                            drop_off,
                            field_slot,
                        } => {
                            h.update([field_slot]);
                            h.update(drop_off.index.to_le_bytes());
                            h.update(drop_off.generation.to_le_bytes());
                        }
                    }
                }
            }
        }
    }
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

/// The cell a worker should be routed to when heading for a building.
///
/// The footprint's **centre cell**. The flow field's destination must be
/// unblocked, and T10 stamps a finished building's footprint as blocked — so
/// this returns the centre cell *before* T10 lands and T10 changes it to the
/// nearest free cell adjacent to the footprint. Recorded here so the change is
/// a deliberate edit, not a surprise.
pub(crate) fn drop_off_approach_cell(store: &EntityStore, id: EntityId) -> Cell {
    let slot = store.slot(id).expect("live drop-off");
    let pos = store.position(slot);
    node_cell(pos)
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
}
