# T10: Wire window modes, focus, and pointer confinement

**Plan:** `./artifacts/PLAN_2026_08_10_rts-interaction-ui-audio-hardening.md`  
**Depends:** T7, T8  
**Commit outcome:** default borderless desktop, exclusive closest-1920×1080, and 1280×720 resizable windowed modes apply safely; focus clears input/releases/restores configured grab.

## Context (self-contained)

- Goal: make persisted display/pointer/focus settings affect real SDL window without contaminating offscreen runs.
- This slice: window adapter + mode transitions + focus lifecycle. No visual settings panel yet.
- Out of scope here: nested menu FSM, HUD controls, audio device.
- Assumptions: window built resizable once; fullscreen ignores flag; menu later keeps grab while focused; default focus loss keeps sim running; optional focus pause emits request for T13.
- Decision: `docs/ADR/018_ADR_settings_window_canvas_and_camera.md`.

## Requirements

- Create `src/rts_window.rs`:
  ```rust
  pub trait WindowOps { /* narrow tested wrappers for mode/size/border/grab/sync */ }
  pub struct RtsWindowState { pub mode: WindowMode, pub focused: bool, pub viewport: DisplayViewport }
  pub enum FocusAction { None, PauseRequested }

  pub fn build_rts_window(video: &VideoSubsystem, mode: WindowMode) -> Result<Window, RunError>;
  pub fn apply_window_mode<W: WindowOps>(window: &mut W, mode: WindowMode) -> Result<(), RunError>;
  pub fn refresh_viewport(window: &Window) -> Result<DisplayViewport, RunError>;
  pub fn handle_focus<W: WindowOps>(...)->Result<FocusAction, RunError>;
  ```
- Build hidden + resizable; apply mode; center where relevant; show; claim GPU window afterward.
- Mode sequences:
  - Borderless desktop: leave fullscreen → `set_display_mode(None)` → borderless → fullscreen true → `sync`.
  - Exclusive: choose current display `get_closest_display_mode` for 1920×1080, highest refresh on equal geometry → bordered true → set mode Some → fullscreen true → sync.
  - Windowed: fullscreen false → mode None → bordered true → size 1280×720 → centered → sync.
- Runtime transition on claimed window: release GPU claim → apply mode/sync/refresh viewport → reclaim → redraw. On failure, attempt old mode + reclaim; return actionable error if rollback fails. Never drop claimed window.
- Pointer: focused + `confine_pointer` → `Window::set_mouse_grab(true)` must return success; focus lost always false. Focus gained restores true when configured.
- Focus lost always clears `pan_held`, keyboard/edge dirs, press, drag, pointer owner. Never restore held input on gain.
- `pause_on_focus_loss=false` → `FocusAction::None`; true → `PauseRequested`. T13 maps request to menu pause.
- Window resize/pixel-size/display-change refreshes T8 `DisplayViewport`.
- Offscreen path constructs no window state and calls no SDL display/grab methods.
- Native exclusive/fullscreen claim is manual/platform evidence only; merge gate tests pure `WindowOps` sequence + existing offscreen app.

## Inputs

- `src/rts_run.rs` window claim/release/event loop/session input fields.
- pinned SDL methods listed in ADR 018.
- `render::DisplayViewport` from T8.
- **From Depends:** T7 `WindowMode`, confine/pause settings; T8 viewport refresh/input math.

## TDD

1. **Red** — fake `WindowOps` exact call sequence/rollback/focus clear/grab tests; offscreen no-window test.
2. **Green** — adapter + real wrappers; integrate startup/events.
3. **Refactor** — keep claim/release in one transition fn; no raw mode calls in `rts_run.rs`.

## Test plan

| Test | Input | Expect |
| --- | --- | --- |
| `borderless_desktop_is_default_sequence` | default cfg | exact leave/mode/border/fullscreen/sync calls |
| `exclusive_chooses_closest_1920x1080` | fake mode list | closest; highest refresh tie |
| `windowed_is_1280x720_resizable` | Windowed | exact size/center; builder resizable |
| `failed_mode_change_rolls_back_before_reclaim` | injected failure | old mode restored or actionable fatal |
| `focus_loss_clears_every_held_input` | pan/drag/press | all zero/None |
| `focus_loss_releases_pointer` | confined | grab false |
| `focus_gain_restores_configured_grab` | true/false cfg | true/false |
| `pause_request_respects_toggle` | focus event | request only when enabled |
| `offscreen_never_builds_window_adapter` | offscreen run | no fake calls |

## Impl steps

- [x] 1. Add fake adapter + red sequence/focus tests.
- [x] 2. Implement mode selector/sequences against `WindowOps`.
- [x] 3. Implement real SDL wrappers + startup default mode.
- [x] 4. Add release/apply/reclaim rollback transition.
- [x] 5. Route focus/resize/display events; clear held input exactly once.
- [x] 6. Apply pointer confinement on startup/gain/loss.
- [x] 7. Add offscreen no-window regression; keep native smoke manual.

## Outputs

- New: `src/rts_window.rs`.
- Modified: main/run/input/CLI tests.
- App API: adapter/state/focus types above.
- Behavior: three modes + pointer/focus lifecycle.
- Config: consumes T7 display/gameplay fields.

## Validation

- [x] `cargo test -p millions_must_die --locked rts_window` — 11 passed
- [x] `cargo test -p millions_must_die --locked --test rts_cli_contract focus_` — 1 passed
- [x] `cargo test -p millions_must_die --locked --test rts_cli_contract offscreen_never_builds_window_adapter` — 1 passed
- [x] `cargo check --workspace --all-targets --all-features --locked` — clean
- [ ] manual check: cycle 3 modes; Alt-Tab releases pointer; regain confines; no stuck pan/drag —
      left for a human: this host is the maintainer's live desktop session, and an
      agent must not seize its focus/grab the pointer to self-verify a manual step.
- [x] app functional: `SDL_VIDEODRIVER=offscreen cargo run -- rts --frames 3` — clean exit
- [x] commit msg draft: `feat(app): apply RTS window modes and focus-safe confinement`
