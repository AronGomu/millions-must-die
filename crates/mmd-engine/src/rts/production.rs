//! Per-building production queues and rally points — T11.

use sha2::{Digest, Sha256};

use crate::scenario::Cell;

use super::economy::Resources;
use super::entity::{BuildingKind, MAX_ENTITIES, UnitKind};

/// Entries a building may hold, including the one in progress.
///
/// Five, as in the genre this one clones. It is small enough that the whole
/// queue fits in the state hash as five bytes and large enough that a player
/// can bank a build order or two.
pub const PRODUCTION_QUEUE_CAP: usize = 5;

/// Cost of each producible unit.
pub const WORKER_COST: Resources = Resources {
    crystal: 50,
    gas: 0,
};
pub const SOLDIER_COST: Resources = Resources {
    crystal: 50,
    gas: 25,
};

/// Cost of a producible unit kind.
pub fn unit_cost(kind: UnitKind) -> Resources {
    match kind {
        UnitKind::Worker => WORKER_COST,
        UnitKind::Soldier => SOLDIER_COST,
    }
}

/// Ticks each unit takes to build. 60 ticks = 1 second.
pub const WORKER_PRODUCE_TICKS: u32 = 300;
pub const SOLDIER_PRODUCE_TICKS: u32 = 360;

/// Ticks a unit kind takes to produce.
pub fn produce_ticks(kind: UnitKind) -> u32 {
    match kind {
        UnitKind::Worker => WORKER_PRODUCE_TICKS,
        UnitKind::Soldier => SOLDIER_PRODUCE_TICKS,
    }
}

/// Which building produces which unit. The whole build tree of this slice.
///
/// A Depot produces nothing — it exists to raise the supply cap, and a
/// building that both raises supply and produces would make the supply
/// mechanic unobservable.
pub fn can_produce(building: BuildingKind, unit: UnitKind) -> bool {
    matches!(
        (building, unit),
        (BuildingKind::Hq, UnitKind::Worker) | (BuildingKind::Barracks, UnitKind::Soldier)
    )
}

/// Why an enqueue was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ProduceError {
    #[error("no such building")]
    NoBuilding,
    #[error("this building cannot produce that unit")]
    WrongBuilding,
    #[error("the building is still under construction")]
    UnderConstruction,
    #[error("the production queue is full")]
    QueueFull,
    #[error("not enough resources")]
    Unaffordable,
    #[error("not enough supply")]
    SupplyBlocked,
    #[error("the entity store is full")]
    StoreFull,
}

/// One building's queue: a fixed ring plus the head's elapsed ticks.
///
/// Backed by a plain `[UnitKind; CAP]` with only the first `len` entries
/// meaningful, rather than `[Option<UnitKind>; CAP]` — `UnitKind` carries no
/// `Default`, and this way `entries()` can hand back a `&[UnitKind]` slice
/// directly instead of filtering an `Option` array on every read. A slot past
/// `len` is always reset to a canonical placeholder on shrink, so two queues
/// that reach the same logical state compare equal regardless of history.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProductionQueue {
    entries: [UnitKind; PRODUCTION_QUEUE_CAP],
    len: u8,
    progress: u32,
}

impl Default for ProductionQueue {
    fn default() -> Self {
        Self {
            entries: [UnitKind::Worker; PRODUCTION_QUEUE_CAP],
            len: 0,
            progress: 0,
        }
    }
}

