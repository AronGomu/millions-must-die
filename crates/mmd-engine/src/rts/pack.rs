//! Turning RTS world state into instances — T12.
//!
//! Pure CPU, no device: the same split phase 0 uses, where
//! [`crate::runtime::pack_instance_groups`] packs and the renderer draws. The
//! packer only ever *reads* the world, so a frame can never be a place state
//! quietly changes.
//!
//! The three layers of a [`ScenePass`] are not interchangeable. `overlay` is
//! drawn with texture slot 0 bound and is only honest for texture-free
//! procedural instances: selection rings and death flashes ([`SpriteInstance::ring`]) and world
//! grid lines and HP bars ([`SpriteInstance::diagonal_line`]). Every *textured* depth-off
//! instance — the placement ghost, the rally flags, the drag box — goes in
//! `ui` instead, or it would sample the wrong sheet. A textured instance in
//! the overlay layer is always a bug.

use super::build::{Placement, PlacementCandidate, placement_candidate};
use super::entity::{
    BuildingKind, EntityId, EntityKind, MAX_ENTITIES, RTS_UNIT_BODY_DIAMETER_CELLS, ResourceKind,
    UnitKind, max_hp,
};
use super::orders::{GatherPhase, Order};
use super::production::RallyTarget;
use super::selection::{RTS_SPRITE_SIZE_PX, building_quad_px, normalise_rect, stand_on};
use super::world::{DeathEvent, RtsWorld};
use crate::render::{
    DrawGroup, FrameUniforms, IsoView, SLOT_RTS_BUILDINGS, SLOT_RTS_PROPS, SLOT_RTS_SOLDIER,
    SLOT_RTS_WORKER, SLOT_UI_FONT, ScenePass, SpriteInstance, frame_uv_rect, quad_is_visible,
};
use crate::runtime::ring_quad_size_px;
use crate::scenario::{BUILD_SQUARE_CELLS, RTS_MAX_MAP_EDGE};

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

/// Maximum grid lines for a full-map isometric lattice.
///
/// The lattice is drawn in **build squares**, not cells: two boundary
/// families, `x = 0..=width` and `y = 0..=height`, each stepping by
/// [`BUILD_SQUARE_CELLS`]. The map is square-bounded by [`RTS_MAX_MAP_EDGE`],
/// so the ceiling is `2 * (RTS_MAX_MAP_EDGE / BUILD_SQUARE_CELLS + 1)`.
pub const MAX_GRID_LINES: usize = 2 * (RTS_MAX_MAP_EDGE as usize / BUILD_SQUARE_CELLS as usize + 1);

/// Grid-line stroke width in screen pixels.
pub const GRID_LINE_PX: f32 = 1.0;

/// Alternating dash / gap count per dashed move-route line.
pub const DASH_SEGMENTS: usize = 12;
/// Maximum selection size for which dashed lines are drawn; beyond this limit
/// all dashes are suppressed (a partial picture is worse than none).
pub const MAX_SELECTION_FOR_DASHES: usize = 24;
/// Overlay reservation for dashed lines: at most one dash set per mover in the
/// maximum supported selection, with `DASH_SEGMENTS / 2` pieces each.
pub const MAX_DASH_LINES: usize = MAX_SELECTION_FOR_DASHES * DASH_SEGMENTS / 2;

/// Worst case for the rally dashes: the same ceiling again, since a selection
/// of nothing but rallied producers draws one dashed line each.
pub const MAX_RALLY_DASH_LINES: usize = MAX_DASH_LINES;

