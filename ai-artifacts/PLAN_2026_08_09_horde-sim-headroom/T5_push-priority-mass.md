# T5: Per-agent push priority

**Plan:** `./ai-artifacts/PLAN_2026_08_09_horde-sim-headroom.md`
**Depends:** T1
**Commit outcome:** Repulsion is weighted by a per-agent push priority, so a
dense goal sink breaks its own symmetry; `collision_sprite_v1` runs two classes;
every scene at one class keeps a bit-identical state hash.

## Context (self-contained)

- Goal: buy simulation headroom in the per-agent neighbour scan
  (`crates/mmd-engine/src/sim/collision.rs`) without spending a behavioural
  guarantee. Five changes land over T2–T6.
- This slice: the deadlock escape hatch. *March of the Froblins* names the bug
  precisely — agents pile at a small goal and *"become unable to navigate out of
  the goal area."* The shipped-RTS answer is asymmetric push, not a better
  solver: StarCraft II 5.0.15's patch notes read *"Increased allied push
  priority for Thors and Siege Tanks."* A per-agent priority byte multiplying
  the repulsion is far cheaper than any reciprocal-velocity solver, and it is
  the mechanism Phase 1 will hang unit types on.
- Out of scope here: threading (T6), the flow field, the renderer, shaders. Do
  not touch the neighbour cap, the 3×3 window, the coincidence tie-break table,
  or the linear falloff. **No performance number in any doc, test name or commit
  message** — perf gating is retired; this ticket is accepted on behaviour.
- Assumptions in force:
  - `mass_class_count == 1` must be **bit-identical** to the pre-ticket engine.
    Every mass is `1`, every reciprocal is `1.0`, and IEEE multiplication by
    exactly `1.0` is exact — so the pinned digest cannot move. If it does, the
    scale factor is being computed wrong.
  - Class assignment is `mass[i] = (i % classes) + 1`, deterministic and
    index-derived. Phase 1 replaces this with per-unit-type mass; the field is
    the thing being built now, the assignment rule is a placeholder and says so
    in its doc comment.
  - This is still steering. Nothing here conserves momentum and nothing here
    forbids an overlap — ADR 009 stands unchanged.

## Requirements

- `Simulation` carries `mass: Vec<u8>` and `inv_mass: Vec<f32>`, both filled at
  construction, never written per tick, never reallocated.
- The push on agent `i` from neighbour `j` is scaled by
  `mass[j] as f32 * inv_mass[i]`. Heavy neighbours push harder; heavy agents are
  pushed less.
- The scale applies to **both** arms of the accumulation — the linear-falloff
  arm and the coincidence tie-break arm.
- At `mass_class_count == 1` the scale is exactly `1.0` and `BODIED_STACK_HASH`
  does not move.
- `collision_sprite_v1` declares `mass_class_count: 2` and its sidecar is
  regenerated.

## Inputs

- **Inherited from T0 — the pinned gate digest (do not recompute, do not
  re-derive):**
  `hash=864147ca3a0e09f7ebc5762b778fce193e705a2bc943ceaf67acf087581ee881`
  produced by `cargo run -- run --agents 5000 --frames 300` on commit `df309d3`.
  Every ticket after T0 must reproduce this value byte for byte. A different
  `hash=` means the plan's core invariant is broken — stop and report `failed`
  rather than re-pinning it. The pre-T0 value
  `f647e7f590ed5814e4e61388e23836dfacb980217fb1762542ec3abfe85549b3` is
  superseded and must never reappear.
  T0 also delivered: `MAX_LIVE_AGENTS = 5_000` enforced by a single
  `check_population` at the validator dispatcher (not per family),
  `COLLISION_SCENE_MAX_AGENTS` removed, the three tracked scenes retuned to
  48 px sprites / `collision_radius_q8: 1_536` with fresh `.sha256` sidecars,
  the four `fixture_*` scenes byte-identical, `MIN_SCANNED_TESTS` at `174`, and
  `BenchPolicy::test_short()` split onto its own `test-short-v1` ladder while
  `production()` keeps the frozen phase-0 ladder verbatim.

