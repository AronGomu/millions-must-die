# Implement progress: Technical Prototype

- Branch: plan/technical-prototype
- Plan: .tmp/IMPLEMENTATION_PLAN_technical_prototype.md
- Started: 2026-08-02T13:09:11+02:00
- Updated: 2026-08-06 (T32 done, T33 next)
- Terminal: active — resumed 2026-08-06 from handoff; T31+T32 done, running T33 (final).

## Assumptions

- `origin` configured as `git@github.com:AronGomu/millions-must-die.git`; `main` and `plan/technical-prototype` pushed, feature branch tracks origin.
- Parallel off; serial tickets.
- Prep commits on blocked_user tickets keep Linux green; native acceptance still unmet.
- 2026-08-05: user directed "ignore all tests for specific hardware" → plan amended with Hardware deferral policy; `[deferred-hw]` items stay unchecked; synthetic baselines never enabled.
- 2026-08-05 (orchestrator auto-decide): plan `TODO(user)` on perf/lab code disposition resolved to **(a) freeze in place** — the plan's own recommended option and the safest (nothing deleted or moved, fully reversible in the optimization phase). No user ping per skill rules. T28 implements freeze-in-place: bench/lab keep compiling and keep their unit tests, but nothing they emit gates anything.

## Status

| ID  | Title | State | SHA | Note |
| --- | ----- | ----- | --- | ---- |
| T1  | Workspace + governance shell | done | 8888c27 | |
| T2  | Versioned scenario contract | done | d246199 | |
| T3  | Deterministic atlas generator | done | bd0db6f | |
| T4  | Shared flow field | done | ba15068 | |
| T5  | SoA movement + recycling | done | 15f421c | |
| T6  | Pinned native deps + shaders | done | 5ecc832 | |
| T7  | Linux SDL3 GPU static slice | done | ee607f8 | |
| T8  | Moving 50k interactive slice | done | 5ada751 | |
| T9  | Windows/D3D12 port | skipped | 1785ca6 | deferred-hw (prep commit kept) |
| T10 | macOS/Metal port | skipped | c37a377 | deferred-hw (prep commit kept) |
| T11 | Benchmark + JSON report | done | 6d2047a | |
| T12 | Zero-allocation contract | done | 2943a8e | |
| T13 | Backend golden correctness | done | 081e50a | Linux golden real (RTX 5060 Ti/Vulkan); Win/mac deferred-hw; absorbed WIP files |
| T14 | Trusted local lab CLI | done | e625451 | |
| T15 | Ubuntu runner contract | done | 979bd14 | |
| T16 | Ubuntu recovery + attestation | skipped | 6297174 | deferred-hw (prep commit kept) |
| T17 | Ubuntu candidate gate | done | d81afa1 | fixture lane verdict Pass; live mode deferred-hw; trusted install refresh pending promotion |
| T18 | Windows runner contract | done | 4c87635 | |
| T19 | Windows recovery + attestation | skipped | 9702913 | deferred-hw (prep commit kept) |
| T20 | Windows candidate gate | done | aeaf7ed | fixture lane Pass; synthetic golden disclosed; live D3D12 deferred-hw |
| T21 | macOS runner contract | done | feeab47 | |
| T22 | macOS recovery + attestation | skipped | f3fce59 | deferred-hw (prep commit kept) |
| T23 | macOS candidate gate | done | 740281c | fixture lane Pass; live Metal/EACS deferred-hw |
| T24 | Exact-hash 3-host merge gate | done | ff6e108 | aggregate fixture gate merge-exact-hash; real lanes deferred-hw; 2 MED follow-ups for real-transport ticket |
| T25 | Relative calibration engine | done | f802b90 | |
| T26 | 50-run pilot baselines | done | f2def9e | synthetic pipeline proven; candidates disabled + provenance-guarded; real 150-run pilot deferred-hw |
| T27 | Final proof + phase close | closed-inconclusive | d37bfe6 | 0 allocs at all counts; 50k medians ~10x inside limits but nmad over limit on a capture-loaded host → no proof frozen; owner stopped reruns |
| T9–T27 (perf/lab) | — | retired (perf deferred) | — | 2026-08-05 #2 amendment: perf + platform-matrix program out of phase-0 scope |
| T28 | Retire perf gating | done | 94a075b | perf gating retired: docs/05-testing.md is the single gate source of truth; bench/lab frozen in place (nothing deleted/moved), only gating authority removed |
| T29 | Deterministic system test harness | done | 1725eae | `mmd_engine::testkit::Harness` is the one seeded headless entry point; cross-process hash proof; 2 tracked fixtures; sim/nav suites migrated with full assertion parity |
| T30 | Simulation + navigation behaviour | done | b70fd12 | 8 new behaviour tests + 2 tracked fixtures; all 12 injected regressions killed their target test; no engine bug surfaced |
| T31 | Render correctness (single host) | done | fa355d7 | 15 tests (11 headless + 4 GPU auto-skip); 25/25 mutations killed; 2 real use-after-free defects found and fixed in the shutdown path |
| T32 | App + CLI behaviour | done | 8ec342c | `tests/cli_contract.rs` 3 → 24 cases + 4 `src/input.rs` unit tests; 32/35 mutants killed (3 survive by construction, bounded in comments); 3 defects found and fixed in the run loop |
| T33 | Functional phase close | pending | — | |

