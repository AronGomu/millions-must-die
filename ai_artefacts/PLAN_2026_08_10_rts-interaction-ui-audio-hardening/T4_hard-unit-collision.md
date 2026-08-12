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

- [ ] 1. Add new `rts_collision.rs` integration test with exact geometry cases.
- [ ] 2. Add allocation + reproducibility red tests.
- [ ] 3. Implement pure overlap/sweep helpers.
- [ ] 4. Add preallocated unit/candidate scratch to `RtsWorld` construction.
- [ ] 5. Split movement into proposal + rotated sequential commit.
- [ ] 6. Include idle/all-owner units in collision checks.
- [ ] 7. Add deterministic pre-tick overlap repair for explicit testkit-invalid positions.
- [ ] 8. Restrict raw entity mutation visibility; add narrow testkit hooks.
- [ ] 9. Run full engine suite; verify no `sim/` diff.

## Outputs

- New: `crates/mmd-engine/src/rts/collision.rs`, `crates/mmd-engine/tests/rts_collision.rs`.
- Modified: world/entity/mod/testkit/allocation tests.
- Public API: geometry helpers; testkit force hook only behind feature.
- Behavior: hard non-overlap all RTS units every completed tick.
- Migrate/config: none.

## Validation

- [ ] `cargo test -p mmd-engine --locked --test rts_collision`
- [ ] `cargo test -p mmd-engine --locked --test frame_allocations hard_collision_tick_allocates_nothing`
- [ ] `cargo test -p mmd-engine --locked --test rts_world`
- [ ] `cargo check --workspace --all-targets --all-features --locked`
- [ ] `git diff --exit-code -- crates/mmd-engine/src/sim`
- [ ] app functional: `cargo run -- rts --frames 300`
- [ ] commit msg draft: `feat(rts): reject movement that would merge unit bodies`
