# ADR 003: Simulation + Flow Field

- Status: Accepted
- Date: 2026-08-02
- Superseded by: [ADR 009](009_ADR_agent_separation_and_collision.md), in part — the "no agent collision" decision only

## Context

Prototype must move 50k agents without per-enemy pathfinding or frame allocation. Phase 0 needs future-relevant nav cost, not full crowd behavior.

## Decision

Scenario:

- 480×270 cells; 4 px/cell.
- 20% fixed obstacle cells.
- One fixed destination.
- Seeded spawn cells.

Navigation:

- Reverse 8-neighbor Dijkstra integration field.
- Cardinal cost 1000; diagonal cost 1414.
- No diagonal corner-cutting.
- Stable neighbor order N, NE, E, SE, S, SW, W, NW; heap ties use cell index.
- Normalized `f32` descent vectors.
- Build once before timed run.

Simulation:

- `f32` structure-of-arrays.
- Fixed 60 Hz; speed 8 cells/s; nearest-cell flow sample.
- Arrival radius 0.5 cell; blocked/out-of-bounds next step retains prior position.
- Benchmark: exactly one sim tick per rendered frame.
- Stable agent iteration.
- Arrivals recycle to seeded spawn cells without allocation.
- ~~Overlap allowed. No agent collision/separation/spatial neighbor grid.~~ Superseded by [ADR 009](009_ADR_agent_separation_and_collision.md): agents now carry a scenario-declared body radius and steer apart through a uniform neighbour grid. Every other decision in this record stands.
- Same-platform state hash exact.
- Cross-platform positions quantized to 1/256 cell; drift ≤1 quantum; direction/frame exact.

## Consequences

Positive:

- One field amortized across whole horde.
- Data-oriented hot loops.
- Deterministic workload/population.
- Relevant foundation for later gameplay nav.

Negative:

- Overlap hurts crowd realism/readability.
- Static field omits rebuild spikes.
- `f32` output not bit-identical across CPUs.
- No evidence yet for local avoidance cost.

## Rejected alternatives

- Direct target steering: under-tests chosen architecture.
- Per-agent A*: violates architecture rule.
- Dynamic field rebuilds: extra phase-0 scope.
- Fixed-point: stronger lockstep, higher math complexity; offline prototype does not need it.
- Uniform agent spatial grid: no phase-0 consumer.

## Validation

- Small-map Dijkstra behavior tests.
- Full-scene field hash.
- 10k-tick 50k-agent population soak.
- Quantized cross-platform checksum tolerance tests.
- Counting allocator hard-fails post-warmup Rust allocations.

## References

- `docs/01-technical-architecture.md`
- `docs/simulation-navigation-architecture.html`
- `.tmp/IMPLEMENTATION_PLAN_technical_prototype.md` T2–T5
