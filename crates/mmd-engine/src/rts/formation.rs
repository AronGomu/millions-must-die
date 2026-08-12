//! Deterministic group formations: one shared anchor field, one distinct slot
//! per member, planned atomically.
//!
//! A group order used to be N orders to one cell. Only one body can stand on a
//! cell, so with T4's hard collision the rest of the group piled up against it
//! forever. The fix is not per-unit pathfinding — the engine rule stands — but
//! a *plan*: the click resolves to one anchor cell, the pool is asked for that
//! anchor's field exactly once, and every member is handed its own cell on a
//! [`FORMATION_SPACING_CELLS`]-cell lattice around the anchor. A unit descends
//! the shared field until it is inside its own capture ring, then steers
//! straight at its slot — bounded local placement, never a second field.
//!
//! Everything here is a pure function of the world state: candidate slots are
//! enumerated in Chebyshev rings around the anchor, row-major inside a ring,
//! and units are matched to them in ascending entity-slot order. The caller's
//! argument order never reaches the plan.

use crate::nav::field_pool::FieldPool;
use crate::scenario::Cell;

use super::collision::units_overlap;
use super::entity::{EntityId, EntityKind, EntityStore, MAX_ENTITIES, RTS_UNIT_BODY_RADIUS_CELLS};
use super::orders::dist2;
use super::static_nav::StaticNav;

/// Distance between two neighbouring formation slots, in cells.
///
/// One body diameter (`RTS_UNIT_BODY_DIAMETER_CELLS`, 6.0): the tightest
/// lattice on which every member of a finished formation is still legally
/// placed, since two bodies at exactly one diameter apart are touching, not
/// merged.
pub const FORMATION_SPACING_CELLS: i32 = 6;

/// How far past its own slot offset a unit may be and still be steering
/// terminally, in cells.
///
/// The capture ring is `distance(anchor, slot) + FORMATION_CAPTURE_MARGIN_CELLS`
/// around the anchor: inside it a unit aims at its own slot, outside it a unit
/// descends the shared field. One body diameter of margin means a unit that is
/// shoved a body's width out of the ring by a neighbour is still steering at
/// its slot rather than snapping back to field descent.
pub const FORMATION_CAPTURE_MARGIN_CELLS: f32 = 6.0;

/// How close a unit's centre must get to its slot's centre to have arrived, in
/// cells.
///
/// Tight, unlike the pre-formation group tolerance it replaces: a unit now has
/// a cell of its own to stand on rather than sharing one destination with the
/// whole group, so "close enough" no longer has to cover a crowd.
pub const FORMATION_ARRIVAL_CELLS: f32 = 0.25;

/// Where one member of a group is going: the group's shared anchor, and the
/// member's own slot around it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FormationGoal {
    pub anchor: Cell,
    pub slot: Cell,
}

impl FormationGoal {
    /// A goal that is its own anchor — a single unit sent to one cell, which
    /// is what an order with nothing to spread out around (a hauler heading
    /// back to a drop-off) walks.
    pub(crate) fn at(cell: Cell) -> Self {
        Self {
            anchor: cell,
            slot: cell,
        }
    }

    pub(crate) fn anchor_center(self) -> [f32; 2] {
        cell_center(self.anchor)
    }

    pub(crate) fn slot_center(self) -> [f32; 2] {
        cell_center(self.slot)
    }

    /// Squared radius of the capture ring around the anchor: inside it a unit
    /// steers at its own slot instead of descending the shared field.
    pub(crate) fn capture_radius2(self) -> f32 {
        let d =
            dist2(self.anchor_center(), self.slot_center()).sqrt() + FORMATION_CAPTURE_MARGIN_CELLS;
        d * d
    }
}

/// Why a group order was refused. Every variant leaves the world untouched.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum FormationError {
    /// Nothing in the group was a live, player-owned unit eligible for this
    /// target — a Soldier at a build site, or a whole group of workers sent to
    /// a depleted node.
    #[error("no unit in the group can take this order")]
    NoUnits,
    /// The target id is stale, is not the kind of thing the order names, or is
    /// a resource node with nothing left in it.
    #[error("the order's target no longer exists")]
    NoTarget,
    /// No navigation field can be built to the group's anchor.
    #[error("no navigation field can be built to the group's anchor")]
    Unreachable,
    /// Fewer legal slots exist around the anchor than the group has members.
    /// Nothing is ordered: a formation is placed whole or not at all.
    #[error("no legal slot exists for every member of the group")]
    NoFormationSpace,
}

