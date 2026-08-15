# ADR 021: RTS feedback polish and gather-worker collision policy

- Status: Accepted
- Date: 2026-08-15 (proposed 2026-08-14)
- Closes on: [feedback-polish functional close](../rts-feedback-polish-functional-close.md)
- Supersedes in part: [ADR 017](017_ADR_rts_hard_collision_navigation_and_formations.md) — unconditional RTS no-penetration invariant
- Supplements: [ADR 018](018_ADR_settings_window_canvas_and_camera.md), [ADR 019](019_ADR_hud_minimap_and_input_routing.md), [ADR 020](020_ADR_audio_events_buses_and_generated_assets.md)

## Context

Phase 1.1 landed working menu/settings/HUD/audio/hard-body systems. User feedback now asks for clearer controls, direct settings manipulation, world grid, easier placement, richer building feedback, positional command keys, exact drag-selection color, and gather-only worker overlap.

Several requests extend landed behavior rather than add missing systems:

- Barracks already exposes Soldier in command slot 0.
- Buildings already pick by footprint + show construction/rally basics.
- Settings tracks already click-snap + commit transactionally.
- Area selection already has green-ish 2px border.
- Right-click already cancels pending placement.

Collision change conflicts with ADR 017’s unconditional “no completed tick leaves two RTS unit bodies merged.” Order changes can leave two previously exempt workers penetrating. Exit policy must be explicit, deterministic, bounded, hashed, allocation-free.

## Decision

### Interaction controls

Every discrete interactive control has stable identity + framed visual state:

```text
Disabled > Pressed > Hover > Selected > Idle
```

Applies to Menu/modal buttons, selection icons, 3×3 command cells, checkbox label rows, audio mute labels. Sliders retain slider-specific range/thumb visuals.

HUD top-right becomes framed text `MENU` at `[1832,8,80,32]`. Existing point `[1888,24]` remains valid and the frame stays inside the 1920 logical edge. Pause Menu adds `CLOSE MENU` at `[800,588,320,64]` below the unmoved Settings button `[800,508,320,64]`, so the canonical script's `[960,540]` still opens Settings. Close clears menu + focus pause, warning, preserves manual Space pause.

One ticket pins the whole settings geometry before any behavior ticket touches it. The panel widens to `[440,100,1040,880]` (still centred on x=960) to make room for per-row numeric value fields at x 1368 and a scrollbar track at x 1432 without narrowing any track. Pan tracks, checkbox squares and `[1170,288]`'s meaning are unchanged; the audio block moves to a 96px pitch to fit framed mute-label rows. Body viewport is `[456,160,1008,720]`, ending above the fixed Back button at y 900.

Escape keeps nested FSM: Gameplay → Pause Menu; Settings → Pause Menu; Pause Menu → Gameplay. F10 remains unbound.

Pointer activation requires same stable control on down/up. Slider/scrollbar retain down owner through motion. Modal ownership never leaks to world.

### Settings model and UI

Schema/path stay compatible:

```text
schema_version = 1
settings-v1.json
```

Additive serde-defaulted fields:

- `gameplay.show_grid = true` when absent;
- `audio.{master,music,voice,sfx}_muted = false` when absent.

Old values survive. Invalid present types still trigger existing fallback warning.

Six numeric settings own one descriptor table: keyboard pan, edge pan, master, music, voice, SFX. Sliders commit each changed snapped step live through existing runtime/gain/save transaction. Duplicate motion inside same step does nothing.

Numeric fields accept max three ASCII digits. Enter + pointer blur + OS focus loss parse, clamp, nearest-step snap, commit. Empty restores original without save. Escape restores original + consumes key before menu navigation.

Audio label click toggles explicit persisted flag. Numeric level remains unchanged/editable. Muted gain becomes zero; stream continues. Unmute restores current exact level.

Settings body scrolls by wheel + retained draggable thumb. Content render/hit share one offset transform. CPU clipping adjusts quad position, size, UV; Back + warning remain fixed. Content height is 848px against a 720px viewport, so max scroll is exactly 128px at a 72px wheel step.

A live wheel event is mapped through the same `DisplayViewport::map_pointer` every other pointer event uses and dropped when it lands in a letterbox bar; it scrolls only on the Settings page with the mapped point inside the body viewport. Fractional trackpad deltas truncate toward zero and a zero delta emits no command, so no device can desync a replay.

A **scripted** run normalises `gameplay` settings to defaults exactly as it already normalises the camera. `show_grid` now reaches the observed exit line, so a persisted value would otherwise make the same script end differently per machine — the same machine-dependence the camera rule exists to prevent.

### World grid

