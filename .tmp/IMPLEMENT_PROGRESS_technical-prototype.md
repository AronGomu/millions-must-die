# Implement progress: Technical Prototype

- Branch: plan/technical-prototype
- Plan: .tmp/IMPLEMENTATION_PLAN_technical_prototype.md
- Updated: 2026-08-02T20:00:00+02:00

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
| T9  | Windows/D3D12 port | blocked_user | 1785ca6 | Win11 ref PC |
| T10 | macOS/Metal port | blocked_user | c37a377 | M4 Mac |
| T11 | Benchmark + JSON report | done | 6d2047a | |
| T12 | Zero-allocation contract | done | 2943a8e | |
| T13 | Backend golden correctness | blocked_dep | — | T9+T10 |
| T14 | Trusted local lab CLI | done | e625451 | |
| T15 | Ubuntu runner contract | done | 979bd14 | |
| T16 | Ubuntu recovery + attestation | blocked_user | 6297174 | physical Ubuntu |
| T17 | Ubuntu candidate gate | blocked_dep | — | T13 T16 |
| T18 | Windows runner contract | done | 4c87635 | |
| T19 | Windows recovery + attestation | blocked_user | 9702913 | physical Windows |
| T20 | Windows candidate gate | blocked_dep | — | T13 T19 |
| T21 | macOS runner contract | done | feeab47 | |
| T22 | macOS recovery + attestation | blocked_user | f3fce59 | TODO(user) MDM + Mac lab |
| T23 | macOS candidate gate | blocked_dep | — | T13 T22 |
| T24 | Exact-hash 3-host merge gate | blocked_dep | — | T17 T20 T23 |
| T25 | Relative calibration engine | done | f802b90 | |
| T26 | 50-run pilot baselines | blocked_dep | — | T24 T25 |
| T27 | Final proof + phase close | blocked_dep | — | T26 |

## Log

- T22 blocked_user f3fce59
- T25 done f802b90