- `crates/mmd-engine/src/sim/agents.rs` — `struct Simulation` (line ~33),
  `Simulation::new_custom` (line ~91), which already receives
  `collision: CollisionParams` and builds `sep_x` / `sep_y` as
  `vec![0.0; agent_count]`.
- `crates/mmd-engine/src/sim/collision.rs` — `accumulate_separation_phase` and
  its wrapper `accumulate_separation`. The two accumulation arms are:
  ```rust
  if d2 <= COINCIDENT_EPS2 {
      let (ux, uy) = SEPARATION_DIR16[(i ^ j) & 15];
      let sign = if i < j { 1.0 } else { -1.0 };
      sx += sign * ux;
      sy += sign * uy;
  } else {
      let d = d2.sqrt();
      let w = (contact - d) * inv_contact;
      let inv_d = 1.0 / d;
      sx += dx * inv_d * w;
      sy += dy * inv_d * w;
  }
  ```
- `crates/mmd-engine/src/sim/tick.rs` — the `if collision_on` block that calls
  `accumulate_separation_phase`.
- `crates/mmd-engine/tests/separation.rs` — the four tests that call
  `accumulate_separation` directly:
  `separation_of_a_pair_is_equal_and_opposite`,
  `separation_is_capped_at_eight_neighbours`,
  `separation_ignores_agents_beyond_contact`,
  `a_lone_agent_accumulates_no_repulsion`.
- `assets/scenarios/collision_sprite_v1.ron` + `.sha256`.
- **From Depends (T1; T2, T3 and T4 have also landed):**
  - `CollisionParams` is
    `{ radius_cells: f32, strength: f32, phases: u32, mass_classes: u32, threads: u32 }`;
    `mass_classes` is the value this ticket consumes, validated to `1..=8` and
    forced to `1` on a bodyless scene and on `technical_prototype_v1`.
  - `Scenario::mass_class_count() -> u32` and
    `GridSpec::with_mass_classes(u32)` exist.
  - The scan lives in
    `pub fn accumulate_separation_phase(x, y, grid, radius_cells, phases, phase, sep_x, sep_y)`,
    whose per-agent loop is
    `let step = phases as usize; let mut i = phase as usize; while i < n { … i += step; }`
    and which early-outs when the 3×3 window holds one agent.
    `pub fn accumulate_separation(x, y, grid, radius_cells, sep_x, sep_y)` wraps
    it with `phases = 1, phase = 0`.
  - The scan walks rows via `grid.agents_in_bin_row(bx0, bx1, cy)`.
  - `#[cfg(feature = "testkit")] Simulation::separation_of(index) -> (f32, f32)`
    exists — this ticket's asymmetry test uses it.
  - Pinned digest that must not move, in
    `crates/mmd-engine/tests/separation.rs`:
    `BODIED_STACK_HASH = "81f958301624ff2033e797032d7cff8fd59344036263fdc5c6e13f00b5c80e8b"`.

## TDD

1. **Red** — write the four tests below. Three fail to compile (`mass_of`,
   `with_mass_classes` reaching the sim); `mass_classes_change_the_bodied_digest`
   fails because nothing changes the digest yet.
2. **Green** — add the two SoA vectors, thread the slices through the scan, apply
   the scale, then set the sprite scene's class count.
3. **Refactor** — none. Do not restructure the two accumulation arms; add one
   factor to each.

## Test plan

