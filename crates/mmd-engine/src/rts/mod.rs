//! Phase-1 real-time-strategy world: entities, economy, orders, buildings.

mod build;
mod collision;
mod combat;
mod economy;
mod entity;
mod formation;
mod hud;
mod minimap;
mod orders;
mod pack;
mod production;
mod selection;
mod static_nav;
mod world;

pub use build::{
    BARRACKS_BUILD_TICKS, BARRACKS_COST, BARRACKS_SUPPLY_GRANT, DEPOT_BUILD_TICKS, DEPOT_COST,
    DEPOT_SUPPLY_GRANT, EXTRA_BUILDERS_SPEED_UP, HQ_BUILD_TICKS, HQ_COST, HQ_SUPPLY_GRANT,
    Placement, PlacementCandidate, PlacementError, TURRET_BUILD_TICKS, TURRET_COST,
    TURRET_SUPPLY_GRANT, build_ticks, building_cost, footprint_cells, ghost_min_corner,
    placement_candidate, placement_valid, snap_to_build_square, supply_grant,
};
pub use collision::{
    GATHER_PAIR_ACTIVE, GATHER_SEPARATION_STEP_CELLS, GATHER_SEPARATION_TICKS,
    moving_circle_hits_point, units_overlap,
};
pub use combat::{Weapon, building_weapon, weapon};
pub use economy::{
    GATHER_TICKS, Resources, SOLDIER_SUPPLY_COST, Supply, WORKER_CARRY_CAPACITY,
    WORKER_SUPPLY_COST, node_amount, supply_cost,
};
pub use entity::{
    BARRACKS_ARMOR, BARRACKS_MAX_HP, BuildingKind, CARRY_NONE, DEPOT_ARMOR, DEPOT_MAX_HP, EntityId,
    EntityKind, EntityStore, HQ_ARMOR, HQ_MAX_HP, MAX_ENTITIES, OWNER_ENEMY, OWNER_NEUTRAL,
    OWNER_PLAYER, RTS_UNIT_BODY_DIAMETER_CELLS, RTS_UNIT_BODY_RADIUS_CELLS, ResourceKind,
    SOLDIER_ARMOR, SOLDIER_MAX_HP, TURRET_ARMOR, TURRET_MAX_HP, UnitKind, WORKER_ARMOR,
    WORKER_MAX_HP, armor, max_hp,
};
pub use formation::{
    FORMATION_ARRIVAL_CELLS, FORMATION_CAPTURE_MARGIN_CELLS, FORMATION_SPACING_CELLS,
    FormationError, FormationGoal, FormationScratch,
};
pub use hud::{
    AudioChannelId, BOTTOM_PANEL_RECT, BUILD_MENU, CAMERA_POLY_PX, CAMERA_POLY_TINT,
    COMMAND_GRID_RECT, COMMAND_ICON_PX, COMMAND_PANEL_RECT, COMMAND_SLOT_KEYS, CONFINE_CHECKBOX,
    CONFINE_CONTROL_RECT, CONTROL_FRAME_PX, CONTROL_TINT_DISABLED, CONTROL_TINT_HOVER,
    CONTROL_TINT_IDLE, CONTROL_TINT_PRESSED, CONTROL_TINT_SELECTED, CommandId, CommandSlot,
    ControlId, ControlVisualState, DETAIL_TEXT_X, DETAIL_TEXT_Y, EDGE_PAN_TRACK, FOCUS_CHECKBOX,
    FOCUS_CONTROL_RECT, GRID_CHECKBOX, GRID_CONTROL_RECT, HudLayout, ICON_PX, InteractionSnapshot,
    KEYBOARD_PAN_TRACK, MASTER_TRACK, MENU_RECT, MENU_TEXT_POS, MINIMAP_ENEMY_DOT_PX,
    MINIMAP_ENEMY_TINT, MINIMAP_MAP_RECT, MINIMAP_PANEL_RECT, MULTI_ICON_CAP, MULTI_ICON_COLS,
    MULTI_ICON_GAP_PX, MULTI_ICON_ORIGIN, MULTI_ICON_PX, MULTI_ICON_ROWS, MUSIC_TRACK,
    MUTE_LABEL_H, MUTE_LABEL_RECTS, MUTE_LABEL_W, MUTE_LABELS, MUTE_LABELS_MUTED, ModalHit,
    ModalPage, ModalSnapshot, NUM_BUF, NUMERIC_SETTING_SPECS, NumericSettingId, NumericSettingSpec,
    PAN_MAX, PAN_MIN, PAN_STEP, PANEL_LINE_PX, PANEL_TEXT_SCALE, PANEL_TINT, PORTRAIT_POS,
    PORTRAIT_PX, SELECTION_PANEL_RECT, SETTINGS_CONTENT_HEIGHT_PX, SETTINGS_SCROLL_STEP_PX,
    SETTINGS_SCROLLBAR_MIN_THUMB_PX, SFX_TRACK, SLIDER_FILL_TINT, SLIDER_THUMB_OVERHANG_PX,
    SLIDER_THUMB_TINT, SLIDER_THUMB_W_PX, SLIDER_TRACK_TINT, TEXT_TINT, TEXT_TINT_BLOCKED,
    TEXT_TINT_HOTKEY, TOP_BAR_RECT, TOP_TEXT_SCALE, VALUE_FIELD_H, VALUE_FIELD_W, VALUE_FIELD_X,
    VOICE_TRACK, VOLUME_MAX, VOLUME_MIN, VOLUME_STEP, WINDOW_MODE_BUTTONS, WINDOW_MODE_LABELS,
    clamp_settings_scroll, clamp_snap, clip_sprite_to_rect, command_slot_rect, command_slots,
    control_id_from_modal_hit, control_tint, control_visual_state, fmt_ratio, fmt_u32, kind_label,
    modal_hit_test, mute_label_rect, numeric_id_from_field_control, numeric_id_from_slider_control,
    pack_hud, pack_hud_interactive, pack_modal, pack_modal_interactive, settings_max_scroll,
    settings_scrollbar_thumb, slider_thumb_rect, snap_numeric_at_x, snap_track, value_field_rect,
};
pub use minimap::{
    HudHit, MinimapProjection, control_id_from_hud_hit, hud_hit_test, minimap_projection,
};
pub use orders::{
    GHOUL_SPEED_CELLS_PER_SEC, GatherPhase, NAV_CENTER_TOLERANCE_CELLS, Order, OrderTable,
    SOLDIER_SPEED_CELLS_PER_SEC, WORKER_SPEED_CELLS_PER_SEC, interaction_reach, unit_speed,
};
pub use pack::{
    DEATH_FLASH_FRAMES, DEATH_FLASH_INNER, DEATH_FLASH_OUTER, DEATH_FLASH_TINT,
    DRAG_BOX_BORDER_TINT, DRAG_BOX_FILL_TINT, DRAG_BOX_THICKNESS_PX, DeathFlashes, DragBox,
    FramePackOptions, GHOST_TINT, GRID_LINE_PX, GRID_TINT, HP_BAR_BACKING_TINT, HP_BAR_GREEN_TINT,
    HP_BAR_RAISE_CELLS, HP_BAR_RED_TINT, HP_BAR_YELLOW_TINT, MAX_GRID_LINES, Prop, RtsFrame,
    SELECTION_RING_INNER, SELECTION_RING_OUTER, SELECTION_TINT, building_uv, hp_bar_fill_tint,
    node_uv, pack_frame, pack_frame_with_options, prop_uv, unit_slot,
};
pub use production::{
    PRODUCTION_QUEUE_CAP, ProduceError, ProductionQueue, ProductionTable, SOLDIER_COST,
    SOLDIER_PRODUCE_TICKS, WORKER_COST, WORKER_PRODUCE_TICKS, can_produce, produce_ticks,
    unit_cost,
};
pub use selection::{
    DRAG_MIN_PX, MAX_SELECTION, Pick, RTS_SPRITE_SIZE_PX, Selection, box_select,
    building_pick_contains, building_plot_contains, building_quad_contains, building_quad_px,
    building_screen_rect, entity_pick_depth, footprint_contains, footprint_min, is_drag,
    normalise_rect, pick_at, sprite_screen_rect, unit_pick_contains,
};
pub use static_nav::{StaticNav, StaticNavError};
pub use world::{
    CommandReceipt, CommandRejectReason, ContextOrderReason, ContextOrderResult,
    DEFAULT_CAMERA_PAN_SPEED, DamageResult, DeathEvent, IssuedOrder, NODE_CRYSTAL_AMOUNT,
    NODE_GAS_AMOUNT, OrderReceiptBuffer, RtsWorld, RtsWorldError, TickError, UnitOrderReceipt,
};
