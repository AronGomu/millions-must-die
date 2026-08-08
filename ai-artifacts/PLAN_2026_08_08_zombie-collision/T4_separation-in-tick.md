# T4: Separation wired into the tick

**Plan:** `./ai-artifacts/PLAN_2026_08_08_zombie-collision.md`
**Depends:** T3
**Commit outcome:** Zombies push each other apart. Every tick, on every scenario that declares a body radius, a repulsion vector is summed into the flow-field vector before the single move. The gate scene's state hash changes; the two behavioural contracts that assumed no agent-agent forces are re-derived rather than deleted.

## Context (self-contained)

- Goal: zombies stop passing through each other. The chosen model is **soft separation steering** — `v = flow(cell) + k * sum(repulsion)`, then `pos += normalize(v) * step`. Speed never changes; only heading bends. There is no hard non-overlap guarantee and no test may claim one.
- This slice: the behaviour flip. Everything before it was data and plumbing.
- Out of scope here: new scenario files (T5), docs and the systems map (T6), the renderer, the CLI, `crates/mmd-engine/src/nav/**`.
- Assumptions in force:
  - **Always on.** There is no flag. A scenario with `collision_radius_q8: 0` simply has no body, which is what makes "collision off equals the old flow-only path" a testable claim.
  - **Jam is real.** 50 000 agents funnelling into one destination cell will pile up; arrivals still drain through the 0.5-cell arrival radius. Do not add a drain valve, do not widen `ARRIVAL_RADIUS`, do not disable separation near the destination.
  - Determinism is a hard contract: fixed iteration order, no randomness, no time input.
  - No per-frame heap allocation (`crates/mmd-engine/src/alloc_guard.rs`).

## Inputs

- `crates/mmd-engine/src/sim/tick.rs` (122 lines). Current `step` body, which this ticket edits:
  - hoists `width`, `height`, `dest_cx`, `dest_cy`, `dest_x`, `dest_y`, `step_len = SPEED_CELLS_PER_SEC * TICK_DT`, `arrival_r2`, `frame_count`, `n_spawn`, `n = sim.x.len()`;
  - loops `for i in 0..n`, and per agent: arrival check → `recycle_one`; `nearest_cell` sample with an out-of-bounds guard; destination-cell recycle; `let vx = sim.field_vx[cidx]; let vy = sim.field_vy[cidx];`; a `vx == 0.0 && vy == 0.0` inert guard; `let nx = px + vx * step_len; let ny = py + vy * step_len;`; a `!position_walkable(nx, ny, width, height, &sim.blocked)` guard that retains position; then the commit `sim.x[i] = nx; sim.y[i] = ny; sim.dir[i] = dir_from_vector(vx, vy); advance_frame(&mut sim.frame[i], frame_count);`
  - private helpers already present: `recycle_one`, `nearest_cell`, `position_walkable`, `advance_frame`, `dir_from_vector`.
- `crates/mmd-engine/src/sim/agents.rs` (249 lines). `Simulation` has `pub(super)` fields `width, height, dest_cx, dest_cy, dest_x, dest_y, field_vx, field_vy, blocked, spawn_x, spawn_y, initial_agent_count, recycle_cursor, tick_index, atlas_count, dir_count, frame_count, x, y, atlas, dir, frame`. Constructors: `Simulation::new(scenario: &Scenario, field: &FlowField)` which delegates to `Simulation::new_custom(field, destination, spawn_cells, agent_count, atlas_count, dir_count, frame_count)`.
- `crates/mmd-engine/src/runtime.rs` — the **only** external caller of `new_custom`, at line ~124, inside `Runtime::from_scenario`.
- **From Depends (T2), quoted because the worker cannot read that ticket:**
  - `mmd_engine::scenario::COLLISION_Q8: u32 = 256`
  - `Scenario::collision_radius_q8(&self) -> u32`, `Scenario::separation_strength_q8(&self) -> u32`
  - Tracked tunings already in the `.ron` files: gate scene `102 / 256`; all four fixtures `32 / 256`; inline `GridSpec` grids default to `0 / 0`.
  - `GridSpec::with_collision(self, radius_q8: u32, strength_q8: u32) -> GridSpec` opts an inline grid in.
