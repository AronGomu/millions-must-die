//! The phase-1 RTS game state: seeded from a scenario, ticked deterministically.

use sha2::{Digest, Sha256};

use crate::scenario::{self, Scenario};

use super::economy::{Resources, Supply, WORKER_SUPPLY_COST};
use super::entity::{
    BuildingKind, EntityId, EntityKind, EntityStore, OWNER_NEUTRAL, OWNER_PLAYER, ResourceKind,
    UnitKind,
};

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

        Ok(Self {
            scenario,
            entities,
            resources,
            supply,
            tick_index: 0,
            start_hq: Some(start_hq),
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

    /// Advance one fixed 1/60 s step.
    ///
    /// Systems are added by later tickets and each one runs at a fixed point in
    /// this order, so a reordering is a visible diff rather than an accident:
    /// 1. commands, 2. camera, 3. construction, 4. production, 5. orders,
    /// 6. movement, 7. supply recount. Today only the tick counter advances.
    pub fn tick(&mut self) {
        self.tick_index += 1;
    }

    /// Exact same-host state digest.
    ///
    /// Covers `tick_index`, live entity count, then every live slot in
    /// ascending order (kind tag, owner, x bits, y bits, dir, frame, progress,
    /// progress_target, amount), then resources and supply. `f32` goes in as
    /// raw bits, matching `Simulation::state_hash`.
    pub fn state_hash(&self) -> [u8; 32] {
        let mut h = Sha256::new();
        h.update(self.tick_index.to_le_bytes());
        h.update((self.entities.len() as u64).to_le_bytes());
        self.entities.hash_into(&mut h);
        h.update(self.resources.crystal.to_le_bytes());
        h.update(self.resources.gas.to_le_bytes());
        h.update(self.supply.used().to_le_bytes());
        h.update(self.supply.cap().to_le_bytes());
        h.finalize().into()
    }
}
