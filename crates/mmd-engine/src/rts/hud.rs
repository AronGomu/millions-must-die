//! On-screen StarCraft-like control HUD — `T11`.
//!
//! Pure layout: reads world state and appends instances to the frame's UI
//! groups. No input, no device, no world mutation. Every number is formatted
//! into a fixed stack buffer, never a `String` — the HUD runs every frame and
//! a heap allocation per number would put four allocations inside the frame
//! path.
//!
//! The `overlay` layer of a `ScenePass` is only honest for procedural rings;
//! a HUD is entirely textured, so every push here lands in one of
//! [`RtsFrame::ui`]'s five groups (worker, soldier, building, props, font).

use crate::render::{DrawGroup, GLYPH_H_PX, SpriteInstance, frame_uv_rect, push_text};

use super::entity::{BuildingKind, EntityId, EntityKind, EntityStore, ResourceKind, UnitKind};
use super::minimap::minimap_projection;
use super::pack::{Prop, RtsFrame, building_uv, node_uv, prop_uv};
use super::world::RtsWorld;

/// Every fixed logical-space rect the HUD's chrome occupies.
///
/// One constant source: a later hit-testing pass (`T12`) reads exactly these
/// values instead of a second derivation that could drift from the picture.
pub struct HudLayout;

impl HudLayout {
    /// Top resource bar.
    pub const TOP_BAR: [f32; 4] = [0.0, 0.0, 1920.0, 40.0];
    /// Settings gear, top-right of the top bar.
    pub const GEAR: [f32; 4] = [1872.0, 8.0, 32.0, 32.0];
    /// Bottom command panel, the whole strip.
    pub const BOTTOM_PANEL: [f32; 4] = [0.0, 840.0, 1920.0, 240.0];
    /// Minimap frame, left of the bottom panel.
    pub const MINIMAP_PANEL: [f32; 4] = [16.0, 856.0, 384.0, 208.0];
    /// The minimap's own map area, inset inside [`Self::MINIMAP_PANEL`].
    pub const MINIMAP_MAP: [f32; 4] = [32.0, 872.0, 352.0, 176.0];
    /// Selection card, centre of the bottom panel.
    pub const SELECTION_PANEL: [f32; 4] = [424.0, 856.0, 880.0, 208.0];
    /// Command card, right of the bottom panel.
    pub const COMMAND_PANEL: [f32; 4] = [1328.0, 856.0, 576.0, 208.0];
    /// The 3x3 command grid, inset to the right of [`Self::COMMAND_PANEL`].
    pub const COMMAND_GRID: [f32; 4] = [1696.0, 856.0, 208.0, 208.0];
}

/// Flat aliases of [`HudLayout`]'s rects, for call sites that only need one.
pub const TOP_BAR_RECT: [f32; 4] = HudLayout::TOP_BAR;
pub const GEAR_RECT: [f32; 4] = HudLayout::GEAR;
pub const BOTTOM_PANEL_RECT: [f32; 4] = HudLayout::BOTTOM_PANEL;
pub const MINIMAP_PANEL_RECT: [f32; 4] = HudLayout::MINIMAP_PANEL;
pub const MINIMAP_MAP_RECT: [f32; 4] = HudLayout::MINIMAP_MAP;
pub const SELECTION_PANEL_RECT: [f32; 4] = HudLayout::SELECTION_PANEL;
pub const COMMAND_PANEL_RECT: [f32; 4] = HudLayout::COMMAND_PANEL;
pub const COMMAND_GRID_RECT: [f32; 4] = HudLayout::COMMAND_GRID;

/// Icon edge in the top bar.
pub const ICON_PX: f32 = 32.0;
/// Text scale in the top bar (8 px glyphs).
pub const TOP_TEXT_SCALE: f32 = 3.0;
/// Text scale in the bottom panel (8 px glyphs).
pub const PANEL_TEXT_SCALE: f32 = 2.0;
/// Line height in the bottom panel.
pub const PANEL_LINE_PX: f32 = 22.0;

