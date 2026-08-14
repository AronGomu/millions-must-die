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
    /// Framed `MENU` text control, top-right of the top bar.
    ///
    /// 4 glyphs × `GLYPH_W_PX * PANEL_TEXT_SCALE` = 64 px of text, drawn at
    /// `[1840, 20]`; `[1888, 24]` stays inside and the right edge at 1912
    /// stays inside the 1920 logical edge.
    pub const MENU: [f32; 4] = [1832.0, 8.0, 80.0, 32.0];
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

    /// The paused Escape menu (`T13`/`T1`).
    pub const PAUSE_MENU: [f32; 4] = [720.0, 408.0, 480.0, 264.0];
    /// Pause menu Settings button — unmoved so `lclick:960,540` still opens it.
    pub const PAUSE_MENU_SETTINGS_BTN: [f32; 4] = [800.0, 508.0, 320.0, 64.0];
    /// Pause menu Close Menu button, directly below Settings.
    pub const PAUSE_MENU_CLOSE_BTN: [f32; 4] = [800.0, 588.0, 320.0, 64.0];
    /// Nested settings panel — widened for value fields + scrollbar (`T1`).
    pub const SETTINGS_PANEL: [f32; 4] = [440.0, 100.0, 1040.0, 880.0];
    /// Settings panel Back control (fixed footer below the body viewport).
    pub const SETTINGS_BACK_BTN: [f32; 4] = [472.0, 900.0, 160.0, 56.0];
    /// Scrollable settings body viewport (`T6` adds scroll/clip behaviour).
    pub const SETTINGS_BODY_VIEWPORT: [f32; 4] = [456.0, 160.0, 1008.0, 720.0];
    /// Settings body scrollbar track (`T6` adds thumb behaviour).
    pub const SETTINGS_SCROLLBAR_TRACK: [f32; 4] = [1432.0, 160.0, 16.0, 720.0];
}

/// Settings rows share one left margin/width inside [`HudLayout::SETTINGS_PANEL`]
/// so every row lines up under the panel's own left/right padding. Unchanged
/// from T13 so `snap_track` still reads 78 at x=1170 on the keyboard pan track.
const ROW_X: f32 = 568.0;
const ROW_W: f32 = 784.0;

/// The three window-mode buttons, row y=180 — equal width, one gap each side
/// matching [`ROW_X`]'s margin.
pub const WINDOW_MODE_BUTTONS: [[f32; 4]; 3] = [
    [ROW_X, 180.0, 240.0, 56.0],
    [ROW_X + 272.0, 180.0, 240.0, 56.0],
    [ROW_X + 544.0, 180.0, 240.0, 56.0],
];
/// Labels for [`WINDOW_MODE_BUTTONS`], in the same order the app's
/// `WindowMode` variants are declared.
pub const WINDOW_MODE_LABELS: [&str; 3] = ["BORDERLESS", "EXCLUSIVE", "WINDOWED"];

