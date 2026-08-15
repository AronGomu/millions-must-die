//! Turning RTS world state into instances — T12.
//!
//! Pure CPU, no device: the same split phase 0 uses, where
//! [`crate::runtime::pack_instance_groups`] packs and the renderer draws. The
//! packer only ever *reads* the world, so a frame can never be a place state
//! quietly changes.
//!
//! The three layers of a [`ScenePass`] are not interchangeable. `overlay` is
//! drawn with texture slot 0 bound and is only honest for procedural rings
//! ([`SpriteInstance::ring`]); every *textured* depth-off instance — the
//! placement ghost, the rally flags, the drag box — goes in `ui` instead, or it
//! would sample the wrong sheet.

use super::build::{Placement, PlacementCandidate, placement_candidate};
use super::entity::{BuildingKind, EntityKind, MAX_ENTITIES, ResourceKind, UnitKind};
use super::selection::{RTS_SPRITE_SIZE_PX, building_quad_px, normalise_rect, stand_on};
use super::world::RtsWorld;
use crate::render::{
    DrawGroup, FrameUniforms, SLOT_RTS_BUILDINGS, SLOT_RTS_PROPS, SLOT_RTS_SOLDIER,
    SLOT_RTS_WORKER, SLOT_UI_FONT, ScenePass, SpriteInstance, frame_uv_rect, quad_is_visible,
};
use crate::runtime::ring_quad_size_px;

/// Columns of every RTS sheet — the grid [`frame_uv_rect`] addresses.
const SHEET_COLS: u32 = 4;
/// Row of the buildings sheet holding finished buildings.
const BUILDING_ROW_DONE: u32 = 0;
/// Row of the buildings sheet holding construction sites.
const BUILDING_ROW_SITE: u32 = 1;
/// Row of the buildings sheet holding resource nodes.
const NODE_ROW: u32 = 2;
/// Column offset from a node's full sprite to its depleted one.
const NODE_DEPLETED_COL_OFFSET: u32 = 2;

/// HUD glyph ceiling for the UI font group — T13 fills it.
const UI_TEXT_CAPACITY: usize = 4_096;

/// Named cells of the props sheet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Prop {
    SelectionRing = 0,
    PlacementOk = 1,
    PlacementBad = 2,
    RallyFlag = 3,
    CrystalIcon = 4,
    GasIcon = 5,
    SupplyIcon = 6,
    PanelFill = 7,
    GearIcon = 8,
    MinimapFrame = 9,
    IconBuildHq = 10,
    IconBuildDepot = 11,
    IconBuildBarracks = 12,
    IconTrainWorker = 13,
    IconTrainSoldier = 14,
    IconSetRally = 15,
}

/// UV rect of a prop cell. `(row, col) = (i / 4, i % 4)`.
pub fn prop_uv(prop: Prop) -> [f32; 4] {
    let i = prop as u32;
    frame_uv_rect(i / SHEET_COLS, i % SHEET_COLS)
}

/// UV rect of a building's sprite. Row 0 finished, row 1 under construction.
pub fn building_uv(kind: BuildingKind, under_construction: bool) -> [f32; 4] {
    let row = if under_construction {
        BUILDING_ROW_SITE
    } else {
        BUILDING_ROW_DONE
    };
    frame_uv_rect(row, kind as u32)
}

/// UV rect of a resource node. Row 2; columns 0/1 full, 2/3 depleted.
pub fn node_uv(kind: ResourceKind, depleted: bool) -> [f32; 4] {
    let col = kind as u32
        + if depleted {
            NODE_DEPLETED_COL_OFFSET
        } else {
            0
        };
    frame_uv_rect(NODE_ROW, col)
}

/// Texture slot a unit kind draws from.
pub fn unit_slot(kind: UnitKind) -> u32 {
    match kind {
        UnitKind::Worker => SLOT_RTS_WORKER,
        UnitKind::Soldier => SLOT_RTS_SOLDIER,
    }
}

/// Selection-ring tint, premultiplied. Green, deliberately not the cyan the
/// hitbox overlay uses: two rings that meant different things in the same
/// colour would be worse than no ring.
pub const SELECTION_TINT: [f32; 4] = [0.0, 0.60, 0.24, 0.60];
/// Selection-ring outer radius in normalised quad units, matching
/// `runtime::RING_OUTER`'s convention (`0.5` is the quad edge).
pub const SELECTION_RING_OUTER: f32 = 0.5;
/// Selection-ring inner radius in normalised quad units.
pub const SELECTION_RING_INNER: f32 = SELECTION_RING_OUTER - 1.0 / 24.0;