/// Single-selection portrait edge, in pixels.
pub const PORTRAIT_PX: f32 = 128.0;
/// Portrait's top-left, inside [`HudLayout::SELECTION_PANEL`].
pub const PORTRAIT_POS: [f32; 2] = [440.0, 872.0];
/// Detail text's left edge, right of the portrait.
pub const DETAIL_TEXT_X: f32 = PORTRAIT_POS[0] + PORTRAIT_PX + 16.0;
/// Detail text's top edge, level with the portrait.
pub const DETAIL_TEXT_Y: f32 = PORTRAIT_POS[1];
/// The direction row and animation frame the portrait is cropped from — a
/// fixed, camera-facing pose, matching `sim::tick::dir_from_vector`'s South.
const PORTRAIT_DIR: u32 = 6;
const PORTRAIT_FRAME: u32 = 0;

/// Multi-selection icon grid: columns, rows, edge and gap.
pub const MULTI_ICON_COLS: usize = 8;
pub const MULTI_ICON_ROWS: usize = 3;
pub const MULTI_ICON_CAP: usize = MULTI_ICON_COLS * MULTI_ICON_ROWS;
pub const MULTI_ICON_PX: f32 = 48.0;
pub const MULTI_ICON_GAP_PX: f32 = 8.0;
/// Multi-selection icon grid's top-left, inside [`HudLayout::SELECTION_PANEL`].
pub const MULTI_ICON_ORIGIN: [f32; 2] = [440.0, 872.0];
/// Where the "+N" overflow marker is drawn, right of the icon grid.
const OVERFLOW_TEXT_POS: [f32; 2] = [
    MULTI_ICON_ORIGIN[0]
        + MULTI_ICON_COLS as f32 * MULTI_ICON_PX
        + (MULTI_ICON_COLS as f32 - 1.0) * MULTI_ICON_GAP_PX
        + 16.0,
    MULTI_ICON_ORIGIN[1],
];

/// Command grid: 3x3 icons, edge and gap sized to exactly fill
/// [`HudLayout::COMMAND_GRID`] (`3 * 64 + 2 * 8 == 208`).
pub const COMMAND_GRID_COLS: usize = 3;
pub const COMMAND_ICON_PX: f32 = 64.0;
pub const COMMAND_ICON_GAP_PX: f32 = 8.0;

/// Panel tint, premultiplied. The sheet cell already carries the alpha; this
/// keeps the tint neutral so the panel colour lives in exactly one place.
pub const PANEL_TINT: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
/// Normal HUD text.
pub const TEXT_TINT: [f32; 4] = [0.90, 0.92, 0.96, 1.0];
/// Text for something the player cannot currently afford or do.
pub const TEXT_TINT_BLOCKED: [f32; 4] = [0.75, 0.28, 0.24, 1.0];
/// Text for a hotkey letter.
pub const TEXT_TINT_HOTKEY: [f32; 4] = [0.95, 0.80, 0.25, 1.0];

/// The build hotkeys, in display order, with their letters.
///
/// The letters are the HUD's copy of the binding, and `T14` asserts they match
/// the app's keyboard table — a menu that says `Q` while the key is `B` is
/// worse than no menu. This slice's command grid does not draw these letters
/// (that lands with the interactive menu, `T13`); the table stays the single
/// source both sides read.
pub const BUILD_MENU: [(u8, BuildingKind); 3] = [
    (b'Q', BuildingKind::Hq),
    (b'W', BuildingKind::Depot),
    (b'E', BuildingKind::Barracks),
];

/// Longest decimal a `u32` needs, plus room for a `/` pair.
pub const NUM_BUF: usize = 12;

