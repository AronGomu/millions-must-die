//! A small preallocated pool of flow fields, keyed by destination cell.
//!
//! The engine rule is flow fields, never per-entity pathfinding. A player,
//! unlike a horde, issues orders to arbitrary destinations, so the answer is a
//! bounded set of fields sharing one grid: units sent to the same cell share
//! one field, which is also where group cohesion comes from.

use crate::scenario::Cell;

use super::flow_field::{FieldScratch, FlowField, FlowFieldError};

/// Flow fields held simultaneously. Eight is the working set an RTS actually
/// needs: a player rarely has more than a handful of distinct live
/// destinations, and each field costs `width * height * 12` bytes.
pub const NAV_FIELD_SLOTS: usize = 8;

/// Pool errors.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum FieldPoolError {
    #[error("flow field: {0:?}")]
    Field(FlowFieldError),
    #[error("blocked mask length {got}, expected {expected}")]
    MaskLength { got: usize, expected: usize },
}

/// A fixed set of flow fields keyed by destination cell, LRU-evicted.
///
/// # Allocation
///
/// Every field and the scratch heap are allocated in [`Self::new`]. A **hit**
/// allocates nothing at all. A **miss** rebuilds into those reused buffers and
/// may grow the scratch heap once, as the first rebuild at a grid size settles
/// its high-water mark — that miss is the single bounded exception to the
/// engine's no-allocation-after-warmup rule, and nothing else here is exempt.
#[derive(Debug)]
pub struct FieldPool {
    width: u32,
    height: u32,
    /// `width * height` obstacle mask, indexed `x + y * width`.
    blocked: Vec<bool>,
    fields: Vec<FlowField>,
    keys: [Option<Cell>; NAV_FIELD_SLOTS],
    /// Clock reading of each slot's last use. `0` means never used.
    last_used: [u64; NAV_FIELD_SLOTS],
    clock: u64,
    rebuilds: u64,
    scratch: FieldScratch,
}

impl FieldPool {
    /// Build a pool over a grid, with `obstacle_cells` as the initial mask.
    ///
    /// Every slot is allocated here — this is the only place the pool
    /// allocates for fields — and the scratch heap is reserved to the grid's
    /// size, so a later rebuild reuses both.
    pub fn new(width: u32, height: u32, obstacle_cells: &[u32]) -> Result<Self, FieldPoolError> {
        if width == 0 || height == 0 {
            return Err(FieldPoolError::Field(FlowFieldError::EmptyGrid));
        }
        let n = (width as usize)
            .checked_mul(height as usize)
            .ok_or(FieldPoolError::Field(FlowFieldError::EmptyGrid))?;

        let mut blocked = vec![false; n];
        for &oi in obstacle_cells {
            if oi as usize >= n {
                return Err(FieldPoolError::Field(FlowFieldError::InvalidObstacleIndex(
                    oi,
                )));
            }
            blocked[oi as usize] = true;
        }

        let mut fields = Vec::with_capacity(NAV_FIELD_SLOTS);
        for _ in 0..NAV_FIELD_SLOTS {
            fields.push(FlowField::blank(width, height).map_err(FieldPoolError::Field)?);
        }

        Ok(Self {
            width,
            height,
            blocked,
            fields,
            keys: [None; NAV_FIELD_SLOTS],
            last_used: [0; NAV_FIELD_SLOTS],
            clock: 0,
            rebuilds: 0,
            scratch: FieldScratch::with_capacity(n),
        })
    }

    /// Slot holding a field to `dest`, rebuilding into the least-recently-used
    /// slot on a miss. Every call counts as a use, so a destination in constant
    /// use is never evicted.
    pub fn acquire(&mut self, dest: Cell) -> Result<u8, FieldPoolError> {
        self.clock += 1;
        if let Some(slot) = self.keys.iter().position(|k| *k == Some(dest)) {
            self.last_used[slot] = self.clock;
            return Ok(slot as u8);
        }

        // Miss: the least recently used slot, ties broken by lowest index. An
        // unused slot has `last_used == 0` and therefore always wins.
        let mut victim = 0usize;
        for slot in 1..NAV_FIELD_SLOTS {
            if self.last_used[slot] < self.last_used[victim] {
                victim = slot;
            }
        }

        // A refused rebuild leaves the victim's field and key untouched, so a
        // right-click on a rock cannot evict a field that is still in use.
        self.fields[victim]
            .rebuild_in_place(dest, &self.blocked, &mut self.scratch)
            .map_err(|e| match e {
                // The pool owns the mask, so a length mismatch is the pool's
                // own invariant breaking, not the caller's input.
                FlowFieldError::MaskLength { got, expected } => {
                    FieldPoolError::MaskLength { got, expected }
                }
                other => FieldPoolError::Field(other),
            })?;
        self.keys[victim] = Some(dest);
        self.last_used[victim] = self.clock;
        self.rebuilds += 1;
        Ok(victim as u8)
    }

    /// Read a slot's field. Panics on an out-of-range slot — slots come from
    /// [`Self::acquire`] and a fabricated one is a caller bug.
    pub fn field(&self, slot: u8) -> &FlowField {
        &self.fields[slot as usize]
    }

    /// The destination a slot currently holds, if any.
    pub fn key(&self, slot: u8) -> Option<Cell> {
        self.keys[slot as usize]
    }

    /// Rebuilds performed since construction. A test needs to see a hit not
    /// rebuild.
    pub fn rebuild_count(&self) -> u64 {
        self.rebuilds
    }

    /// [`Self::acquire`] calls since construction — the LRU clock itself.
    ///
    /// [`Self::rebuild_count`] cannot stand in for this: acquiring one
    /// destination N times is one rebuild and N acquires, so a caller that was
    /// meant to acquire once per *group* and acquires once per *unit* instead
    /// is invisible in the rebuild count.
    pub fn acquire_count(&self) -> u64 {
        self.clock
    }

    /// The blocked mask, `width * height` long, indexed `x + y * width`.
    pub fn blocked(&self) -> &[bool] {
        &self.blocked
    }

    /// Mark a cell blocked or free and invalidate **every** cached field.
    ///
    /// Invalidating all of them rather than the ones that "look affected" is
    /// deliberate: a single new obstacle can change the descent vector anywhere
    /// downstream of it, and a partial invalidation is a bug that only shows up
    /// as units walking into a wall built ten seconds ago.
    ///
    /// An out-of-bounds cell is a no-op: the mask has no such entry to set, and
    /// there is nothing to invalidate.
    pub fn set_blocked(&mut self, cell: Cell, blocked: bool) {
        if cell.x >= self.width || cell.y >= self.height {
            return;
        }
        let idx = (cell.x + cell.y * self.width) as usize;
        self.blocked[idx] = blocked;
        self.keys = [None; NAV_FIELD_SLOTS];
    }

    /// Scratch heap capacity, for the allocation-invariant test.
    pub fn scratch_capacity(&self) -> usize {
        self.scratch.capacity()
    }
}