- **From Depends (T3), quoted verbatim:**
  - `mmd_engine::sim::SpatialGrid`
  - `SpatialGrid::new(width: u32, height: u32, bin_size_cells: f32, capacity: usize) -> SpatialGrid`
  - `SpatialGrid::rebuild(&mut self, x: &[f32], y: &[f32])` — counting sort, no allocation, buckets ascending by agent index
  - `SpatialGrid::bin_of(&self, x: f32, y: f32) -> (u32, u32)` — clamps out-of-rect and non-finite input
  - `SpatialGrid::agents_in_bin(&self, bx: u32, by: u32) -> &[u32]` — empty slice for an out-of-range bin
  - `SpatialGrid::cols(&self) -> u32`, `rows(&self) -> u32`, `len(&self) -> usize`, `capacity(&self) -> usize`, `bin_size_cells(&self) -> f32`
  - `crates/mmd-engine/tests/separation.rs` already exists with six `SpatialGrid` tests and opens with `use mmd_engine::sim::SpatialGrid;`
  - `crates/mmd-engine/tests/frame_allocations.rs` already has `spatial_rebuild_allocates_nothing` and a `lock_alloc_tests()` mutex helper that every test in that file takes first.

## Requirements

- New module `crates/mmd-engine/src/sim/collision.rs` holding `CollisionParams`, the coincidence tie-break table, the neighbour cap, and `accumulate_separation`.
- `Simulation` owns its grid and two separation accumulators, all sized at construction.
- `tick::step` blends only when collision is enabled, so a zero-radius scenario is **bit-identical** to today's flow-only path.
- Two exactly-coincident agents must separate — deterministically, with no randomness and no time input.
- Separation must never wedge an agent the field alone could have moved (see the fallback in the code below); this is what keeps `no_agent_is_stuck_against_an_obstacle` provably safe rather than empirically lucky.
- `aggregate_progress_is_monotone` is re-derived, not deleted.

## Code to add — copy verbatim

`crates/mmd-engine/src/sim/collision.rs`:

