//! Optional stdout HUD text for interactive `rts`.
//!
//! Distinct from `mmd_engine::rts::pack_hud`, which packs the *on-screen*
//! resource/command panel into every frame regardless of this toggle: this
//! is the F1-gated `key=value` line a human or a script parses from stdout,
//! the same role `overlay::format_overlay` plays for `run`.

use mmd_engine::rts::{BuildingKind, Placement, RtsWorld};

/// Format one `rts: hud ...` line: tick, resources, supply, selection count,
/// and the pending build ghost (if any).
pub fn format_rts_overlay(world: &RtsWorld) -> String {
    let res = world.resources();
    let supply = world.supply();
    let ghost = match world.placement() {
        Placement::None => "none",
        Placement::Pending { kind } => ghost_name(kind),
    };
    format!(
        "rts: hud tick={} crystal={} gas={} supply={}/{} sel={} ghost={}",
        world.tick_index(),
        res.crystal,
        res.gas,
        supply.used(),
        supply.cap(),
        world.selection().len(),
        ghost
    )
}

fn ghost_name(kind: BuildingKind) -> &'static str {
    match kind {
        BuildingKind::Hq => "hq",
        BuildingKind::Depot => "depot",
        BuildingKind::Barracks => "barracks",
    }
}
