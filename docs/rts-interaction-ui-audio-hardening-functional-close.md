# RTS Interaction, UI and Audio Hardening — Functional Close (Phase 1.1)

**Status: closed on functional evidence, 2026-08-12.**

Phase 1 proved an RTS engine could run. Phase 1.1 asked a narrower question:
does the game a *person* drives behave like the game the headless tests drive?
It did not. A visible resource sprite is 48 × 48 px and the picker recognised
one map cell, so most visible clicks became a Move; units merged into each
other because nothing resolved an overlap; there was no minimap, no command
card, no menu, no settings, no window mode, no sound.

Phase 1.1 closes that gap on functional scope only. Every entry in the
[system → test map](#system--test-map) is backed by named automated tests that
run on the merge gate, and one 1,600-frame script drives the whole loop —
select, gather, build through the command card, produce, minimap jump, pause
menu, settings edit, quit — through both the engine and the shipped binary.

Phase 1.1 did **not** ask how fast any of it runs, and this document claims
nothing about that. Performance stays retired to a later optimization phase,
exactly as in phases 0 and 1.

What gates a merge is defined in one place and one place only:
[testing strategy](05-testing.md). This document explains what that gate
proves; it does not add to it. The decisions behind the code are
[ADR 016](ADR/016_ADR_phase1_1_scope_and_input_geometry.md),
[ADR 017](ADR/017_ADR_rts_hard_collision_navigation_and_formations.md),
[ADR 018](ADR/018_ADR_settings_window_canvas_and_camera.md),
[ADR 019](ADR/019_ADR_hud_minimap_and_input_routing.md) and
[ADR 020](ADR/020_ADR_audio_events_buses_and_generated_assets.md), with the
shape of the whole slice on the
[architecture page](rts-interaction-ui-audio-hardening-architecture.html).
Phase 1's own record stands untouched and is not rewritten as if it had
included any of this:
[RTS engine prototype functional close](rts-engine-prototype-functional-close.md).

## What phase 1.1 proves

Eleven systems, all inside the RTS slice.

| Claim | Status |
| --- | --- |
| A click on any part of a rendered resource sprite hits that node; a unit is hit by its sprite quad **or** its body circle; ties resolve exactly as the renderer's depth test resolved them | proven — `crates/mmd-engine/tests/rts_selection.rs` |
| One engine call turns a right-click into per-unit Gather / Build / Move receipts, and live input, scripts and the harness all go through it | proven — `crates/mmd-engine/tests/rts_economy.rs` |
| No completed tick leaves two RTS unit bodies merged — including head-on, crossing, idle bystanders and pushed chains | proven — `crates/mmd-engine/tests/rts_collision.rs` |
| A 3-cell body keeps its clearance from terrain, nodes, finished buildings and map edges, and stops outside a target rather than on it | proven — `crates/mmd-engine/tests/rts_radius_nav.rs` |
| A group order takes one pooled field and gives every member a distinct deterministic slot, or is refused whole | proven — `crates/mmd-engine/tests/rts_formation.rs` |
| Production and construction never place or trap a body inside another: they spawn on the nearest free position, or wait, paid and reserved | proven — `crates/mmd-engine/tests/rts_production.rs`, `crates/mmd-engine/tests/rts_build.rs` |
| Settings persist per user, validate on load, fall back to defaults with a warning, and are never touched by an offscreen run | proven — `src/rts_settings.rs` |
| The 1920 × 1080 logical canvas survives any window shape as an exact centred 16:9 rect, with bars that reject clicks | proven — `crates/mmd-engine/tests/display_viewport.rs` |
| Three window modes apply, roll back on failure, and focus loss clears every held input | proven — `src/rts_window.rs` |
| The camera clamps to the projected map frontier, with independent keyboard and edge speeds | proven — `crates/mmd-engine/tests/camera.rs` |
| The HUD owns its regions and wins the pointer before the world does; the minimap round-trips and recentres | proven — `crates/mmd-engine/tests/rts_hud.rs`, `crates/mmd-engine/tests/rts_minimap.rs` |
| Accepted actions — never post-hoc guesses — produce deterministic, capped, bus-weighted audio events | proven — `src/rts_feedback.rs`, `src/rts_audio.rs` |
| Every audio asset is generated from integer code, byte-identical on regeneration, and carries a manifest | proven — `xtask/src/audio.rs`, on the gate as `cargo run -p xtask -- audio --check` |
| The whole live-equivalent loop runs from one tracked 1,600-frame script, through the engine and through the shipped binary | proven — `crates/mmd-engine/tests/rts_acceptance.rs`, `tests/rts_acceptance.rs`, both on the merge gate |
| An RTS tick still allocates nothing after warmup, collision, formations and HUD included | proven — `crates/mmd-engine/tests/frame_allocations.rs` |
| Phase 0 is undisturbed: the horde's exit-line hash, the render golden, the scenario contract, and `crates/mmd-engine/src/sim` byte-for-byte | proven — `git diff --exit-code -- crates/mmd-engine/src/sim` is clean against phase 1, and the phase-0 suites are unchanged |
| Performance | **unmeasured.** Retired to a later optimization phase; nothing here is a speed claim |
| Anything you can hear or see on real hardware | **unproven.** Offscreen runs open no window, grab no pointer and open no audio device; see [proof boundaries](#proof-boundaries--what-no-automated-test-here-can-claim) |

## What phase 1.1 does not prove

- **No combat, no enemy AI, no fog of war, no zoom, no save/load**, no second
  faction, no victory condition. Unchanged from phase 1.
- **No hard collision for the horde.** ADR 009 stands: `crates/mmd-engine/src/sim`
  is soft separation steering over 5 000 overlap-capable agents. The hard-body
  invariant is RTS-only, and any doc that drops that scope is wrong.
- **No audible claim.** Nothing on the gate proves a sound was heard. The
  dummy-driver tests prove the device API and lifecycle; hearing it is a human
  checklist item.
- **No physical display claim.** Exclusive fullscreen, pointer confinement,
  Alt-Tab and compositor behaviour are proven as pure state sequences only.
- **No licensed soundtrack.** Every WAV is a generated MIT-0 placeholder. No
  StarCraft or other copyrighted audio file exists in this repo.
- **No cross-platform verification.** Linux/Vulkan development host only.
- **No balance pass, no real art.** Every cost, time, sprite and cue is a
  placeholder chosen to make the systems observable.

## System → test map

The eleven systems are pinned by the `PHASE1_1_SYSTEMS` list in
`tests/validation_contract.rs`, which resolves one representative name per
system against a source scan of the tree *and* against this page. The whole
table below is resolved the same way by
`phase1_1_close_names_only_real_tests`: every name in the third column must
exist as a live `#[test]` fn in the file in the second. This table is
therefore checked, not merely written.

| System | Test binary | Named tests |
| --- | --- | --- |
| pick geometry — sprite quad ∪ body circle, frontmost depth | `crates/mmd-engine/tests/rts_selection.rs` | `the_pick_radius_is_the_body_radius`, `every_resource_quad_corner_is_pickable`, `unit_pick_is_sprite_rect_union_body_circle`, `sprite_screen_rect_is_forty_eight_pixels_square`, `entity_pick_depth_matches_the_render_ground_y`, `frontmost_rendered_entity_wins`, `equal_depth_ties_go_to_the_lower_entity_slot`, `clicking_a_node_selects_it`, `clicking_a_node_one_cell_off_misses_it`, `box_never_selects_nodes`, `state_hash_sees_the_selection` |
| context orders — one API for live, scripted and harness input | `crates/mmd-engine/tests/rts_economy.rs` | `click_path_gather_banks_crystal`, `mixed_resource_order_partitions_by_capability`, `gatherers_get_distinct_legal_approaches`, `nonworkers_move_around_resource`, `context_order_with_no_selection_is_a_no_op`, `context_receipts_allocate_nothing_after_new`, `delivery_uses_the_footprint_not_the_centre` |
| hard bodies — contact, sweep, push chain, deflection | `crates/mmd-engine/tests/rts_collision.rs` | `touching_bodies_do_not_overlap`, `sub_six_distance_penetrates`, `a_sweep_catches_what_the_endpoints_miss`, `head_on_units_never_penetrate`, `crossing_units_cannot_tunnel`, `idle_units_are_collision_bodies`, `all_rts_owners_collide`, `a_push_chain_moves_a_row_of_bodies`, `a_chain_that_cannot_end_legally_moves_nobody`, `a_push_chain_stops_at_its_depth_bound`, `no_body_is_displaced_twice_in_one_tick`, `a_mover_deflects_around_a_body_it_cannot_shove`, `a_shovable_body_is_shoved_rather_than_walked_around`, `priority_rotates_deterministically`, `forced_overlap_is_repaired`, `hard_collision_is_reproducible` |
| static navigation — body-inflated centres, clearance, approach cells | `crates/mmd-engine/tests/rts_radius_nav.rs` | `body_clears_map_edges`, `body_clears_static_rectangles`, `five_cell_corridor_is_unreachable`, `wide_corridor_is_reachable`, `sites_remain_walkable_until_completion`, `a_full_gather_round_trip_credits_crystal_on_the_tracked_scenario`, `approach_targets_stay_outside_solids`, `initial_workers_are_relocated_without_overlap`, `initial_spawn_relocation_is_deterministic` |
| formations — deterministic slots, atomic reject, choke fairness | `crates/mmd-engine/tests/rts_formation.rs` | `group_input_order_does_not_change_slots`, `group_move_acquires_one_anchor_field`, `formation_slots_are_six_cells_apart`, `formation_order_is_atomic_when_space_missing`, `terminal_steering_uses_no_new_field`, `a_group_spreads_into_distinct_final_positions`, `a_lone_unit_stops_on_its_slot`, `choke_priority_rotates`, `state_hash_sees_the_formation_slot` |
| body-safe production — nearest free spawn, wait and resume | `crates/mmd-engine/tests/rts_production.rs` | `production_uses_nearest_free_body_position`, `production_waits_when_no_spawn_is_free`, `waiting_production_resumes_once`, `new_spawn_joins_same_tick_collision`, `a_produced_unit_spawns_beside_its_building`, `a_produced_unit_walks_to_the_rally`, `queue_advance_completes_at_the_documented_tick`, `supply_used_is_recomputed_not_incremented` |
| body-safe construction — atomic evacuation or wait | `crates/mmd-engine/tests/rts_build.rs` | `completion_evacuates_every_overlapping_body`, `completion_waits_when_evacuation_impossible`, `later_completion_sees_earlier_building`, `builders_get_distinct_site_approaches`, `a_group_of_builders_finishes_the_site`, `a_site_is_walkable`, `a_finished_depot_blocks_navigation`, `finishing_invalidates_the_cached_fields` |
| world, orders and movement under bodies | `crates/mmd-engine/tests/rts_world.rs` | `rts_unit_body_radius_is_three_cells`, `rts_unit_speeds_are_tripled`, `worker_outruns_soldier`, `order_move_snaps_a_blocked_destination_to_a_legal_anchor`, `order_move_group_acquires_once`, `a_group_sharing_a_destination_shares_a_field`, `arrival_is_measured_from_the_slot_centre`, `a_unit_never_enters_a_blocked_cell`, `an_unreachable_destination_clears_the_order`, `movement_is_reproducible`, `state_hash_is_reproducible_across_worlds` |
| navigation staleness — live orders across evictions | `crates/mmd-engine/tests/rts_nav_staleness.rs` | `a_walking_unit_re_paths_when_a_building_blocks_its_route`, `an_evicted_field_does_not_hang_a_gather`, `an_evicted_field_does_not_hang_a_build`, `a_unit_caught_in_a_finished_footprint_escapes`, `a_unit_caught_in_a_finished_footprint_can_still_gather` |
| settings — schema 1, validation, fallback, offscreen isolation | `src/rts_settings.rs` | `defaults_match_phase_1_1_contract`, `settings_validate_steps_and_bounds`, `missing_file_loads_defaults`, `malformed_file_warns_and_uses_defaults`, `unsupported_schema_warns`, `save_load_round_trip_is_canonical`, `failed_replace_restores_backup`, `offscreen_run_does_not_touch_settings` |
| logical canvas — exact 16:9 fit, inverse pointer, bars | `crates/mmd-engine/tests/display_viewport.rs` | `exact_canvas_uses_full_drawable`, `ultrawide_pillarboxes`, `four_three_letterboxes`, `odd_drawable_keeps_exact_ratio`, `too_small_drawable_has_no_fit`, `hidpi_pointer_inverse_round_trips`, `round_trip_inverts_render_transform_pillarbox_and_letterbox`, `bar_click_is_outside`, `bar_motion_clamps_to_edge` |
| window modes — sequences, rollback, focus and grab | `src/rts_window.rs` | `borderless_desktop_is_default_sequence`, `exclusive_chooses_closest_1920x1080`, `choose_exclusive_mode_prefers_closest_geometry_over_any_refresh`, `windowed_is_1280x720_resizable`, `failed_mode_change_rolls_back_before_reclaim`, `failed_mode_change_with_failed_rollback_is_actionable_fatal`, `focus_loss_clears_every_held_input`, `focus_loss_releases_pointer`, `focus_gain_restores_configured_grab`, `pause_request_respects_toggle`, `refresh_viewport_falls_back_to_identity_below_one_16x9_unit` |
| camera frontier — projected clamp, split speeds | `crates/mmd-engine/tests/camera.rs` | `frontier_shrinks_projected_map_by_view`, `undersized_axis_collapses_to_midpoint`, `camera_cannot_cross_any_frontier_edge`, `look_at_point_clamps_fractional_target`, `a_non_finite_pan_is_ignored`, `a_non_finite_look_at_point_is_ignored`, `screen_axes_map_to_cell_axes`, `edge_pan_fires_only_inside_the_margin`, `edge_pan_corner_pans_both_axes`, `look_at_cell_centres_that_cell` |
| HUD — regions, selection card, command card, hit routing | `crates/mmd-engine/tests/rts_hud.rs` | `rts_frame_ui_groups_are_texture_slots_4_through_8`, `hud_uses_only_the_five_ui_groups`, `hud_regions_cover_bottom_without_overlap`, `the_gear_icon_appears_in_the_top_bar`, `single_selection_draws_portrait_and_full_details`, `multi_selection_draws_first_24_sorted_icons`, `multi_selection_skips_stale_ids`, `worker_card_uses_stable_three_build_slots`, `producer_cards_show_train_and_rally`, `mixed_or_empty_selection_disables_card`, `hit_gear_is_gear`, `hit_selection_icon_click_isolates`, `hit_shift_icon_click_toggles`, `hit_command_grid_maps_to_the_clicked_slot`, `hit_disabled_command_slot_is_still_a_hit_caller_must_gate_enabled`, `hit_hud_background_never_orders_world`, `hit_a_point_off_the_hud_is_the_world`, `settings_menu_contains_only_settings_action`, `settings_back_button_hits`, `settings_window_mode_buttons_hit_their_own_index`, `settings_tracks_snap_within_bounds`, `settings_pack_modal_settings_page_draws_every_control_and_warning`, `pack_hud_does_not_mutate_the_world` |
| minimap — diamond projection, camera polygon, click | `crates/mmd-engine/tests/rts_minimap.rs` | `minimap_projection_round_trips_map_corners`, `minimap_projection_matches_the_helper_built_from_a_world`, `outside_diamond_is_rejected`, `camera_polygon_projects_four_view_corners` |
| menu and settings panel — pages, pause reasons, live edits | `src/rts_ui.rs` | `default_pointer_owner_is_none`, `gameplay_escape_opens_paused_menu`, `escape_backs_out_one_level`, `manual_pause_survives_menu_close`, `space_never_changes_the_page`, `focus_toggle_controls_pause_reason`, `gear_only_opens_from_gameplay`, `modal_owns_every_pointer_point_while_open`, `settings_button_and_back_button_hit`, `sliders_snap_to_legal_steps`, `each_accepted_change_saves_once`, `failed_save_rolls_back_runtime_and_cfg`, `window_mode_change_uses_safe_transition`, `a_rejected_change_never_touches_runtime_or_disk`, `no_window_skips_runtime_but_still_saves`, `keyboard_and_edge_pan_apply_to_the_world`, `a_committed_volume_edit_pushes_new_gains`, `a_failed_gain_push_rolls_back_like_a_failed_save`, `a_failed_save_restores_the_old_gains`, `settings_pan_and_volume_bounds_match_app_contract` |
| audio events and buses — receipts to cues, caps, gains | `src/rts_feedback.rs` | `default_effective_gains_are_exact`, `muted_master_zeroes_every_bus`, `selection_cues_only_new_player_units`, `selection_batch_is_sorted_and_capped`, `mixed_resource_order_uses_one_global_cap`, `partial_success_emits_voice_and_one_reject`, `total_rejection_emits_one_reject`, `accepted_overflow_is_not_rejection`, `ui_actions_map_to_sfx_sources`, `disabled_or_keyboard_action_has_no_ui_sfx`, `music_starts_once_and_survives_focus`, `audio_events_do_not_change_world_hash`, `fake_sink_never_grows_after_new` |
| audio runtime — device, 14 streams, lanes, watermark | `src/rts_audio.rs` | `loader_rejects_wrong_wav_spec_or_hash`, `loader_accepts_the_tracked_assets`, `music_watermark_queues_two_buffers`, `maintain_before_start_music_queues_nothing`, `voice_batch_replaces_eight_lanes`, `reject_uses_dedicated_voice_lane`, `fifth_ui_click_steals_oldest_sfx_lane`, `live_gain_change_updates_all_bus_streams`, `buffered_frame1_events_replay_once`, `dummy_driver_starts_and_maintains`, `audio_device_open_failure_is_actionable` |
| generated audio assets — deterministic WAVs and manifest | `xtask/src/audio.rs` | `generated_wavs_use_locked_pcm_format`, `generated_durations_are_exact`, `music_loop_endpoints_are_zero`, `cue_hashes_are_distinct`, `source_mix_has_headroom`, `double_generation_is_byte_identical`, `audio_check_detects_tamper`, `manifest_hashes_match_files`, `triangle_wave_peak_matches_amplitude`, `stereo_channels_are_duplicated` |
| script contract — tokens, coordinates, `quit` | `src/rts_script.rs` | `script_parses_every_kind`, `script_rejects_frame_zero`, `script_rejects_an_unknown_kind`, `script_rejects_an_unknown_key`, `script_rejects_bad_coordinates`, `script_rejects_an_empty_entry`, `parse_file_text_strips_comments`, `parse_file_text_accepts_semicolons_too`, `parse_file_text_rejects_a_bad_entry`, `parse_file_text_rejects_a_script_with_no_entries`, `the_tracked_script_parses`, `quit_stops_the_frames_sweep`, `quit_takes_no_arguments`, `quit_parses_bare` |
| key bindings — mouse and keyboard parity | `src/rts_input.rs` | `keyboard_and_script_agree`, `pan_keyboard_and_script_agree`, `each_key_binds_one_command`, `no_key_is_both_a_command_and_a_pan`, `the_build_menu_matches_the_bindings`, `unbound_keys_are_rejected`, `the_window_banner_lists_every_binding` |
| app and CLI — the `rts` subcommand, end to end | `tests/rts_cli_contract.rs` | `script_coordinates_remain_logical`, `offscreen_never_builds_window_adapter`, `focus_state_never_diverges_from_default_offscreen`, `offscreen_settings_run_does_not_touch_settings`, `hud_command_grid_click_shares_the_keyboard_executor`, `hud_disabled_command_slot_is_consumed_without_action`, `hud_rally_arms_and_waits_for_the_next_world_click`, `hud_minimap_click_recentres_the_camera`, `hud_background_click_never_reaches_the_world`, `menu_escape_opens_the_pause_menu_without_quitting`, `menu_escape_nesting_backs_out_to_gameplay`, `menu_manual_pause_survives_the_menu`, `menu_gear_click_opens_the_menu_exactly_like_escape`, `menu_settings_button_navigates_and_back_returns`, `menu_modal_consumes_clicks_outside_its_own_controls`, `audio_events_start_music_once_with_exact_default_gains`, `audio_events_music_survives_the_pause_menu`, `audio_events_voice_new_selection_only_once`, `audio_events_order_batch_is_capped_and_rejectless`, `audio_events_only_pointer_ui_actions_click`, `audio_events_map_gear_and_minimap_sources`, `audio_events_keep_the_world_hash_stable`, `audio_offscreen_survives_invalid_audio_driver`, `the_exit_line_reports_every_counter`, `no_gpu_exits_with_code_three`, `the_window_is_released_before_it_drops` |
| acceptance — through the engine | `crates/mmd-engine/tests/rts_acceptance.rs` | `the_full_economy_loop_runs_end_to_end`, `acceptance_never_has_body_penetration`, `the_body_oracle_sees_a_forced_penetration`, `resource_click_uses_visible_quad_corner`, `acceptance_minimap_click_moves_camera`, `the_acceptance_run_is_reproducible`, `the_script_coordinates_hit_what_they_name` |
| acceptance — through the shipped binary | `tests/rts_acceptance.rs` | `the_tracked_script_runs_clean`, `the_acceptance_run_builds_two_buildings`, `the_acceptance_run_produces_a_soldier`, `the_acceptance_run_earns_crystal`, `the_acceptance_run_earns_gas`, `the_acceptance_run_pans_the_camera`, `the_acceptance_run_raises_the_supply_cap`, `the_acceptance_run_is_deterministic`, `acceptance_never_has_body_penetration`, `acceptance_uses_command_card_for_build_and_produce`, `acceptance_menu_settings_round_trip_resumes`, `acceptance_audio_counts_are_exact`, `phase1_1_run_is_cross_process_deterministic`, `the_acceptance_run_fires_every_entry`, `inject_input_and_file_are_mutually_exclusive`, `a_missing_script_file_is_actionable`, `comments_and_blank_lines_are_ignored` |
| allocation invariant — collision, formations, HUD, joined frame | `crates/mmd-engine/tests/frame_allocations.rs` | `pack_frame_allocates_nothing`, `new_hud_pack_allocates_nothing`, `cold_field_acquire_allocates_nothing`, `hard_collision_tick_allocates_nothing`, `formation_planning_allocates_nothing`, `blocked_transitions_allocate_nothing`, `joined_phase1_1_frame_allocates_nothing` |
| render packing under the moved camera | `crates/mmd-engine/tests/rts_pack.rs` | `keyboard_pan_moves_the_camera_at_the_documented_speed`, `keyboard_and_edge_speeds_are_independent`, `look_at_map_point_moves_the_camera_and_hashes`, `depth_uniforms_follow_the_panned_camera`, `state_hash_sees_the_camera`, `a_selected_building_gets_a_ring_sized_to_its_footprint` |
| scenario contracts — the 512-cell RTS cap | `crates/mmd-engine/tests/scenario_contract.rs` | `rts_map_edge_cap_is_512`, `tracked_rts_scene_remains_320_by_320`, `rts_scene_compat_radius_matches_three_cells`, `rts_obstacles_match_the_published_formula`, `rts_scene_geometry_is_locked`, `rts_baseline_spec_is_valid` |
| docs and gate contract | `tests/validation_contract.rs` | `phase1_1_close_names_only_real_tests`, `phase1_1_systems_have_behavioral_tests`, `required_gate_contains_audio_check`, `required_gate_keeps_phase_smokes`, `no_perf_claim_in_docs`, `adr_index_lists_every_adr_file`, `every_doc_link_resolves` |

The table names representative tests per system, not the whole suite — the
suite is larger. Every name above runs on a plain `cargo test`; the contract
test refuses an `#[ignore]`d name in this table rather than letting one pass as
everyday proof. The one ignored test phase 1.1 added is *not* mapped here: see
[known gap 1](#1-a-nav-channel-the-centre-mask-and-the-sweep-disagree-about).

### The interactive smoke

```sh
cargo run -- rts --frames 1600 --inject-input-file assets/scenarios/rts_acceptance_v1.script
```

Same command and same 1,600-frame budget as phase 1 — the script inside it now
drives the whole phase-1.1 surface: box-select the starting workers, gather
crystal and gas by clicking the visible sprite corner, build a Depot and a
Barracks **through the command card**, produce a Worker and a Soldier, jump the
camera with a minimap click, hold a keyboard pan, open the pause menu, open
Settings, drag the keyboard-pan track, back out of both pages, and quit through
the `quit` script token.

The run's exit line ends with exactly:

```text
body_overlaps=0 ui_page=gameplay music_starts=1 voice_select=8 voice_order=9 voice_reject=1 sfx_ui=8 keyboard_pan=78
```

Every field is asserted, in both the engine and the binary layers, and a
scripted event that never fires fails the run. The `body_overlaps=0` field is
computed by `RtsWorld::body_overlap_count` over every live unit pair, and
`the_body_oracle_sees_a_forced_penetration` proves that oracle can fail — so
the zero means "checked", not "never looked at".

## What landed differently from the plan

Recorded here because the decision records were written before the code. Each
is also carried in the ADR it belongs to.

1. **Interaction reach became adaptive.** The planned flat
   `body radius + tolerance` reach strands a unit whenever dense inflated
   terrain pushes every legal approach cell past it. The landed rule is
   `orders::adaptive_reach` = `max(orders::interaction_reach(kind),
   chosen approach-cell rect distance + NAV_CENTER_TOLERANCE_CELLS)`, and
   `orders::entity_approach_cell` returns `(Cell, distance)` so the chosen
   cell's own distance is available. See
   [ADR 017](ADR/017_ADR_rts_hard_collision_navigation_and_formations.md).
2. **Hard collision needed push-aside, a bounded chain and a deflection
   fallback.** Implemented literally — accept the whole candidate or stand
   still — the game froze, because a pooled field is body-blind and the tracked
   scene seeds workers one body diameter apart. A mover now displaces bodies
   along their contact normals, to `MAX_PUSH_DEPTH = 3` links and
   `MAX_PUSHED_BODIES = 8` bodies, one displacement per body per tick,
   all-or-nothing over the chain; only after a chain is rejected does it try
   its descent rotated -45°, +45°, -90°, +90°. Nothing relaxes: every displaced
   body's final position is proven legal before anything commits, and the
   no-merge invariant is unchanged. This supersedes the earlier
   "no push, no relaxation" wording.
3. **Arrival became slot-based.** `ARRIVAL_RADIUS_CELLS` is gone; a unit stops
   within `FORMATION_ARRIVAL_CELLS = 0.25` of *its own* slot centre. A shared
   radius cannot express "the group arrived" once each member owns a slot.
4. **Production drains through `ProductionQueue` self-methods** —
   `ProductionQueue::tick_head`, `ProductionQueue::head_ready`,
   `ProductionQueue::pop_ready` — not an EntityId-keyed API. The queue holds
   the payment, so the wait/resume decision belongs to it.
5. **Escape opens the pause menu instead of quitting**, which removed the only
   way a scripted run could end itself. `src/rts_script.rs` gained a bare
   `quit` token, and the tracked script ends with it.
6. **Hit testing lives in `crates/mmd-engine/src/rts/minimap.rs`.** `HudHit`
   and `minimap::hud_hit_test` resolve against the same `MinimapProjection` the
   minimap draws with; splitting them would have duplicated that projection.
7. **Settings are observed on stdout, not in the HUD.** A fallback prints
   `rts: settings warning=…` and the effective values are echoed on the
   `rts: settings …` startup line. The in-panel `SETTINGS NOT SAVED:` warning
   is a different surface, for live edit failures.
8. **A scripted settings edit commits in memory only.** The acceptance run
   moves `camera.keyboard_pan` to 78 and reports it, without writing the real
   user's settings file — offscreen runs never touch the pref path at all.

## Proof boundaries — what no automated test here can claim

The gate runs offscreen. That is deliberate: it is the only way the merge gate
can be deterministic. It also means the following are proven as *state
sequences and API calls*, never as physical behaviour, and each has a human
checklist item in `ai_artefacts/manual_test_checklist.md`:

| Not claimed by any test on the gate | What is claimed instead |
| --- | --- |
| A window appears, in any mode | `src/rts_window.rs` proves the request/rollback/refresh sequence a mode change issues |
| Exclusive fullscreen actually switches the display | The chosen mode is the closest 1920 × 1080 entry, highest refresh on a tie |
| The pointer is really confined by the compositor | Grab/release is requested on focus change and cleared on loss |
| Anything was audible | The device is opened, streams are bound and gains are pushed; the counters are derived from accepted actions |
| Loudness, mix balance, loop click | The WAV bytes are byte-identical on regeneration and the loop endpoints are zero samples |
| Any non-development host | Nothing. Render goldens stay host-scoped |

## Known gaps — all non-blocking

### 1. A nav channel the centre mask and the sweep disagree about

`StaticNav::center_blocked` marks a 1–2 cell diagonal channel of the tracked
scene (y ≈ 172..174, x ≈ 190..199) as legal centres, but no step through it
survives `StaticNav::sweep_clear` plus `sim::step_admissible`: the field claims
the channel is reachable and a 3-cell body wedges in it. It is **pre-existing**
— the per-cell circle test and the continuous sweep were never required to
agree — and it is pinned rather than fixed, as an `#[ignore]`d reproducer,
`a_body_wedges_in_the_narrow_eastern_channel` in
`crates/mmd-engine/tests/rts_nav_staleness.rs`. Reconciling the two is a
navigation change with its own ticket, not a documentation change. Nothing on
the tracked acceptance path routes through that channel.

### 2. Collision is O(U²) by choice

Every candidate step is swept against every other live body. Proof simplicity
won, and no number gates this phase. Revisit it in the optimization phase, with
a measurement, not a guess.

### 3. The horde still overlaps, and always will in phase 1.1

`crates/mmd-engine/src/sim` is untouched — `git diff --exit-code` clean against
phase 1. Any future doc claiming the horde cannot overlap is wrong; ADR 009
remains authoritative there.

### 4. Audio is placeholder and unmixed

Seven generated WAVs, integer oscillators, no polyphony normalisation. Muted
streams keep progressing rather than stopping. A licensed replacement needs
source, license, attribution and checksum before it may be tracked.

### 5. The command card is context-driven but small

Three build slots for workers, one train slot plus rally for a finished
producer, disabled everywhere else. Anything richer needs a real ability
system.

### 6. `gpu_smoke::the_hud_draws_over_the_world` remains environment-sensitive

Carried forward unchanged from the phase-1 close: it has failed and passed at
the same commit on different machines, and was reconfirmed as environmental
during this phase by re-running it against a stashed baseline. Recorded, not
quarantined.

### 7. The acceptance script is still one path

It proves the documented loop, now including HUD, menu, minimap and audio. It
does not explore alternative orders, mid-flight cancellations or contention.
Each of those has unit coverage; none has an end-to-end run.

### 8. Determinism is same-host, same-binary

Unchanged from phases 0 and 1. `phase1_1_run_is_cross_process_deterministic`
proves this binary reproduces itself across processes; it claims nothing about
another compiler, optimization level or architecture.

## Phase-2 backlog

Carried forward, in no committed order:

- Reconcile `StaticNav::center_blocked` with `StaticNav::sweep_clear`, and
  un-ignore the pinned reproducer.
- Combat, enemy AI, fog of war, zoom — unchanged from the phase-1 backlog.
- A real art pass, and a licensed soundtrack with its provenance contract.
- Broaden the acceptance matrix beyond one scripted path.
- Revisit collision cost in the optimization phase, with measurement.

## Related

- [Phase 1.1 architecture](rts-interaction-ui-audio-hardening-architecture.html)
  — the shape of the slice as implemented, and the rules a contributor can get
  wrong.
- [Testing strategy](05-testing.md) — the single source of truth for the merge
  gate.
- [Phase 1 functional close](rts-engine-prototype-functional-close.md) — what
  the RTS prototype proved before this hardening pass.
- [Phase 0 functional close](technical-prototype-functional-close.md) — the
  horde engine, untouched by this phase.
</content>
