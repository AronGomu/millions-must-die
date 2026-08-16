//! Building costs, build times, supply grants, footprints, and placement
//! validity — T10.

use crate::scenario::Cell;

use super::economy::Resources;
use super::entity::{BuildingKind, EntityKind};
use super::selection::footprint_min;
use super::world::RtsWorld;

/// The minimum corner of a footprint of `edge` cells centred on `cell`.
///
/// `cell - edge/2`, saturating at zero. The ghost follows the cursor's cell as
/// its **centre**, which is what every RTS does; anchoring the min corner to
/// the cursor makes a 12-cell building appear down-right of the pointer.
pub fn ghost_min_corner(cell: Cell, edge: u32) -> Cell {
    let half = edge / 2;
    Cell {
        x: cell.x.saturating_sub(half),
        y: cell.y.saturating_sub(half),
    }
}

/// Cost of each building.
pub const HQ_COST: Resources = Resources {
    crystal: 400,
    gas: 0,
};
pub const DEPOT_COST: Resources = Resources {
    crystal: 100,
    gas: 0,
};
pub const BARRACKS_COST: Resources = Resources {
    crystal: 150,
    gas: 25,
};

/// Cost of a building kind.
pub fn building_cost(kind: BuildingKind) -> Resources {
    match kind {
        BuildingKind::Hq => HQ_COST,
        BuildingKind::Depot => DEPOT_COST,
        BuildingKind::Barracks => BARRACKS_COST,
    }
}

/// Ticks of *attended* construction each building needs. 60 ticks = 1 second.
pub const HQ_BUILD_TICKS: u32 = 600;
pub const DEPOT_BUILD_TICKS: u32 = 180;
pub const BARRACKS_BUILD_TICKS: u32 = 300;

/// Attended construction time of a building kind, in ticks.
pub fn build_ticks(kind: BuildingKind) -> u32 {
    match kind {
        BuildingKind::Hq => HQ_BUILD_TICKS,
        BuildingKind::Depot => DEPOT_BUILD_TICKS,
        BuildingKind::Barracks => BARRACKS_BUILD_TICKS,
    }
}

/// Supply ceiling a finished building grants.
pub const HQ_SUPPLY_GRANT: u32 = 10;
pub const DEPOT_SUPPLY_GRANT: u32 = 10;
pub const BARRACKS_SUPPLY_GRANT: u32 = 0;

/// Supply cap a finished building of `kind` grants.
pub fn supply_grant(kind: BuildingKind) -> u32 {
    match kind {
        BuildingKind::Hq => HQ_SUPPLY_GRANT,
        BuildingKind::Depot => DEPOT_SUPPLY_GRANT,
        BuildingKind::Barracks => BARRACKS_SUPPLY_GRANT,
    }
}

/// Whether extra workers speed a site up.
///
/// They do not. One attending worker advances construction by exactly one tick
/// per tick; a second changes nothing. Additive build speed is a balance knob,
/// and phase 1 is not a balance pass — a constant that reads `false` is the
/// honest way to record that the question was asked and deferred.
pub const EXTRA_BUILDERS_SPEED_UP: bool = false;

/// Why a footprint was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PlacementError {
    #[error("footprint leaves the map")]
    OutOfBounds,
    #[error("footprint covers terrain at ({x}, {y})")]
    BlockedTerrain { x: u32, y: u32 },
    #[error("footprint overlaps another building")]
    OverlapsBuilding,
    #[error("footprint covers a resource node at ({x}, {y})")]
    CoversNode { x: u32, y: u32 },
    #[error("not enough resources")]
    Unaffordable,
    #[error("no live player worker was given as the builder")]
    NoBuilder,
    #[error("the entity store is full")]
    StoreFull,
}

/// The pending build ghost.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Placement {
    #[default]
    None,
    /// A build was chosen and is following the cursor.
    Pending { kind: BuildingKind },
}

