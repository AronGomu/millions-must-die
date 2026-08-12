# T12: Route HUD input and minimap camera

**Plan:** `./ai_artefacts/PLAN_2026_08_10_rts-interaction-ui-audio-hardening.md`  
**Depends:** T2, T9, T11  
**Commit outcome:** HUD owns clicks before world; selection icons, shared command card, and isometric minimap camera work; disabled/background hits never leak.

## Context (self-contained)

- Goal: make new HUD functional through same action paths as hotkeys/world input.
- This slice: minimap projection/render, hit testing, pointer ownership, icon/card/minimap actions.
- Out of scope here: pause/settings modal behavior, audio cues, fog/entity dots.
- Assumptions: minimap shows only isometric map diamond + projected camera polygon; click recenters; no drag-pan; top-right gear hit is exposed for T13.
- Decision: `docs/ADR/019_ADR_hud_minimap_and_input_routing.md`.

## Requirements

- Create `crates/mmd-engine/src/rts/minimap.rs`:
  ```rust
  pub struct MinimapProjection { pub map_rect: [f32; 4], pub width: u32, pub height: u32, pub scale: f32 }
  pub fn map_to_minimap(&self, cell: [f32; 2]) -> [f32; 2];
  pub fn minimap_to_map(&self, point: [f32; 2]) -> Option<[f32; 2]>;
  pub fn camera_polygon(&self, view: &IsoView) -> [[f32; 2]; 4];
  ```
- Raw projection: `rx=cx-cy`, `ry=(cx+cy)/2`; fit bounds `[-height,width] × [0,(width+height)/2]` into `MINIMAP_MAP` preserving ratio. Inverse rejects outside map diamond.
- Camera polygon = unproject logical canvas corners `(0,0),(1920,0),(1920,1080),(0,1080)`, then map each to minimap; pack 4 two-pixel edges. Draw no entities/resources/fog/terrain detail.
- Add:
  ```rust
  pub enum HudHit { Gear, Minimap([f32;2]), SelectionIcon(EntityId), CommandSlot(u8), Background }
  pub fn hud_hit_test(world: &RtsWorld, point: [f32;2]) -> Option<HudHit>;
  ```
  Any point in top/bottom HUD returns a hit/background; world never sees it.
- Create/extend `src/rts_ui.rs` with `PointerOwner::{None,World,Hud(HudHit)}`. Owner chosen on mouse-down; same owner receives motion/up. Release outside content cancels world gesture.
- Minimap valid click → `RtsWorld::look_at_map_point`; invalid diamond area consumed/no move.
- Selection icon click → `RtsWorld::select_only(id)`; Shift-click → `toggle_selection(id)`; stale ID consumed.
- Unify keyboard/mouse actions:
  - `RtsCommand::Execute(CommandId)`;
  - `command_from_keycode` maps Q/W/E/A/S/R to same `CommandId` shown by card;
  - one `execute_command(world,session,id)` validates/enacts.
- Enabled command click executes; disabled/empty consumes without action.
- Rally becomes pending action: `SetRally` arms; next world left-click sets cell. Do not use cursor currently over HUD.
- Extend script grammar with `hud_click x y` only if existing `left x y` cannot express owner routing; prefer existing pointer commands.

## Inputs

- T11 `HudLayout`, sorted icon positions, `CommandId`, `command_slots`.
- T9 `look_at_map_point`, current `IsoView`/frontier.
- T2 shared world context order path.
- `src/rts_input.rs`, `src/rts_run.rs`, `src/rts_script.rs`.
- **From Depends:** exact APIs above.

## TDD

1. **Red** — minimap roundtrip/polygon/hits; HUD consumption; icon/card mouse-hotkey parity; rally pending.
2. **Green** — projection + pure hit model + app pointer owner/router.
3. **Refactor** — remove direct event→world branches; one action executor per command.

## Test plan

| Test | Input | Expect |
| --- | --- | --- |
| `minimap_projection_round_trips_map_corners` | four map corners + center | <=0.01 cell error |
| `outside_diamond_is_rejected` | panel corner | None; consumed |
| `camera_polygon_projects_four_view_corners` | current camera | exact four mini points |
| `minimap_click_recentres_and_clamps` | valid edge point | camera at frontier |
| `selection_icon_click_isolates` | multi icon | one selected |
| `shift_icon_click_toggles` | selected icon | removed; others stay |
| `hud_background_never_orders_world` | bottom gap | hash/orders unchanged |
| `mouse_and_hotkey_share_command_executor` | worker/card vs Q | same placement state/outcome |
| `disabled_slot_is_consumed` | invalid context | no world click/order |
| `rally_waits_for_next_world_click` | R over HUD then world click | rally set to world click |

## Impl steps

- [x] 1. Add `rts_minimap.rs` tests + HUD hit tests. Evidence: `crates/mmd-engine/tests/rts_minimap.rs` (4 tests), `hit_*` tests added to `crates/mmd-engine/tests/rts_hud.rs` (13 tests).
- [x] 2. Implement projection/inverse/camera polygon; pack diamond + edges. Evidence: `crates/mmd-engine/src/rts/minimap.rs::MinimapProjection`; camera-polygon edge stamping wired into `hud.rs::push_minimap`.
- [x] 3. Add `HudHit` and shared layout-driven hit testing. Evidence: `crates/mmd-engine/src/rts/minimap.rs::{HudHit, hud_hit_test}`.
- [x] 4. Add pointer ownership/UI router before world input. Evidence: `src/rts_ui.rs::{PointerOwner, owner_for_point}`, wired into `src/rts_run.rs::apply` before every click/drag branch.
- [x] 5. Add selection icon world APIs + route click/Shift-click. Evidence: `RtsWorld::{select_only, toggle_selection}` in `world.rs`; routed through `rts_ui::handle_hud_click`.
- [x] 6. Replace separate key/card commands with `CommandId` executor. Evidence: `RtsCommand::Execute(CommandId)` in `src/rts_input.rs`; `src/rts_ui.rs::execute_command` is the one shared executor for hotkeys and command-grid clicks.
- [x] 7. Make rally pending-next-world-click. Evidence: `RtsSession::pending_rally`, armed by `CommandId::SetRally`, consumed by the next world `LeftClick` in `apply()`.
- [x] 8. Add CLI scripted HUD/minimap regressions. Evidence: 5 `hud_*` tests in `tests/rts_cli_contract.rs`.

## Outputs

- New: `rts/minimap.rs`, engine minimap tests; app `rts_ui.rs` if not present.
- Modified: hud/world/mod/input/run/script/CLI tests.
- Public API: minimap/hit/world selection/look-at APIs above.
- Behavior: functional minimap/icons/cards; HUD-first input.
- Migrate/config: script grammar only if required; update parser docs/tests in same commit.

## Validation

- [x] `cargo test -p mmd-engine --locked --test rts_minimap` — 4 passed.
- [x] `cargo test -p mmd-engine --locked --test rts_hud hit_` — 13 passed.
- [x] `cargo test -p millions_must_die --locked rts_input` — 7 passed.
- [x] `cargo test -p millions_must_die --locked --test rts_cli_contract hud_` — 5 passed.
- [x] `cargo check --workspace --all-targets --all-features --locked` — clean.
- [ ] manual check: minimap/card/icon clicks work; bottom-panel gaps never select/order world — left for a human (no window on this desktop, per worker constraint); logged in `ai_artefacts/manual_test_checklist.md`.
- [x] commit msg draft: `feat(rts): route minimap and command HUD before world input`
