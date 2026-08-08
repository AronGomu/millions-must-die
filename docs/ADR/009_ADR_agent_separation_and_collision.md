# ADR 009: Agent Separation + Collision

- Status: Accepted
- Date: 2026-08-08
- Supersedes in part: [ADR 003](003_ADR_simulation_and_flow_field.md) — the "overlap allowed, no agent collision/separation/spatial neighbor grid" decision only
- Supersedes in part: [ADR 001](001_ADR_technical_prototype_scope_and_acceptance.md) — the "Collision, separation" half of its explicit exclusions only; dynamic obstacles and per-agent paths stay excluded

## Context

ADR 003 froze phase 0 on a horde with no agent-agent interaction: agents read a
flow-field vector and walked, straight through each other. Readable at a
distance, wrong up close — zombies merge into a single smear.

Geometry rules out the naive fix. The view is 1920×1080 = 2 073 600 px. A
sprite is 30×30 = 900 px. The gate scene carries 50 000 agents: 45 000 000 px
of sprite against a 2 073 600 px screen, 22× over. Hex-packed 30 px discs on
the free 80% of that screen top out near 1 700. "50 000 agents" and "sprites
never overlap" cannot both hold. Something has to give, and it must be an
explicit, recorded choice rather than a tuning accident.

Constraints already load-bearing: no per-frame heap allocation
(`crates/mmd-engine/src/alloc_guard.rs`), no per-enemy pathfinding, exact
same-host determinism, and no performance number may gate a merge
(`docs/05-testing.md`).

## Decision

**Model — soft separation steering, not resolution.**
Per tick, per agent: sum a repulsion vector over overlapping neighbours, add it
to the flow-field descent vector, renormalise, walk one step along the blend.

```
v     = flow(cell) + strength * Σ repulsion
pos  += normalize(v) * step_len
```

Speed never changes. Only heading bends. Nothing forbids an overlap.

Rejected: **hard push** (gather pairs, displace each by half the penetration).
Cleaner guarantee, but it needs a position-correction phase that fights the
obstacle clamp and the arrival test, and its guarantee is approximate anyway
after one relaxation pass. Rejected: **occupancy lattice** (one agent per
sub-cell slot). Exact zero overlap, but movement snaps to a lattice and
corridors deadlock.

**Body size is scenario data, not a constant.** Two new fields on the scenario
contract, Q8 fixed point (256 = one cell, or the scalar 1.0):

- `collision_radius_q8` — body radius. Contact distance is twice it.
- `separation_strength_q8` — weight of the repulsion sum against the unit flow
  vector.

`u32`, not `f32`, because `Scenario` and `ScenarioSpec` derive `Eq`. The
conversion `q as f32 / 256.0` is exact — the divisor is a power of two. Both
fields are required in the RON: a stale scenario must fail loudly, never run
silently bodyless.

**One model, four radii**, because one radius cannot serve both scale and
legibility:

| Scenario | Agents | Radius | Reads as |
| --- | --- | --- | --- |
| `technical_prototype_v1` (gate) | 50 000 | 0.398 cell ≈ 1.6 px | a dense fluid; sprites still overlap, by geometry |
| `collision_mid_v1` | 10 000 | 1.25 cells = 5 px | body boxes touch, shoulders overlap |
| `collision_sprite_v1` | 1 200 | 3.75 cells = 15 px | full sprite half-width; sprites keep clear |
| `fixture_*` | 64–256 | 0.125 cell | collision exercised without choking 1-cell corridors |

The two demo scenes are a third version family, `collision_scene_v1`: the gate
scene's screen geometry and destination, free population and radius under a
20 000 cap. `technical_prototype_v1` freezes its 50k/100k workload and its
exact-20% obstacle ratio, and the `fixture_` family caps grids at 65 536 cells
— neither can express a full-screen low-count scene.

**Always on.** No CLI flag, no runtime switch. A scenario with
`collision_radius_q8: 0` has no body, which makes "collision off is bit-identical
to the old flow-only walk" a testable claim rather than an assurance.

**Neighbour index — preallocated uniform grid.** `sim::SpatialGrid`: bins of
`max(1.0, 2 × radius)` cells, counting-sorted each tick into buffers reserved
at construction. A 3×3 bin scan therefore covers every possible contact.
Buckets come out in ascending agent index, which is what makes the scan
reproducible and not merely correct. No allocation after load, so the
zero-allocation-per-frame invariant stands.

