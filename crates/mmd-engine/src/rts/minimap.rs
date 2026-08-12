//! Minimap projection and HUD hit testing — `T12`.
//!
//! Pure logic: no SDL, no rendering. [`super::hud::pack_hud`] draws the
//! minimap chrome through [`MinimapProjection`]; the app's pointer router
//! reads [`hud_hit_test`] to decide whether a click belongs to the HUD or
//! the world, before either one ever sees it.

use crate::render::IsoView;

use super::entity::EntityId;
use super::hud::{
    COMMAND_GRID_COLS, COMMAND_ICON_GAP_PX, COMMAND_ICON_PX, HudLayout, MULTI_ICON_CAP,
    MULTI_ICON_COLS, MULTI_ICON_GAP_PX, MULTI_ICON_ORIGIN, MULTI_ICON_PX, PORTRAIT_POS,
    PORTRAIT_PX,
};
use super::world::RtsWorld;

/// The isometric raw-space projection a map cell lands at before it is fit
/// into the minimap's pixel box: `rx = cx - cy`, `ry = (cx + cy) / 2`.
///
/// Every field is derived once, at construction, from the map's own cell
/// bounds and the minimap's pixel box — not recomputed per call, so
/// [`Self::map_to_minimap`]/[`Self::minimap_to_map`] agree on exactly one
/// `scale` for the lifetime of one projection.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MinimapProjection {
    /// The map's own bounds in cell space: `[x, y, width, height]`.
    pub map_rect: [f32; 4],
    /// The minimap's pixel box width (fits [`super::hud::HudLayout::MINIMAP_MAP`]).
    pub width: u32,
    /// The minimap's pixel box height.
    pub height: u32,
    /// Raw-space units to minimap pixels, fit to preserve the raw box's
    /// aspect ratio (always `2:1` for a square map, matching the minimap's
    /// own `2:1` pixel box).
    pub scale: f32,
}

impl MinimapProjection {
    /// Derive a projection for a `map_rect`-bounded map fit into a
    /// `width` x `height` pixel box.
    pub fn new(map_rect: [f32; 4], width: u32, height: u32) -> Self {
        let (raw_w, raw_h) = Self::raw_size(map_rect);
        let sx = if raw_w > 0.0 {
            width as f32 / raw_w
        } else {
            0.0
        };
        let sy = if raw_h > 0.0 {
            height as f32 / raw_h
        } else {
            0.0
        };
        let scale = sx.min(sy);
        Self {
            map_rect,
            width,
            height,
            scale,
        }
    }

    /// The raw `(rx, ry)` bounding box's own size, before scaling: always
    /// `(map_w + map_h, (map_w + map_h) / 2)`.
    fn raw_size(map_rect: [f32; 4]) -> (f32, f32) {
        let map_w = map_rect[2];
        let map_h = map_rect[3];
        (map_w + map_h, (map_w + map_h) * 0.5)
    }

    /// Centring offset that letterboxes the raw box inside the pixel box
    /// when the two aspect ratios do not exactly agree.
    fn offset(&self) -> [f32; 2] {
        let (raw_w, raw_h) = Self::raw_size(self.map_rect);
        [
            (self.width as f32 - raw_w * self.scale) * 0.5,
            (self.height as f32 - raw_h * self.scale) * 0.5,
        ]
    }

    /// A map cell, projected into the minimap's local pixel space
    /// (`[0, width] x [0, height]`, not yet offset onto the screen).
    pub fn map_to_minimap(&self, cell: [f32; 2]) -> [f32; 2] {
        let map_x = self.map_rect[0];
        let map_y = self.map_rect[1];
        let map_h = self.map_rect[3];
        let cx = cell[0] - map_x;
        let cy = cell[1] - map_y;
        let rx = cx - cy;
        let ry = (cx + cy) * 0.5;
        let off = self.offset();
        [(rx + map_h) * self.scale + off[0], ry * self.scale + off[1]]
    }

    /// The inverse of [`Self::map_to_minimap`]. `None` when `point` falls
    /// outside the map's own diamond — the raw box the minimap fits also
    /// covers area no real map cell projects to.
    pub fn minimap_to_map(&self, point: [f32; 2]) -> Option<[f32; 2]> {
        if self.scale <= 0.0 {
            return None;
        }
        let map_x = self.map_rect[0];
        let map_y = self.map_rect[1];
        let map_w = self.map_rect[2];
        let map_h = self.map_rect[3];
        let off = self.offset();
        let rx = (point[0] - off[0]) / self.scale - map_h;
        let ry = (point[1] - off[1]) / self.scale;
        let cx = ry + rx * 0.5;
        let cy = ry - rx * 0.5;
        if cx < 0.0 || cx > map_w || cy < 0.0 || cy > map_h {
            return None;
        }
        Some([cx + map_x, cy + map_y])
    }

    /// The camera's screen-space footprint, unprojected back to map cells
    /// and reprojected onto the minimap: the four logical-canvas corners
    /// `(0,0)`, `(1920,0)`, `(1920,1080)`, `(0,1080)`, in that order — a
    /// closed quad whose edges the HUD packer stamps.
    pub fn camera_polygon(&self, view: &IsoView) -> [[f32; 2]; 4] {
        const CORNERS: [[f32; 2]; 4] = [[0.0, 0.0], [1920.0, 0.0], [1920.0, 1080.0], [0.0, 1080.0]];
        let mut out = [[0.0; 2]; 4];
        for (i, c) in CORNERS.iter().enumerate() {
            let world_pt = view.unproject(c[0], c[1]);
            out[i] = self.map_to_minimap(world_pt);
        }
        out
    }
}

