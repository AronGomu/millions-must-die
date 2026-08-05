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

/// Every tracked fixture, for suites that assert across all of them.
pub const ALL_FIXTURES: &[&str] = &[FIXTURE_SMALL_V1, FIXTURE_CORRIDOR_V1];

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