## Local setup completed

- GitHub SSH auth works as `AronGomu`; `origin`, pushed branches, and feature tracking configured. Local `.git/info/exclude` hides harness/generated junk without repo changes.
- Trusted `$HOME/.local/bin/mmd-lab` self-check passes; SHA256 `92228215f18b651286cc4e7c8a02ba8a8c1c5fb04ca013ab0b4ad9d746134348`. Installed binary predates T18/T21/T25 commands; install latest only after feature review and promotion to trusted `main`.
- Fake three-agent local-dev validation passes; archive SHA256 `ba9fc09ac5371eea3686545ad19e6592ea680671fffeacf5b3b470651bb18f77`. `dirty_worktree=true` → smoke evidence only.
- Locked bootstrap and shader checks pass. Real DXIL and metallib remain host-gated.
- Locked `mmd-lab` tests pass: 57 unit tests plus integration suites.
- Ubuntu and macOS pass fixtures report ready-for-recovery via workspace binary. Windows PowerShell pass fixture reports ready-for-recovery via ephemeral `nix shell nixpkgs#powershell` plus workspace binary.

## Deferred hardware backlog (was Hard stops)

Deferred until real hardware available — no longer blocking ticket completion:

1. **T9** — Windows 11 25H2 ref PC (8600G+RX6400): SDL3 prefix-windows, DXC real DXIL, `cargo test --workspace`, `run --agents 50000`, assert D3D12. See `docs/platform/windows-bootstrap.md`.
2. **T10** — M4 Mac mini macOS 15 + Xcode Metal: SDL3 arm64, metallib regen, same matrix. See `docs/platform/macos-bootstrap.md`.
3. **T16** — Ubuntu 24.04 ref + PXE/raw image store + external controller + recovery/candidate VLANs; restore→attest→restore + egress deny logs.
4. **T19** — Windows ref + WinPE/FFU + external power; FFU→attest→FFU + egress deny.
5. **T22** — Select MDM provider + ABM/ADE (`TODO(user)`); M4 + second Mac + USB-C; EACS + DFU drills.

When hardware arrives → run all `[deferred-hw]` plan items; only then full-confidence 3-OS claim.

## Log

