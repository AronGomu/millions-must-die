# ADR 010: Separation Amortisation, Bin Stamping + Push Priority

- Status: Accepted
- Date: 2026-08-09
- Supplements: [ADR 009](009_ADR_agent_separation_and_collision.md) — the
  separation *model* is unchanged. This records how often it runs, how it is
  indexed, and how it is weighted.
- Plan: `artifacts/PLAN_2026_08_09_horde-sim-headroom.md`
- Evidence: `.tmp/RESEARCH_they_are_billions_performance.md` (primary source per
  claim)

## Context

ADR 009 gave agents bodies. Every agent now runs a neighbour scan every tick.
That scan is the only per-agent, per-tick, neighbour-visiting work in
`sim::tick::step`, and it is the only part of the tick whose cost tracks agent
count closely.

The navigation half does not. Age of Empires IV published the shape: raising
unit count by a factor of 200 raised flow-field cost by a small factor and
steering cost by a large one. Field work is sublinear in agent count; steering
work is near-linear. So this ADR is entirely about one loop, and the flow field
is deliberately left alone.

Constraints that already bind: no per-frame heap allocation
(`crates/mmd-engine/src/alloc_guard.rs`), exact same-host determinism with a
cross-process proof, and **no performance number may gate a merge**
(`docs/05-testing.md`). The last one is load-bearing here — it means every
decision below has to be justified by mechanism and accepted on behaviour, and
none of it may be defended with a measurement, because none exists.

One more constraint, self-imposed and worth naming: nothing here may cost a
guarantee. Every capability is scenario data with an identity default, so a
scene that does not opt in walks a **bit-identical** path. The two pinned
digests in `crates/mmd-engine/tests/separation.rs` are the regression guard for
the whole plan.

## Decision

### (a) Amortise the pass — `separation_phases`

Spread both halves of the pass — the neighbour scan and the grid rebuild — over
`separation_phases` ticks. Agent `i` recomputes when
`i % phases == tick_index % phases`; the grid rebuilds when
`tick_index % phases == 0`. `sep_x` / `sep_y` are already persistent fields on
`Simulation`, so a skipped agent already reads exactly what it would have read.

Precedent: Reynolds, *Big Fast Crowds on PS3* (2006), ran 15 000 agents with a
`skipThink` count of 8 in two demos and 10 in the 2D crowd — steering recomputed
once every ten frames, reused in between.

This is **not** the thing Graham warns about. His critique is that time-slicing
spreads the same work thinner without changing the total. `skipThink` does
*less* total work: the skipped scan is never run. Different mechanism, different
outcome.

**Strided, not blocked.** An agent belongs to `i % phases`. Agent index
correlates with spawn cell, so splitting into contiguous blocks would refresh
one region of the crowd at a time and read as a wave crossing the horde. A
stride spreads the refresh uniformly. The stride costs locality; the block costs
legibility. Legibility wins — this is a game.

**One cadence, not two.** The grid rebuild drops to the same cadence as the
scan. At four phases a neighbour position is at most three ticks old. That is
accepted: ADR 009 already forbids any zero-overlap claim, so nothing written
down is weakened. A scene that cannot tolerate staleness pins `1`, which every
tracked scene except `collision_mid_v1` does.

**`1` is the identity.** Both rules degenerate to the pre-ticket engine, and
`BODIED_STACK_HASH` is what proves it.

### (b) Stamp the bins — `counts`, `count_stamp`, `stamp`

`SpatialGrid::rebuild` cleared `starts` before counting into it. Bin populations
now carry a rebuild stamp: a bin whose stamp is stale reads as empty, so the
clear is gone and the prefix pass reads zero for it. `starts` is still written
for every bin, so an empty bin keeps a valid zero-width span — which the row
window in (d) depends on.

Precedent: Teschner et al. — *"our implementation of the hash table does not
require a re-initialization in each simulation step … each simulation step is
labeled with a unique time stamp."* BioDynaMo converged on the same trick
independently.

The stamp is `u32` and wraps by doing one full clear, so a stale bin can never
resurrect its old population.

**Rejected: BioDynaMo's O(#agents) rebuild.** Reaching it means abandoning the
prefix-sum layout for a per-bin linked list — which destroys the bucket
contiguity that (d) exploits and turns a contiguous scan into pointer chasing —
or sorting the touched-bin list every tick, which trades a streaming pass over
the bin array for a comparison sort over the agent array. With the gate scene's
bin-to-agent ratio there is no reason to expect that trade to pay, and since
benchmarking is retired it cannot be settled by measurement. The stamped form
ships; the rewrite does not. What the stamped form definitely buys is an O(1)
`bin_count`, which (d) uses.

### (c) Weight the push — `mass_class_count`

Each agent carries a push priority byte. The push on `i` from neighbour `j` is
scaled by `mass[j] / mass[i]`, evaluated as `mass[j] as f32 * inv_mass[i]` with
the reciprocal precomputed at construction. Heavy neighbours push harder; heavy
agents are pushed less.

