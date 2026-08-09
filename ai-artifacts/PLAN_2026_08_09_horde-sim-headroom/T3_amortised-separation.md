# T3: Amortised separation

**Plan:** `./ai-artifacts/PLAN_2026_08_09_horde-sim-headroom.md`
**Depends:** T1
**Commit outcome:** The separation scan and the grid rebuild are spread over the
scenario's `separation_phases` ticks; `collision_mid_v1` runs at 4 phases; every
scene still pinned to 1 phase keeps a bit-identical state hash.

## Context (self-contained)

- Goal: buy simulation headroom in the per-agent neighbour scan
  (`crates/mmd-engine/src/sim/collision.rs`) without spending a behavioural
  guarantee. Five changes land over T2–T6.
- This slice: the highest-leverage one. Reynolds' *Big Fast Crowds on PS3*
  (2006) ran 15 000 agents recomputing steering once every 8–10 frames and
  reusing the result in between. `sep_x` / `sep_y` are already persistent
  `Vec<f32>` fields on `Simulation`, so a skipped agent already reads exactly
  what it would have read — the change is which agents recompute, and when.
- Out of scope here: `sim/spatial.rs` internals, the push-priority mass byte
  (T5), threading (T6), the renderer, shaders, the flow field. **No performance
  number in any doc, test name or commit message** — perf gating is retired.
  This ticket is accepted on behaviour: identity at 1 phase, bounded staleness
  above it, obstacles still never entered.
- Assumptions in force:
  - `separation_phases == 1` must be **bit-identical** to the pre-ticket engine.
    The pinned digest is the proof.
  - Staleness is accepted and bounded. At 4 phases a neighbour position is at
    most 3 ticks old. ADR 009 already forbids any zero-overlap claim, so nothing
    written is weakened. A scene that cannot tolerate it pins `1` — which every
    scene except `collision_mid_v1` does.
  - The grid rebuild drops to the same cadence as the scan, not a separate one.
    One cadence, one knob.
  - Bucketing is **strided** (`i % phases`), not blocked. Agent index correlates
    with spawn cell, so a blocked split would refresh one region of the crowd at
    a time and read as a wave; a stride spreads the refresh uniformly.

## Requirements

- `Simulation::tick` rebuilds the grid only when
  `tick_index % separation_phases == 0`.
- Only agents with `i % separation_phases == tick_index % separation_phases`
  recompute their repulsion; every other `sep_x[i]` / `sep_y[i]` is left exactly
  as it was.
- At `separation_phases == 1` both rules degenerate to today's behaviour and the
  state hash is unchanged.
- `accumulate_separation`'s existing 6-argument signature keeps working — it is
  called directly by `crates/mmd-engine/tests/separation.rs`.
- `collision_mid_v1` declares `separation_phases: 4` and its sidecar is
  regenerated. It must still pass
  `collision_scene_agents_never_enter_an_obstacle`.

## Inputs

- `crates/mmd-engine/src/sim/tick.rs` — `pub fn step(sim: &mut Simulation)`. The
  block to change is lines ~35–47:
  ```rust
  let collision = sim.collision;
  let collision_on = collision.enabled();
  if collision_on {
      sim.grid.rebuild(&sim.x, &sim.y);
      super::collision::accumulate_separation(
          &sim.x, &sim.y, &sim.grid, collision.radius_cells,
          &mut sim.sep_x, &mut sim.sep_y,
      );
  }
  ```
  `sim.tick_index` is incremented at the **end** of `step`
  (`sim.tick_index = sim.tick_index.wrapping_add(1);`), so inside the block it
  still holds the index of the tick being computed. Tick 0 therefore rebuilds.
- `crates/mmd-engine/src/sim/collision.rs` — `pub fn accumulate_separation(...)`
  at line ~122, whose per-agent loop is `for i in 0..n { … }`.
- `crates/mmd-engine/src/sim/agents.rs` — `struct Simulation`; `sep_x`, `sep_y`
  and `grid` are `pub(super)`, `tick_index` is `pub(super)` with a public
  `tick_index()` accessor.
- `crates/mmd-engine/tests/separation.rs` — `stacked_collision_grid(agents)`
  helper (line ~244), `mean_pairwise_distance(h)` (line ~254),
  `mid_scene_reports_its_tuning` (line ~745),
  `collision_scene_agents_never_enter_an_obstacle` (line ~765).
- `assets/scenarios/collision_mid_v1.ron` + `.sha256`.
- **From Depends (T1, and T2 which also landed):**
  - `CollisionParams` is now
    `{ radius_cells: f32, strength: f32, phases: u32, mass_classes: u32, threads: u32 }`.
    `CollisionParams::NONE` sets all three counts to `1`.
    `CollisionParams::from_scenario` reads them from the scenario.
    `sim.collision` is a `CollisionParams` and `collision.phases` is the value
    this ticket consumes.
  - `Scenario::separation_phases() -> u32`, validated to `1..=16`, required in
    every `.ron`, rejected as anything but `1` on a bodyless scene and on
    `technical_prototype_v1`.
  - `GridSpec::with_separation_phases(u32)` exists for inline harness grids.
  - `SpatialGrid::bin_count(bx, by) -> u32` exists (T2). **T3 does not use it.**
  - These digests are pinned in `crates/mmd-engine/tests/separation.rs` and must
    not move:
    `BODIED_STACK_HASH = "81f958301624ff2033e797032d7cff8fd59344036263fdc5c6e13f00b5c80e8b"`,
    `BODYLESS_GRID_PRE_SEPARATION_HASH`.