```rust
//! Agent-agent soft separation.
//!
//! The model is steering, not resolution: each agent sums a repulsion vector
//! from the neighbours overlapping its body, that sum is added to the
//! flow-field descent vector, and the *blended* direction is what the agent
//! walks along at its unchanged speed. Overlap is therefore reduced, never
//! forbidden — no caller may claim a zero-overlap guarantee.
//!
//! Determinism: the neighbour scan visits bins in a fixed order and each bin's
//! agents in ascending index order (`super::spatial::SpatialGrid`), the
//! coincidence tie-break is a table lookup keyed on the index pair, and nothing
//! reads a clock or an RNG.

use crate::scenario::{COLLISION_Q8, Scenario};

use super::spatial::SpatialGrid;

/// Neighbours one agent may accumulate in a tick.
///
/// A cap is required, not an optimisation: 50 000 agents seeded onto 127 spawn
/// cells start ~394 deep on one coordinate, and an uncapped scan would be
/// quadratic in that stack. Capping keeps the pass bounded and, because the
/// scan order is fixed, keeps it reproducible.
pub const MAX_SEPARATION_NEIGHBORS: usize = 8;

/// Below this squared distance two agents count as coincident and the tie-break
/// table replaces the (undefined) direction between them.
pub const COINCIDENT_EPS2: f32 = 1e-12;

/// Deterministic push directions for coincident agents, keyed by `(i ^ j) & 15`
/// and signed by `i < j` so a pair always pushes equally and oppositely.
/// Sixteen unit vectors at 22.5-degree steps.
pub const SEPARATION_DIR16: [(f32, f32); 16] = [
    (1.0, 0.0),
    (0.923_879_5, 0.382_683_43),
    (0.707_106_78, 0.707_106_78),
    (0.382_683_43, 0.923_879_5),
    (0.0, 1.0),
    (-0.382_683_43, 0.923_879_5),
    (-0.707_106_78, 0.707_106_78),
    (-0.923_879_5, 0.382_683_43),
    (-1.0, 0.0),
    (-0.923_879_5, -0.382_683_43),
    (-0.707_106_78, -0.707_106_78),
    (-0.382_683_43, -0.923_879_5),
    (0.0, -1.0),
    (0.382_683_43, -0.923_879_5),
    (0.707_106_78, -0.707_106_78),
    (0.923_879_5, -0.382_683_43),
];

/// Body size and steering weight for one scenario.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CollisionParams {
    /// Body radius in cells. Two agents are in contact at twice this.
    pub radius_cells: f32,
    /// Weight of the repulsion sum against the unit flow vector.
    pub strength: f32,
}

impl CollisionParams {
    /// No body: the separation pass is skipped entirely and the tick is
    /// bit-identical to a pure flow-field walk.
    pub const NONE: Self = Self {
        radius_cells: 0.0,
        strength: 0.0,
    };

    /// Convert from the scenario's Q8 fixed point. Exact: the divisor is a
    /// power of two.
    pub fn from_q8(radius_q8: u32, strength_q8: u32) -> Self {
        Self {
            radius_cells: radius_q8 as f32 / COLLISION_Q8 as f32,
            strength: strength_q8 as f32 / COLLISION_Q8 as f32,
        }
    }

    pub fn from_scenario(scenario: &Scenario) -> Self {
        Self::from_q8(
            scenario.collision_radius_q8(),
            scenario.separation_strength_q8(),
        )
    }

    pub fn enabled(&self) -> bool {
        self.radius_cells > 0.0 && self.strength > 0.0
    }

    /// Bin edge that makes a 3x3 bin scan cover every possible contact.
    pub fn bin_size_cells(&self) -> f32 {
        (2.0 * self.radius_cells).max(1.0)
    }
}

/// Write each agent's repulsion sum into `sep_x` / `sep_y`. Allocates nothing.
///
/// `grid` must have been rebuilt from the same positions this tick, and every
/// slice must be the same length.
pub fn accumulate_separation(
    x: &[f32],
    y: &[f32],
    grid: &SpatialGrid,
    radius_cells: f32,
    sep_x: &mut [f32],
    sep_y: &mut [f32],
) {
    let n = x.len();
    debug_assert_eq!(y.len(), n);
    debug_assert_eq!(sep_x.len(), n);
    debug_assert_eq!(sep_y.len(), n);
    debug_assert_eq!(grid.len(), n);

    let contact = 2.0 * radius_cells;
    let contact2 = contact * contact;
    let inv_contact = 1.0 / contact;
    let last_col = grid.cols() - 1;
    let last_row = grid.rows() - 1;

    for i in 0..n {
        let px = x[i];
        let py = y[i];
        let (bx, by) = grid.bin_of(px, py);
        let bx0 = bx.saturating_sub(1);
        let bx1 = (bx + 1).min(last_col);
        let by0 = by.saturating_sub(1);
        let by1 = (by + 1).min(last_row);

        let mut sx = 0.0f32;
        let mut sy = 0.0f32;
        let mut taken = 0usize;

        'scan: for cy in by0..=by1 {
            for cx in bx0..=bx1 {
                for &raw in grid.agents_in_bin(cx, cy) {
                    let j = raw as usize;
                    if j == i {
                        continue;
                    }
                    let dx = px - x[j];
                    let dy = py - y[j];
                    let d2 = dx * dx + dy * dy;
                    if d2 >= contact2 {
                        continue;
                    }
                    if d2 <= COINCIDENT_EPS2 {
                        // No direction exists between two identical points.
                        // The table gives one that is stable across runs and
                        // opposite for the two members of the pair.
                        let (ux, uy) = SEPARATION_DIR16[(i ^ j) & 15];
                        let sign = if i < j { 1.0 } else { -1.0 };
                        sx += sign * ux;
                        sy += sign * uy;
                    } else {
                        let d = d2.sqrt();
                        // Linear falloff: full push at coincidence, none at
                        // contact distance.
                        let w = (contact - d) * inv_contact;
                        let inv_d = 1.0 / d;
                        sx += dx * inv_d * w;
                        sy += dy * inv_d * w;
                    }
                    taken += 1;
                    if taken == MAX_SEPARATION_NEIGHBORS {
                        break 'scan;
                    }
                }
            }
        }

        sep_x[i] = sx;
        sep_y[i] = sy;
    }
}
```

