# T6: Make production and construction body-safe

**Plan:** `./ai_artefacts/PLAN_2026_08_10_rts-interaction-ui-audio-hardening.md`  
**Depends:** T3, T4, T5  
**Commit outcome:** initial/produced/completion transitions preserve hard-body invariant; blocked production or completion waits without loss/double charge.

## Context (self-contained)

- Goal: close non-movement paths that can create overlap.
- This slice: ready-but-blocked queues, nearest-free spawn, atomic completion evacuation + solid stamp.
- Out of scope here: UI/settings/audio; combat/destruction.
- Assumptions: sites walkable until complete; no legal evacuation → progress stays final pre-complete tick; no legal production spawn → paid/reserved head stays ready.
- Implementation assumptions (T6, logged at execution):
  1. The ticket's `tick_head/head_ready/pop_ready(building: EntityId)` signature is a plan defect: `production.rs` holds `ProductionQueue` (no id) and slot-keyed `ProductionTable`, neither of which can resolve an `EntityId` without the entity store. Implemented as `ProductionQueue` self-methods; call sites stay in `RtsWorld::production_system`. Behaviour contract unchanged. Supervisor-approved.
  2. "Nearest legal center" is searched over the **whole grid**, exactly as scenario seeding and the tick's overlap repair already do — one shared planner, `nearest_free_body_center`. No search radius constant was invented, so "no space" means the grid holds no legal free body centre at all.
  3. `StaticNav::position_clear` is read through its own precomputation, `center_blocked()` (same answer for the unit body radius, without recomputing it per candidate); the footprint a completion has not stamped yet is excluded separately via `circle_clear_of_cell_rect`.
  4. Evacuation scores each evacuee's candidates by squared distance to **its own current position** (minimum displacement), then flat cell index.
  5. The inflated centre mask is rebuilt per completed building rather than once per tick, so a later completion on the same tick sees an earlier one as solid; the pooled fields are still replaced once per batch.
- ~~Residual risk (pre-existing, not T6): `center_blocked` marks the 1–2 cell diagonal channel around `y = 172..174, x = 190..199` of the tracked scene as legal body centres and the pooled field routes through it, but no step there survives `sweep_clear` + `step_admissible`, so a body wedges (fresh body at `[176.5, 166.5]` ordered to `(200, 200)` stops at ~`[193.27, 174.27]`). Reproduces against pre-T6 code. Pinned by the ignored `rts_nav_staleness::a_body_wedges_in_the_narrow_eastern_channel`; owner T3/T4 navigation.~~ **Corrected in the review-fix pass — this diagnosis was wrong.** `center_blocked` and `sweep_clear` agree over that whole region, and the same walk arrives when nothing is standing in it. The body that wedges is stopped by *another body*: the Depot leaves a single-file corridor (`x = 191`/`x = 192`) and the scaffolding builder parked at `[191.0, 180.0]` plugs it, because the contact-normal push target is a `center_blocked` cell and the mover's deflection then ping-pongs. Regression of this phase, owner the collision/push rule, not T3/T4 navigation. Pinned by the un-ignored `rts_nav_staleness::a_parked_body_plugs_the_single_file_depot_corridor`; see `docs/ADR/017`.
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

- [x] 1. Add production ready/wait/retry red tests. *Criterion:* `production_uses_nearest_free_body_position`, `production_waits_when_no_spawn_is_free`, `waiting_production_resumes_once`, `new_spawn_joins_same_tick_collision` exist in `tests/rts_production.rs` and fail against the pre-change code.
- [x] 2. Add construction atomic evacuation red tests. *Criterion:* `completion_evacuates_every_overlapping_body`, `completion_waits_when_evacuation_impossible`, `later_completion_sees_earlier_building` exist in `tests/rts_build.rs` and fail against the pre-change code.
- [x] 3. Split production queue advance from pop/reset. *Criterion:* `ProductionQueue::{tick_head, head_ready, pop_ready}` exist, `advance`/`push_front` are gone, and `cargo test -p mmd-engine --locked --test rts_production` queue-unit cases pass.
- [x] 4. Add collision-safe nearest-free spawn search around producer footprint. *Criterion:* `production_uses_nearest_free_body_position` and `production_waits_when_no_spawn_is_free` pass.
- [x] 5. Ensure spawned unit enters live/collision scratch before movement. *Criterion:* `new_spawn_joins_same_tick_collision` passes (no body overlap at the spawn tick's end).
- [x] 6. Replace building push with preplanned atomic evacuation. *Criterion:* `push_units_off_blocked_cells` no longer exists in `rts/world.rs`; `completion_evacuates_every_overlapping_body` and `completion_waits_when_evacuation_impossible` pass.
- [x] 7. Commit finish/stamp/mask invalidation/supply only after full plan succeeds. *Criterion:* `later_completion_sees_earlier_building` passes and `completion_waits_when_evacuation_impossible` observes an unchanged site (progress `build_ticks - 1`, still walkable, no supply grant).
- [x] 8. Add success/failure allocation cases; run supply/nav-staleness regressions. *Criterion:* `cargo test -p mmd-engine --locked --test frame_allocations blocked_transitions_allocate_nothing` passes and `--test rts_nav_staleness` plus the supply cases of `--test rts_production` stay green.

## Outputs

- Modified: production/world/build/static-nav/formation + production/build/nav/allocation tests.
- Public API: queue methods above; no new app API.
- Behavior: no spawn/completion overlap; wait preserves player payment/reservation.
- Migrate/config: production state hash changes because ready head can persist; update RTS-only expected hashes.

## Validation

- [x] `cargo test -p mmd-engine --locked --test rts_production`
- [x] `cargo test -p mmd-engine --locked --test rts_build evacuation`
- [x] `cargo test -p mmd-engine --locked --test rts_nav_staleness`
- [x] `cargo test -p mmd-engine --locked --test frame_allocations blocked_transitions_allocate_nothing`
- [x] `cargo check --workspace --all-targets --all-features --locked`
- [x] app functional: tracked script produces Worker + Soldier by frame 1600. *Criterion:* `cargo run -- rts --frames 1600 --inject-input-file assets/scenarios/rts_acceptance_v1.script` prints its exit line with a produced Worker and Soldier.
- [x] commit msg draft: `fix(rts): keep production and construction outside occupied bodies`. *Criterion:* the commit landing this ticket carries that subject.