## TDD

1. **Red** — write the four tests below first. Three fail to compile
   (`separation_of`, `grid_rebuild_count`, `with_separation_phases` on a
   `Harness`), one (`an_amortised_stack_still_spreads`) fails because nothing is
   amortised yet and the digest assertion in it is wrong.
2. **Green** — add `accumulate_separation_phase`, the two testkit accessors, the
   `step` change, then the scenario data change.
3. **Refactor** — keep `accumulate_separation` as a thin wrapper. Do not delete
   it; tests call it directly.

## Test plan

| Test | Input | Expect |
| ---- | ---- | ---- |
| `separation_phases_of_one_is_the_identity` | `stacked_collision_grid(32)` built twice — once default, once `.with_separation_phases(1)` — 200 ticks each | `state_hash()` equal, and both equal `BODIED_STACK_HASH` |
| `an_amortised_agent_keeps_its_repulsion_between_phases` | `stacked_collision_grid(32).with_separation_phases(4)` | after 1 tick `sim().separation_of(1) == (0.0, 0.0)`; after a 2nd tick it is non-zero |
| `the_grid_rebuilds_once_per_phase_cycle` | same spec, 8 ticks | `sim().grid_rebuild_count() == 2` |
| `an_amortised_stack_still_spreads` | `stacked_collision_grid(64).with_separation_phases(4)`, 200 ticks | `mean_pairwise_distance` after > `mean_pairwise_distance` at tick 0 |
| `mid_scene_reports_its_tuning` | **extend** the existing test | additionally asserts `scenario().separation_phases() == 4` |
| `collision_scene_agents_never_enter_an_obstacle` | unchanged | still green with the mid scene now amortised — **the load-bearing safety check** |
| `a_bodied_scenario_is_pinned_to_a_golden_digest` | unchanged | still green, unedited |
| `a_bodyless_scenario_walks_the_flow_only_path` | unchanged | still green, unedited |

Note on `an_amortised_agent_keeps_its_repulsion_between_phases`: agent `1` is in
phase `1`, so it recomputes on ticks where `tick_index % 4 == 1` — that is tick
1, the **second** tick. `sep` starts at `(0.0, 0.0)` from construction, so the
assertion pair is exact and needs no tolerance.

## Impl steps

- [ ] 1. In `crates/mmd-engine/src/sim/collision.rs`, rename the existing
      `accumulate_separation` body into a new public function and add the two
      phase parameters:
      ```rust
      /// Write the repulsion sum for the agents of `phase` into `sep_x` /
      /// `sep_y`, leaving every other entry exactly as it was.
      ///
      /// An agent belongs to `i % phases`. The stride, rather than a contiguous
      /// block, is deliberate: agent index correlates with spawn cell, so a
      /// blocked split would refresh one region of the crowd at a time.
      ///
      /// `phases == 1` visits every agent and is bit-identical to the
      /// unamortised pass.
      ///
      /// # Panics
      /// If `phases == 0` or `phase >= phases`.
      #[allow(clippy::too_many_arguments)]
      pub fn accumulate_separation_phase(
          x: &[f32],
          y: &[f32],
          grid: &SpatialGrid,
          radius_cells: f32,
          phases: u32,
          phase: u32,
          sep_x: &mut [f32],
          sep_y: &mut [f32],
      ) { /* … */ }
      ```
- [ ] 2. Inside it, keep every existing `debug_assert!`, then add
      `assert!(phases > 0, "phases must be >= 1");` and
      `assert!(phase < phases, "phase {phase} out of range for {phases}");`.
- [ ] 3. Replace the loop header `for i in 0..n {` with:
      ```rust
      let step = phases as usize;
      let mut i = phase as usize;
      while i < n {
      ```
      and replace the loop's closing brace with:
      ```rust
          i += step;
      }
      ```
      Change nothing inside the body — not the 3×3 window, not the cap, not the
      tie-break, not the falloff.
- [ ] 4. Re-add the old entry point as a wrapper, so existing callers compile
      unchanged:
      ```rust
      /// Write every agent's repulsion sum. Equivalent to
      /// [`accumulate_separation_phase`] with `phases = 1, phase = 0`.
      pub fn accumulate_separation(
          x: &[f32],
          y: &[f32],
          grid: &SpatialGrid,
          radius_cells: f32,
          sep_x: &mut [f32],
          sep_y: &mut [f32],
      ) {
          accumulate_separation_phase(x, y, grid, radius_cells, 1, 0, sep_x, sep_y);
      }
      ```
