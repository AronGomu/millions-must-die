//! The phase-1 RTS game state: seeded from a scenario, ticked deterministically.

use sha2::{Digest, Sha256};

use crate::nav::field_pool::{FieldPool, FieldPoolError};
use crate::render::IsoView;
use crate::scenario::{self, Cell, Scenario};
use crate::sim::{TICK_DT, dir_from_vector};

use super::economy::{Resources, Supply, WORKER_SUPPLY_COST};
use super::entity::{
    BuildingKind, EntityId, EntityKind, EntityStore, MAX_ENTITIES, OWNER_NEUTRAL, OWNER_PLAYER,
    ResourceKind, UnitKind,
};
use super::orders::{ARRIVAL_RADIUS_CELLS, Order, OrderTable, step_admissible, unit_speed};
use super::selection::{Pick, Selection, box_select, pick_at};

/// Starting amount in a freshly seeded Crystal node.
pub const NODE_CRYSTAL_AMOUNT: u32 = 1_500;
/// Starting amount in a freshly seeded Gas node.
pub const NODE_GAS_AMOUNT: u32 = 2_500;

/// Failures building an [`RtsWorld`].
#[derive(Debug, thiserror::Error)]
pub enum RtsWorldError {
    #[error(transparent)]
    Scenario(#[from] crate::scenario::ScenarioError),
    #[error("scenario {version} carries no rts block; the rts world needs one")]
    NotAnRtsScene { version: String },
    #[error("entity store full while seeding: {what}")]
    StoreFull { what: String },
    #[error("navigation pool: {0}")]
    Nav(#[from] FieldPoolError),
}

/// The phase-1 RTS game state.
#[derive(Debug)]
pub struct RtsWorld {
    scenario: Scenario,
    entities: EntityStore,
    resources: Resources,
    supply: Supply,
    tick_index: u64,
    start_hq: Option<EntityId>,
    nav: FieldPool,
    orders: OrderTable,
    /// Live-slot buffer the per-tick sweeps reuse. Reserved to
    /// [`MAX_ENTITIES`] so a tick never grows it.
    live_scratch: Vec<usize>,
    selection: Selection,
    /// Scratch buffer for a box select's result, before it replaces
    /// [`Self::selection`]. Reserved to [`MAX_ENTITIES`] so no selection
    /// operation allocates.
    pick_scratch: Vec<EntityId>,
}

impl RtsWorld {
    /// Load a hash-verified scenario and seed the world from its RTS block.
    pub fn load(path: impl AsRef<std::path::Path>) -> Result<Self, RtsWorldError> {
        let scenario = Scenario::load_verified(path)?;
        Self::from_scenario(scenario)
    }

    /// Seed from an already-validated scenario.
    pub fn from_scenario(scenario: Scenario) -> Result<Self, RtsWorldError> {
        let rts = scenario.rts().ok_or_else(|| RtsWorldError::NotAnRtsScene {
            version: scenario.version().to_string(),
        })?;

        let mut entities = EntityStore::new();

        // 1. The HQ, at its footprint centre. Finished, not a site.
        let hq_pos = [
            rts.hq_cell.x as f32 + scenario::HQ_FOOTPRINT_CELLS as f32 * 0.5,
            rts.hq_cell.y as f32 + scenario::HQ_FOOTPRINT_CELLS as f32 * 0.5,
        ];
        let start_hq = entities
            .spawn(EntityKind::Building(BuildingKind::Hq), OWNER_PLAYER, hq_pos)
            .ok_or_else(|| RtsWorldError::StoreFull {
                what: "hq".to_string(),
            })?;

        // 2. Crystal nodes, then gas nodes, each at its cell centre.
        for c in &rts.crystal_nodes {
            let pos = [c.x as f32 + 0.5, c.y as f32 + 0.5];
            let id = entities
                .spawn(EntityKind::Node(ResourceKind::Crystal), OWNER_NEUTRAL, pos)
                .ok_or_else(|| RtsWorldError::StoreFull {
                    what: "crystal node".to_string(),
                })?;
            entities.set_amount(id.index as usize, NODE_CRYSTAL_AMOUNT);
        }
        for c in &rts.gas_nodes {
            let pos = [c.x as f32 + 0.5, c.y as f32 + 0.5];
            let id = entities
                .spawn(EntityKind::Node(ResourceKind::Gas), OWNER_NEUTRAL, pos)
                .ok_or_else(|| RtsWorldError::StoreFull {
                    what: "gas node".to_string(),
                })?;
            entities.set_amount(id.index as usize, NODE_GAS_AMOUNT);
        }

        // 3. One worker per scenario spawn cell.
        let mut worker_count: u32 = 0;
        for c in scenario.spawn_cells() {
            let pos = [c.x as f32 + 0.5, c.y as f32 + 0.5];
            entities
                .spawn(EntityKind::Unit(UnitKind::Worker), OWNER_PLAYER, pos)
                .ok_or_else(|| RtsWorldError::StoreFull {
                    what: "worker".to_string(),
                })?;
            worker_count += 1;
        }

        let resources = Resources {
            crystal: rts.start_crystal,
            gas: rts.start_gas,
        };
        let mut supply = Supply::new(rts.start_supply_cap);
        supply.add_used(WORKER_SUPPLY_COST * worker_count);

        let nav = FieldPool::new(
            scenario.width(),
            scenario.height(),
            scenario.obstacle_cells(),
        )?;

        Ok(Self {
            scenario,
            entities,
            resources,
            supply,
            tick_index: 0,
            start_hq: Some(start_hq),
            nav,
            orders: OrderTable::new(),
            live_scratch: Vec::with_capacity(MAX_ENTITIES),
            selection: Selection::new(),
            pick_scratch: Vec::with_capacity(MAX_ENTITIES),
        })
    }

    pub fn scenario(&self) -> &Scenario {
        &self.scenario
    }

    pub fn entities(&self) -> &EntityStore {
        &self.entities
    }

    pub fn entities_mut(&mut self) -> &mut EntityStore {
        &mut self.entities
    }

    /// Direct resource-stock mutation. A test hook: phase 1 has no order
    /// system to spend resources through yet, and [`Self::state_hash`] needs
    /// exercising against a spend regardless.
    #[cfg(feature = "testkit")]
    pub fn resources_mut(&mut self) -> &mut Resources {
        &mut self.resources
    }

    pub fn resources(&self) -> Resources {
        self.resources
    }

    pub fn supply(&self) -> Supply {
        self.supply
    }

    pub fn tick_index(&self) -> u64 {
        self.tick_index
    }

    /// The starting HQ. `None` only after it is destroyed, which nothing in
    /// phase 1 can do.
    pub fn start_hq(&self) -> Option<EntityId> {
        self.start_hq
    }

    /// The navigation pool. Buildings stamp obstacles into it (T10).
    pub fn nav(&self) -> &FieldPool {
        &self.nav
    }

    pub fn selection(&self) -> &Selection {
        &self.selection
    }

    pub fn selection_mut(&mut self) -> &mut Selection {
        &mut self.selection
    }

    /// Apply a plain click: replace the selection with what was picked, or
    /// clear it when nothing was.
    pub fn click_select(&mut self, view: &IsoView, screen: [f32; 2]) -> Pick {
        let pick = pick_at(self, view, screen);
        self.selection.clear();
        match pick {
            Pick::Unit(id) | Pick::Building(id) | Pick::Node(id) => {
                self.selection.insert(id);
            }
            Pick::Nothing => {}
        }
        pick
    }

    /// Apply an additive (shift) click: toggle what was picked. A click on
    /// nothing leaves the selection alone — a modifier click is a refinement,
    /// and clearing on a near-miss is the single most annoying selection bug in
    /// the genre.
    pub fn shift_click_select(&mut self, view: &IsoView, screen: [f32; 2]) -> Pick {
        let pick = pick_at(self, view, screen);
        match pick {
            Pick::Unit(id) | Pick::Building(id) | Pick::Node(id) => {
                self.selection.toggle(id);
            }
            Pick::Nothing => {}
        }
        pick
    }

    /// Apply a drag rectangle: replace the selection with every own unit inside.
    /// An empty box clears the selection.
    pub fn box_select_into_selection(&mut self, view: &IsoView, a: [f32; 2], b: [f32; 2]) -> usize {
        let mut scratch = std::mem::take(&mut self.pick_scratch);
        box_select(self, view, a, b, &mut scratch);
        self.selection.replace(&scratch);
        self.pick_scratch = scratch;
        self.selection.len()
    }

    /// Order one unit to walk to `dest`.
    ///
    /// Returns `false` when `id` is stale, is not a unit, is not owned by
    /// [`OWNER_PLAYER`], or `dest` is out of bounds or blocked — a right-click on
    /// a rock must be a no-op, not an order nobody can finish.
    pub fn order_move(&mut self, id: EntityId, dest: Cell) -> bool {
        let Some(slot) = self.orderable_slot(id) else {
            return false;
        };
        let Ok(field_slot) = self.nav.acquire(dest) else {
            return false;
        };
        self.orders.set(slot, Order::Move { dest, field_slot });
        true
    }

    /// Order several units to one destination, acquiring the field **once**.
    ///
    /// This is the API the input layer uses. Issuing N single orders would
    /// acquire N times, and on a full pool that is N rebuilds of the same field.
    pub fn order_move_group(&mut self, ids: &[EntityId], dest: Cell) -> usize {
        let Ok(field_slot) = self.nav.acquire(dest) else {
            return 0;
        };
        let mut ordered = 0;
        for &id in ids {
            if let Some(slot) = self.orderable_slot(id) {
                self.orders.set(slot, Order::Move { dest, field_slot });
                ordered += 1;
            }
        }
        ordered
    }

    /// The current order of a live entity.
    pub fn order_of(&self, id: EntityId) -> Option<Order> {
        self.entities.slot(id).map(|slot| self.orders.get(slot))
    }

    /// Slot of a live player-owned unit — the only thing an order applies to.
    fn orderable_slot(&self, id: EntityId) -> Option<usize> {
        let slot = self.entities.slot(id)?;
        if !matches!(self.entities.kind(slot), EntityKind::Unit(_)) {
            return None;
        }
        if self.entities.owner(slot) != OWNER_PLAYER {
            return None;
        }
        Some(slot)
    }

    /// Advance one fixed 1/60 s step.
    ///
    /// Systems are added by later tickets and each one runs at a fixed point in
    /// this order, so a reordering is a visible diff rather than an accident:
    /// 1. commands, 2. camera, 3. construction, 4. production, 5. orders,
    /// 6. movement, 7. supply recount.
    ///
    /// Today the tick counter and the movement system (6) run, followed by
    /// pruning the selection of anything that died this tick — last, so a
    /// unit that died on this tick is out of the selection before anything
    /// reads it next tick.
    pub fn tick(&mut self) {
        self.tick_index += 1;
        self.movement();
        self.selection.retain_live(&self.entities);
    }

    /// System 6: walk every unit under a move order one step down its field.
    ///
    /// The step obeys the same admissibility rule the horde walk obeys
    /// ([`super::orders::step_admissible`]), so a unit can never be placed in a
    /// walkable-but-unreachable pocket it could not then leave.
    fn movement(&mut self) {
        let width = self.scenario.width();
        let height = self.scenario.height();

        self.entities.collect_live(&mut self.live_scratch);
        for i in 0..self.live_scratch.len() {
            let slot = self.live_scratch[i];
            let EntityKind::Unit(kind) = self.entities.kind(slot) else {
                continue;
            };
            let Order::Move { dest, field_slot } = self.orders.get(slot) else {
                continue;
            };

            let p = self.entities.position(slot);
            // 1. Arrival, against the destination cell centre: a group is sent
            //    to one cell and only one of them can stand on it.
            let dx = p[0] - (dest.x as f32 + 0.5);
            let dy = p[1] - (dest.y as f32 + 0.5);
            if dx * dx + dy * dy <= ARRIVAL_RADIUS_CELLS * ARRIVAL_RADIUS_CELLS {
                self.orders.clear(slot);
                continue;
            }

            // 2. Sample the field at the unit's own cell.
            let cx = p[0].floor() as i32;
            let cy = p[1].floor() as i32;
            if cx < 0 || cy < 0 || cx >= width as i32 || cy >= height as i32 {
                continue;
            }
            let (vx, vy) = self.nav.field(field_slot).vector_at(cx as u32, cy as u32);
            if vx == 0.0 && vy == 0.0 {
                // Unreachable, or already on the destination cell: stop rather
                // than spin on an order that can never complete.
                self.orders.clear(slot);
                continue;
            }

            // 3. Step, with the horde's admissibility rule.
            let step = unit_speed(kind) * TICK_DT;
            let nx = p[0] + vx * step;
            let ny = p[1] + vy * step;
            if step_admissible(cx, cy, nx, ny, width, height, self.nav.blocked()) {
                self.entities.set_position(slot, [nx, ny]);
            }
            self.entities.set_dir(slot, dir_from_vector(vx, vy));
            let f = (self.entities.frame(slot) + 1) % 4;
            self.entities.set_frame(slot, f);
        }
    }

    /// Exact same-host state digest.
    ///
    /// Covers `tick_index`, live entity count, then every live slot in
    /// ascending order (kind tag, owner, x bits, y bits, dir, frame, progress,
    /// progress_target, amount), then every live slot's order, then the
    /// selection, then resources and supply. `f32` goes in as raw bits,
    /// matching `Simulation::state_hash`.
    pub fn state_hash(&self) -> [u8; 32] {
        let mut h = Sha256::new();
        h.update(self.tick_index.to_le_bytes());
        h.update((self.entities.len() as u64).to_le_bytes());
        self.entities.hash_into(&mut h);
        // Not `live_scratch`: hashing is a `&self` read and must not disturb a
        // buffer the tick owns. This path runs outside the tick, never in it.
        let mut live = Vec::with_capacity(self.entities.len());
        self.entities.collect_live(&mut live);
        self.orders.hash_into(&mut h, &live);
        self.selection.hash_into(&mut h);
        h.update(self.resources.crystal.to_le_bytes());
        h.update(self.resources.gas.to_le_bytes());
        h.update(self.supply.used().to_le_bytes());
        h.update(self.supply.cap().to_le_bytes());
        h.finalize().into()
    }
}