pub const KEYBOARD_PAN_TRACK: [f32; 4] = [ROW_X, 276.0, ROW_W, 24.0];
pub const EDGE_PAN_TRACK: [f32; 4] = [ROW_X, 372.0, ROW_W, 24.0];
pub const CONFINE_CHECKBOX: [f32; 4] = [ROW_X, 468.0, 32.0, 32.0];
pub const FOCUS_CHECKBOX: [f32; 4] = [ROW_X, 516.0, 32.0, 32.0];
pub const GRID_CHECKBOX: [f32; 4] = [ROW_X, 564.0, 32.0, 32.0];
/// Full-width label-row hit target for each checkbox-style control (the T1
/// contract: the label row is the control, not only the 32px square).
pub const CONFINE_CONTROL_RECT: [f32; 4] = [ROW_X, 468.0, ROW_W, 32.0];
pub const FOCUS_CONTROL_RECT: [f32; 4] = [ROW_X, 516.0, ROW_W, 32.0];
pub const GRID_CONTROL_RECT: [f32; 4] = [ROW_X, 564.0, ROW_W, 32.0];
/// Audio block on a 96 px pitch so value fields / mute labels fit (`T1`).
pub const MASTER_TRACK: [f32; 4] = [ROW_X, 680.0, ROW_W, 24.0];
pub const MUSIC_TRACK: [f32; 4] = [ROW_X, 776.0, ROW_W, 24.0];
pub const VOICE_TRACK: [f32; 4] = [ROW_X, 872.0, ROW_W, 24.0];
pub const SFX_TRACK: [f32; 4] = [ROW_X, 968.0, ROW_W, 24.0];
/// Numeric value field geometry — one field per slider row, right of the track.
pub const VALUE_FIELD_X: f32 = 1368.0;
pub const VALUE_FIELD_W: f32 = 56.0;
pub const VALUE_FIELD_H: f32 = 24.0;
/// Mute label row geometry — one label per audio channel, above its track.
pub const MUTE_LABEL_W: f32 = 240.0;
pub const MUTE_LABEL_H: f32 = 32.0;

/// Value-field rect for a slider track row: `[VALUE_FIELD_X, track_y, 56, 24]`.
pub fn value_field_rect(track: [f32; 4]) -> [f32; 4] {
    [VALUE_FIELD_X, track[1], VALUE_FIELD_W, VALUE_FIELD_H]
}

/// Mute-label rect for an audio track: `[ROW_X, track_y - 40, 240, 32]`.
pub fn mute_label_rect(track: [f32; 4]) -> [f32; 4] {
    [ROW_X, track[1] - 40.0, MUTE_LABEL_W, MUTE_LABEL_H]
}

/// Keyboard/edge pan bounds — must match `crate::rts_settings`'s
/// `PAN_MIN`/`PAN_MAX`/`PAN_STEP` (schema-1 contract, duplicated here so this
/// engine crate never depends on the app crate's settings type; kept in sync
/// by `settings_pan_and_volume_bounds_match_app_contract`, `T13`).
pub const PAN_MIN: u32 = 6;
pub const PAN_MAX: u32 = 96;
pub const PAN_STEP: u32 = 6;
/// Volume bounds — same duplication contract as [`PAN_MIN`].
pub const VOLUME_MIN: u32 = 0;
pub const VOLUME_MAX: u32 = 100;
pub const VOLUME_STEP: u32 = 5;

/// Flat aliases of [`HudLayout`]'s rects, for call sites that only need one.
pub const TOP_BAR_RECT: [f32; 4] = HudLayout::TOP_BAR;
pub const MENU_RECT: [f32; 4] = HudLayout::MENU;
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

/// Frame thickness around discrete interactive controls (`T1`).
pub const CONTROL_FRAME_PX: f32 = 2.0;
/// Idle frame tint — premultiplied, pairwise distinct from the other four.
pub const CONTROL_TINT_IDLE: [f32; 4] = [0.22, 0.24, 0.30, 0.90];
/// Hover frame tint.
pub const CONTROL_TINT_HOVER: [f32; 4] = [0.34, 0.38, 0.46, 0.95];
/// Pressed frame tint.
pub const CONTROL_TINT_PRESSED: [f32; 4] = [0.10, 0.12, 0.16, 1.00];
/// Selected frame tint.
pub const CONTROL_TINT_SELECTED: [f32; 4] = [0.20, 0.52, 0.24, 1.00];
/// Disabled frame tint.
pub const CONTROL_TINT_DISABLED: [f32; 4] = [0.14, 0.14, 0.16, 0.55];

/// Discrete visual state of one interactive control (`T1`).
///
/// Precedence when resolving: Disabled → Pressed → Hover → Selected → Idle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlVisualState {
    Idle,
    Hover,
    Pressed,
    Selected,
    Disabled,
}