/// Format `v` into `buf` and return the written slice as a `&str`.
///
/// No allocation: the HUD runs every frame and a `String` per number would
/// put a heap allocation inside the frame path.
pub fn fmt_u32(buf: &mut [u8; NUM_BUF], v: u32) -> &str {
    let mut i = NUM_BUF;
    let mut n = v;
    loop {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    std::str::from_utf8(&buf[i..]).expect("ascii digits")
}

/// Format `a/b` into `buf`.
pub fn fmt_ratio(buf: &mut [u8; NUM_BUF], a: u32, b: u32) -> &str {
    let mut i = NUM_BUF;

    let mut n = b;
    loop {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 {
            break;
        }
    }

    i -= 1;
    buf[i] = b'/';

    let mut n = a;
    loop {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 {
            break;
        }
    }

    std::str::from_utf8(&buf[i..]).expect("ascii digits")
}

/// A one-word name for an entity kind, for the selection card.
pub fn kind_label(kind: EntityKind) -> &'static str {
    match kind {
        EntityKind::Unit(UnitKind::Worker) => "WORKER",
        EntityKind::Unit(UnitKind::Soldier) => "SOLDIER",
        EntityKind::Building(BuildingKind::Hq) => "HQ",
        EntityKind::Building(BuildingKind::Depot) => "DEPOT",
        EntityKind::Building(BuildingKind::Barracks) => "BARRACKS",
        EntityKind::Node(ResourceKind::Crystal) => "CRYSTAL",
        EntityKind::Node(ResourceKind::Gas) => "GAS",
    }
}

/// One command a context card can offer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandId {
    BuildHq,
    BuildDepot,
    BuildBarracks,
    TrainWorker,
    TrainSoldier,
    SetRally,
}

/// One cell of the 3x3 command grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommandSlot {
    pub command: Option<CommandId>,
    pub enabled: bool,
}

const EMPTY_SLOT: CommandSlot = CommandSlot {
    command: None,
    enabled: false,
};

/// The command icon for one [`CommandId`].
fn command_icon(cmd: CommandId) -> Prop {
    match cmd {
        CommandId::BuildHq => Prop::IconBuildHq,
        CommandId::BuildDepot => Prop::IconBuildDepot,
        CommandId::BuildBarracks => Prop::IconBuildBarracks,
        CommandId::TrainWorker => Prop::IconTrainWorker,
        CommandId::TrainSoldier => Prop::IconTrainSoldier,
        CommandId::SetRally => Prop::IconSetRally,
    }
}

/// The 9 stable, row-major command-grid slots for the current selection.
///
/// Exact context, matching the ticket:
/// - any selected worker, no selected non-worker unit/building: build
///   commands at slots 0/1/2 (HQ, Depot, Barracks).
/// - exactly one selected, finished HQ: Worker at slot 0, Rally at slot 8.
/// - exactly one selected, finished Barracks: Soldier at slot 0, Rally at
///   slot 8.
/// - anything else (mixed kinds, empty, multiple buildings, an unfinished
///   site): every slot disabled.
///
/// Allocation-free: called from the per-frame packing path.
pub fn command_slots(world: &RtsWorld) -> [CommandSlot; 9] {
    let store = world.entities();

    let mut worker_count = 0u32;
    let mut building_count = 0u32;
    let mut disqualified = false;
    let mut finished_building: Option<BuildingKind> = None;

    for &id in world.selection().ids() {
        let Some(slot) = store.slot(id) else {
            continue; // stale — not counted either way
        };
        match store.kind(slot) {
            EntityKind::Unit(UnitKind::Worker) => worker_count += 1,
            EntityKind::Building(kind) => {
                building_count += 1;
                if store.progress_target(slot) == 0 {
                    finished_building = Some(kind);
                } else {
                    disqualified = true;
                }
            }
            _ => disqualified = true,
        }
    }

    let mut out = [EMPTY_SLOT; 9];

    if worker_count > 0 && building_count == 0 && !disqualified {
        out[0] = CommandSlot {
            command: Some(CommandId::BuildHq),
            enabled: true,
        };
        out[1] = CommandSlot {
            command: Some(CommandId::BuildDepot),
            enabled: true,
        };
        out[2] = CommandSlot {
            command: Some(CommandId::BuildBarracks),
            enabled: true,
        };
    } else if worker_count == 0 && building_count == 1 && !disqualified {
        let produce = match finished_building {
            Some(BuildingKind::Hq) => Some(CommandId::TrainWorker),
            Some(BuildingKind::Barracks) => Some(CommandId::TrainSoldier),
            _ => None,
        };
        if let Some(cmd) = produce {
            out[0] = CommandSlot {
                command: Some(cmd),
                enabled: true,
            };
            out[8] = CommandSlot {
                command: Some(CommandId::SetRally),
                enabled: true,
            };
        }
    }

    out
}