- base scaffold 4cf0fbb; branch plan/technical-prototype
- T1–T8 done (Linux prototype path)
- T9 T10 blocked_user (cross-OS native)
- T11 T12 done
- T13 blocked_dep
- T14 T15 done; T16 blocked_user
- T17 blocked_dep
- T18 done; T19 blocked_user; T20 blocked_dep
- T21 done; T22 blocked_user; T23 blocked_dep
- T24 blocked_dep
- T25 done f802b90
- T26 T27 blocked_dep
- orchestrator stop: no unblocked remaining tickets
- 2026-08-05 user: ignore hardware tests → plan amended (Hardware deferral policy); T9/T10/T16/T19/T22 → skipped; T13/T17/T20/T23/T24/T26/T27 → pending (fixture scope)
- 2026-08-05 dirty worktree detected (24 modified files + untracked v2 schema, overlaps ticket paths) → user chose: keep, workers build on top
- 2026-08-05 T13 done 081e50a; WIP absorbed (entangled with ReportManifests/atlas hash); baseline fmt/clippy pre-red in untouched files noted
- 2026-08-05 T17 start
- 2026-08-05 T17 done d81afa1; bounded risk logged: golden HostBinding from candidate evidence until ref-host recapture (deferred-hw)
- 2026-08-05 T20 start
- 2026-08-05 T20 done aeaf7ed
- 2026-08-05 T23 start
- 2026-08-05 T23 done 740281c; residual: pre-existing flake `warmup_allocation_passes` (T12 scope) under parallel load — passes isolated; consider T12 follow-up
- 2026-08-05 T24 start
- 2026-08-05 T24 done ff6e108; MED follow-ups logged: lane outcomes should return HostEvidence (aggregate re-delivers via fake probe); per-lane config wiring duplicated — both for real-transport ticket
- 2026-08-05 T26 start
- 2026-08-05 T26 done f2def9e; provenance enum blocks synthetic enable structurally; flake family note: panic_restores_guard also flaked once under load
- 2026-08-05 T27 start
- 2026-08-05 T27 bench rerun at 82f162c: 0 project Rust allocs at 1k/10k/50k/100k (sample-buffer sizing fix); 50k p95 1.670 / p99 1.981 ms but nmad_p95 0.0653 nmad_p99 0.1388 → inconclusive (OBS capture + compositor live on GPU); 100k quiet (nmad 0.0034)
- 2026-08-05 user: give up on quiet-host rerun → T27 closed honestly at d37bfe6; no release proof frozen; gate open
- 2026-08-05 user: remove all platform/architecture benchmarking from plan; perf → later optimization phase; focus on game-system functional tests → plan amended (Performance and platform-matrix deferral policy #2); T9–T27 retired; T28–T33 added; awaiting owner validation
- 2026-08-05 orchestrator resume (`continue`): perf/lab disposition auto-decided = freeze in place; active chain T28 → T29 → {T30,T31,T32} → T33 starts serial
- 2026-08-05 T28 start
- 2026-08-06 T29 start
- 2026-08-06 T29 done 1725eae. `mmd_engine::testkit::Harness` is the single seeded headless entry point (scenario source + agent count + seed → step N ticks → positions / alive count / spawn+recycle counters / state hash). Clock-free by construction: `step` goes through the new `Runtime::tick_only`, which never samples `Instant` (`render_frame` does, and is documented as the one exception). Cross-process determinism is genuinely proven — the test binary re-executes itself via `current_exe()` and compares the child's printed hash. 4 scenario sources: gate scene / tracked fixture / explicit path / synthetic in-memory grid, so unit-scale tests share the same entry point without losing their surgical assertions. Seed 0 == canonical (bit-identical to `Runtime::load`), nonzero seed redistributes spawn placement via SplitMix64 and provably touches nothing else. New tracked fixtures `assets/scenarios/fixtures/{fixture_small_v1,fixture_corridor_v1}` follow the same `.sha256` contract as the gate scene. Scenario validation gained a `fixture_*` family: v1 keeps every frozen constant AND the exact-20% rule untouched; fixtures relax only geometry/agent counts, still enforcing the 4-atlas/8-dir/4-frame renderer contract, size caps, and all structural rules. Migration preserved every base assertion (both reviewers verified assertion-by-assertion); `arrival_radius_recycles` and `cross_platform_quantized_drift_is_bounded` came out stronger. The 6 pure-geometry `FlowField::build` tests deliberately stay direct — see the plan's migration note. Reviewer follow-ups applied: labelled RNG sub-streams (`rng("camera")`) so phase-1 systems cannot silently share a stream, `Simulation` stores `initial_agent_count` instead of inferring it from the live vector, `Runtime::sim_mut` is testkit-gated out of shipping builds, and `testkit` is genuinely excluded from the release binary. Test-quality review caught a CRITICAL — the new fixture-validation branches had zero negative coverage and `fixture_scenarios_stay_small` asserted committed data rather than enforcement; fixed with 10 negative tests in `scenario_contract.rs`, mutation-verified against 2 bypasses. Accepted//not-fixed: the shipping CLI now accepts a `fixture_*` scenario via `--scenario` (renders small into the fixed 1080p view; bench records the version in `WorkloadIdentity` and is FROZEN by T28, so it was left untouched), and `Runtime` retains ~1.6 MiB of nav data per instance (no consumer holds more than one).
- 2026-08-06 T28 done 94a075b. Retired by name (nothing deleted, nothing moved): 50k frame-time gate (p95 16.67 / p99 25 ms), nmad 0.03 noise bound, scale curve as acceptance artifact, Windows/D3D12 + macOS/Metal lanes, 3-host lab merge gate, calibration/pilot baselines, release proofs. New guard `tests/validation_contract.rs` (`gate_list_has_no_perf_thresholds`, `bench_binary_still_builds`, plus superseded/pointer checks) parses the documented gate list and fails if a timing threshold or a frozen perf tool returns to the required path; mutation-verified against 3 bypasses. `frame_allocation_fails` renamed `alloc_invariant_still_enforced` (T12 plan body annotated, not rewritten) and reframed as a code-health invariant, unchanged in strength. Docs: 05-testing.md is now the single source of truth; README/roadmap/results/CONTRIBUTING point at it; ADR 001/005/006/007 marked superseded-in-part via their own field; docs/lab/{merge-workflow,local-validation}.md banner-retired. `benchmark_policy.rs` + `tools/mmd-lab/tests/*` deliberately untouched — they are the freeze-in-place proof. Known-flaky `warmup_allocation_passes`/`panic_restores_guard` both passed under full parallel load this run; not touched.
- 2026-08-06 T30 start
- 2026-08-06 T30 done b70fd12. Behaviour, not timings: `tests/simulation.rs` gains `agents_reach_destination`, `obstacles_are_never_entered`, `no_agent_is_stuck_against_an_obstacle`, `aggregate_progress_is_monotone`, `no_group_is_starved`, `positions_finite_and_in_bounds`, `alive_count_is_stable`, `determinism_holds_for_50k_agents`; `tests/flow_field.rs` gains `every_reachable_cell_has_a_valid_direction`, `unreachable_region_is_explicit`, `agent_in_an_unreachable_region_is_inert_not_panicking`. Shared assertion helpers live in `tests/common/mod.rs` (`cell_of`, `assert_positions_finite_and_in_bounds`, `agents_in_obstacles`, and `Tracker`, which detects recycles geometrically — a jump no walk can produce — so the sim's counters are checked against an independent observation instead of against themselves). Two new tracked fixtures under the same `.sha256` contract: `fixture_dense_v1` (32x24, 33.3% blocked, 4 west spawn groups, 1-row/2-column corridors) and `fixture_walled_v1` (24x16, two offset walls plus a fully sealed 3x3 chamber). Progress is measured in flow-field integration cost, not Euclidean distance, because an agent rounding a wall legitimately moves away in straight-line terms. Constants read off the current systems, not invented: dense-fixture first arrival tick 230, every spawn group has an arrival by 263, 100% recycled by 300 -> 400-tick budget with a 90% threshold; corridor first arrival 565 -> 700-tick budget for the monotone case; the 50k gate scene needs ~1785 ticks to cover its ~238-cell route, so `alive_count_is_stable` asserts `recycled == 0` at 1000 ticks as a real check rather than a vacuous one. 50k determinism is deliberately bounded at 300 ticks (~2 s) to keep the suite at ~45 s total. Mutation verification: 12 injected regressions, each killed its target test (recycle-no-respawn, recycle-cursor double-increment, obstacle guard removed, population pop, NaN injection, frozen spawn band, wedged agent, zero step length, unreachable/wall sentinel conflation, destination rejected as descent target, process-global counter leaked into positions, stranded agent teleporting). The first mutation pass exposed a genuinely weak test: `obstacles_are_never_entered` SURVIVED removal of the movement walkability guard, because the flow field alone never aims into a wall on that fixture. Fixed by adding a hostile-field phase (every cell forced to point south into the pillar rows) which now kills that mutation. Two clock-derived nondeterminism mutations were discarded as flaky and replaced with a deterministic process-global-counter leak. No engine bug found: 0 obstacle entries, 0 stuck agents, 0 bounds/finiteness violations across all four fixtures and the 50k gate scene. Noted for later (not a defect today): the movement step samples only the *destination* cell, so an agent sitting near a cell corner and taking a diagonal step can in principle land in a different neighbour than the field intended and, if that cell is blocked, hold position forever — the `no_agent_is_stuck_against_an_obstacle` test is the guard that would catch it if a future scene produces one. `tests/harness.rs` fixture loops moved from a hardcoded pair to `ALL_FIXTURES`, so the new fixtures inherit the hash-verification and size-cap coverage.
- 2026-08-06 orchestrator resume (`continue`, from handoff): picked up at T31; chain T31 → T32 → T33 serial
- 2026-08-06 T31 start
- 2026-08-06 T31 done fa355d7. New `crates/mmd-engine/tests/render_correctness.rs`: 11 headless tests (`instance_per_alive_agent`, `instances_carry_agent_position_and_animation_uvs`, `packing_is_a_pure_projection_of_sim_state`, `world_to_clip_transform`, `atlas_manifest_matches_generated_pngs`, `golden_drift_fails`, `golden_drift_reports_the_first_differing_pixel`, `diff_rejects_mismatched_buffers`, `unbound_golden_writes_no_diff`, `no_gpu_skips_cleanly`, `device_creation_failure_classifies_as_unavailable`) plus 4 GPU tests that auto-skip without a device (`golden_frame_matches`, `golden_drift_fails_on_gpu`, `world_to_clip_matches_gpu_raster`, `renderer_smoke_device_resize_shutdown`). Mutation verification: 25 injected regressions, 25 killed; the one initial survivor (single-corner quad shrink) was replaced with a uniform half-scale mutation that kills. **Two real defects found and fixed, not merely tested:** destroying a window still claimed by the GPU device left a dangling swapchain → SIGSEGV on shutdown (`sdl3` 0.18.4 exposes no release, so `GpuContext::release_window` was added — it drains the device first — and paired at every exit in `src/run.rs`, including paths that previously returned through `?` with a live window); and `event_pump()?` sat between claim and release, the same hazard on an error path, hoisted above the claim. Validated with GPU present (`MMD_REQUIRE_GPU=1`, 34 binaries, 0 failures) and GPU absent (`VK_DRIVER_FILES=/nonexistent`, no DISPLAY → GPU cases skip); strict+no-GPU correctly fails. testkit still 0 occurrences in the shipping build. Accepted/not-fixed: goldens stay at `lab/goldens/<family>/` rather than the plan's `assets/goldens/` (deferred-platform placeholder families and the T28-frozen `lab/` layout bind to that path — recorded in the plan); a lavapipe-only host fails rather than skips, because reclassifying would also excuse the macOS never-MoltenVK policy rejection and the two are indistinguishable on a host with neither; helper duplication between `render_correctness.rs` and `gpu_golden.rs` declined as out-of-scope churn; `gpu_golden.rs::host_offscreen_matches_tracked_golden` is now a near-duplicate but stays `#[ignore]` and frozen. Carried to T32: `src/run.rs` has no automated coverage (only the `run --frames 300` smoke) — T32 should add a `run --frames 3` exit-0 case gated on `MMD_REQUIRE_GPU`.
- 2026-08-06 T32 start
- 2026-08-06 T32 done 8ec342c. `tests/cli_contract.rs` extended from 3 to 24 cases (plus 4 unit tests in the new `src/input.rs`): frame budget offscreen + windowed (1/6/9), agent override, pause freeze vs control run, pause-from-frame-1, overlay inertness (exact HUD line count), quit release + no-signal exit, quit-before-frame-1, quit cancels its own frame, multi-entry scripts, unfired-press rejection, env budget (valid/malformed/zero), headless default, missing/corrupt/drifted/invalid scenario, missing sidecar, `--agents 0` and over-cap, `--frames 0`, malformed scripts, exit codes 1/2/3. **Three defects found and fixed in this diff:** the windowed loop checked its budget at the tail so `--frames 1` rendered 2 frames; a parallel first-frame body meant a frame-1 overlay/quit was ignored and the documented contract did not match reality (frame 1 now uses the same body); and the first "state hash unchanged" guard could never fire because `state_hash` digests `tick_index` — replaced with a tick/frame lockstep guard that can. Mutation verification: 35 mutants, 32 killed. Three survive **by construction, not by weak tests**, each bounded in a test comment: (1) removing `ctx.release_window()` while keeping its report — the T31 use-after-free does not reproduce deterministically at CLI level, so the engine-level claim/release contract stays owned by `render_correctness.rs` while this suite owns reaching the release site and exiting by return rather than signal; (2) disabling the tick/frame lockstep guard — redundant defensive code on the *interactive* command that no test drives, and every tested path asserts the same fact externally; (3) `is_device_unavailable()` → `true`, unobservable from the CLI on a GPU host, with both observable directions killed by the two exit-code mutants. Validation green both ways: `MMD_REQUIRE_GPU=1 cargo test --workspace --locked` (34 binaries, 0 failures) and GPU-absent `VK_DRIVER_FILES=/nonexistent` with no `DISPLAY` (34 binaries, 0 failures, GPU cases skip); fmt, clippy, `nix flake check`, xtask checks, `cargo run -- run --agents 50000 --frames 300` → exit 0 `mode=window tick=300 frames=300`; `cargo tree -e features | grep -c testkit` → 0. New deps on the app package: `hex` (normal — replaces a hand-rolled encoder, already present transitively) and `sha2` (**dev-only**, lets tests write scenarios with honest sidecar hashes). `docs/05-testing.md` gained an "App and CLI lifecycle" section (stdout contract + exit-code table + injection) for T33 to fold into the phase close. Accepted/not-fixed: the `Io` message prints the scenario path once via the loader rather than the wrapper (deliberate); `overlay_lines()` counts one of the two HUD lines per frame (documented).
- 2026-08-06 user: stop implementation after current ticket, generate handoff → orchestrator stopped cleanly at T30 (no worker in flight, nothing uncommitted); handoff written to `.tmp/HANDOFF_technical-prototype.md`; resume at T31 (T31/T32 both depend only on T29, T33 last)
