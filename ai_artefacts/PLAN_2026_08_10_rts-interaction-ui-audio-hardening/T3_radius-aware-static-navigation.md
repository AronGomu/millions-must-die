# T3: Add radius-aware static navigation

**Plan:** `./ai_artefacts/PLAN_2026_08_10_rts-interaction-ui-audio-hardening.md`  
**Depends:** T1  
**Commit outcome:** 3-cell RTS bodies clear terrain/nodes/buildings/map edges; pooled fields route centers through inflated mask; gather/build/drop-off still work.

## Context (self-contained)

- Goal: make body geometry true against static world before unit-unit collision lands.
- This slice: continuous circle-vs-static checks, inflated flow mask, legal approaches, collision-safe initial spawn.
- Out of scope here: moving-unit collision, formations, production completion evacuation, UI/audio.
- Assumptions: touching is legal; penetration uses strict `<`; resource nodes + finished buildings are solid; sites stay walkable until completion; placement validation keeps raw terrain.
- Decision: `docs/ADR/017_ADR_rts_hard_collision_navigation_and_formations.md`.

## Requirements

- Create `crates/mmd-engine/src/rts/static_nav.rs`:
  ```rust
  pub struct StaticNav {
      width: u32,
      height: u32,
      solids: Vec<bool>,
      center_blocked: Vec<bool>,
  }

  impl StaticNav {
      pub fn new(scenario: &Scenario, store: &EntityStore) -> Result<Self, StaticNavError>;
      pub fn center_blocked(&self) -> &[bool];
      pub fn position_clear(&self, p: [f32; 2], radius: f32) -> bool;
      pub fn sweep_clear(&self, from: [f32; 2], to: [f32; 2], radius: f32) -> bool;
      pub fn stamp_finished_building(&mut self, min: Cell, edge: u32);
      pub fn rebuild_center_blocked(&mut self, radius: f32);
  }
  ```
- Solids = scenario terrain + resource 1×1 footprints + finished buildings; exclude sites.
- Circle/AABB clearance: squared distance from center to closed cell/footprint AABB `>= radius²`; map center constrained to `[r,width-r] × [r,height-r]`.
- `sweep_clear` uses exact segment-vs-expanded-AABB or segment-to-AABB distance; no sampled-step gaps/tunneling.
- `center_blocked[cell] = !position_clear(cell_center, 3.0)`.
- Extend `nav::FieldPool`:
  ```rust
  pub fn from_blocked_mask(width: u32, height: u32, blocked: &[bool]) -> Result<Self, FieldPoolError>;
  pub fn replace_blocked_mask(&mut self, blocked: &[bool]) -> Result<(), FieldPoolError>;
  pub fn reachable(&self, field_slot: usize, cell: Cell) -> bool;
  ```
  `replace_blocked_mask` copy + invalidate all slots; no allocation.
- Reserve `FieldScratch` to `8 * cells + 1` heap entries at world creation so first cold miss allocates zero.
- `RtsWorld` owns `StaticNav`; constructs `FieldPool` from `center_blocked`, not raw scenario obstacles.
- Initial scenario units spawn in scenario order at nearest legal free cell center: minimum squared distance from preferred; tie lower flat cell index. No legal position → `RtsWorldError::NoFreeUnitPosition`.
- Replace node-center target with `entity_approach_cell(static_nav, store, target, mover_kind)`. Score legal centers by distance to target footprint, then flat index.
- Replace independent reach constants with:
  ```rust
  pub const NAV_CENTER_TOLERANCE_CELLS: f32 = 0.5;
  pub const fn interaction_reach(kind: UnitKind) -> f32;
  ```
  Return radius + 0.5. Gather/build/drop-off use distance to target footprint rectangle.
- Keep building placement on raw scenario/build/site/node rules; inflated mask must not enlarge placement exclusion.
- Initial relocation changes worker pixels. Update `assets/scenarios/rts_acceptance_v1.script` and coordinate helpers in `tests/rts_cli_contract.rs` in this commit to the exact deterministic relocated positions; keep existing select→gather→build→produce flow green. T17 extends this script; it does not defer repair.

## Inputs

- `crates/mmd-engine/src/nav/field_pool.rs`, `nav/flow_field.rs`.
- `crates/mmd-engine/src/rts/orders.rs`: `building_approach_cell`, `drop_off_approach_cell`, reach logic.
- `crates/mmd-engine/src/rts/world.rs`: load, gather, build, movement.
- `crates/mmd-engine/src/rts/build.rs`, `economy.rs`.
- **From Depends:** T1 supplies 512 cap, `UnitKind::body_radius_cells`, 3-cell radius.

## TDD

1. **Red** — add static geometry, corridor, approach, initial-spawn, cold-field allocation tests.
2. **Green** — implement `StaticNav`; feed mask to existing pooled fields; swap approach/reach logic.
3. **Refactor** — keep raw placement mask separate; remove point-sized RTS nav helpers only after all economy/build tests pass.

## Test plan

| Test | Input | Expect |
| --- | --- | --- |
| `body_clears_map_edges` | centers at 2.99/3/width-3 | reject/accept/accept |
| `body_clears_static_rectangles` | terrain/node/building AABBs | no penetration; contact legal |
| `five_cell_corridor_is_unreachable` | 3-cell body | field marks unreachable |
| `wide_corridor_is_reachable` | >=6-cell clearance | finite field path |
| `sites_remain_walkable_until_completion` | unfinished site | center mask open |
| `approach_targets_stay_outside_solids` | node/HQ/site | legal center + interaction succeeds |
| `initial_workers_are_relocated_without_overlap` | tracked adjacent spawns | deterministic legal positions |
| `cold_field_acquire_allocates_nothing` | first miss after load | 0 allocations |

## Impl steps

- [ ] 1. Add `rts_radius_nav.rs` tests for map/static/sweep/corridor geometry.
- [ ] 2. Add red economy/build approach tests + initial-spawn test.
- [ ] 3. Implement `StaticNav` buffers and exact clearance/sweep math.
- [ ] 4. Add `FieldPool` mask constructors/replacement/reachability; reserve worst-case scratch.
- [ ] 5. Build RTS field pool from inflated mask; keep raw placement checks unchanged.
- [ ] 6. Route scenario spawn through deterministic nearest-free cell search.
- [ ] 7. Replace node/build/drop-off target + reach calculations.
- [ ] 8. Recompute relocated worker screen coords through `IsoView::project`; update tracked acceptance script + CLI helpers.
- [ ] 9. Add allocation cases; run nav/economy/build/acceptance regressions.

## Outputs

- New: `rts/static_nav.rs`, `tests/rts_radius_nav.rs`.
- Modified: field pool/world/orders/build/economy/mod + tests + tracked acceptance script/CLI coordinate helpers.
- Public API: signatures above.
- Behavior: static body clearance + reachable legal interactions + collision-safe initial load.
- Migrate/config: no scenario byte change in this ticket.

## Validation

- [ ] `cargo test -p mmd-engine --locked --test rts_radius_nav`
- [ ] `cargo test -p mmd-engine --locked --test rts_economy`
- [ ] `cargo test -p mmd-engine --locked --test rts_build`
- [ ] `cargo test -p mmd-engine --locked --test frame_allocations cold_field_acquire_allocates_nothing`
- [ ] `cargo check --workspace --all-targets --all-features --locked`
- [ ] app functional: `cargo run -- rts --frames 300 --inject-input-file assets/scenarios/rts_acceptance_v1.script`
- [ ] commit msg draft: `feat(rts): route unit bodies around static world geometry`
