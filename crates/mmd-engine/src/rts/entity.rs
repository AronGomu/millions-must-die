//! Preallocated SoA entity store with generational ids.

use sha2::{Digest, Sha256};

use crate::scenario;

/// Hard ceiling on simultaneous RTS entities: units + buildings + nodes.
///
/// Sized from the design pillar, not guessed: the supply cap tops out at
/// `scenario::MAX_SUPPLY_CAP` (500) and the cheapest unit costs one supply, so
/// at most 500 units can exist, plus a bounded number of buildings and the
/// scene's nodes. 2 048 leaves room for all of it and for a construction site
/// per queued building without ever reallocating a column.
pub const MAX_ENTITIES: usize = 2_048;

/// Owner id of the human player.
pub const OWNER_PLAYER: u8 = 0;
/// Owner id of unowned world objects (resource nodes).
pub const OWNER_NEUTRAL: u8 = 255;

/// Producible unit kinds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum UnitKind {
    Worker = 0,
    Soldier = 1,
}

/// Placeable building kinds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum BuildingKind {
    Hq = 0,
    Depot = 1,
    Barracks = 2,
}

/// Harvestable resource kinds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum ResourceKind {
    Crystal = 0,
    Gas = 1,
}

/// What an entity is. Discriminants are appended, never inserted — a recorded
/// state hash names a kind by value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EntityKind {
    Unit(UnitKind),
    Building(BuildingKind),
    Node(ResourceKind),
}

impl EntityKind {
    /// One stable byte per kind, for the state hash. `0x1_`=unit, `0x2_`=building,
    /// `0x3_`=node, low nibble the inner discriminant.
    pub fn tag(self) -> u8 {
        match self {
            Self::Unit(k) => 0x10 | (k as u8),
            Self::Building(k) => 0x20 | (k as u8),
            Self::Node(k) => 0x30 | (k as u8),
        }
    }

    /// Footprint edge in cells. Units and nodes occupy a single cell (`1`).
    pub fn footprint_cells(self) -> u32 {
        match self {
            Self::Unit(_) | Self::Node(_) => 1,
            Self::Building(k) => k.footprint_cells(),
        }
    }
}

impl BuildingKind {
    /// Footprint edge in cells, from the scenario contract's constants.
    pub fn footprint_cells(self) -> u32 {
        match self {
            Self::Hq => scenario::HQ_FOOTPRINT_CELLS,
            Self::Depot => scenario::DEPOT_FOOTPRINT_CELLS,
            Self::Barracks => scenario::BARRACKS_FOOTPRINT_CELLS,
        }
    }

    /// Whether workers may return cargo here. Only the HQ, in this slice.
    pub fn is_drop_off(self) -> bool {
        matches!(self, Self::Hq)
    }
}

/// Sentinel in the `carry_kind` column meaning "carrying nothing".
///
/// A separate byte rather than `Option<ResourceKind>` so the column stays a
/// plain `Vec<u8>` the state hash can feed in one `update`.
pub const CARRY_NONE: u8 = 0xFF;

/// A stable handle into [`EntityStore`].
///
/// The generation is what makes a handle safe to hold across a despawn: a slot
/// reused by a new entity gets a higher generation, so the old handle resolves
/// to `None` instead of silently naming the newcomer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct EntityId {
    pub index: u32,
    pub generation: u32,
}

/// Preallocated SoA store. Every column has `MAX_ENTITIES` capacity from
/// construction and is never grown.
#[derive(Debug, Clone)]
pub struct EntityStore {
    alive: Vec<bool>,
    generation: Vec<u32>,
    kind: Vec<EntityKind>,
    owner: Vec<u8>,
    x: Vec<f32>,
    y: Vec<f32>,
    dir: Vec<u8>,
    frame: Vec<u8>,
    progress: Vec<u32>,
    progress_target: Vec<u32>,
    amount: Vec<u32>,
    carry_kind: Vec<u8>,
    carry_amount: Vec<u32>,
    /// LIFO free list of dead slot indices.
    free: Vec<u32>,
    live: usize,
}

impl Default for EntityStore {
    fn default() -> Self {
        Self::new()
    }
}

