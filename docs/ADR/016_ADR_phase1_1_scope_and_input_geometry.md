# ADR 016: Phase-1.1 scope and input geometry

- Status: Accepted
- Date: 2026-08-10
- Accepted: 2026-08-12 (T18, on landed phase-1.1 evidence)
- Supplements: [ADR 013](013_ADR_phase1_scope_and_rts_entity_model.md), [ADR 014](014_ADR_movable_camera_texture_table_and_ui_layer.md), [ADR 015](015_ADR_economy_construction_and_production_determinism.md)
- Plan: `artifacts/PLAN_2026_08_10_rts-interaction-ui-audio-hardening.md`
- Source: `feedback.md`; confirmed interview at `artifacts/GRILL_2026_08_10_rts-feedback/ANSWERS.md`

## Context

Phase 1 closed on functional engine scope. Manual use then exposed mismatch: headless gather tests call `RtsWorld::order_gather` directly, while visible input passes through `pick_at`. Resource sprite is 48×48 px; picker recognizes one map cell. Most visible resource clicks become Move.

Unit selection has same coupling problem. Current pick radius equals scenario collision radius. Player selection geometry and physical body geometry need independent contracts.

Phase 1.1 is post-close hardening. It does not rewrite phase-1 evidence.

## Decision

### Scope

Phase 1.1 ships one player-facing vertical slice:

- visible pick correctness;
- hard RTS bodies/navigation/formations;
- settings/window/camera/HUD/minimap;
- generated placeholder audio;
- one live-equivalent deterministic acceptance run.

Horde `sim/`, combat, enemy AI implementation, zoom, fog, perf gates, copyrighted soundtrack stay out.

### Visible pick geometry

- Resource node: full rendered 48×48 standing quad.
- Unit: full rendered 48×48 standing quad **union** circular RTS body.
- Building: existing projected footprint.
- Transparent sprite-quad corners count. Stable/gameplay geometry beats alpha-mask asset coupling.

Picker uses same ground anchor/size/depth helpers as packer. No duplicated visual constants.

### Frontmost rule

Every hit competes by renderer depth:

1. greatest `IsoView::depth(ground_y)` wins;
2. equal depth → lower entity slot wins.

Reason: world pack order is ascending slot; strict `GREATER` keeps first equal-depth fragment.

### Context orders

One engine API serves live SDL, scripted input, and `RtsHarness`:

- resource: workers Gather; selected non-workers Move to legal resource approach;
- site: eligible workers Build; other selected units reject;
- ground/other entity: selected orderable units Move.

API returns sorted per-unit receipts + accepted/rejected totals. Audio later consumes facts; it never infers success after mutation.

### Tuning

RTS-only speeds:

- Worker: 30 cells/s;
- Soldier: 24 cells/s.

Horde remains 8 cells/s. Phase-0 hashes stay unchanged.

RTS map engine cap becomes 512×512 cells. Current tracked scene remains 320×320.

## Consequences

- Current `clicking_a_node_one_cell_off_misses_it` test becomes obsolete by design.
- Frontmost unit may intentionally intercept resource click. User chose render-front priority over resource priority.
- Selection ring/body can shrink without shrinking sprite pick rect.
- Shared context outcome becomes dependency for collision-safe approaches and audio cues.
- RTS state/timing hashes change. Horde state/hash must not.

## Rejected

- Exact opaque-pixel mask: asset-coupled, costly, fragile.
- Type priority (unit before building/node): disagrees with rendered frontmost choice.
- App-private right-click router: recreates headless/live drift.
- Triple horde speed: violates frozen phase-0 behavior.

## Implementation (as landed, T1–T2)

The decision shipped as written. Two corrections to this record, both found by
the code rather than by review:

1. **`clicking_a_node_one_cell_off_misses_it` is not obsolete.** Consequences
   above predicted the full-quad picker would delete it. It survives unchanged:
   the point it clicks (one cell off along the projected axis) lands outside
   the 48×48 quad as well, so it still proves a miss. What replaced the old
   one-cell picker is the pair `every_resource_quad_corner_is_pickable` and
   `sprite_screen_rect_is_forty_eight_pixels_square`.
2. **The context-order entry point is `RtsWorld::issue_context_order_at`**,
   returning a `ContextOrderResult` of sorted `IssuedOrder` receipts plus
   `ContextOrderReason`. One API for live SDL, scripts and `RtsHarness`, as
   decided; the name is recorded here because the plan did not fix one.

The geometry helpers landed as `sprite_screen_rect`, `unit_pick_contains` and
`entity_pick_depth` in `crates/mmd-engine/src/rts/selection.rs`, all called by
`pick_at` and provably the same rect/depth `pack_frame` draws
(`entity_pick_depth_matches_the_render_ground_y`). Tuning landed as
`RTS_MAX_MAP_EDGE = 512` in `scenario.rs` (`rts_map_edge_cap_is_512`,
`tracked_rts_scene_remains_320_by_320`) with 30/24 cells/s unit speeds
(`rts_unit_speeds_are_tripled`, `worker_outruns_soldier`) and the horde's
8 cells/s untouched.

## Validation contract

Planned tests must prove:

- every resource-quad corner inside hits; one pixel outside misses;
- unit rect-only and circle-only points both hit;
- depth + equal-slot tie match renderer;
- corner click banks resource through shared path;
- mixed selection partitions Gather/Move receipts;
- RTS 512 accepted, 513 rejected; tracked scene remains320;
- horde hashes/speeds unchanged.