| Test | Input | Expect |
| ---- | ---- | ---- |
| `one_mass_class_leaves_every_agent_equal` | `stacked_collision_grid(32)` (defaults to 1 class), 200 ticks | `sim().mass_of(i) == 1` for every `i`, and `state_hash_hex() == BODIED_STACK_HASH` |
| `mass_is_assigned_round_robin_by_index` | `stacked_collision_grid(9).with_mass_classes(3)` | `mass_of(0..9) == [1, 2, 3, 1, 2, 3, 1, 2, 3]` |
| `a_heavier_neighbour_pushes_a_lighter_one_harder` | 32×32 grid, `.with_collision(128, 256).with_mass_classes(2).with_agents(2)`, both agents placed 0.1 cells apart with `set_position`, one tick | `hypot(separation_of(0)) > hypot(separation_of(1))` — agent 0 is class 1, agent 1 is class 2 |
| `mass_classes_change_the_bodied_digest` | `stacked_collision_grid(32).with_mass_classes(2)`, 200 ticks | `state_hash_hex() != BODIED_STACK_HASH` — proves the feature is not a no-op |
| `sprite_scene_pulls_agents_out_of_deep_overlap` | unchanged, now with 2 classes | still green — see the risk note below |
| `a_bodied_scenario_is_pinned_to_a_golden_digest` | unchanged | still green, unedited |
| `a_bodyless_scenario_walks_the_flow_only_path` | unchanged | still green, unedited |
| `separation_of_a_pair_is_equal_and_opposite` | call site gains two args, **assertions unchanged** | still green |
| `separation_is_capped_at_eight_neighbours` | call site gains two args, **assertions unchanged** | still green |
| `separation_ignores_agents_beyond_contact` | call site gains two args, **assertions unchanged** | still green |
| `a_lone_agent_accumulates_no_repulsion` | call site gains two args, **assertions unchanged** | still green |

**Risk, and what to do about it (step 10).** Putting two classes on
`collision_sprite_v1` changes how that scene unpacks, and
`sprite_scene_pulls_agents_out_of_deep_overlap` is a statistical claim about it.
If that test goes red, do **not** weaken it. Revert
`collision_sprite_v1.ron` to `mass_class_count: 1`, restore its sidecar, and
prove the feature on the inline grid tests alone. Record the reversal in the
commit body. The feature is the deliverable; the demo scene is not.

## Impl steps

- [ ] 1. In `crates/mmd-engine/src/sim/agents.rs`, add two fields to
      `struct Simulation`, after `sep_y`:
      ```rust
      /// Push priority per agent, `1..=mass_class_count`. A heavier neighbour
      /// pushes harder and is itself pushed less, which is what breaks the
      /// symmetry of a jam at the goal.
      ///
      /// Assignment is `(i % classes) + 1` — a deterministic placeholder.
      /// Phase 1 replaces it with per-unit-type mass; the storage is what is
      /// being built here.
      pub(super) mass: Vec<u8>,
      /// `1.0 / mass[i]`, precomputed so the scan spends one multiply per pair
      /// rather than a divide.
      pub(super) inv_mass: Vec<f32>,
      ```
- [ ] 2. In `new_custom`, next to `let sep_x = vec![0.0; agent_count];`, build
      both:
      ```rust
      let classes = collision.mass_classes.max(1) as usize;
      let mut mass = Vec::with_capacity(agent_count);
      let mut inv_mass = Vec::with_capacity(agent_count);
      for i in 0..agent_count {
          let m = ((i % classes) + 1) as u8;
          mass.push(m);
          inv_mass.push(1.0 / m as f32);
      }
      ```
      and add `mass,` and `inv_mass,` to the returned struct literal.
- [ ] 3. Add a testkit-gated accessor to `impl Simulation`, beside
      `separation_of`:
      ```rust
      /// This agent's push priority.
      #[cfg(feature = "testkit")]
      pub fn mass_of(&self, index: usize) -> u8 {
          self.mass[index]
      }
      ```
- [ ] 4. In `crates/mmd-engine/src/sim/collision.rs`, add two parameters to
      `accumulate_separation_phase`, immediately after `radius_cells`:
      `mass: &[u8]`, `inv_mass: &[f32]`. Add
      `debug_assert_eq!(mass.len(), n);` and
      `debug_assert_eq!(inv_mass.len(), n);` beside the existing length
      assertions.
- [ ] 5. Inside the per-agent loop, after `let py = y[i];`, add
      `let inv_mi = inv_mass[i];`.
- [ ] 6. Inside the neighbour loop, after `if j == i { continue; }` and the
      `d2 >= contact2` skip, add `let scale = mass[j] as f32 * inv_mi;` and
      apply it to both arms:
      ```rust
      sx += sign * ux * scale;
      sy += sign * uy * scale;
      ```
      and
      ```rust
      sx += dx * inv_d * w * scale;
      sy += dy * inv_d * w * scale;
      ```
      Append the factor at the **end** of each expression; do not reassociate
      the existing products, or the one-class path stops being bit-exact.
