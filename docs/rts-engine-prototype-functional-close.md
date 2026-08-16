# RTS Engine Prototype — Functional Close (Phase 1)

**Status: closed on functional evidence, 2026-08-10.**

> **Followed by phase 1.1** (interaction, UI and audio hardening,
> [functional close](rts-interaction-ui-audio-hardening-functional-close.md)),
> which changed several of the gaps recorded below — units now have hard
> bodies, and a minimap, menus and sound exist. Nothing on this page has been
> rewritten to pretend it included any of that: it is the record of what phase
> 1 closed on, and it stays that way.

Phase 1 asked whether this engine can run an RTS: a camera you move, units you
select, workers that gather, an economy that banks, buildings you place, and
units you produce — all on a horde-free scene, without disturbing anything
phase 0 froze. It can: every entry in the
[system → test map](#system--test-map) below is backed by named automated tests
that run on the merge gate, and one tracked script drives the whole loop end to
end through both the engine and the shipped binary, including crystal and gas
income plus a held camera pan away from the starting base.

Phase 1 did **not** ask how fast any of it runs, and this document claims
nothing about that. Performance stays retired to a later optimization phase,
exactly as in phase 0.

The prose outside that map — the known gaps, the deviations from plan, the
phase-2 backlog — is a record of judgement, not of test output. Where a
statement is backed by a test, this page names it, and
`phase1_close_doc_names_only_real_tests` resolves every name in the map against
a scan of the binary it is mapped to.

What gates a merge is defined in one place and one place only:
[testing strategy](05-testing.md). This document explains what that gate
proves; it does not add to it. The decisions behind the code are
[ADR 013](ADR/013_ADR_phase1_scope_and_rts_entity_model.md),
[ADR 014](ADR/014_ADR_movable_camera_texture_table_and_ui_layer.md) and
[ADR 015](ADR/015_ADR_economy_construction_and_production_determinism.md), with
the shape of the whole slice on the
[architecture page](rts-engine-prototype-architecture.html).

## What phase 1 proves

Six systems, cut thin and vertical. Each is proven by its own test binary; the
representative names are in the map below.

| Claim | Status |
| --- | --- |
| The camera pans, edge-pans and clamps, and the depth key does not move with it | proven — `crates/mmd-engine/tests/camera.rs` |
| A click, a shift-click and a drag box select the entities they name, and a stale id never resurrects a reused slot | proven — `crates/mmd-engine/tests/rts_selection.rs` |
| A worker walks to a node, mines, walks to a drop-off and banks its cargo, repeatedly | proven — `crates/mmd-engine/tests/rts_economy.rs` |
| A node pays out exactly what is carried, empties, and stays as depleted scenery | proven — `crates/mmd-engine/tests/rts_economy.rs` |
| A building is placed on validated ground, is walkable as a site, advances only while attended, blocks navigation when finished, and refunds in full when cancelled | proven — `crates/mmd-engine/tests/rts_build.rs` |
| Production charges resources and *reserves* supply at enqueue, completes at its documented tick, spawns beside its building and walks to a rally point | proven — `crates/mmd-engine/tests/rts_production.rs` |
| The whole loop — select, gather both resources, build, produce and pan — runs from one tracked script, through the engine and through the shipped binary | proven — `crates/mmd-engine/tests/rts_acceptance.rs`, `tests/rts_acceptance.rs`, both on the merge gate |
| An RTS tick allocates nothing after warmup | proven — `crates/mmd-engine/tests/frame_allocations.rs`, one documented exception below |
| The RTS world reproduces itself on one host and one binary | proven — hash-equality tests, with the narrowing recorded under Known gaps |
| Phase 0 is undisturbed: the horde's exit-line hash, the render golden, `SpriteInstance`'s 48 bytes, `shaders/sprite.hlsl` and the `atlas_count/direction_count/frame_count` contract | proven — no phase-0 test was renamed or removed, the phase-0 suites gained phase-1 cases only, and `tests/cli_contract.rs` is byte-identical to its phase-0 self |
| Performance | **unmeasured.** Retired to a later optimization phase; nothing here is a speed claim |
| Any other platform | **unverified.** Render goldens are host-scoped; no cross-platform evidence exists |

## What phase 1 does not prove

- **No combat.** The Soldier has no weapon, no health and no combat behaviour;
  its selection-panel line reads `IDLE` because anything else would be a claim
  the code cannot support.
- **No enemy AI**, no second faction, no waves, no victory or defeat condition.
- **No zoom, no minimap, no fog of war**, no save/load, no menus, no sound.
- **No balance pass.** Every cost, time and supply number is a placeholder
  chosen to make the systems observable inside one acceptance run.
- **No real art.** Every phase-1 sprite is generated from code.
- **No performance number of any kind.** Nothing in phase 1 is measured,
  published or gated on speed.
- **No cross-platform verification.** Linux/Vulkan development host only. The
  goldens under `lab/goldens/` are host-scoped and prove nothing about any
  other backend.
- **No claim that units cannot overlap.** Separation steering bends a descent
  vector; it never resolves an overlap. Phase 1 does not add resolution, and
  player units are moved by the same admissibility rule the horde uses.

## System → test map

The six phase-1 systems are also pinned by the `SCOPE_SYSTEMS` list in
`tests/validation_contract.rs`, which resolves one representative name per
system against a source scan of the test tree *and* against this page. The
whole table below is resolved the same way by
`phase1_close_doc_names_only_real_tests`: every name in the third column must
exist as a `#[test]` fn in the binary in the second. This table is therefore
checked, not merely written.

| System | Test binary | Named tests |
| --- | --- | --- |
| camera — pan, edge pan, screen→cell, depth key, projected frontier | `crates/mmd-engine/tests/camera.rs` | `unproject_inverts_project_exactly`, `unproject_of_a_degenerate_tile_is_not_finite`, `cell_at_returns_the_cell_a_sprite_was_packed_from`, `cell_at_rejects_offscreen_and_off_grid`, `depth_key_is_camera_independent`, `frontier_shrinks_projected_map_by_view`, `undersized_axis_collapses_to_midpoint`, `camera_cannot_cross_any_frontier_edge`, `look_at_point_clamps_fractional_target`, `screen_axes_map_to_cell_axes`, `edge_pan_fires_only_inside_the_margin`, `look_at_cell_centres_that_cell` |
| selection — click, shift-click, drag box | `crates/mmd-engine/tests/rts_selection.rs` | `clicking_a_worker_selects_it`, `clicking_between_two_workers_picks_the_nearer`, `clicking_empty_ground_clears_the_selection`, `shift_click_adds`, `shift_click_on_a_selected_unit_removes_it`, `box_selects_every_own_unit_inside`, `box_excludes_units_outside`, `box_never_selects_buildings`, `box_respects_the_camera`, `retain_live_drops_a_stale_generation` |
| workers — the gather round trip | `crates/mmd-engine/tests/rts_economy.rs` | `a_worker_walks_to_its_node`, `a_worker_that_arrives_starts_mining_the_same_tick`, `mining_takes_the_documented_time`, `a_mining_worker_does_not_move`, `a_full_round_trip_banks_crystal`, `a_full_round_trip_banks_gas`, `the_worker_keeps_cycling`, `six_workers_on_one_node_all_deliver`, `delivery_uses_the_footprint_not_the_centre` |
| economy — nodes, cargo, drop-offs | `crates/mmd-engine/tests/rts_economy.rs` | `the_node_loses_exactly_the_carried_amount`, `a_partial_node_pays_out_what_is_left`, `an_emptied_node_ends_the_order`, `cargo_is_cleared_on_delivery`, `nearest_drop_off_prefers_the_closer_building`, `nearest_drop_off_ties_go_to_the_lower_slot`, `no_drop_off_stops_the_worker_holding_cargo`, `the_economy_is_reproducible` |
| building — placement, attended construction, cancel | `crates/mmd-engine/tests/rts_build.rs` | `costs_and_times_are_the_published_constants`, `placement_rejects_terrain`, `placement_rejects_overlap_with_a_site`, `a_unit_standing_there_does_not_block_placement`, `confirm_debits_the_cost`, `a_site_is_walkable`, `construction_does_not_advance_without_a_worker`, `construction_advances_one_tick_per_tick`, `a_second_worker_does_not_speed_it_up`, `a_depot_finishes_in_its_documented_time`, `a_finished_depot_blocks_navigation`, `finishing_invalidates_the_cached_fields`, `a_finished_depot_raises_the_supply_cap`, `the_supply_cap_is_clamped_at_the_pillar`, `cancel_refunds_the_full_cost`, `a_gatherer_still_delivers_after_the_hq_is_stamped` |
| unit production — queue, supply, rally | `crates/mmd-engine/tests/rts_production.rs` | `the_build_tree_is_hq_worker_and_barracks_soldier`, `queue_advance_completes_at_the_documented_tick`, `queueing_cannot_exceed_the_cap`, `enqueue_debits_immediately`, `enqueue_reserves_supply_immediately`, `enqueue_rejects_over_supply`, `a_supply_blocked_enqueue_does_not_charge`, `supply_used_is_recomputed_not_incremented`, `supply_used_counts_reservations`, `a_worker_appears_after_its_build_time`, `a_produced_unit_spawns_beside_its_building`, `a_produced_unit_walks_to_the_rally`, `production_stops_when_the_store_is_full`, `cancel_queued_refunds_and_releases`, `a_barracks_can_be_queued_the_tick_it_finishes` |
| entity store, orders and movement | `crates/mmd-engine/tests/rts_world.rs` | `a_stale_id_does_not_resolve_to_its_replacement`, `the_free_list_is_lifo`, `the_store_refuses_to_overfill`, `every_column_is_reserved_at_construction`, `world_seeds_the_scene`, `the_seeded_hq_is_stamped_into_navigation`, `a_phase0_scenario_is_refused`, `a_unit_reaches_its_destination`, `a_unit_walks_around_an_obstacle`, `a_unit_never_enters_a_blocked_cell`, `an_unreachable_destination_clears_the_order`, `arrival_is_measured_from_the_slot_centre`, `a_group_sharing_a_destination_shares_a_field`, `movement_is_reproducible`, `state_hash_is_reproducible_across_worlds` |
| navigation — the flow-field pool | `crates/mmd-engine/tests/nav_pool.rs` | `rebuild_in_place_matches_build`, `pool_hit_does_not_rebuild`, `pool_holds_eight_distinct_destinations`, `pool_evicts_the_least_recently_used`, `eviction_ties_prefer_the_lowest_slot`, `pool_rejects_a_blocked_destination`, `a_refused_acquire_preserves_every_cached_handle`, `set_blocked_invalidates_every_slot`, `set_blocked_stops_every_handle_being_current`, `set_blocked_changes_the_walk`, `an_evicted_slot_stops_being_current`, `a_cached_acquire_allocates_nothing`, `a_miss_may_grow_scratch_only_once` |
| navigation staleness — live RTS orders | `crates/mmd-engine/tests/rts_nav_staleness.rs` | `a_walking_unit_re_paths_when_a_building_blocks_its_route`, `an_evicted_field_does_not_hang_a_gather`, `an_evicted_field_does_not_hang_a_build`, `a_unit_caught_in_a_finished_footprint_escapes`, `a_unit_caught_in_a_finished_footprint_can_still_gather` |
| scenario family — the horde-free RTS scene | `crates/mmd-engine/tests/scenario_contract.rs` | `rts_scene_loads_verified`, `rts_scene_is_horde_free`, `rts_scene_geometry_is_locked`, `rts_scene_carries_its_block`, `a_nonzero_population_is_rejected_for_the_rts_family`, `a_nonzero_stretch_is_rejected_for_the_rts_family`, `an_rts_scene_without_a_block_is_rejected`, `a_phase0_family_with_an_rts_block_is_rejected`, `every_phase0_scene_has_no_rts_block`, `the_renderer_contract_still_binds_the_rts_family` |
| UI text — the bitmap font | `crates/mmd-engine/tests/ui_text.rs` | `glyph_rect_tiles_the_sheet_without_gaps`, `glyph_rects_are_unique_per_code_point`, `no_glyph_can_be_mistaken_for_a_ring`, `push_text_emits_one_instance_per_visible_glyph`, `space_advances_without_an_instance`, `lowercase_is_uppercased_not_boxed`, `lowercase_cell_is_the_fallback_box`, `begin_text_group_sets_the_font_slot`, `push_text_does_not_allocate_when_reserved` |
| world render packing | `crates/mmd-engine/tests/rts_pack.rs` | `frame_new_reserves_the_documented_groups`, `every_entity_packs_exactly_once`, `nodes_and_buildings_share_the_building_slot`, `workers_land_in_the_worker_slot`, `a_units_uv_is_its_dir_and_frame`, `sprites_stand_on_their_ground_point`, `culling_uses_the_same_rect_as_the_horde`, `packing_follows_the_camera`, `selection_rings_are_procedural`, `the_ghost_covers_the_whole_footprint`, `the_drag_box_normalises_its_corners`, `pack_frame_does_not_mutate_the_world` |
| HUD — stock, selection card, command grid | `crates/mmd-engine/tests/rts_hud.rs` | `hud_uses_only_the_five_ui_groups`, `rts_frame_ui_groups_are_texture_slots_4_through_8`, `the_top_bar_shows_the_stock`, `the_top_bar_shows_supply_as_a_ratio`, `supply_turns_red_at_the_cap`, `hud_regions_cover_bottom_without_overlap`, `single_selection_draws_portrait_and_full_details`, `multi_selection_draws_first_24_sorted_icons`, `worker_card_uses_stable_three_build_slots`, `producer_cards_show_train_and_rally`, `mixed_or_empty_selection_disables_card`, `the_hud_never_panics_on_a_stale_primary`, `pack_hud_does_not_mutate_the_world` |
| app and CLI — the `rts` subcommand | `tests/rts_cli_contract.rs` | `rts_runs_headless_and_exits_clean`, `frame0_line_reports_three_world_groups`, `the_run_is_deterministic`, `a_phase0_scenario_is_refused`, `an_unfired_entry_fails_the_run`, `a_click_selects_a_worker`, `a_drag_selects_the_group`, `a_right_click_on_a_node_starts_gathering`, `arrow_keys_pan_the_camera`, `w_opens_the_depot_ghost`, `a_left_click_places_the_ghost`, `a_produces_a_worker_at_the_hq`, `the_exit_line_reports_every_counter`, `no_gpu_exits_with_code_three`, `the_window_is_released_before_it_drops` |
| acceptance — through the engine | `crates/mmd-engine/tests/rts_acceptance.rs` | `the_full_economy_loop_runs_end_to_end`, `the_acceptance_run_is_reproducible`, `the_script_coordinates_hit_what_they_name` |
| acceptance — through the shipped binary | `tests/rts_acceptance.rs` | `the_tracked_script_runs_clean`, `the_acceptance_run_builds_two_buildings`, `the_acceptance_run_produces_a_soldier`, `the_acceptance_run_earns_crystal`, `the_acceptance_run_earns_gas`, `the_acceptance_run_pans_the_camera`, `the_acceptance_run_raises_the_supply_cap`, `the_acceptance_run_is_deterministic`, `the_acceptance_run_fires_every_entry` |
| allocation invariant — the RTS tick | `crates/mmd-engine/tests/frame_allocations.rs` | `pack_frame_allocates_nothing`, `new_hud_pack_allocates_nothing`, `movement_allocates_nothing`, `the_gather_loop_allocates_nothing`, `selection_operations_allocate_nothing`, `construction_allocates_nothing`, `production_allocates_nothing` |
| phase-0 render contracts, unmoved | `crates/mmd-engine/tests/render_correctness.rs` | `golden_frame_matches`, `every_tracked_manifest_pins_the_live_shader_and_atlas`, `packing_is_a_pure_projection_of_sim_state`, `a_ring_and_a_sprite_share_a_pass` |
| docs and gate contract | `tests/validation_contract.rs` | `every_system_has_a_test`, `phase1_close_doc_names_only_real_tests`, `no_perf_claim_in_docs`, `gate_list_has_no_perf_thresholds`, `adr_index_lists_every_adr_file`, `every_doc_link_resolves` |

The table names representative tests per system, not the whole suite — the
suite is larger. No phase-1 test is `#[ignore]`d: everything mapped above runs
on a plain `cargo test`, and the contract test refuses an ignored name in this
table rather than letting one pass as everyday proof.

### The interactive smoke

```sh
cargo run -- rts --frames 1600 --inject-input-file assets/scenarios/rts_acceptance_v1.script
```

One tracked script selects the starting workers, puts five on a crystal node
and one on a gas node, places and finishes a Depot and a Barracks, produces a
Worker and a Soldier, then holds a camera pan away from the base. It is asserted
twice — once through the engine by world state, milestone by milestone, and once
through the shipped binary by its exit line — and it is on the required merge
gate. It consumes no measurement number, and a scripted event that never fires
fails the run.

## Deviations from the plan, as built

Recorded here because the decision records were written before the code:

1. **Two visibility promotions in the frozen `sim/`, not one.**
   `sim::tick::dir_from_vector` became `pub`, and `sim::tick::step_admissible`
   became `pub(crate)` with a `#[cfg(test)]` re-export, so the RTS mover's own
   copy of the step rule could be proven equal to the horde's over an
   exhaustive grid. No signature and no body changed, and the horde's exit-line
   hash is unmoved. See
   [ADR 013](ADR/013_ADR_phase1_scope_and_rts_entity_model.md).
2. **The RTS scenario family (`assets/scenarios/rts_prototype_v1.ron`) is
   locked to 320 × 320 cells.** Several
   tickets sketched 32 × 32 test scenes; the validator refuses them, so those
   scenes were built at the locked size with their local geometry preserved
   unscaled.
3. **The overlay layer carries procedural rings only.** Every textured
   depth-off element — placement tiles, drag box, rally flag, resource and
   supply icons, HUD panel fill and glyphs — is a `ui` draw group. See
   [ADR 014](ADR/014_ADR_movable_camera_texture_table_and_ui_layer.md).
4. **Four test hooks beyond the planned API.** `RtsWorld::resources_mut` and
   `RtsWorld::nav_mut` are `#[cfg(feature = "testkit")]`, following the
   existing `sim/agents.rs` precedent, and are compiled out of the shipping
   binary. `nav::FieldPool::acquire_count` and
   `render::SpriteRenderer::pack_capacity` are unconditional read-only
   observation seams for mutation tests.
5. **A latent mover bug was fixed, not documented around.** The arrival-radius
   early stop was ending `Gather` and `Build` orders as well as `Move`; it now
   applies to `Order::Move` only, and the other two complete on their own reach
   tests. See
   [ADR 015](ADR/015_ADR_economy_construction_and_production_determinism.md).

## Post-review findings and fixes

Post-close review found three navigation failures with one structural cause.
Orders cached only a pool slot, but an LRU eviction or obstacle-mask change
could rebuild that slot for another field while the order stayed live. A
`FieldRef` now carries slot plus nonzero epoch; every moving `Move`, `Gather`
and `Build` order checks destination, slot and epoch, then re-acquires and
stores the replacement handle coherently before sampling it. Pool hits still
refresh LRU use, failed rebuilds preserve cached handles, and mask changes
invalidate every key.

Review also found units stranded when a site finished over their current cell.
Construction now relocates such units to the nearest in-bounds open cell using
a deterministic, allocation-free ring scan before movement resumes. The
starting HQ is stamped into the navigation mask during scenario seeding; its
approach ring remains open for gathering and production.

Finally, the headline acceptance path previously proved crystal income but not
gas income, and input entries for camera movement did not prove that the camera
moved. The tracked script now assigns one worker to gas and holds a right pan.
Engine milestones assert both resource banks and camera displacement; shipped
binary assertions read gas and camera centre from the exit line. Mutations that
disabled gas credit or camera pan failed both acceptance layers before being
restored.

## Known gaps — all non-blocking

These are recorded because they are true, not because they are outstanding
work for phase 1. **None of them blocks the close.** Each is either out of
phase-1 scope by decision, or a bounded narrowness the tests state honestly
rather than paper over.

### 1. Performance is entirely unmeasured

Unchanged from phase 0: there is no speed claim anywhere in this project's live
documentation, and none is added here. `no_perf_claim_in_docs` covers this page
and the phase-1 architecture page.

### 2. Placeholder art everywhere

Every phase-1 sprite and glyph is generated from code by
`xtask/src/placeholder_art.rs`. A real art pass will change every hash in
`assets/sprites/generated/rts/manifest.json` and `ui/manifest.json`, and
`cargo run -p xtask -- atlases --check` will fail until the new bytes are
reviewed and tracked. That is the intended failure mode, not a defect.

### 3. Additive build speed is deferred

`EXTRA_BUILDERS_SPEED_UP == false`: a second attendee at a construction site
changes nothing. The constant exists so the question reads as asked-and-deferred
rather than unconsidered, and `a_second_worker_does_not_speed_it_up` pins the
current answer.

### 4. The Soldier is a unit with nothing to do

No weapon, no health, no combat behaviour. It can be produced, selected, rallied
and moved, and that is all phase 1 claims.

### 5. Buildings cannot be destroyed

So `Supply::revoke_cap` and the "cap fell below usage" saturation are exercised
by unit tests only (`supply_free_saturates_when_the_cap_drops`,
`supply_new_clamps_to_the_pillar`), never in play. Both are written and tested
anyway — the arithmetic that underflows is the arithmetic nobody looked at.

### 6. The flow-field pool is eight slots

A player holding more than eight distinct live destinations will thrash it.
Thrashing stays *correct* — every order detects an evicted handle and every
miss rebuilds, while `a_miss_may_grow_scratch_only_once` shows the scratch heap
does not keep growing. No performance number gates this phase.

### 7. The camera is pan-only

No zoom, no minimap, no edge-scroll acceleration. Zoom multiplies through the
tile size, the depth normalisation, the cull and every render golden; it is
deferred with its own ticket in a later phase.

### 8. Depleted nodes stay on the map

A node that runs out is not despawned. It remains as scenery and draws a
distinct depleted sprite (`a_depleted_node_draws_the_depleted_sprite`), so a
player can see where the seam ran out.

### 9. A worker whose drop-off disappears keeps its cargo

It stops rather than re-targeting: the only other drop-off might be across the
map, and silently sending a worker there is worse than stopping visibly.
`no_drop_off_stops_the_worker_holding_cargo` is the guard. Nothing in phase 1
can remove a building, so this is reachable only from a test today.

### 10. One documented allocation exception

An RTS tick allocates nothing after warmup, with a single exception: a
flow-field **miss** rebuilds into the reused scratch heap and may grow it once,
at the grid's size. Tested in both directions —
`a_cached_acquire_allocates_nothing`, and `a_miss_may_grow_scratch_only_once`
cycles 32 fresh destinations after a warm 8 without growing it again.

### 11. Determinism is same-host, same-binary

Unchanged from phase 0: `f32` results are not claimed bit-identical across
compilers, optimization levels or architectures. Every hash-equality claim above
means "this binary on this host reproduces itself".

### 12. No cross-platform verification

Render goldens are host-scoped (`lab/goldens/linux-vulkan/`). Nothing in phase 1
was run on Windows or macOS, and the placeholder golden families still cannot
pass a comparison.

### 13. Known flakiness in the GPU-facing suites, recorded not fixed

Three observations, all made while landing this phase, none of them fixed:

- `gpu_smoke::the_hud_draws_over_the_world` was observed failing three runs out
  of three on one machine and passing two out of two on another **at the same
  commit**. The disagreement is not understood. It is recorded here rather than
  quarantined, because a test that fails on one host and passes on another is
  evidence about the harness, not a reason to stop running it.
- `VK_DRIVER_FILES=/nonexistent` is used to force the no-GPU path, and it does
  not force it consistently across runs.
- Running several GPU-touching test binaries in parallel can hit
  `VK_ERROR_DEVICE_LOST`. T14 and T15 steer around it by serialising their
  subprocess runs rather than fixing the cause.

The phase-0 pair `warmup_allocation_passes` / `panic_restores_guard` is not
known to be flaky any more; the per-thread `MeasureGuard` fix recorded in the
phase-0 close addressed the cross-talk that made it so.

### 14. The acceptance script is one path, not a matrix

It proves that the documented loop works, including both resource kinds and a
camera pan. It does not explore alternative orders, cancellations mid-flight,
or contention between many players' worth of commands. Every one of those has
unit coverage; none has an end-to-end run.

## Phase-2 backlog

Carried forward, in no committed order:

- Combat: weapons, damage, health, and whatever the horde and the RTS entity
  model have to agree on before a fight can happen.
- Decide where the horde lives once combat exists — inside `rts::EntityStore`,
  or beside it as today. ADR 013 deliberately does not pre-empt this.
- Zoom, minimap and fog of war.
- Destroyable buildings, which is what puts `Supply::revoke_cap` in play.
- Additive build speed, or a decision to keep attended construction flat.
- Understand the GPU-suite flakiness above before it hides a real regression.
- A real art pass, and with it a reviewed regeneration of every phase-1 atlas.

## Related

- [Phase 1 architecture](rts-engine-prototype-architecture.html) — the shape of
  the slice, and the rules a contributor can get wrong.
- [Testing strategy](05-testing.md) — the single source of truth for the merge
  gate.
- [Phase 0 functional close](technical-prototype-functional-close.md) — what
  the horde engine proved, and the gaps phase 1 inherited.
- [Phase 1.1 functional close](rts-interaction-ui-audio-hardening-functional-close.md)
  — the hardening pass that followed this one.
</content>