/// One stretched `Prop::PanelFill` quad.
fn push_panel(out: &mut Vec<SpriteInstance>, rect: [f32; 4], tint: [f32; 4]) {
    out.push(SpriteInstance::new(
        [rect[0], rect[1]],
        [rect[2], rect[3]],
        prop_uv(Prop::PanelFill),
        tint,
    ));
}

/// One square icon of `prop` at `pos`, edge `edge_px`, white — the sheet
/// already carries the colour.
fn push_icon(out: &mut Vec<SpriteInstance>, pos: [f32; 2], edge_px: f32, prop: Prop) {
    out.push(SpriteInstance::new(
        pos,
        [edge_px, edge_px],
        prop_uv(prop),
        SpriteInstance::WHITE,
    ));
}

/// Section: the top resource bar plus the settings gear.
fn push_top_bar(world: &RtsWorld, props: &mut Vec<SpriteInstance>, font: &mut Vec<SpriteInstance>) {
    push_panel(props, HudLayout::TOP_BAR, PANEL_TINT);

    let resources = world.resources();
    let supply = world.supply();
    let mut buf = [0u8; NUM_BUF];
    let icon_y = 4.0;
    let text_y =
        HudLayout::TOP_BAR[1] + (HudLayout::TOP_BAR[3] - GLYPH_H_PX * TOP_TEXT_SCALE) * 0.5;

    push_icon(props, [16.0, icon_y], ICON_PX, Prop::CrystalIcon);
    let s = fmt_u32(&mut buf, resources.crystal);
    push_text(font, s, [56.0, text_y], TOP_TEXT_SCALE, TEXT_TINT);

    push_icon(props, [280.0, icon_y], ICON_PX, Prop::GasIcon);
    let s = fmt_u32(&mut buf, resources.gas);
    push_text(font, s, [320.0, text_y], TOP_TEXT_SCALE, TEXT_TINT);

    push_icon(props, [544.0, icon_y], ICON_PX, Prop::SupplyIcon);
    let supply_tint = if supply.free() == 0 {
        TEXT_TINT_BLOCKED
    } else {
        TEXT_TINT
    };
    let s = fmt_ratio(&mut buf, supply.used(), supply.cap());
    push_text(font, s, [584.0, text_y], TOP_TEXT_SCALE, supply_tint);

    push_icon(
        props,
        [HudLayout::GEAR[0], HudLayout::GEAR[1]],
        HudLayout::GEAR[2],
        Prop::GearIcon,
    );
}

/// Camera-polygon edge tint — the minimap's only "live" element.
pub const CAMERA_POLY_TINT: [f32; 4] = [0.95, 0.85, 0.30, 1.0];
/// Edge stamp size, in pixels — the minimap's camera polygon is drawn as a
/// run of small squares along each edge (no rotated-quad support in this
/// renderer), not a single line primitive.
pub const CAMERA_POLY_PX: f32 = 2.0;

/// Stamp one polygon edge (`a` to `b`, minimap-local, offset onto screen by
/// the caller) as a run of [`CAMERA_POLY_PX`] squares.
fn push_camera_edge(props: &mut Vec<SpriteInstance>, a: [f32; 2], b: [f32; 2]) {
    let dx = b[0] - a[0];
    let dy = b[1] - a[1];
    let len = (dx * dx + dy * dy).sqrt();
    let steps = ((len / CAMERA_POLY_PX).ceil() as usize).max(1);
    for s in 0..=steps {
        let t = s as f32 / steps as f32;
        let pos = [
            a[0] + dx * t - CAMERA_POLY_PX * 0.5,
            a[1] + dy * t - CAMERA_POLY_PX * 0.5,
        ];
        props.push(SpriteInstance::new(
            pos,
            [CAMERA_POLY_PX, CAMERA_POLY_PX],
            prop_uv(Prop::PanelFill),
            CAMERA_POLY_TINT,
        ));
    }
}