impl EntityStore {
    pub fn new() -> Self {
        Self {
            alive: Vec::with_capacity(MAX_ENTITIES),
            generation: Vec::with_capacity(MAX_ENTITIES),
            kind: Vec::with_capacity(MAX_ENTITIES),
            owner: Vec::with_capacity(MAX_ENTITIES),
            x: Vec::with_capacity(MAX_ENTITIES),
            y: Vec::with_capacity(MAX_ENTITIES),
            dir: Vec::with_capacity(MAX_ENTITIES),
            frame: Vec::with_capacity(MAX_ENTITIES),
            progress: Vec::with_capacity(MAX_ENTITIES),
            progress_target: Vec::with_capacity(MAX_ENTITIES),
            amount: Vec::with_capacity(MAX_ENTITIES),
            carry_kind: Vec::with_capacity(MAX_ENTITIES),
            carry_amount: Vec::with_capacity(MAX_ENTITIES),
            free: Vec::with_capacity(MAX_ENTITIES),
            live: 0,
        }
    }

    /// Live entity count.
    pub fn len(&self) -> usize {
        self.live
    }

    pub fn is_empty(&self) -> bool {
        self.live == 0
    }

    /// Slots ever allocated — the iteration bound. Includes dead slots.
    pub fn slot_count(&self) -> usize {
        self.alive.len()
    }

    /// Allocate an entity at cell-space position `pos`.
    ///
    /// Returns `None` when the store is full: a caller that cannot spawn must
    /// see it, not silently drop a unit it charged the player for.
    pub fn spawn(&mut self, kind: EntityKind, owner: u8, pos: [f32; 2]) -> Option<EntityId> {
        let idx = if let Some(i) = self.free.pop() {
            i as usize
        } else if self.alive.len() < MAX_ENTITIES {
            let i = self.alive.len();
            self.alive.push(false);
            self.generation.push(0);
            self.kind.push(kind);
            self.owner.push(owner);
            self.x.push(0.0);
            self.y.push(0.0);
            self.dir.push(0);
            self.frame.push(0);
            self.progress.push(0);
            self.progress_target.push(0);
            self.amount.push(0);
            self.carry_kind.push(CARRY_NONE);
            self.carry_amount.push(0);
            i
        } else {
            return None;
        };

        self.alive[idx] = true;
        self.kind[idx] = kind;
        self.owner[idx] = owner;
        self.x[idx] = pos[0];
        self.y[idx] = pos[1];
        self.dir[idx] = 0;
        self.frame[idx] = 0;
        self.progress[idx] = 0;
        self.progress_target[idx] = 0;
        self.amount[idx] = 0;
        self.carry_kind[idx] = CARRY_NONE;
        self.carry_amount[idx] = 0;
        self.live += 1;

        Some(EntityId {
            index: idx as u32,
            generation: self.generation[idx],
        })
    }

    /// Free a slot. Returns `false` for a stale or already-dead id.
    ///
    /// Callers own per-slot side tables and must clear them before this slot is
    /// reused. In particular, despawning a building must clear its production
    /// queue and rally point.
    pub fn despawn(&mut self, id: EntityId) -> bool {
        if !self.contains(id) {
            return false;
        }
        let idx = id.index as usize;
        self.alive[idx] = false;
        self.generation[idx] = self.generation[idx].wrapping_add(1);
        self.free.push(idx as u32);
        self.live -= 1;
        true
    }

    /// Whether `id` names a live entity.
    pub fn contains(&self, id: EntityId) -> bool {
        let idx = id.index as usize;
        idx < self.alive.len() && self.alive[idx] && self.generation[idx] == id.generation
    }

    /// Slot index of a live id.
    pub fn slot(&self, id: EntityId) -> Option<usize> {
        self.contains(id).then_some(id.index as usize)
    }

    /// The id currently occupying `slot`, if it is live.
    pub fn id_at(&self, slot: usize) -> Option<EntityId> {
        if slot < self.alive.len() && self.alive[slot] {
            Some(EntityId {
                index: slot as u32,
                generation: self.generation[slot],
            })
        } else {
            None
        }
    }

    fn assert_live(&self, slot: usize) {
        assert!(self.alive[slot], "slot {slot} is dead");
    }

    pub fn alive(&self, slot: usize) -> bool {
        self.alive[slot]
    }

    pub fn kind(&self, slot: usize) -> EntityKind {
        self.assert_live(slot);
        self.kind[slot]
    }

    pub fn owner(&self, slot: usize) -> u8 {
        self.assert_live(slot);
        self.owner[slot]
    }

    pub fn position(&self, slot: usize) -> [f32; 2] {
        self.assert_live(slot);
        [self.x[slot], self.y[slot]]
    }

    pub fn dir(&self, slot: usize) -> u8 {
        self.assert_live(slot);
        self.dir[slot]
    }