**Coincident agents.** 50 000 agents seed onto 127 spawn cells — roughly 394
share one exact coordinate at tick 0, and every recycle drops an arrival back
onto an occupied spawn. There is no direction between two identical points, so
a 16-entry unit-vector table indexed by `(i ^ j) & 15` supplies one, signed by
`i < j` so the pair pushes equally and oppositely. Deterministic, no RNG, no
clock, no trigonometry at runtime.

**Neighbour cap: eight per agent per tick.** A correctness measure, not an
optimisation: an uncapped scan of a 394-deep stack is quadratic in the stack.
The scan order is fixed, so the cap is reproducible. A deep stack unpacks over
several ticks instead of one.

**Obstacles win.** If the blended step leaves the walkable area, the agent
retries the pure descent step before giving up. Separation may never wedge an
agent the field alone could have moved — that is what keeps "no agent is stuck
against an obstacle" a structural guarantee rather than an empirical one. The
cost is that a wall beats a crowd: bodies compress against it.

**The funnel jams, and that is the behaviour.** 50 000 agents converge on one
destination cell. With bodies, they pile up; arrivals still drain through the
0.5-cell arrival radius. No drain valve, no widened arrival radius, no
collision-free zone near the goal.

## Consequences

- The strict "mean routing cost falls every tick" contract was **kept**, not
  re-derived. At the tracked fixtures' body radius (1/8 cell) a spawn stack
  opens without moving anyone into a costlier cell, so
  `aggregate_progress_is_monotone` still passes
  (`crates/mmd-engine/tests/simulation.rs`). What changed is that the strict
  claim is now tuning-dependent — a body large enough to push members backwards
  for a few ticks would break it — so a weaker sibling,
  `aggregate_progress_never_stalls`, was added *alongside* it: the horde never
  goes a full second of simulated time closing no distance at all. If a future
  tuning breaks the strict claim, the strict test is meant to fail loudly and
  the stall floor is what still holds.
- Every bodied state hash changes. Two digests *are* pinned as literals in
  `crates/mmd-engine/tests/separation.rs`, and they say opposite things:
  `BODYLESS_GRID_PRE_SEPARATION_HASH` was measured on the commit before
  separation existed and must **not** move, since it is the only real proof that
  a zero-radius scenario still walks the old flow-only path;
  `BODIED_STACK_HASH` is a change detector on the separation model itself and is
  re-measured whenever a deliberate change moves it. Everything else is
  observable at the CLI rather than a test edit.
- Render goldens are unaffected: the golden scene is
  `SpriteRenderer::static_demo_groups`, independent of simulation state.
- Every scenario `.ron` outside this repo is invalid until it declares both
  fields. Intended.
- **No hard guarantee ships.** Under crowd pressure — most visibly in the
  destination jam — bodies interpenetrate. No code comment, doc sentence or
  test may claim otherwise.
- Cost is **unmeasured** and claims nothing. Performance gating is retired to a
  later optimization phase (`docs/05-testing.md`); this record states no speed
  figure and none gates a merge.

## Testing

Behavioural, deterministic, and in `crates/mmd-engine/tests/separation.rs`
unless noted:

- bins hold every agent exactly once, in ascending index order, with
  out-of-rect and non-finite coordinates clamped rather than panicking;
- a coincident pair pushes equally and oppositely, and splits on the first tick;
- the repulsion sum is capped at eight contributions;
- a blended step keeps the step length — heading bends, speed does not;
- a released stack of sixteen spreads apart;
- a bodyless scenario is hash-identical to the flow-only walk;
- deep overlap on `collision_sprite_v1` at least halves within 300 ticks and no
  later sample rises back above the tick-1 baseline (the asserted bar is the
  halving; the tracked scene measures 16 229 deeply overlapping pairs at tick 1
  against 5 919 at tick 300);
- agents on a collision scene never enter an obstacle and never leave the world
  rect;
- a collision tick allocates nothing
  (`crates/mmd-engine/tests/frame_allocations.rs`).

## Related

- [ADR 003 — Simulation + flow field](003_ADR_simulation_and_flow_field.md)
- [Agent collision architecture](../agent-collision-architecture.html)
- [Testing strategy](../05-testing.md)
