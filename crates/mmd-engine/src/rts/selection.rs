//! Turning a screen-space pointer gesture into a set of entity handles.
//!
//! Pure logic: no SDL, no rendering, no orders. Drawing the selection ring
//! (T12), the HUD panel (T13) and mouse plumbing (T14) are out of scope here.

use sha2::{Digest, Sha256};

use crate::render::IsoView;
use crate::scenario::Cell;

use super::entity::{EntityId, EntityKind, EntityStore, MAX_ENTITIES, OWNER_PLAYER};
use super::world::RtsWorld;

/// Selection ceiling.
///
/// `docs/DESIGN.md` records "Unlimited unit selection" as a design decision, so
/// the only honest ceiling is the entity store's own. A smaller,
/// RTS-traditional 12 would be a gameplay rule this prototype has not chosen.
pub const MAX_SELECTION: usize = MAX_ENTITIES;

/// Rendered sprite quad size, in pixels, shared by the picker and the packer.
///
/// The tracked RTS scenes pin `sprite_size_px: 48` (`scenario::RTS_SPRITE_PX`);
/// naming it here rather than reading it back off the scenario every call is
/// what makes [`sprite_screen_rect`] and `pack_frame`'s node/unit quads provably
/// the same rectangle instead of two derivations that happen to agree today.
pub const RTS_SPRITE_SIZE_PX: [f32; 2] = [48.0, 48.0];

/// Whether a drag is long enough to be a box rather than a click.
///
/// Below this, a press-and-release is a click even if the pointer twitched.
pub const DRAG_MIN_PX: f32 = 4.0;

/// An ordered set of entity handles.
///
/// Iteration is by **ascending entity slot**, not by click order: an order
/// issued to a selection must be reproducible, and click order is not part of
/// the world's state.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Selection {
    ids: Vec<EntityId>,
}

impl Selection {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.ids.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    pub fn clear(&mut self) {
        self.ids.clear();
    }

    fn sort_key(id: EntityId) -> (u32, u32) {
        (id.index, id.generation)
    }

    fn position_of(&self, id: EntityId) -> Result<usize, usize> {
        self.ids
            .binary_search_by_key(&Self::sort_key(id), |&e| Self::sort_key(e))
    }

    pub fn contains(&self, id: EntityId) -> bool {
        self.position_of(id).is_ok()
    }

    /// Ascending by slot index.
    pub fn ids(&self) -> &[EntityId] {
        &self.ids
    }

    /// Insert, keeping sort order. `false` when already present or at the cap.
    pub fn insert(&mut self, id: EntityId) -> bool {
        match self.position_of(id) {
            Ok(_) => false,
            Err(pos) => {
                if self.ids.len() >= MAX_SELECTION {
                    return false;
                }
                self.ids.insert(pos, id);
                true
            }
        }
    }

    /// Remove. `false` when absent.
    pub fn remove(&mut self, id: EntityId) -> bool {
        match self.position_of(id) {
            Ok(pos) => {
                self.ids.remove(pos);
                true
            }
            Err(_) => false,
        }
    }

    /// Insert if absent, remove if present. Returns the new membership.
    pub fn toggle(&mut self, id: EntityId) -> bool {
        if self.contains(id) {
            self.remove(id);
            false
        } else {
            self.insert(id)
        }
    }

    /// Replace the whole set with `ids`, sorted and deduplicated.
    pub fn replace(&mut self, ids: &[EntityId]) {
        self.ids.clear();
        for &id in ids {
            self.insert(id);
        }
    }

    /// Drop every handle the store no longer resolves. Returns how many went.
    pub fn retain_live(&mut self, store: &EntityStore) -> usize {
        let before = self.ids.len();
        self.ids.retain(|&id| store.contains(id));
        before - self.ids.len()
    }

    /// The first id, in slot order — the "primary" selection the HUD describes.
    pub fn primary(&self) -> Option<EntityId> {
        self.ids.first().copied()
    }

    pub fn hash_into(&self, h: &mut Sha256) {
        h.update((self.ids.len() as u64).to_le_bytes());
        for id in &self.ids {
            h.update(id.index.to_le_bytes());
            h.update(id.generation.to_le_bytes());
        }
    }
}

/// What a click landed on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pick {
    Unit(EntityId),
    Building(EntityId),
    Node(EntityId),
    Nothing,
}