- [ ] 7. Update the doc comment on `accumulate_separation_phase`: add — *"Each
      contribution is scaled by `mass[j] / mass[i]`. With one class every mass
      is 1 and the scale is exactly 1.0, so the pass is bit-identical to the
      unweighted one."*
- [ ] 8. Add the same two parameters to the `accumulate_separation` wrapper,
      after `radius_cells`, and forward them.
- [ ] 9. In `crates/mmd-engine/src/sim/tick.rs`, pass `&sim.mass` and
      `&sim.inv_mass` in the `accumulate_separation_phase` call, after
      `collision.radius_cells`. Rust's borrow checker allows this: `mass` and
      `inv_mass` are shared borrows while `sep_x` / `sep_y` are the only
      mutable ones.
- [ ] 10. Edit `assets/scenarios/collision_sprite_v1.ron`: change
      `mass_class_count: 1,` to `mass_class_count: 2,`. Regenerate its sidecar:
      ```sh
      sha256sum assets/scenarios/collision_sprite_v1.ron | cut -d' ' -f1 \
        > assets/scenarios/collision_sprite_v1.sha256
      ```
      If `sprite_scene_pulls_agents_out_of_deep_overlap` goes red, apply the
      fallback in the risk note above rather than editing the test.
- [ ] 11. Update the four direct `accumulate_separation` call sites in
      `crates/mmd-engine/tests/separation.rs` to pass a mass slice of `1`s and a
      reciprocal slice of `1.0`s of the right length. **Change no assertion in
      those four tests** — their expected values must survive untouched, which
      is what proves the one-class path is the identity.
- [ ] 12. Add the four new tests from the test plan to
      `crates/mmd-engine/tests/separation.rs`, after
      `a_bodied_scenario_is_pinned_to_a_golden_digest`.
- [ ] 13. Run validation. If `a_bodied_scenario_is_pinned_to_a_golden_digest`
      moves, step 6 reassociated an expression — do **not** re-measure the
      digest.

## Outputs

- Touched: `crates/mmd-engine/src/sim/agents.rs`,
  `crates/mmd-engine/src/sim/collision.rs`,
  `crates/mmd-engine/src/sim/tick.rs`,
  `crates/mmd-engine/tests/separation.rs`,
  `assets/scenarios/collision_sprite_v1.ron` + `.sha256`.
- Public API changed: `accumulate_separation` and
  `accumulate_separation_phase` each take `mass: &[u8]` and `inv_mass: &[f32]`
  after `radius_cells`. Added:
  `#[cfg(feature = "testkit")] Simulation::mass_of(index) -> u8`.
- Behaviour change: only for a scenario declaring `mass_class_count > 1` — today
  that is `collision_sprite_v1` alone.
- Memory: two vectors of `agent_count`, allocated once at construction.

## Validation

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo test --workspace --locked` — green
- [ ] `cargo test -p mmd-engine --test separation` — green, with
      `a_bodied_scenario_is_pinned_to_a_golden_digest` and
      `a_bodyless_scenario_walks_the_flow_only_path` **unedited**
- [ ] `cargo test -p mmd-engine --test frame_allocations` — green, including
      `a_collision_tick_allocates_nothing`
- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- [ ] `cargo build -p mmd-engine --no-default-features --features gpu`
- [ ] `cargo run -- run --agents 5000 --frames 300` — exits 0
- [ ] the gate smoke `hash=` equals the T0 pinned digest, byte for byte
- [ ] `graphify update .` run (graph refresh; `graphify-out/` is gitignored)
- [ ] `cargo run -- run --scenario assets/scenarios/collision_sprite_v1.ron --frames 300` — exits 0
- [ ] manual check — watch the sprite scene; the stack must still open, and no
      agent may end up shoved inside a wall
- [ ] `nix flake check`
- [ ] app functional — every scene loads, ticks and exits
- [ ] commit msg draft: `feat(sim): weight separation by a per-agent push priority`