/// Drag-rectangle edge thickness in screen pixels.
pub const DRAG_BOX_THICKNESS_PX: f32 = 2.0;
/// Drag-rectangle 10%-opacity fill tint, premultiplied.
pub const DRAG_BOX_FILL_TINT: [f32; 4] = [0.0, 0.1, 0.0, 0.1];
/// Drag-rectangle opaque border tint, premultiplied.
pub const DRAG_BOX_BORDER_TINT: [f32; 4] = [0.0, 1.0, 0.0, 1.0];
/// Placement-ghost tint, premultiplied white — the sheet already carries the
/// colour and the alpha.
pub const GHOST_TINT: [f32; 4] = [1.0, 1.0, 1.0, 1.0];

/// Reusable per-frame instance buffers.
///
/// Owned by the caller (the app, or a test), not by [`RtsWorld`]: packing is a
/// projection of world state and must never be able to mutate it.
#[derive(Debug)]
pub struct RtsFrame {
    /// Depth-tested world groups, in slot order 4, 5, 6.
    pub world: Vec<DrawGroup>,
    /// Procedural rings only — selection rings.
    pub overlay: Vec<SpriteInstance>,
    /// Depth-off textured groups, texture slots [`SLOT_RTS_WORKER`] through
    /// [`SLOT_UI_FONT`] in order: worker, soldier, building, props, font.
    /// The placement ghost, the rally flags and the drag box always land in
    /// the props group; from `T11` the selection card's portrait/icons can
    /// also land in the worker/soldier/building groups, since a portrait is
    /// a crop of an existing sheet, never a duplicate image.
    pub ui: Vec<DrawGroup>,
    /// Live-slot buffer [`pack_frame`] reuses, reserved to [`MAX_ENTITIES`] so
    /// `collect_live` never allocates.
    scratch: Vec<usize>,
    /// The projection [`pack_frame`] packed this frame through — what
    /// [`Self::scene`] hands the renderer so the depth uniforms follow the
    /// live camera instead of the stale value fixed at renderer construction.
    /// `None` before the first `pack_frame` call.
    frame_uniforms: Option<FrameUniforms>,
}

impl Default for RtsFrame {
    fn default() -> Self {
        Self::new()
    }
}

impl RtsFrame {
    /// Reserve every buffer at the ceilings it can reach, so a frame never
    /// grows one.
    ///
    /// The prop group is reserved at [`MAX_ENTITIES`] rather than at its real
    /// ceiling (144 ghost tiles, one silhouette, one rally flag per selected
    /// building, four drag edges): the 96 KB is paid once at construction and
    /// keeps a per-slice tuning constant off the surface.
    pub fn new() -> Self {
        Self {
            world: [SLOT_RTS_WORKER, SLOT_RTS_SOLDIER, SLOT_RTS_BUILDINGS]
                .into_iter()
                .map(|atlas_id| DrawGroup {
                    atlas_id,
                    instances: Vec::with_capacity(MAX_ENTITIES),
                })
                .collect(),
            overlay: Vec::with_capacity(MAX_ENTITIES),
            ui: [
                SLOT_RTS_WORKER,
                SLOT_RTS_SOLDIER,
                SLOT_RTS_BUILDINGS,
                SLOT_RTS_PROPS,
            ]
            .into_iter()
            .map(|atlas_id| DrawGroup {
                atlas_id,
                instances: Vec::with_capacity(MAX_ENTITIES),
            })
            .chain(std::iter::once(DrawGroup {
                atlas_id: SLOT_UI_FONT,
                instances: Vec::with_capacity(UI_TEXT_CAPACITY),
            }))
            .collect(),
            scratch: Vec::with_capacity(MAX_ENTITIES),
            frame_uniforms: None,
        }
    }

    /// Clear every buffer, keeping capacity. `atlas_id` is never touched.
    pub fn clear(&mut self) {
        for g in self.world.iter_mut().chain(self.ui.iter_mut()) {
            g.instances.clear();
        }
        self.overlay.clear();
    }

