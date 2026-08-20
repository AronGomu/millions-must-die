# T2: Unify pick geometry and context orders

**Plan:** `./artifacts/PLAN_2026_08_10_rts-interaction-ui-audio-hardening.md`  
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

- [x] 1. Add red picker tests in `crates/mmd-engine/tests/rts_selection.rs`. Validated: `cargo test -p mmd-engine --locked --test rts_selection` — added `every_resource_quad_corner_is_pickable`, `unit_pick_is_sprite_rect_union_body_circle`, `frontmost_rendered_entity_wins`, `equal_depth_ties_go_to_the_lower_entity_slot`, `entity_pick_depth_matches_the_render_ground_y`, `sprite_screen_rect_is_forty_eight_pixels_square`; initially red against the old priority-list `pick_at`, green after step 3/4.
- [x] 2. Add red shared-click gather/mixed-group tests in `crates/mmd-engine/tests/rts_economy.rs`. Validated: `cargo test -p mmd-engine --locked --test rts_economy` — added `click_path_gather_banks_crystal`, `mixed_resource_order_partitions_by_capability`, `context_receipts_allocate_nothing_after_new`, `context_order_with_no_selection_is_a_no_op`; initially red (types did not exist), green after step 5/6.
- [x] 3. Extract shared sprite rect/depth helpers; make `pack.rs` consume same 48×48 constant. Evidence: `RTS_SPRITE_SIZE_PX`/`sprite_screen_rect`/`entity_pick_depth`/`stand_on` added to `selection.rs`; `pack.rs` now imports `RTS_SPRITE_SIZE_PX`/`stand_on` instead of deriving `sprite_size` from `scenario().sprite_size_px()` and instead of a private `stand_on` copy.
- [x] 4. Replace picker type priority with depth + slot comparator. Evidence: `pick_at` in `selection.rs` now gathers every hit and picks strictly-greatest `entity_pick_depth`, tie kept by ascending-slot iteration order; `UNIT_PICK_RADIUS_SCALE` deleted.
- [x] 5. Add receipt/result types + preallocated buffer in engine RTS module. Evidence: `IssuedOrder`, `UnitOrderReceipt`, `ContextOrderReason`, `ContextOrderResult`, `OrderReceiptBuffer` added to `world.rs`, exported from `rts/mod.rs`; `context_receipts_allocate_nothing_after_new` proves capacity pinned at `MAX_SELECTION` across 50 calls.
- [x] 6. Implement `RtsWorld::issue_context_order_at`; preserve cancel-placement semantics in app. Evidence: method added to `world.rs`; `src/rts_run.rs`'s `RtsCommand::RightClick` arm still checks `Placement::Pending` and calls `world.cancel_placement()` first, unchanged.
- [x] 7. Replace `src/rts_run.rs::right_click_order` with shared API call. Evidence: `right_click_order` fn and `RtsSession::group` scratch deleted; `RtsCommand::RightClick` now calls `world.issue_context_order_at(&view, p, &mut session.receipts)` directly; `cargo check --workspace --all-targets --all-features --locked` passes.
- [x] 8. Extend CLI regression to click away from node center. Evidence: `tests/rts_cli_contract.rs::a_right_click_on_a_node_starts_gathering` now clicks `crystal_node_corner_screen()` (a corner of the node's 48×48 quad, `screen_of(140.5,150.5) + (-20,-4)`), not the centre; `cargo test --locked --test rts_cli_contract right_click` passes.

## Outputs

- Modified: selection/pack/world/mod/app + selection/economy/CLI tests.
- Public API: helpers + receipt/result types/signature above.
- Behavior: visible quad/body selection; frontmost hit; shared live/headless context orders.
- Migrate/config: none.

## Validation

- [x] `cargo test -p mmd-engine --locked --test rts_selection` — 36 passed; 0 failed.
- [x] `cargo test -p mmd-engine --locked --test rts_economy click_path` — `click_path_gather_banks_crystal ... ok` (1 passed).
- [x] `cargo test -p millions_must_die --locked --test rts_cli_contract right_click` — 3 passed (`a_right_click_on_a_node_starts_gathering`, `a_right_click_moves_the_selection`, `a_right_click_cancels_the_ghost_without_ordering`).
- [x] `cargo check --workspace --all-targets --all-features --locked` — clean, no warnings/errors.
- [x] manual check: selected worker right-clicks four resource-square corners → Gather starts. Evidence: `every_resource_quad_corner_is_pickable` proves all four inside corners resolve `Pick::Node`; `a_right_click_on_a_node_starts_gathering` proves a corner right-click on a selected group actually starts Gather (crystal banks above the starting 300).
- [x] app functional: `cargo run -- rts --frames 1600 --inject-input-file assets/scenarios/rts_acceptance_v1.script` — the ticket's literal `--frames 160` exits early with `rts failed: --inject-input entries never fired` (the tracked script's last scripted event is at tick 1450; this is true on this branch before this ticket's changes too — not something T2 introduced). Ran the frame count `AGENT.md`'s merge-gate line and this same script use elsewhere in the repo (`1600`); exits `quit=true` clean with `hash=73e65fca25299a97d07e5acd85b08d47e4a38fb71f993dd44026be7b8b6e87af`. Logged under Assumptions below.
- [x] commit msg draft: `fix(rts): make visible pick geometry drive context orders` — used verbatim as the commit subject.
