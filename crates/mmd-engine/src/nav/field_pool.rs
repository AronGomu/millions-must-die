//! A small preallocated pool of flow fields, keyed by destination cell.
//!
//! The engine rule is flow fields, never per-entity pathfinding. A player,
//! unlike a horde, issues orders to arbitrary destinations, so the answer is a
//! bounded set of fields sharing one grid: units sent to the same cell share
//! one field, which is also where group cohesion comes from.

use crate::scenario::Cell;

use super::flow_field::{COST_UNREACHABLE, FieldScratch, FlowField, FlowFieldError};

/// Flow fields held simultaneously. Eight is the working set an RTS actually
/// needs: a player rarely has more than a handful of distinct live
/// destinations, and each field costs `width * height * 12` bytes.
pub const NAV_FIELD_SLOTS: usize = 8;

/// A handle to a pooled field: the slot, plus the epoch that slot carried when
/// the handle was issued.
///
/// A bare slot number is **not** a handle. A slot is rebuilt for a new
/// destination on an LRU miss, and every key is dropped when the obstacle mask
/// changes, so the field behind a slot is only the field a caller asked for
/// until someone else asks for something else. The epoch is what lets
/// [`FieldPool::is_current`] answer "is this still my field?" instead of the
/// caller assuming it is and walking a unit into a wall.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FieldRef {
    /// Slot index, `0..NAV_FIELD_SLOTS`.
    pub slot: u8,
    /// The slot's epoch when this handle was issued. `0` is never issued.
    pub epoch: u64,
}

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
    /// Epoch stamped into each slot by its last rebuild. `0` means never built.
    epochs: [u64; NAV_FIELD_SLOTS],
    /// Epoch issued to the last rebuild — the source of [`Self::epochs`].
    epoch_clock: u64,
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
            epochs: [0; NAV_FIELD_SLOTS],
            epoch_clock: 0,
            clock: 0,
            rebuilds: 0,
            scratch: FieldScratch::reserve_worst_case(n),
        })
    }

    /// Build a pool over a grid from a caller-owned `blocked` mask directly —
    /// `width * height` long, one bool per cell — rather than a sparse
    /// obstacle-index list. This is how the RTS world feeds a pool the
    /// radius-inflated centre mask [`crate::rts::StaticNav::center_blocked`]
    /// produces, which has no sparse-index representation.
    pub fn from_blocked_mask(
        width: u32,
        height: u32,
        blocked: &[bool],
    ) -> Result<Self, FieldPoolError> {
        if width == 0 || height == 0 {
            return Err(FieldPoolError::Field(FlowFieldError::EmptyGrid));
        }
        let n = (width as usize)
            .checked_mul(height as usize)
            .ok_or(FieldPoolError::Field(FlowFieldError::EmptyGrid))?;
        if blocked.len() != n {
            return Err(FieldPoolError::MaskLength {
                got: blocked.len(),
                expected: n,
            });
        }

        let mut fields = Vec::with_capacity(NAV_FIELD_SLOTS);
        for _ in 0..NAV_FIELD_SLOTS {
            fields.push(FlowField::blank(width, height).map_err(FieldPoolError::Field)?);
        }

        Ok(Self {
            width,
            height,
            blocked: blocked.to_vec(),
            fields,
            keys: [None; NAV_FIELD_SLOTS],
            last_used: [0; NAV_FIELD_SLOTS],
            epochs: [0; NAV_FIELD_SLOTS],
            epoch_clock: 0,
            clock: 0,
            rebuilds: 0,
            scratch: FieldScratch::reserve_worst_case(n),
        })
    }

    /// Replace the whole blocked mask in place and invalidate every cached
    /// field, without allocating: `blocked` must already be this pool's
    /// `width * height` length, and the copy writes into the mask's existing
    /// buffer.
    pub fn replace_blocked_mask(&mut self, blocked: &[bool]) -> Result<(), FieldPoolError> {
        if blocked.len() != self.blocked.len() {
            return Err(FieldPoolError::MaskLength {
                got: blocked.len(),
                expected: self.blocked.len(),
            });
        }
        self.blocked.copy_from_slice(blocked);
        self.keys = [None; NAV_FIELD_SLOTS];
        Ok(())
    }

    /// Whether `cell` has a finite integration cost in the field held at
    /// `field_slot` — reachable from that field's destination, as opposed to
    /// blocked or disconnected. Out-of-bounds is unreachable.
    pub fn reachable(&self, field_slot: usize, cell: Cell) -> bool {
        if field_slot >= NAV_FIELD_SLOTS || cell.x >= self.width || cell.y >= self.height {
            return false;
        }
        self.fields[field_slot].cost_at(cell.x, cell.y) < COST_UNREACHABLE
    }

    /// Slot holding a field to `dest`, rebuilding into the least-recently-used
    /// slot on a miss. Every call counts as a use, so a destination in constant
    /// use is never evicted.
    pub fn acquire(&mut self, dest: Cell) -> Result<FieldRef, FieldPoolError> {
        self.clock += 1;
        if let Some(slot) = self.keys.iter().position(|k| *k == Some(dest)) {
            self.last_used[slot] = self.clock;
            return Ok(FieldRef {
                slot: slot as u8,
                epoch: self.epochs[slot],
            });
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
        self.epoch_clock = next_epoch(self.epoch_clock);
        self.epochs[victim] = self.epoch_clock;
        self.rebuilds += 1;
        Ok(FieldRef {
            slot: victim as u8,
            epoch: self.epoch_clock,
        })
    }

    /// Whether `handle` still names a field built to `dest`.
    ///
    /// Both halves are load-bearing. The key catches a slot rebuilt for some
    /// other destination; the epoch catches a slot rebuilt for the *same*
    /// destination over a changed mask — a different field answering the same
    /// question, which the holder must re-acquire to be walking the live one.
    pub fn is_current(&self, handle: FieldRef, dest: Cell) -> bool {
        let slot = handle.slot as usize;
        slot < NAV_FIELD_SLOTS && self.keys[slot] == Some(dest) && self.epochs[slot] == handle.epoch
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
    /// Invalidation is on the *keys*: a handle issued before this call stops
    /// being current ([`Self::is_current`]) and its holder must re-acquire.
    /// Nothing is rebuilt here — the rebuild happens when someone asks for the
    /// destination again, so a mask change costs nothing for a field nobody
    /// is using any more.
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

/// Advance the field epoch without ever issuing the reserved value zero.
///
/// Wrapping is deliberate: rebuilding must remain total even after the clock
/// reaches its integer limit. Skipping zero preserves the handle invariant.
#[inline]
fn next_epoch(epoch: u64) -> u64 {
    let next = epoch.wrapping_add(1);
    if next == 0 { 1 } else { next }
}

#[cfg(test)]
mod tests {
    use super::next_epoch;

    #[test]
    fn epoch_wrap_skips_the_reserved_zero() {
        assert_eq!(next_epoch(u64::MAX), 1);
    }
}
