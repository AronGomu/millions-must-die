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

- [x] 1. Add `rts_radius_nav.rs` tests for map/static/sweep/corridor geometry. Evidence: `crates/mmd-engine/tests/rts_radius_nav.rs`, 9 tests, all pass.
- [x] 2. Add red economy/build approach tests + initial-spawn test. Evidence: `rts_economy.rs`/`rts_build.rs` updated (park-near-node helper, far-worker/decoy-field fixtures); `rts_radius_nav.rs::initial_workers_are_relocated_without_overlap` + `initial_spawn_relocation_is_deterministic`.
- [x] 3. Implement `StaticNav` buffers and exact clearance/sweep math. Evidence: `crates/mmd-engine/src/rts/static_nav.rs`.
- [x] 4. Add `FieldPool` mask constructors/replacement/reachability; reserve worst-case scratch. Evidence: `crates/mmd-engine/src/nav/field_pool.rs` (`from_blocked_mask`, `replace_blocked_mask`, `reachable`), `nav/flow_field.rs` (`FieldScratch::reserve_worst_case`); `nav_pool` tests + `frame_allocations::cold_field_acquire_allocates_nothing` green.
- [x] 5. Build RTS field pool from inflated mask; keep raw placement checks unchanged. Evidence: `RtsWorld::from_scenario` builds `nav` via `FieldPool::from_blocked_mask(static_nav.center_blocked())`; `placement_valid` reads `static_nav.placement_solids()` (raw terrain + finished buildings, no nodes) — `rts_build.rs` placement-rejection tests unchanged and green.
- [x] 6. Route scenario spawn through deterministic nearest-free cell search. Evidence: `nearest_legal_free_center` in `world.rs`; `workers_start_on_the_scenario_spawn_cells`, `initial_workers_are_relocated_without_overlap`, `initial_spawn_relocation_is_deterministic` green.
- [x] 7. Replace node/build/drop-off target + reach calculations. Evidence: `orders::entity_approach_cell` + `interaction_reach`/`adaptive_reach`; every `world.rs` call site (`order_gather_group`, `order_build`, `issue_context_order_at`, `construction`, `production_system`, `gather`, `movement`) routed through it.
- [x] 8. Recompute relocated worker screen coords through `IsoView::project`; update tracked acceptance script + CLI helpers. Evidence: `assets/scenarios/rts_acceptance_v1.script` drag box widened; `crates/mmd-engine/tests/rts_acceptance.rs` `DRAG_A`/`DRAG_B` + `SCRIPT_COORDS`; `tests/rts_cli_contract.rs` `spawn_group_drag`/`hq_click_screen`; live run `cargo run -- rts --frames 1600 --inject-input-file assets/scenarios/rts_acceptance_v1.script` — exit 0, `units=8 buildings=3`.
- [x] 9. Add allocation cases; run nav/economy/build/acceptance regressions. Evidence: `frame_allocations::cold_field_acquire_allocates_nothing` added; full `cargo test --workspace --locked` green (see Validation).

## Outputs

- New: `rts/static_nav.rs`, `tests/rts_radius_nav.rs`.
- Modified: field pool/world/orders/build/economy/mod + tests + tracked acceptance script/CLI coordinate helpers.
- Public API: signatures above.
- Behavior: static body clearance + reachable legal interactions + collision-safe initial load.
- Migrate/config: no scenario byte change in this ticket.

## Validation

- [x] `cargo test -p mmd-engine --locked --test rts_radius_nav` — 9 passed.
- [x] `cargo test -p mmd-engine --locked --test rts_economy` — 34 passed.
- [x] `cargo test -p mmd-engine --locked --test rts_build` — 37 passed.
- [x] `cargo test -p mmd-engine --locked --test frame_allocations cold_field_acquire_allocates_nothing` — 1 passed.
- [x] `cargo check --workspace --all-targets --all-features --locked` — clean.
- [x] app functional — the ticket's literal `--frames 300` exits early with `rts failed: --inject-input entries never fired` (the tracked script's last scripted event is at tick 1450; true on this branch before this ticket's changes too, same as T2 logged). Ran the frame count `AGENT.md`'s merge-gate line uses (`1600`): `cargo run -- rts --frames 1600 --inject-input-file assets/scenarios/rts_acceptance_v1.script` — exit 0, `quit=true`, `tick=1449 frames=1449 crystal=270 gas=122 supply=9/20 units=8 buildings=3 nodes=10`.
- [x] commit msg draft: `feat(rts): route unit bodies around static world geometry` — used as the commit subject.