/// Stable identity of one interactive control for the pointer FSM (`T1`).
///
/// Identity never carries a live value (slider position, typed digits) — those
/// are derived from the pointer / field buffer at activation time.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ControlId {
    Menu,
    /// Selection-card icon slot (`0` for the single portrait; multi-grid index).
    SelectionIcon(u8),
    CommandSlot(u8),
    PauseSettings,
    PauseClose,
    WindowMode(u8),
    Confine,
    Focus,
    Grid,
    KeyboardPanSlider,
    EdgePanSlider,
    MasterSlider,
    MusicSlider,
    VoiceSlider,
    SfxSlider,
    KeyboardPanField,
    EdgePanField,
    MasterField,
    MusicField,
    VoiceField,
    SfxField,
    MasterMute,
    MusicMute,
    VoiceMute,
    SfxMute,
    SettingsBack,
    ScrollbarTrack,
    ScrollbarThumb,
}

/// Pointer hover/press snapshot fed into interactive HUD/modal packers (`T1`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InteractionSnapshot {
    pub hovered: Option<ControlId>,
    pub pressed: Option<ControlId>,
}

/// Resolve one control's visual state from interaction + selected/disabled flags.
///
/// Precedence: Disabled → Pressed → Hover → Selected → Idle.
pub fn control_visual_state(
    id: ControlId,
    interaction: &InteractionSnapshot,
    selected: bool,
    disabled: bool,
) -> ControlVisualState {
    if disabled {
        return ControlVisualState::Disabled;
    }
    if interaction.pressed == Some(id) {
        return ControlVisualState::Pressed;
    }
    if interaction.hovered == Some(id) {
        return ControlVisualState::Hover;
    }
    if selected {
        return ControlVisualState::Selected;
    }
    ControlVisualState::Idle
}

/// Premultiplied frame tint for a resolved [`ControlVisualState`].
pub fn control_tint(state: ControlVisualState) -> [f32; 4] {
    match state {
        ControlVisualState::Idle => CONTROL_TINT_IDLE,
        ControlVisualState::Hover => CONTROL_TINT_HOVER,
        ControlVisualState::Pressed => CONTROL_TINT_PRESSED,
        ControlVisualState::Selected => CONTROL_TINT_SELECTED,
        ControlVisualState::Disabled => CONTROL_TINT_DISABLED,
    }
}

/// Top-left of the `MENU` label inside [`HudLayout::MENU`].
pub const MENU_TEXT_POS: [f32; 2] = [1840.0, 20.0];

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