    /// A [`ScenePass`] borrowing this frame, carrying the frame uniforms of
    /// the projection [`pack_frame`] last packed through.
    pub fn scene(&self) -> ScenePass<'_> {
        ScenePass {
            world: &self.world,
            overlay: &self.overlay,
            ui: &self.ui,
            frame_uniforms: self.frame_uniforms,
        }
    }

    /// Total instances across all three layers.
    pub fn instance_count(&self) -> usize {
        self.world
            .iter()
            .chain(self.ui.iter())
            .map(|g| g.instances.len())
            .sum::<usize>()
            + self.overlay.len()
    }

    /// The world group drawing texture slot `slot`.
    fn world_group(&mut self, slot: u32) -> &mut Vec<SpriteInstance> {
        let index = (slot - SLOT_RTS_WORKER) as usize;
        &mut self.world[index].instances
    }

    /// The UI group drawing texture slot `slot`. Slots [`SLOT_RTS_WORKER`]
    /// through [`SLOT_UI_FONT`], same order as [`Self::world_group`] plus the
    /// two UI-only slots.
    pub(super) fn ui_group(&mut self, slot: u32) -> &mut Vec<SpriteInstance> {
        let index = (slot - SLOT_RTS_WORKER) as usize;
        &mut self.ui[index].instances
    }

    /// The UI group drawing the prop sheet.
    fn prop_group(&mut self) -> &mut Vec<SpriteInstance> {
        self.ui_group(SLOT_RTS_PROPS)
    }
}

/// The drag rectangle currently being dragged, in screen pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DragBox {
    pub a: [f32; 2],
    pub b: [f32; 2],
}

