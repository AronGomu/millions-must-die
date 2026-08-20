//! Tracked scenario assets the harness can load by name.
//!
//! Fixtures live under `assets/scenarios/fixtures/` and obey the same
//! tracked-hash contract as the gate scene: each `<name>.ron` has a committed
//! `<name>.sha256` sidecar and is loaded through [`crate::scenario::Scenario::load_verified`].

use std::path::PathBuf;

/// Directory holding the small harness fixtures, relative to the workspace root.
pub const FIXTURE_DIR: &str = "assets/scenarios/fixtures";
/// The real phase-0 workload, relative to the workspace root.
pub const GATE_SCENARIO: &str = "assets/scenarios/technical_prototype_v1.ron";

/// 40×30 open grid with isolated pillars. 64 agents, 8 spawn corners.
/// Cheap default for determinism and lifecycle tests.
pub const FIXTURE_SMALL_V1: &str = "fixture_small_v1";
/// 32×24 serpentine walls. 200 agents, 4 west-edge spawns. Every route to the
/// destination must go around an obstacle, so navigation is actually exercised.
pub const FIXTURE_CORRIDOR_V1: &str = "fixture_corridor_v1";
/// 32×24 pillar field, one third of the grid blocked (T30 obstacle-dense case).
/// 256 agents, 4 west-edge spawn groups. Corridors are one row / two columns
/// wide, so every route weaves and the "never enters an obstacle" claim is
/// tested against constant wall contact rather than open ground.
pub const FIXTURE_DENSE_V1: &str = "fixture_dense_v1";
/// 24×16 with two offset walls **and a fully sealed 3×3 chamber** (T30
/// unreachable case): cells 10..=12 × 6..=8 are free ground with no route to
/// the destination, so the field must mark them
/// [`crate::nav::flow_field::COST_UNREACHABLE`] with a zero vector.
pub const FIXTURE_WALLED_V1: &str = "fixture_walled_v1";

/// 96×96 RTS combat fixture: two pre-placed Ghouls, two spawn points,
/// two timed waves (8 at tick 50, 4 at tick 120), two workers, one node
/// of each kind.
///
/// Deliberately **not** in [`ALL_FIXTURES`]: that list feeds the phase-0
/// flow-field suites, and this is an RTS-family scene
/// (`version: "rts_prototype_v1"` — the validator requires that version
/// on any scene carrying an `rts:` block) with `hard_agent_count: 0`.
pub const FIXTURE_RTS_COMBAT_V1: &str = "fixture_rts_combat_v1";

/// Every tracked fixture, for suites that assert across all of them.
pub const ALL_FIXTURES: &[&str] = &[
    FIXTURE_SMALL_V1,
    FIXTURE_CORRIDOR_V1,
    FIXTURE_DENSE_V1,
    FIXTURE_WALLED_V1,
];

/// Absolute path to a tracked fixture scenario.
pub fn fixture_path(name: &str) -> PathBuf {
    crate::workspace_root()
        .join(FIXTURE_DIR)
        .join(format!("{name}.ron"))
}

/// Absolute path to the real gate scenario, at the live agent ceiling.
pub fn gate_scenario_path() -> PathBuf {
    crate::workspace_root().join(GATE_SCENARIO)
}

/// Full-screen collision demo at the live ceiling: 5 000 agents with a 6-cell
/// body, separation amortised over four phases.
pub const COLLISION_MID_SCENE: &str = "assets/scenarios/collision_mid_v1.ron";
/// Full-screen collision demo at sprite scale: 1 200 agents with the same
/// 6-cell body over two push-priority classes, so the asymmetric push shows.
pub const COLLISION_SPRITE_SCENE: &str = "assets/scenarios/collision_sprite_v1.ron";
/// Both collision demo scenes, for suites that assert across the family.
pub const ALL_COLLISION_SCENES: &[&str] = &[COLLISION_MID_SCENE, COLLISION_SPRITE_SCENE];

/// The phase-1 RTS prototype scene, relative to `assets/scenarios/`.
pub const RTS_SCENE: &str = "rts_prototype_v1.ron";

/// Absolute path of the tracked RTS prototype scene.
///
/// `RTS_SCENE` is a bare filename (relative to `assets/scenarios/`, unlike
/// `scene_path`'s other callers which pass a full workspace-relative path),
/// so this joins the directory itself rather than delegating to `scene_path`.
pub fn rts_scene_path() -> PathBuf {
    crate::workspace_root()
        .join("assets/scenarios")
        .join(RTS_SCENE)
}

/// Absolute path to a scenario given workspace-relative.
pub fn scene_path(rel: &str) -> PathBuf {
    crate::workspace_root().join(rel)
}