/// One discrete-control frame fill — single shared path, no per-control tint copies.
fn push_control_frame(out: &mut Vec<SpriteInstance>, rect: [f32; 4], state: ControlVisualState) {
    let _ = CONTROL_FRAME_PX; // thickness reserved for border-style frames later
    push_panel(out, rect, control_tint(state));
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

/// Section: the top resource bar plus the framed `MENU` text control.
fn push_top_bar(
    world: &RtsWorld,
    props: &mut Vec<SpriteInstance>,
    font: &mut Vec<SpriteInstance>,
    interaction: &InteractionSnapshot,
) {
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

    let menu_state = control_visual_state(ControlId::Menu, interaction, false, false);
    push_control_frame(props, HudLayout::MENU, menu_state);
    push_text(font, "MENU", MENU_TEXT_POS, PANEL_TEXT_SCALE, TEXT_TINT);
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

/// Single selection: a framed 128x128 portrait plus the full detail text.
fn push_single_selection(
    world: &RtsWorld,
    id: EntityId,
    worker: &mut DrawGroup,
    soldier: &mut DrawGroup,
    building: &mut DrawGroup,
    props: &mut Vec<SpriteInstance>,
    font: &mut Vec<SpriteInstance>,
    interaction: &InteractionSnapshot,
) {
    let store = world.entities();
    let Some(slot) = store.slot(id) else {
        return; // a stale primary draws nothing rather than a dead column
    };

    let frame_rect = [PORTRAIT_POS[0], PORTRAIT_POS[1], PORTRAIT_PX, PORTRAIT_PX];
    let state = control_visual_state(ControlId::SelectionIcon(0), interaction, true, false);
    push_control_frame(props, frame_rect, state);

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

/// Multi selection: the first 24 sorted ids as 8x3 framed icons; stale ids are
/// skipped; a selection over 24 draws a "+N" marker for the remainder.
///
/// `ids` is already ascending by slot ([`super::selection::Selection::ids`]).
fn push_multi_selection(
    world: &RtsWorld,
    ids: &[EntityId],
    worker: &mut DrawGroup,
    soldier: &mut DrawGroup,
    building: &mut DrawGroup,
    props: &mut Vec<SpriteInstance>,
    font: &mut Vec<SpriteInstance>,
    interaction: &InteractionSnapshot,
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
        let frame_rect = [pos[0], pos[1], MULTI_ICON_PX, MULTI_ICON_PX];
        let state = control_visual_state(
            ControlId::SelectionIcon(drawn as u8),
            interaction,
            true,
            false,
        );
        push_control_frame(props, frame_rect, state);
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
    interaction: &InteractionSnapshot,
) {
    push_panel(props, HudLayout::SELECTION_PANEL, PANEL_TINT);

    let sel = world.selection();
    if sel.is_empty() {
        return;
    }
    if sel.len() == 1 {
        push_single_selection(
            world,
            sel.ids()[0],
            worker,
            soldier,
            building,
            props,
            font,
            interaction,
        );
    } else {
        push_multi_selection(
            world,
            sel.ids(),
            worker,
            soldier,
            building,
            props,
            font,
            interaction,
        );
    }
}

/// Command-grid cell rect for slot `i` (`0..9`, row-major).
pub fn command_slot_rect(i: usize) -> [f32; 4] {
    let row = i / COMMAND_GRID_COLS;
    let col = i % COMMAND_GRID_COLS;
    [
        HudLayout::COMMAND_GRID[0] + col as f32 * (COMMAND_ICON_PX + COMMAND_ICON_GAP_PX),
        HudLayout::COMMAND_GRID[1] + row as f32 * (COMMAND_ICON_PX + COMMAND_ICON_GAP_PX),
        COMMAND_ICON_PX,
        COMMAND_ICON_PX,
    ]
}

/// Section: the command card — background, then framed 3x3 grid (all 9 cells).
fn push_command_card(
    world: &RtsWorld,
    props: &mut Vec<SpriteInstance>,
    interaction: &InteractionSnapshot,
) {
    push_panel(props, HudLayout::COMMAND_PANEL, PANEL_TINT);

    for (i, cmd_slot) in command_slots(world).into_iter().enumerate() {
        let rect = command_slot_rect(i);
        let disabled = cmd_slot.command.is_none() || !cmd_slot.enabled;
        let state = control_visual_state(
            ControlId::CommandSlot(i as u8),
            interaction,
            false,
            disabled,
        );
        push_control_frame(props, rect, state);
        if let Some(cmd) = cmd_slot.command {
            push_icon(
                props,
                [rect[0], rect[1]],
                COMMAND_ICON_PX,
                command_icon(cmd),
            );
        }
    }
}

/// Which nested modal page [`pack_modal`]/[`modal_hit_test`] render/hit-test
/// against — `T13`'s app-crate `UiPage` maps its `PauseMenu`/`Settings`
/// variants onto this; `Gameplay` has no modal at all, so it is not a variant
/// here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModalPage {
    PauseMenu,
    Settings,
}

/// What a pointer point inside an open modal landed on. [`Self::Consumed`]
/// covers every other point in modal space (panel gaps, headers) — while a
/// modal is open it owns *every* pointer point, never just its own controls.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ModalHit {
    /// The pause menu's Settings button.
    OpenSettings,
    /// The pause menu's Close Menu button.
    CloseMenu,
    /// The settings panel's Back control.
    Back,
    /// One of [`WINDOW_MODE_BUTTONS`], `0..3`.
    WindowMode(u8),
    /// Keyboard pan track, already snapped to a legal step.
    KeyboardPan(u32),
    /// Edge pan track, already snapped to a legal step.
    EdgePan(u32),
    /// Confine-pointer checkbox (full label row).
    Confine,
    /// Pause-on-focus-loss checkbox (full label row).
    Focus,
    Master(u32),
    Music(u32),
    Voice(u32),
    Sfx(u32),
    /// Anywhere else inside the modal — consumed, no action.
    Consumed,
}

/// Stable [`ControlId`] for a modal hit, when the hit names a discrete control.
pub fn control_id_from_modal_hit(hit: ModalHit) -> Option<ControlId> {
    match hit {
        ModalHit::OpenSettings => Some(ControlId::PauseSettings),
        ModalHit::CloseMenu => Some(ControlId::PauseClose),
        ModalHit::Back => Some(ControlId::SettingsBack),
        ModalHit::WindowMode(i) => Some(ControlId::WindowMode(i)),
        ModalHit::KeyboardPan(_) => Some(ControlId::KeyboardPanSlider),
        ModalHit::EdgePan(_) => Some(ControlId::EdgePanSlider),
        ModalHit::Confine => Some(ControlId::Confine),
        ModalHit::Focus => Some(ControlId::Focus),
        ModalHit::Master(_) => Some(ControlId::MasterSlider),
        ModalHit::Music(_) => Some(ControlId::MusicSlider),
        ModalHit::Voice(_) => Some(ControlId::VoiceSlider),
        ModalHit::Sfx(_) => Some(ControlId::SfxSlider),
        ModalHit::Consumed => None,
    }
}

/// The raw settings values [`pack_modal`] renders, in the numeric domain the
/// caller (`crate::rts_ui`, app crate) owns — kept as plain fields rather
/// than the app's `RtsSettings` type so this engine crate never depends on
/// it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ModalSnapshot {
    pub window_mode_index: u8,
    pub keyboard_pan: u32,
    pub edge_pan: u32,
    pub confine_pointer: bool,
    pub pause_on_focus_loss: bool,
    pub master: u32,
    pub music: u32,
    pub voice: u32,
    pub sfx: u32,
}

