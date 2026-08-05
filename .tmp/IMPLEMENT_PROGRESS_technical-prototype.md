# Implement progress: Technical Prototype

- Branch: plan/technical-prototype
- Plan: .tmp/IMPLEMENTATION_PLAN_technical_prototype.md
- Started: 2026-08-02T13:09:11+02:00
- Updated: 2026-08-05T00:00:00+02:00
- Terminal: resumable — 2026-08-05 Hardware deferral policy (user-directed) skips hw-only tickets; dependants unblocked for fixture/synthetic scope

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
| T28 | Retire perf gating | pending | — | |
| T29 | Deterministic system test harness | pending | — | |
| T30 | Simulation + navigation behaviour | pending | — | |
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
