//! Weapons and the shared range geometry of instant-hit combat.
//!
//! Data only: the combat *system* lives in `world.rs` beside the other
//! tick systems because it mutates `RtsWorld` internals. What lives here
//! is the per-kind weapon table and the surface-distance rule both the
//! system and its tests share.

use super::entity::{BuildingKind, EntityKind, EntityStore, UnitKind};
use super::orders::{dist2, rect_distance};

/// An instant-hit weapon: no projectile entity, the damage lands on the
/// tick the shot fires.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Weapon {
    /// Raw damage per shot, before the defender's armor — the world
    /// applies `max(1, damage - armor)`.
    pub damage: u32,
    /// Ticks between shots. A unit fires when its cooldown column reads
    /// 0 and the column resets to this on every shot, so the firing
    /// period is exactly this many ticks.
    pub cooldown_ticks: u32,
    /// Reach in cells, measured to the target's *surface* — see
    /// [`surface_distance`].
    pub range_cells: f32,
}

/// The weapon a unit kind carries. `None` never fires. Buildings join
/// this table in the turret slice; until then only units are armed.
pub fn weapon(kind: UnitKind) -> Option<Weapon> {
    match kind {
        UnitKind::Worker => None,
        UnitKind::Soldier => Some(Weapon {
            damage: 6,
            cooldown_ticks: 15,
            range_cells: 24.0,
        }),
        UnitKind::Ghoul => Some(Weapon {
            damage: 5,
            cooldown_ticks: 30,
            range_cells: 8.0,
        }),
    }
}

/// Weapon of a building kind. Only the Turret is armed.
///
/// A `None` building never enters the combat scan as a firer, and an
/// armed building fires only once **finished** (`progress_target == 0`)
/// — a construction site has no working weapon. Buildings are static
/// firers: no order, no chase; the scan alone decides.
pub fn building_weapon(kind: BuildingKind) -> Option<Weapon> {
    match kind {
        BuildingKind::Turret => Some(Weapon {
            damage: 10,
            cooldown_ticks: 20,
            range_cells: 36.0,
        }),
        BuildingKind::Hq | BuildingKind::Depot | BuildingKind::Barracks => None,
    }
}

/// Distance from an attacker standing at `p` to the *surface* of the
/// entity in store slot `target_slot`: centre distance minus body radius
/// for a unit, distance to the footprint rectangle for a building — the
/// same geometry family as `interaction_reach`. `None` for a node:
/// nodes are indestructible and never a combat target.
pub(crate) fn surface_distance(
    store: &EntityStore,
    p: [f32; 2],
    target_slot: usize,
) -> Option<f32> {
    let q = store.position(target_slot);
    match store.kind(target_slot) {
        EntityKind::Unit(k) => Some(dist2(p, q).sqrt() - k.body_radius_cells()),
        EntityKind::Building(b) => Some(rect_distance(p, q, b.footprint_cells())),
        EntityKind::Node(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The design numbers, pinned: a balance edit must be a visible diff
    /// here, not only in behaviour.
    #[test]
    fn the_weapon_table_matches_the_design_numbers() {
        assert_eq!(weapon(UnitKind::Worker), None);
        assert_eq!(
            weapon(UnitKind::Soldier),
            Some(Weapon {
                damage: 6,
                cooldown_ticks: 15,
                range_cells: 24.0
            })
        );
        assert_eq!(
            weapon(UnitKind::Ghoul),
            Some(Weapon {
                damage: 5,
                cooldown_ticks: 30,
                range_cells: 8.0
            })
        );
    }
}
