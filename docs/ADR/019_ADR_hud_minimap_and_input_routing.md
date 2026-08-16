# ADR 019: HUD, minimap, menu, and input routing

- Status: Accepted
- Date: 2026-08-10
- Accepted: 2026-08-12 (T18, on landed phase-1.1 evidence)
- Supplements: [ADR 014](014_ADR_movable_camera_texture_table_and_ui_layer.md), [ADR 018](018_ADR_settings_window_canvas_and_camera.md)
- Plan: `ai_artefacts/PLAN_2026_08_10_rts-interaction-ui-audio-hardening.md`

## Context

Current HUD is output-only: selection left, production center, text build rows right. Every mouse release still reaches world selection/order. No minimap, gear, menu, settings controls, command card, or UI ownership.

Phase1.1 needs StarCraft-like regions and strict HUD-first actions.

## Decision

### Logical layout

Fixed 1920×1080:

- top bar + gear `[1872,8,32,32]`;
- bottom panel y840..1080;
- minimap left `[16,856,384,208]`, map `[32,872,352,176]`;
- selection center `[424,856,880,208]`;
- command right `[1328,856,576,208]`, 3×3 grid `[1696,856,208,208]`.

Every textured HUD item remains `ui`; `overlay` stays procedural rings only.

### Selection center

- Single: 128×128 existing-atlas portrait + full details.
- Multi: first24 sorted IDs; 8×3, 48px icons, 8px gaps; `+N` overflow.
- Click icon isolates; Shift-click toggles.
- Primary remains lowest live slot.

### Command card

Fixed row-major 3×3. `CommandId` shared by mouse/hotkey executor.

- all-worker selection: HQ/Depot/Barracks slots0/1/2;
- exactly one finished HQ: Worker slot0, Rally slot8;
- exactly one finished Barracks: Soldier slot0, Rally slot8;
- other/mixed contexts: disabled.

Disabled/empty slots consume click. Rally arms pending action; next world click commits.

### Minimap

No terrain detail, entities, resources, fog. Draw isometric map diamond + four-edge projected camera polygon.

Raw mapping:

```text
rx=cx-cy
ry=(cx+cy)/2
```

Fit full diamond into map rect. Inverse click rejects outside diamond. Valid click calls fractional `Camera::look_at_point`; frontier clamps.

### Routing

Pointer owner selected at mouse-down, retained through motion/up:

1. window/focus/resize lifecycle;
2. Escape;
3. modal settings/menu;
4. gear;
5. minimap;
6. selection icons;
7. command card;
8. HUD background consume;
9. world.

Bar button events have no owner. World release outside content cancels gesture.

### Menu/settings

State pages: Gameplay, PauseMenu, Settings. Separate pause reasons: manual/menu/focus.

- Gameplay gear/Escape → paused menu.
- Menu contains one actionable Settings button.
- Settings Escape/Back → menu.
- Menu Escape → gameplay; preserves manual pause.
- Space toggles manual reason.
- Focus toggle can open paused menu.

Settings edits validate, apply runtime, persist, publish. Failure rolls runtime/config back + visible warning. No Apply button.

## Consequences

- `RtsFrame.ui` expands to texture slots4..8 for portraits/props/font.
- stdout UI group count changes from2 to5.
- HUD pack/hit geometry shares one layout source.
- Existing `R` cursor-immediate rally behavior is replaced by pending world click.
- UI consumes large screen areas; world tests must assert no leak.
- Minimap camera polygon may clip outside map diamond near frontier; clip/edge packing must remain deterministic.

## Rejected

- Display-only card/minimap: fails requested interaction.
- Dynamic command positions: weak muscle memory.
- Unlimited visible portraits: 2,048 IDs cannot fit.
- Top-down square minimap: user chose isometric diamond.
- UI click then world fallback: creates accidental orders.

## Implementation (as landed, T11–T13)

Shipped as decided, with two consequences this record has to carry because
they change contracts outside the HUD:

1. **Escape no longer quits.** In gameplay it opens the paused menu; from
   Settings it returns to the menu; from the menu it returns to gameplay,
   preserving a manual pause. That removed the only way a scripted run had to
   end itself, so `src/rts_script.rs` gained a **`quit` script token** — a bare
   verb taking no arguments — and the tracked acceptance script terminates with
   it instead of `key:esc` (`quit_parses_bare`, `quit_takes_no_arguments`,
   `quit_stops_the_frames_sweep`,
   `menu_escape_opens_the_pause_menu_without_quitting`). Any script still
   pressing Escape to exit now opens a menu and runs to its frame budget.
2. **Hit testing lives in `crates/mmd-engine/src/rts/minimap.rs`**, not in
   `hud.rs`: `HudHit` and `hud_hit_test` resolve a logical point against the
   same `MinimapProjection` the minimap draws with, and splitting them across
   two modules would have duplicated that projection. `hud.rs` still owns the
   layout rects both of them read.

The menu FSM is `src/rts_ui.rs` (`UiPage` = `gameplay | pause_menu |
settings`, reported on the exit line as `ui_page=`), and the gear opens exactly
what Escape opens (`menu_gear_click_opens_the_menu_exactly_like_escape`).

## Validation contract

Tests cover exact non-overlap rects, single/24/overflow, command contexts/slots, minimap roundtrip/polygon/click, icon actions, mouse-hotkey parity, rally pending, modal transition table/pause preservation, setting rollback, HUD background consumption, allocation-free packing.

## Amendment 2026-08-15 — control identity, positional keys, building pick

Supplemented by
[ADR 021](021_ADR_rts_feedback_polish_and_gather_collision.md):

- Every discrete control gains a stable identity and a framed visual state
  (`Disabled > Pressed > Hover > Selected > Idle`); pointer activation requires
  the same control on down and up.
- The top-right gear becomes the framed text control `MENU` at
  `[1832,8,80,32]`; its hit point `[1888,24]` is unchanged. The pause menu
  gains `CLOSE MENU` below the unmoved Settings button.
- Command keys bind by card **position** (`QWE`/`ASD`/`ZXC` → slots 0–8), so
  the old semantic A/S/R bindings end and `X` becomes slot 7. Right-click
  remains placement cancel only.
- A player building is picked by its rendered sprite rect ∪ its ground
  footprint, and the single-building card is exactly six lines.