`crates/mmd-engine/src/sim/mod.rs` becomes:

```rust
//! Fixed-tick SoA agent simulation.

mod agents;
mod collision;
mod spatial;
mod tick;

pub use agents::{AgentsView, Simulation, quantize_cell};
pub use collision::{
    COINCIDENT_EPS2, CollisionParams, MAX_SEPARATION_NEIGHBORS, SEPARATION_DIR16,
    accumulate_separation,
};
pub use spatial::SpatialGrid;
pub use tick::{ARRIVAL_RADIUS, SPEED_CELLS_PER_SEC, TICK_DT};
```

New `Simulation` fields, appended after `frame: Vec<u8>`:

```rust
pub(super) collision: CollisionParams,
/// Rebuilt every tick when collision is enabled; a 1x1 stub otherwise, so a
/// bodyless scenario reserves nothing.
pub(super) grid: SpatialGrid,
pub(super) sep_x: Vec<f32>,
pub(super) sep_y: Vec<f32>,
```

Construction inside `new_custom`, before the `Self { .. }` literal:

```rust
let grid = if collision.enabled() {
    SpatialGrid::new(width, height, collision.bin_size_cells(), agent_count)
} else {
    SpatialGrid::new(1, 1, 1.0, 0)
};
let sep_x = vec![0.0; agent_count];
let sep_y = vec![0.0; agent_count];
```

Accessor on `impl Simulation`, beside `agent_count`:

```rust
/// Body radius and steering weight this sim was built with.
pub fn collision(&self) -> CollisionParams {
    self.collision
}
```

The separation pass and blend in `tick::step` — insert the pass directly after
the existing hoists and before `for i in 0..n`:

```rust
let collision = sim.collision;
let collision_on = collision.enabled();
if collision_on {
    sim.grid.rebuild(&sim.x, &sim.y);
    super::collision::accumulate_separation(
        &sim.x,
        &sim.y,
        &sim.grid,
        collision.radius_cells,
        &mut sim.sep_x,
        &mut sim.sep_y,
    );
}
```

Then replace the block that today reads

```rust
        let nx = px + vx * step_len;
        let ny = py + vy * step_len;

        // OOB / obstacle next position → retain prior.
        if !position_walkable(nx, ny, width, height, &sim.blocked) {
            sim.dir[i] = dir_from_vector(vx, vy);
            advance_frame(&mut sim.frame[i], frame_count);
            continue;
        }

        sim.x[i] = nx;
        sim.y[i] = ny;
        sim.dir[i] = dir_from_vector(vx, vy);
        advance_frame(&mut sim.frame[i], frame_count);
```

with

```rust
        // Steering blend: the descent direction bent by the neighbours pressing
        // on this agent, walked at the unchanged speed. Skipped entirely when
        // the scenario declares no body, so a bodyless run stays bit-identical
        // to a pure flow-field walk.
        let (mut mx, mut my) = (vx, vy);
        if collision_on {
            let bx = vx + collision.strength * sim.sep_x[i];
            let by = vy + collision.strength * sim.sep_y[i];
            let l2 = bx * bx + by * by;
            if l2 > BLEND_EPS2 {
                let inv = 1.0 / l2.sqrt();
                mx = bx * inv;
                my = by * inv;
            }
        }

        let mut nx = px + mx * step_len;
        let mut ny = py + my * step_len;

        // Separation must never wedge an agent the field alone could have
        // moved: fall back to the pure descent step before giving up. Without
        // this, a crowd could pin an agent against a wall forever.
        if !position_walkable(nx, ny, width, height, &sim.blocked) {
            mx = vx;
            my = vy;
            nx = px + mx * step_len;
            ny = py + my * step_len;
            if !position_walkable(nx, ny, width, height, &sim.blocked) {
                sim.dir[i] = dir_from_vector(mx, my);
                advance_frame(&mut sim.frame[i], frame_count);
                continue;
            }
        }

        sim.x[i] = nx;
        sim.y[i] = ny;
        sim.dir[i] = dir_from_vector(mx, my);
        advance_frame(&mut sim.frame[i], frame_count);
```

