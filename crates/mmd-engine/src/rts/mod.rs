//! Phase-1 real-time-strategy world: entities, economy, orders, buildings.

mod build;
mod economy;
mod entity;
mod orders;
mod pack;
mod production;
mod selection;
mod world;

pub use build::{
    BARRACKS_BUILD_TICKS, BARRACKS_COST, BARRACKS_SUPPLY_GRANT, BUILD_REACH_CELLS,
    DEPOT_BUILD_TICKS, DEPOT_COST, DEPOT_SUPPLY_GRANT, EXTRA_BUILDERS_SPEED_UP, HQ_BUILD_TICKS,
    HQ_COST, HQ_SUPPLY_GRANT, Placement, PlacementError, build_ticks, building_cost,
    footprint_cells, placement_valid, supply_grant,
};
pub use economy::{
    DROP_OFF_REACH_CELLS, GATHER_REACH_CELLS, GATHER_TICKS, Resources, SOLDIER_SUPPLY_COST, Supply,
    WORKER_CARRY_CAPACITY, WORKER_SUPPLY_COST, node_amount, supply_cost,
};
pub use entity::{
    BuildingKind, CARRY_NONE, EntityId, EntityKind, EntityStore, MAX_ENTITIES, OWNER_NEUTRAL,
    OWNER_PLAYER, ResourceKind, UnitKind,
};
pub use orders::{
    ARRIVAL_RADIUS_CELLS, GatherPhase, Order, OrderTable, SOLDIER_SPEED_CELLS_PER_SEC,
    WORKER_SPEED_CELLS_PER_SEC, unit_speed,
};
pub use pack::{
    DRAG_BOX_THICKNESS_PX, DRAG_BOX_TINT, DragBox, GHOST_TINT, Prop, RtsFrame,
    SELECTION_RING_INNER, SELECTION_RING_OUTER, SELECTION_TINT, building_quad_px, building_uv,
    ghost_min_corner, node_uv, pack_frame, prop_uv, unit_slot,
};
pub use production::{
    PRODUCTION_QUEUE_CAP, ProduceError, ProductionQueue, ProductionTable, SOLDIER_COST,
    SOLDIER_PRODUCE_TICKS, WORKER_COST, WORKER_PRODUCE_TICKS, can_produce, produce_ticks,
    unit_cost,
};
pub use selection::{
    DRAG_MIN_PX, MAX_SELECTION, Pick, Selection, UNIT_PICK_RADIUS_SCALE, box_select,
    footprint_contains, footprint_min, is_drag, normalise_rect, pick_at,
};
pub use world::{NODE_CRYSTAL_AMOUNT, NODE_GAS_AMOUNT, RtsWorld, RtsWorldError};