/// Preallocated working storage for one formation plan.
///
/// Reserved once, at world load: planning runs on a click and must not
/// allocate, exactly like every other per-frame path in the engine.
#[derive(Debug, Default)]
pub struct FormationScratch {
    /// Per-cell "already handed to a member of this plan" mask, `width *
    /// height` long. Cleared by unsetting exactly the cells this plan set, so
    /// clearing costs the plan's own size rather than the grid's.
    reserved_cells: Vec<bool>,
    /// The slot assigned to each planned unit, parallel to
    /// [`Self::planned_units`] once [`Self::plan`] has returned `Ok`.
    planned_slots: Vec<Cell>,
    /// The group's members, ascending by entity slot.
    planned_units: Vec<EntityId>,
}

impl FormationScratch {
    /// Reserve for a `cells`-cell grid. The two plan buffers are reserved to
    /// [`MAX_ENTITIES`], the largest group the store can ever hold.
    pub(crate) fn new(cells: usize) -> Self {
        Self {
            reserved_cells: vec![false; cells],
            planned_slots: Vec::with_capacity(MAX_ENTITIES),
            planned_units: Vec::with_capacity(MAX_ENTITIES),
        }
    }

    /// Start a new plan: drop the previous one's members and slots.
    pub(crate) fn begin(&mut self) {
        self.planned_units.clear();
        self.planned_slots.clear();
    }

    /// Add one member. The caller pushes in ascending entity-slot order, which
    /// is what makes the plan independent of its argument order.
    pub(crate) fn push_unit(&mut self, id: EntityId) {
        self.planned_units.push(id);
    }

    pub(crate) fn len(&self) -> usize {
        self.planned_units.len()
    }

    pub(crate) fn unit(&self, i: usize) -> EntityId {
        self.planned_units[i]
    }

    pub(crate) fn slot(&self, i: usize) -> Cell {
        self.planned_slots[i]
    }

    /// Plan one slot per member around `anchor`, or refuse the whole group.
    ///
    /// The algorithm, in the order it runs:
    ///
    /// 1. candidate slots are the [`FORMATION_SPACING_CELLS`]-cell lattice
    ///    around the anchor, enumerated in Chebyshev rings (the anchor itself
    ///    first) and row-major inside a ring;
    /// 2. a candidate is legal when its centre is a legal body position, it is
    ///    reachable in the anchor's own field, the anchor can be swept to it
    ///    with a body radius, it is not already reserved by this plan, and no
    ///    unit outside the group stands on it;
    /// 3. the first `len()` legal candidates are the slot set — fewer than
    ///    that is [`FormationError::NoFormationSpace`] and nothing is written;
    /// 4. each member, in ascending entity-slot order, takes the remaining
    ///    slot nearest its current position, ties broken by the lower flat
    ///    cell index.
    pub(crate) fn plan(
        &mut self,
        static_nav: &StaticNav,
        store: &EntityStore,
        nav: &FieldPool,
        field_slot: u8,
        anchor: Cell,
    ) -> Result<(), FormationError> {
        let n = self.planned_units.len();
        if n == 0 {
            return Err(FormationError::NoUnits);
        }
        self.planned_slots.clear();

        let width = static_nav.width();
        let height = static_nav.height();
        let anchor_center = cell_center(anchor);
        // Every in-bounds lattice point is inside this ring count.
        let max_ring = width.max(height) as i32 / FORMATION_SPACING_CELLS + 1;

        'rings: for r in 0..=max_ring {
            for dy in -r..=r {
                for dx in -r..=r {
                    if dx.abs().max(dy.abs()) != r {
                        continue;
                    }
                    let Some(cell) = lattice_cell(anchor, dx, dy, width, height) else {
                        continue;
                    };
                    if !self.slot_is_legal(static_nav, store, nav, field_slot, anchor_center, cell)
                    {
                        continue;
                    }
                    self.reserved_cells[flat_index(cell, width)] = true;
                    self.planned_slots.push(cell);
                    if self.planned_slots.len() == n {
                        break 'rings;
                    }
                }
            }
        }

        // The reservation mask is a within-plan device only; the cells it
        // names are already in `planned_slots`, so it is released either way.
        for i in 0..self.planned_slots.len() {
            self.reserved_cells[flat_index(self.planned_slots[i], width)] = false;
        }

        if self.planned_slots.len() < n {
            self.planned_slots.clear();
            return Err(FormationError::NoFormationSpace);
        }

