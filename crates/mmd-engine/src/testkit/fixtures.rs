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

/// Absolute path to the real 50k gate scenario.
pub fn gate_scenario_path() -> PathBuf {
    crate::workspace_root().join(GATE_SCENARIO)
}
