# T11: Rebuild StarCraft-like HUD layout

**Plan:** `./ai_artefacts/PLAN_2026_08_10_rts-interaction-ui-audio-hardening.md`  
**Depends:** T8  
**Commit outcome:** bottom HUD renders minimap frame left, single/multi selection center, fixed 3×3 context card right; top-right gear appears; packing stays allocation-free.

## Context (self-contained)

- Goal: replace text rows with stable RTS control surface before clicks land.
- This slice: layout + visuals + command model only. Pointer behavior lands T12; menus T13.
- Out of scope here: minimap projection/click, settings panel, audio.
- Assumptions: logical layout remains 1920×1080; UI textured elements use `ScenePass::ui`, never procedural overlay; selection IDs sorted ascending; primary = lowest live slot.
- Decision: `docs/ADR/019_ADR_hud_minimap_and_input_routing.md`.

## Requirements

- Replace old HUD rects with exact logical rects:
  - `TOP_BAR [0,0,1920,40]`; `GEAR [1872,8,32,32]`;
  - `BOTTOM_PANEL [0,840,1920,240]`;
  - `MINIMAP_PANEL [16,856,384,208]`; `MINIMAP_MAP [32,872,352,176]`;
  - `SELECTION_PANEL [424,856,880,208]`;
  - `COMMAND_PANEL [1328,856,576,208]`; `COMMAND_GRID [1696,856,208,208]`.
- Create in `rts/hud.rs`:
  ```rust
  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub enum CommandId { BuildHq, BuildDepot, BuildBarracks, TrainWorker, TrainSoldier, SetRally }
  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub struct CommandSlot { pub command: Option<CommandId>, pub enabled: bool }
  pub fn command_slots(world: &RtsWorld) -> [CommandSlot; 9];
  ```
- Stable row-major slots: 0 HQ/Worker depending context; 1 Depot/Soldier depending context; 2 Barracks; 8 Rally for finished producer. Exact context:
  - any selected worker, no selected non-worker unit/building → build commands 0/1/2;
  - exactly one finished HQ → Worker slot 0 + Rally slot 8;
  - exactly one finished Barracks → Soldier slot 0 + Rally slot 8;
  - otherwise disabled.
- Single selection: 128×128 portrait crop from existing worker/soldier/building atlas + full existing detail text.
- Multi selection: first 24 sorted IDs as 8×3 icons, 48×48, 8px gaps, origin `[440,872]`; if >24 render `+N`; skip stale.
- Expand `RtsFrame.ui` from props/font two groups to fixed slots 4/5/6/7/8 (worker/soldier/building/props/font). Update `frame0 ui=[…]` contract to 5 counts. Overlay remains rings only.
- Gear/grid/minimap frame props added deterministically in `xtask/src/placeholder_art.rs`; update tracked props PNG/manifest. Portraits crop existing sheets; no duplicate images.
- `pack_hud` accepts a read-only UI-page descriptor only when needed in T13; this ticket packs Gameplay only.
- Reserve UI instance capacities at `RtsFrame::new`; repeated `pack_hud` grows nothing.

## Inputs

- `rts/hud.rs` current blocks/constants.
- `rts/pack.rs::RtsFrame` groups/reservations.
- existing texture slots ADR 014.
- `xtask/src/placeholder_art.rs` deterministic props generation.
- **From Depends:** T8 fixed logical canvas. No runtime viewport scaling inside HUD.

## TDD

1. **Red** — exact non-overlap layout, single/multi/overflow, command context, group slots, allocation tests.
2. **Green** — pack new regions/icons/cards/gear; expand UI groups.
3. **Refactor** — one `HudLayout` constant source shared later by hit testing; remove old selection/production/build rects.

## Test plan

| Test | Input | Expect |
| --- | --- | --- |
| `hud_regions_cover_bottom_without_overlap` | rect constants | inside panel; disjoint active regions |
| `single_selection_draws_portrait_and_full_details` | each kind | correct texture slot/UV + text |
| `multi_selection_draws_first_24_sorted_icons` | 30 shuffled IDs | sorted first24 + `+6` |
| `worker_card_uses_stable_three_build_slots` | worker selection | slots 0/1/2 enabled |
| `producer_cards_show_train_and_rally` | HQ/Barracks | exact 0/8 commands |
| `mixed_or_empty_selection_disables_card` | mixed/none | all disabled |
| `rts_frame_ui_groups_are_texture_slots_4_through_8` | frame | 5 ordered groups |
| `new_hud_pack_allocates_nothing` | repeated pack | capacities unchanged |

## Impl steps

- [ ] 1. Replace test expectations first; add exact layout/card/multi tests.
- [ ] 2. Add gear/minimap/grid props to generator; regenerate atlas + manifest.
- [ ] 3. Expand `RtsFrame.ui` groups/reservations and stdout count formatting/tests.
- [ ] 4. Add `HudLayout`, `CommandId`, `CommandSlot`, `command_slots`.
- [ ] 5. Implement left/center/right packing + gear.
- [ ] 6. Add single portrait and multi 8×3 icon packing.
- [ ] 7. Add allocation + render layer regressions.

## Outputs

- Modified: placeholder art/assets/hud/pack/mod + HUD/pack/GPU/allocation/CLI tests.
- Public API: command types + `HudLayout`/`command_slots`.
- Behavior: new visual HUD/control card; no pointer action yet.
- Assets: deterministic props atlas hash changes; `xtask atlases --check` remains source of truth.

## Validation

- [ ] `cargo run -p xtask -- atlases --check`
- [ ] `cargo test -p mmd-engine --locked --test rts_hud`
- [ ] `cargo test -p mmd-engine --locked --test rts_pack`
- [ ] `cargo test -p mmd-engine --locked --test frame_allocations new_hud_pack_allocates_nothing`
- [ ] `cargo test -p mmd-engine --locked --test gpu_smoke the_hud_`
- [ ] manual check: single/multi selections show expected center panel; worker/HQ/Barracks card changes
- [ ] commit msg draft: `feat(rts): replace bottom HUD with RTS control regions`