        self.assign(store, width);
        Ok(())
    }

    /// Whether `cell` may hold one member of this plan.
    fn slot_is_legal(
        &self,
        static_nav: &StaticNav,
        store: &EntityStore,
        nav: &FieldPool,
        field_slot: u8,
        anchor_center: [f32; 2],
        cell: Cell,
    ) -> bool {
        let idx = flat_index(cell, static_nav.width());
        if static_nav.center_blocked()[idx] || self.reserved_cells[idx] {
            return false;
        }
        if !nav.reachable(field_slot as usize, cell) {
            return false;
        }
        let center = cell_center(cell);
        if !static_nav.sweep_clear(anchor_center, center, RTS_UNIT_BODY_RADIUS_CELLS) {
            return false;
        }
        // Clear of every body that is not this plan's to move: a group's own
        // members are about to leave where they stand, anyone else is not.
        for slot in 0..store.slot_count() {
            if !store.alive(slot) {
                continue;
            }
            let EntityKind::Unit(kind) = store.kind(slot) else {
                continue;
            };
            let Some(id) = store.id_at(slot) else {
                continue;
            };
            if self.planned_units.contains(&id) {
                continue;
            }
            if units_overlap(
                center,
                RTS_UNIT_BODY_RADIUS_CELLS,
                store.position(slot),
                kind.body_radius_cells(),
            ) {
                return false;
            }
        }
        true
    }

    /// Match members to slots: each member in turn takes the nearest slot no
    /// earlier member has taken. Ties (equal squared distance) go to the lower
    /// flat cell index, so the match never depends on enumeration order.
    fn assign(&mut self, store: &EntityStore, width: u32) {
        for i in 0..self.planned_units.len() {
            let Some(slot) = store.slot(self.planned_units[i]) else {
                continue;
            };
            let p = store.position(slot);
            let mut best = i;
            let mut best_d = dist2(p, cell_center(self.planned_slots[i]));
            let mut best_idx = flat_index(self.planned_slots[i], width);
            for j in (i + 1)..self.planned_slots.len() {
                let d = dist2(p, cell_center(self.planned_slots[j]));
                let idx = flat_index(self.planned_slots[j], width);
                if d < best_d || (d == best_d && idx < best_idx) {
                    best = j;
                    best_d = d;
                    best_idx = idx;
                }
            }
            self.planned_slots.swap(i, best);
        }
    }
}

/// The cell a group sent to `preferred` actually forms up on: the nearest cell
/// whose centre is a legal body position, scored by squared offset from
/// `preferred` and then by flat cell index.
///
/// A click is a point and a body is three cells wide, so the clicked cell is
/// very often not a cell a body may stand on — the inside of a building's
/// clearance, a cell against a wall. Snapping is what makes such a click an
/// order the group can finish instead of one it can never start. `None` only
/// when the grid holds no legal body position at all.
pub(crate) fn nearest_body_clear_cell(static_nav: &StaticNav, preferred: Cell) -> Option<Cell> {
    let width = static_nav.width();
    let height = static_nav.height();
    let blocked = static_nav.center_blocked();
    let target = cell_center(preferred);
    let mut best: Option<(f32, u32)> = None;
    for y in 0..height {
        for x in 0..width {
            let idx = (x + y * width) as usize;
            if blocked[idx] {
                continue;
            }
            let d = dist2(cell_center(Cell { x, y }), target);
            let idx = idx as u32;
            if best.is_none_or(|(bd, bi)| d < bd || (d == bd && idx < bi)) {
                best = Some((d, idx));
            }
        }
    }
    best.map(|(_, idx)| Cell {
        x: idx % width,
        y: idx / width,
    })
}

/// The centre of a cell.
pub(crate) fn cell_center(c: Cell) -> [f32; 2] {
    [c.x as f32 + 0.5, c.y as f32 + 0.5]
}

fn flat_index(c: Cell, width: u32) -> usize {
    (c.x + c.y * width) as usize
}

/// The lattice point `(dx, dy)` steps of [`FORMATION_SPACING_CELLS`] from
/// `anchor`, or `None` when it falls off the grid.
fn lattice_cell(anchor: Cell, dx: i32, dy: i32, width: u32, height: u32) -> Option<Cell> {
    let x = anchor.x as i64 + (dx * FORMATION_SPACING_CELLS) as i64;
    let y = anchor.y as i64 + (dy * FORMATION_SPACING_CELLS) as i64;
    if x < 0 || y < 0 || x >= width as i64 || y >= height as i64 {
        return None;
    }
    Some(Cell {
        x: x as u32,
        y: y as u32,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lone_goal_captures_at_the_bare_margin() {
        let g = FormationGoal::at(Cell { x: 10, y: 10 });
        assert_eq!(
            g.capture_radius2(),
            FORMATION_CAPTURE_MARGIN_CELLS * FORMATION_CAPTURE_MARGIN_CELLS
        );
    }

    #[test]
    fn the_lattice_is_one_body_diameter_wide() {
        assert_eq!(
            FORMATION_SPACING_CELLS as f32,
            super::super::entity::RTS_UNIT_BODY_DIAMETER_CELLS,
            "two neighbouring slots must be exactly one body diameter apart"
        );
    }

    #[test]
    fn a_ring_offset_off_the_grid_is_no_cell() {
        assert_eq!(lattice_cell(Cell { x: 2, y: 2 }, -1, 0, 64, 64), None);
        assert_eq!(
            lattice_cell(Cell { x: 8, y: 8 }, 1, 1, 64, 64),
            Some(Cell { x: 14, y: 14 })
        );
    }
}
