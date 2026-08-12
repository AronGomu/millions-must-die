# T4: Enforce hard RTS unit collision

**Plan:** `./ai_artefacts/PLAN_2026_08_10_rts-interaction-ui-audio-hardening.md`  
**Depends:** T3  
**Commit outcome:** every current/future RTS unit owner ends each tick outside every other 3-cell body; movement remains deterministic, pooled-field, allocation-free.

## Context (self-contained)

- Goal: replace RTS merge-through movement with proof-carrying hard non-overlap. Horde `sim/` stays soft.
- This slice: sequential proposal/commit collision; all RTS units participate, idle or moving.
- Out of scope here: distinct formation destinations, body-safe production/build completion, UI/audio.
- Assumptions: contact at center distance 6 is legal; `<6` penetrates; whole candidate accepted/rejected; no push/relaxation/epsilon/RNG.
- Decision: `docs/ADR/017_ADR_rts_hard_collision_navigation_and_formations.md`.

### Deviations taken during implementation (approved, in order)

The literal "no push/relaxation" assumption above proved to make the game stop
working: a pooled flow field is body-blind, so a mover whose descent points at
a stationary body is frozen for good. Implemented as ticketed, that froze 26
pre-existing tests including the merge-gate acceptance run — traced, not
guessed (worker slot 11 at `[162.5, 178.5]` never left its spawn cell in 2 000
ticks because slot 12 sat exactly one diameter east of it). Three bounded
mechanisms were added, each escalated and approved before implementation:

