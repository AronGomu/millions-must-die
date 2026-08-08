# Glossary

[x] Activated
[x] Project scanned

## Sim / nav

| word       | short description                                    | ref in code                                                                    |
| ---------- | ----------------------------------------------------- | ------------------------------------------------------------------------------- |
| runtime    | Owns scenario+sim+renderer, drives one frame           | `crates/mmd-engine/src/runtime.rs`, `struct Runtime`                            |
| scenario   | Parsed/validated level definition (grid, spawns, counts) | `crates/mmd-engine/src/scenario.rs`, `struct Scenario`                        |
| flowfield  | Per-cell cost/direction grid agents steer by           | `crates/mmd-engine/src/nav/flow_field.rs`, `struct FlowField`                   |
| simulation | Agent position/velocity state and tick stepping         | `crates/mmd-engine/src/sim/agents.rs`, `struct Simulation`                      |
| agentsview | Read-only borrowed view into live agent buffers         | `crates/mmd-engine/src/sim/agents.rs`, `struct AgentsView`                      |
| statehash  | Deterministic 32-byte digest of sim/runtime state        | `crates/mmd-engine/src/runtime.rs`, `fn state_hash`                             |
| allocguard | Zero-heap-alloc-per-frame counting allocator/guard      | `crates/mmd-engine/src/alloc_guard.rs`, `struct MeasureGuard`                   |
| body radius | Scenario-declared collision radius an agent's body occupies, in Q8 | `crates/mmd-engine/src/scenario.rs`, `fn collision_radius_cells`             |
| contact distance | Twice the body radius; agents within it count as overlapping   | `crates/mmd-engine/src/sim/collision.rs`                                    |
| separation strength | Scenario-declared Q8 weight blending the repulsion sum into the descent vector | `crates/mmd-engine/src/scenario.rs`, `fn separation_strength`  |
| Q8 | Fixed-point encoding where 256 represents 1.0 (one cell)          | `crates/mmd-engine/src/scenario.rs`, `const COLLISION_Q8`                    |
| neighbour bin | Preallocated uniform-grid bucket agents are sorted into each tick | `crates/mmd-engine/src/sim/spatial.rs`, `struct SpatialGrid`                 |
| coincidence tie-break | Deterministic push direction for agents at (near-)identical positions, keyed on the index pair | `crates/mmd-engine/src/sim/collision.rs`, `const SEPARATION_DIR16` |
| collision scene | A scenario version family requiring a nonzero body radius and locked geometry | `crates/mmd-engine/src/scenario.rs`, `const COLLISION_SCENE_V1`             |

## Render

| word      | short description                              | ref in code                                                             |
| --------- | ----------------------------------------------- | ------------------------------------------------------------------------ |
| atlas     | Packed sprite sheet (RGBA) loaded for rendering  | `crates/mmd-engine/src/render/atlas.rs`, `struct AtlasRgba`              |
| drawgroup | Per-atlas batched instance draw data             | `crates/mmd-engine/src/render/instance.rs`, `DrawGroup`                  |
| golden    | Reference image + manifest for pixel-diff render tests | `crates/mmd-engine/src/render/golden.rs`, `struct GoldenManifest`  |
| readback  | GPU frame pixels pulled back to host for comparison | `crates/mmd-engine/src/render/golden.rs`, `fn compare_readback`      |

## Test / lab

| word      | short description                                        | ref in code                                                               |
| --------- | ---------------------------------------------------------- | ---------------------------------------------------------------------------- |
| harness   | Test scaffold wrapping scenario+runtime for behaviour tests | `crates/mmd-engine/src/testkit/mod.rs`, `struct Harness`                   |
| gatescene | Canonical fixture scenario used as the merge-gate scene    | `crates/mmd-engine/src/testkit/mod.rs`, `Harness::gate_scene`              |
| fixture   | Named on-disk scenario/data file loaded by tests            | `crates/mmd-engine/src/testkit/fixtures.rs`, `fn fixture_path`             |
| splitmix  | Deterministic seedable RNG used across tests/lab             | `crates/mmd-engine/src/testkit/rng.rs`, `struct SplitMix64`                |
| bench     | Frozen scale/perf harness (non-gating) with JSON report      | `crates/mmd-engine/src/bench/runner.rs`, `src/bench.rs`                    |
| lane      | One host's pipeline stage in the merge gate (build/test/etc.) | `tools/mmd-lab/src/gate.rs`, `enum LaneId`                                |
| calibrate | Derives/reviews perf baselines from lab samples               | `tools/mmd-lab/src/calibrate.rs`, `struct CalibrationManifest`             |
| baseline  | Reviewed/enabled per-host metric thresholds                    | `tools/mmd-lab/src/calibrate.rs`, `struct Baseline`; `lab/baselines/*.json` |
| pilot     | Synthetic/real report batch validating the lab pipeline itself | `tools/mmd-lab/src/pilot.rs`, `struct PilotReport`                       |
| xtask     | Repo maintenance tasks: bootstrap/shaders/atlases checks        | `xtask/src/main.rs`, `xtask/src/atlases.rs`, `xtask/src/shaders.rs`        |

## CLI / app

| word    | short description                                          | ref in code                                                        |
| ------- | ------------------------------------------------------------ | ---------------------------------------------------------------------- |
| overlay | On-screen debug/stat HUD toggled during `run`                 | `src/overlay.rs`                                                    |
| inject  | CLI-scripted key input / forced alloc for deterministic runs   | `src/main.rs`, `Commands::Run.inject_input`, `Commands::Bench.inject_frame_alloc` |