/// Whether `kind` may occupy the footprint whose minimum corner is `min`.
///
/// Checked in this order, and the first failure is returned:
/// 1. the whole rectangle is inside the grid,
/// 2. no cell is scenario terrain (`FieldPool::blocked`, which also carries
///    every already-placed building — see the note below),
/// 3. no cell lies inside another live building's footprint,
/// 4. no resource node's cell lies inside the rectangle.
///
/// **Units are not an obstruction.** A worker standing where you want a Depot is
/// not a reason to refuse the build; the genre does not do that, and a rule that
/// depends on a moving unit makes placement validity flicker frame to frame.
///
/// Rule 3 is redundant with rule 2 for *finished* buildings, which are stamped
/// into the mask — but not for **sites**, which are not stamped until they
/// finish. Both rules stay: rule 2 catches terrain and finished buildings,
/// rule 3 catches sites under construction.
pub fn placement_valid(
    world: &RtsWorld,
    kind: BuildingKind,
    min: Cell,
) -> Result<(), PlacementError> {
    let edge = kind.footprint_cells();
    let width = world.scenario().width();
    let height = world.scenario().height();

    // 1. inside the grid.
    let fits_x = min.x.checked_add(edge).is_some_and(|mx| mx <= width);
    let fits_y = min.y.checked_add(edge).is_some_and(|my| my <= height);
    if !fits_x || !fits_y {
        return Err(PlacementError::OutOfBounds);
    }

    // 2. no terrain, and no already-finished building (both live in the same
    // mask). Deliberately the *raw* mask, not the radius-inflated navigation
    // mask `world.nav()` now serves: a body clearance requirement is not a
    // placement exclusion rule, and inflating this check would refuse a
    // building the raw terrain has room for.
    let blocked = world.static_nav().placement_solids();
    for y in min.y..min.y + edge {
        for x in min.x..min.x + edge {
            let idx = (x + y * width) as usize;
            if blocked[idx] {
                return Err(PlacementError::BlockedTerrain { x, y });
            }
        }
    }

    // 3. no overlap with another live building's footprint, finished or not.
    let slot_count = world.entities().slot_count();
    for slot in 0..slot_count {
        if !world.entities().alive(slot) {
            continue;
        }
        let EntityKind::Building(other_kind) = world.entities().kind(slot) else {
            continue;
        };
        let other_edge = other_kind.footprint_cells();
        let other_min = footprint_min(world.entities().position(slot), other_edge);
        let overlaps = min.x < other_min.x + other_edge
            && min.x + edge > other_min.x
            && min.y < other_min.y + other_edge
            && min.y + edge > other_min.y;
        if overlaps {
            return Err(PlacementError::OverlapsBuilding);
        }
    }

    // 4. no resource node inside the rectangle.
    for slot in 0..slot_count {
        if !world.entities().alive(slot) {
            continue;
        }
        let EntityKind::Node(_) = world.entities().kind(slot) else {
            continue;
        };
        let pos = world.entities().position(slot);
        let cell = Cell {
            x: pos[0].floor() as u32,
            y: pos[1].floor() as u32,
        };
        if cell.x >= min.x && cell.x < min.x + edge && cell.y >= min.y && cell.y < min.y + edge {
            return Err(PlacementError::CoversNode {
                x: cell.x,
                y: cell.y,
            });
        }
    }

    Ok(())
}

/// The result of a placement candidate search.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlacementCandidate {
    pub min: Cell,
    pub valid: bool,
}

/// Compute the best placement candidate for `kind` at `cursor_cell`.
///
/// Returns the raw min corner unchanged if it is valid. Otherwise searches all
/// min-corner deltas `dx, dy ∈ [-edge, +edge]` (one-footprint-width radius),
/// calls `placement_valid` for each, and returns the candidate closest to the
/// raw min corner (squared `i64` distance measured from the **saturated** raw
/// min corner; ties broken by lowest flat index `x + y * map_width`). If no
/// candidate is found, returns the raw min corner with `valid = false`.
///
/// Allocates nothing: all work is on the stack.
pub fn placement_candidate(
    world: &RtsWorld,
    kind: BuildingKind,
    cursor_cell: Cell,
) -> PlacementCandidate {
    let edge = kind.footprint_cells();
    let raw = ghost_min_corner(cursor_cell, edge);

    if placement_valid(world, kind, raw).is_ok() {
        return PlacementCandidate {
            min: raw,
            valid: true,
        };
    }

    let width = world.scenario().width();
    let raw_x = raw.x as i64;
    let raw_y = raw.y as i64;
    let radius = edge as i64;

    let mut best_min: Option<Cell> = None;
    let mut best_dist2: i64 = i64::MAX;
    let mut best_flat: u64 = u64::MAX;

    for dy in -radius..=radius {
        for dx in -radius..=radius {
            let cx = raw_x + dx;
            let cy = raw_y + dy;
            if cx < 0 || cy < 0 || cx > u32::MAX as i64 || cy > u32::MAX as i64 {
                continue;
            }
            let cand = Cell {
                x: cx as u32,
                y: cy as u32,
            };
            if placement_valid(world, kind, cand).is_err() {
                continue;
            }
            let dist2 = dx * dx + dy * dy;
            let flat = cx as u64 + cy as u64 * width as u64;
            if dist2 < best_dist2 || (dist2 == best_dist2 && flat < best_flat) {
                best_dist2 = dist2;
                best_flat = flat;
                best_min = Some(cand);
            }
        }
    }

    match best_min {
        Some(min) => PlacementCandidate { min, valid: true },
        None => PlacementCandidate {
            min: raw,
            valid: false,
        },
    }
}

/// The cells a footprint occupies, as an iterator, from its centre and edge.
pub fn footprint_cells(center: [f32; 2], edge: u32) -> impl Iterator<Item = Cell> {
    let min = footprint_min(center, edge);
    (0..edge).flat_map(move |dy| {
        (0..edge).map(move |dx| Cell {
            x: min.x + dx,
            y: min.y + dy,
        })
    })
}
