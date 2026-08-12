# T2: Unify pick geometry and context orders

**Plan:** `./ai_artefacts/PLAN_2026_08_10_rts-interaction-ui-audio-hardening.md`  
**Depends:** T1  
**Commit outcome:** every visible resource/unit body point is pickable; frontmost render hit wins; live + headless right-click use one engine path.

## Context (self-contained)

- Goal: remove manual/headless discrepancy before collision/UI layers grow.
- This slice: shared sprite geometry, depth comparator, mixed resource context dispatch, observable receipts.
- Out of scope here: radius-inflated nav, hard collision, formation slots, HUD routing, audio playback.
- Assumptions: full 48×48 resource quad is clickable; unit pick = full 48×48 rendered body rect ∪ 3-cell world circle; strict GPU `GREATER` means larger depth wins; equal depth keeps lower slot because pack order is ascending.
- Decisions: `docs/ADR/016_ADR_phase1_1_scope_and_input_geometry.md`.

## Requirements

- In `crates/mmd-engine/src/rts/selection.rs`, delete `UNIT_PICK_RADIUS_SCALE` coupling.
- Add exact shared helpers:
  ```rust
  pub const RTS_SPRITE_SIZE_PX: [f32; 2] = [48.0, 48.0];
  pub fn sprite_screen_rect(view: &IsoView, ground: [f32; 2]) -> [f32; 4];
  pub fn unit_pick_contains(view: &IsoView, ground: [f32; 2], radius: f32, screen: [f32; 2]) -> bool;
  pub fn entity_pick_depth(view: &IsoView, ground: [f32; 2]) -> f32;
  ```
- `sprite_screen_rect` must match `pack_frame` ground anchoring exactly; one helper/constant used by picker and packer.
- Resource-node hit = full 48×48 rect. Unit hit = rect OR unprojected cell-space circle (`distance² <= radius²`). Building hit remains footprint-based.
- `pick_at` gathers every hit, chooses greatest `entity_pick_depth`; equal depth chooses lower entity slot. No old type-priority branch.
- Add in `crates/mmd-engine/src/rts/world.rs`:
  ```rust
  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub enum IssuedOrder { Move, Gather, Build }

  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub struct UnitOrderReceipt { pub id: EntityId, pub order: IssuedOrder }

  #[derive(Debug)]
  pub struct OrderReceiptBuffer { receipts: Vec<UnitOrderReceipt> }

  pub fn issue_context_order_at(
      &mut self,
      view: &IsoView,
      screen: [f32; 2],
      receipts: &mut OrderReceiptBuffer,
  ) -> ContextOrderResult;
  ```
- `OrderReceiptBuffer::new()` reserves `MAX_SELECTION`; each call clears/reuses it; no allocation after construction.
- Node target: selected workers receive Gather; selected non-workers receive Move toward current deterministic node approach. Site target: eligible workers Build; others rejected. Ground/other target: all orderable selected units Move.
- Move private `src/rts_run.rs::right_click_order` into engine API above. SDL and scripted paths call it. `RtsHarness` uses same API; no third harness.
- `ContextOrderResult` includes `pick`, `accepted`, `rejected`, optional reason. Receipts sorted by entity slot.

## Inputs

- `crates/mmd-engine/src/rts/selection.rs`: `pick_at`, `Pick`, `footprint_contains`.
- `crates/mmd-engine/src/rts/pack.rs`: entity ground projection/48×48 packing.
- `crates/mmd-engine/src/render/instance.rs`: `IsoView::{project,unproject,depth}`.
- `src/rts_run.rs`: `right_click_order`, `apply`.
- `tests/rts_cli_contract.rs::a_right_click_on_a_node_starts_gathering`.
- **From Depends:** T1 provides `UnitKind::body_radius_cells() == 3.0`, `RTS_SPRITE_SIZE` scene geometry, 30/24 speeds.

## TDD

1. **Red** — add quad corners/union/depth-tie tests; add click-path gather test that right-clicks a resource corner, not center.
2. **Green** — share geometry + context API; replace app-private dispatcher.
3. **Refactor** — remove duplicated app routing and any picker-vs-packer constants; verify receipt buffer capacity never grows.

## Test plan

| Test | Input | Expect |
| --- | --- | --- |
| `every_resource_quad_corner_is_pickable` | four inside corners + four outside points | inside picks node; outside misses |
| `unit_pick_is_sprite_rect_union_body_circle` | rect-only, circle-only, neither | hit, hit, miss |
| `frontmost_rendered_entity_wins` | overlapping unit/node different depth | greater depth wins |
| `equal_depth_uses_lower_slot` | equal ground depth | lower slot wins |
| `mixed_resource_order_partitions_by_capability` | workers + soldier | workers Gather; soldier Move |
| `click_path_gather_banks_crystal` | corner click via shared API | stock increases |
| `context_receipts_allocate_nothing_after_new` | repeated calls | buffer capacity unchanged |

## Impl steps

- [ ] 1. Add red picker tests in `crates/mmd-engine/tests/rts_selection.rs`.
- [ ] 2. Add red shared-click gather/mixed-group tests in `crates/mmd-engine/tests/rts_economy.rs`.
- [ ] 3. Extract shared sprite rect/depth helpers; make `pack.rs` consume same 48×48 constant.
- [ ] 4. Replace picker type priority with depth + slot comparator.
- [ ] 5. Add receipt/result types + preallocated buffer in engine RTS module.
- [ ] 6. Implement `RtsWorld::issue_context_order_at`; preserve cancel-placement semantics in app.
- [ ] 7. Replace `src/rts_run.rs::right_click_order` with shared API call.
- [ ] 8. Extend CLI regression to click away from node center.

## Outputs

- Modified: selection/pack/world/mod/app + selection/economy/CLI tests.
- Public API: helpers + receipt/result types/signature above.
- Behavior: visible quad/body selection; frontmost hit; shared live/headless context orders.
- Migrate/config: none.

## Validation

- [ ] `cargo test -p mmd-engine --locked --test rts_selection`
- [ ] `cargo test -p mmd-engine --locked --test rts_economy click_path`
- [ ] `cargo test -p millions_must_die --locked --test rts_cli_contract right_click`
- [ ] `cargo check --workspace --all-targets --all-features --locked`
- [ ] manual check: selected worker right-clicks four resource-square corners → Gather starts
- [ ] app functional: `cargo run -- rts --frames 160 --inject-input-file assets/scenarios/rts_acceptance_v1.script`
- [ ] commit msg draft: `fix(rts): make visible pick geometry drive context orders`