Grid stays app/render config, outside `RtsWorld` + state hash. App default is on. Legacy engine `pack_frame` remains grid-off; app calls explicit pack options.

Grid is full map lattice:

```text
x=0..=width:  project(x,0) -> project(x,height)
y=0..=height: project(0,y) -> project(width,y)
```

Count = `width + height + 2`, max 1,026 at 512×512. Thin subdued texture-free diagonal-line instances pack before selection rings in depth-off procedural overlay. Line sentinel is distinct from ring sentinel; sprite instance remains 48 bytes; no atlas sample. Overlay capacity reserves max grid lines + max selection rings.

Grid does not affect nav, placement validity, picking, sim, or hash.

### Placement assistance

Raw valid cursor candidate wins. Raw invalid candidate searches footprint min corners with:

```text
abs(dx) <= footprint_edge
abs(dy) <= footprint_edge
```

Candidate rank, measured from the **saturated raw min corner** `ghost_min_corner(cursor_cell, edge)` — the cell the ghost is actually drawn at, which at the map's top/left edge is not `cursor - edge/2`:

1. squared distance from that raw min corner, on `i64` deltas;
2. lowest flat min-corner index `x + y * map_width`.

Validity remains exactly `placement_valid`; units remain non-obstacles. No valid bounded candidate → raw red ghost + click no-op. Packer + click commit consume same `placement_candidate`; displayed green footprint equals committed footprint.

### Building interaction

Player building pickshape:

```text
rendered sprite screen rect ∪ ground footprint
```

Owner/depth/lower-slot tie rules remain.

Single-building card is six total lines, in this exact order:

1. kind;
2. `READY` / `BUILDING N%`;
3. `SUPPLY +N`;
4. `QUEUE -` / `QUEUE W,S,...` oldest-first;
5. `PROGRESS -` / `PROGRESS N%`;
6. `RALLY -` / `RALLY x,y`.

Queue cap stays 5. No HP/armor invention.

### Command grid and area selection

Keys map by row-major position:

```text
Q W E  -> 0 1 2
A S D  -> 3 4 5
Z X C  -> 6 7 8
```

Keyboard + pointer resolve same current `command_slots` cell. Disabled/empty no-op. Keyboard emits no pointer-click SFX. Old semantic A/S/R mappings end; X becomes slot 7. Right-click remains only placement cancel.

Area selection packs pure-green 10%-opacity premultiplied fill `[0,0.1,0,0.1]`, then opaque pure-green 2px border `[0,1,0,1]` — through the **texture-free line primitive**, not `Prop::PanelFill`. That prop's atlas texel is `premul([16,18,24,200])` and the fragment shader returns `texel * tint`, so a pure-green tint through it renders dark and 78% opaque. The drag box is therefore five line instances (one full-height fill segment, four half-inset border segments) and shares its primitive with the world grid.

### Gather-worker collision

Dynamic mutual collision is ignored only when both live entities:

- are `UnitKind::Worker`;
- hold `Order::Gather`;
- use any `GatherPhase` combination;
- regardless ownership.

Static terrain/map edges/nodes/finished buildings remain hard. Any pair with non-worker or non-gather member remains hard. Formation slots, spawn, production, construction evacuation, placement-relocation targets remain all-body-free.

Pure geometry functions remain order-free. `RtsWorld` owns pair policy across repair, sweep, push chain, arrival, oracle.

Every active gather pair records provenance, even before overlap. State uses preallocated generation-safe dense triangular `u8` table (~2 MiB at `MAX_ENTITIES=2048`):

- `0`: hard/no provenance;
- `255`: active gather provenance;
- `1..=12`: completed gather-exit separation attempts.

State enters the hash with an exactly framed block after the order hash: a `u64` live-unit count, then `(index, generation)` as two `u32` LE per live unit in ascending slot order, then one byte per canonical `(i, j)` pair — including zero. The length prefix and the identity run are what make the block unambiguous; `EntityStore::hash_into` carries neither slot index nor generation, so the pair block supplies both rather than assuming the digest already separates two slot assignments. Slot generation change clears the row.

Exited penetrating pair gets max 12 deterministic attempts. One worker moves max 0.5 cell directly away per attempt; tick-rotated rank chooses first mover, partner retries if blocked. State advances `255 → 1` and `n → n+1`, and **an attempt that reaches non-penetration clears the pair to `0` in the same tick, before normal movement** — hard collision resumes immediately and the pair cannot re-merge on the tick it separated.