and add, beside `ARRIVAL_RADIUS` in `tick.rs`:

```rust
/// Squared length below which a blended steering vector is treated as
/// cancelled, and the pure descent direction is kept instead.
const BLEND_EPS2: f32 = 1e-12;
```

`crates/mmd-engine/src/runtime.rs` — extend the `Simulation::new_custom` call at
line ~124 with a final argument:

```rust
                CollisionParams::from_scenario(&scenario),
```

and import it: `use crate::sim::{CollisionParams, Simulation};`.

`Simulation::new` passes `CollisionParams::from_scenario(scenario)` as the final
argument of its internal `Self::new_custom(..)` call.

## New Tracker helper — copy verbatim

Append to `impl Tracker` in `crates/mmd-engine/tests/common/mod.rs`:

```rust
    /// Longest run of consecutive ticks, before the first recycle, during which
    /// the mean routing cost never improved on the best value seen so far.
    ///
    /// Replaces the strict tick-by-tick monotonicity claim. With agent-agent
    /// separation the mean legitimately rises for a few ticks while a spawn
    /// stack pushes itself apart, so "never rises" is no longer true and never
    /// was interesting. What must stay true is that the horde never *stalls* —
    /// never goes a long stretch closing no distance at all.
    pub fn longest_progress_stall(&self) -> u64 {
        let horizon = match self.first_recycle_tick {
            Some(t) => (t as usize).saturating_sub(1),
            None => self.mean_cost.len(),
        }
        .min(self.mean_cost.len());
        if horizon == 0 {
            return 0;
        }
        let mut best = self.mean_cost[0];
        let mut run = 0u64;
        let mut worst = 0u64;
        for t in 1..horizon {
            if self.mean_cost[t] < best {
                best = self.mean_cost[t];
                run = 0;
            } else {
                run += 1;
                worst = worst.max(run);
            }
        }
        worst
    }
```

## Re-derived contract

Exactly one existing test changes. In `crates/mmd-engine/tests/simulation.rs`,
rename `aggregate_progress_is_monotone` to `aggregate_progress_never_stalls` and
replace its body with:

```rust
#[test]
fn aggregate_progress_never_stalls() {
    // Per-agent progress was never monotone, and with agent-agent separation
    // the horde's mean is not monotone either: a spawn stack pushing itself
    // apart moves some of its members backwards for a few ticks. The claim that
    // survives is stronger than "never rises" is useful: the horde must never
    // stop closing distance for a whole second of simulated time.
    const STALL_BUDGET_TICKS: u64 = 60; // 1 s at 60 Hz
    for (name, ticks) in [(FIXTURE_DENSE_V1, 400u64), (FIXTURE_CORRIDOR_V1, 700)] {
        let mut h = Harness::fixture(name).build().expect("fixture");
        let mut t = Tracker::new(&h);
        t.run(&mut h, ticks);

        let stall = t.longest_progress_stall();
        assert!(
            stall <= STALL_BUDGET_TICKS,
            "{name}: the horde closed no distance for {stall} consecutive ticks \
             (budget {STALL_BUDGET_TICKS})"
        );
        // Without this the assertion above passes trivially on a horde that
        // never moves.
        let series = t.mean_cost_series();
        let start = series[0];
        let lowest = series.iter().copied().fold(f64::INFINITY, f64::min);
        assert!(
            lowest < start,
            "{name}: mean routing cost never fell below its start ({start} → {lowest})"
        );
        assert!(
            t.first_recycle_tick().is_some(),
            "{name}: no agent arrived within {ticks} ticks"
        );
    }
}
```

