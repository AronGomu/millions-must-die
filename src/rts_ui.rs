//! Pointer ownership and the shared command executor — `T12`.
//!
//! A HUD click is resolved here, before the world ever sees it: its
//! [`HudHit`] decides whether it drives a card/icon/minimap action or falls
//! through to `RtsWorld`'s own click/order path. Keyboard hotkeys and
//! command-grid clicks both resolve through [`execute_command`], so the two
//! input paths cannot drift.

use mmd_engine::rts::{
    BuildingKind, CommandId, HudHit, HudLayout, RtsWorld, UnitKind, command_slots, hud_hit_test,
    minimap_projection,
};

use crate::rts_run::RtsSession;

/// Which side of the input boundary a pointer gesture belongs to.
///
/// [`Self::None`] is the idle state before any point has been classified;
/// [`owner_for_point`] only ever returns [`Self::World`] or [`Self::Hud`].
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum PointerOwner {
    #[default]
    None,
    World,
    Hud(HudHit),
}

/// Classify one logical (1920x1080) point: [`PointerOwner::Hud`] when it
/// lands on the HUD's chrome, else [`PointerOwner::World`].
pub fn owner_for_point(world: &RtsWorld, point: [f32; 2]) -> PointerOwner {
    match hud_hit_test(world, point) {
        Some(hit) => PointerOwner::Hud(hit),
        None => PointerOwner::World,
    }
}

/// Run one [`CommandId`] — shared by the command-grid click path (which
/// checks `enabled` itself before calling this) and every keyboard hotkey
/// (which always calls this: a hotkey acts on whatever is actually
/// selected, independent of what the card currently shows).
///
/// A build/produce call that is not currently legal (wrong selection, no
/// resources, no supply) is a no-op here exactly as it always was —
/// `RtsWorld::begin_placement`/`enqueue_unit` already validate and refuse.
pub fn execute_command(world: &mut RtsWorld, session: &mut RtsSession, id: CommandId) {
    match id {
        CommandId::BuildHq => {
            let _ = world.begin_placement(BuildingKind::Hq);
        }
        CommandId::BuildDepot => {
            let _ = world.begin_placement(BuildingKind::Depot);
        }
        CommandId::BuildBarracks => {
            let _ = world.begin_placement(BuildingKind::Barracks);
        }
        CommandId::TrainWorker => {
            if let Some(building) = world.selection().primary() {
                let _ = world.enqueue_unit(building, UnitKind::Worker);
            }
        }
        CommandId::TrainSoldier => {
            if let Some(building) = world.selection().primary() {
                let _ = world.enqueue_unit(building, UnitKind::Soldier);
            }
        }
        CommandId::SetRally => {
            // Arms a pending action rather than acting immediately: the next
            // world left-click (not one over the HUD) sets the cell.
            session.pending_rally = world.selection().primary();
        }
    }
}

/// Resolve one HUD click. Every variant is consumed here — none of them
/// ever issues a world order or falls through to selection/placement logic.
pub fn handle_hud_click(world: &mut RtsWorld, session: &mut RtsSession, hit: HudHit, shift: bool) {
    match hit {
        HudHit::Gear => {
            // The settings modal is `T13`'s scope; the click is still
            // consumed here so it can never leak through as a world order.
        }
        HudHit::Minimap(point) => {
            let origin = [HudLayout::MINIMAP_MAP[0], HudLayout::MINIMAP_MAP[1]];
            let local = [point[0] - origin[0], point[1] - origin[1]];
            let projection = minimap_projection(world);
            if let Some(map_point) = projection.minimap_to_map(local) {
                world.look_at_map_point(map_point);
            }
            // Outside the map diamond: consumed, no move — per T12's spec.
        }
        HudHit::SelectionIcon(id) => {
            if shift {
                world.toggle_selection(id);
            } else {
                world.select_only(id);
            }
        }
        HudHit::CommandSlot(idx) => {
            let slots = command_slots(world);
            if let Some(slot) = slots.get(idx as usize)
                && slot.enabled
                && let Some(cmd) = slot.command
            {
                execute_command(world, session, cmd);
            }
            // Disabled/empty: consumed, no action — per T12's spec.
        }
        HudHit::Background => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_pointer_owner_is_none() {
        assert_eq!(PointerOwner::default(), PointerOwner::None);
    }
}