/// Grid-line tint (premultiplied). Thin subdued green, nearly transparent.
pub const GRID_TINT: [f32; 4] = [0.05, 0.08, 0.05, 0.16];

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
    IconAttack = 16,
    IconStop = 17,
    IconBuildTurret = 18,
    MoveMarker = 19,
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
    #[allow(clippy::match_same_arms)]
    match kind {
        UnitKind::Worker => SLOT_RTS_WORKER,
        UnitKind::Soldier => SLOT_RTS_SOLDIER,
        // Enemy art is out of this slice's scope: the Ghoul draws from the
        // soldier sheet until a dedicated sheet exists.
        UnitKind::Ghoul => SLOT_RTS_SOLDIER,
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
/// Target-ring inner radius — same tint and outer as the selection ring, half
/// the band thickness, so "targeted" is visually distinct from "selected" at
/// a glance without a second hue.
pub const TARGET_RING_INNER: f32 = SELECTION_RING_OUTER - 1.0 / 48.0;

/// Drag-rectangle edge thickness in screen pixels.
pub const DRAG_BOX_THICKNESS_PX: f32 = 2.0;
/// Drag-rectangle 10%-opacity fill tint, premultiplied.
pub const DRAG_BOX_FILL_TINT: [f32; 4] = [0.0, 0.1, 0.0, 0.1];
/// Drag-rectangle opaque border tint, premultiplied.
pub const DRAG_BOX_BORDER_TINT: [f32; 4] = [0.0, 1.0, 0.0, 1.0];
/// Placement-ghost tint, premultiplied white — the sheet already carries the
/// colour and the alpha.
pub const GHOST_TINT: [f32; 4] = [1.0, 1.0, 1.0, 1.0];

/// HP-bar backing tint — near-black, premultiplied, slightly translucent
/// so the world reads through the empty part of a bar.
pub const HP_BAR_BACKING_TINT: [f32; 4] = [0.02, 0.02, 0.02, 0.85];
/// HP-bar fill strictly above 2/3 health.
pub const HP_BAR_GREEN_TINT: [f32; 4] = [0.05, 0.80, 0.10, 1.0];
/// HP-bar fill between 1/3 and 2/3 health, both boundaries included.
pub const HP_BAR_YELLOW_TINT: [f32; 4] = [0.85, 0.75, 0.10, 1.0];
/// HP-bar fill strictly below 1/3 health.
pub const HP_BAR_RED_TINT: [f32; 4] = [0.85, 0.10, 0.08, 1.0];
/// How far above the sprite quad's top edge a bar's centreline sits, in
/// multiples of the isometric tile height (1.5 cells).
pub const HP_BAR_RAISE_CELLS: f32 = 1.5;

/// The fill tint for `hp` of `max` remaining health.
///
/// Integer thresholds, never a float ratio: green strictly above 2/3,
/// red strictly below 1/3, yellow between — both exact boundaries land
/// yellow, so a threshold can never flicker on rounding. `max` is never
/// 0 here: nodes are excluded before any bar is packed.
pub fn hp_bar_fill_tint(hp: u32, max: u32) -> [f32; 4] {
    let (hp3, max_u) = (3 * u64::from(hp), u64::from(max));
    if hp3 > 2 * max_u {
        HP_BAR_GREEN_TINT
    } else if hp3 < max_u {
        HP_BAR_RED_TINT
    } else {
        HP_BAR_YELLOW_TINT
    }
}

/// Per-frame packing options forwarded by the app.
#[derive(Clone, Copy, Debug, Default)]
pub struct FramePackOptions {
    /// Render the full-map isometric grid lattice in the overlay layer.
    pub show_grid: bool,
}

/// Reusable per-frame instance buffers.
///
/// Owned by the caller (the app, or a test), not by [`RtsWorld`]: packing is a
/// projection of world state and must never be able to mutate it.
#[derive(Debug)]
pub struct RtsFrame {
    /// Depth-tested world groups, in slot order 4, 5, 6.
    pub world: Vec<DrawGroup>,
    /// Texture-free procedural instances: world grid lines, selection rings, HP bars, then the app-appended death-flash rings.
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
            overlay: Vec::with_capacity(
                4 * MAX_ENTITIES + MAX_GRID_LINES + MAX_DASH_LINES + MAX_RALLY_DASH_LINES,
            ),
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

/// Death-flash lifetime, in rendered frames.
pub const DEATH_FLASH_FRAMES: u8 = 12;
/// Death-flash ring tint — premultiplied ember red-orange.
pub const DEATH_FLASH_TINT: [f32; 4] = [0.95, 0.30, 0.10, 0.80];
/// Death-flash outer radius in normalised quad units (`0.5` = quad edge).
pub const DEATH_FLASH_OUTER: f32 = 0.5;
/// Death-flash inner radius — twice the selection ring's band, so a
/// flash reads as an event, not as a selection.
pub const DEATH_FLASH_INNER: f32 = DEATH_FLASH_OUTER - 1.0 / 12.0;

/// App-owned render feedback: the death flashes currently on screen.
///
/// Not world state — the world only surfaces [`DeathEvent`]s, and how
/// long a flash lingers is a property of rendered frames, which only the
/// app counts. Both buffers are reserved once at construction and reused
/// every frame; nothing here allocates after `new`.
#[derive(Debug)]
pub struct DeathFlashes {
    /// Live flashes with their remaining frame counts.
    active: Vec<(DeathEvent, u8)>,
    /// Drain buffer handed to [`RtsWorld::drain_death_events`].
    events: Vec<DeathEvent>,
}

impl Default for DeathFlashes {
    fn default() -> Self {
        Self::new()
    }
}

impl DeathFlashes {
    /// Reserve both buffers at [`MAX_ENTITIES`].
    pub fn new() -> Self {
        Self {
            active: Vec::with_capacity(MAX_ENTITIES),
            events: Vec::with_capacity(MAX_ENTITIES),
        }
    }

    /// Pull this tick's deaths out of `world` and start a
    /// [`DEATH_FLASH_FRAMES`]-frame flash for each. Bounded: with
    /// [`MAX_ENTITIES`] flashes already live, a further event is dropped
    /// rather than grown into.
    pub fn absorb(&mut self, world: &mut RtsWorld) {
        world.drain_death_events(&mut self.events);
        for &event in &self.events {
            if self.active.len() < MAX_ENTITIES {
                self.active.push((event, DEATH_FLASH_FRAMES));
            }
        }
    }

    /// Append one procedural ring per live flash to `frame.overlay`,
    /// culled by the same visibility test every packed quad obeys.
    pub fn pack(&self, iso: &IsoView, frame: &mut RtsFrame) {
        for &(event, _) in &self.active {
            let radius_cells = match event.kind {
                EntityKind::Unit(u) => u.body_radius_cells(),
                EntityKind::Building(b) => b.footprint_cells() as f32 * 0.5,
                EntityKind::Node(_) => continue,
            };
            let size = ring_quad_size_px(iso.tile_w, iso.tile_h, radius_cells);
            let ground = iso.project(event.center[0], event.center[1]);
            let pos = [ground[0] - size[0] * 0.5, ground[1] - size[1] * 0.5];
            if !quad_is_visible(pos, size, iso.view_size) {
                continue;
            }
            frame.overlay.push(SpriteInstance::ring(
                pos,
                size,
                DEATH_FLASH_INNER,
                DEATH_FLASH_OUTER,
                DEATH_FLASH_TINT,
            ));
        }
    }

    /// Age every flash by one rendered frame, dropping the expired in
    /// place (`Vec::retain` compacts without allocating).
    pub fn age(&mut self) {
        for f in &mut self.active {
            f.1 -= 1;
        }
        self.active.retain(|&(_, left)| left > 0);
    }

    /// Live flash count — the observability seam tests read.
    pub fn active_count(&self) -> usize {
        self.active.len()
    }
}

/// Pack the isometric build-square lattice into `frame.overlay`.
///
/// Two families of boundary lines: `x = 0..=width` projected as column edges,
/// and `y = 0..=height` projected as row edges, both stepping by
/// [`BUILD_SQUARE_CELLS`]. Exactly `width / BUILD_SQUARE_CELLS + height /
/// BUILD_SQUARE_CELLS + 2` lines are pushed on a map whose edges are whole
/// squares, plus one closing boundary per axis that is not; the caller must
/// ensure `frame.overlay` has capacity.
fn pack_grid(world: &RtsWorld, frame: &mut RtsFrame) {
    let iso = world.iso_view();
    let w = world.scenario().width();
    let h = world.scenario().height();
    let step = BUILD_SQUARE_CELLS as usize;
    // x-family: vertical column edges, x in 0..=width.
    for x in (0..=w).step_by(step) {
        let a = iso.project(x as f32, 0.0);
        let b = iso.project(x as f32, h as f32);
        frame
            .overlay
            .push(SpriteInstance::diagonal_line(a, b, GRID_LINE_PX, GRID_TINT));
    }
    // A map edge that is not a whole number of squares still gets its last line.
    if !w.is_multiple_of(BUILD_SQUARE_CELLS) {
        let a = iso.project(w as f32, 0.0);
        let b = iso.project(w as f32, h as f32);
        frame
            .overlay
            .push(SpriteInstance::diagonal_line(a, b, GRID_LINE_PX, GRID_TINT));
    }
    // y-family: horizontal row edges, y in 0..=height.
    for y in (0..=h).step_by(step) {
        let a = iso.project(0.0, y as f32);
        let b = iso.project(w as f32, y as f32);
        frame
            .overlay
            .push(SpriteInstance::diagonal_line(a, b, GRID_LINE_PX, GRID_TINT));
    }
    if !h.is_multiple_of(BUILD_SQUARE_CELLS) {
        let a = iso.project(0.0, h as f32);
        let b = iso.project(w as f32, h as f32);
        frame
            .overlay
            .push(SpriteInstance::diagonal_line(a, b, GRID_LINE_PX, GRID_TINT));
    }
}

/// Pack one frame of `world` into `frame`, with options forwarded from the app.
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
///
/// The overlay is ordered: grid lines (if enabled), selection rings, then HP bars; the app appends death-flash rings after this returns.
pub fn pack_frame_with_options(
    world: &RtsWorld,
    cursor: [f32; 2],
    drag: Option<DragBox>,
    options: FramePackOptions,
    frame: &mut RtsFrame,
) {
    pack_frame_inner(world, cursor, drag, options, frame);
}

/// Legacy wrapper: grid is always off. Preserved for tests that predated
/// [`FramePackOptions`] and do not want grid lines in their overlay counts.
pub fn pack_frame(world: &RtsWorld, cursor: [f32; 2], drag: Option<DragBox>, frame: &mut RtsFrame) {
    pack_frame_inner(
        world,
        cursor,
        drag,
        FramePackOptions { show_grid: false },
        frame,
    );
}

/// The entity an order points at, if it points at one at all.
///
/// Exhaustive by design: a new [`Order`] variant has to decide here whether it
/// wears a target ring, instead of silently inheriting "no ring". A goal-cell
/// order ([`Order::Move`], [`Order::AttackMove`]) targets ground, not an
/// entity, and gets nothing.
/// Push one dashed segment run — `DASH_SEGMENTS / 2` texture-free diagonal
/// line instances, on for the first half of each segment pair — from `from`
/// to `to`, both already in projected ground space.
///
/// The one place the lerp lives: a mover's line to its formation slot and a
/// producer's line to its rally flag are the same picture drawn between
/// different endpoints.
fn push_dashed_line(frame: &mut RtsFrame, from: [f32; 2], to: [f32; 2]) {
    for i in 0..(DASH_SEGMENTS / 2) {
        let t0 = (2 * i) as f32 / DASH_SEGMENTS as f32;
        let t1 = (2 * i + 1) as f32 / DASH_SEGMENTS as f32;
        let a = [
            from[0] + (to[0] - from[0]) * t0,
            from[1] + (to[1] - from[1]) * t0,
        ];
        let b = [
            from[0] + (to[0] - from[0]) * t1,
            from[1] + (to[1] - from[1]) * t1,
        ];
        frame.overlay.push(SpriteInstance::diagonal_line(
            a,
            b,
            GRID_LINE_PX,
            SELECTION_TINT,
        ));
    }
}

fn order_ring_target(world: &RtsWorld, id: EntityId) -> Option<EntityId> {
    match world.order_of(id)? {
        Order::Gather { node, .. } => Some(node),
        Order::Build { site, .. } => Some(site),
        Order::Attack { target, .. } => Some(target),
        Order::Follow { target, .. } => Some(target),
        Order::Idle | Order::Move { .. } | Order::AttackMove { .. } => None,
    }
}

fn pack_frame_inner(
    world: &RtsWorld,
    cursor: [f32; 2],
    drag: Option<DragBox>,
    options: FramePackOptions,
    frame: &mut RtsFrame,
) {
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

    // 2a. Grid overlay: full-map isometric lattice, packed before rings. The
    //     lattice is a placement aid first and a setting second, so a pending
    //     ghost forces it on whatever the setting says.
    if options.show_grid || matches!(world.placement(), Placement::Pending { .. }) {
        pack_grid(world, frame);
    }

    // 2b. Overlay: one procedural ring per selected entity, centred on its
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

    // 2c. Target rings: one ring on whatever each selected entity is heading
    //    toward, in the selection ring's tint but half its band. Derived from
    //    the live order every frame, which is what makes it survive a
    //    deselect/reselect; at most one ring per selected entity, so the
    //    overlay reserve above still covers the worst case.
    let sel_ids = world.selection().ids();
    for (i, &id) in sel_ids.iter().enumerate() {
        let Some(target_id) = order_ring_target(world, id) else {
            continue;
        };
        // Skip: the target already wears a selection ring of its own.
        if world.selection().contains(target_id) {
            continue;
        }
        // Skip: an earlier selected entity already ringed this target. The
        // rescan is O(n²) over the selection and allocates nothing, which is
        // the trade `frame_allocations.rs` demands.
        if sel_ids[..i]
            .iter()
            .any(|&prev| order_ring_target(world, prev) == Some(target_id))
        {
            continue;
        }
        let Some(target_slot) = store.slot(target_id) else {
            continue;
        };
        let target_kind = store.kind(target_slot);
        let radius_cells = match target_kind {
            EntityKind::Unit(_) => body_radius,
            _ => target_kind.footprint_cells() as f32 * 0.5,
        };
        let size = ring_quad_size_px(iso.tile_w, iso.tile_h, radius_cells);
        if !size[0].is_finite() || !size[1].is_finite() || size[0] <= 0.0 || size[1] <= 0.0 {
            continue;
        }
        let p = store.position(target_slot);
        let ground = iso.project(p[0], p[1]);
        let pos = [ground[0] - size[0] * 0.5, ground[1] - size[1] * 0.5];
        if !quad_is_visible(pos, size, iso.view_size) {
            continue;
        }
        frame.overlay.push(SpriteInstance::ring(
            pos,
            size,
            TARGET_RING_INNER,
            SELECTION_RING_OUTER,
            SELECTION_TINT,
        ));
    }

    // 2d. Overlay: HP bars — two texture-free line instances per shown
    //    entity, after the rings so a bar paints over its own ring. Shown =
    //    live, not a node, and damaged (hp < max) ∪ selected. Render-side
    //    derivation only: a bar can no more enter the world hash than a
    //    grid line can.
    let scratch = std::mem::take(&mut frame.scratch);
    for &slot in &scratch {
        let kind = store.kind(slot);
        let (width_cells, quad) = match kind {
            EntityKind::Unit(_) => (RTS_UNIT_BODY_DIAMETER_CELLS, sprite_size),
            EntityKind::Building(b) => {
                let edge = b.footprint_cells();
                (edge as f32, building_quad_px(edge, iso.tile_w, iso.tile_h))
            }
            // A node has no HP semantics: no bar, selected or not.
            EntityKind::Node(_) => continue,
        };
        let hp = store.hp(slot);
        let max = max_hp(kind);
        let selected = store
            .id_at(slot)
            .is_some_and(|id| world.selection().contains(id));
        if hp >= max && !selected {
            continue;
        }
        let p = store.position(slot);
        let ground = iso.project(p[0], p[1]);
        let width = width_cells * iso.tile_w;
        let y = ground[1] - quad[1] - HP_BAR_RAISE_CELLS * iso.tile_h;
        let left = ground[0] - width * 0.5;
        let t = DRAG_BOX_THICKNESS_PX;
        if !quad_is_visible([left, y - t * 0.5], [width, t], iso.view_size) {
            continue;
        }
        frame.overlay.push(SpriteInstance::diagonal_line(
            [left, y],
            [left + width, y],
            t,
            HP_BAR_BACKING_TINT,
        ));
        let fill = width * (hp as f32 / max as f32);
        frame.overlay.push(SpriteInstance::diagonal_line(
            [left, y],
            [left + fill, y],
            t,
            hp_bar_fill_tint(hp, max),
        ));
    }
    frame.scratch = scratch;

    // 2e. Dashed lines: one set of `DASH_SEGMENTS / 2` diagonal_line pieces
    //     per selected mover, from its ground point to its formation slot.
    //     Suppressed entirely when the selection exceeds MAX_SELECTION_FOR_DASHES
    //     (a partial picture is worse than none, and the overlay reserve stays
    //     honest). Texture-free, so the overlay is the correct layer.
    if world.selection().ids().len() <= MAX_SELECTION_FOR_DASHES {
        for &id in world.selection().ids() {
            let goal = match world.order_of(id) {
                Some(Order::Move { goal, .. }) => goal,
                Some(Order::Build { goal, .. }) => goal,
                Some(Order::Gather {
                    phase: GatherPhase::ToNode { goal, .. },
                    ..
                }) => goal,
                Some(Order::AttackMove { goal, .. }) => goal,
                _ => continue,
            };
            let Some(slot) = store.slot(id) else {
                continue;
            };
            let p = store.position(slot);
            let unit_ground = iso.project(p[0], p[1]);
            let goal_ground = iso.project(goal.slot.x as f32 + 0.5, goal.slot.y as f32 + 0.5);
            push_dashed_line(frame, unit_ground, goal_ground);
        }
    }

    // 3. UI, depth-off and textured: rally flags with dashes, then the placement ghost,
    //    then the drag box.
    for &id in world.selection().ids() {
        let Some(rally) = world.rally(id) else {
            continue;
        };
        // Resolve the flag's ground point from the rally target.
        let flag_pos_cells = match rally {
            RallyTarget::Cell(cell) => [cell.x as f32 + 0.5, cell.y as f32 + 0.5],
            RallyTarget::Entity(eid) => {
                let Some(t_slot) = store.slot(eid) else {
                    continue;
                };
                let p = store.position(t_slot);
                [p[0], p[1]]
            }
        };
        let flag_ground = iso.project(flag_pos_cells[0], flag_pos_cells[1]);
        let flag_sprite_pos = stand_on(flag_ground, sprite_size);
        if !quad_is_visible(flag_sprite_pos, sprite_size, iso.view_size) {
            continue;
        }
        frame.prop_group().push(SpriteInstance::new(
            flag_sprite_pos,
            sprite_size,
            prop_uv(Prop::RallyFlag),
            SpriteInstance::WHITE,
        ));
        // The user's precision on the rally feedback: a dashed line from the
        // producing building to the flag, so a rally reads as one gesture
        // rather than a flag dropped somewhere off-screen. Gated by the same
        // selection ceiling the mover dashes use, which is what keeps the
        // overlay reserve above an exact bound.
        if world.selection().ids().len() <= MAX_SELECTION_FOR_DASHES
            && let Some(bslot) = store.slot(id)
            && matches!(store.kind(bslot), EntityKind::Building(_))
        {
            let b_pos = store.position(bslot);
            let b_ground = iso.project(b_pos[0], b_pos[1]);
            push_dashed_line(frame, b_ground, flag_ground);
        }
    }

    // Move-marker quads: one prop quad per live marker, same pattern as the
    // rally flag. Textured, depth-off — must go in the prop group, not overlay.
    for (cell, _) in world.move_markers() {
        let ground = iso.project(cell.x as f32 + 0.5, cell.y as f32 + 0.5);
        let pos = stand_on(ground, sprite_size);
        if !quad_is_visible(pos, sprite_size, iso.view_size) {
            continue;
        }
        frame.prop_group().push(SpriteInstance::new(
            pos,
            sprite_size,
            prop_uv(Prop::MoveMarker),
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

            // One tile per build square, centred on that square's ground point.
            let squares = edge / BUILD_SQUARE_CELLS;
            let tile = [
                iso.tile_w * BUILD_SQUARE_CELLS as f32,
                iso.tile_h * 2.0 * BUILD_SQUARE_CELLS as f32,
            ];
            for sy in 0..squares {
                for sx in 0..squares {
                    let cx = min.x.saturating_add(sx * BUILD_SQUARE_CELLS) as f32
                        + BUILD_SQUARE_CELLS as f32 * 0.5;
                    let cy = min.y.saturating_add(sy * BUILD_SQUARE_CELLS) as f32
                        + BUILD_SQUARE_CELLS as f32 * 0.5;
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