`no_group_is_starved` needs **no** change: it already asserts `progress > 0.0`
and `group_recycles(g) > 0` per group, both of which survive a jam. Do not
touch it. Do not touch any other test in that file — in particular
`no_agent_is_stuck_against_an_obstacle`, `obstacles_are_never_entered`,
`agents_reach_destination`, `alive_count_is_stable` and
`determinism_holds_for_50k_agents` must pass unmodified. Renaming
`aggregate_progress_is_monotone` is safe: it is **not** listed in `SCOPE_SYSTEMS`
in `tests/validation_contract.rs` and is not named in
`docs/technical-prototype-functional-close.md` (both verified).

## Escalation rule — no silent weakening

If any of `aggregate_progress_never_stalls`, `agents_reach_destination`,
`no_group_is_starved` or `no_agent_is_stuck_against_an_obstacle` fails after the
wiring, **do not** raise a budget, lower a threshold, lengthen a tick count, or
add a `#[ignore]`. The one permitted remedy is to reduce the *fixture* body
radius from `32` to `16` in the four `assets/scenarios/fixtures/*.ron` files and
regenerate their `.sha256` sidecars with:

```sh
python3 - <<'PY'
import hashlib, pathlib
for p in sorted(pathlib.Path("assets/scenarios/fixtures").glob("*.ron")):
    d = hashlib.sha256(p.read_bytes()).hexdigest()
    p.with_suffix(".sha256").write_text(d + "\n")
    print(p, d)
PY
```

Re-measure. If a test still fails at radius `16`, stop and report the observed
numbers — the tuning is wrong and that is a decision for the plan author, not
a constant to nudge.

## TDD

1. **Red** — add the eight tests below to `crates/mmd-engine/tests/separation.rs` and the one allocation test to `crates/mmd-engine/tests/frame_allocations.rs`. They fail to compile (`CollisionParams`, `accumulate_separation`, `with_collision`-driven behaviour do not exist yet).
2. **Green** — add `collision.rs`, the `Simulation` fields, the `tick::step` blend, and the `runtime.rs` call-site argument.
3. **Refactor** — none. Do not extract a trait, do not parallelise, do not cache.

## Test plan

Added to `crates/mmd-engine/tests/separation.rs`. Imports to add:
`use mmd_engine::scenario::Cell; use mmd_engine::sim::{CollisionParams, MAX_SEPARATION_NEIGHBORS, SPEED_CELLS_PER_SEC, TICK_DT, accumulate_separation}; use mmd_engine::testkit::{FIXTURE_DENSE_V1, GridSpec, Harness};`

| Test | Input | Expect |
| ---- | ----- | ------ |
| `separation_of_a_pair_is_equal_and_opposite` | 2 agents both at `(2.5, 2.5)`, `SpatialGrid::new(8, 8, 1.0, 2)` rebuilt, `accumulate_separation(.., 0.5, ..)` | `sep_x[0] == -sep_x[1]` and `sep_y[0] == -sep_y[1]` exactly; `sep_x[0] != 0.0 \|\| sep_y[0] != 0.0` |
| `separation_is_capped_at_eight_neighbours` | 64 agents all at `(2.5, 2.5)`, radius `0.5` | for every `i`, `0.0 < hypot(sep_x[i], sep_y[i]) <= MAX_SEPARATION_NEIGHBORS as f32 + 1e-4` |
| `separation_ignores_agents_beyond_contact` | agents at `(1.5, 1.5)` and `(5.5, 1.5)`, radius `0.5` (contact 1.0) | both sep components are exactly `0.0` |
| `coincident_agents_separate_on_the_first_tick` | `GridSpec::new(32, 32, Cell { x: 31, y: 16 }).with_spawns(vec![Cell { x: 1, y: 16 }]).with_collision(128, 256)`, 2 agents, `step_exact(1)` | the two positions differ: `(x0 - x1).abs() + (y0 - y1).abs() > 1e-4` |
| `separation_is_reproducible` | the same 32-agent collision grid built twice, each stepped 120 ticks | equal `state_hash()`; a third run with `.seed(7)` differs |
| `separation_keeps_the_step_length` | 8 agents coincident on an open collision grid, one tick, agents that did not recycle | each moved distance is within `1e-4` of `SPEED_CELLS_PER_SEC * TICK_DT` — bending the heading must not change the speed |
| `a_released_stack_spreads_apart` | 16 agents coincident on a 32×32 collision grid (radius `128` = 0.5 cell, contact 1.0), 90 ticks | mean pairwise distance rises from `0.0` to `> 0.5`; no position is non-finite |
| `a_bodyless_scenario_walks_the_flow_only_path` | two runs of `GridSpec::new(24, 24, Cell { x: 23, y: 12 }).with_spawns(vec![Cell { x: 1, y: 12 }]).with_agents(32)` — one built plainly, one built with `.with_collision(0, 0)` — each stepped 200 ticks | equal `state_hash()`; and `h.sim().collision().enabled()` is `false` for both |
| `a_fixture_scenario_reports_its_body` | `Harness::fixture(FIXTURE_DENSE_V1)` | `h.sim().collision().enabled()` is `true`; `(h.sim().collision().radius_cells - 0.125).abs() < 1e-6` |

