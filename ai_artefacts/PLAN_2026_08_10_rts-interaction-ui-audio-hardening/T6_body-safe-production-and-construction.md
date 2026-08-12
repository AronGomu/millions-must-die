# T6: Make production and construction body-safe

**Plan:** `./ai_artefacts/PLAN_2026_08_10_rts-interaction-ui-audio-hardening.md`  
**Depends:** T3, T4, T5  
**Commit outcome:** initial/produced/completion transitions preserve hard-body invariant; blocked production or completion waits without loss/double charge.

## Context (self-contained)

- Goal: close non-movement paths that can create overlap.
- This slice: ready-but-blocked queues, nearest-free spawn, atomic completion evacuation + solid stamp.
- Out of scope here: UI/settings/audio; combat/destruction.
- Assumptions: sites walkable until complete; no legal evacuation → progress stays final pre-complete tick; no legal production spawn → paid/reserved head stays ready.
- Decisions: `docs/ADR/017_ADR_rts_hard_collision_navigation_and_formations.md`, existing supply semantics in ADR 015.

## Requirements

- Change `crates/mmd-engine/src/rts/production.rs` API:
  ```rust
  pub fn tick_head(&mut self, building: EntityId);
  pub fn head_ready(&self, building: EntityId) -> bool;
  pub fn pop_ready(&mut self, building: EntityId) -> Option<UnitKind>;
  ```
  Progress saturates at required ticks; no pop/reset until spawn succeeds.
- Production system processes buildings ascending slot:
  1. tick head;
  2. if ready, search nearest legal center outside producer footprint;
  3. score squared distance to current preferred approach, tie lower flat index;
  4. require `StaticNav::position_clear` + no live/planned unit overlap;
  5. spawn then pop head; apply rally via one-unit formation API;
  6. no cell/store capacity → leave head ready; resources/supply unchanged.
- Replace `push_units_off_blocked_cells` with atomic completion plan:
  1. for ready site, collect every unit circle overlapping proposed finished footprint;
  2. include proposed footprint in temporary static clearance;
  3. plan nearest-free centers in rotated unit priority; score distance then flat index;
  4. check unaffected current positions + earlier planned positions; ignore old positions of evacuees;
  5. any failure → discard plan; site remains `build_ticks - 1` and walkable;
  6. success → commit all moves, mark finished, stamp static nav, replace field mask/invalidate fields, grant supply, clear builder orders.
- Multiple site completions process ascending site slot; later sees earlier as solid.
- New production spawn enters same tick's live-unit collision sweep.
- No frame allocation in success/failure retries. Reuse world scratch from T3/T4/T5.
- Supply remains reserved at enqueue/recomputed each tick. Never charge/refund during blocked wait.

## Inputs

- `rts/production.rs`: queue progress/pop semantics.
- `rts/world.rs::{production_system,construction,push_units_off_blocked_cells}`.
- `rts/static_nav.rs`, `rts/formation.rs`, hard collision from dependencies.
- `rts/economy.rs::Supply` rules.
- **From Depends:** T3 nearest legal static centers/mask replacement; T4 unit overlap gate; T5 one-unit rally formation + rotated priority.

## TDD

1. **Red** — nearest spawn tie, fully blocked wait/retry, atomic evacuation success/failure, no allocation.
2. **Green** — split queue tick/pop; preplan transitions before mutating state.
3. **Refactor** — one reusable nearest-free planner for seed/production/evacuation with caller-provided exclusion set.

## Test plan

| Test | Input | Expect |
| --- | --- | --- |
| `production_uses_nearest_free_body_position` | preferred occupied | exact deterministic next candidate |
| `production_waits_when_no_spawn_is_free` | enclosed producer | head ready; no unit; no charge |
| `waiting_production_resumes_once` | free one cell later | one spawn; one pop; no double charge |
| `new_spawn_joins_same_tick_collision` | occupied path | end tick no overlap |
| `completion_evacuates_every_overlapping_body` | multiple units in site | distinct legal positions + solid building |
| `completion_waits_when_evacuation_impossible` | sealed site | site pre-complete; no partial moves/stamp |
| `later_completion_sees_earlier_building` | two ready sites | both legal or latter waits |
| `blocked_transitions_allocate_nothing` | repeated no-space ticks | 0 allocations |
| supply regression tests | queued ready head | reserved usage stable |

## Impl steps

- [ ] 1. Add production ready/wait/retry red tests.
- [ ] 2. Add construction atomic evacuation red tests.
- [ ] 3. Split production queue advance from pop/reset.
- [ ] 4. Add collision-safe nearest-free spawn search around producer footprint.
- [ ] 5. Ensure spawned unit enters live/collision scratch before movement.
- [ ] 6. Replace building push with preplanned atomic evacuation.
- [ ] 7. Commit finish/stamp/mask invalidation/supply only after full plan succeeds.
- [ ] 8. Add success/failure allocation cases; run supply/nav-staleness regressions.

## Outputs

- Modified: production/world/build/static-nav/formation + production/build/nav/allocation tests.
- Public API: queue methods above; no new app API.
- Behavior: no spawn/completion overlap; wait preserves player payment/reservation.
- Migrate/config: production state hash changes because ready head can persist; update RTS-only expected hashes.

## Validation

- [ ] `cargo test -p mmd-engine --locked --test rts_production`
- [ ] `cargo test -p mmd-engine --locked --test rts_build evacuation`
- [ ] `cargo test -p mmd-engine --locked --test rts_nav_staleness`
- [ ] `cargo test -p mmd-engine --locked --test frame_allocations blocked_transitions_allocate_nothing`
- [ ] `cargo check --workspace --all-targets --all-features --locked`
- [ ] app functional: tracked script produces Worker + Soldier by frame 1600
- [ ] commit msg draft: `fix(rts): keep production and construction outside occupied bodies`
