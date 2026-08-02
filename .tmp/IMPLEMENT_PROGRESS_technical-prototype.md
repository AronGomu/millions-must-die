# Implement progress: Technical Prototype

- Branch: plan/technical-prototype
- Plan: .tmp/IMPLEMENTATION_PLAN_technical_prototype.md
- Started: 2026-08-02T13:09:11+02:00
- Updated: 2026-08-02T20:00:00+02:00
- Terminal: hard-stop — remaining tickets blocked_user or blocked_dep only

## Assumptions

- `origin` configured as `git@github.com:AronGomu/millions-must-die.git`; `main` and `plan/technical-prototype` pushed, feature branch tracks origin.
- Parallel off; serial tickets.
- Prep commits on blocked_user tickets keep Linux green; native acceptance still unmet.

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
| T9  | Windows/D3D12 port | blocked_user | 1785ca6 | need Win11 25H2 ref PC + RX6400 |
| T10 | macOS/Metal port | blocked_user | c37a377 | need M4 Mac mini + Xcode Metal |
| T11 | Benchmark + JSON report | done | 6d2047a | |
| T12 | Zero-allocation contract | done | 2943a8e | |
| T13 | Backend golden correctness | blocked_dep | — | needs T9+T10 |
| T14 | Trusted local lab CLI | done | e625451 | |
| T15 | Ubuntu runner contract | done | 979bd14 | |
| T16 | Ubuntu recovery + attestation | blocked_user | 6297174 | need PXE/raw + controller + VLANs |
| T17 | Ubuntu candidate gate | blocked_dep | — | needs T12 T13 T16 |
| T18 | Windows runner contract | done | 4c87635 | |
| T19 | Windows recovery + attestation | blocked_user | 9702913 | need WinPE/FFU lab |
| T20 | Windows candidate gate | blocked_dep | — | needs T12 T13 T19 |
| T21 | macOS runner contract | done | feeab47 | |
| T22 | macOS recovery + attestation | blocked_user | f3fce59 | TODO(user) MDM + Mac lab |
| T23 | macOS candidate gate | blocked_dep | — | needs T12 T13 T22 |
| T24 | Exact-hash 3-host merge gate | blocked_dep | — | needs T17 T20 T23 |
| T25 | Relative calibration engine | done | f802b90 | |
| T26 | 50-run pilot baselines | blocked_dep | — | needs T24 T25 |
| T27 | Final proof + phase close | blocked_dep | — | needs T26 |

## Local setup completed

- GitHub SSH auth works as `AronGomu`; `origin`, pushed branches, and feature tracking configured. Local `.git/info/exclude` hides harness/generated junk without repo changes.
- Trusted `$HOME/.local/bin/mmd-lab` self-check passes; SHA256 `92228215f18b651286cc4e7c8a02ba8a8c1c5fb04ca013ab0b4ad9d746134348`. Installed binary predates T18/T21/T25 commands; install latest only after feature review and promotion to trusted `main`.
- Fake three-agent local-dev validation passes; archive SHA256 `ba9fc09ac5371eea3686545ad19e6592ea680671fffeacf5b3b470651bb18f77`. `dirty_worktree=true` → smoke evidence only.
- Locked bootstrap and shader checks pass. Real DXIL and metallib remain host-gated.
- Locked `mmd-lab` tests pass: 57 unit tests plus integration suites.
- Ubuntu and macOS pass fixtures report ready-for-recovery via workspace binary. Windows PowerShell pass fixture reports ready-for-recovery via ephemeral `nix shell nixpkgs#powershell` plus workspace binary.

## Hard stops — human next actions

1. **T9** — Windows 11 25H2 ref PC (8600G+RX6400): SDL3 prefix-windows, DXC real DXIL, `cargo test --workspace`, `run --agents 50000`, assert D3D12. See `docs/platform/windows-bootstrap.md`.
2. **T10** — M4 Mac mini macOS 15 + Xcode Metal: SDL3 arm64, metallib regen, same matrix. See `docs/platform/macos-bootstrap.md`.
3. **T16** — Ubuntu 24.04 ref + PXE/raw image store + external controller + recovery/candidate VLANs; restore→attest→restore + egress deny logs.
4. **T19** — Windows ref + WinPE/FFU + external power; FFU→attest→FFU + egress deny.
5. **T22** — Select MDM provider + ABM/ADE (`TODO(user)`); M4 + second Mac + USB-C; EACS + DFU drills.

After T9+T10+T16+T19+T22 unblocked → resume implement-plan for T13/T17/T20/T23/T24/T26/T27.

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