Added to `crates/mmd-engine/tests/frame_allocations.rs`:

| Test | Input | Expect |
| ---- | ----- | ------ |
| `a_collision_tick_allocates_nothing` | `Harness::fixture(FIXTURE_DENSE_V1)` built and warmed with 2 ticks **outside** the guard, then 10 ticks inside `MeasureGuard::enter()` | `guard.allocations() == 0`; `guard.assert_zero()` does not panic |

```rust
#[test]
fn a_collision_tick_allocates_nothing() {
    let _lock = lock_alloc_tests();
    reset_count();

    let mut h = Harness::fixture(FIXTURE_DENSE_V1).build().expect("dense");
    assert!(h.sim().collision().enabled(), "fixture must have a body");
    h.step_exact(2); // warm-up outside the measured scope

    let guard = MeasureGuard::enter();
    h.step_exact(10);
    std::hint::black_box(h.tick_index());
    assert_eq!(guard.allocations(), 0);
    guard.assert_zero();
}
```

## Impl steps

- [ ] 1. Append the nine tests above to `crates/mmd-engine/tests/separation.rs` and `crates/mmd-engine/tests/frame_allocations.rs`; run `cargo test -p mmd-engine --test separation` and confirm it fails to compile (red).
- [ ] 2. Create `crates/mmd-engine/src/sim/collision.rs` with the module contents given above, verbatim.
- [ ] 3. Replace `crates/mmd-engine/src/sim/mod.rs` with the version given above.
- [ ] 4. In `crates/mmd-engine/src/sim/agents.rs`, add `use super::collision::CollisionParams;` and `use super::spatial::SpatialGrid;` to the imports.
- [ ] 5. Append the four new fields (`collision`, `grid`, `sep_x`, `sep_y`) to `struct Simulation`.
- [ ] 6. Add `collision: CollisionParams` as the **last** parameter of `Simulation::new_custom`, and document it in the fn doc comment.
- [ ] 7. Insert the `grid` / `sep_x` / `sep_y` construction block given above before the `Self { .. }` literal, and add the four fields to that literal.
- [ ] 8. Pass `CollisionParams::from_scenario(scenario)` as the final argument of the `Self::new_custom(..)` call inside `Simulation::new`.
- [ ] 9. Add the `pub fn collision(&self) -> CollisionParams` accessor.
- [ ] 10. In `crates/mmd-engine/src/runtime.rs`, change the import to `use crate::sim::{CollisionParams, Simulation};` and add `CollisionParams::from_scenario(&scenario),` as the final argument of the `Simulation::new_custom` call at line ~124.
- [ ] 11. In `crates/mmd-engine/src/sim/tick.rs`, add the `BLEND_EPS2` const.
- [ ] 12. Insert the separation pass (grid rebuild + `accumulate_separation`) after the hoists and before the agent loop.
- [ ] 13. Replace the move/commit block with the blended version given above, including the flow-only fallback.
- [ ] 14. Run `cargo test -p mmd-engine --test separation` → all 15 tests in the file pass.
- [ ] 15. Run `cargo test -p mmd-engine --test frame_allocations` → 7 passed.
- [ ] 16. Run `cargo test -p mmd-engine --test simulation` **before** renaming anything, and record which tests fail. Expect `aggregate_progress_is_monotone` to fail; anything else failing means the escalation rule applies.
- [ ] 17. Apply the `longest_progress_stall` helper to `crates/mmd-engine/tests/common/mod.rs`.
- [ ] 18. Rename `aggregate_progress_is_monotone` to `aggregate_progress_never_stalls` and replace its body verbatim.
- [ ] 19. Run `cargo test -p mmd-engine --test simulation` again → green. If not, follow the escalation rule; never edit a threshold.
- [ ] 20. Run `cargo run -- run --agents 2000 --frames 120 | grep 'clean exit'` and confirm the `hash=` value **differs** from the one recorded in T2 — the behaviour flip must be observable.
- [ ] 21. Run the full validation list below.