/// Section: the minimap frame, its map area, and the camera's footprint.
///
/// Draws no entities, resources, fog or terrain detail (`T12`'s scope): only
/// the map diamond's chrome and a projected outline of what the camera can
/// currently see. Clicking/dragging the minimap is the app's pointer router,
/// not this packer — see `mmd_engine::rts::hud_hit_test`.
fn push_minimap(world: &RtsWorld, props: &mut Vec<SpriteInstance>) {
    push_panel(props, HudLayout::MINIMAP_PANEL, PANEL_TINT);
    props.push(SpriteInstance::new(
        [HudLayout::MINIMAP_MAP[0], HudLayout::MINIMAP_MAP[1]],
        [HudLayout::MINIMAP_MAP[2], HudLayout::MINIMAP_MAP[3]],
        prop_uv(Prop::MinimapFrame),
        PANEL_TINT,
    ));

    let projection = minimap_projection(world);
    let origin = [HudLayout::MINIMAP_MAP[0], HudLayout::MINIMAP_MAP[1]];
    let corners = projection.camera_polygon(&world.iso_view());
    for i in 0..corners.len() {
        let a = corners[i];
        let b = corners[(i + 1) % corners.len()];
        push_camera_edge(
            props,
            [a[0] + origin[0], a[1] + origin[1]],
            [b[0] + origin[0], b[1] + origin[1]],
        );
    }
}

/// Which UI draw group a portrait/icon for `kind` belongs in.
#[derive(Clone, Copy)]
enum PortraitTarget {
    Worker,
    Soldier,
    Building,
}

/// The draw group and UV rect a portrait/icon for the entity at `slot` reads
/// from — one of the worker/soldier/building sheets, never a duplicate image.
fn portrait_source(store: &EntityStore, slot: usize) -> (PortraitTarget, [f32; 4]) {
    match store.kind(slot) {
        EntityKind::Unit(UnitKind::Worker) => (
            PortraitTarget::Worker,
            frame_uv_rect(PORTRAIT_DIR, PORTRAIT_FRAME),
        ),
        EntityKind::Unit(UnitKind::Soldier) => (
            PortraitTarget::Soldier,
            frame_uv_rect(PORTRAIT_DIR, PORTRAIT_FRAME),
        ),
        EntityKind::Building(kind) => (
            PortraitTarget::Building,
            building_uv(kind, store.progress_target(slot) > 0),
        ),
        EntityKind::Node(kind) => (
            PortraitTarget::Building,
            node_uv(kind, store.amount(slot) == 0),
        ),
    }
}

fn push_to_target(
    target: PortraitTarget,
    worker: &mut DrawGroup,
    soldier: &mut DrawGroup,
    building: &mut DrawGroup,
    inst: SpriteInstance,
) {
    match target {
        PortraitTarget::Worker => worker.instances.push(inst),
        PortraitTarget::Soldier => soldier.instances.push(inst),
        PortraitTarget::Building => building.instances.push(inst),
    }
}

