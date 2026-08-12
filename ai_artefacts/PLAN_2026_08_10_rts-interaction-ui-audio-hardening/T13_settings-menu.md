# T13: Add gear menu and settings panel

**Plan:** `./ai_artefacts/PLAN_2026_08_10_rts-interaction-ui-audio-hardening.md`  
**Depends:** T7, T10, T12  
**Commit outcome:** gear/Escape opens paused one-button menu; nested settings edits camera/window/pointer/focus/audio values transactionally and persists each accepted change.

## Context (self-contained)

- Goal: expose confirmed settings with RTS-like nested modal semantics.
- This slice: menu FSM, pause reasons, controls, immediate save/apply, warnings.
- Out of scope here: physical audio response (T16); audio values still persist/update in-memory.
- Assumptions: focused menus retain pointer confinement; focus-pause toggle defaults off; menu contains exactly one actionable `Settings` button; no Quit button requested.
- Worker-added assumptions (approved by supervisor mid-implementation, see below):
  - Escape follows the FSM literally (Gameplay -> PauseMenu, no quit-via-Escape anymore). This is a direct, unavoidable consequence, not scope creep: it required (a) a new script-only `quit` token in `src/rts_script.rs` (mapped to the existing `RtsCommand::Quit`, no keyboard binding), (b) retargeting `assets/scenarios/rts_acceptance_v1.script`'s terminating `1450:key:esc` to `1450:quit` (no other line touched), and (c) updating the two `tests/rts_cli_contract.rs` assertions that drove quit through `key:esc` (`a_quit_on_frame_one_renders_nothing`, `the_window_is_released_before_it_drops`) to use `quit` instead. `RtsCommand::Escape` is a new, separate command bound to the Escape key; the window banner text changed from "Esc quit" to "Esc menu".
  - `hud_rally_arms_and_waits_for_the_next_world_click` (`T12`, `tests/rts_cli_contract.rs`) assumed a gear click was a pure no-op; `T12`'s own `HudHit::Gear` comment flagged this as "awaiting this ticket". Updated to route through an open-then-Escape-closed menu detour with an equalized frame budget (the modal pauses the sim for exactly the one frame it's open).
  - `SettingsChange`'s pan/volume payloads are `u32` (matching `RtsSettings`'s actual field types), not the ticket snippet's `u8` — the snippet is illustrative; the real T7 schema is `u32`.
  - Settings-row -> `SettingsChange` field order top-to-bottom matches the `SettingsChange` enum's own declared order (`WindowMode, KeyboardPan, EdgePan, Confine, PauseOnFocusLoss, Master, Music, Voice, Sfx`), i.e. row y=468 is Confine, y=516 is PauseOnFocusLoss — the ticket's prose ("focus/confine 468/516") names them in the opposite order from the rects; the enum order was taken as authoritative.
  - Per the hard isolation constraint (settings must never touch real/temp disk in offscreen runs), the `T13` CLI regression (`menu_*`, all offscreen) exercises only FSM navigation and pause-reason bookkeeping — no scripted click ever lands on a value-changing settings control (window mode / sliders / checkboxes). That path is instead covered exhaustively by pure unit tests on `rts_ui::commit_setting_change` (fake `WindowOps`, temp-dir `SettingsStore`) and is wired for real interactive use only at the one live `MouseButtonUp` left-click site in `src/rts_run.rs` (manual/visual verification only — no real window on this worker's host).
  - Engine-crate `PAN_MIN`/`PAN_MAX`/`PAN_STEP`/`VOLUME_MIN`/`VOLUME_MAX`/`VOLUME_STEP` (`crates/mmd-engine/src/rts/hud.rs`) duplicate `src/rts_settings.rs`'s private bound constants rather than the app crate depending on the engine's copy or vice versa (dependency direction only allows app -> engine); kept honest by `rts_ui::tests::settings_pan_and_volume_bounds_match_app_contract`.
- Decisions: ADR 018 + ADR 019.

## Requirements

- Extend `src/rts_ui.rs`:
  ```rust
  pub enum UiPage { Gameplay, PauseMenu, Settings }
  #[derive(Default)] pub struct PauseReasons { pub manual: bool, pub menu: bool, pub focus: bool }
  pub struct RtsUiState { pub page: UiPage, pub pauses: PauseReasons, /* pointer/input/warning */ }
  pub enum SettingsChange { WindowMode(WindowMode), KeyboardPan(u8), EdgePan(u8), Confine(bool), PauseOnFocusLoss(bool), Master(u8), Music(u8), Voice(u8), Sfx(u8) }
  pub fn sim_paused(&self) -> bool;
  ```
- FSM:
  - Gameplay gear/Escape → PauseMenu + `menu=true`;
  - PauseMenu Settings button → Settings;
  - Settings Escape/Back → PauseMenu;
  - PauseMenu Escape → Gameplay + `menu=false`; preserve manual pause;
  - Space toggles manual pause only; does not skip page nesting;
  - focus loss toggle off: clear input only; on: `focus=true`, open PauseMenu; user closing menu clears focus reason.
- Pause menu modal `[720,408,480,264]`; only Settings button `[800,508,320,64]`.
- Settings panel `[520,100,880,880]`; exact rows: display 180, keyboard 276, edge 372, focus/confine 468/516, master 584, music 656, voice 728, SFX 800; Back `[552,900,160,56]`.
- Window mode = three labeled buttons. Speeds/volumes = tracks; click snaps nearest legal step (6/5). Toggles explicit checkbox state.
- Extend `pack_hud(world, ui_state, settings, frame)` to render modal last in textured UI; world + normal HUD stay visible underneath.
- Modal owns every pointer point while open; no world/HUD action leaks.
- Transactional setting commit:
  1. clone current settings; apply/validate change;
  2. apply runtime adapter (window/camera/pointer; audio hook no-op until T16);
  3. save T7 store;
  4. publish candidate;
  5. if runtime/save fails, apply old runtime values, retain old cfg, show `SETTINGS NOT SAVED: <reason>`.
- Window-mode runtime change uses T10 release/apply/reclaim rollback.
- Every accepted discrete change saves before handler returns. No Apply button.

## Inputs

- T7 `RtsSettings`, `SettingsStore`, validation.
- T10 window/focus/grab adapter + `FocusAction`.
- T12 `HudHit::Gear`, pointer owner/router, HUD consumption.
- T11 HUD pack/UI groups.
- **From Depends:** exact APIs above.

## TDD

1. **Red** — entire transition table, pause-reason preservation, modal consumption, snap/transaction/rollback tests.
2. **Green** — reducer + modal layout/packing + controller.
3. **Refactor** — remove standalone `paused` bool; route Space/Escape/gear/focus through reducer only.

## Test plan

| Test | Input | Expect |
| --- | --- | --- |
| `gameplay_escape_opens_paused_menu` | Escape | PauseMenu; paused |
| `menu_contains_only_settings_action` | pack/hit | one enabled button |
| `escape_backs_out_one_level` | Settings then Esc twice | PauseMenu then Gameplay |
| `manual_pause_survives_menu_close` | Space, Esc open/close | still paused |
| `focus_toggle_controls_pause_reason` | lost focus off/on | run / menu+pause |
| `modal_consumes_all_pointer_input` | clicks outside panel | no world/HUD actions |
| `sliders_snap_to_legal_steps` | between ticks | nearest 6/5 |
| `each_accepted_change_saves_once` | fake store | exactly one canonical write |
| `failed_save_rolls_back_runtime_and_cfg` | injected failure | old values + visible warning |
| `window_mode_change_uses_safe_transition` | mode click | release/apply/reclaim sequence |

## Impl steps

- [x] 1. Add reducer transition/pause-reason red tests. (`src/rts_ui.rs` `tests` mod: `gameplay_escape_opens_paused_menu`, `escape_backs_out_one_level`, `manual_pause_survives_menu_close`, `space_never_changes_the_page`, `focus_toggle_controls_pause_reason`, `gear_only_opens_from_gameplay`.)
- [x] 2. Add modal layout/hit/pack tests. (`crates/mmd-engine/tests/rts_hud.rs` `settings_*`; `src/rts_ui.rs` `modal_owns_every_pointer_point_while_open`, `settings_button_and_back_button_hit`, `sliders_snap_to_legal_steps`.)
- [x] 3. Implement `UiPage`, `PauseReasons`, reducer; remove direct pause/Escape quit. (`src/rts_ui.rs::{UiPage, PauseReasons, RtsUiState}`; `RtsSession.paused: bool` removed, replaced by `RtsSession.ui: RtsUiState`; Escape repurposed to `RtsCommand::Escape`, see Assumptions.)
- [x] 4. Pack pause menu + settings panel in existing UI groups. (`mmd_engine::rts::pack_modal` appends to `frame.ui`'s props/font groups; app-crate `rts_ui::pack_hud` wraps `mmd_engine::rts::pack_hud` + `pack_modal`.)
- [x] 5. Implement settings hit map, snapping, toggle/mode actions. (`mmd_engine::rts::modal_hit_test` + `snap_track`; `rts_ui::handle_modal_click`.)
- [x] 6. Add transactional settings controller + rollback/warning. (`rts_ui::commit_setting_change` + `apply_runtime`; unit-tested: `each_accepted_change_saves_once`, `failed_save_rolls_back_runtime_and_cfg`, `window_mode_change_uses_safe_transition`, `a_rejected_change_never_touches_runtime_or_disk`, `no_window_skips_runtime_but_still_saves`.)
- [x] 7. Connect focus request, pointer confinement, camera speeds, window mode. (`src/rts_run.rs`: `FocusAction::PauseRequested` -> `session.ui.focus_lost()`; left-click modal commit wired through `SdlWindowOps` + `settings_store`; `world.set_camera_speeds` called from `commit_setting_change`.)
- [x] 8. Add scripted gear/settings/Escape CLI regression. (`tests/rts_cli_contract.rs` `menu_*`, 6 tests — navigation/pause-reason only, see Assumptions for why value-changing modal clicks are not scripted.)

## Outputs

- Modified: app UI/run/input/settings/window; engine HUD pack/tests; CLI tests.
- App API: UI/pause/change types above.
- Behavior: nested paused menu + immediately persisted settings.
- Config: all schema-1 fields editable; no schema change.

## Validation

- [x] `cargo test -p millions_must_die --locked rts_ui` — 17 passed.
- [x] `cargo test -p mmd-engine --locked --test rts_hud settings_` — 9 passed.
- [x] `cargo test -p millions_must_die --locked --test rts_cli_contract menu_` — 6 passed.
- [x] `cargo check --workspace --all-targets --all-features --locked` — clean.
- [ ] manual check: gear→Settings; edit each control; relaunch; values persist; Escape nesting correct — queued in `ai_artefacts/manual_test_checklist.md`'s new `T13 settings-menu` section (no real window/pointer grab from this worker, per harness constraint).
- [x] app functional: menu closed → sim resumes unless manually paused — `menu_escape_nesting_backs_out_to_gameplay`, `menu_manual_pause_survives_the_menu`.
- [x] commit msg draft: `feat(rts): add nested menu with live persisted settings` — used verbatim for the commit.