/// Whether `cell` lies in the footprint of edge `edge` centred at `center`.
///
/// The footprint's minimum corner is `center - edge/2`, matching how
/// `RtsWorld` seeds the HQ (`hq_cell` is the min corner, position is the centre).
pub fn footprint_contains(center: [f32; 2], edge: u32, cell: Cell) -> bool {
    let half = edge as f32 * 0.5;
    let x0 = (center[0] - half).round() as i64;
    let y0 = (center[1] - half).round() as i64;
    let cx = cell.x as i64;
    let cy = cell.y as i64;
    cx >= x0 && cx < x0 + edge as i64 && cy >= y0 && cy < y0 + edge as i64
}

/// The footprint's minimum corner, as cell coordinates.
pub fn footprint_min(center: [f32; 2], edge: u32) -> Cell {
    let half = edge as f32 * 0.5;
    Cell {
        x: (center[0] - half).round() as u32,
        y: (center[1] - half).round() as u32,
    }
}

/// Screen rectangle normalised to `(min, max)`.
pub fn normalise_rect(a: [f32; 2], b: [f32; 2]) -> ([f32; 2], [f32; 2]) {
    (
        [a[0].min(b[0]), a[1].min(b[1])],
        [a[0].max(b[0]), a[1].max(b[1])],
    )
}

/// Classify a press/release pair.
pub fn is_drag(a: [f32; 2], b: [f32; 2]) -> bool {
    (b[0] - a[0]).abs() >= DRAG_MIN_PX || (b[1] - a[1]).abs() >= DRAG_MIN_PX
}

/// The screen-space pixel dimensions of a building of `edge_cells` cells.
///
/// Shared with [`super::pack`], which uses the same formula to size the quad
/// the renderer draws — one derivation rather than two that must agree.
pub fn building_quad_px(edge_cells: u32, tile_w: f32, tile_h: f32) -> [f32; 2] {
    let edge = edge_cells as f32;
    [edge * tile_w, edge * tile_h * 2.0]
}

/// The screen-space rect the renderer draws a building of `edge_cells` into.
///
/// `(min_x, min_y, max_x, max_y)`. The building stands on its footprint
/// centre: [`stand_on`] positions the sprite so its bottom edge sits on the
/// projected ground point.
pub fn building_screen_rect(view: &IsoView, ground: [f32; 2], edge_cells: u32) -> [f32; 4] {
    let g = view.project(ground[0], ground[1]);
    let size = building_quad_px(edge_cells, view.tile_w, view.tile_h);
    let pos = stand_on(g, size);
    [pos[0], pos[1], pos[0] + size[0], pos[1] + size[1]]
}

/// Whether a click at `screen` lands inside the **rendered sprite quad** of a
/// building whose footprint is centred at `ground` (cell space).
///
/// The quad is a square that towers far above the plot and is mostly
/// transparent up there, so it is [`pick_at`]'s *fallback* shape only: it
/// covers ground points belonging to entities drawn behind the tower, and a
/// click there means the entity, not the pixels of empty air over it.
pub fn building_quad_contains(
    view: &IsoView,
    ground: [f32; 2],
    edge_cells: u32,
    screen: [f32; 2],
) -> bool {
    let rect = building_screen_rect(view, ground, edge_cells);
    screen[0] >= rect[0] && screen[0] <= rect[2] && screen[1] >= rect[1] && screen[1] <= rect[3]
}

/// Whether a click at `screen` lands on the **footprint cells** of a building
/// whose footprint is centred at `ground` (cell space) — the plot it stands
/// on, which is the building's exact shape.
pub fn building_plot_contains(
    view: &IsoView,
    ground: [f32; 2],
    edge_cells: u32,
    screen: [f32; 2],
) -> bool {
    let p = view.unproject(screen[0], screen[1]);
    if !p[0].is_finite() || !p[1].is_finite() {
        return false;
    }
    let cell = Cell {
        x: p[0].floor().max(0.0) as u32,
        y: p[1].floor().max(0.0) as u32,
    };
    footprint_contains(ground, edge_cells, cell)
}