/// The projection for `world`'s current scenario, fit into
/// [`HudLayout::MINIMAP_MAP`]'s own pixel size.
pub fn minimap_projection(world: &RtsWorld) -> MinimapProjection {
    let map_rect = [
        0.0,
        0.0,
        world.scenario().width() as f32,
        world.scenario().height() as f32,
    ];
    MinimapProjection::new(
        map_rect,
        HudLayout::MINIMAP_MAP[2] as u32,
        HudLayout::MINIMAP_MAP[3] as u32,
    )
}

/// What a point in HUD space landed on.
///
/// [`Self::Minimap`] carries the raw screen point, not a map cell: the
/// diamond/rectangle rejection lives in [`MinimapProjection::minimap_to_map`],
/// which the caller runs after subtracting the minimap's own screen origin —
/// one inverse, not two agreeing derivations.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HudHit {
    /// The settings gear, top-right of the top bar.
    Gear,
    /// A point inside the minimap's own map rect, in screen space.
    Minimap([f32; 2]),
    /// One selection-card icon (or the single-selection portrait).
    SelectionIcon(EntityId),
    /// One command-grid cell, `0..9`, row-major.
    CommandSlot(u8),
    /// Any other point inside the HUD's chrome — panel gaps, headers, the
    /// bottom strip outside a card. Never falls through to the world.
    Background,
}

fn point_in_rect(point: [f32; 2], rect: [f32; 4]) -> bool {
    point[0] >= rect[0]
        && point[0] < rect[0] + rect[2]
        && point[1] >= rect[1]
        && point[1] < rect[1] + rect[3]
}

/// The command-grid cell (`0..9`, row-major) a point inside
/// [`HudLayout::COMMAND_GRID`] lands on.
fn command_slot_index(point: [f32; 2]) -> u8 {
    let cell = COMMAND_ICON_PX + COMMAND_ICON_GAP_PX;
    let local_x = point[0] - HudLayout::COMMAND_GRID[0];
    let local_y = point[1] - HudLayout::COMMAND_GRID[1];
    let col = ((local_x / cell).max(0.0) as usize).min(COMMAND_GRID_COLS - 1);
    let row = ((local_y / cell).max(0.0) as usize).min(COMMAND_GRID_COLS - 1);
    (row * COMMAND_GRID_COLS + col) as u8
}

/// The selection icon (or single-selection portrait) a point inside
/// [`HudLayout::SELECTION_PANEL`] lands on — the same geometry
/// [`super::hud::pack_hud`]'s selection-card section draws, so a hit can
/// only ever agree with what is actually on screen.
fn selection_icon_hit(world: &RtsWorld, point: [f32; 2]) -> Option<EntityId> {
    let sel = world.selection();
    if sel.is_empty() {
        return None;
    }
    if sel.len() == 1 {
        let id = sel.primary()?;
        let rect = [PORTRAIT_POS[0], PORTRAIT_POS[1], PORTRAIT_PX, PORTRAIT_PX];
        return point_in_rect(point, rect).then_some(id);
    }

    let store = world.entities();
    let mut drawn = 0usize;
    for &id in sel.ids() {
        if drawn >= MULTI_ICON_CAP {
            break;
        }
        if store.slot(id).is_none() {
            continue; // stale — skipped exactly like the packer skips it
        }
        let row = drawn / MULTI_ICON_COLS;
        let col = drawn % MULTI_ICON_COLS;
        let pos = [
            MULTI_ICON_ORIGIN[0] + col as f32 * (MULTI_ICON_PX + MULTI_ICON_GAP_PX),
            MULTI_ICON_ORIGIN[1] + row as f32 * (MULTI_ICON_PX + MULTI_ICON_GAP_PX),
        ];
        if point_in_rect(point, [pos[0], pos[1], MULTI_ICON_PX, MULTI_ICON_PX]) {
            return Some(id);
        }
        drawn += 1;
    }
    None
}

/// Classify a logical (1920x1080) point against the HUD's chrome.
///
/// `None` means the point belongs to the world — everywhere else in this
/// function's rect chain is HUD ground: [`HudHit::Background`] for chrome
/// with nothing under the pointer, so the app's pointer router can consume
/// every HUD-area click and never let one fall through as a world order.
pub fn hud_hit_test(world: &RtsWorld, point: [f32; 2]) -> Option<HudHit> {
    if point_in_rect(point, HudLayout::GEAR) {
        return Some(HudHit::Gear);
    }
    if point_in_rect(point, HudLayout::TOP_BAR) {
        return Some(HudHit::Background);
    }
    if point_in_rect(point, HudLayout::MINIMAP_MAP) {
        return Some(HudHit::Minimap(point));
    }
    if point_in_rect(point, HudLayout::MINIMAP_PANEL) {
        return Some(HudHit::Background);
    }
    if point_in_rect(point, HudLayout::SELECTION_PANEL) {
        return Some(match selection_icon_hit(world, point) {
            Some(id) => HudHit::SelectionIcon(id),
            None => HudHit::Background,
        });
    }
    if point_in_rect(point, HudLayout::COMMAND_GRID) {
        return Some(HudHit::CommandSlot(command_slot_index(point)));
    }
    if point_in_rect(point, HudLayout::COMMAND_PANEL) {
        return Some(HudHit::Background);
    }
    if point_in_rect(point, HudLayout::BOTTOM_PANEL) {
        return Some(HudHit::Background);
    }
    None
}