Motivation: *March of the Froblins* names the bug this fixes before it happens —
*"agents can deadlock and will become stuck. This typically happens at sinks in
agent navigations such as at a small goal … agents that reach the goal will be
unable to navigate out of the goal area."* Millions Must Die is a fortress
defence; a small goal with a crowd on it is the entire game.

The shipped-RTS answer is asymmetry, not a better solver. StarCraft II 5.0.15's
patch notes: *"Increased allied push priority for Thors and Siege Tanks."*
Emerson describes the same device for *"super large robots that could push back
a hundred tanks."*

**Rejected: ORCA / RVO2.** Its own authors report the linear program becoming
infeasible in dense conditions and producing global deadlock, and Narain et al.
found RVO *"failed to run on scenes containing more than 70,000 agents."* At
this project's densities it fails exactly where it is needed.

**Rejected: a physical power-law force.** Karamouzas et al. state their measured
model *"leads to collisions and other discontinuities in motion with time steps
much larger than 10 ms."* This tick is longer than that. The existing linear
falloff is the numerically safer choice at this timestep and stays.

Assignment is `mass[i] = (i % classes) + 1` — a deterministic placeholder. Phase
1 replaces the rule with per-unit-type mass; the storage is what is being built
now. With one class every mass is `1`, every reciprocal is `1.0`, and IEEE
multiplication by exactly `1.0` is exact, so one class is bit-exact.

### (d) Walk each row as one run

A bin's linear index is `bx + by * cols`, and the counting sort lays buckets out
in ascending linear index. The three bins of one row of the 3×3 window are
therefore already a single contiguous run in `items`. The scan fetches three row
slices instead of nine bin slices, yielding the identical agents in the
identical order. An agent whose whole window holds one agent — itself — writes
zeroes without touching `items` at all.

**Rejected: Ericson's min-corner binning.** The research backlog proposed
binning by min corner so a 2×2 window replaces the 3×3 one. It does not
transfer. Ericson's 2×2 works because each *pair* is enumerated once and
contributes to both members; `accumulate_separation` is a **gather** — for agent
`i` it must see every neighbour `j` — and a 2×2 window anchored at `i`'s
min-corner bin misses every `j` whose bin is one lower on either axis.
Converting to a symmetric scatter would write `sep[j]` from agent `i`'s
iteration, which destroys the index-disjointness the worker pool in
[ADR 011](011_ADR_parallel_separation_and_the_allocation_invariant.md) depends
on, and contradicts the decision recorded in `sim/collision.rs` that a
cap-truncated pair need not cancel. The transferable half — visit fewer, longer
runs — shipped instead, and unlike the 2×2 it is bit-exact.

### Unchanged

The neighbour cap stays at 8. Detour hard-codes 6, RVO2's demos ship 10, and
Reynolds used 5 at 15 000 agents; 8 sits inside the band everything that shipped
at scale uses, and Guy & Karamouzas give the reason — a fixed maximum makes
runtime nearly linear in agent count. The coincidence tie-break table, the
linear falloff, the 3×3 window and the steering-not-resolution model are all
untouched.

## Consequences

- Three new required scenario fields. A `.ron` authored before this ADR fails to
  parse, deliberately — a stale scenario must never run silently untuned.
- `collision_mid_v1` runs four phases; `collision_sprite_v1` runs two mass
  classes. Every other tracked scene, the gate scene included, is pinned to the
  identity tuning, and its walk is bit-identical.
- Amortisation is honest about what it costs: a stale repulsion lets an agent
  walk further into an overlap before being pushed out. ADR 009 already refuses
  to claim agents cannot overlap, so this narrows nothing that was promised.
- The obstacle guarantee is untouched. `step_admissible` runs on every agent
  every tick regardless of phase, so an amortised agent still cannot enter a
  blocked cell or cut a blocked corner.
- **No claim of speed is made here, by anyone, about this engine.** Performance
  measurement is retired for phase 0 and nothing above has been measured on this
  code. The figures quoted are other people's published results, cited to
  attribute a mechanism, and they gate nothing.

## Sources

- Cheng, *Pathing in Age of Empires IV*, GDC 2022
- Reynolds, *Big Fast Crowds on PS3*, Sandbox 2006
- Graham, *Efficient, Event-Based Simulations*, Game AI Pro
- Teschner et al., *Optimized Spatial Hashing for Collision Detection of
  Deformable Objects*
- BioDynaMo, PPoPP 2023
- Ericson, *Real-Time Collision Detection*, ch. 7
- Shopf, Barczak, Oat, Tatarchuk, *March of the Froblins*, SIGGRAPH 2008
- Blizzard, *StarCraft II 5.0.15 patch notes*
- Emerson, *Crowd Pathfinding and Steering Using Flow Field Tiles*, Game AI Pro
- van den Berg et al., *Reciprocal n-Body Collision Avoidance* (ORCA)
- Narain, Golas, Curtis, Lin, *Aggregate Dynamics for Dense Crowd Simulation*
- Karamouzas et al., *Universal Power Law Governing Pedestrian Interactions*
- Guy & Karamouzas, *A Guide to Anticipatory Collision Avoidance*, Game AI Pro 2
