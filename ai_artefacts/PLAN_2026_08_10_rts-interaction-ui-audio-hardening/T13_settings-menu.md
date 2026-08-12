# T13: Add gear menu and settings panel

**Plan:** `./ai_artefacts/PLAN_2026_08_10_rts-interaction-ui-audio-hardening.md`  
**Depends:** T7, T10, T12  
**Commit outcome:** gear/Escape opens paused one-button menu; nested settings edits camera/window/pointer/focus/audio values transactionally and persists each accepted change.

## Context (self-contained)

- Goal: expose confirmed settings with RTS-like nested modal semantics.
- This slice: menu FSM, pause reasons, controls, immediate save/apply, warnings.
- Out of scope here: physical audio response (T16); audio values still persist/update in-memory.
- Assumptions: focused menus retain pointer confinement; focus-pause toggle defaults off; menu contains exactly one actionable `Settings` button; no Quit button requested.
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

- [ ] 1. Add reducer transition/pause-reason red tests.
- [ ] 2. Add modal layout/hit/pack tests.
- [ ] 3. Implement `UiPage`, `PauseReasons`, reducer; remove direct pause/Escape quit.
- [ ] 4. Pack pause menu + settings panel in existing UI groups.
- [ ] 5. Implement settings hit map, snapping, toggle/mode actions.
- [ ] 6. Add transactional settings controller + rollback/warning.
- [ ] 7. Connect focus request, pointer confinement, camera speeds, window mode.
- [ ] 8. Add scripted gear/settings/Escape CLI regression.

## Outputs

- Modified: app UI/run/input/settings/window; engine HUD pack/tests; CLI tests.
- App API: UI/pause/change types above.
- Behavior: nested paused menu + immediately persisted settings.
- Config: all schema-1 fields editable; no schema change.

## Validation

- [ ] `cargo test -p millions_must_die --locked rts_ui`
- [ ] `cargo test -p mmd-engine --locked --test rts_hud settings_`
- [ ] `cargo test -p millions_must_die --locked --test rts_cli_contract menu_`
- [ ] `cargo check --workspace --all-targets --all-features --locked`
- [ ] manual check: gear→Settings; edit each control; relaunch; values persist; Escape nesting correct
- [ ] app functional: menu closed → sim resumes unless manually paused
- [ ] commit msg draft: `feat(rts): add nested menu with live persisted settings`
