# ADR 005: Benchmark Measurement + Baselines

- Status: Accepted
- Date: 2026-08-02
- Superseded by: —

## Context

Average FPS, one-off runs, ephemeral HW cannot prove stable massive-horde performance. SDL3 GPU exposes fences, not portable timestamp queries.

## Decision

Hard scene:

- 1k + 10k required scale points; nonblocking.
- 50k agents, all simulated + drawn, 1920×1080 offscreen; hard gate.
- Median p95 frame service ≤16.67 ms.
- Median p99 frame service ≤25 ms.
- 100k required stretch report; nonblocking.

Sampling:

- Per count: 10s warmup; 7 captures × 60s.
- SDL GPU frames in flight capped at 2. Wait oldest fence before frame 3; final drain requires completed=submitted.
- Frame service sample includes queue backpressure, sim, upload, encode, submit.
- Hyndman–Fan type-7 percentile per trial.
- Median of 7 trial p95/p99 scalars.
- Normalized MAD = `1.4826 × MAD / median`; >3% → inconclusive.
- One fixed sim tick/rendered measured frame.

Metrics:

- Frame service: absolute + calibrated relative gate.
- Simulation: calibrated relative gate.
- Instance upload: calibrated relative gate.
- `gpu_queue_latency`: async submit-to-fence completion proxy; calibrated relative gate.
- Never call fence proxy true GPU execution time.
- True GPU diagnosis: RenderDoc/Xcode manual captures.

Calibration:

- 50 clean full runs/platform.
- Noise envelope + reviewed margin.
- Driver/HW/OS/scenario/shader manifest binds baseline.
- Manifest change requires reviewed recalibration.

Correctness:

- Post-warmup Rust alloc count = 0.
- Per-backend golden images + bounded reviewed tolerance.
- Full reports local; ordinary retention 90 days; release/calibration retained indefinitely.

## Consequences

Positive:

- Tail latency visible.
- Absolute usability + relative headroom protected.
- Noise handled explicitly.
- Metric labels stay technically honest.

Negative:

- Every-merge run takes ~7+ minutes/platform before restore overhead.
- Fence latency includes queue delay.
- Calibration consumes 150 full platform runs.
- Golden tolerance needs native evidence.

## Rejected alternatives

- Average FPS only.
- One 60s sample.
- VSync-on benchmark.
- Universal pixel-exact golden.
- Native timestamp backend forks.
- Silent threshold relaxation after miss.

## Validation

- Deterministic stats fixtures.
- JSON Schema report validation.
- Absolute/relative/inconclusive verdict tests.
- 100k nonblocking test.
- Final exact-hash 3-host report.

## References

- `docs/05-testing.md`
- `docs/local-validation-lab-architecture.html`
- `.tmp/IMPLEMENTATION_PLAN_technical_prototype.md` T11–T12, T18
