# Combat Prototype — Functional Close

**Status: closed on functional evidence, 2026-08-21.**

Phase 2 makes the game fight back. Phase 1 built a base with nobody to defend
it against; this slice adds the enemy faction, the weapons on both sides, the
deaths, and the first defensive building — on the same gate scene, driven end
to end by a new tracked script through the shipped binary. Folded into the same
phase, because the user asked for them while it was open, is a second surface:
the build grid, the builder-exit fix, order status text, target rings, move
markers, follow orders, entity rally points, and an untimed sandbox scene.

This slice did **not** ask how fast any of it runs, and this document claims
nothing about that. Performance stays retired to a later optimization phase,
exactly as in phases 0, 1, 1.1 and the feedback-polish slice.

What gates a merge is defined in one place only:
[testing strategy](05-testing.md). The decisions behind the code are
[ADR 022](ADR/022_ADR_combat_model_and_enemy_faction.md),
[ADR 023](ADR/023_ADR_combat_gate_scale_and_rebaseline.md),
[ADR 024](ADR/024_ADR_build_grid_and_placement_snap.md) and
[ADR 025](ADR/025_ADR_order_feedback_follow_and_entity_rally.md); the shape of
the slice is on the
[combat architecture page](combat-prototype-architecture.html) and the
[feedback round 2 architecture page](rts-feedback-round2-architecture.html).
The earlier phase records stand and are not rewritten as if they had included
any of this: [phase 1](rts-engine-prototype-functional-close.md),
[phase 1.1](rts-interaction-ui-audio-hardening-functional-close.md),
[feedback polish](rts-feedback-polish-functional-close.md) — see
[the re-baseline, named honestly](#the-re-baseline-named-honestly) for what
changed under them.

## What this slice proves

| Claim | Status |
| --- | --- |
| HP and armor columns exist for every entity kind; damage applies `max(1, damage - armor)` and never zeroes a hit | proven — `crates/mmd-engine/tests/rts_combat.rs` |
| Deaths route completely: units despawn, buildings — the HQ included — un-stamp their footprint and invalidate every pooled field, production queues cancel with no refund, HQ death idles the gatherers | proven — `crates/mmd-engine/tests/rts_combat.rs` |
| The enemy faction exists as RTS entities: `OWNER_ENEMY = 1`, `UnitKind::Ghoul`, never producible, never supply-counted, excluded from the drag box | proven — `crates/mmd-engine/tests/rts_enemy.rs`, `crates/mmd-engine/tests/rts_selection.rs` |
| Scenario waves spawn deterministically at their exact tick, defer bounded when the store is full, and the validator caps total enemies at `MAX_ENEMIES = 1_200` | proven — `crates/mmd-engine/tests/rts_enemy.rs`, `crates/mmd-engine/tests/scenario_contract.rs` |
| Enemies march on **one** objective cell over **one** shared pooled flow field (hundreds of enemies, one field — `NAV_FIELD_SLOTS = 8` stands), melee the first player thing in range, retarget the nearest player building when the HQ dies, and go Idle when nothing is left | proven — `crates/mmd-engine/tests/rts_combat.rs` |
| `Idle` and `AttackMove` auto-acquire the nearest hostile in range and fire in place; `Attack` fires only at its own target and falls back to auto-acquire when that target dies; `Move`, `Gather`, `Build` and `Follow` never fire; targeting is nearest-in-range with a lowest-slot tie-break | proven — `crates/mmd-engine/tests/rts_combat.rs` |
| Attack, attack-move and Stop are player commands sharing one executor with the card; workers in an attack-move selection just move; an enemy selection rejects every command | proven — `crates/mmd-engine/tests/rts_combat_commands.rs`, `src/rts_run.rs`, `crates/mmd-engine/tests/rts_hud.rs` |
| The Turret is worker-built for 75 crystal under the unchanged four placement rules, grants no supply, is no drop-off, never fires as an unfinished site, and auto-fires the nearest enemy measured from its footprint rectangle | proven — `crates/mmd-engine/tests/rts_turret.rs` |
| Damaged-or-selected entities carry texture-free HP bars, deaths flash a 12-frame procedural ring, enemies are red dots on the minimap (`MINIMAP_ENEMY_TINT`) | proven — `crates/mmd-engine/tests/rts_pack.rs`, `crates/mmd-engine/tests/rts_hud.rs` |
| Buildings sit on a visible `BUILD_SQUARE_CELLS = 8` lattice, footprints are whole numbers of squares, the ghost snaps its min corner, and scenario-declared buildings are square-validated at load — while units keep moving in true float cells and ignore the square entirely | proven — `crates/mmd-engine/tests/rts_build.rs`, `crates/mmd-engine/tests/rts_pack.rs`, `crates/mmd-engine/tests/scenario_contract.rs` |
| A finished building evacuates every body its footprint covers to a legal, distinct centre and clears the builder's order; a site whose evacuation can never succeed now gives up after `STALLED_SITE_TICKS = 180` and refunds in full instead of holding one tick short forever | proven — `crates/mmd-engine/tests/rts_build.rs` |
| A selected unit's card names what it is doing from live order state, a node card shows `YIELD`, and the target ring is re-derived each frame so it survives reselection | proven — `crates/mmd-engine/tests/rts_hud.rs`, `crates/mmd-engine/tests/rts_pack.rs` |
| A ground order plants one bounded, hashed, self-expiring `Prop::MoveMarker`, and a selected mover draws a dashed line toward its goal | proven — `crates/mmd-engine/tests/rts_world.rs`, `crates/mmd-engine/tests/rts_pack.rs` |
| `Order::Follow` closes to interaction reach, holds, re-paths past `FOLLOW_REPATH_CELLS`, dies with its target and never targets an enemy or itself; a rally point may name a cell **or** an entity, and right-click resolves by ownership (enemy ⇒ attack, friendly ⇒ follow, node ⇒ gather, ground ⇒ move) | proven — `crates/mmd-engine/tests/rts_world.rs`, `crates/mmd-engine/tests/rts_production.rs`, `src/rts_run.rs` |
| The combat script runs clean through the shipped binary, fires every entry it names, pins five combat tokens exactly, and reproduces its whole exit line in a second process | proven — `tests/rts_acceptance.rs` |
| Combat provably began by marching: `first_combat_tick` is strictly after the first spawn tick, with no enemy starting in reach of anything | proven — `tests/rts_acceptance.rs` |
| The march allocates nothing per frame | proven — `crates/mmd-engine/tests/frame_allocations.rs` |
| The untimed sandbox scene loads and runs from the CLI, and is **absent** from every gate block | proven — `tests/rts_cli_contract.rs`, `tests/validation_contract.rs` |
| Which class of firer scored a kill | **unproven.** No exit token attributes a kill to a unit, a turret or an auto-acquire; the gate counts kills, not killers |
| That the scripted defence is any good | **disproven, deliberately recorded.** The 4,500-frame run ends `kills=2 losses=5`; see [what the script proves](#what-the-script-proves) |
| Performance | **unmeasured.** Retired to a later optimization phase; nothing here is a speed claim |
| Anything a person can see or hear on real hardware | **unproven by the gate.** Offscreen runs open no window, grab no pointer and open no audio device; see [proof boundaries](#proof-boundaries) |

## What this slice does not prove

- **No window or sound claim.** The gate runs offscreen; nothing proves a bar,
  flash, dot or turret shot was *visible*, and the turret is **silent by
  design this phase** — the human checks live in
  `artifacts/manual_test_checklist.md`.
- **No balance.** Every stat is a placeholder: Ghoul 30 HP / 0 armor, damage 5,
  cooldown 30 ticks, range 8, speed 18 cells/s; Soldier 40 / 0, damage 6,
  cooldown 15, range 24, speed 24 cells/s; Turret 150 / 1, damage 10,
  cooldown 20, range 36; Worker 25 / 0, unarmed, speed 30 cells/s; HQ 400 / 2;
  Depot 150 / 1; Barracks 200 / 1. Balance is a later phase, and the gate run
  says out loud that the current numbers do not hold a line.
- **Hundreds, not the horde.** The gate scene spawns 400 Ghouls across four
  waves. The phase-3 claim (tens of thousands) is untouched and nothing here
  advances it.
- **One enemy kind, melee only.** No ranged enemy, no worker attack, no
  projectile entities, no damage types.
- **No win, no lose.** The run is a sandbox; the outcome is carried by exit
  tokens, not by any end state.
- **No performance claim.** Unmeasured, by policy, as in every prior phase.
- **`buildings=` is not a completion count.** The exit line's `buildings=`
  token counts building *sites* as well as finished buildings, so no exit line
  in this repo proves that a building finished. Only a test that reads
  `progress_target == 0`, or a human looking at the screen, proves that.
- **Unchanged absences.** No fog of war, no zoom, no save/load — as phase 1.1
  left them.

## The collision contract, named

Two collision contracts exist, and every sentence here names the one it means.
Enemies live under the **RTS hard-body contract**: a Ghoul is an ordinary hard
pair with everything it touches — ADR 021's gather-worker exception is **not**
widened to enemies, so `body_overlaps=0` on the combat run is the same *policy*
claim it was in the feedback-polish close, not a raw-geometry one. A pair may
end a tick merged only when both bodies are active gather workers, or when that
exact pair is inside its bounded gather-exit transition; every other merged
pair is repaired, or counted by `body_overlaps` and reported through
`TickError::UnrepairableOverlap`.

The **horde contract** is the opposite one and is untouched: ADR 009's `sim/`
soft separation steering bends a descent vector and never resolves an overlap,
and no doc may claim horde agents cannot overlap. Nothing in phase 2 touched
`sim/`.

## What the script proves

The tracked script `assets/scenarios/rts_combat_v1.script` runs on the merge
gate as
`cargo run -- rts --frames 4500 --inject-input-file assets/scenarios/rts_combat_v1.script`
and its exit line ends, in this pinned order:

```text
kills=2 losses=5 enemies_spawned=400 first_combat_tick=3691 hq_alive=1
```

| Token | Value | Meaning |
| --- | --- | --- |
| `kills=` | 2 | enemy deaths by combat fire over the whole run |
| `losses=` | 5 | player unit and building deaths by combat fire |
| `enemies_spawned=` | 400 | cumulative Ghouls ever spawned (all four waves fired) |
| `first_combat_tick=` | 3691 | first combat-system damage; strictly after the first spawn at tick 3000, so combat provably began by marching |
| `hq_alive=` | 1 | the HQ survived this scripted run |

The values are exact, pinned by `combat_tokens_exact` in
`tests/rts_acceptance.rs`, and the whole exit line — including
`body_overlaps=0`, `frames=4459` and the state hash — is compared between two
independent processes. A drifted token is a red gate, not a curiosity.

**Read `kills=2` as the honest number it is.** Two kills against four hundred
spawned enemies is a losing defence, and the run is pinned that way on purpose
rather than tuned until it looked heroic. The mechanism is known: an auto-acquiring firer
re-picks its target every tick from `RtsWorld::nearest_hostile_in_range` while
damage only lands when its cooldown reaches 0, so consecutive shots into a
moving clump land on different Ghouls and finish none of them. Five player
losses and two enemy deaths is what that produces. Fixing it is a targeting/balance
change, which is a later phase; this document records the behaviour rather than
hiding it behind a re-baseline.

**The one objective cell.** The enemy faction marches at a single cell — the
approach cell of the player building nearest the faction's origin, which on the
gate scene is the HQ's north approach cell `(160, 156)`. Both spawn corners are
in the far south, so a wave from the south-west corner walks up the **west**
flank on its way round to that cell. There is no "southern line" to hold, and
any plan or checklist text that describes one is describing a scene that does
not exist.

## The re-baseline, named honestly

Putting enemies into `assets/scenarios/rts_prototype_v1.ron` changed the world
both phase-1 scripts run in, and the build-grid recut moved the scene's worker
spawn row onto a square-aligned position. Their pinned counters and the scene's
sha256 sidecar were re-baselined — once per change, in the same commit as the
change, after verifying each moved value by hand. These were deliberate
evidence updates, not a way to make a red test green. The phase-1 and phase-1.1
close documents describe the pre-enemy, pre-recut runs and are not rewritten;
their claims hold for the world they measured. `body_overlaps=0` still holds on
both phase-1 scripts with marching enemies in the same scene — enemies are
ordinary hard pairs under the ADR 021 policy.

## System → test map

No validation-contract test resolves this table yet (a named gap below); every
name in it was checked by hand against a live `#[test]` fn at close time.

| System | Test binary | Named tests |
| --- | --- | --- |
| combat data model — HP, armor, damage floor, death routing | `crates/mmd-engine/tests/rts_combat.rs` | `stats_are_the_published_constants`, `every_entity_spawns_at_full_hp`, `damage_reduces_hp_by_damage_minus_armor`, `damage_floors_at_one`, `damage_to_a_node_is_a_no_op`, `damage_to_a_stale_id_is_refused`, `a_unit_dies_at_zero_hp_and_its_slot_frees`, `building_death_unstamps_its_footprint`, `building_death_cancels_its_queue_without_refund`, `building_death_revokes_its_supply_grant`, `hq_death_idles_its_returning_gatherers`, `site_death_idles_its_builders_without_refund`, `hp_enters_the_state_hash`, `death_events_drain_once`, `death_events_never_enter_the_state_hash` |
| enemy faction and scenario waves | `crates/mmd-engine/tests/rts_enemy.rs` | `ghoul_kind_tags_stably`, `pre_placed_ghouls_spawn_at_start`, `wave_spawns_at_exact_tick`, `enemies_never_consume_supply`, `drag_box_excludes_enemies`, `spawn_determinism`, `wave_defers_when_store_full` |
| enemy scenario schema and caps | `crates/mmd-engine/tests/scenario_contract.rs` | `enemy_baseline_block_is_valid`, `enemy_block_optional_old_scenes_parse`, `enemy_spec_validates_cells_and_totals` |
| weapons, targeting and enemy march AI | `crates/mmd-engine/tests/rts_combat.rs` | `soldier_auto_acquires_idle`, `plain_move_never_fires`, `nearest_target_lowest_slot_tie`, `cooldown_gates_fire_rate`, `a_gathering_worker_never_fires`, `ghouls_march_on_hq`, `ghoul_attacks_first_thing_in_range`, `ghouls_besiege_and_kill_hq`, `objective_retargets_on_hq_death`, `no_player_buildings_enemies_idle`, `counters_and_first_combat_tick`, `combat_determinism`, `cooldown_enters_state_hash`, `field_pool_not_churned`, `the_ghoul_is_the_slowest_unit` |
| player combat commands — engine side | `crates/mmd-engine/tests/rts_combat_commands.rs` | `attack_target_orders_armed_selection`, `attack_target_walks_then_kills`, `attack_move_formation_semantics`, `workers_in_selection_move_dont_fight`, `attack_target_walks_workers_instead`, `attack_target_rejects_unarmed_dead_and_missing`, `stop_idles_and_cancels`, `enemy_selection_rejects_commands`, `command_receipts_do_not_grow_the_buffer` |
| player combat commands — app side | `src/rts_run.rs` | `attack_mode_armed_by_execute_slot`, `escape_cancels_attack_mode_without_opening_menu`, `right_click_cancels_armed_attack_mode`, `enemy_selected_right_click_rejects`, `cmd_attack_target_on_ghoul_emits_voice`, `attack_move_ground_click_with_armed_mode` |
| enemy pick and card | `crates/mmd-engine/tests/rts_selection.rs`, `crates/mmd-engine/tests/rts_hud.rs` | `enemy_unit_is_pickable_and_click_selects_exactly_one`, `shift_click_never_mixes_enemy_and_player`, `card_shows_attack_stop_for_armed`, `enemy_selection_shows_no_commands`, `enemy_card_two_lines_kind_and_hp` |
| turret — the first armed building | `crates/mmd-engine/tests/rts_turret.rs` | `turret_stats_cost_and_footprint_are_published`, `building_weapon_arms_only_the_turret`, `turret_placeable_under_four_rules`, `turret_costs_75_and_builds`, `unfinished_turret_never_fires`, `turret_auto_fires_nearest`, `turret_target_ties_break_to_lowest_slot`, `turret_range_measured_from_footprint`, `ghouls_kill_turret`, `turret_grants_no_supply_not_dropoff`, `turret_card_button_positional` |
| combat feedback — bars, flashes, dots | `crates/mmd-engine/tests/rts_pack.rs`, `crates/mmd-engine/tests/rts_hud.rs` | `bar_hidden_at_full_hp`, `bar_shown_damaged_and_selected`, `bar_color_thresholds`, `building_bar_spans_footprint`, `flash_lasts_twelve_frames`, `flash_radius_units_body_buildings_half_footprint`, `minimap_draws_enemy_dots`, `the_card_shows_hp_for_every_hp_bearing_kind` |
| build grid and footprint recut | `crates/mmd-engine/tests/rts_build.rs`, `crates/mmd-engine/tests/rts_pack.rs`, `crates/mmd-engine/tests/scenario_contract.rs` | `every_footprint_is_a_whole_number_of_build_squares`, `the_ghost_snaps_its_min_corner_to_a_build_square`, `an_assisted_placement_stays_on_the_grid`, `blocked_raw_snaps_to_nearest_valid_footprint`, `the_grid_overlay_draws_build_squares_not_cells`, `a_pending_ghost_forces_the_grid_on`, `the_ghost_draws_one_tile_per_build_square`, `hq_cell_must_sit_on_a_build_square`, `the_tracked_scene_is_square_aligned`, `a_prebuilt_building_must_be_square_aligned` |
| builder exit and the bounded stall | `crates/mmd-engine/tests/rts_build.rs` | `a_builder_is_never_trapped_by_the_building_it_finished`, `completion_evacuates_every_overlapping_body`, `evacuated_bodies_land_on_legal_centres`, `completion_waits_when_evacuation_impossible`, `a_site_that_cannot_evacuate_resolves_within_a_bounded_time`, `cancel_refunds_the_full_cost` |
| order status line and target ring | `crates/mmd-engine/tests/rts_hud.rs`, `crates/mmd-engine/tests/rts_pack.rs` | `order_status_label_covers_every_order_and_phase`, `the_card_shows_the_status_under_the_hp_line`, `a_node_card_shows_yield_per_trip`, `order_status_label_covers_follow`, `the_card_names_an_entity_rally`, `a_gathering_worker_rings_its_node`, `the_target_ring_survives_reselection`, `two_workers_on_one_node_draw_one_target_ring`, `a_selected_target_is_not_ringed_twice`, `an_attacking_unit_rings_its_target` |
| move markers and dashed lines | `crates/mmd-engine/tests/rts_world.rs`, `crates/mmd-engine/tests/rts_pack.rs` | `a_ground_order_plants_one_marker`, `a_marker_expires_on_schedule`, `markers_are_bounded_and_drop_oldest_first`, `a_rallied_unit_plants_no_marker`, `markers_enter_the_state_hash`, `the_marker_packs_one_quad_per_live_marker`, `a_selected_mover_draws_a_dashed_line_to_its_goal`, `a_huge_selection_skips_dashes_entirely`, `a_selected_producer_dashes_to_its_rally_point` |
| follow orders and entity rally | `crates/mmd-engine/tests/rts_world.rs`, `crates/mmd-engine/tests/rts_production.rs`, `src/rts_run.rs` | `a_follower_closes_to_interaction_reach_and_holds`, `a_follower_repaths_when_its_target_walks_away`, `a_follow_order_dies_with_its_target`, `follow_never_targets_an_enemy_or_itself`, `follow_enters_the_state_hash_distinctly`, `rally_onto_a_cell_still_moves`, `rally_onto_a_node_makes_produced_workers_gather`, `rally_onto_a_unit_makes_produced_units_follow`, `rally_rejects_an_enemy_target`, `a_stale_entity_rally_is_dropped`, `cell_and_entity_rallies_hash_differently`, `right_click_on_a_friendly_unit_follows_it`, `right_click_on_an_enemy_still_attacks`, `right_click_on_a_selected_unit_is_still_a_ground_move`, `a_building_only_selection_keeps_its_right_click` |
| the combat gate — through the shipped binary | `tests/rts_acceptance.rs` | `the_combat_script_runs_clean`, `combat_tokens_close_the_exit_line_in_order`, `combat_begins_by_marching`, `the_combat_run_fires_every_entry`, `combat_tokens_exact`, `combat_run_is_cross_process_deterministic` |
| allocation invariant under combat | `crates/mmd-engine/tests/frame_allocations.rs` | `combat_march_allocates_nothing` |
| the sandbox scene, and its absence from the gate | `tests/rts_cli_contract.rs`, `tests/validation_contract.rs` | `the_sandbox_scene_runs_from_the_cli`, `the_sandbox_is_not_on_the_merge_gate` |

## Proof boundaries

- **Offscreen.** The gate run maps no window, grabs no pointer and opens no
  audio device. It therefore carries no visibility claim for HP bars, death
  flashes, enemy minimap dots, the build-grid overlay, move markers, dashed
  lines or turret fire, and no audibility claim for anything — the turret
  emits nothing at all by design this phase.
- **Render correctness stays a development-host claim.** Goldens are
  host-scoped and exact-match, per [testing strategy](05-testing.md); nothing
  here is a cross-platform or cross-backend claim.
- **No number in this document is a performance claim.** Tick counts, frame
  budgets and cooldowns are simulation quantities. Performance is
  **unmeasured**.
- **`body_overlaps=0` is a policy claim**, evaluated against the ADR 021
  gather rule — not a statement that no two bodies ever shared space.
- **The sandbox scene is on no timer and on no gate.** Everything it is for is
  a human step.

## Known gaps

- **G1 — the turret is silent.** No fire SFX asset exists this phase; firing is
  visible (to a human) but inaudible everywhere.
- **G2 — the Ghoul wears the Soldier's sheet.** Enemy art is out of scope; the
  Ghoul renders from the RTS soldier sprite slot with the label `GHOUL`.
  Readability on screen is a checklist judgment, not a proven claim.
- **G3 — no attack-cursor visual.** Arming Attack changes nothing on screen
  until the resolving click (the same pattern as rally). A misclick while armed
  is easy; recorded as a feedback-phase candidate.
- **G4 — the doc-lint scanners do not cover this page yet.**
  `no_perf_claim_in_docs` (`LIVE_DOCS`) and
  `rts_overlap_invariant_names_its_gather_exception` (`INVARIANT_DOCS`) in
  `tests/validation_contract.rs` were not extended to this file — that is a
  code change and this close is docs-only. Until a follow-up adds this page and
  a phase-2 map-resolver test, the table above is hand-verified only.
- **G5 — the defence loses.** `kills=2 losses=5` against 400 spawned enemies.
  The per-tick retarget through `nearest_hostile_in_range` spreads consecutive
  shots across a clump instead of finishing one Ghoul. Targeting persistence and balance are
  later phases; the gate pins the current behaviour rather than a wish.
- **G6 — no kill is attributable.** Nothing on the exit line says whether a
  kill came from an ordered soldier, an auto-acquire or a turret. Any sentence
  of the form "the turret killed *n*" is unprovable from the gate today.
- **G7 — `Attack` and `AttackMove` both read `MOVING` on the card.** The
  shipped status vocabulary is `IDLE`, `MOVING`, `BUILDING`, `FOLLOWING`,
  `MOVING TO MINERAL|GAS`, `COLLECTING MINERAL|GAS`, `RETURNING MINERAL|GAS`.
  There is no `ATTACKING` string, so the card cannot distinguish an attack-move
  from a plain move.
- **G8 — an enemy card has no status line.** An enemy selection returns kind
  and HP only: the read-only early return sits above the status-line code, so a
  marching Ghoul's card never says what it is doing.
- **G9 — the status line says `MINERAL` while the carry line says `CRYSTAL`.**
  User-chosen wording, kept on purpose; a one-token change if the two are ever
  unified.
- **G10 — the dashed line is a bearing, not a route.** A selected mover draws a
  straight dashed line to its goal. The unit actually descends a flow field
  around obstacles, so the dashes show *where*, never *how*.
- **G11 — the builder trap did not reproduce as reported, and one real freeze
  is still open.** The repro
  (`a_builder_is_never_trapped_by_the_building_it_finished`) passed on first
  run in both shapes of the report: in the densest cluster a 16-cell footprint
  can swallow, every one of the eight covered bodies landed *outside* the
  footprint, on a centre `StaticNav::position_clear` accepts, with its order
  already cleared to `Idle`, and each answered a fresh move order when ordered
  alone. What was fixed instead is a different genuine permanent freeze: a site
  whose evacuation can never succeed used to hold at `build_ticks - 1` forever,
  silently, with the resources already spent. That is now bounded by
  `STALLED_SITE_TICKS = 180` and cancels with a full refund. **Still open:** the
  same instrumented run observed a body queued behind seven others toward a
  shared destination moving 0 cells for 120+ ticks. That is formation/traffic
  behaviour, it was not fixed, and it is the most likely thing the original
  report was actually describing.
- **G12 — the sandbox's workers start idle.** Nothing in the engine gives a
  seeded unit a starting order and this phase added no such behaviour, so an
  unattended 3,000-tick sandbox run still reads `crystal=1500 gas=400`. Give
  the workers their first gather order by hand.
- **G13 — `MMD_REQUIRE_GPU=1` reds two tests, and it predates this phase.**
  `dummy_driver_run_does_not_touch_settings` and
  `no_rts_run_creates_the_real_user_config` both force
  `SDL_VIDEODRIVER=dummy` on purpose, hit `EXIT_NO_GPU`, and `or_skip` converts
  that skip into a failure while the variable is set. The documented gate
  command — `cargo test --workspace --locked`, without the variable — is green.
  Not caused by this plan; recorded so the next GPU-host run is not surprised.
- **Carried forward:** ADR 017's parked single-file corridor defect, and the
  development host's intermittent GPU device loss under sustained runs — both
  unchanged from the feedback-polish close.