/// Whether a building whose footprint is centred at `ground` (cell space) is
/// hit by a click at `screen`.
///
/// Union of two shapes: the full rendered sprite rect (so the whole tower is
/// always clickable) **or** the footprint cells that project below the sprite
/// (so a click on the building's plot rather than its tower still picks it).
/// [`pick_at`] does not use the union directly — it ranks the two shapes, see
/// there — but the union is what "this click is over that building at all"
/// means.
pub fn building_pick_contains(
    view: &IsoView,
    ground: [f32; 2],
    edge_cells: u32,
    screen: [f32; 2],
) -> bool {
    building_quad_contains(view, ground, edge_cells, screen)
        || building_plot_contains(view, ground, edge_cells, screen)
}

/// The top-left corner of a quad of `size` whose **bottom edge** sits on
/// screen point `ground` and which is horizontally centred on it.
///
/// Shared with [`super::pack::pack_frame`], which anchors every world sprite
/// the same way: a sprite stands on its tile rather than being centred on it.
/// One function, not two agreeing derivations, is what makes
/// [`sprite_screen_rect`] provably the rectangle the renderer draws.
pub(crate) fn stand_on(ground: [f32; 2], size: [f32; 2]) -> [f32; 2] {
    [ground[0] - size[0] * 0.5, ground[1] - size[1]]
}

/// The screen-space rectangle `pack_frame` draws a [`RTS_SPRITE_SIZE_PX`] quad
/// into for an entity whose ground point is `ground` (cell space).
///
/// `(min_x, min_y, max_x, max_y)`.
pub fn sprite_screen_rect(view: &IsoView, ground: [f32; 2]) -> [f32; 4] {
    let g = view.project(ground[0], ground[1]);
    let pos = stand_on(g, RTS_SPRITE_SIZE_PX);
    [
        pos[0],
        pos[1],
        pos[0] + RTS_SPRITE_SIZE_PX[0],
        pos[1] + RTS_SPRITE_SIZE_PX[1],
    ]
}

/// Whether a unit whose ground point is `ground` (cell space) is hit by a
/// click at `screen`.
///
/// The union of two shapes: the full rendered [`sprite_screen_rect`] (so the
/// visible sprite is always clickable) **or** the unprojected body circle of
/// `radius` cells around `ground` (so the hitbox the collision pass separates
/// on stays clickable even where the sprite quad does not cover it —
/// diagonally, past the sprite's edges).
pub fn unit_pick_contains(view: &IsoView, ground: [f32; 2], radius: f32, screen: [f32; 2]) -> bool {
    let rect = sprite_screen_rect(view, ground);
    if screen[0] >= rect[0] && screen[0] <= rect[2] && screen[1] >= rect[1] && screen[1] <= rect[3]
    {
        return true;
    }
    let p = view.unproject(screen[0], screen[1]);
    if !p[0].is_finite() || !p[1].is_finite() {
        return false;
    }
    let dx = p[0] - ground[0];
    let dy = p[1] - ground[1];
    dx * dx + dy * dy <= radius * radius
}

/// The depth key [`super::pack::pack_frame`] would give an entity whose
/// ground point is `ground` (cell space) — the same [`IsoView::depth`] call,
/// so a pick can only ever agree with what the GPU's `GREATER` depth test put
/// on top of the pixel.
pub fn entity_pick_depth(view: &IsoView, ground: [f32; 2]) -> f32 {
    let s = view.project(ground[0], ground[1]);
    view.depth(s[1])
}

/// Which shape of an entity a click landed on — the rank a candidate competes
/// in inside [`pick_at`].
#[derive(Clone, Copy, PartialEq, Eq)]
enum PickTier {
    /// An exact shape: a unit's sprite/body, a node's quad, a building's plot.
    Exact,
    /// A building's rendered sprite quad, most of which is empty air.
    BuildingQuad,
}