/// The selection card's detail text, right of the portrait: kind, then a
/// state line (carry/idle/progress/ready/remaining), then rally if any.
fn push_detail_text(world: &RtsWorld, slot: usize, font: &mut Vec<SpriteInstance>) {
    let store = world.entities();
    let mut buf = [0u8; NUM_BUF];
    let x = DETAIL_TEXT_X;
    let mut y = DETAIL_TEXT_Y;

    let kind = store.kind(slot);
    push_text(font, kind_label(kind), [x, y], PANEL_TEXT_SCALE, TEXT_TINT);
    y += PANEL_LINE_PX;

    match kind {
        EntityKind::Unit(UnitKind::Worker) => match store.carry(slot) {
            Some((res_kind, amount)) => {
                let mut cx = x;
                cx += push_text(font, "CARRYING ", [cx, y], PANEL_TEXT_SCALE, TEXT_TINT);
                cx += push_text(
                    font,
                    kind_label(EntityKind::Node(res_kind)),
                    [cx, y],
                    PANEL_TEXT_SCALE,
                    TEXT_TINT,
                );
                cx += push_text(font, " ", [cx, y], PANEL_TEXT_SCALE, TEXT_TINT);
                let s = fmt_u32(&mut buf, amount);
                push_text(font, s, [cx, y], PANEL_TEXT_SCALE, TEXT_TINT);
            }
            None => {
                push_text(
                    font,
                    "CARRYING NOTHING",
                    [x, y],
                    PANEL_TEXT_SCALE,
                    TEXT_TINT,
                );
            }
        },
        EntityKind::Unit(UnitKind::Soldier) => {
            push_text(font, "IDLE", [x, y], PANEL_TEXT_SCALE, TEXT_TINT);
        }
        EntityKind::Building(_) => {
            let target = store.progress_target(slot);
            // Not a manual `checked_div`: `target == 0` is the site-vs-finished
            // business branch (READY has no percentage at all), not a guard
            // against dividing by zero.
            #[allow(clippy::manual_checked_ops)]
            if target == 0 {
                push_text(font, "READY", [x, y], PANEL_TEXT_SCALE, TEXT_TINT);
            } else {
                let progress = store.progress(slot);
                let pct = progress * 100 / target;
                let mut cx = x;
                cx += push_text(font, "BUILDING ", [cx, y], PANEL_TEXT_SCALE, TEXT_TINT);
                let s = fmt_u32(&mut buf, pct);
                cx += push_text(font, s, [cx, y], PANEL_TEXT_SCALE, TEXT_TINT);
                push_text(font, "%", [cx, y], PANEL_TEXT_SCALE, TEXT_TINT);
            }

            if let Some(id) = store.id_at(slot)
                && let Some(cell) = world.rally(id)
            {
                y += PANEL_LINE_PX;
                let mut cx = x;
                cx += push_text(font, "RALLY ", [cx, y], PANEL_TEXT_SCALE, TEXT_TINT);
                let s = fmt_u32(&mut buf, cell.x);
                cx += push_text(font, s, [cx, y], PANEL_TEXT_SCALE, TEXT_TINT);
                cx += push_text(font, ",", [cx, y], PANEL_TEXT_SCALE, TEXT_TINT);
                let s = fmt_u32(&mut buf, cell.y);
                push_text(font, s, [cx, y], PANEL_TEXT_SCALE, TEXT_TINT);
            }
        }
        EntityKind::Node(_) => {
            let amount = store.amount(slot);
            let mut cx = x;
            cx += push_text(font, "REMAINING ", [cx, y], PANEL_TEXT_SCALE, TEXT_TINT);
            let s = fmt_u32(&mut buf, amount);
            push_text(font, s, [cx, y], PANEL_TEXT_SCALE, TEXT_TINT);
        }
    }
}

/// Single selection: a 128x128 portrait plus the full detail text.
fn push_single_selection(
    world: &RtsWorld,
    id: EntityId,
    worker: &mut DrawGroup,
    soldier: &mut DrawGroup,
    building: &mut DrawGroup,
    font: &mut Vec<SpriteInstance>,
) {
    let store = world.entities();
    let Some(slot) = store.slot(id) else {
        return; // a stale primary draws nothing rather than a dead column
    };

    let (target, uv) = portrait_source(store, slot);
    push_to_target(
        target,
        worker,
        soldier,
        building,
        SpriteInstance::new(
            PORTRAIT_POS,
            [PORTRAIT_PX, PORTRAIT_PX],
            uv,
            SpriteInstance::WHITE,
        ),
    );

    push_detail_text(world, slot, font);
}

