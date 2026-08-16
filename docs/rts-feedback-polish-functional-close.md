# RTS Feedback Polish — Functional Close

**Status: closed on functional evidence, 2026-08-15, with one open regression
named below.**

Phase 1.1 proved the systems existed. This slice answers the complaints a
person driving them actually made: controls that never look pressed, settings
you can only nudge, a world with no visible lattice, placement that refuses a
click instead of helping it, a building card that says almost nothing,
hotkeys that move when the card does, a selection box whose colour is not the
colour it was asked for, and gathering workers that shove each other off a
node.

Every entry in the [system → test map](#system--test-map) is backed by named
automated tests that run on the merge gate, and one tracked script
(`assets/scenarios/rts_feedback_polish_v1.script`) drives the new chrome path
end to end through the shipped binary: select the HQ by its own footprint
corner, queue a Worker with the positional `q`, open the menu, open Settings,
drag a slider, toggle the world grid, scroll the body with the wheel, Back,
Close Menu, quit.

This slice did **not** ask how fast any of it runs, and this document claims
nothing about that. Performance stays retired to a later optimization phase,
exactly as in phases 0, 1 and 1.1.

What gates a merge is defined in one place only:
[testing strategy](05-testing.md). The decision behind the code is
[ADR 021](ADR/021_ADR_rts_feedback_polish_and_gather_collision.md), which
narrows [ADR 017](ADR/017_ADR_rts_hard_collision_navigation_and_formations.md)
and supplements
[ADR 018](ADR/018_ADR_settings_window_canvas_and_camera.md),
[ADR 019](ADR/019_ADR_hud_minimap_and_input_routing.md) and
[ADR 020](ADR/020_ADR_audio_events_buses_and_generated_assets.md); each of
those four carries a forward amendment rather than a rewritten decision. The
shape of the slice is on the
[architecture page](rts-feedback-polish-architecture.html). Phase 1.1's own
record stands and is not rewritten as if it had included any of this:
[phase 1.1 functional close](rts-interaction-ui-audio-hardening-functional-close.md).

## What this slice proves

| Claim | Status |
| --- | --- |
| Every discrete control has a stable identity and a framed visual state, and an activation needs the same control under the pointer on down and on up | proven — `crates/mmd-engine/tests/rts_hud.rs`, `src/rts_feedback.rs` |
| The top-right control is framed text at the coordinate the canonical script already clicks, and the pause menu closes itself without disturbing a manual pause | proven — `crates/mmd-engine/tests/rts_hud.rs`, `src/rts_ui.rs` |
| A phase-1.1 settings file loads with every value intact and gains the new grid/mute keys by default | proven — `src/rts_settings.rs` |
| Six numeric settings can be dragged live or typed, and every accepted edit commits through the one runtime → gains → save → publish transaction, rolling back whole on failure | proven — `src/rts_ui.rs` |
| A muted bus contributes gain zero while keeping its stored level, and unmuting restores exactly that level | proven — `src/rts_ui.rs`, `src/rts_audio.rs` |
| The settings body scrolls by wheel and by thumb over one shared offset; Back and the warning never move | proven — `crates/mmd-engine/tests/rts_acceptance.rs`, `tests/rts_cli_contract.rs` |
| All nine command keys map by card position, keyboard and pointer share one executor, and a disabled slot is silent | proven — `src/rts_input.rs`, `src/rts_ui.rs` |
| A player building is picked by its rendered sprite rect ∪ its ground footprint, and its card is exactly six lines | proven — `crates/mmd-engine/tests/rts_selection.rs`, `crates/mmd-engine/tests/rts_hud.rs` |
| A blocked cursor cell snaps to the nearest legal footprint, ranked from the corner the ghost is actually drawn at, and the click commits the footprint the player saw | proven — `crates/mmd-engine/tests/rts_build.rs`, `crates/mmd-engine/tests/rts_pack.rs` |
| The grid and the drag box are texture-free line instances: exact colours, no atlas sample, unchanged 48-byte layout | proven — `crates/mmd-engine/tests/render_correctness.rs`, `crates/mmd-engine/tests/rts_pack.rs` |
| The grid is the full map lattice, follows the camera, defaults on, persists, and never touches the world hash | proven — `crates/mmd-engine/tests/rts_pack.rs`, `src/rts_run.rs` |
| Two workers may overlap only while both are gathering, or while that exact pair is inside its bounded exit; every other merged pair is repaired, or counted and reported | proven — `crates/mmd-engine/tests/rts_collision.rs` |
| The gather-pair table enters the state hash in a framed block and reproduces in a separate process | proven — `crates/mmd-engine/tests/rts_collision.rs` |
| The joined frame — settings edits, scroll, building card, placement search, grid, gather exit — allocates nothing after warmup | proven — `crates/mmd-engine/tests/frame_allocations.rs` |
| The canonical 1,600-frame run still reports zero collision-policy violations, and the new focused script fires every entry it names | proven — `tests/rts_acceptance.rs`, `crates/mmd-engine/tests/rts_acceptance.rs` |
| Node clicks near a player building | **regressed.** Three `crates/mmd-engine/tests/rts_economy.rs` tests fail; see [open regression](#open-regression--building-pick-steals-node-clicks) |
| Performance | **unmeasured.** Retired to a later optimization phase; nothing here is a speed claim |
| Anything a person can see, hear or feel on real hardware | **unproven by the gate.** Offscreen runs open no window, grab no pointer and open no audio device; see [proof boundaries](#proof-boundaries) |

## What this slice does not prove

- **No combat, no enemy AI, no fog of war, no zoom, no save/load.** Unchanged
  from phase 1.1.
- **No hard collision for the horde.** ADR 009 stands: `crates/mmd-engine/src/sim`
  is soft separation steering over overlap-capable agents, and nothing in this
  slice touched it — `git diff -- crates/mmd-engine/src/sim` is empty against
  the branch point.
- **No unconditional RTS no-overlap promise any more.** The invariant this
  slice ships is narrower, and is stated in full below.
- **No audible claim.** Nothing on the gate proves a sound was heard; the mute
  tests prove gains, flags and stream lifecycle, not sound.
- **No host-window claim.** Nothing on the gate proves a window appeared, a
  wheel notch arrived from a real device, or a letterboxed click landed where
  the player aimed. Those are checklist items.
- **No balance pass, no real art.** Every cost, time, sprite and cue is still a
  placeholder.

## The collision invariant, as it now stands

ADR 021 narrows ADR 017. A completed tick leaves two RTS unit bodies merged
only in the two gather cases below; written in full, the rule the code enforces
is:

> Every penetrating RTS-unit pair after a tick is either both active gather
> workers, or the exact remembered pair inside its bounded 12-attempt
> gather-exit transition. Every other pair is non-penetrating, or is counted by
> the `body_overlaps=` exit token and reported through
> `TickError::UnrepairableOverlap`.

Three consequences worth stating plainly, because each is a place a reader
could otherwise believe the old rule:

1. **Legal overlap is not free overlap.** Static terrain, map edges, resource
   nodes and finished buildings stay hard for gathering workers, and a pair
   with a non-worker or non-gathering member stays hard.
2. **The exit is bounded and self-clearing.** An attempt that reaches
   non-penetration clears the pair the same tick, before normal movement, so a
   separated pair cannot re-merge on the tick it separated.
3. **A failed exit is reported, never granted.** After twelve attempts a
   relocation is tried inside the same navigation component; if that fails too,
   the pair goes back to hard, `RtsWorld::body_overlap_count` counts it, the
   exit line's `body_overlaps=` token goes non-zero, and next tick's generic
   repair retries it.

The horde contract is the opposite one and is not affected: ADR 009 soft
separation steering never resolves an overlap, and no doc may claim horde
agents cannot overlap.

## Closed regression — building pick stole node clicks

The building pickshape became `rendered sprite rect ∪ ground footprint`. A
building sprite is taller than its footprint, so a building could win a click
on a resource node whose centre it covers on screen. Three tests in
`crates/mmd-engine/tests/rts_economy.rs` failed on the merge gate because of
it:

| Test | Observed while red |
| --- | --- |
| `context_order_with_no_selection_is_a_no_op` | a click on the crystal node's own projected centre returned the HQ |
| `mixed_resource_order_partitions_by_capability` | the same click, so the mixed order never partitioned |
| `nonworkers_move_around_resource` | the same click, so the non-worker never routed around the node |

Bisected: green at `d7ddb60`, red at `af16e7c`, which is the commit that
introduced the union pickshape. `af16e7c` is **not** an ancestor of `main`, so
this was a regression introduced by this branch, not a pre-existing red gate,
and it was mislabelled as pre-existing while it stood. It was a real behaviour
regression against a phase-1.1 claim ("a click on any part of a rendered
resource sprite hits that node"), not a stale expectation, so it was **not**
repaired by editing the tests.

**Fixed** by the tie rule the paragraph above asked for: `pick_at` now ranks
candidates in two tiers. Tier 1 is the exact shapes — a unit's sprite rect ∪
body circle, a node's sprite rect, a building's ground footprint. Tier 2 is a
building's rendered sprite quad, consulted only when tier 1 matched nothing.
Depth ordering is unchanged **within** a tier, so a narrower, exact shape
always wins a contended click while the whole rendered building stays
clickable where nothing stands behind it. Pinned by
`an_exact_shape_beats_a_building_sprite_quad` in
`crates/mmd-engine/tests/rts_selection.rs`; the three
`crates/mmd-engine/tests/rts_economy.rs` tests are green again with their
original expectations untouched.

## System → test map

The eighteen systems are pinned by the `POLISH_SYSTEMS` list in
`tests/validation_contract.rs`, which resolves one representative name per
system against a source scan of the tree *and* against this page. The whole
table below is resolved the same way by
`feedback_polish_close_names_only_real_tests`: every name in the third column
must exist as a live `#[test]` fn in the file in the second.

| System | Test binary | Named tests |
| --- | --- | --- |
| control feedback — identity, framed states, hit coordinates | `crates/mmd-engine/tests/rts_hud.rs` | `control_visual_states_have_distinct_tints`, `menu_is_framed_text_control_at_existing_hit_coordinate`, `all_command_cells_have_frames`, `checkbox_label_row_is_one_control`, `close_menu_is_below_settings`, `settings_button_still_contains_the_canonical_click`, `the_menu_control_appears_in_the_top_bar`, `hit_menu_is_menu`, `settings_menu_contains_settings_and_close_actions` |
| pointer activation and modal ownership | `src/rts_feedback.rs` | `activation_requires_matching_down_and_up_control`, `modal_press_never_leaks_to_world`, `master_mute_zeroes_all_buses_without_changing_levels`, `bus_mute_zeroes_only_its_bus` |
| interaction FSM — Menu, Close, manual pause | `src/rts_ui.rs` | `menu_only_opens_from_gameplay`, `close_menu_preserves_manual_pause`, `close_menu_clears_menu_and_focus_pause`, `close_menu_activation_emits_menu_cue`, `escape_backs_out_one_level`, `manual_pause_survives_menu_close`, `space_never_changes_the_page` |
| settings geometry — one owner, disjoint rects | `crates/mmd-engine/tests/rts_hud.rs` | `settings_geometry_is_disjoint_and_inside_the_viewport`, `spec_rects_equal_the_pinned_layout_constants`, `numeric_specs_cover_exactly_six_settings`, `keyboard_pan_track_still_reads_seventy_eight_at_1170`, `slider_thumb_reaches_both_track_ends_exactly`, `clamp_snap_handles_bounds_and_half_steps`, `numeric_fields_are_framed_and_hit_testable` |
| settings schema 1 — additive fields, preserved values | `src/rts_settings.rs` | `legacy_schema_one_preserves_values_and_defaults_new_fields`, `schema_one_round_trips_grid_and_mutes`, `malformed_new_bool_warns_and_defaults`, `unsupported_schema_warns`, `save_load_round_trip_is_canonical`, `malformed_file_warns_and_uses_defaults`, `offscreen_run_does_not_touch_settings` |
| live sliders — snapped steps through the transaction | `src/rts_ui.rs` | `slider_drag_retains_stable_control`, `slider_drag_commits_only_distinct_steps`, `slider_drag_updates_camera_and_audio_before_next_frame`, `failed_slider_commit_rolls_back_runtime_and_value`, `slider_drag_never_selects_world`, `scripted_slider_drag_uses_same_controller` |
| numeric fields — three digits, four commit paths | `src/rts_ui.rs` | `numeric_id_mapping_is_exhaustive`, `numeric_edit_accepts_three_ascii_digits`, `unsupported_text_is_ignored`, `backspace_edits_fixed_buffer`, `enter_clamps_snaps_and_commits`, `empty_enter_restores_without_save`, `pointer_blur_commits_before_activation`, `escape_restores_and_consumes_navigation`, `os_focus_loss_finalizes_before_pause_and_clear`, `offscreen_never_starts_text_input` |
| mute labels — rects, visual state, hit priority | `crates/mmd-engine/tests/rts_hud.rs` | `mute_rects_are_40px_above_their_tracks`, `audio_label_full_rect_returns_toggle_mute_hit`, `mute_label_does_not_overlap_slider_track`, `muted_label_uses_selected_visual`, `unmuted_label_uses_idle_visual`, `mute_label_text_changes_when_muted`, `mute_label_hit_priority_over_nothing_below_track`, `pack_modal_with_muted_flag_does_not_panic` |
| mute transaction — gains, rollback, persistence | `src/rts_ui.rs` | `muting_master_keeps_stored_levels`, `muting_zeroes_gains_without_touching_stored_levels`, `unmuting_restores_gain_to_stored_level`, `mute_commit_pushes_gains_and_saves_once`, `mute_gain_failure_rolls_back_flag_and_gains`, `mute_save_failure_rolls_back_flag_and_gains` |
| mute runtime — streams keep running | `src/rts_audio.rs` | `mute_gain_update_does_not_clear_streams`, `live_gain_change_updates_all_bus_streams` |
| settings scroll and scripted isolation — through the binary | `tests/rts_cli_contract.rs` | `exit_line_reports_settings_scroll_px`, `scripted_slider_drag_commits_keyboard_pan_memory_only`, `focused_script_is_independent_of_persisted_gameplay_settings` |
| scroll input — a wheel notch is a mapped pointer event | `src/rts_script.rs` | `wheel_script_parses_copyable_command`, `script_parses_every_kind`, `the_tracked_script_parses` |
| positional command keys — row-major QWE/ASD/ZXC | `src/rts_input.rs` | `all_nine_command_keys_map_row_major`, `x_executes_slot_seven`, `r_is_unbound`, `each_key_binds_one_command`, `no_key_is_both_a_command_and_a_pan`, `unbound_keys_are_rejected`, `the_window_banner_lists_every_binding` |
| command execution — one executor for key and pointer | `src/rts_ui.rs` | `hq_q_queues_worker`, `barracks_q_queues_soldier`, `c_arms_rally`, `disabled_slot_key_is_noop_without_sfx`, `keyboard_and_pointer_share_execute_slot`, `out_of_range_slot_is_false_not_panic` |
| building pick — sprite rect ∪ ground footprint | `crates/mmd-engine/tests/rts_selection.rs` | `all_building_sprite_corners_are_pickable`, `building_footprint_only_region_remains_pickable`, `outside_building_union_misses`, `building_union_preserves_owner_and_depth_rules`, `building_pick_contains_sprite_and_footprint` |
| building detail card — exactly six lines | `crates/mmd-engine/tests/rts_hud.rs` | `building_details_use_exact_six_line_contract`, `queue_entries_render_oldest_first`, `ready_blocked_head_shows_one_hundred_percent`, `zero_supply_and_empty_queue_are_explicit`, `command_cells_show_positional_letters` |
| assisted placement — one candidate, preview equals commit | `crates/mmd-engine/tests/rts_build.rs` | `valid_raw_placement_is_unchanged`, `blocked_raw_snaps_to_nearest_valid_footprint`, `ranking_reference_is_the_saturated_raw_min_corner`, `candidate_tie_uses_lowest_flat_index`, `candidate_never_exceeds_one_footprint_width`, `map_edge_search_is_safe`, `no_nearby_candidate_returns_raw_invalid`, `red_preview_click_is_noop` |
| placement through the binary — a red click does nothing | `tests/rts_cli_contract.rs` | `a_red_click_with_no_snap_is_a_noop`, `right_click_is_only_placement_cancel`, `hud_disabled_command_slot_is_consumed_without_action` |
| line primitive — texture-free diagonal instances | `crates/mmd-engine/tests/render_correctness.rs` | `line_instance_keeps_pinned_layout`, `zero_length_line_is_degenerate_not_nan`, `ring_and_line_sentinels_are_disjoint`, `rising_and_falling_lines_raster`, `line_thickness_is_exact`, `line_branch_never_samples_atlas` |
| world grid — full lattice, camera-projected, unhashed | `crates/mmd-engine/tests/rts_pack.rs` | `grid_defaults_on_and_toggle_produces_frame_pack_options`, `grid_packs_exact_map_lattice`, `disabled_grid_packs_no_lines`, `grid_precedes_selection_rings`, `grid_uses_only_diagonal_line_instances`, `grid_toggle_does_not_change_world_hash`, `grid_capacity_covers_max_map_and_selection`, `grid_follows_camera_projection` |
| grid persistence — default on, scripted runs normalised | `src/rts_run.rs` | `exit_line_reports_live_show_grid`, `scripted_replay_normalises_gameplay_settings`, `a_scripted_run_takes_the_default_camera_speeds`, `an_unscripted_run_keeps_the_persisted_camera_speeds` |
| area selection — exact green fill and border | `crates/mmd-engine/tests/rts_pack.rs` | `drag_box_has_pure_green_ten_percent_fill`, `drag_box_has_opaque_two_pixel_pure_green_border`, `drag_box_never_samples_the_atlas`, `the_drag_box_normalises_its_corners`, `no_drag_means_no_box`, `green_preview_commits_exact_displayed_min` |
| gather-pair collision — who may overlap, and who never may | `crates/mmd-engine/tests/rts_collision.rs` | `two_gathering_workers_may_overlap`, `all_gather_phase_pairs_qualify`, `gather_exemption_is_owner_blind`, `one_non_gather_worker_keeps_pair_hard`, `worker_soldier_pair_stays_hard`, `gather_workers_still_hit_static_geometry`, `nested_push_checks_use_pair_policy`, `body_overlap_count_reports_policy_violations`, `forced_non_gather_overlap_repairs_immediately` |
| bounded gather exit — twelve attempts, then relocation | `crates/mmd-engine/tests/rts_collision.rs` | `active_pair_overlaps_during_movement_then_exits_into_transition`, `first_attempt_stores_one_not_two_fifty_six`, `exited_pair_separates_gradually`, `separated_pair_clears_to_hard_in_the_same_tick`, `separated_pair_cannot_remerge_on_the_tick_it_cleared`, `open_concentric_pair_reaches_contact_by_attempt_twelve`, `concentric_normal_moves_lower_slot_negative`, `blocked_pair_relocates_after_attempt_twelve_same_tick`, `transition_never_exempts_third_body`, `normal_movement_respects_transition_pair`, `fallback_priority_rotates`, `failed_fallback_clears_to_hard_and_reports_a_violation`, `fallback_never_crosses_connected_component`, `fallback_tries_the_partner_when_the_first_body_has_nowhere_to_go`, `fallback_preserves_active_provenance_of_a_third_pair`, `multi_pair_pass_is_snapshot_eligible_and_order_stable`, `multi_pair_move_never_decreases_other_transition_distance`, `gather_exit_state_reproduces_cross_process` |
| feedback-polish acceptance — through the shipped binary | `tests/rts_acceptance.rs` | `feedback_polish_script_exercises_menu_close_grid_scroll_slider_and_q`, `canonical_acceptance_reports_zero_collision_policy_violations`, `focused_run_is_cross_process_deterministic`, `the_focused_run_fires_every_entry`, `the_tracked_script_runs_clean`, `the_acceptance_run_is_deterministic`, `acceptance_audio_counts_are_exact` |
| acceptance — through the engine | `crates/mmd-engine/tests/rts_acceptance.rs` | `canonical_gather_overlap_is_non_vacuous`, `focused_script_hq_click_picks_the_building_not_a_worker`, `focused_script_back_button_is_fixed_under_scroll`, `the_focused_script_coordinates_hit_what_they_name`, `grid_toggle_changes_frame_not_world_hash`, `acceptance_never_has_body_penetration`, `the_acceptance_run_is_reproducible` |
| allocation invariant — the joined feedback frame | `crates/mmd-engine/tests/frame_allocations.rs` | `settings_edit_field_pack_allocates_nothing`, `scroll_pack_allocates_nothing`, `gather_exit_allocates_nothing`, `full_building_detail_pack_allocates_nothing`, `placement_search_allocates_nothing`, `grid_pack_allocates_nothing`, `grid_capacity_is_at_least_max_map`, `combined_feedback_frame_allocates_nothing` |
| context orders — **currently red**, see the open regression above | `crates/mmd-engine/tests/rts_economy.rs` | `context_order_with_no_selection_is_a_no_op`, `mixed_resource_order_partitions_by_capability`, `nonworkers_move_around_resource`, `click_path_gather_banks_crystal`, `gatherers_get_distinct_legal_approaches`, `delivery_uses_the_footprint_not_the_centre` |
| docs and gate contract | `tests/validation_contract.rs` | `feedback_polish_close_names_only_real_tests`, `feedback_polish_systems_have_behavioral_tests`, `feedback_polish_adr_is_accepted_and_amended_forward`, `feedback_polish_architecture_page_is_landed_with_evidence`, `rts_overlap_invariant_names_its_gather_exception`, `glossary_defines_the_feedback_polish_vocabulary`, `manual_checklist_covers_every_human_only_flow`, `adr_index_lists_every_adr_file`, `every_doc_link_resolves`, `no_perf_claim_in_docs` |

## Automated, manual, and neither

The three columns a close doc most often blurs, kept apart:

| Behaviour | Proven automatically | Proven by hand | Claimed by nobody |
| --- | --- | --- | --- |
| Control states | tint/frame per state, activation pairing | that the pressed state is *visible* at a glance | that it looks good |
| Menu / Close | page transitions, pause reasons, cue emission | that Escape and Close feel the same | — |
| Sliders and typed fields | snapped commits, rollback, focus-loss ordering | that a drag tracks the cursor without stutter | — |
| Mutes | gain zero, level preserved, stream kept | that muting silences the speakers | that any sound was heard on the gate |
| Scroll | offset clamp, clipped rects, fixed Back, scripted wheel token | wheel direction on a real device, and inside a letterboxed window | that a host wheel event was ever delivered |
| World grid | line count, layer order, camera projection, hash independence | that the lattice is legible against the terrain | — |
| Placement | candidate rank, preview/commit parity, red no-op | that snapping feels helpful rather than surprising | — |
| Building card | six exact lines, queue order | that the six lines are readable at 1080p | — |
| Command keys | row-major mapping, silent disabled slot | muscle memory across the three cards | — |
| Gather overlap | every phase/owner pair, bounded exit, fallback, hash | that two workers on one node look right | — |

The hand column is `ai_artefacts/manual_test_checklist.md`, section T16, and
`manual_checklist_covers_every_human_only_flow` fails if a flow named here has
no steps there.

## Proof boundaries

The gate runs offscreen. It opens no window, grabs no pointer, and opens no
audio device. Therefore:

- No test here proves a sound was audible, only that gains, flags, lanes and
  stream lifecycle behaved.
- No test here proves a window appeared, a mode change was honoured by the
  compositor, or a letterbox bar rejected a real click — those are proven as
  pure state sequences, and checked by hand.
- No test here proves a rendered pixel on the host display. Render correctness
  is a separate development-host claim, described in
  [testing strategy](05-testing.md).
- No test here measures speed, and no number in this document is a
  performance claim.

## Known gaps carried forward

- The node-click regression above.
- ADR 017's parked single-file corridor defect: a body that cannot pass a
  five-cell corridor is still refused rather than routed through it. Untouched
  by this slice, and still out of scope.
- The host GPU on the development machine intermittently loses the device
  under sustained GPU test runs; affected render tests pass when run alone.
  That is a host condition, not a claim about the code.
