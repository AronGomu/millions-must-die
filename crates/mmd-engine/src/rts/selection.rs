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

/// Pick radius for a unit, in cells, as a multiple of its body radius.
///
/// `1.0` — the pointer must land inside the circle the separation pass
/// separates on. Any other value would make "what you clicked" and "what
/// collides" two different shapes, and the hitbox overlay (`H`) would stop
/// being an explanation of the click.
pub const UNIT_PICK_RADIUS_SCALE: f32 = 1.0;

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

/// Resolve a screen-space click against the world.
///
/// Priority, in order, and the first hit wins:
/// 1. **Own units** — the player-owned unit whose centre is nearest the click's
///    cell-space point, within `body_radius * UNIT_PICK_RADIUS_SCALE` cells.
///    Nearest, not first-found, so overlapping bodies pick the one under the
///    cursor rather than the one with the lowest slot.
/// 2. **Own buildings** — the building whose footprint rectangle contains the
///    clicked cell. Footprints cannot overlap (T10 rejects that), so at most one.
/// 3. **Resource nodes** — the node occupying the clicked cell exactly.
/// 4. Nothing.
///
/// Units come first because a worker standing on its own base must be
/// clickable; the building is the larger target and would always win otherwise.
pub fn pick_at(world: &RtsWorld, view: &IsoView, screen: [f32; 2]) -> Pick {
    let p = view.unproject(screen[0], screen[1]);
    if !p[0].is_finite() || !p[1].is_finite() {
        return Pick::Nothing;
    }
    let r = world.scenario().collision_radius_cells() * UNIT_PICK_RADIUS_SCALE;
    let r2 = r * r;
    let slot_count = world.entities().slot_count();

    // 1. nearest own unit inside r
    let mut best: Option<(f32, EntityId)> = None;
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
        let q = world.entities().position(slot);
        let d2 = (q[0] - p[0]).powi(2) + (q[1] - p[1]).powi(2);
        if d2 <= r2 && best.is_none_or(|(bd, _)| d2 < bd) {
            best = Some((d2, world.entities().id_at(slot).expect("live")));
        }
    }
    if let Some((_, id)) = best {
        return Pick::Unit(id);
    }

    // 2. own building footprint
    let width = world.scenario().width();
    let height = world.scenario().height();
    let Some(cell) = view.cell_at(screen[0], screen[1], width, height) else {
        return Pick::Nothing;
    };
    for slot in 0..slot_count {
        if !world.entities().alive(slot) {
            continue;
        }
        let EntityKind::Building(b) = world.entities().kind(slot) else {
            continue;
        };
        if world.entities().owner(slot) != OWNER_PLAYER {
            continue;
        }
        if footprint_contains(world.entities().position(slot), b.footprint_cells(), cell) {
            return Pick::Building(world.entities().id_at(slot).expect("live"));
        }
    }

    // 3. node on that exact cell
    for slot in 0..slot_count {
        if !world.entities().alive(slot) {
            continue;
        }
        let EntityKind::Node(_) = world.entities().kind(slot) else {
            continue;
        };
        let q = world.entities().position(slot);
        if q[0].floor() as u32 == cell.x && q[1].floor() as u32 == cell.y {
            return Pick::Node(world.entities().id_at(slot).expect("live"));
        }
    }
    Pick::Nothing
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