/// Resolve a screen-space click against the world.
///
/// Candidates are ranked in two tiers, and a tier-1 hit always beats a tier-2
/// one however deep:
/// 1. **Exact shapes** — the geometry an entity actually occupies.
/// 2. **Building sprite quads** — consulted only when tier 1 matched nothing.
///
/// Within a tier, the candidate with the greatest [`entity_pick_depth`] wins —
/// the same comparison the GPU's `GREATER` depth test makes when two sprites
/// overlap a pixel. Iteration is ascending by entity slot and only a
/// **strictly** greater depth replaces the current best of that tier, so an
/// exact tie keeps the lower slot: the earlier-packed instance the depth test
/// would not have let a later, equal-depth one overwrite.
///
/// Hit shapes, by kind:
/// - **Unit** (player-owned only) — tier 1, [`unit_pick_contains`]: the full
///   sprite rect union the unit kind's body circle.
/// - **Building** (player-owned only) — tier 1 on its plot, via
///   [`building_plot_contains`]; tier 2 over the rest of its rendered quad,
///   via [`building_quad_contains`]. The whole tower stays clickable, but its
///   quad is a square of mostly transparent air whose ground point is deeper
///   than everything drawn behind it, so letting it compete as a peer would
///   swallow every node and unit standing near a building.
/// - **Node** — tier 1, the full [`sprite_screen_rect`], no ownership filter.
///
/// No type-priority branch beyond that: a worker standing on its own HQ's plot
/// wins only when its ground point renders strictly in front of the HQ's.
pub fn pick_at(world: &RtsWorld, view: &IsoView, screen: [f32; 2]) -> Pick {
    let slot_count = world.entities().slot_count();

    let mut exact: Option<(f32, usize)> = None;
    let mut quad: Option<(f32, usize)> = None;
    for slot in 0..slot_count {
        if !world.entities().alive(slot) {
            continue;
        }
        let mine = world.entities().owner(slot) == OWNER_PLAYER;
        let ground = world.entities().position(slot);
        let hit = match world.entities().kind(slot) {
            EntityKind::Unit(kind) => (mine
                && unit_pick_contains(view, ground, kind.body_radius_cells(), screen))
            .then_some(PickTier::Exact),
            EntityKind::Building(b) => {
                let edge = b.footprint_cells();
                if !mine {
                    None
                } else if building_plot_contains(view, ground, edge, screen) {
                    Some(PickTier::Exact)
                } else if building_quad_contains(view, ground, edge, screen) {
                    Some(PickTier::BuildingQuad)
                } else {
                    None
                }
            }
            EntityKind::Node(_) => {
                let rect = sprite_screen_rect(view, ground);
                (screen[0] >= rect[0]
                    && screen[0] <= rect[2]
                    && screen[1] >= rect[1]
                    && screen[1] <= rect[3])
                    .then_some(PickTier::Exact)
            }
        };
        let best = match hit {
            Some(PickTier::Exact) => &mut exact,
            Some(PickTier::BuildingQuad) => &mut quad,
            None => continue,
        };
        let depth = entity_pick_depth(view, ground);
        if best.is_none_or(|(bd, _)| depth > bd) {
            *best = Some((depth, slot));
        }
    }

    match exact.or(quad) {
        Some((_, slot)) => {
            let id = world.entities().id_at(slot).expect("live");
            match world.entities().kind(slot) {
                EntityKind::Unit(_) => Pick::Unit(id),
                EntityKind::Building(_) => Pick::Building(id),
                EntityKind::Node(_) => Pick::Node(id),
            }
        }
        None => Pick::Nothing,
    }
}

/// Every player-owned **unit** whose ground point lies inside a screen-space
/// rectangle, in ascending slot order, appended to `out` (cleared first).
///
/// Units only. A box that selected buildings would make "select all and move"
/// silently mean something different, and no RTS this one is modelled on does
/// it. Neutral nodes are never boxed.
///
/// The rectangle is normalised, so a drag in any direction works; a degenerate
/// rectangle (zero width or height) selects nothing.
pub fn box_select(
    world: &RtsWorld,
    view: &IsoView,
    a: [f32; 2],
    b: [f32; 2],
    out: &mut Vec<EntityId>,
) {
    out.clear();
    let (min, max) = normalise_rect(a, b);
    if min[0] == max[0] || min[1] == max[1] {
        return;
    }
    let slot_count = world.entities().slot_count();
    for slot in 0..slot_count {
        if !world.entities().alive(slot) {
            continue;
        }
        if !matches!(world.entities().kind(slot), EntityKind::Unit(_)) {
            continue;
        }
        if world.entities().owner(slot) != OWNER_PLAYER {
            continue;
        }
        let pos = world.entities().position(slot);
        let s = view.project(pos[0], pos[1]);
        if s[0] >= min[0] && s[0] <= max[0] && s[1] >= min[1] && s[1] <= max[1] {
            out.push(world.entities().id_at(slot).expect("live"));
        }
    }
}
