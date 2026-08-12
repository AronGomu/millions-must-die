# T5: Add formations and fair choke queues

**Plan:** `./ai_artefacts/PLAN_2026_08_10_rts-interaction-ui-audio-hardening.md`  
**Depends:** T2, T4  
**Commit outcome:** group move/gather/build uses one pooled anchor field plus distinct deterministic 6-cell slots; collision priority rotates through chokes.

## Context (self-contained)

- Goal: make hard bodies useful: groups finish orders without merging forever at one point.
- This slice: atomic slot planning, shared anchor field, bounded terminal steering, group build API, stable outcomes.
- Out of scope here: production spawn/build completion, camera/UI/audio sink.
- Assumptions: no per-unit flow field/A*; group gets one anchor field; terminal straight-line slot steering is local collision placement, not pathfinding; insufficient slots rejects whole group order.
- Decision: `docs/ADR/017_ADR_rts_hard_collision_navigation_and_formations.md`.

## Requirements

- Create `crates/mmd-engine/src/rts/formation.rs`:
  ```rust
  pub const FORMATION_SPACING_CELLS: i32 = 6;
  pub const FORMATION_CAPTURE_MARGIN_CELLS: f32 = 6.0;

  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub struct FormationGoal { pub anchor: Cell, pub slot: Cell }

  pub struct FormationScratch {
      reserved_cells: Vec<bool>,
      planned_slots: Vec<Cell>,
      planned_units: Vec<EntityId>,
  }
  ```
- `Order::Move` stores `FormationGoal` + shared field handle. State hash digests anchor/slot/field epoch.
- `order_move_group` exact algorithm:
  1. canonicalize live orderable IDs ascending; input order irrelevant;
  2. resolve click to nearest body-clear reachable anchor; score squared offset then flat cell index;
  3. acquire field once for anchor;
  4. enumerate 6-cell square lattice around anchor in Chebyshev rings, row-major ties;
  5. candidate must be body-clear, reachable in anchor field, `StaticNav::sweep_clear(anchor_center,slot_center,3)`, unreserved, clear of non-group units;
  6. assign each unit nearest remaining slot to current pos; ties offset² then flat index;
  7. insufficient slots → `NoFormationSpace`, mutate nothing;
  8. commit all orders with same field handle.
- Movement: outside `distance(anchor,slot)+6` follow shared field; inside, try direct slot vector; if static/collision gate rejects, try shared field; if both reject, wait. Arrival = center reaches slot within 0.25 cell and no penetration; clear order.
- Gather group: assign distinct legal approach slots around resource footprint; workers Gather, non-workers Move formation around same resource anchor. No more node-center targets.
- Build group: add `RtsWorld::order_build_group(ids, site, receipts)`; eligible workers get distinct approach slots; others rejected.
- T2 `OrderReceiptBuffer` remains ascending; context result returns accepted/rejected exact per unit.
- Rotating traversal from T4 is sole fairness rule. Add no random/choke detector.
- Assert one `FieldPool::acquire` per group command, not per member.

## Inputs

- `rts/orders.rs::Order`, gather phases.
- `rts/world.rs::{order_move_group,order_gather_group,order_build,movement}`.
- `nav/field_pool.rs` acquire count/readability.
- **From Depends:** T2 shared `issue_context_order_at`, receipts, mixed resource split. T4 rotated hard collision, 3-cell bodies, static gate.

## TDD

1. **Red** — order permutation, one-field, slot spacing, atomic failure, gather/build slots, choke fairness tests.
2. **Green** — formation scratch/planner + order fields + terminal mode.
3. **Refactor** — route move/gather/build group APIs through one planner; remove per-call duplicated ring scans.

## Test plan

| Test | Input | Expect |
| --- | --- | --- |
| `group_input_order_does_not_change_slots` | permuted IDs | same goals/hash |
| `group_move_acquires_one_anchor_field` | 24 units | acquire count +1 |
| `formation_slots_are_six_cells_apart` | open map | all pair distances >=6 |
| `formation_order_is_atomic_when_space_missing` | enclosed target | no order mutated |
| `gatherers_get_distinct_legal_approaches` | 6 workers/node | unique clear slots |
| `nonworkers_move_around_resource` | mixed group | Move receipts, no Gather |
| `builders_get_distinct_site_approaches` | workers/site | unique legal goals |
| `terminal_steering_uses_no_new_field` | near anchor | acquire count unchanged |
| `choke_priority_rotates` | symmetric queue | deterministic alternating progress |
| `formation_planning_allocates_nothing` | warm world/group | 0 allocations |

## Impl steps

- [ ] 1. Add `rts_formation.rs` red tests + allocation case.
- [ ] 2. Add `FormationGoal`/scratch/constants; reserve buffers at world load.
- [ ] 3. Extend `Order::Move` + gather/build phases; update hash.
- [ ] 4. Implement atomic shared-anchor formation planner exactly as scored above.
- [ ] 5. Add terminal steering branch before shared-field fallback.
- [ ] 6. Rework gather group into worker approaches + non-worker formation moves.
- [ ] 7. Add `order_build_group`; route context site orders through it.
- [ ] 8. Assert receipt order + one field acquisition; run economy/build/nav-staleness suites.

## Outputs

- New: `rts/formation.rs`, `tests/rts_formation.rs`.
- Modified: orders/world/mod/context tests/economy/build/allocation.
- Public API: `FormationGoal`, group build API; existing group APIs return structured outcomes.
- Behavior: distinct stable final positions + deterministic fair choke waiting.
- Migrate/config: state hashes change intentionally; no phase-0 hash changes.

## Validation

- [ ] `cargo test -p mmd-engine --locked --test rts_formation`
- [ ] `cargo test -p mmd-engine --locked --test rts_economy approach`
- [ ] `cargo test -p mmd-engine --locked --test rts_build approach`
- [ ] `cargo test -p mmd-engine --locked --test frame_allocations formation_planning_allocates_nothing`
- [ ] `cargo check --workspace --all-targets --all-features --locked`
- [ ] manual check: box-select six workers; right-click ground/node/site → visible spread, no merge
- [ ] commit msg draft: `feat(rts): give group orders deterministic collision-safe slots`
