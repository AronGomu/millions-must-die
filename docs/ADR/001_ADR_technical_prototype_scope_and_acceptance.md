# ADR 001: Technical Prototype Scope + Acceptance

- Status: Accepted
- Date: 2026-08-02
- Superseded by: —

## Context

Roadmap phase 0 asks one question: can custom Rust engine move + render massive hordes? Later roadmap phases own camera, RTS controls, combat, defense, campaign.

Vague “stable frame time” cannot produce go/no-go result. Fixed workload + fixed HW needed.

## Decision

Phase 0 includes:

- 1k + 10k measured as nonblocking scale points.
- 50k enemies simulated + drawn inside fixed 1920×1080 target; hard gate.
- 100k enemies measured as nonblocking stretch.
- Total-frame hard gate: p95 ≤16.67 ms; p99 ≤25 ms.
- Native Linux/Vulkan, Windows/D3D12, macOS/Metal validation.
- Developer engineering UX only: `run`, `bench`, metrics overlay.

Explicit exclusions:

- Camera pan/zoom, selection, workers, economy, buildings, production.
- Combat, enemy AI, walls, waves.
- Collision, separation, dynamic obstacles, per-agent paths.
- Production assets, packaging, Steam, online CI, WASM.

Phase passes only when same exact source hash passes every ref host. Failure remains valid result. Threshold never weakened silently.

## Consequences

Positive:

- Clear technical answer before gameplay expansion.
- Smallest workload matching vision.
- 100k data retained without blocking realistic 50k gate.

Negative:

- Scene not playable RTS.
- Fixed camera overstates worst-case visible density versus typical gameplay.
- Physical lab becomes phase-completion dependency.

## Rejected alternatives

- Phase 0 + RTS prototype: too broad; hides scale result.
- 100k hard gate: excessive first proof on entry HW.
- Average FPS: hides tail stutter.
- 30 Hz target: conflicts with mechanical RTS feel.

## Validation

- Locked scenario hash.
- 10s warmup; 7×60s captures; median trial + MAD noise checks.
- Required visible native smoke on all platforms.
- Required final exact-hash 3-host report.

## References

- `docs/00-vision.md`
- `docs/02-prototype-roadmap.md`
- `docs/05-testing.md`
- `.tmp/IMPLEMENTATION_PLAN_technical_prototype.md`