    pub fn frame(&self, slot: usize) -> u8 {
        self.assert_live(slot);
        self.frame[slot]
    }

    /// Construction/production progress in ticks; `0` when nothing is in progress.
    pub fn progress(&self, slot: usize) -> u32 {
        self.assert_live(slot);
        self.progress[slot]
    }

    /// Ticks the current progress must reach; `0` when nothing is in progress.
    pub fn progress_target(&self, slot: usize) -> u32 {
        self.assert_live(slot);
        self.progress_target[slot]
    }

    /// Remaining amount in a resource node; `0` for every other kind.
    pub fn amount(&self, slot: usize) -> u32 {
        self.assert_live(slot);
        self.amount[slot]
    }

    pub fn set_position(&mut self, slot: usize, pos: [f32; 2]) {
        self.assert_live(slot);
        self.x[slot] = pos[0];
        self.y[slot] = pos[1];
    }

    pub fn set_dir(&mut self, slot: usize, dir: u8) {
        self.assert_live(slot);
        self.dir[slot] = dir;
    }

    pub fn set_frame(&mut self, slot: usize, frame: u8) {
        self.assert_live(slot);
        self.frame[slot] = frame;
    }

    pub fn set_progress(&mut self, slot: usize, progress: u32, target: u32) {
        self.assert_live(slot);
        self.progress[slot] = progress;
        self.progress_target[slot] = target;
    }

    pub fn set_amount(&mut self, slot: usize, amount: u32) {
        self.assert_live(slot);
        self.amount[slot] = amount;
    }

    /// What this unit is carrying, and how much. `None` when empty-handed.
    pub fn carry(&self, slot: usize) -> Option<(ResourceKind, u32)> {
        self.assert_live(slot);
        match self.carry_kind[slot] {
            CARRY_NONE => None,
            k if k == ResourceKind::Crystal as u8 => {
                Some((ResourceKind::Crystal, self.carry_amount[slot]))
            }
            k if k == ResourceKind::Gas as u8 => Some((ResourceKind::Gas, self.carry_amount[slot])),
            k => unreachable!("invalid carry_kind byte {k}"),
        }
    }

    /// Set the carried cargo. `None` clears both columns.
    pub fn set_carry(&mut self, slot: usize, cargo: Option<(ResourceKind, u32)>) {
        self.assert_live(slot);
        match cargo {
            None => {
                self.carry_kind[slot] = CARRY_NONE;
                self.carry_amount[slot] = 0;
            }
            Some((kind, amount)) => {
                self.carry_kind[slot] = kind as u8;
                self.carry_amount[slot] = amount;
            }
        }
    }

    /// Live slot indices in ascending order, into a caller-owned buffer.
    ///
    /// Takes an `&mut Vec` rather than returning one so a per-tick sweep costs
    /// no allocation. The buffer is cleared first.
    pub fn collect_live(&self, out: &mut Vec<usize>) {
        out.clear();
        for (i, &a) in self.alive.iter().enumerate() {
            if a {
                out.push(i);
            }
        }
    }

    /// Feed every live slot's state into `h`, in ascending slot order.
    pub fn hash_into(&self, h: &mut Sha256) {
        for i in 0..self.alive.len() {
            if !self.alive[i] {
                continue;
            }
            h.update([self.kind[i].tag()]);
            h.update([self.owner[i]]);
            h.update(self.x[i].to_bits().to_le_bytes());
            h.update(self.y[i].to_bits().to_le_bytes());
            h.update([self.dir[i]]);
            h.update([self.frame[i]]);
            h.update(self.progress[i].to_le_bytes());
            h.update(self.progress_target[i].to_le_bytes());
            h.update(self.amount[i].to_le_bytes());
            h.update([self.carry_kind[i]]);
            h.update(self.carry_amount[i].to_le_bytes());
        }
    }

    /// Capacity of every preallocated column, in declaration order. A test
    /// hook only: proves the zero-growth contract without exposing the
    /// storage layout to production code.
    #[cfg(feature = "testkit")]
    pub fn column_capacities(&self) -> [usize; 13] {
        [
            self.alive.capacity(),
            self.generation.capacity(),
            self.kind.capacity(),
            self.owner.capacity(),
            self.x.capacity(),
            self.y.capacity(),
            self.dir.capacity(),
            self.frame.capacity(),
            self.progress.capacity(),
            self.progress_target.capacity(),
            self.amount.capacity(),
            self.carry_kind.capacity(),
            self.carry_amount.capacity(),
        ]
    }
}