1. **Push-aside** — a mover may displace the bodies its candidate touches
   along their contact normals. Whole-or-nothing: nothing moves unless every
   displaced body ends fully legal (in map, static-clear along its own swept
   segment, admissible, clear of the mover's candidate, of every non-displaced
   body, and of every other displaced body's final position). A body is
   displaced at most once per tick, and only its position changes.
2. **Bounded push chain** — a displaced body may in turn displace what it
   would land on, to `MAX_PUSH_DEPTH = 3` links and `MAX_PUSHED_BODIES = 8`
   bodies total, iteratively (no recursion). Needed because the tracked scene
   seeds three workers in a row exactly one diameter apart, so freeing the
   first requires moving the second and third.
3. **Deflection fallback** — only after a push chain is rejected, the mover
   may try its descent rotated `-45°, +45°, -90°, +90°` (`MOVE_DEFLECTIONS`,
   fixed order, first legal wins). Needed because a body pinned against a
   building's static clearance cannot be shoved in any legal direction, and
   only the mover can then resolve the standoff.

All three keep the ticket's real invariant — no completed tick leaves two unit
bodies merged — plus determinism (no RNG, fixed traversal, cross-process hash
proven) and the allocation-free contract (fixed-size arrays and the buffers
reserved at load). What is *not* relaxed: contact stays legal, penetration
stays strict `<`, and no epsilon appears anywhere in the collision rule; a
displaced body is moved clear by the penetration depth plus one mover step
instead.

## Requirements

- Create `crates/mmd-engine/src/rts/collision.rs`:
  ```rust
  pub fn units_overlap(a: [f32; 2], ar: f32, b: [f32; 2], br: f32) -> bool;
  pub fn moving_circle_hits_point(
      from: [f32; 2],
      to: [f32; 2],
      radius: f32,
      other: [f32; 2],
      other_radius: f32,
  ) -> bool;
  ```
- `RtsWorld` owns `unit_scratch: Vec<usize>` reserved to `MAX_ENTITIES` + `candidate_pos: Vec<[f32;2]>` sized once at load.
- Revised movement per tick:
  1. collect all live unit slots ascending;
  2. rotate traversal start by `tick_index % unit_count`;
  3. for mover, compute existing flow-field candidate;
  4. require `StaticNav::sweep_clear`;
  5. sweep candidate circle against every other current unit position;
  6. already processed units expose final position; unprocessed units expose old position;
  7. accept whole candidate or stay.
- Sweep test uses point-to-segment distance² `< (r1+r2)²`, preventing high-speed tunneling. No O(N²) perf claim/gate.
- Idle/mining/building-attending units remain bodies even when not proposed.
- All owners + all `UnitKind` variants collide. Buildings/nodes handled by `StaticNav`.
- A `#[cfg(feature="testkit")] force_position_for_test` may create overlap. At tick start, detect existing penetration; repair units in rotated order via T3 nearest-free search before movement. If no repair exists, return/stash deterministic world error and keep last valid state; never silently preserve penetration after tick.
- Seal raw mutation: `EntityStore::spawn`/`set_position` become `pub(crate)`; `RtsWorld::entities_mut` stays testkit-only; shipping paths route through world invariants.
- Existing state hash already includes tick + positions. Rotation derives from hashed tick; add no redundant cursor state.

## Inputs

- `crates/mmd-engine/src/rts/world.rs::movement`, `RtsWorld::tick`.
- `crates/mmd-engine/src/rts/entity.rs`: store/mutation/live slots.
- `crates/mmd-engine/src/rts/static_nav.rs` from T3.
- `crates/mmd-engine/tests/frame_allocations.rs`.
- **From Depends:** T3 supplies `StaticNav::{position_clear,sweep_clear}`, legal initial positions, 3-cell bodies, preallocated field misses.

## TDD

1. **Red** — exact contact/penetration/head-on/crossing/all-owner/idle-body tests; allocation + reproducibility tests.
2. **Green** — proposal/commit using O(U²) sweeps; rotated order; repair test-only overlap.
3. **Refactor** — seal mutation APIs; document inductive non-overlap proof beside movement loop.

## Test plan

| Test | Input | Expect |
| --- | --- | --- |
| `touching_bodies_do_not_overlap` | distance 6 | false |
| `sub_six_distance_penetrates` | distance 5.999 | true |
| `head_on_units_never_penetrate` | opposing paths 600 ticks | all samples >=6 |
| `crossing_units_cannot_tunnel` | crossing segments | one candidate waits |
| `idle_units_are_collision_bodies` | mover vs idle | mover stops |
| `all_rts_owners_collide` | player vs neutral/future owner | no overlap |
| `priority_rotates_deterministically` | symmetric contention | winner rotates by tick |
| `forced_overlap_is_repaired` | testkit invalid state | next tick legal |
| `hard_collision_is_reproducible` | two worlds/processes | same hash |
| `hard_collision_tick_allocates_nothing` | contended movement | 0 allocations |

## Impl steps

- [x] 1. Add new `rts_collision.rs` integration test with exact geometry cases. — verify: `crates/mmd-engine/tests/rts_collision.rs` exists and holds `touching_bodies_do_not_overlap`, `sub_six_distance_penetrates`, `head_on_units_never_penetrate`, `crossing_units_cannot_tunnel`, `idle_units_are_collision_bodies`, `all_rts_owners_collide`, `priority_rotates_deterministically`, `forced_overlap_is_repaired`; each fails (red) before impl.
- [x] 2. Add allocation + reproducibility red tests. — verify: `hard_collision_tick_allocates_nothing` exists in `tests/frame_allocations.rs`, `hard_collision_is_reproducible` (in-process + child process) in `tests/rts_collision.rs`; both compile-fail or fail before impl.
- [x] 3. Implement pure overlap/sweep helpers. — verify: `crates/mmd-engine/src/rts/collision.rs` exports `units_overlap` + `moving_circle_hits_point`; `cargo test -p mmd-engine --test rts_collision touching_bodies_do_not_overlap sub_six_distance_penetrates` passes.
- [x] 4. Add preallocated unit/candidate scratch to `RtsWorld` construction. — verify: `unit_scratch`/`candidate_pos` reserved to `MAX_ENTITIES` in `from_scenario`; `hard_collision_tick_allocates_nothing` reports 0 allocations.
- [x] 5. Split movement into proposal + rotated sequential commit. — verify: `head_on_units_never_penetrate`, `crossing_units_cannot_tunnel`, `priority_rotates_deterministically` pass.
- [x] 6. Include idle/all-owner units in collision checks. — verify: `idle_units_are_collision_bodies` and `all_rts_owners_collide` pass.
- [x] 7. Add deterministic pre-tick overlap repair for explicit testkit-invalid positions. — verify: `forced_overlap_is_repaired` passes; `RtsWorld::last_tick_error` is `None` after the repair tick.
- [x] 8. Restrict raw entity mutation visibility; add narrow testkit hooks. — verify: `EntityStore::spawn`/`set_position` and `RtsWorld::entities_mut` are unreachable without the `testkit` feature; `cargo check -p mmd-engine --no-default-features --features gpu` passes.
- [x] 9. Run full engine suite; verify no `sim/` diff. — verify: `cargo test -p mmd-engine --locked` passes and `git diff --exit-code -- crates/mmd-engine/src/sim` is clean.

Evidence per step: (1) `tests/rts_collision.rs`, 16 cases; (2)
`hard_collision_tick_allocates_nothing` reports 0 allocations,
`hard_collision_is_reproducible` matches in-process and across a re-exec'd
child process; (3) `src/rts/collision.rs` + its 3 unit tests; (4)
`unit_scratch`/`candidate_pos`/`pushed` reserved to `MAX_ENTITIES` in
`from_scenario`; (5) rotated worklist in `RtsWorld::movement`, pinned by
`priority_rotates_deterministically` (mutation-checked: pinning the start to
slot order fails it); (6) `idle_units_are_collision_bodies` +
`all_rts_owners_collide`; (7) `forced_overlap_is_repaired`; (8)
`cargo check -p mmd-engine --no-default-features --features gpu` clean with
`spawn`/`set_position`/`entities_mut` sealed; (9) `cargo test --workspace
--locked` green except the pre-existing, environment-dependent
`gpu_smoke::the_hud_draws_over_the_world`, which fails identically on a stashed
clean tree.

Test-fixture changes called out (both encoded the old point-unit world, neither
weakens an assertion):

- `rts_nav_staleness::a_walking_unit_re_paths_when_a_building_blocks_its_route`
  despawns the scene's six starting workers first. Its destination, cell
  `(170, 180)`, sits inside their cluster, where a second 3-cell body cannot
  stand at all; the case is about a *field* going stale, not about a crowd.
- `rts_nav_staleness::place_depot` stands its builder off the Depot's east edge
  (`[191.0, 180.0]`) instead of its south-east corner (`[190.5, 186.5]`). The
  corner spot clears the finished footprint by 0.04 cells on two sides: a body
  parked there can be neither walked around nor shoved, and it walled in the
  unit each case was actually about.

## Outputs

- New: `crates/mmd-engine/src/rts/collision.rs`, `crates/mmd-engine/tests/rts_collision.rs`.
- Modified: world/entity/mod/testkit/allocation tests.
- Public API: geometry helpers; testkit force hook only behind feature.
- Behavior: hard non-overlap all RTS units every completed tick.
- Migrate/config: none.

## Validation

- [x] `cargo test -p mmd-engine --locked --test rts_collision` — 16 passed, 1 ignored (child-process fixture).
- [x] `cargo test -p mmd-engine --locked --test frame_allocations hard_collision_tick_allocates_nothing` — 1 passed, 0 allocations.
- [x] `cargo test -p mmd-engine --locked --test rts_world` — 49 passed.
- [x] `cargo check --workspace --all-targets --all-features --locked` — clean.
- [x] `git diff --exit-code -- crates/mmd-engine/src/sim` — no diff.
- [x] app functional: `cargo run -- rts --frames 300` — `rts: clean exit … tick=300 units=6 buildings=1 nodes=10`.
- [x] merge-gate script: `cargo run -- rts --frames 1600 --inject-input-file assets/scenarios/rts_acceptance_v1.script` — `clean exit … tick=1449 units=8 buildings=3` (gather → build → produce all complete under hard bodies).
- [x] commit msg draft: `feat(rts): reject movement that would merge unit bodies`
