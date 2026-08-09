# T1: Scenario headroom knobs

**Plan:** `./ai-artifacts/PLAN_2026_08_09_horde-sim-headroom.md`
**Depends:** T0
**Commit outcome:** Every scenario declares `separation_phases`,
`mass_class_count` and `separation_threads` at their identity value `1`; the
engine carries them through to `CollisionParams`; not one state hash moves.

## Context (self-contained)

- Goal: buy simulation headroom in the per-agent neighbour scan
  (`crates/mmd-engine/src/sim/collision.rs`) without spending a behavioural
  guarantee. Five changes land over T2–T6; this ticket builds the shelf they sit
  on.
- This slice: contract only. Three new scenario fields, validated and plumbed as
  far as `CollisionParams`. **Nothing reads them yet.** Behaviour is frozen and
  the whole test suite proves it.
- Out of scope here: any change to how the tick behaves; `sim/tick.rs`,
  `sim/spatial.rs` and the body of `accumulate_separation` are not touched.
  No renderer, no shader, no flow field. No performance number in any doc, test
  name or commit message.
- Assumptions in force:
  - Identity default is `1` for all three fields. `1` must mean "exactly what
    the engine did before this plan", and the existing pinned digests are the
    proof.
  - Fields are **required** in the RON (no `#[serde(default)]`), matching the
    `collision_radius_q8` precedent: a stale scenario must fail loudly, never
    run silently untuned.
  - `.sha256` sidecars are bare lowercase hex, trimmed.
  - `graphify` **is** installed. Orient with `graphify query "<question>"`
    before reading source, and run `graphify update .` as the last validation
    step. `graphify-out/` is gitignored, so the refresh never appears in the
    diff.

## Requirements

- `ScenarioSpec` and `Scenario` gain `separation_phases: u32`,
  `mass_class_count: u32`, `separation_threads: u32`.
- Each is rejected outside `1..=` its cap. `0` is rejected explicitly — there is
  no "zero phases".
- A bodyless scenario (`collision_radius_q8 == 0`) must declare all three as
  `1`; tuning a pass that never runs is an authoring mistake.
- `technical_prototype_v1` pins all three to `1` in the same locked-value list
  that already pins `collision_radius_q8`.
- `CollisionParams` carries the three values so `Simulation::new_custom` keeps
  its current arity.
- `GridSpec` gains three builders so inline harness grids can opt in.
- All seven tracked `.ron` scenes declare the fields; all seven `.sha256`
  sidecars are regenerated.
- Every existing test stays green **unmodified**, including
  `a_bodied_scenario_is_pinned_to_a_golden_digest` and
  `a_bodyless_scenario_walks_the_flow_only_path`.

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

- `crates/mmd-engine/src/scenario.rs` — `ScenarioSpec` (line ~102), `Scenario`
  (line ~73), `from_spec` (line ~191), `validate_version_and_dims` (line ~346),
  `validate_collision` (line ~422), `validate_collision_scene_dims` (line ~490).
- `crates/mmd-engine/src/sim/collision.rs` — `CollisionParams` (line ~76).
- `crates/mmd-engine/src/testkit/mod.rs` — `GridSpec` (line ~112),
  `impl From<GridSpec> for ScenarioSpec` (line ~174).
- `crates/mmd-engine/tests/scenario_contract.rs` — where the new tests go.
- Scenes: `assets/scenarios/technical_prototype_v1.ron`,
  `assets/scenarios/collision_mid_v1.ron`,
  `assets/scenarios/collision_sprite_v1.ron`,
  `assets/scenarios/fixtures/fixture_small_v1.ron`,
  `assets/scenarios/fixtures/fixture_corridor_v1.ron`,
  `assets/scenarios/fixtures/fixture_dense_v1.ron`,
  `assets/scenarios/fixtures/fixture_walled_v1.ron`.
- **From Depends (T0):** `MAX_LIVE_AGENTS = 5_000`, the retuned locked
  constants (`V1_HARD_AGENTS`, `V1_STRETCH_AGENTS`, `V1_SPRITE_PX`,
  `V1_COLLISION_RADIUS_Q8`), the three retuned scenes with fresh sidecars, and
  the re-pinned gate digest this ticket must reproduce byte for byte.