## Assumptions (logged during implementation, no user ask)

- **Adaptive interaction reach (approved via supervisor decision mid-implementation).** The tracked `rts_prototype_v1.ron` scenario's obstacle field is a scattered, roughly-every-7-cells maze: sparse for a point agent, but a 3-cell-radius body's inflated mask blocks ~47% of the map, and the legal approach cell nearest a target can land measurably past the flat-ground `interaction_reach` floor (observed ~3.7–4.3 cells against a 3.5 floor near the HQ). A literal fixed `interaction_reach` stalls a gather round trip forever, short of delivery, on this exact scene. Fix: `entity_approach_cell` returns `(Cell, f32)` — the chosen cell and its own distance to the target's footprint — and gather/construction/movement reach checks compare against `adaptive_reach(kind, chosen_cell_dist) = interaction_reach(kind).max(chosen_cell_dist + NAV_CENTER_TOLERANCE_CELLS)`. `interaction_reach`/`NAV_CENTER_TOLERANCE_CELLS` stay exactly as specified (a floor, never edited). Deterministic, allocation-free (no caching, recomputed from the same static inputs every check). Regression: `rts_radius_nav.rs::a_full_gather_round_trip_credits_crystal_on_the_tracked_scenario` reproduces the exact stall case and proves it now completes.
- **`movement()`'s zero-descent-vector fallback could not tell "arrived" from "truly unreachable."** A unit standing on an order's own destination cell samples a zero vector (cost 0, the field's sink) exactly like a genuinely disconnected cell (cost `COST_UNREACHABLE`) does, and the pre-existing code cleared the order in both cases. Adaptive reach makes "parked exactly on the approach cell, still short of the interaction reach a tick ago" a real, common case, so this now uses the ticket-mandated `FieldPool::reachable` to clear the order only when genuinely unreachable, holding position otherwise and letting the gather/construction reach check own completion. In scope: `reachable` exists in the API surface specifically for this.
- **Test-fixture repairs required by relocation, beyond the ticket's own two named files.** The ticket named `assets/scenarios/rts_acceptance_v1.script` and `tests/rts_cli_contract.rs`; in practice the same relocation (six spawn-cell workers, one cell apart, redistributed to legal non-overlapping cells) also broke hardcoded worker-position assumptions in `crates/mmd-engine/tests/{rts_world,rts_selection,rts_economy,rts_build,rts_production,rts_acceptance,rts_nav_staleness}.rs` and `frame_allocations.rs` — box-select rectangles, a teleport-onto-a-node test helper (a resource node is now itself solid, so its own cell is never a legal body position), a decoy-field-eviction fixture's diagonal stride (lands on solid cells ~half the time in this scene), and one CLI click point whose target (the HQ) is now outranked on pick depth by a relocated worker's oversized sprite quad. Each is a narrow, mechanical fix (reposition fixtures / widen a box / pick a different legal click point), not a design change; documented inline at each site.
- **`wall_with_a_gap`/`sealed_chamber` synthetic obstacle fixtures in `rts_world.rs`** were sized for a point agent (a 1-cell gap, a 3x3 interior) and are physically impossible for a 6-cell-diameter body; widened to a 13-row gap and an 11x11 sealed interior respectively, preserving each test's original intent (walk through a gap / prove a sealed destination's own order-move is accepted but the field still finds it unreachable).

Also run (full merge gate, beyond this ticket's own list): `cargo fmt --all -- --check`, `cargo test --workspace --locked` (every crate, all green), `cargo clippy --workspace --all-targets --all-features -- -D warnings` (clean), `nix flake check` (all checks passed), `cargo run -p xtask -- bootstrap/shaders/atlases --check` (all ok), `cargo run -- run --agents 5000 --frames 300` / `--scenario collision_mid_v1.ron` / `--scenario collision_sprite_v1.ron` (unaffected, `sim/` is frozen).
