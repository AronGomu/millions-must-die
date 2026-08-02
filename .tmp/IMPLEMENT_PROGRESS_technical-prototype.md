# Implement progress: Technical Prototype

- Branch: plan/technical-prototype
- Plan: .tmp/IMPLEMENTATION_PLAN_technical_prototype.md
- Started: 2026-08-02T13:09:11+02:00
- Updated: 2026-08-02T13:30:00+02:00

## Assumptions

- No git remote → local commits only until origin exists. Push deferred not hard-fail ticket if commit OK.
- Base scaffold commit `4cf0fbb` on main + feature branch.
- Ship mode: balanced default; production for auth/pay/migrate/webhook/jobs/multi-subsystem.
- Parallel off; serial tickets despite flowchart siblings.

## Status

| ID  | Title | State   | SHA | Note |
| --- | ----- | ------- | --- | ---- |
| T1  | Workspace + governance shell | done | 8888c27 | push_skipped no origin |
| T2  | Versioned scenario contract | done | 1457f9b | push_skipped no origin |
| T3  | Deterministic atlas generator | pending | — | |
| T4  | Shared flow field | pending | — | |
| T5  | SoA movement + recycling | pending | — | |
| T6  | Pinned native deps + shaders | pending | — | |
| T7  | Linux SDL3 GPU static slice | pending | — | |
| T8  | Moving 50k interactive slice | pending | — | |
| T9  | Windows/D3D12 port | pending | — | needs Windows host |
| T10 | macOS/Metal port | pending | — | needs M4 host |
| T11 | Benchmark + JSON report | pending | — | |
| T12 | Zero-allocation contract | pending | — | |
| T13 | Backend golden correctness | pending | — | needs T9/T10 |
| T14 | Trusted local lab CLI | pending | — | |
| T15 | Ubuntu runner contract | pending | — | |
| T16 | Ubuntu recovery + attestation | pending | — | physical host |
| T17 | Ubuntu candidate gate | pending | — | physical host |
| T18 | Windows runner contract | pending | — | |
| T19 | Windows recovery + attestation | pending | — | physical host |
| T20 | Windows candidate gate | pending | — | physical host |
| T21 | macOS runner contract | pending | — | |
| T22 | macOS recovery + attestation | pending | — | TODO(user) MDM |
| T23 | macOS candidate gate | pending | — | |
| T24 | Exact-hash 3-host merge gate | pending | — | |
| T25 | Relative calibration engine | pending | — | |
| T26 | 50-run pilot baselines | pending | — | physical lab |
| T27 | Final proof + phase close | pending | — | |

## Log

- 2026-08-02T13:09:11+02:00 base scaffold `4cf0fbb`; branch `plan/technical-prototype`; plan Impl boxes added
- 2026-08-02T13:09:11+02:00 T1 start
- 2026-08-02T13:25:00+02:00 T1 done 8888c27 ship=locally-verified push_skipped
- 2026-08-02T13:25:00+02:00 T2 start
- 2026-08-02T13:30:00+02:00 T2 green scenario_contract 4/4; ship locally-verified
- 2026-08-02T13:31:00+02:00 T2 done 1457f9b push_skipped no origin