impl ProductionQueue {
    pub fn len(&self) -> usize {
        self.len as usize
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn is_full(&self) -> bool {
        self.len as usize == PRODUCTION_QUEUE_CAP
    }

    /// The unit currently being produced.
    pub fn head(&self) -> Option<UnitKind> {
        (self.len > 0).then_some(self.entries[0])
    }

    /// Ticks elapsed on the head.
    pub fn progress(&self) -> u32 {
        self.progress
    }

    /// Queue contents, oldest first.
    pub fn entries(&self) -> &[UnitKind] {
        &self.entries[..self.len as usize]
    }

    /// Push at the back. `false` when full.
    pub fn push(&mut self, kind: UnitKind) -> bool {
        if self.is_full() {
            return false;
        }
        self.entries[self.len as usize] = kind;
        self.len += 1;
        true
    }

    /// Remove entry `index` (0 = the one in progress). Returns what was
    /// removed. Removing the head resets `progress` to zero — a partly built
    /// unit is not carried over to the next entry.
    pub fn cancel(&mut self, index: usize) -> Option<UnitKind> {
        if index >= self.len as usize {
            return None;
        }
        let removed = self.entries[index];
        let len = self.len as usize;
        for i in index..len - 1 {
            self.entries[i] = self.entries[i + 1];
        }
        self.entries[len - 1] = UnitKind::Worker;
        self.len -= 1;
        if index == 0 {
            self.progress = 0;
        }
        Some(removed)
    }

    /// Advance the head by one tick, **saturating** at its build time.
    ///
    /// Ticking never removes an entry. A finished head sits at
    /// [`produce_ticks`] and waits to be taken by [`Self::pop_ready`], which
    /// the production system only calls once the unit has actually been
    /// placed on the grid: a building with nowhere legal to put a finished
    /// unit must keep it — the player has already paid for it and its supply
    /// is already reserved — rather than drop it or produce it into another
    /// body. Saturating (not free-running) progress is what makes that wait a
    /// stable state: a blocked head hashes the same every tick it waits.
    pub fn tick_head(&mut self) {
        if self.len == 0 {
            return;
        }
        let need = produce_ticks(self.entries[0]);
        if self.progress < need {
            self.progress += 1;
        }
    }

    /// Whether the head has served its full build time and is waiting to be
    /// placed.
    pub fn head_ready(&self) -> bool {
        self.head()
            .is_some_and(|kind| self.progress >= produce_ticks(kind))
    }

    /// Take a ready head, resetting progress for the next entry. `None` when
    /// the queue is empty or its head is not ready — so a caller that could
    /// not place the unit simply does not call this, and the entry stays
    /// exactly where it was.
    pub fn pop_ready(&mut self) -> Option<UnitKind> {
        if !self.head_ready() {
            return None;
        }
        let kind = self.entries[0];
        self.progress = 0;
        let len = self.len as usize;
        for i in 0..len - 1 {
            self.entries[i] = self.entries[i + 1];
        }
        self.entries[len - 1] = UnitKind::Worker;
        self.len -= 1;
        Some(kind)
    }

    /// Feed this queue into the state hash: a fixed five bytes of entry tags
    /// (`0` = empty, matching no live `UnitKind`, `1 + kind as u8` for a real
    /// entry) plus the head's progress — fixed width regardless of `len`, so
    /// the digest cannot alias a shorter queue against a longer one.
    pub fn hash_into(&self, h: &mut Sha256) {
        for i in 0..PRODUCTION_QUEUE_CAP {
            let tag = if i < self.len as usize {
                1 + self.entries[i] as u8
            } else {
                0
            };
            h.update([tag]);
        }
        h.update(self.progress.to_le_bytes());
    }
}

/// One queue and one rally point per entity slot, preallocated to
/// [`MAX_ENTITIES`].
#[derive(Debug, Clone)]
pub struct ProductionTable {
    queues: Vec<ProductionQueue>,
    rally: Vec<Option<Cell>>,
}

impl Default for ProductionTable {
    fn default() -> Self {
        Self::new()
    }
}

impl ProductionTable {
    pub fn new() -> Self {
        Self {
            queues: vec![ProductionQueue::default(); MAX_ENTITIES],
            rally: vec![None; MAX_ENTITIES],
        }
    }

    pub fn queue(&self, slot: usize) -> &ProductionQueue {
        &self.queues[slot]
    }

    pub fn queue_mut(&mut self, slot: usize) -> &mut ProductionQueue {
        &mut self.queues[slot]
    }

    pub fn rally(&self, slot: usize) -> Option<Cell> {
        self.rally[slot]
    }

    pub fn set_rally(&mut self, slot: usize, cell: Option<Cell>) {
        self.rally[slot] = cell;
    }

    /// Reset a slot — called when a building is despawned so a reused slot
    /// does not inherit a queue.
    pub fn clear(&mut self, slot: usize) {
        self.queues[slot] = ProductionQueue::default();
        self.rally[slot] = None;
    }

    /// Feed every live slot's queue and rally point into the state hash,
    /// ascending. Fixed width per slot, matching [`super::orders::OrderTable::hash_into`]'s
    /// contract: a slot with no queue (never a building, or cleared) hashes
    /// its zeroed payload rather than being skipped.
    pub fn hash_into(&self, h: &mut Sha256, live: &[usize]) {
        for &slot in live {
            self.queues[slot].hash_into(h);
            match self.rally[slot] {
                None => {
                    h.update([0u8]);
                    h.update(0u32.to_le_bytes());
                    h.update(0u32.to_le_bytes());
                }
                Some(c) => {
                    h.update([1u8]);
                    h.update(c.x.to_le_bytes());
                    h.update(c.y.to_le_bytes());
                }
            }
        }
    }
}
