# Testing Strategy

Prototype-first development.

Each prototype answers one question before continuing.

Benchmark:
- 1k, 10k, 50k, 100k enemies.
- Stable frame time.
- CPU/GPU profiling.
- Test on entry-level hardware when available.

Maintain benchmark scenarios throughout development.

Phase-0 accepted measurement design:
- [Benchmark measurement ADR](ADR/005_ADR_benchmark_measurement_and_baselines.md)
- [Native platform/HW ADR](ADR/006_ADR_native_platform_and_reference_hardware.md)
- [Local validation lab architecture](local-validation-lab-architecture.html)

Phase-0 measured results (2026-08-05): a real Linux/Vulkan production bench ran
and is recorded in [technical prototype results](technical-prototype-results.md)
with raw evidence at `lab/releases/evidence/linux-vulkan-bench-production-v1.json`.
The blocking 50k measurement came back **inconclusive** (trial noise above the
locked `nmad <= 0.03` bound on a host running a capture session), so no release
proof is frozen: `mmd-lab release-freeze` refuses to derive any status —
pass or failure — from indecisive evidence. Scope is therefore **gate open;
native cross-platform matrix deferred** — Windows/macOS native lanes and
physical-pilot relative baselines stay `deferred-hw`; relative gates remain
disabled until reviewed real-hardware baselines exist.