/// Multi selection: the first 24 sorted ids as 8x3 icons; stale ids are
/// skipped; a selection over 24 draws a "+N" marker for the remainder.
///
/// `ids` is already ascending by slot ([`super::selection::Selection::ids`]).
fn push_multi_selection(
    world: &RtsWorld,
    ids: &[EntityId],
    worker: &mut DrawGroup,
    soldier: &mut DrawGroup,
    building: &mut DrawGroup,
    font: &mut Vec<SpriteInstance>,
) {
    let store = world.entities();
    let mut drawn = 0usize;
    for &id in ids {
        if drawn >= MULTI_ICON_CAP {
            break;
        }
        let Some(slot) = store.slot(id) else {
            continue; // skip stale
        };
        let row = drawn / MULTI_ICON_COLS;
        let col = drawn % MULTI_ICON_COLS;
        let pos = [
            MULTI_ICON_ORIGIN[0] + col as f32 * (MULTI_ICON_PX + MULTI_ICON_GAP_PX),
            MULTI_ICON_ORIGIN[1] + row as f32 * (MULTI_ICON_PX + MULTI_ICON_GAP_PX),
        ];
        let (target, uv) = portrait_source(store, slot);
        push_to_target(
            target,
            worker,
            soldier,
            building,
            SpriteInstance::new(
                pos,
                [MULTI_ICON_PX, MULTI_ICON_PX],
                uv,
                SpriteInstance::WHITE,
            ),
        );
        drawn += 1;
    }

    if ids.len() > MULTI_ICON_CAP {
        let mut buf = [0u8; NUM_BUF];
        let mut cx = OVERFLOW_TEXT_POS[0];
        cx += push_text(
            font,
            "+",
            [cx, OVERFLOW_TEXT_POS[1]],
            PANEL_TEXT_SCALE,
            TEXT_TINT,
        );
        let s = fmt_u32(&mut buf, (ids.len() - MULTI_ICON_CAP) as u32);
        push_text(
            font,
            s,
            [cx, OVERFLOW_TEXT_POS[1]],
            PANEL_TEXT_SCALE,
            TEXT_TINT,
        );
    }
}

/// Section: the selection card — background, then portrait/icons + text.
#[allow(clippy::too_many_arguments)]
fn push_selection_card(
    world: &RtsWorld,
    props: &mut Vec<SpriteInstance>,
    worker: &mut DrawGroup,
    soldier: &mut DrawGroup,
    building: &mut DrawGroup,
    font: &mut Vec<SpriteInstance>,
) {
    push_panel(props, HudLayout::SELECTION_PANEL, PANEL_TINT);

    let sel = world.selection();
    if sel.is_empty() {
        return;
    }
    if sel.len() == 1 {
        push_single_selection(world, sel.ids()[0], worker, soldier, building, font);
    } else {
        push_multi_selection(world, sel.ids(), worker, soldier, building, font);
    }
}

/// Section: the command card — background, then the 3x3 grid.
fn push_command_card(world: &RtsWorld, props: &mut Vec<SpriteInstance>) {
    push_panel(props, HudLayout::COMMAND_PANEL, PANEL_TINT);

    for (i, cmd_slot) in command_slots(world).into_iter().enumerate() {
        let Some(cmd) = cmd_slot.command else {
            continue;
        };
        let row = i / COMMAND_GRID_COLS;
        let col = i % COMMAND_GRID_COLS;
        let pos = [
            HudLayout::COMMAND_GRID[0] + col as f32 * (COMMAND_ICON_PX + COMMAND_ICON_GAP_PX),
            HudLayout::COMMAND_GRID[1] + row as f32 * (COMMAND_ICON_PX + COMMAND_ICON_GAP_PX),
        ];
        push_icon(props, pos, COMMAND_ICON_PX, command_icon(cmd));
    }
}

/// Append the whole HUD to `frame`.
///
/// Appends to every group of [`RtsFrame::ui`]; never touches `world` or
/// `overlay`. Call **after** [`super::pack_frame`], which owns the
/// world-space half of the UI layer. Allocation-free.
pub fn pack_hud(world: &RtsWorld, frame: &mut RtsFrame) {
    let [worker, soldier, building, props, font] = frame.ui.as_mut_slice() else {
        unreachable!("RtsFrame::new always reserves exactly 5 UI groups")
    };

    push_top_bar(world, &mut props.instances, &mut font.instances);
    push_panel(&mut props.instances, HudLayout::BOTTOM_PANEL, PANEL_TINT);
    push_minimap(world, &mut props.instances);
    push_selection_card(
        world,
        &mut props.instances,
        worker,
        soldier,
        building,
        &mut font.instances,
    );
    push_command_card(world, &mut props.instances);
}
