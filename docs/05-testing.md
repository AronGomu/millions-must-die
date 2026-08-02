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

These docs define planned gates. Measured results remain unavailable until implementation + physical calibration complete.
