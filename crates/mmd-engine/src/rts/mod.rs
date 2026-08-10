//! Phase-1 real-time-strategy world: entities, economy, orders, buildings.

mod economy;
mod entity;
mod orders;
mod selection;
mod world;

pub use economy::{Resources, SOLDIER_SUPPLY_COST, Supply, WORKER_SUPPLY_COST, supply_cost};
pub use entity::{
    BuildingKind, EntityId, EntityKind, EntityStore, MAX_ENTITIES, OWNER_NEUTRAL, OWNER_PLAYER,
    ResourceKind, UnitKind,
};
pub use orders::{
    ARRIVAL_RADIUS_CELLS, Order, OrderTable, SOLDIER_SPEED_CELLS_PER_SEC,
    WORKER_SPEED_CELLS_PER_SEC, unit_speed,
};
pub use selection::{
    DRAG_MIN_PX, MAX_SELECTION, Pick, Selection, UNIT_PICK_RADIUS_SCALE, box_select,
    footprint_contains, footprint_min, is_drag, normalise_rect, pick_at,
};
pub use world::{NODE_CRYSTAL_AMOUNT, NODE_GAS_AMOUNT, RtsWorld, RtsWorldError};
