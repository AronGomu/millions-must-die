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
| separation phase | Cadence spreading the neighbour scan and grid rebuild over `separation_phases` ticks; agent `i` recomputes when `i % phases == tick % phases` | `crates/mmd-engine/src/scenario.rs`, `separation_phases`                    |
| push priority / mass class | Per-agent byte scaling a neighbour's push by `mass[j] / mass[i]`, so heavy agents push harder and are pushed less | `crates/mmd-engine/src/sim/agents.rs`, `mass`, `inv_mass`                   |
| bin stamp | Per-bin rebuild counter that makes a stale bin read as empty without clearing it | `crates/mmd-engine/src/sim/spatial.rs`, `SpatialGrid::stamp`                |
| row window | The 3×3 neighbour window walked as three contiguous per-row runs instead of nine per-bin slices | `crates/mmd-engine/src/sim/spatial.rs`, `SpatialGrid::agents_in_bin_row`    |
| separation pool | Persistent worker pool running the separation pass across `separation_threads` participants, spawned once at construction | `crates/mmd-engine/src/sim/pool.rs`, `struct SeparationPool`                |
| identity tuning | The default value (`1`) of a scenario knob, at which the tuned code path degenerates to bit-identical behaviour with the untuned engine | `crates/mmd-engine/src/scenario.rs`, `separation_phases`/`mass_class_count`/`separation_threads` |

## RTS (phase 1)

| word      | short description                                       | ref in code                                                                 |
| --------- | ------------------------------------------------------- | ---------------------------------------------------------------------------- |
| entity    | One RTS unit, building or node in the preallocated store | `crates/mmd-engine/src/rts/entity.rs`, `struct EntityStore`                  |
| order     | Per-unit task: move, gather (3 phases), or build         | `crates/mmd-engine/src/rts/orders.rs`, `enum Order`                          |
| fieldpool | 8 flow fields keyed by destination cell, LRU-evicted     | `crates/mmd-engine/src/nav/field_pool.rs`, `struct FieldPool`                |
| dropoff   | Building a loaded worker banks its cargo at              | `crates/mmd-engine/src/rts/entity.rs`, `fn is_drop_off`                      |
| footprint | Square of cells a building occupies on the grid          | `crates/mmd-engine/src/rts/entity.rs`, `BuildingKind::footprint_cells`       |
| ghost     | Pending placement preview following the cursor cell      | `crates/mmd-engine/src/rts/build.rs`, `enum Placement`                       |
| site      | Unfinished building: walkable, advances while attended   | `crates/mmd-engine/src/rts/world.rs`, `fn confirm_placement`                 |
| supply    | Population counter: cap granted, usage recomputed        | `crates/mmd-engine/src/rts/economy.rs`, `struct Supply`                      |
| rally     | Cell a produced unit walks to on spawn                   | `crates/mmd-engine/src/rts/world.rs`, `fn set_rally`                         |
| scenepass | One frame's three layers handed to the renderer          | `crates/mmd-engine/src/render/renderer.rs`, `struct ScenePass`               |
| uilayer   | Depth-off *textured* groups: HUD, ghost, flags, glyphs   | `crates/mmd-engine/src/render/renderer.rs`, `ScenePass::ui`                  |

## RTS (phase 1.1)

| word        | short description                                            | ref in code                                                                  |
| ----------- | ------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| pickshape   | Clickable geometry of an entity: sprite quad ∪ body circle     | `crates/mmd-engine/src/rts/selection.rs`, `fn unit_pick_contains`             |
| body        | Hard 3-cell collision circle of an RTS unit (never the horde)  | `crates/mmd-engine/src/rts/entity.rs`, `fn body_radius_cells`                 |
| staticnav   | Body-inflated blocked-centre mask + continuous sweeps          | `crates/mmd-engine/src/rts/static_nav.rs`, `struct StaticNav`                 |
| formation   | Per-member deterministic lattice slot around one group anchor  | `crates/mmd-engine/src/rts/formation.rs`, `struct FormationGoal`              |
| frontier    | Projected-map camera bound, inset by half the logical view     | `crates/mmd-engine/src/render/camera.rs`, `struct CameraFrontier`             |
| canvas      | Fixed 1920×1080 logical surface mapped to a centred 16:9 rect  | `crates/mmd-engine/src/render/viewport.rs`, `struct DisplayViewport`          |
| commandcard | Context-driven 3×3 action grid on the right of the HUD         | `crates/mmd-engine/src/rts/hud.rs`, `fn command_slots`                        |
| minimap     | Isometric map diamond plus the projected camera polygon        | `crates/mmd-engine/src/rts/minimap.rs`, `struct MinimapProjection`            |
| windowmode  | Borderless-desktop / exclusive / windowed presentation choice  | `src/rts_settings.rs`, `enum WindowMode`                                     |
| audiosink   | Where semantic audio events go: SDL device, buffer, or fake    | `src/rts_feedback.rs`, `trait AudioSink`                                     |
| audiobus    | Music / Voice / SFX gain lane under the master scalar          | `src/rts_feedback.rs`, `enum AudioBus`                                       |

