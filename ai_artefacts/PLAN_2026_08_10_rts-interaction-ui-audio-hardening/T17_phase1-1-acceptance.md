# T17: Prove phase 1.1 end to end

**Plan:** `./ai_artefacts/PLAN_2026_08_10_rts-interaction-ui-audio-hardening.md`  
**Depends:** T6, T9, T13, T16  
**Commit outcome:** tracked 1,600-frame live-equivalent script proves visible-edge gather, hard bodies, clickable HUD/minimap/settings, production, and deterministic fake-audio counters.

## Context (self-contained)

- Goal: join every lane through one shipped-binary acceptance path; no “unit-tested but unwired” feature.
- This slice: final script/events/exit evidence/cross-process hash + allocation assertions.
- Out of scope here: docs/status close (T18), real-device audio claim, perf numbers.
- Assumptions: offscreen uses canonical defaults/fake audio; current tracked relocated worker coordinates from T3 are authoritative; frame budget stays exactly1600.

## Requirements

- Update `assets/scenarios/rts_acceptance_v1.script`; preserve comments + 1-based deterministic ordering.
- Resource regression uses visible quad interior corner, not ground/node cell center:
  - nearest crystal ground remains `(920,458)` at initial camera; click `(897,411)` (inside top-left of `[896,410,48,48]`);
  - gas uses same one-pixel-inside corner derived from current ground via shared helper, committed as literal with comment.
- Use T3 current relocated-worker literals for selections; do not reintroduce old adjacent-spawn assumptions.
- Replace Q/W/E/A/S production/build hotkeys in acceptance with command-grid clicks at exact centers:
  - slot0 `[1728,888]`, slot1 `[1800,888]`, slot2 `[1872,888]` under T11 layout.
- Add invalid world order at `[1900,100]` while one worker selected → one Reject; ensure point is in logical content, outside map/HUD.
- Add minimap click near right diamond interior (derive/commit exact literal from `MinimapProjection`, not guessed panel point) → camera center changes + clamps.
- Add gear `[1888,24]` → Settings `[960,540]` → keyboard slider exact click producing legal 78 cells/s → Escape to menu → Escape to gameplay. Offscreen controller changes in-memory only; no user file.
- Script still completes crystal+gas gathering, Depot, Barracks, Worker, Soldier by frame1600; sim resumes after menu.
- Extend deterministic exit line with:
  - `body_overlaps=0`
  - `ui_page=gameplay`
  - `music_starts=1`
  - `voice_select=8`
  - `voice_order=9`
  - `voice_reject=1`
  - `sfx_ui=8`
  - `keyboard_pan=78`
  Counts follow exact script: initial six + two later newly selected workers; six crystal + one gas + two build receipts; four command clicks + minimap + gear + Settings + slider.
- Add engine milestone oracle iterating every live unit pair: distance² >= `(r1+r2)²`; assert initial, after each order phase, final.
- Cross-process determinism reexec compares full state hash + counters; seed0 canonical.
- Add/extend frame allocation test covering joined collision/formation/HUD/minimap/fake-audio frame after warm load; no perf timing.
- Physical window/audio/window-mode/confinement remain manual evidence only; offscreen acceptance claims state machines/assets/events, not sound/display hardware.

## Inputs

- Current tracked script (already repaired by T3).
- `tests/rts_acceptance.rs`, `crates/mmd-engine/tests/rts_acceptance.rs`, `tests/rts_cli_contract.rs`.
- T6 body-safe full world; T9 camera exit state; T13 UI/settings actions; T16 fake/physical sink split.
- **From Depends:** exact output APIs/counters above.

## TDD

1. **Red** — add exit-field/parser/body/event assertions; change script to corner/HUD/minimap/menu actions; watch old app fail.
2. **Green** — only missing integration glue/observation fields; do not redesign subsystems.
3. **Refactor** — central acceptance helper derives overlap/audio counters; stdout field order documented once.

## Test plan

