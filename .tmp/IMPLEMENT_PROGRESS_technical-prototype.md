# Implement progress: Technical Prototype

- Branch: plan/technical-prototype
- Plan: .tmp/IMPLEMENTATION_PLAN_technical_prototype.md
- Started: 2026-08-02T13:09:11+02:00
- Updated: 2026-08-02T14:43:31+02:00

## Assumptions

- No git remote → local commits only until origin exists.
- Parallel off; serial tickets.

## Status

| ID  | Title | State   | SHA | Note |
| --- | ----- | ------- | --- | ---- |
| T1  | Workspace + governance shell | done | 8888c27 | |
| T2  | Versioned scenario contract | done | d246199 | |
| T3  | Deterministic atlas generator | done | bd0db6f | |
| T4  | Shared flow field | done | ba15068 | |
| T5  | SoA movement + recycling | done | 15f421c | |
| T6  | Pinned native deps + shaders | done | 5ecc832 | |
| T7  | Linux SDL3 GPU static slice | done | ee607f8 | |
| T8  | Moving 50k interactive slice | done | 5ada751 | |
| T9  | Windows/D3D12 port | blocked_user | 1785ca6 | need Win11 ref PC |
| T10 | macOS/Metal port | blocked_user | c37a377 | need M4 Mac |
| T11 | Benchmark + JSON report | done | cf22e70 | |
| T12 | Zero-allocation contract | pending | — | |
| T13 | Backend golden correctness | pending | — | blocked_dep T9/T10 |
| T14 | Trusted local lab CLI | pending | — | |
| T15 | Ubuntu runner contract | pending | — | |
| T16 | Ubuntu recovery + attestation | pending | — | physical |
| T17 | Ubuntu candidate gate | pending | — | physical |
| T18 | Windows runner contract | pending | — | |
| T19 | Windows recovery + attestation | pending | — | physical |
| T20 | Windows candidate gate | pending | — | physical |
| T21 | macOS runner contract | pending | — | |
| T22 | macOS recovery + attestation | pending | — | TODO(user) MDM |
| T23 | macOS candidate gate | pending | — | |
| T24 | Exact-hash 3-host merge gate | pending | — | |
| T25 | Relative calibration engine | pending | — | |
| T26 | 50-run pilot baselines | pending | — | physical |
| T27 | Final proof + phase close | pending | — | |

## Log

- T1–T8 done
- T9 blocked_user need Windows
- T10 blocked_user need M4
- T11 start
- T11 done: bench scale curve + schema + 2-frame queue; release short smoke pass