fn point_in_modal_rect(point: [f32; 2], rect: [f32; 4]) -> bool {
    point[0] >= rect[0]
        && point[0] < rect[0] + rect[2]
        && point[1] >= rect[1]
        && point[1] < rect[1] + rect[3]
}

/// Nearest legal step for a click at `local_x` inside a `[x, y, w, h]` track,
/// clamped to `min..=max`.
pub fn snap_track(local_x: f32, track: [f32; 4], min: u32, max: u32, step: u32) -> u32 {
    let frac = ((local_x - track[0]) / track[2]).clamp(0.0, 1.0);
    let span = (max - min) as f32;
    let raw = min as f32 + frac * span;
    let steps = ((raw - min as f32) / step as f32).round();
    (min + steps as u32 * step).min(max)
}

/// Classify a logical (1920x1080) point against an open modal's own chrome.
/// Never returns `None` — an open modal owns every pointer point.
pub fn modal_hit_test(page: ModalPage, point: [f32; 2]) -> ModalHit {
    match page {
        ModalPage::PauseMenu => {
            if point_in_modal_rect(point, HudLayout::PAUSE_MENU_SETTINGS_BTN) {
                ModalHit::OpenSettings
            } else if point_in_modal_rect(point, HudLayout::PAUSE_MENU_CLOSE_BTN) {
                ModalHit::CloseMenu
            } else {
                ModalHit::Consumed
            }
        }
        ModalPage::Settings => {
            if point_in_modal_rect(point, HudLayout::SETTINGS_BACK_BTN) {
                return ModalHit::Back;
            }
            for (i, rect) in WINDOW_MODE_BUTTONS.iter().enumerate() {
                if point_in_modal_rect(point, *rect) {
                    return ModalHit::WindowMode(i as u8);
                }
            }
            if point_in_modal_rect(point, KEYBOARD_PAN_TRACK) {
                return ModalHit::KeyboardPan(snap_track(
                    point[0],
                    KEYBOARD_PAN_TRACK,
                    PAN_MIN,
                    PAN_MAX,
                    PAN_STEP,
                ));
            }
            if point_in_modal_rect(point, EDGE_PAN_TRACK) {
                return ModalHit::EdgePan(snap_track(
                    point[0],
                    EDGE_PAN_TRACK,
                    PAN_MIN,
                    PAN_MAX,
                    PAN_STEP,
                ));
            }
            // Full label row is the control; the 32px square remains a subset.
            if point_in_modal_rect(point, CONFINE_CONTROL_RECT)
                || point_in_modal_rect(point, CONFINE_CHECKBOX)
            {
                return ModalHit::Confine;
            }
            if point_in_modal_rect(point, FOCUS_CONTROL_RECT)
                || point_in_modal_rect(point, FOCUS_CHECKBOX)
            {
                return ModalHit::Focus;
            }
            if point_in_modal_rect(point, MASTER_TRACK) {
                return ModalHit::Master(snap_track(
                    point[0],
                    MASTER_TRACK,
                    VOLUME_MIN,
                    VOLUME_MAX,
                    VOLUME_STEP,
                ));
            }
            if point_in_modal_rect(point, MUSIC_TRACK) {
                return ModalHit::Music(snap_track(
                    point[0],
                    MUSIC_TRACK,
                    VOLUME_MIN,
                    VOLUME_MAX,
                    VOLUME_STEP,
                ));
            }
            if point_in_modal_rect(point, VOICE_TRACK) {
                return ModalHit::Voice(snap_track(
                    point[0],
                    VOICE_TRACK,
                    VOLUME_MIN,
                    VOLUME_MAX,
                    VOLUME_STEP,
                ));
            }
            if point_in_modal_rect(point, SFX_TRACK) {
                return ModalHit::Sfx(snap_track(
                    point[0],
                    SFX_TRACK,
                    VOLUME_MIN,
                    VOLUME_MAX,
                    VOLUME_STEP,
                ));
            }
            ModalHit::Consumed
        }
    }
}

