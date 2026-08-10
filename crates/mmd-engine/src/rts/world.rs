//! The phase-1 RTS game state: seeded from a scenario, ticked deterministically.

use sha2::{Digest, Sha256};

use crate::nav::field_pool::{FieldPool, FieldPoolError};
use crate::render::IsoView;
use crate::scenario::{self, Cell, Scenario};
use crate::sim::{TICK_DT, dir_from_vector};

use super::build::{
    BUILD_REACH_CELLS, Placement, PlacementError, build_ticks, building_cost, footprint_cells,
    placement_valid, supply_grant,
};
use super::economy::{
    DROP_OFF_REACH_CELLS, GATHER_REACH_CELLS, GATHER_TICKS, Resources, Supply,
    WORKER_CARRY_CAPACITY, WORKER_SUPPLY_COST, node_amount,
};
use super::entity::{
    BuildingKind, EntityId, EntityKind, EntityStore, MAX_ENTITIES, OWNER_NEUTRAL, OWNER_PLAYER,
    ResourceKind, UnitKind,
};
use super::orders::{
    ARRIVAL_RADIUS_CELLS, GatherPhase, Order, OrderTable, building_approach_cell, dist2,
    drop_off_approach_cell, node_cell, rect_distance, step_admissible, unit_speed,
};
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
    /// The pending build ghost.
    placement: Placement,
    /// Per-slot "is this site attended this tick" scratch, reused every tick.
    /// Reserved to [`MAX_ENTITIES`] so construction never allocates.
    build_attend: Vec<bool>,
    /// Sites that finished this tick, reused every tick. Reserved to
    /// [`MAX_ENTITIES`] so construction never allocates.
    finished: Vec<EntityId>,
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
            entities.set_amount(id.index as usize, node_amount(ResourceKind::Crystal));
        }
        for c in &rts.gas_nodes {
            let pos = [c.x as f32 + 0.5, c.y as f32 + 0.5];
            let id = entities
                .spawn(EntityKind::Node(ResourceKind::Gas), OWNER_NEUTRAL, pos)
                .ok_or_else(|| RtsWorldError::StoreFull {
                    what: "gas node".to_string(),
                })?;
            entities.set_amount(id.index as usize, node_amount(ResourceKind::Gas));
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
            placement: Placement::None,
            build_attend: vec![false; MAX_ENTITIES],
            finished: Vec::with_capacity(MAX_ENTITIES),
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

    /// Direct navigation mutation. A test hook: phase 1 has no gameplay path
    /// that blocks an arbitrary cell outside the construction system, and the
    /// approach-cell regression test needs to simulate a stamped HQ.
    #[cfg(feature = "testkit")]
    pub fn nav_mut(&mut self) -> &mut FieldPool {
        &mut self.nav
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

    /// Slot of a live player-owned worker — the only unit kind that can gather.
    fn worker_slot(&self, id: EntityId) -> Option<usize> {
        let slot = self.entities.slot(id)?;
        if !matches!(self.entities.kind(slot), EntityKind::Unit(UnitKind::Worker)) {
            return None;
        }
        if self.entities.owner(slot) != OWNER_PLAYER {
            return None;
        }
        Some(slot)
    }

    /// Order one worker to gather from `node`.
    ///
    /// `false` when: `id` is stale, is not a `UnitKind::Worker`, is not
    /// `OWNER_PLAYER`; `node` is stale or is not an `EntityKind::Node`; the
    /// node is already empty; or no field can be built to the node's cell.
    /// A Soldier cannot gather — refusing is what makes the HUD's "no valid
    /// order" state real rather than cosmetic.
    pub fn order_gather(&mut self, id: EntityId, node: EntityId) -> bool {
        self.order_gather_group(&[id], node) == 1
    }

    /// Order several workers onto one node, acquiring the field once.
    pub fn order_gather_group(&mut self, ids: &[EntityId], node: EntityId) -> usize {
        let Some(node_slot) = self.entities.slot(node) else {
            return 0;
        };
        if !matches!(self.entities.kind(node_slot), EntityKind::Node(_)) {
            return 0;
        }
        if self.entities.amount(node_slot) == 0 {
            return 0;
        }
        let cell = node_cell(self.entities.position(node_slot));
        let Ok(field_slot) = self.nav.acquire(cell) else {
            return 0;
        };
        let mut ordered = 0;
        for &id in ids {
            if let Some(slot) = self.worker_slot(id) {
                self.orders.set(
                    slot,
                    Order::Gather {
                        node,
                        phase: GatherPhase::ToNode { field_slot },
                    },
                );
                ordered += 1;
            }
        }
        ordered
    }

    /// The nearest live drop-off building owned by the player, by distance
    /// from `pos` to its footprint rectangle. Ties go to the lower entity
    /// slot.
    pub fn nearest_drop_off(&self, pos: [f32; 2]) -> Option<EntityId> {
        let slot_count = self.entities.slot_count();
        let mut best: Option<(f32, usize)> = None;
        for slot in 0..slot_count {
            if !self.entities.alive(slot) {
                continue;
            }
            let EntityKind::Building(b) = self.entities.kind(slot) else {
                continue;
            };
            if !b.is_drop_off() {
                continue;
            }
            if self.entities.owner(slot) != OWNER_PLAYER {
                continue;
            }
            let d = rect_distance(pos, self.entities.position(slot), b.footprint_cells());
            if best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, slot));
            }
        }
        best.and_then(|(_, slot)| self.entities.id_at(slot))
    }

    /// The pending build ghost.
    pub fn placement(&self) -> Placement {
        self.placement
    }

    /// Choose a building to place. `false` when the player cannot currently
    /// afford it — the cost check is repeated at [`Self::confirm_placement`],
    /// because the stock can fall while the ghost is up.
    pub fn begin_placement(&mut self, kind: BuildingKind) -> bool {
        if !self.resources.covers(building_cost(kind)) {
            return false;
        }
        self.placement = Placement::Pending { kind };
        true
    }

    /// Drop the ghost. Idempotent.
    pub fn cancel_placement(&mut self) {
        self.placement = Placement::None;
    }

    /// Commit the pending ghost at `min`, built by `builder`.
    ///
    /// On success: debits the cost, spawns the site with
    /// `set_progress(0, build_ticks(kind))`, orders `builder` to
    /// `Order::Build { site, field_slot }`, clears the ghost, and returns the
    /// site's id. The footprint is **not** stamped into navigation yet — a site
    /// is walkable until it finishes, which is what lets the builder stand in it.
    pub fn confirm_placement(
        &mut self,
        min: Cell,
        builder: EntityId,
    ) -> Result<EntityId, PlacementError> {
        // No pending ghost is a caller-contract violation (the UI never calls
        // this without one); there is no dedicated error for it, so this falls
        // back to `NoBuilder` rather than adding an eighth variant for an
        // unreachable-in-practice path.
        let Placement::Pending { kind } = self.placement else {
            return Err(PlacementError::NoBuilder);
        };

        let Some(builder_slot) = self.entities.slot(builder) else {
            return Err(PlacementError::NoBuilder);
        };
        let is_worker = matches!(
            self.entities.kind(builder_slot),
            EntityKind::Unit(UnitKind::Worker)
        );
        if !is_worker || self.entities.owner(builder_slot) != OWNER_PLAYER {
            return Err(PlacementError::NoBuilder);
        }

        let cost = building_cost(kind);
        if !self.resources.covers(cost) {
            return Err(PlacementError::Unaffordable);
        }

        placement_valid(self, kind, min)?;

        let edge = kind.footprint_cells();
        let center = [
            min.x as f32 + edge as f32 * 0.5,
            min.y as f32 + edge as f32 * 0.5,
        ];
        let Some(site) = self
            .entities
            .spawn(EntityKind::Building(kind), OWNER_PLAYER, center)
        else {
            return Err(PlacementError::StoreFull);
        };
        let site_slot = self.entities.slot(site).expect("just spawned");
        self.entities.set_progress(site_slot, 0, build_ticks(kind));

        let debited = self.resources.try_debit(cost);
        debug_assert!(debited, "affordability was just checked above");

        self.order_build(builder, site);
        self.placement = Placement::None;
        Ok(site)
    }

    /// Cancel an unfinished site: refund the full cost, unstamp nothing (a site
    /// was never stamped), despawn it, and clear every worker whose `Build`
    /// order named it.
    ///
    /// A **finished** building is not cancellable and returns `false`.
    pub fn cancel_construction(&mut self, site: EntityId) -> bool {
        let Some(site_slot) = self.entities.slot(site) else {
            return false;
        };
        let EntityKind::Building(kind) = self.entities.kind(site_slot) else {
            return false;
        };
        if self.entities.progress_target(site_slot) == 0 {
            return false;
        }

        self.resources.credit(building_cost(kind));
        self.entities.despawn(site);

        for slot in 0..self.entities.slot_count() {
            if !self.entities.alive(slot) {
                continue;
            }
            if let Order::Build { site: s, .. } = self.orders.get(slot)
                && s == site
            {
                self.orders.clear(slot);
            }
        }
        true
    }

    /// Whether a building entity is still under construction.
    pub fn is_site(&self, id: EntityId) -> bool {
        let Some(slot) = self.entities.slot(id) else {
            return false;
        };
        matches!(self.entities.kind(slot), EntityKind::Building(_))
            && self.entities.progress_target(slot) > 0
    }

    /// Order an existing worker to attend an existing site.
    pub fn order_build(&mut self, id: EntityId, site: EntityId) -> bool {
        let Some(slot) = self.worker_slot(id) else {
            return false;
        };
        let Some(site_slot) = self.entities.slot(site) else {
            return false;
        };
        if !matches!(self.entities.kind(site_slot), EntityKind::Building(_)) {
            return false;
        }
        if self.entities.progress_target(site_slot) == 0 {
            return false;
        }
        let width = self.scenario.width();
        let height = self.scenario.height();
        let cell = building_approach_cell(&self.entities, self.nav.blocked(), width, height, site);
        let Ok(field_slot) = self.nav.acquire(cell) else {
            return false;
        };
        self.orders.set(slot, Order::Build { site, field_slot });
        true
    }

    /// Advance one fixed 1/60 s step.
    ///
    /// Systems are added by later tickets and each one runs at a fixed point in
    /// this order, so a reordering is a visible diff rather than an accident:
    /// 1. commands, 2. camera, 3. construction, 4. production, 5. orders,
    /// 6. movement, 7. supply recount.
    ///
    /// Today the tick counter, the construction system (3), the gather system
    /// (5) and the movement system (6) run, followed by pruning the selection
    /// of anything that died this tick — last, so a unit that died on this
    /// tick is out of the selection before anything reads it next tick.
    pub fn tick(&mut self) {
        self.tick_index += 1;
        self.entities.collect_live(&mut self.live_scratch);
        self.construction();
        self.gather();
        self.movement();
        self.selection.retain_live(&self.entities);
    }

    /// System 3: advance every attended construction site by one tick, finish
    /// sites that reach their target, and clear the orders of workers whose
    /// site just finished.
    ///
    /// Runs before orders (5) and movement (6), so a site that finishes this
    /// tick is finished for everything downstream.
    fn construction(&mut self) {
        // Pass A: which sites have an attending worker this tick?
        self.build_attend.fill(false);
        for i in 0..self.live_scratch.len() {
            let slot = self.live_scratch[i];
            let Order::Build { site, .. } = self.orders.get(slot) else {
                continue;
            };
            if !matches!(self.entities.kind(slot), EntityKind::Unit(UnitKind::Worker)) {
                continue;
            }
            let Some(site_slot) = self.entities.slot(site) else {
                self.orders.clear(slot);
                continue;
            };
            let EntityKind::Building(b) = self.entities.kind(site_slot) else {
                self.orders.clear(slot);
                continue;
            };
            if self.entities.progress_target(site_slot) == 0 {
                self.orders.clear(slot);
                continue;
            }
            if rect_distance(
                self.entities.position(slot),
                self.entities.position(site_slot),
                b.footprint_cells(),
            ) <= BUILD_REACH_CELLS
            {
                self.build_attend[site_slot] = true;
            }
        }

        // Pass B: advance every attended site by exactly one tick.
        self.finished.clear();
        for i in 0..self.live_scratch.len() {
            let slot = self.live_scratch[i];
            let EntityKind::Building(b) = self.entities.kind(slot) else {
                continue;
            };
            let target = self.entities.progress_target(slot);
            if target == 0 {
                continue;
            }
            if !self.build_attend[slot] {
                continue;
            }
            let p = self.entities.progress(slot) + 1;
            if p < target {
                self.entities.set_progress(slot, p, target);
            } else {
                // Finish.
                self.entities.set_progress(slot, 0, 0);
                let center = self.entities.position(slot);
                for cell in footprint_cells(center, b.footprint_cells()) {
                    self.nav.set_blocked(cell, true);
                }
                self.supply.grant_cap(supply_grant(b));
                self.finished.push(self.entities.id_at(slot).expect("live"));
            }
        }

        // Clear the orders of every worker that was building something now
        // finished.
        for i in 0..self.live_scratch.len() {
            let slot = self.live_scratch[i];
            if let Order::Build { site, .. } = self.orders.get(slot)
                && self.finished.contains(&site)
            {
                self.orders.clear(slot);
            }
        }
    }

    /// System 5: advance every gathering worker's round trip one step.
    ///
    /// Runs before movement, so a phase change decided this tick is walked
    /// on this same tick — otherwise the round trip would lag its own state
    /// by one frame.
    fn gather(&mut self) {
        let width = self.scenario.width();
        let height = self.scenario.height();

        for i in 0..self.live_scratch.len() {
            let slot = self.live_scratch[i];
            let Order::Gather { node, phase } = self.orders.get(slot) else {
                continue;
            };

            // The node may have been removed; the order dies with it.
            let Some(node_slot) = self.entities.slot(node) else {
                self.orders.clear(slot);
                continue;
            };
            let EntityKind::Node(res) = self.entities.kind(node_slot) else {
                self.orders.clear(slot);
                continue;
            };
            let node_pos = self.entities.position(node_slot);
            let p = self.entities.position(slot);

            match phase {
                GatherPhase::ToNode { .. } => {
                    if self.entities.amount(node_slot) == 0 {
                        self.orders.clear(slot);
                        continue;
                    }
                    if dist2(p, node_pos) <= GATHER_REACH_CELLS * GATHER_REACH_CELLS {
                        self.orders.set(
                            slot,
                            Order::Gather {
                                node,
                                phase: GatherPhase::Mining {
                                    ticks_left: GATHER_TICKS,
                                },
                            },
                        );
                    }
                    // else: leave it; the movement system walks it down
                    // field_slot toward the node cell.
                }
                GatherPhase::Mining { ticks_left } => {
                    if ticks_left > 1 {
                        self.orders.set(
                            slot,
                            Order::Gather {
                                node,
                                phase: GatherPhase::Mining {
                                    ticks_left: ticks_left - 1,
                                },
                            },
                        );
                    } else {
                        // Load up: take min(capacity, remaining).
                        let take = WORKER_CARRY_CAPACITY.min(self.entities.amount(node_slot));
                        if take == 0 {
                            self.orders.clear(slot);
                            continue;
                        }
                        self.entities
                            .set_amount(node_slot, self.entities.amount(node_slot) - take);
                        self.entities.set_carry(slot, Some((res, take)));
                        match self.nearest_drop_off(p) {
                            Some(d) => {
                                let cell = drop_off_approach_cell(
                                    &self.entities,
                                    self.nav.blocked(),
                                    width,
                                    height,
                                    d,
                                );
                                match self.nav.acquire(cell) {
                                    Ok(fs) => self.orders.set(
                                        slot,
                                        Order::Gather {
                                            node,
                                            phase: GatherPhase::Returning {
                                                drop_off: d,
                                                field_slot: fs,
                                            },
                                        },
                                    ),
                                    Err(_) => self.orders.clear(slot),
                                }
                            }
                            // nowhere to deliver: stop, holding the cargo
                            None => self.orders.clear(slot),
                        }
                    }
                }
                GatherPhase::Returning { drop_off, .. } => {
                    let Some(d_slot) = self.entities.slot(drop_off) else {
                        self.orders.clear(slot);
                        continue;
                    };
                    let EntityKind::Building(b) = self.entities.kind(d_slot) else {
                        self.orders.clear(slot);
                        continue;
                    };
                    if rect_distance(p, self.entities.position(d_slot), b.footprint_cells())
                        <= DROP_OFF_REACH_CELLS
                    {
                        if let Some((kind, amount)) = self.entities.carry(slot) {
                            match kind {
                                ResourceKind::Crystal => self.resources.credit(Resources {
                                    crystal: amount,
                                    gas: 0,
                                }),
                                ResourceKind::Gas => self.resources.credit(Resources {
                                    crystal: 0,
                                    gas: amount,
                                }),
                            }
                            self.entities.set_carry(slot, None);
                        }
                        if self.entities.amount(node_slot) == 0 {
                            self.orders.clear(slot);
                            continue;
                        }
                        match self.nav.acquire(node_cell(node_pos)) {
                            Ok(fs) => self.orders.set(
                                slot,
                                Order::Gather {
                                    node,
                                    phase: GatherPhase::ToNode { field_slot: fs },
                                },
                            ),
                            Err(_) => self.orders.clear(slot),
                        }
                    }
                }
            }
        }
    }

    /// System 6: walk every unit under a `Move` order, or a `Gather` order
    /// mid-transit (`ToNode` or `Returning`), one step down its field.
    ///
    /// The step obeys the same admissibility rule the horde walk obeys
    /// ([`super::orders::step_admissible`]), so a unit can never be placed in a
    /// walkable-but-unreachable pocket it could not then leave.
    fn movement(&mut self) {
        let width = self.scenario.width();
        let height = self.scenario.height();

        for i in 0..self.live_scratch.len() {
            let slot = self.live_scratch[i];
            let EntityKind::Unit(kind) = self.entities.kind(slot) else {
                continue;
            };
            let order = self.orders.get(slot);
            let (dest, field_slot) = match order {
                Order::Move { dest, field_slot } => (dest, field_slot),
                Order::Gather {
                    node,
                    phase: GatherPhase::ToNode { field_slot },
                } => {
                    let Some(node_slot) = self.entities.slot(node) else {
                        continue;
                    };
                    (node_cell(self.entities.position(node_slot)), field_slot)
                }
                Order::Gather {
                    phase:
                        GatherPhase::Returning {
                            drop_off,
                            field_slot,
                        },
                    ..
                } => {
                    if self.entities.slot(drop_off).is_none() {
                        continue;
                    }
                    (
                        drop_off_approach_cell(
                            &self.entities,
                            self.nav.blocked(),
                            width,
                            height,
                            drop_off,
                        ),
                        field_slot,
                    )
                }
                Order::Build { site, field_slot } => {
                    let Some(site_slot) = self.entities.slot(site) else {
                        continue;
                    };
                    let EntityKind::Building(b) = self.entities.kind(site_slot) else {
                        continue;
                    };
                    // A worker that has reached the site stops and attends it;
                    // it does not clear the order, since the construction
                    // system — not the mover — decides when a `Build` order ends.
                    if rect_distance(
                        self.entities.position(slot),
                        self.entities.position(site_slot),
                        b.footprint_cells(),
                    ) <= BUILD_REACH_CELLS
                    {
                        continue;
                    }
                    (
                        building_approach_cell(
                            &self.entities,
                            self.nav.blocked(),
                            width,
                            height,
                            site,
                        ),
                        field_slot,
                    )
                }
                // Idle, and Mining (a mining worker stands still).
                _ => continue,
            };
            let is_move_order = matches!(order, Order::Move { .. });

            let p = self.entities.position(slot);
            // 1. Arrival, against the destination cell centre: a group is sent
            //    to one cell and only one of them can stand on it.
            //
            //    Only `Order::Move` stops here. A gathering or building worker's
            //    real completion condition is a *reach* test against a
            //    footprint rectangle (the gather system's drop-off check, or the
            //    `BUILD_REACH_CELLS` guard above), not proximity to the approach
            //    cell's own centre — and since T10 an approach cell sits just
            //    outside that footprint, `ARRIVAL_RADIUS_CELLS` alone can no
            //    longer be trusted to fall inside the reach threshold. Freezing
            //    such an order here, before its own reach test is satisfied,
            //    would strand the unit short of the building it was sent to.
            let dx = p[0] - (dest.x as f32 + 0.5);
            let dy = p[1] - (dest.y as f32 + 0.5);
            if is_move_order && dx * dx + dy * dy <= ARRIVAL_RADIUS_CELLS * ARRIVAL_RADIUS_CELLS {
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
                // than spin on an order that can never complete. Only for
                // `Order::Move`, for the same reason arrival above is.
                if is_move_order {
                    self.orders.clear(slot);
                }
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
    /// progress_target, amount, carry kind, carry amount), then every live
    /// slot's order, then the selection, then the pending placement ghost (one
    /// tag byte plus the kind byte), then resources and supply. `f32` goes in
    /// as raw bits, matching `Simulation::state_hash`.
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
        match self.placement {
            Placement::None => h.update([0u8, 0u8]),
            Placement::Pending { kind } => h.update([1u8, kind as u8]),
        }
        h.update(self.resources.crystal.to_le_bytes());
        h.update(self.resources.gas.to_le_bytes());
        h.update(self.supply.used().to_le_bytes());
        h.update(self.supply.cap().to_le_bytes());
        h.finalize().into()
    }
}