/// Pack one frame of `world` into `frame`.
///
/// `cursor` is the pointer position in screen pixels, used to place the
/// placement ghost. `drag` is the live drag rectangle, if any.
///
/// Allocates nothing: every push lands in a buffer [`RtsFrame::new`] reserved.
///
/// The world layer is packed nodes-and-buildings-and-units in ascending entity
/// slot. The order does not affect the picture — the depth test sorts it — but
/// it does fix the packed byte order, and a frame is compared byte for byte in
/// tests.
pub fn pack_frame(world: &RtsWorld, cursor: [f32; 2], drag: Option<DragBox>, frame: &mut RtsFrame) {
    frame.clear();

    let iso = world.iso_view();
    frame.frame_uniforms = Some(iso.frame_uniforms());
    let store = world.entities();
    let sprite_size = RTS_SPRITE_SIZE_PX;

    // 1. World, depth-tested.
    //
    // `scratch` is moved out and back rather than borrowed: `collect_live`
    // needs it mutably while the loop below pushes into the same frame.
    let mut scratch = std::mem::take(&mut frame.scratch);
    store.collect_live(&mut scratch);
    for &slot in &scratch {
        let p = store.position(slot);
        let ground = iso.project(p[0], p[1]);
        match store.kind(slot) {
            EntityKind::Node(res) => {
                let pos = stand_on(ground, sprite_size);
                if !quad_is_visible(pos, sprite_size, iso.view_size) {
                    continue;
                }
                let uv = node_uv(res, store.amount(slot) == 0);
                frame
                    .world_group(SLOT_RTS_BUILDINGS)
                    .push(SpriteInstance::new(
                        pos,
                        sprite_size,
                        uv,
                        SpriteInstance::WHITE,
                    ));
            }
            EntityKind::Building(b) => {
                let size = building_quad_px(b.footprint_cells(), iso.tile_w, iso.tile_h);
                let pos = stand_on(ground, size);
                if !quad_is_visible(pos, size, iso.view_size) {
                    continue;
                }
                let uv = building_uv(b, store.progress_target(slot) > 0);
                frame
                    .world_group(SLOT_RTS_BUILDINGS)
                    .push(SpriteInstance::new(pos, size, uv, SpriteInstance::WHITE));
            }
            EntityKind::Unit(u) => {
                let pos = stand_on(ground, sprite_size);
                if !quad_is_visible(pos, sprite_size, iso.view_size) {
                    continue;
                }
                let uv = frame_uv_rect(u32::from(store.dir(slot)), u32::from(store.frame(slot)));
                frame.world_group(unit_slot(u)).push(SpriteInstance::new(
                    pos,
                    sprite_size,
                    uv,
                    SpriteInstance::WHITE,
                ));
            }
        }
    }
    frame.scratch = scratch;

    // 2. Overlay: one procedural ring per selected entity, centred on its
    //    ground point. The quad is the same expression the hitbox overlay
    //    uses, so "the ring shows the shape the world uses" stays one
    //    derivation: a unit's body radius, a footprint's half edge.
    let body_radius = world.scenario().collision_radius_cells();
    for &id in world.selection().ids() {
        let Some(slot) = store.slot(id) else {
            continue;
        };
        let kind = store.kind(slot);
        let radius_cells = match kind {
            EntityKind::Unit(_) => body_radius,
            _ => kind.footprint_cells() as f32 * 0.5,
        };
        let size = ring_quad_size_px(iso.tile_w, iso.tile_h, radius_cells);
        if !size[0].is_finite() || !size[1].is_finite() || size[0] <= 0.0 || size[1] <= 0.0 {
            continue;
        }
        let p = store.position(slot);
        let ground = iso.project(p[0], p[1]);
        let pos = [ground[0] - size[0] * 0.5, ground[1] - size[1] * 0.5];
        if !quad_is_visible(pos, size, iso.view_size) {
            continue;
        }
        frame.overlay.push(SpriteInstance::ring(
            pos,
            size,
            SELECTION_RING_INNER,
            SELECTION_RING_OUTER,
            SELECTION_TINT,
        ));
    }

    // 3. UI, depth-off and textured: rally flags, then the placement ghost,
    //    then the drag box.
    for &id in world.selection().ids() {
        let Some(cell) = world.rally(id) else {
            continue;
        };
        let ground = iso.project(cell.x as f32 + 0.5, cell.y as f32 + 0.5);
        let pos = stand_on(ground, sprite_size);
        if !quad_is_visible(pos, sprite_size, iso.view_size) {
            continue;
        }
        frame.prop_group().push(SpriteInstance::new(
            pos,
            sprite_size,
            prop_uv(Prop::RallyFlag),
            SpriteInstance::WHITE,
        ));
    }

    if let Placement::Pending { kind } = world.placement() {
        let width = world.scenario().width();
        let height = world.scenario().height();
        if let Some(cell) = iso.cell_at(cursor[0], cursor[1], width, height) {
            let PlacementCandidate { min, valid } = placement_candidate(world, kind, cell);
            let edge = kind.footprint_cells();
            let uv = prop_uv(if valid {
                Prop::PlacementOk
            } else {
                Prop::PlacementBad
            });

            // One tile per footprint cell, centred on that cell's ground point.
            let tile = [iso.tile_w, iso.tile_h * 2.0];
            for dy in 0..edge {
                for dx in 0..edge {
                    let cx = min.x.saturating_add(dx) as f32 + 0.5;
                    let cy = min.y.saturating_add(dy) as f32 + 0.5;
                    let ground = iso.project(cx, cy);
                    let pos = [ground[0] - tile[0] * 0.5, ground[1] - tile[1] * 0.5];
                    if !quad_is_visible(pos, tile, iso.view_size) {
                        continue;
                    }
                    frame
                        .prop_group()
                        .push(SpriteInstance::new(pos, tile, uv, GHOST_TINT));
                }
            }

            // …plus one silhouette of the building itself, standing on the
            // footprint's centre exactly the way the finished building will.
            let centre = [
                min.x as f32 + edge as f32 * 0.5,
                min.y as f32 + edge as f32 * 0.5,
            ];
            let size = building_quad_px(edge, iso.tile_w, iso.tile_h);
            let ground = iso.project(centre[0], centre[1]);
            let pos = stand_on(ground, size);
            if quad_is_visible(pos, size, iso.view_size) {
                frame.prop_group().push(SpriteInstance::new(
                    pos,
                    size,
                    building_uv(kind, true),
                    GHOST_TINT,
                ));
            }
        }
    }

    if let Some(d) = drag {
        let (min, max) = normalise_rect(d.a, d.b);
        let x = min[0];
        let y = min[1];
        let w = max[0] - x;
        let h = max[1] - y;
        let t = DRAG_BOX_THICKNESS_PX;
        // Fill first: a horizontal segment whose thickness equals the rect height
        // produces a solid-fill quad without sampling the atlas.
        frame.prop_group().push(SpriteInstance::diagonal_line(
            [x, y + h * 0.5],
            [x + w, y + h * 0.5],
            h,
            DRAG_BOX_FILL_TINT,
        ));
        // Four edges with half-thickness insets so each border quad sits fully
        // inside the rect rather than straddling its edge.
        frame.prop_group().push(SpriteInstance::diagonal_line(
            [x, y + 1.0],
            [x + w, y + 1.0],
            t,
            DRAG_BOX_BORDER_TINT,
        ));
        frame.prop_group().push(SpriteInstance::diagonal_line(
            [x, y + h - 1.0],
            [x + w, y + h - 1.0],
            t,
            DRAG_BOX_BORDER_TINT,
        ));
        frame.prop_group().push(SpriteInstance::diagonal_line(
            [x + 1.0, y],
            [x + 1.0, y + h],
            t,
            DRAG_BOX_BORDER_TINT,
        ));
        frame.prop_group().push(SpriteInstance::diagonal_line(
            [x + w - 1.0, y],
            [x + w - 1.0, y + h],
            t,
            DRAG_BOX_BORDER_TINT,
        ));
    }
}