**Existing signatures this ticket must preserve** (callers exist in
`crates/mmd-engine/tests/separation.rs` and
`crates/mmd-engine/src/sim/agents.rs`):

```rust
// crates/mmd-engine/src/sim/collision.rs
impl CollisionParams {
    pub const NONE: Self;
    pub fn from_q8(radius_q8: u32, strength_q8: u32) -> Self;
    pub fn from_scenario(scenario: &Scenario) -> Self;
    pub fn enabled(&self) -> bool;
    pub fn bin_size_cells(&self) -> f32;
}
```

`from_q8` keeps its two-argument signature and fills the three new fields with
`1`. Only `from_scenario` reads the scenario's values.

## TDD

1. **Red** — add the seven tests below to
   `crates/mmd-engine/tests/scenario_contract.rs` first. They fail to compile
   (no such field / no such accessor), which is the red state for a contract
   change.
2. **Green** — add the fields, accessors, caps and validation; edit the seven
   `.ron` files; regenerate the seven sidecars.
3. **Refactor** — none expected. Do not restructure `validate_collision`; append
   to it.

## Test plan

| Test | Input | Expect |
| ---- | ----- | ------ |
| `tracked_scenes_declare_the_identity_tuning` | each of the 7 tracked `.ron` via `Scenario::load_verified` | `separation_phases() == 1 && mass_class_count() == 1 && separation_threads() == 1` |
| `zero_separation_phases_is_rejected` | valid bodied spec, `separation_phases: 0` | `Err(ScenarioError::InvalidCollision(_))` |
| `separation_phases_above_the_cap_is_rejected` | same spec, `separation_phases: 17` | `Err(ScenarioError::InvalidCollision(_))` |
| `mass_classes_above_the_cap_is_rejected` | same spec, `mass_class_count: 9` | `Err(ScenarioError::InvalidCollision(_))` |
| `separation_threads_above_the_cap_is_rejected` | same spec, `separation_threads: 17` | `Err(ScenarioError::InvalidCollision(_))` |
| `a_bodyless_scenario_may_not_tune_separation` | `collision_radius_q8: 0`, `separation_strength_q8: 0`, `separation_phases: 4` | `Err(ScenarioError::InvalidCollision(_))` |
| `the_gate_scene_pins_the_identity_tuning` | `technical_prototype_v1` spec with `separation_phases: 2` | `Err(ScenarioError::InvalidDimension(_))` |

Already-green tests that must stay green **without edits** — this is the
ticket's real acceptance criterion:

| Test | File | Why it matters |
| ---- | ---- | ---- |
| `a_bodied_scenario_is_pinned_to_a_golden_digest` | `crates/mmd-engine/tests/separation.rs` | `BODIED_STACK_HASH` must not move |
| `a_bodyless_scenario_walks_the_flow_only_path` | `crates/mmd-engine/tests/separation.rs` | `BODYLESS_GRID_PRE_SEPARATION_HASH` must not move |
| `mid_scene_reports_its_tuning` | `crates/mmd-engine/tests/separation.rs` | tracked scene still loads and reports |
| `v1_geometry_stays_frozen_against_the_fixture_relaxation` | `crates/mmd-engine/tests/scenario_contract.rs` | gate scene stays locked |
| `fixture_scenarios_are_hash_verified` | `crates/mmd-engine/tests/harness.rs` | regenerated sidecars are correct |

## Impl steps

- [x] 1. **TODO(user) — RESOLVED by the orchestrator, 2026-08-09.**
      `plan/zombie-collision` (`108230a`) is already an ancestor of `main`
      (`git merge-base --is-ancestor plan/zombie-collision main` → true) and
      `origin/main` == local `main` (`5ec9df6`). The branch
      `plan/horde-sim-headroom` was cut from that `main` in pre-flight and is
      already checked out. **Do not create or switch branches** — work on the
      current one.
- [x] 2. In `crates/mmd-engine/src/scenario.rs`, below
      `MAX_SEPARATION_STRENGTH_Q8`, add:
      ```rust
      /// Largest number of ticks the separation pass may be spread over.
      /// `1` is the identity: every agent, every tick.
      pub const MAX_SEPARATION_PHASES: u32 = 16;
      /// Largest number of distinct push-priority classes. `1` is the identity:
      /// every agent pushes and is pushed equally.
      pub const MAX_MASS_CLASSES: u32 = 8;
      /// Largest worker-thread count for the separation pass. `1` is the
      /// identity: the pass runs inline on the calling thread, no pool exists.
      pub const MAX_SEPARATION_THREADS: u32 = 16;
      ```