- [ ] 5. Export the new function wherever `accumulate_separation` is re-exported
      — check `crates/mmd-engine/src/sim/mod.rs` and add
      `accumulate_separation_phase` beside it.
- [ ] 6. In `crates/mmd-engine/src/sim/agents.rs`, add a counter field to
      `struct Simulation`, after `tick_index`:
      ```rust
      /// Grid rebuilds performed since construction. Amortisation makes this
      /// diverge from `tick_index`, and a test needs to see that it did.
      pub(super) grid_rebuilds: u64,
      ```
      Initialise it to `0` in `new_custom`'s struct literal.
- [ ] 7. In the same file, add two testkit-gated accessors to `impl Simulation`,
      next to `collision()`:
      ```rust
      /// This agent's stored repulsion sum. Amortisation makes it outlive the
      /// tick that computed it, and a test needs to see that it did.
      #[cfg(feature = "testkit")]
      pub fn separation_of(&self, index: usize) -> (f32, f32) {
          (self.sep_x[index], self.sep_y[index])
      }

      /// Grid rebuilds performed since construction.
      #[cfg(feature = "testkit")]
      pub fn grid_rebuild_count(&self) -> u64 {
          self.grid_rebuilds
      }
      ```
- [ ] 8. In `crates/mmd-engine/src/sim/tick.rs`, replace the `if collision_on`
      block with:
      ```rust
      if collision_on {
          // One cadence for both halves of the pass. Reynolds' `skipThink`
          // recomputes less, it does not spread the same work thinner — the
          // scan that is skipped is never run.
          let phases = collision.phases.max(1) as u64;
          let phase = sim.tick_index % phases;
          if phase == 0 {
              sim.grid.rebuild(&sim.x, &sim.y);
              sim.grid_rebuilds += 1;
          }
          super::collision::accumulate_separation_phase(
              &sim.x,
              &sim.y,
              &sim.grid,
              collision.radius_cells,
              phases as u32,
              phase as u32,
              &mut sim.sep_x,
              &mut sim.sep_y,
          );
      }
      ```
- [ ] 9. Update the doc comment on `pub fn step` in `tick.rs`: after the
      existing "no neighbour is ever queried" sentence add — *"When the scenario
      spreads the pass over several ticks, an agent outside this tick's phase
      keeps the repulsion it was last given; the grid it was measured against is
      rebuilt once per cycle, so a neighbour position can be up to
      `separation_phases - 1` ticks old."*
- [ ] 10. Edit `assets/scenarios/collision_mid_v1.ron`: change
      `separation_phases: 1,` to `separation_phases: 4,`. Leave
      `mass_class_count` and `separation_threads` at `1`.
- [ ] 11. Regenerate that one sidecar:
      ```sh
      sha256sum assets/scenarios/collision_mid_v1.ron | cut -d' ' -f1 \
        > assets/scenarios/collision_mid_v1.sha256
      ```
- [ ] 12. Add the four new tests to `crates/mmd-engine/tests/separation.rs`
      after `a_released_stack_spreads_apart`, and extend
      `mid_scene_reports_its_tuning` with the `separation_phases() == 4`
      assertion.
- [ ] 13. Run validation. If `a_bodied_scenario_is_pinned_to_a_golden_digest`
      moves, the `phases == 1` path is not the identity — do **not** re-measure;
      fix steps 3 and 8.

## Outputs

- Touched: `crates/mmd-engine/src/sim/collision.rs`,
  `crates/mmd-engine/src/sim/tick.rs`,
  `crates/mmd-engine/src/sim/agents.rs`,
  `crates/mmd-engine/src/sim/mod.rs`,
  `crates/mmd-engine/tests/separation.rs`,
  `assets/scenarios/collision_mid_v1.ron` + `.sha256`.
- Public API added: `sim::accumulate_separation_phase`;
  `Simulation::separation_of` and `Simulation::grid_rebuild_count`, both behind
  `#[cfg(feature = "testkit")]`.
- Behaviour change: only for a scenario declaring `separation_phases > 1` —
  today that is `collision_mid_v1` alone.
- Migration / config: none beyond the one regenerated sidecar.

## Validation

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo test --workspace --locked` — green
- [ ] `cargo test -p mmd-engine --test separation` — green, with
      `a_bodied_scenario_is_pinned_to_a_golden_digest` and
      `a_bodyless_scenario_walks_the_flow_only_path` **unedited**
- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- [ ] `cargo build -p mmd-engine --no-default-features --features gpu`
- [ ] `cargo run -- run --agents 50000 --frames 300` — exits 0
- [ ] `cargo run -- run --scenario assets/scenarios/collision_mid_v1.ron --frames 300` — exits 0
- [ ] manual check — watch the mid scene; the crowd must still open up, with no
      agent frozen inside a wall
- [ ] `nix flake check`
- [ ] app functional — every scene loads, ticks and exits
- [ ] commit msg draft: `feat(sim): spread the separation pass over the scenario's phase count`