Concentric direction (centres within 1e-6) comes from the stable cardinal key `[[1,0],[0,1],[-1,0],[0,-1]][pair_index % 4]`, oriented from the lower-slot unit toward the higher-slot one: the lower-slot unit moves along `-normal`, the higher along `+normal`. Tick rotation changes the mover, never the normal. Candidate obeys static/admissibility + hard third-body checks; multi-pair movement cannot decrease another transition distance. The transition pass walks pairs in the same ascending live-unit order the hash uses, decides eligibility from a snapshot taken before the pass, and applies accepted moves immediately — one pass, never a fixpoint loop.

After attempt 12, still-penetrating pair falls back same tick. Rotated-priority worker, then partner, searches nearest legal centre free of every live body in same `StaticNav` connected region. Distance then flat cell index breaks ties. A successful relocation clears only that worker's `1..=12` transition entries; `255` entries are left alone, because active-gather provenance is mandatory for every active pair and the relocated worker may still be gathering with a third one.

No target for either worker → stash `TickError::UnrepairableOverlap` **and set the pair back to `0`**. There is no terminal exemption: the pair is hard again from that instant, `body_overlap_count` counts it, the `body_overlaps` exit token goes non-zero, and the next tick's generic repair retries it. A failed exit is a reported, self-retrying failure, never silent permanent grace — which matters because `RtsWorld::tick` returns `()` and the app does not poll `last_tick_error`.

Runtime invariant becomes:

> Every penetrating RTS-unit pair after tick is either both active gather workers or the exact remembered pair inside its bounded 12-attempt gather-exit transition. Every other pair is non-penetrating, or is counted by `body_overlaps` and reported through `TickError`.

`body_overlap_count`/`body_overlaps` report policy violations. Raw overlap stays testkit-only for anti-vacuity. Horde `sim/` remains ADR 009 soft-separation/overlap-capable.

## Consequences

- Default frame shows grid; RTS frame instance counts/golden pixels can change.
- Shader canonical hash changes; legacy sprite/ring pixels must not.
- Existing schema-1 files gain explicit bool keys on next successful save.
- X/R/A/S behavior changes by current command slot context.
- Gather paths/state hashes/resource timing can change; deterministic expectations need reviewed rebase.
- Legal gather overlap means raw RTS overlap can exceed zero. Policy violation count must remain zero.
- Pair table adds fixed ~2 MiB/world; no per-tick allocation/sparse-growth risk.
- ADR 017 parked single-file corridor defect remains open/out of scope.

## Rejected

- F10: Escape already owns confirmed menu navigation.
- Reset/bump settings without migration: loses user values.
- Muting by writing volume 0: destroys restore level.
- Naive cell-per-instance full grid: exceeds 100,000 renderer budget.
- Textured grid in overlay: slot 0/zombie atlas bug.
- Grid in world/hash: poisons deterministic sim with environment setting.
- Duplicate preview/commit placement searches: visible green can fail click.
- Semantic command bindings: break stable positional muscle memory.
- Global worker no-collision: removes blocking outside gather.
- Sparse pair map: growth/allocation/capacity ambiguity.
- Immediate gather-exit teleport: visible pop; user chose bounded separation.
- Indefinite exit grace: can leave permanent merged hard workers. A failed fallback returns the pair to hard state instead of parking it at attempt 12.
- `Prop::PanelFill` for exact UI colours: `texel * tint` cannot produce an opaque pure green.
- Ranking placement candidates from the cursor cell: diverges from the drawn ghost wherever `ghost_min_corner` saturates.
- Per-ticket layout re-derivation: one ticket pins every settings rect, so no two tickets can disagree about where a control is.
- Collision semantics inside pure geometry: violates module boundary.

## Validation contract

Implementation must prove:

- every control state + Menu/Close/pause rule;
- legacy settings preservation, slider/text/mute/scroll transaction rollback;
- exact grid line count/layer/capacity/no-hash/no-allocation;
- preview/commit placement parity;
- building union picker + exact detail lines;
- all 9 positional keys/right-click cancel;
- exact drag fill/border;
- all gather phase/owner combinations, hard third/static pairs, nested push coherence;
- 12-attempt exit, same-tick clear on contact, same-component fallback, failed-fallback return to hard state, preserved active provenance, slot reuse, framed hash, cross-process reproducibility;
- canonical + focused RTS scripts; unchanged horde smokes;
- full merge gate in `AGENT.md`/`docs/05-testing.md`.

## Landed evidence

Accepted on the implementation that landed, not on this document. Every claim
below names the symbol that carries it and a test that fails when it stops
being true; the whole map is in the
[functional close](../rts-feedback-polish-functional-close.md).