- [x] 3. In the same file, below `V1_SEPARATION_STRENGTH_Q8`, add
      `const V1_SEPARATION_PHASES: u32 = 1;`,
      `const V1_MASS_CLASSES: u32 = 1;`,
      `const V1_SEPARATION_THREADS: u32 = 1;`.
- [x] 4. Add three fields to `struct Scenario`, directly after
      `separation_strength_q8`:
      ```rust
      /// Ticks the separation pass is spread over. 1 = every agent, every tick.
      separation_phases: u32,
      /// Distinct push-priority classes. 1 = every agent equal.
      mass_class_count: u32,
      /// Worker threads for the separation pass. 1 = inline, no pool.
      separation_threads: u32,
      ```
- [x] 5. Add the same three `pub` fields, same order, same doc comments, to
      `struct ScenarioSpec`.
- [x] 6. In `Scenario::from_spec`, copy all three through alongside the existing
      `collision_radius_q8: doc.collision_radius_q8,` line.
- [x] 7. Add three accessors next to `separation_strength_q8()`:
      ```rust
      pub fn separation_phases(&self) -> u32 { self.separation_phases }
      pub fn mass_class_count(&self) -> u32 { self.mass_class_count }
      pub fn separation_threads(&self) -> u32 { self.separation_threads }
      ```
- [x] 8. In `validate_collision`, append — after the existing
      `separation_strength_q8 is set but collision_radius_q8 is 0` check:
      ```rust
      for (got, max, name) in [
          (doc.separation_phases, MAX_SEPARATION_PHASES, "separation_phases"),
          (doc.mass_class_count, MAX_MASS_CLASSES, "mass_class_count"),
          (doc.separation_threads, MAX_SEPARATION_THREADS, "separation_threads"),
      ] {
          if got == 0 {
              return Err(ScenarioError::InvalidCollision(format!(
                  "{name} must be >= 1; 1 is the identity tuning"
              )));
          }
          if got > max {
              return Err(ScenarioError::InvalidCollision(format!(
                  "{name}: got {got}, max {max}"
              )));
          }
      }
      // A knob on a pass that never runs is an authoring mistake, not a no-op:
      // it reads as tuned and changes nothing.
      if doc.collision_radius_q8 == 0
          && (doc.separation_phases != 1
              || doc.mass_class_count != 1
              || doc.separation_threads != 1)
      {
          return Err(ScenarioError::InvalidCollision(
              "a bodyless scenario must leave separation_phases, \
               mass_class_count and separation_threads at 1; there is no \
               separation pass to tune"
                  .into(),
          ));
      }
      ```
- [x] 9. In `validate_version_and_dims`, extend the `technical_prototype_v1`
      `checks` array with three more rows:
      `(doc.separation_phases, V1_SEPARATION_PHASES, "separation_phases")`,
      `(doc.mass_class_count, V1_MASS_CLASSES, "mass_class_count")`,
      `(doc.separation_threads, V1_SEPARATION_THREADS, "separation_threads")`.
- [x] 10. In `crates/mmd-engine/src/sim/collision.rs`, extend
      `struct CollisionParams` with `pub phases: u32`, `pub mass_classes: u32`,
      `pub threads: u32`, each documented as "1 = identity".
- [x] 11. Set `CollisionParams::NONE` to
      `Self { radius_cells: 0.0, strength: 0.0, phases: 1, mass_classes: 1, threads: 1 }`.
- [x] 12. In `CollisionParams::from_q8`, fill the three new fields with `1` and
      add the doc line: *"the tuning knobs stay at their identity values;
      only `from_scenario` reads a scenario's."*
- [x] 13. In `CollisionParams::from_scenario`, build the struct directly (do not
      route through `from_q8`) so all five values come from the scenario:
      ```rust
      pub fn from_scenario(scenario: &Scenario) -> Self {
          Self {
              radius_cells: scenario.collision_radius_q8() as f32 / COLLISION_Q8 as f32,
              strength: scenario.separation_strength_q8() as f32 / COLLISION_Q8 as f32,
              phases: scenario.separation_phases(),
              mass_classes: scenario.mass_class_count(),
              threads: scenario.separation_threads(),
          }
      }
      ```
