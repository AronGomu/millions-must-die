# Implement progress: Technical Prototype

- Branch: plan/technical-prototype
- Plan: .tmp/IMPLEMENTATION_PLAN_technical_prototype.md
- Updated: 2026-08-02T18:20:00+02:00

## Status

| ID  | Title | State   | SHA | Note |
| --- | ----- | ------- | --- | ---- |
| T1–T8 T11 T12 T14 T15 | (see log) | done | — | |
| T9  | Windows/D3D12 port | blocked_user | 1785ca6 | need Win11 ref PC |
| T10 | macOS/Metal port | blocked_user | c37a377 | need M4 Mac |
| T11 | Benchmark + JSON report | done | 6d2047a | |
| T12 | Zero-allocation contract | done | 2943a8e | |
| T13 | Backend golden correctness | blocked_dep | — | needs T9+T10 |
| T14 | Trusted local lab CLI | done | e625451 | |
| T15 | Ubuntu runner contract | done | 979bd14 | |
| T16 | Ubuntu recovery + attestation | blocked_user | 6297174 | protocol+docs landed; need physical lab |
| T17 | Ubuntu candidate gate | pending | — | needs T13 T16 |
| T18 | Windows runner contract | pending | — | |
| T19 | Windows recovery + attestation | pending | — | physical |
| T20 | Windows candidate gate | pending | — | |
| T21 | macOS runner contract | pending | — | |
| T22 | macOS recovery + attestation | pending | — | TODO(user) MDM |
| T23 | macOS candidate gate | pending | — | |
| T24 | Exact-hash 3-host merge gate | pending | — | |
| T25 | Relative calibration engine | pending | — | |
| T26 | 50-run pilot baselines | pending | — | physical |
| T27 | Final proof + phase close | pending | — | |

## Log

- T15 done 979bd14
- T16 blocked_user: recovery protocol SM + recover.sh skeleton + docs/lab/ubuntu-runner.md + unit tests; no Ubuntu ref PC / PXE / controller / VLANs on developer host
