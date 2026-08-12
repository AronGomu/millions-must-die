//! Phase-1 real-time-strategy world: entities, economy, orders, buildings.

mod build;
mod collision;
mod economy;
mod entity;
mod hud;
mod orders;
mod pack;
mod production;
mod selection;
mod static_nav;
mod world;

pub use build::{
    BARRACKS_BUILD_TICKS, BARRACKS_COST, BARRACKS_SUPPLY_GRANT, DEPOT_BUILD_TICKS, DEPOT_COST,
    DEPOT_SUPPLY_GRANT, EXTRA_BUILDERS_SPEED_UP, HQ_BUILD_TICKS, HQ_COST, HQ_SUPPLY_GRANT,
    Placement, PlacementError, build_ticks, building_cost, footprint_cells, placement_valid,
    supply_grant,
};
pub use collision::{moving_circle_hits_point, units_overlap};
pub use economy::{
    GATHER_TICKS, Resources, SOLDIER_SUPPLY_COST, Supply, WORKER_CARRY_CAPACITY,
    WORKER_SUPPLY_COST, node_amount, supply_cost,
};
pub use entity::{
    BuildingKind, CARRY_NONE, EntityId, EntityKind, EntityStore, MAX_ENTITIES, OWNER_NEUTRAL,
    OWNER_PLAYER, RTS_UNIT_BODY_DIAMETER_CELLS, RTS_UNIT_BODY_RADIUS_CELLS, ResourceKind, UnitKind,
};
pub use hud::{
    BOTTOM_PANEL_RECT, BUILD_MENU, BUILD_MENU_RECT, ICON_PX, NUM_BUF, PANEL_LINE_PX,
    PANEL_TEXT_SCALE, PANEL_TINT, PRODUCTION_RECT, SELECTION_RECT, TEXT_TINT, TEXT_TINT_BLOCKED,
    TEXT_TINT_HOTKEY, TOP_BAR_RECT, TOP_TEXT_SCALE, fmt_ratio, fmt_u32, kind_label, pack_hud,
};
pub use orders::{
    ARRIVAL_RADIUS_CELLS, GatherPhase, NAV_CENTER_TOLERANCE_CELLS, Order, OrderTable,
    SOLDIER_SPEED_CELLS_PER_SEC, WORKER_SPEED_CELLS_PER_SEC, interaction_reach, unit_speed,
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
    DRAG_MIN_PX, MAX_SELECTION, Pick, RTS_SPRITE_SIZE_PX, Selection, box_select, entity_pick_depth,
    footprint_contains, footprint_min, is_drag, normalise_rect, pick_at, sprite_screen_rect,
    unit_pick_contains,
};
pub use static_nav::{StaticNav, StaticNavError};
pub use world::{
    ContextOrderReason, ContextOrderResult, IssuedOrder, NODE_CRYSTAL_AMOUNT, NODE_GAS_AMOUNT,
    OrderReceiptBuffer, RtsWorld, RtsWorldError, TickError, UnitOrderReceipt,
};