- [x] 14. In `crates/mmd-engine/src/testkit/mod.rs`, add three `pub` fields to
      `GridSpec` (`separation_phases`, `mass_class_count`, `separation_threads`,
      all `u32`), default them to `1` in `GridSpec::new`, and add:
      ```rust
      /// Spread the separation pass over `phases` ticks. `1` (the default) is
      /// the identity.
      pub fn with_separation_phases(mut self, phases: u32) -> Self {
          self.separation_phases = phases;
          self
      }
      /// Give agents `classes` distinct push priorities. `1` (the default) is
      /// the identity.
      pub fn with_mass_classes(mut self, classes: u32) -> Self {
          self.mass_class_count = classes;
          self
      }
      /// Run the separation pass on `threads` threads. `1` (the default) runs
      /// it inline.
      pub fn with_separation_threads(mut self, threads: u32) -> Self {
          self.separation_threads = threads;
          self
      }
      ```
- [x] 15. Map all three in `impl From<GridSpec> for ScenarioSpec`.
- [x] 16. Add the three lines to each of the seven `.ron` files, immediately
      after the `separation_strength_q8:` line, exactly:
      ```
        separation_phases: 1,
        mass_class_count: 1,
        separation_threads: 1,
      ```
      Files: `assets/scenarios/technical_prototype_v1.ron`,
      `assets/scenarios/collision_mid_v1.ron`,
      `assets/scenarios/collision_sprite_v1.ron`, and the four under
      `assets/scenarios/fixtures/`.
- [x] 17. Regenerate every sidecar:
      ```sh
      for f in assets/scenarios/*.ron assets/scenarios/fixtures/*.ron; do
        sha256sum "$f" | cut -d' ' -f1 > "${f%.ron}.sha256"
      done
      ```
- [x] 18. Add the seven tests from the test plan to
      `crates/mmd-engine/tests/scenario_contract.rs`, building each invalid case
      from a valid `ScenarioSpec` and calling `Scenario::from_spec`. Match on the
      error variant, not the message string.
- [x] 19. Run the full validation block below. Do **not** update
      `BODIED_STACK_HASH` or `BODYLESS_GRID_PRE_SEPARATION_HASH` — if either
      moves, the identity default is wrong and the bug is in steps 10–13.

## Outputs

- Touched: `crates/mmd-engine/src/scenario.rs`,
  `crates/mmd-engine/src/sim/collision.rs`,
  `crates/mmd-engine/src/testkit/mod.rs`,
  `crates/mmd-engine/tests/scenario_contract.rs`, 7 × `.ron`, 7 × `.sha256`.
- Public API added: `Scenario::separation_phases/mass_class_count/separation_threads`,
  `CollisionParams::{phases, mass_classes, threads}`,
  `GridSpec::with_separation_phases/with_mass_classes/with_separation_threads`,
  `scenario::{MAX_SEPARATION_PHASES, MAX_MASS_CLASSES, MAX_SEPARATION_THREADS}`.
- Migration: every `.ron` scenario file now requires three more fields. A
  scenario authored before this commit fails to parse — deliberately.

## Validation

- [x] `cargo fmt --all -- --check`
- [x] `cargo test --workspace --locked` — green, and
      `a_bodied_scenario_is_pinned_to_a_golden_digest` passes **unedited**
- [x] `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- [x] `cargo run -p xtask -- bootstrap --check`
- [x] `cargo run -- run --agents 5000 --frames 300` — exits 0
- [x] the gate smoke `hash=` equals the T0 pinned digest, byte for byte
- [x] `graphify update .` run (graph refresh; `graphify-out/` is gitignored)
- [x] `cargo run -- run --scenario assets/scenarios/collision_mid_v1.ron --frames 300` — exits 0
- [x] `cargo run -- run --scenario assets/scenarios/collision_sprite_v1.ron --frames 300` — exits 0
- [x] `nix flake check`
- [ ] app functional — a scene still loads, ticks and exits; no behaviour changed
      (manual/windowed check — see `ai-artifacts/manual_test_checklist.md` §T1)
- [x] commit msg draft: `feat(scenario): declare the separation headroom knobs at their identity values`
