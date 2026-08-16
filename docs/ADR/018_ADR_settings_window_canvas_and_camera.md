# ADR 018: Settings, window modes, logical canvas, and camera frontier

- Status: Accepted
- Date: 2026-08-10
- Accepted: 2026-08-12 (T18, on landed phase-1.1 evidence)
- Supersedes in part: [ADR 014](014_ADR_movable_camera_texture_table_and_ui_layer.md) — fixed 24-cell/s camera and raw grid-edge clamp only
- Plan: `ai_artefacts/PLAN_2026_08_10_rts-interaction-ui-audio-hardening.md`

## Context

RTS window is fixed 1920×1080, centered, ordinary window. Swapchain blit stretches to any surface. Mouse coords remain raw window units. Camera keyboard + edge pan share fixed24 speed and clamp center directly to scenario edges.

Phase1.1 requires persistent settings, three window modes, pointer confinement, separate pan speeds, no stretch, and camera frontier distinct from map extent.

## Decision

### Settings

One app-owned schema1 JSON file:

```text
sdl3::filesystem::get_pref_path("AronGomu", "MillionsMustDie")/settings-v1.json
```

Defaults:

- mode borderless desktop;
- confine pointer true;
- pause on focus loss false;
- keyboard pan48; edge pan48; legal6..96 step6;
- master80/music35/voice70/SFX60; legal0..100 step5.

Missing file → defaults. Malformed/version/range failure → defaults + visible warning. Immediate accepted edits use recoverable temp/backup replacement. Offscreen runs use defaults; no pref-path I/O.

### Window modes

- Default: borderless desktop fullscreen (`display_mode=None`, fullscreen true).
- Exclusive: closest 1920×1080 mode; highest refresh tie.
- Windowed: 1280×720, centered, resizable.

Runtime transition releases GPU claim, applies/syncs/refreshes, reclaims. Failure rolls back old mode before error.

Pointer confined while focused, including menus. Focus loss releases + clears held pan/press/drag. Focus gain restores configured confinement, never stale input. Optional `pause_on_focus_loss` opens paused menu; default sim continues.

### Logical canvas

World/UI remain fixed 1920×1080. Destination uses largest centered exact 16:9 integer rect (`16k×9k`). Swapchain clear supplies bars. Nearest filtering stands.

One `DisplayViewport` maps both directions:

- window units→drawable px→logical canvas;
- button outside content ignored;
- motion outside clamps to logical edge, enabling content-edge pan;
- scripted input already uses logical coords and bypasses conversion.

No dynamic render target/reflow.

### Camera frontier

Playable extent = scenario width/height. Camera area = projected map AABB inset by half logical view:

```text
map x=[-height*tw/2, width*tw/2]
map y=[0, (width+height)*th/2]
```

If inset axis collapses, center locks to midpoint. Candidate center projects, clamps x/y, unprojects. This permits unavoidable empty triangular corners; true diamond erosion cannot contain current rectangular view.

Keyboard + edge intents stay separate. Screen cardinal→cell basis `[sx+sy, sy-sx]`. Speeds apply independently, diagonal unnormalized.

Current `IsoView::frame_uniforms()` travels with each RTS scene pass so depth bias follows camera. Legacy phase0 path/golden unchanged.

## Consequences

- Config path/schema becomes migration contract.
- Non-16:9 display gets bars, not distortion/crop.
- Exclusive/fullscreen/grab physical behavior remains host-manual evidence; pure state/sequence tests gate.
- Existing Escape-quit and raw camera clamp tests become obsolete.
- Camera center remains hashed; settings/input intents do not.
- ADR014 texture/UI/depth decisions stand except fixed speed/raw clamp.

## Rejected

- Stretch: distorts sprites/HUD.
- Dynamic render target: larger renderer/UI scope.
- Raw-grid center clamp: exposes off-map view.
- Release pointer in menus: conflicts confirmed focused confinement.
- Settings inside engine world/hash: environment state would poison determinism.

## Implementation (as landed, T7–T10)

Shipped as decided. Three record-level clarifications:

1. **"Visible warning" is a stdout line at startup, not a HUD element.** A
   missing, malformed, out-of-range or wrong-schema file falls back to defaults
   and prints `rts: settings warning=<escaped>`; the effective values are then
   echoed on the `rts: settings mode=… keyboard_pan=… master=…` line, which is
   also what the tests assert against. The in-panel `SETTINGS NOT SAVED:…`
   warning (ADR 019) covers *live edit* failures; these two are different
   surfaces and both landed.
2. **The offscreen path never looks at the pref path at all** — not "loads
   defaults after reading". There is therefore no warning line offscreen, and a
   malformed real file is left byte-identical
   (`offscreen_run_does_not_touch_settings`,
   `offscreen_settings_run_does_not_touch_settings`).
3. **One canvas degenerate case the decision did not name.** A drawable smaller
   than a single 16×9 unit has no exact aspect-fit rect, so the viewport falls
   back to identity rather than refusing to draw
   (`refresh_viewport_falls_back_to_identity_below_one_16x9_unit`,
   `too_small_drawable_has_no_fit`).

Code: `src/rts_settings.rs` (`RtsSettings`, `WindowMode`), `src/rts_window.rs`
(mode/focus/grab sequences and rollback),
`crates/mmd-engine/src/render/viewport.rs` (`aspect_fit_16_9`,
`DisplayViewport`), `crates/mmd-engine/src/render/camera.rs`
(`CameraFrontier`). The engine mirrors the settings default as
`rts::DEFAULT_CAMERA_PAN_SPEED = 48.0`, so a world built without an app still
pans at the documented speed.

## Validation contract

Tests cover schema/defaults/fallback/recovery/offscreen isolation; exact aspect rect/HiDPI inverse/bar semantics; pure window sequences/rollback/focus clear; projected frontier/split speeds/look-at; per-frame depth uniforms; unchanged phase0 golden.

## Amendment 2026-08-15 — settings surface, schema and scripted normalisation

Supplemented by
[ADR 021](021_ADR_rts_feedback_polish_and_gather_collision.md), which extends
this record without changing what it decided:

- The settings panel widens to `[440,100,1040,880]` with a scrolled body
  viewport `[456,160,1008,720]`; the pan tracks, the checkbox squares and the
  meaning of `[1170,288]` are unchanged, and Back stays fixed outside the
  scroll transform.
- Schema stays `1` at the same path. `gameplay.show_grid` and the four
  `audio.*_muted` flags are additive serde-defaulted fields, so a phase-1.1
  file loads with its values intact.
- Every numeric setting gains a live snapped slider and a three-digit typed
  field; both commit through the transaction this ADR already defined.
- A **scripted** run now normalises `gameplay` settings to defaults exactly as
  this record already normalises the camera, because `show_grid` reaches the
  observed exit line.