## Outputs

- Files touched: `crates/mmd-engine/src/sim/collision.rs` (new), `crates/mmd-engine/src/sim/mod.rs`, `crates/mmd-engine/src/sim/agents.rs`, `crates/mmd-engine/src/sim/tick.rs`, `crates/mmd-engine/src/runtime.rs`, `crates/mmd-engine/tests/separation.rs`, `crates/mmd-engine/tests/frame_allocations.rs`, `crates/mmd-engine/tests/simulation.rs`, `crates/mmd-engine/tests/common/mod.rs`.
- Public API added (quoted verbatim by T5 and T6):
  - `mmd_engine::sim::CollisionParams { pub radius_cells: f32, pub strength: f32 }`
  - `CollisionParams::NONE`, `from_q8(u32, u32)`, `from_scenario(&Scenario)`, `enabled(&self) -> bool`, `bin_size_cells(&self) -> f32`
  - `mmd_engine::sim::MAX_SEPARATION_NEIGHBORS: usize = 8`
  - `mmd_engine::sim::COINCIDENT_EPS2: f32`
  - `mmd_engine::sim::SEPARATION_DIR16: [(f32, f32); 16]`
  - `mmd_engine::sim::accumulate_separation(&[f32], &[f32], &SpatialGrid, f32, &mut [f32], &mut [f32])`
  - `Simulation::collision(&self) -> CollisionParams`
  - `Simulation::new_custom(.., collision: CollisionParams)` — signature changed, last parameter added
  - `Tracker::longest_progress_stall(&self) -> u64` (test-only helper)
- Behaviour change: **yes, deliberately.** Every scenario with a nonzero body radius now walks a blended heading. The gate-scene state hash changes.
- Migrate / config: none.

## Validation

- [ ] `cargo test -p mmd-engine --test separation` → 15 passed
- [ ] `cargo test -p mmd-engine --test frame_allocations` → 7 passed
- [ ] `cargo test -p mmd-engine --test simulation` → green, including the unmodified `no_agent_is_stuck_against_an_obstacle`, `obstacles_are_never_entered`, `agents_reach_destination`, `no_group_is_starved`, `alive_count_is_stable`, `determinism_holds_for_50k_agents`
- [ ] `cargo fmt --all -- --check` → exit 0
- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings` → exit 0
- [ ] `MMD_REQUIRE_GPU=1 cargo test --workspace --locked` → green
- [ ] `cargo run -p xtask -- bootstrap --check` / `shaders --check` / `atlases --check` → exit 0
- [ ] `cargo run -- run --agents 50000 --frames 300` → exit 0, `run: clean exit ...` printed
- [ ] manual check: watch the window with `cargo run -- run --agents 50000` — the horde must visibly spread out of its spawn stacks instead of moving as 127 point-like columns. Press Esc to quit.
- [ ] manual check: the `hash=` from impl step 20 differs from the T2 value
- [ ] app functional — no broken path from this slice
- [ ] commit msg draft: `feat(sim): steer agents apart with soft separation so zombies stop overlapping`