| Decision | Landed symbol | Test |
| --- | --- | --- |
| Framed control states, `MENU` text control, `CLOSE MENU` | `rts::hud::ControlVisualState`, `HudLayout::PAUSE_MENU_CLOSE_BTN` | `control_visual_states_have_distinct_tints`, `close_menu_is_below_settings` |
| Nested Escape FSM, Close clears menu + focus pause, keeps manual pause | `RtsUiState::close_menu` | `close_menu_clears_menu_and_focus_pause`, `close_menu_preserves_manual_pause` |
| Pinned settings geometry, six numeric descriptors | `rts::hud` layout constants, `rts::hud::NUMERIC_SETTING_SPECS` | `spec_rects_equal_the_pinned_layout_constants`, `numeric_specs_cover_exactly_six_settings` |
| Additive schema-1 fields, old values preserved | `GameplaySettings::show_grid`, `AudioSettings::*_muted` | `legacy_schema_one_preserves_values_and_defaults_new_fields` |
| Live snapped slider commits, no duplicate step | `rts_ui::SettingsChange` through the retained pointer owner | `slider_drag_commits_only_distinct_steps` |
| Three-digit fields: Enter / blur / focus loss / Escape | `rts_ui::NumericEdit` | `enter_clamps_snaps_and_commits`, `escape_restores_and_consumes_navigation` |
| Mute flags zero gain, keep the stored level | `SettingsChange::MasterMuted` and its three bus siblings | `unmuting_restores_gain_to_stored_level` |
| Wheel + thumb scroll over one shared offset, Back fixed | `rts::hud::clamp_settings_scroll`, `modal_hit_test(..., scroll_offset)` | `exit_line_reports_settings_scroll_px`, `focused_script_back_button_is_fixed_under_scroll` |
| Positional QWE/ASD/ZXC, right-click cancels placement only | `rts_input::KEY_BINDINGS` | `all_nine_command_keys_map_row_major`, `right_click_is_only_placement_cancel` |
| Building pick = sprite rect ∪ footprint | `rts::building_pick_contains`, `rts::building_screen_rect` | `building_pick_contains_sprite_and_footprint` |
| Six-line building card | `rts::hud` detail-text packing | `building_details_use_exact_six_line_contract` |
| One assisted-placement candidate for preview and commit | `rts::build::placement_candidate`, `ghost_min_corner` | `blocked_raw_snaps_to_nearest_valid_footprint`, `green_preview_commits_exact_displayed_min` |
| Texture-free diagonal line instances | `render::instance::DIAGONAL_LINE_SENTINEL` | `line_instance_keeps_pinned_layout`, `line_branch_never_samples_atlas` |
| Full-lattice world grid, default on, outside the hash | `rts::pack::FramePackOptions::show_grid` | `grid_packs_exact_map_lattice`, `grid_toggle_does_not_change_world_hash` |
| Drag box: 10% pure-green fill, opaque 2px border | `rts::pack` drag-box instances | `drag_box_has_pure_green_ten_percent_fill`, `drag_box_has_opaque_two_pixel_pure_green_border` |
| Gather-pair exemption, static and third bodies still hard | `RtsWorld` pair policy, `rts::collision::GatherCollisionState` | `two_gathering_workers_may_overlap`, `gather_workers_still_hit_static_geometry` |
| Bounded 12-attempt exit, same-tick clear on contact | `GatherCollisionState` `255 → 1 → n+1` | `exited_pair_separates_gradually`, `separated_pair_clears_to_hard_in_the_same_tick` |
| Same-component fallback, failed fallback returns to hard | `TickError::UnrepairableOverlap` | `blocked_pair_relocates_after_attempt_twelve_same_tick`, `failed_fallback_clears_to_hard_and_reports_a_violation` |
| Framed pair block in the state hash, cross-process stable | `GatherCollisionState::hash_into` | `gather_exit_state_reproduces_cross_process`, `pair_hash_frame_is_length_prefixed_and_identity_pinned` |
| Nothing on the joined frame allocates | — | `combined_feedback_frame_allocates_nothing` |

Differences from the decision as written, all narrowing rather than widening:

- The drag box packs **five** line instances (one fill segment, four half-inset
  border segments), which is what `drag_box_has_opaque_two_pixel_pure_green_border`
  measures; the decision above already anticipated this and it is recorded here
  because the count is the thing a future reader will check.
- The focused script `assets/scenarios/rts_feedback_polish_v1.script` was added
  next to the canonical one rather than folding the new controls into it: the
  canonical run's pinned audio counters are evidence for phase 1.1, and
  re-timing them to carry menu/grid/scroll clicks would have rewritten that
  evidence instead of adding to it.
- Three `rts_economy` node-click tests were left failing by the building-pick
  union and are **not** repaired here; they are recorded as an open regression
  in the [functional close](../rts-feedback-polish-functional-close.md), not as
  an accepted behaviour change.