fn push_modal_button(
    props: &mut Vec<SpriteInstance>,
    font: &mut Vec<SpriteInstance>,
    rect: [f32; 4],
    label: &str,
    id: ControlId,
    interaction: &InteractionSnapshot,
    selected: bool,
) {
    let state = control_visual_state(id, interaction, selected, false);
    push_control_frame(props, rect, state);
    let text_y = rect[1] + (rect[3] - GLYPH_H_PX * PANEL_TEXT_SCALE) * 0.5;
    push_text(
        font,
        label,
        [rect[0] + 12.0, text_y],
        PANEL_TEXT_SCALE,
        TEXT_TINT,
    );
}

fn push_modal_track(
    props: &mut Vec<SpriteInstance>,
    font: &mut Vec<SpriteInstance>,
    rect: [f32; 4],
    label: &str,
    value: u32,
    min: u32,
    max: u32,
    slider_id: ControlId,
    interaction: &InteractionSnapshot,
) {
    push_text(
        font,
        label,
        [rect[0], rect[1] - PANEL_LINE_PX],
        PANEL_TEXT_SCALE,
        TEXT_TINT,
    );
    let state = control_visual_state(slider_id, interaction, false, false);
    push_control_frame(props, rect, state);
    let frac = ((value.saturating_sub(min)) as f32 / (max - min) as f32).clamp(0.0, 1.0);
    let fill = [rect[0], rect[1], rect[2] * frac, rect[3]];
    push_panel(props, fill, [0.95, 0.85, 0.30, 1.0]);
    let mut buf = [0u8; NUM_BUF];
    let s = fmt_u32(&mut buf, value);
    // Value still drawn to the right of the track (field rect is reserved for T4).
    push_text(
        font,
        s,
        [rect[0] + rect[2] + 16.0, rect[1]],
        PANEL_TEXT_SCALE,
        TEXT_TINT,
    );
}