| Test | Input | Expect |
| --- | --- | --- |
| `resource_click_uses_visible_quad_corner` | script literal 897,411 | Gather receipt; crystal banks |
| `acceptance_never_has_body_penetration` | milestone pair scans | 0 overlaps every scan |
| `acceptance_uses_command_card_for_build_and_produce` | four slot clicks | Depot/Barracks/Worker/Soldier complete |
| `acceptance_minimap_click_moves_camera` | projected mini point | center changes, remains frontier-safe |
| `acceptance_menu_settings_round_trip_resumes` | gear/settings/slider/Esc/Esc | Gameplay; tick continues; speed78 |
| `acceptance_audio_counts_are_exact` | fake sink | 1/8/9/1/8 fields above |
| `phase1_1_run_is_cross_process_deterministic` | two subprocesses | same hash/counters |
| `joined_phase1_1_frame_allocates_nothing` | representative frame | 0 allocations |
| existing phase-0 smokes | unchanged cmds | unchanged hashes/contracts |

## Impl steps

- [x] 1. Add exit parser + red exact counter/page/body tests.
  - validate: `tests/rts_acceptance.rs` parses `body_overlaps`/`ui_page`/`music_starts`/`voice_select`/`voice_order`/`voice_reject`/`sfx_ui`/`keyboard_pan` and the cases fail against the pre-change binary.
- [x] 2. Replace tracked script resource clicks with exact visible-corner literals.
  - validate: script contains `897,411` and `1049,559`; engine acceptance proves each is one pixel inside the node's `sprite_screen_rect` and picks that node.
- [x] 3. Replace build/produce keys with exact command-grid centers.
  - validate: script has no `key:w|e|a|s`; contains `1800,888`, `1728,888`, `1872,888`; run still reports `buildings=3` and a Soldier.
- [x] 4. Add one invalid order, minimap click, gear/settings/slider/Escape sequence.
  - validate: script contains `1900,100`, `360,960`, `1888,24`, `960,540`, `1170,288`, two `key:esc`; exit line reports `voice_reject=1`, `ui_page=gameplay`, `keyboard_pan=78`.
- [x] 5. Add joined overlap oracle + fake sink counters to exit observation.
  - validate: `RtsWorld::body_overlap_count` exists and the exit line reports `body_overlaps=0`; `AudioCounters` splits select/order cues.
- [x] 6. Update engine/app acceptance expected state/hash after review.
  - validate: `cargo test -p mmd-engine --locked --test rts_acceptance` and `cargo test -p millions_must_die --locked --test rts_acceptance` both green.
- [x] 7. Add cross-process + joined allocation test.
  - validate: `phase1_1_run_is_cross_process_deterministic` and `joined_phase1_1_frame_allocates_nothing` both green.
- [x] 8. Run exact 1,600-frame smoke; do not raise budget or weaken assertions.
  - validate: `cargo run -- rts --frames 1600 --inject-input-file assets/scenarios/rts_acceptance_v1.script` exits 0 with the exact counters.

## Outputs

- Modified: tracked script, engine/app acceptance, CLI/run/script/allocation observation.
- Behavior: all phase1.1 user flows proven through shipped command/input path.
- Public stdout: new exact fields listed above.
- Config: offscreen settings stay memory-only.

## Validation

- [x] `cargo test -p mmd-engine --locked --test rts_acceptance`
- [x] `cargo test -p millions_must_die --locked --test rts_acceptance`
- [x] `cargo test -p millions_must_die --locked --test rts_cli_contract`
- [x] `cargo test -p mmd-engine --locked --test frame_allocations joined_phase1_1_frame_allocates_nothing`
- [x] `cargo run -- rts --frames 1600 --inject-input-file assets/scenarios/rts_acceptance_v1.script`
  - run as `SDL_VIDEODRIVER=offscreen cargo run -- rts --frames 1600 --inject-input-file ...`: an
    agent may not open a window, grab the pointer or play audio on the live desktop. The windowed
    form of the same command is on the human checklist.
- [x] app functional: exit0; exact counters; Worker+Soldier+buildings/resources present
- [x] commit msg draft: `test(rts): prove phase 1.1 feedback loop through live-equivalent input`
