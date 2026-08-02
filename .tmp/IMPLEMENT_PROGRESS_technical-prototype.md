# Implement progress: Technical Prototype

- Branch: plan/technical-prototype
- Plan: .tmp/IMPLEMENTATION_PLAN_technical_prototype.md
- Started: 2026-08-02T13:09:11+02:00
- Updated: 2026-08-02T15:15:00+02:00

## Assumptions

- No git remote → local commits only until origin exists.
- Parallel off; serial tickets.
- Ship balanced default.
- T6: DXIL/metallib placeholders until T9/T10.
- T7: pinned SDL3 3.4.12 under `$MMD_NATIVE_CACHE` (or `~/.cache/mmd/native`); needs `libvulkan` + ICD on `LD_LIBRARY_PATH`. Headless: `SDL_VIDEODRIVER=offscreen`.

## Status

| ID  | Title | State   | SHA | Note |
| --- | ----- | ------- | --- | ---- |
| T1  | Workspace + governance shell | done | 8888c27 | |
| T2  | Versioned scenario contract | done | d246199 | |
| T3  | Deterministic atlas generator | done | bd0db6f | |
| T4  | Shared flow field | done | ba15068 | |
| T5  | SoA movement + recycling | done | 15f421c | |
| T6  | Pinned native deps + shaders | done | 5ecc832 | DXIL/metallib placeholder |
| T7  | Linux SDL3 GPU static slice | done | c180d21 | Vulkan offscreen + window |
| T8  | Moving 50k interactive slice | pending | — | |
| T9  | Windows/D3D12 port | pending | — | needs Windows host |
| T10 | macOS/Metal port | pending | — | needs M4 host |
| T11 | Benchmark + JSON report | pending | — | |
| T12 | Zero-allocation contract | pending | — | |
| T13 | Backend golden correctness | pending | — | needs T9/T10 |
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

- T1–T6 done
- T7 done: Linux/Vulkan static 4-atlas draw + 1920×1080 readback; `run` CLI wires offscreen+window