fn push_modal_checkbox(
    props: &mut Vec<SpriteInstance>,
    font: &mut Vec<SpriteInstance>,
    square: [f32; 4],
    row: [f32; 4],
    label: &str,
    id: ControlId,
    interaction: &InteractionSnapshot,
    checked: bool,
) {
    // Frame the whole label row; the 32px square stays the filled indicator.
    let state = control_visual_state(id, interaction, checked, false);
    push_control_frame(props, row, state);
    let square_tint = if checked {
        CONTROL_TINT_SELECTED
    } else {
        CONTROL_TINT_IDLE
    };
    push_panel(props, square, square_tint);
    let text_y = square[1] + (square[3] - GLYPH_H_PX * PANEL_TEXT_SCALE) * 0.5;
    push_text(
        font,
        label,
        [square[0] + square[2] + 16.0, text_y],
        PANEL_TEXT_SCALE,
        TEXT_TINT,
    );
}

/// Append one open modal (pause menu or settings panel) to `frame`, last in
/// the textured UI groups — drawn over the world and the normal HUD, which
/// stay packed underneath. `warning`, when set, is the
/// `SETTINGS NOT SAVED: <reason>` line a failed transactional commit leaves
/// up.
///
/// Neutral wrapper: no hover/press tinting.
pub fn pack_modal(
    page: ModalPage,
    snapshot: ModalSnapshot,
    warning: Option<&str>,
    frame: &mut RtsFrame,
) {
    pack_modal_interactive(
        page,
        snapshot,
        warning,
        &InteractionSnapshot::default(),
        frame,
    );
}

