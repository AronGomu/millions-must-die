# Technical Prototype — Functional Close (Phase 0)

**Status: closed on functional evidence, 2026-08-06.**

Phase 0 asked whether the game's systems *work*. They do: every entry in the
[system → test map](#system--test-map) below is backed by a named automated
test, and all but the two labelled `(GPU-only)` run on every merge. Phase 0 did
**not** ask how fast they run, and this document claims nothing about that.

The prose outside that map — the known gaps, the observation counts, the
phase-1 backlog — is a record of judgement, not of test output. Where a
statement is backed by a test, this page names it.

What gates a merge is defined in one place and one place only:
[testing strategy](05-testing.md). This document explains what that gate
proves; it does not add to it.

## What "closed" means here

| Claim | Status |
| --- | --- |
| Every game system in scope has at least one behavioural test | proven — see the map below |
| The 50k-agent scene starts, ticks and exits cleanly | proven — `cargo run -- run --agents 50000 --frames 300` is on the required gate |
| The simulation is deterministic on one host and one binary | proven — hash-equality tests, with the narrowing recorded under Known gaps |
| The development host renders the expected frame | proven — committed golden, compared exactly, on Linux/Vulkan only, on the `MMD_REQUIRE_GPU=1` run recorded below |
| The app and CLI honour their documented contract | proven — the built binary is driven as a subprocess |
| Performance | **unmeasured.** Retired to a later optimization phase; nothing here is a speed claim |
| Any other platform | **unverified.** No cross-platform evidence exists |

## System → test map

The authoritative copy of this map is the `SCOPE_SYSTEMS` list in
`tests/validation_contract.rs`. The test `every_system_has_a_test` resolves that
list against a source scan of the test tree *and* against this page, so a test
that is renamed, moved or deleted fails the gate instead of quietly shrinking
the claim. This table is therefore checked, not merely written.

| System | Proven by | Where |
| --- | --- | --- |
| Simulation — movement, obstacles, recycling | `tick_moves_eight_cells_per_second`, `blocked_step_holds_position`, `obstacles_are_never_entered`, `no_agent_is_stuck_against_an_obstacle`, `arrival_radius_recycles`, `population_stays_50000`, `determinism_holds_for_50k_agents` | `crates/mmd-engine/tests/simulation.rs` |
| Collision — agent separation and neighbour bins | `spatial_bins_hold_every_agent_exactly_once`, `spatial_bucket_order_is_ascending_agent_index`, `spatial_clamps_positions_outside_the_world`, `separation_of_a_pair_is_equal_and_opposite`, `separation_is_capped_at_eight_neighbours`, `coincident_agents_separate_on_the_first_tick`, `separation_keeps_the_step_length`, `a_released_stack_spreads_apart`, `a_bodyless_scenario_walks_the_flow_only_path`, `sprite_scene_pulls_agents_out_of_deep_overlap`, `collision_scene_agents_never_enter_an_obstacle` | `crates/mmd-engine/tests/separation.rs` |
| Navigation — flow field | `destination_cost_is_zero`, `obstacles_unreachable`, `diagonal_cannot_cut_corner`, `vectors_descend`, `every_reachable_cell_has_a_valid_direction`, `agent_in_an_unreachable_region_is_inert_not_panicking` | `crates/mmd-engine/tests/flow_field.rs` |
| Scenario loading and hash contract | `loads_v1_scene`, `rejects_wrong_hash`, `rejects_unreachable_spawn`, `v1_geometry_stays_frozen_against_the_fixture_relaxation` | `crates/mmd-engine/tests/scenario_contract.rs` |
| Deterministic test harness | `same_seed_same_state_hash`, `different_seed_differs`, `tick_count_is_exact`, `no_wall_clock_dependence`, `fixture_scenarios_are_hash_verified` | `crates/mmd-engine/tests/harness.rs` |
| Runtime frame loop | `frame_ticks_once`, `pause_keeps_checksum`, `builds_50000_instances`, `partitions_four_groups`, `input_actions_are_stable` | `crates/mmd-engine/tests/runtime_frame.rs` |
| Render correctness — instance data, projection, whole frame | `instance_per_alive_agent`, `instances_carry_agent_position_and_animation_uvs`, `packing_is_a_pure_projection_of_sim_state`, `world_to_clip_transform`, `world_to_clip_matches_gpu_raster`, `golden_frame_matches`, `no_gpu_skips_cleanly` | `crates/mmd-engine/tests/render_correctness.rs` |
| GPU smoke and tracked asset hashes | `instance_layout_is_stable`, `tracked_atlas_hashes_are_enforced`, `readback_is_1920x1080` (GPU-only), `four_groups_drawn` (GPU-only) | `crates/mmd-engine/tests/gpu_smoke.rs` |
| Golden-image comparator | `exact_image_passes`, `delta_above_tolerance_fails`, `backend_cannot_use_other_golden`, `placeholder_golden_cannot_pass`, `tampered_golden_image_fails_hash` | `crates/mmd-engine/tests/gpu_golden.rs` |
| Allocation invariant | `alloc_invariant_still_enforced`, `warmup_allocation_passes`, `guard_resets_between_trials`, `panic_restores_guard`, `foreign_thread_allocations_do_not_leak_into_a_measure_scope`, `spatial_rebuild_allocates_nothing`, `a_collision_tick_allocates_nothing` | `crates/mmd-engine/tests/frame_allocations.rs` |
| App and CLI lifecycle | `run_exits_after_n_frames`, `windowed_run_honours_the_frame_budget`, `pause_freezes_state`, `overlay_toggle_is_inert`, `quit_exits_clean_and_releases_window`, `injection_that_never_fires_is_an_error`, `missing_scenario_fails_clean` | `tests/cli_contract.rs` |
| Merge-gate contract | `gate_list_has_no_perf_thresholds`, `gate_docs_state_perf_gating_is_retired`, `results_doc_is_superseded_history_not_a_claim`, `bench_binary_still_builds`, `every_system_has_a_test`, `no_perf_claim_in_docs`, `perf_claim_scanner_catches_what_it_is_meant_to` | `tests/validation_contract.rs` |

The table names representative tests per system, not the whole suite — the
suite is larger. `crates/mmd-engine/tests/benchmark_policy.rs` is deliberately
absent: it unit-tests the frozen measurement tooling, which is not a game
system and gates nothing.

**`(GPU-only)` means `#[ignore]`d** — the test needs a host GPU and SDL3, and a
plain `cargo test` does not run it. Those two are labelled rather than hidden,
and `every_system_has_a_test` enforces the labelling in both directions: an
`#[ignore]`d test that is mapped without the label fails, and so does a label
on a test that is not actually ignored. Every system also keeps at least one
test that *does* run on the plain gate.

### Evidence behind the render rows

The GPU-bound tests obtain their device through a helper that **skips** when the
host has no GPU, and `cargo test` prints the same `ok` either way — so a green
gate on a headless host is not evidence that the renderer was verified. The
close therefore rests on one run with the skip disabled (plan decision D10):

```sh
MMD_REQUIRE_GPU=1 cargo test --workspace --locked
```

| Field | Value |
| --- | --- |
| Date | 2026-08-06 |
| Host | Linux, Vulkan, NVIDIA GeForce RTX 5060 Ti |
| Result | 34 test binaries, 0 failures, no skips |
| Repeats | 3 further runs under 12-way parallel CPU load, all green |

Read every render row below as "proven on that host, on that run". A merge gate
run without `MMD_REQUIRE_GPU=1` on a machine with no GPU proves the rest of the
suite and skips those.

### How these tests were validated

Every test above was mutation-verified when it was written: a regression was
injected, the test was confirmed to fail, the injection was reverted, and the
test was confirmed to pass. A test that only asserts "no panic" was not
accepted as proof. This is a record of the process that produced the suite —
it is not itself enforced by a test, and re-running the gate does not re-check
it.

| Suite | Mutants | Killed | Survivors |
| --- | --- | --- | --- |
| Simulation and navigation | 12 | 12 | 0 |
| Render correctness | 25 | 25 | 0 |
| App and CLI lifecycle | 35 | 32 | 3, each unobservable by construction — see gap 7 |
| Merge-gate contract (this close) | 28 | 28 | 0 |
| Allocation invariant (this close) | 2 | 2 | 0 |

## Known gaps — all non-blocking

These are recorded because they are true, not because they are outstanding
work for phase 0. **None of them blocks the close.** Each is either out of
phase-0 scope by decision, or a bounded narrowness that the tests state
honestly rather than paper over.

### 1. Performance is entirely unmeasured

There is no speed claim anywhere in this project's live documentation. Frame
budget gating, noise bounds, and scale curves are retired to a later
optimization phase on the finished game; the code that produced them is frozen
in place and gates nothing. Measurements taken before the retirement are kept
as history and claim nothing:
[technical prototype results](technical-prototype-results.md) (superseded).

### 2. No cross-platform verification

Golden images are host-scoped: `lab/goldens/linux-vulkan/` proves that this
development backend, on the adapter recorded in its manifest (Linux, Vulkan,
NVIDIA RTX 5060 Ti), renders the expected frame. The `windows-d3d12` and
`macos-metal` families are `placeholder-deferred-hw` and can never pass a
comparison. The deferred-hardware backlog stays open and out of phase-0 scope;
no hardware is provisioned.

A host whose only Vulkan ICD is a software rasterizer fails rather than skips.
That is deliberate: it is indistinguishable from the macOS "never MoltenVK"
policy rejection, and reclassifying one would silently excuse the other.

### 3. Determinism is same-host, same-binary

`f32` results are not bit-identical across compilers, optimization levels or
architectures. Every hash-equality claim in the map above means "this binary on
this host reproduces itself", which is exactly what the tests assert. It does
not mean two different builds agree.

### 4. Diagonal corner case in the movement step

The movement step samples only the destination cell, so a corner-adjacent
diagonal step could in principle land in a different neighbour than the flow
field intended and wedge an agent permanently.
`no_agent_is_stuck_against_an_obstacle` is the guard, and **0 occurrences have
been observed**. Recorded, not fixed: no engine change in phase 0.

### 5. The shipping CLI accepts a `fixture_*` scenario

`--scenario` will load a test fixture and draw a small world into the fixed
1080p view. Cosmetic, and left alone on purpose — the obvious fix sits inside
the frozen `bench::runner`, which phase 0 does not touch.

### 6. `Runtime` duplicates navigation data

`Runtime` retains roughly 1.6 MiB of navigation data that duplicates what
`Simulation` already copies. No consumer holds more than one `Runtime`, so it
costs nothing today; `Arc<FlowField>` is the fix if a second holder ever
appears.

### 7. Three CLI mutants survive by construction

Mutation testing of the app and CLI suite killed 32 of 35 injected regressions.
The three survivors are unobservable from the CLI, not weakly tested, and each
is bounded by a comment at the test that would otherwise own it:

1. Removing `ctx.release_window()` while keeping its report — the underlying
   use-after-free does not reproduce deterministically at CLI level, so the
   claim/release contract stays owned by `render_correctness.rs`.
2. Disabling the tick/frame lockstep guard — defensive code on the interactive
   command that no test drives.
3. Forcing `is_device_unavailable()` to `true` — unobservable from the CLI on a
   host that has a GPU; both observable directions are killed by the two
   exit-code mutants.

### 8. The allocation invariant counts the measuring thread only

`MeasureGuard` arms allocation counting on the thread that entered it, and the
counting allocator records an allocation only from a thread inside its own
scope. This closed a real cross-talk defect — the process-wide flag let the
test harness's own bookkeeping land inside an open scope, failing the
zero-allocation assertions a few runs in a hundred under parallel load. The
guard is deliberately `!Send`, so a scope cannot be dropped on a thread that
never entered it. The counter *itself* is still one process-wide number, so two
threads measuring simultaneously would share it — nothing does that today, and
the tests that use it serialize on a mutex.

Nothing in this project hands frame work to another thread, so no coverage is
lost today. Introducing a worker thread on the frame path means this guard must
be revisited before it can still claim zero allocations per frame.

### 9. Frozen measurement code rots until re-validated

`crates/mmd-engine/src/bench/` and `tools/mmd-lab/` still compile and their
unit tests still run on every merge — which is what proves they still compile —
but nothing they emit decides whether a change may merge. `lab/` holds no Rust
at all; it is fixtures, manifests and the tracked goldens the render tests read.
All of it must be re-validated before the optimization phase reuses it.

### 10. Collision is steering, not resolution

Agents separate by *steering*: each sums a repulsion vector from the
neighbours overlapping its body, that sum is added to the flow-field descent
vector, and the agent walks the blended heading at its unchanged speed. Nothing
in the tick forbids an overlap, and no test claims one is impossible — the
proven claims are that a coincident pair splits, that a released stack spreads,
and that deep overlap on the sprite-scale scene collapses by an order of
magnitude within 300 ticks. Under crowd pressure, and especially in the jam that
forms at the destination cell, bodies do interpenetrate. That is the accepted
behaviour of the chosen model, recorded in
[ADR 009](ADR/009_ADR_agent_separation_and_collision.md).

Two further narrowings live here rather than in the code. Each agent
accumulates at most eight neighbours per tick, so a very deep stack is pushed
apart over several ticks instead of one. And when the blended step would leave
the walkable area, the agent falls back to the pure descent step — separation
may never wedge an agent the field alone could have moved, which means a wall
can win against a crowd and let bodies compress against it.

## Phase-1 backlog

Carried forward, in no committed order:

- Re-open measurement work as a dedicated optimization phase on the finished
  game, starting by re-validating the frozen tooling above.
- Provision the deferred hardware (Windows/D3D12 and macOS/Metal reference
  hosts) and give the placeholder golden families real evidence.
- Decide the diagonal corner case on its merits: fix the movement step, or keep
  the guard and document it as intended behaviour.
- Reject `fixture_*` scenarios from the shipping `--scenario` path once
  `bench::runner` is unfrozen.
- Revisit `Arc<FlowField>` when a second `Runtime` holder exists.
- Revisit the allocation guard's thread scope if frame work is ever handed to a
  worker thread.

## Related

- [Testing strategy](05-testing.md) — the single source of truth for the merge
  gate.
- [Technical prototype results](technical-prototype-results.md) — superseded
  measurements, retained as history.