## RTS (feedback polish)

| word             | short description                                              | ref in code                                                                  |
| ---------------- | -------------------------------------------------------------- | ------------------------------------------------------------------------------ |
| uicontrol        | Discrete control with a stable id and a framed visual state      | `crates/mmd-engine/src/rts/hud.rs`, `enum ControlVisualState`                 |
| worldgrid        | Full-map isometric lattice of texture-free line instances        | `crates/mmd-engine/src/rts/pack.rs`, `FramePackOptions::show_grid`            |
| placementassist  | Bounded search snapping a blocked ghost to the nearest legal min corner | `crates/mmd-engine/src/rts/build.rs`, `fn placement_candidate`         |
| gathertransition | Bounded exit a formerly exempt gather pair walks before going hard again | `crates/mmd-engine/src/rts/collision.rs`, `struct GatherCollisionState` |

## RTS (combat)

| word         | short description                                                      | ref in code                                                              |
| ------------ | ---------------------------------------------------------------------- | ------------------------------------------------------------------------ |
| ghoul        | Melee enemy unit: 30 HP, damage 5, range 8, speed 18 cells/s            | `crates/mmd-engine/src/rts/entity.rs`, `UnitKind::Ghoul`                  |
| turret       | Static defense building, auto-fires when finished, 75 crystal, no supply | `crates/mmd-engine/src/rts/entity.rs`, `BuildingKind::Turret`           |
| attackmove   | Move order that halts to fight anything met en route                   | `crates/mmd-engine/src/rts/orders.rs`, `Order::AttackMove`               |
| autoacquire  | Idle or attack-moving armed unit fires at nearest target in range      | `crates/mmd-engine/src/rts/world.rs`, `fn nearest_hostile_in_range`      |
| wave         | Scenario-timed enemy spawn batch at one spawn point                    | `crates/mmd-engine/src/scenario.rs`, `struct WaveSpec`                    |
| spawnpoint   | Map cell a wave's ghouls appear around                                 | `crates/mmd-engine/src/scenario.rs`, `EnemySpec::spawn_points`            |
| objective    | The single approach cell the whole enemy faction marches at            | `crates/mmd-engine/src/rts/world.rs`, `fn recompute_enemy_objective`      |
| owner        | Faction byte: player 0, enemy 1, neutral 255                           | `crates/mmd-engine/src/rts/entity.rs`, `OWNER_ENEMY`                      |
| combattokens | Exit-line combat outcome: kills, losses, enemies_spawned, first_combat_tick, hq_alive | `src/rts_run.rs`                                          |

## RTS (feedback round 2)

| word        | short description                                                     | ref in code                                                        |
| ----------- | --------------------------------------------------------------------- | ------------------------------------------------------------------ |
| buildsquare | Visible placement lattice, 8 cells; units still move in true cells     | `crates/mmd-engine/src/scenario.rs`, `BUILD_SQUARE_CELLS`          |
| snap        | Floor a ghost's min corner to a build-square boundary                  | `crates/mmd-engine/src/rts/build.rs`, `fn snap_to_build_square`     |
| stalledsite | Site that can never evacuate, cancelled and refunded after 180 ticks   | `crates/mmd-engine/src/rts/build.rs`, `STALLED_SITE_TICKS`         |
| orderstatus | Card line naming what a selected unit is doing, from live order state  | `crates/mmd-engine/src/rts/hud.rs`, `fn order_status_label`         |
| targetring  | Thin ring on whatever a selected unit's order points at                | `crates/mmd-engine/src/rts/pack.rs`, `TARGET_RING_INNER`            |
| movemarker  | Self-expiring flag at a ground order's destination                     | `crates/mmd-engine/src/rts/pack.rs`, `Prop::MoveMarker`             |
| follow      | Order that tracks a moving friendly target at interaction reach        | `crates/mmd-engine/src/rts/orders.rs`, `Order::Follow`              |
| rallytarget | A rally point that is either a cell or an entity                       | `crates/mmd-engine/src/rts/production.rs`, `enum RallyTarget`       |
| sandbox     | Untimed manual scene: prebuilt base, soldiers, 30 waves, off the gate  | `assets/scenarios/rts_sandbox_v1.ron`                              |

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