/// Interactive modal pack — frames + hover/press/selected tints from `interaction`.
pub fn pack_modal_interactive(
    page: ModalPage,
    snapshot: ModalSnapshot,
    warning: Option<&str>,
    interaction: &InteractionSnapshot,
    frame: &mut RtsFrame,
) {
    let [_, _, _, props, font] = frame.ui.as_mut_slice() else {
        unreachable!("RtsFrame::new always reserves exactly 5 UI groups")
    };
    match page {
        ModalPage::PauseMenu => {
            push_panel(&mut props.instances, HudLayout::PAUSE_MENU, PANEL_TINT);
            push_modal_button(
                &mut props.instances,
                &mut font.instances,
                HudLayout::PAUSE_MENU_SETTINGS_BTN,
                "SETTINGS",
                ControlId::PauseSettings,
                interaction,
                false,
            );
            push_modal_button(
                &mut props.instances,
                &mut font.instances,
                HudLayout::PAUSE_MENU_CLOSE_BTN,
                "CLOSE MENU",
                ControlId::PauseClose,
                interaction,
                false,
            );
        }
        ModalPage::Settings => {
            push_panel(&mut props.instances, HudLayout::SETTINGS_PANEL, PANEL_TINT);
            for (i, rect) in WINDOW_MODE_BUTTONS.iter().enumerate() {
                push_modal_button(
                    &mut props.instances,
                    &mut font.instances,
                    *rect,
                    WINDOW_MODE_LABELS[i],
                    ControlId::WindowMode(i as u8),
                    interaction,
                    i as u8 == snapshot.window_mode_index,
                );
            }
            push_modal_track(
                &mut props.instances,
                &mut font.instances,
                KEYBOARD_PAN_TRACK,
                "KEYBOARD PAN",
                snapshot.keyboard_pan,
                PAN_MIN,
                PAN_MAX,
                ControlId::KeyboardPanSlider,
                interaction,
            );
            push_modal_track(
                &mut props.instances,
                &mut font.instances,
                EDGE_PAN_TRACK,
                "EDGE PAN",
                snapshot.edge_pan,
                PAN_MIN,
                PAN_MAX,
                ControlId::EdgePanSlider,
                interaction,
            );
            push_modal_checkbox(
                &mut props.instances,
                &mut font.instances,
                CONFINE_CHECKBOX,
                CONFINE_CONTROL_RECT,
                "CONFINE POINTER",
                ControlId::Confine,
                interaction,
                snapshot.confine_pointer,
            );
            push_modal_checkbox(
                &mut props.instances,
                &mut font.instances,
                FOCUS_CHECKBOX,
                FOCUS_CONTROL_RECT,
                "PAUSE ON FOCUS LOSS",
                ControlId::Focus,
                interaction,
                snapshot.pause_on_focus_loss,
            );
            push_modal_track(
                &mut props.instances,
                &mut font.instances,
                MASTER_TRACK,
                "MASTER",
                snapshot.master,
                VOLUME_MIN,
                VOLUME_MAX,
                ControlId::MasterSlider,
                interaction,
            );
            push_modal_track(
                &mut props.instances,
                &mut font.instances,
                MUSIC_TRACK,
                "MUSIC",
                snapshot.music,
                VOLUME_MIN,
                VOLUME_MAX,
                ControlId::MusicSlider,
                interaction,
            );
            push_modal_track(
                &mut props.instances,
                &mut font.instances,
                VOICE_TRACK,
                "VOICE",
                snapshot.voice,
                VOLUME_MIN,
                VOLUME_MAX,
                ControlId::VoiceSlider,
                interaction,
            );
            push_modal_track(
                &mut props.instances,
                &mut font.instances,
                SFX_TRACK,
                "SFX",
                snapshot.sfx,
                VOLUME_MIN,
                VOLUME_MAX,
                ControlId::SfxSlider,
                interaction,
            );
            push_modal_button(
                &mut props.instances,
                &mut font.instances,
                HudLayout::SETTINGS_BACK_BTN,
                "BACK",
                ControlId::SettingsBack,
                interaction,
                false,
            );
            if let Some(msg) = warning {
                push_text(
                    &mut font.instances,
                    msg,
                    [
                        HudLayout::SETTINGS_PANEL[0] + 16.0,
                        HudLayout::SETTINGS_PANEL[1] + HudLayout::SETTINGS_PANEL[3] - 40.0,
                    ],
                    PANEL_TEXT_SCALE,
                    TEXT_TINT_BLOCKED,
                );
            }
        }
    }
}

/// Append the whole HUD to `frame` with a neutral (no hover/press) snapshot.
///
/// Call **after** [`super::pack_frame`]. Allocation-free when buffers are warm.
pub fn pack_hud(world: &RtsWorld, frame: &mut RtsFrame) {
    pack_hud_interactive(world, &InteractionSnapshot::default(), frame);
}

/// Interactive HUD pack — frames + hover/press/selected/disabled tints.
pub fn pack_hud_interactive(
    world: &RtsWorld,
    interaction: &InteractionSnapshot,
    frame: &mut RtsFrame,
) {
    let [worker, soldier, building, props, font] = frame.ui.as_mut_slice() else {
        unreachable!("RtsFrame::new always reserves exactly 5 UI groups")
    };

    push_top_bar(
        world,
        &mut props.instances,
        &mut font.instances,
        interaction,
    );
    push_panel(&mut props.instances, HudLayout::BOTTOM_PANEL, PANEL_TINT);
    push_minimap(world, &mut props.instances);
    push_selection_card(
        world,
        &mut props.instances,
        worker,
        soldier,
        building,
        &mut font.instances,
        interaction,
    );
    push_command_card(world, &mut props.instances, interaction);
}
