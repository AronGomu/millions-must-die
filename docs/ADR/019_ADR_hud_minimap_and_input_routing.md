# ADR 019: HUD, minimap, menu, and input routing

- Status: Proposed
- Date: 2026-08-10
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

## Validation contract

Tests cover exact non-overlap rects, single/24/overflow, command contexts/slots, minimap roundtrip/polygon/click, icon actions, mouse-hotkey parity, rally pending, modal transition table/pause preservation, setting rollback, HUD background consumption, allocation-free packing.
