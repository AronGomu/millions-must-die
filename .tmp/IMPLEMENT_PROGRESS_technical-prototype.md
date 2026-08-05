# Implement progress: Technical Prototype

- Branch: plan/technical-prototype
- Plan: .tmp/IMPLEMENTATION_PLAN_technical_prototype.md
- Started: 2026-08-02T13:09:11+02:00
- Updated: 2026-08-06T02:40:00+02:00
- Terminal: resumable — 2026-08-06 user-requested stop after T30; resume at T31. Handoff: `.tmp/HANDOFF_technical-prototype.md`

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
| T31 | Render correctness (single host) | pending | — | |
| T32 | App + CLI behaviour | pending | — | |
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
- 2026-08-06 user: stop implementation after current ticket, generate handoff → orchestrator stopped cleanly at T30 (no worker in flight, nothing uncommitted); handoff written to `.tmp/HANDOFF_technical-prototype.md`; resume at T31 (T31/T32 both depend only on T29, T33 last)
