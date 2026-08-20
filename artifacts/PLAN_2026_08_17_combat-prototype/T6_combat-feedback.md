# T6: Combat feedback

**Plan:** `./artifacts/PLAN_2026_08_17_combat-prototype.md`
**Depends:** T4, T5
**Commit outcome:** HP bars over damaged + selected entities, a brief procedural death flash, and enemy dots on the minimap — all texture-free where the overlay contract demands it.

## Context (self-contained)

- Goal: Phase 2 Combat Prototype — weapons, damage, turrets, enemy AI. Success = scripted combat run + exit tokens.
- This slice: readability. The fight exists (T3–T5); this makes it visible. Render/app-side only — `RtsWorld` state untouched except read, plus exactly one addition: the death-event buffer (`drain_death_events`), which is feedback plumbing, never hashed.
- Out of scope here: gate scene/script (T7), any new textured asset, damage numbers, corpse sprites. `sim/` frozen. `assets/scenarios/*` untouched.
- **Overlay layer contract (load-bearing):** `ScenePass::overlay` is drawn depth-off with texture slot 0 bound and is honest ONLY for texture-free procedural instances — rings (`SpriteInstance::ring`, sentinel `uv_rect.x = -1.0`) and diagonal lines (`SpriteInstance::diagonal_line`, sentinel `uv_rect.x = -2.0`; contract `line < -1.5 <= ring < 0 <= sprite`, `crates/mmd-engine/src/render/instance.rs:24-52`). Every *textured* depth-off element goes in a `ui` draw group or it samples the wrong sheet. HP bars are line instances and death flashes are rings → both legal overlay citizens; minimap dots are textured (`Prop::PanelFill`) → they live in the HUD's `ui` props group like all HUD chrome.
- Bars/flashes/dots never enter `RtsWorld::state_hash` — same discipline the grid obeys (packing is a read; `rts_pack.rs::grid_toggle_does_not_change_world_hash` is the model).
- **Conflicts vs plan brief, codebase wins (verified 2026-08-17, branch `plan/combat-prototype`):**
  1. The brief said `minimap.rs` holds a palette with player/building/node dot colors. False: `crates/mmd-engine/src/rts/minimap.rs` (299 lines) is projection + `hud_hit_test` only; the minimap is *drawn* by `push_minimap` in `crates/mmd-engine/src/rts/hud.rs:867-895`, and it draws **no entities at all** (its doc says so). There are no player/building/node dots to match — this ticket adds enemy dots only, per the Commit outcome, into `hud.rs`. `minimap.rs` is untouched.
  2. The brief placed the line-instance assembly in "`render` crate + `src/rts_run.rs`". Reality: the primitive lives in `crates/mmd-engine/src/render/instance.rs`, but all RTS frame assembly (grid, rings, drag box) lives in `crates/mmd-engine/src/rts/pack.rs` (`pack_frame_inner`); `src/rts_run.rs` only forwards options. Bars and flashes are assembled in `pack.rs`; the app wires the flash lifecycle.
  3. The original Context sentence "hidden at full HP" contradicted the settled shown-rule and this ticket's own test row. Settled rule wins: shown = **selected ∪ damaged** — a selected full-HP entity shows a full green bar.

## Decisions (made while detailing — do not reopen, do not re-derive)

- **D1 — where things pack.** Bars pack inside `pack_frame_inner` (`pack.rs`), as a new section 2c after the selection rings: overlay order = grid lines (if enabled) → selection rings → HP bars. Death flashes are appended by the app *after* `pack_frame_with_options` returns, through a new engine-side `DeathFlashes` struct (also `pack.rs`, app-owned instance) — flash lifetime is measured in rendered frames, which only the app sees. Existing pack tests that pin overlay contents are updated accordingly (step 1).
- **D2 — bar width in pixels** = `width_cells * iso.tile_w`, where `width_cells` = `RTS_UNIT_BODY_DIAMETER_CELLS` (6.0, `entity.rs:24`) for a unit and `footprint_cells()` for a building. This equals the sprite quad's own width in both cases (`RTS_SPRITE_SIZE_PX = [48, 48]` at the tracked scenes' `tile_w = 8`; `building_quad_px(edge, tw, th) = [edge*tw, edge*th*2]`, `selection.rs:191-194`) — the bar visually spans the sprite.
- **D3 — bar vertical anchor**: the bar's centreline sits `HP_BAR_RAISE_CELLS = 1.5` × `iso.tile_h` px above the entity's sprite-quad **top** (`stand_on` y: `ground.y - quad_h`), for units and buildings alike. Horizontally centred on the ground point. Thickness = `DRAG_BOX_THICKNESS_PX` (2.0, `pack.rs:122`) — the settled constant, no new one.
- **D4 — color thresholds are integer**, never a float ratio: green iff `3*hp > 2*max`, red iff `3*hp < max`, else yellow (both exact boundaries land yellow — no flicker on rounding). Arithmetic in `u64` so it can never overflow. Tints (premultiplied): backing `[0.02, 0.02, 0.02, 0.85]`, green `[0.05, 0.80, 0.10, 1.0]`, yellow `[0.85, 0.75, 0.10, 1.0]`, red `[0.85, 0.10, 0.08, 1.0]`.
- **D5 — fill anchors left**: fill length = `width * (hp as f32 / max as f32)`, from the bar's left edge. Backing first, fill second — two `diagonal_line` pushes per bar, per shown entity.
- **D6 — nodes never bar**, selected or not (`max_hp(Node) = 0`, hp is a sentinel; matched out by kind before any hp read).
- **D7 — event emission point**: `DeathEvent` is pushed at the **top of T1's `fn apply_death`** in `world.rs` (before any mutation, so `owner`/`position` read live) — one seam covers unit and building deaths from every damage source (T3 combat, T5 turret, tests). Push is guarded `len() < MAX_ENTITIES` (bounded even for a test that kills without ticking). The buffer clears as the first action of `RtsWorld::tick`.
- **D8 — drain semantics**: `drain_death_events(&mut self, out: &mut Vec<DeathEvent>)` clears `out`, then `Vec::append`s the internal buffer into it (moves all, empties the source, allocation-free when `out` is preallocated to `MAX_ENTITIES`).
- **D9 — flash lifecycle**: `DeathFlashes` holds `(DeathEvent, frames_left)` pairs, capacity `MAX_ENTITIES` (overflow drops the newest — bounded, documented). Per frame in `step_frame`: `absorb` (drain world events, 12-frame flash each) right after the tick, `pack` right after `pack_frame_with_options`, `age` (decrement + in-place `retain`) after the draw. A flash born on frame N renders frames N..N+11 and is gone at N+12. Paused frames drain nothing but still age — a flash is 12 *rendered* frames.
- **D10 — flash look**: tint `DEATH_FLASH_TINT = [0.95, 0.30, 0.10, 0.80]` (ember red-orange — not the selection green, the hitbox cyan, or the minimap enemy red); ring band `DEATH_FLASH_OUTER = 0.5`, `DEATH_FLASH_INNER = 0.5 - 1.0/12.0` (twice the selection ring's band, reads as an event). Radius: unit → `body_radius_cells()`, building → `footprint_cells() * 0.5`, sized through `ring_quad_size_px` (`runtime.rs:117-122`) — the same one derivation the selection ring uses.
- **D11 — minimap dots**: drawn in `push_minimap` (`hud.rs`) between the map-frame sprite and the camera polygon (polygon stays the top element). `MINIMAP_ENEMY_TINT = [0.90, 0.12, 0.10, 1.0]` — a red claimed by nothing on the minimap or its chrome (camera polygon `[0.95, 0.85, 0.30]` yellow, panels atlas-white, blocked text `[0.75, 0.28, 0.24]` muted). Dot = `MINIMAP_ENEMY_DOT_PX = 2.0` square of `Prop::PanelFill` (same stamp the camera polygon uses), positioned by `projection.map_to_minimap(position)` + the map rect origin, centred. Enemy **units** only (enemies own no buildings). `hud_hit_test` untouched — clicking a dot is an ordinary minimap point.
- **D12 — overlay capacity** grows from `MAX_ENTITIES + MAX_GRID_LINES` to `4 * MAX_ENTITIES + MAX_GRID_LINES` (rings ≤ `MAX_ENTITIES` + bars ≤ `2*MAX_ENTITIES` + flashes ≤ `MAX_ENTITIES` + grid), reserved once in `RtsFrame::new` — no per-frame allocation. Pins updated: `rts_pack.rs:88-91` and `frame_allocations.rs::pack_frame_allocates_nothing` (its warm-up count gains the selected HQ's bar `+2` and a measured flash `+1`).
- **D13 — test placement**: no new test file. Event-buffer tests → `crates/mmd-engine/tests/rts_combat.rs` (exists from T1/T3). Bar + flash pack tests → `crates/mmd-engine/tests/rts_pack.rs`. Minimap dot test → `crates/mmd-engine/tests/rts_hud.rs`. Allocation coverage → the rewritten `pack_frame_allocates_nothing`.
- **D14 — manual validation scene** is T2's tracked fixture via `--scenario assets/scenarios/fixtures/fixture_rts_combat_v1.ron` — the gate scene carries no enemies until T7, so a "fight" in the default scene is impossible; the fixture's ghouls march on the HQ from T3 onward.

## Requirements

- `RtsWorld` gains a preallocated, bounded (`MAX_ENTITIES`) death-event buffer + `pub fn drain_death_events(&mut self, out: &mut Vec<DeathEvent>)`; `DeathEvent { kind: EntityKind, owner: u8, center: [f32; 2] }` (`Clone, Copy, Debug, PartialEq`). Cleared every tick; pushed from T1's `apply_death`; **never hashed**.
- HP bar = two `SpriteInstance::diagonal_line`s per shown entity in `frame.overlay`, packed by `pack_frame_inner` after the rings: dark backing at full width, then proportional fill colored by D4. Geometry per D2/D3/D5. Shown = live ∧ not a node ∧ (`hp < max_hp(kind)` ∨ `selection.contains(id)`); enemies included (T4 makes one enemy selectable; damaged Ghouls bar unselected). On-screen cull via `quad_is_visible` on the bar's own AABB.
- Death flash = one `SpriteInstance::ring` per drained event for 12 rendered frames, per D9/D10, appended to `frame.overlay` by the app-owned `DeathFlashes`.
- Minimap enemy dots per D11.
- No per-frame allocation anywhere: overlay reserved at its new ceiling (D12), `DeathFlashes` buffers reserved at construction, dots land in the already-reserved props group (≤ 1200 enemies + existing chrome < its `MAX_ENTITIES` reservation).
- Determinism: `state_hash` composition untouched; both tracked scripts' `rts: clean exit` lines byte-identical to the pre-T6 build (frame0's `overlay=`/`ui=` counts also unchanged for the tracked scene: nothing is selected or damaged at tick 1 and it has no enemies).

## Inputs (inspected, line refs from current `plan/combat-prototype` — pre-T1..T5; re-anchor by symbol)

- **From T1 (lands first, verbatim):** `EntityStore::hp(slot) -> u32`, `EntityStore::set_hp(slot, hp)`, `rts::max_hp(EntityKind) -> u32`, `RtsWorld::apply_damage(&mut self, target, damage) -> DamageResult`, and `fn apply_death(&mut self, id, slot, kind)` in `world.rs` (the emission point; T1 places it directly before `pub fn tick`'s doc). Test file `crates/mmd-engine/tests/rts_combat.rs` with helpers `first_worker`, `DEPOT_CORNER = Cell { x: 180, y: 176 }`, `OVERKILL`, `build_and_finish`.
- **From T2:** `UnitKind::Ghoul` (radius 3.0, max HP 30), `OWNER_ENEMY: u8 = 1` (`mmd_engine::rts::OWNER_ENEMY`), tracked fixture `assets/scenarios/fixtures/fixture_rts_combat_v1.ron` (+`.sha256`) with pre-placed Ghouls at exactly `[20.5, 20.5]` / `[26.5, 20.5]`, testkit `FIXTURE_RTS_COMBAT_V1` + `fixture_path(name)`.
- **From T3:** `RtsWorld::kills()/losses()/first_combat_tick()` (read-only here; not used by this ticket's code, named for completeness); combat routes every kill through `apply_damage` → `apply_death` → this ticket's events.
- **From T4:** enemy click-select (a selected enemy is simply in `world.selection()` — the bar rule needs nothing else).
- **From T5 (verbatim):** `BuildingKind::Turret`, `TURRET_FOOTPRINT_CELLS: u32 = 6` (`footprint_cells()` returns it — the building-bar width case).
- `crates/mmd-engine/src/rts/pack.rs` — module doc overlay contract lines 1-15; `DRAG_BOX_THICKNESS_PX` 122; `RtsFrame` overlay field doc 148-151 + `new()` overlay reserve `MAX_ENTITIES + MAX_GRID_LINES` ~192; `DragBox` ~262; `pack_frame_inner` ~330: `sprite_size`/`store`/`iso` bindings at top, world walk moves `frame.scratch` out/back, section 2a `pack_grid`, section 2b rings (ends pushing `SpriteInstance::ring(pos, size, SELECTION_RING_INNER, SELECTION_RING_OUTER, SELECTION_TINT)`), section 3 UI starts at comment `// 3. UI, depth-off and textured`. Imports: `render::{DrawGroup, FrameUniforms, SLOT_*, ScenePass, SpriteInstance, frame_uv_rect, quad_is_visible}`, `runtime::ring_quad_size_px`, `entity::{BuildingKind, EntityKind, MAX_ENTITIES, ResourceKind, UnitKind}`, `selection::{RTS_SPRITE_SIZE_PX, building_quad_px, normalise_rect, stand_on}`.
- `crates/mmd-engine/src/render/instance.rs` — `SpriteInstance::diagonal_line(a, b, thickness_px, tint)` 88 (`pos = a`, `size = b - a`, `uv_rect = [-2.0, thickness, 0, 0]`), `::ring(pos, size, inner, outer, tint)` 76, `is_ring` 108 / `is_diagonal_line` 98, `quad_is_visible` 242, `IsoView` 252 (`tile_w`, `tile_h`, `view_size`, `project`).
- `crates/mmd-engine/src/runtime.rs` — `ring_quad_size_px(tile_w, tile_h, radius_cells) = [√2·r·tw, √2·r·th]` 117-122.
- `crates/mmd-engine/src/rts/selection.rs` — `RTS_SPRITE_SIZE_PX = [48.0, 48.0]` 27, `building_quad_px` 191, `stand_on(ground, size) = [gx - w/2, gy - h]` 271, `Selection::contains(id)` 70 (binary search), `ids()` 75, `MAX_SELECTION = MAX_ENTITIES` 19.
- `crates/mmd-engine/src/rts/entity.rs` — `RTS_UNIT_BODY_RADIUS_CELLS = 3.0` 22, `RTS_UNIT_BODY_DIAMETER_CELLS = 6.0` 24, `UnitKind::body_radius_cells` 36, `EntityStore::{slot_count 184, contains 275, slot 281, id_at 286, alive 301, kind 305, owner 310, position 315, collect_live 421}`.
- `crates/mmd-engine/src/rts/world.rs` — `struct RtsWorld` 193 (T3 appends `combat_hold … first_combat_tick` after `production:`); `from_scenario`'s `Ok(Self { … })` ~726; `tick` 1730 (`self.tick_index += 1;` is its first line; T2/T3 insert wave/AI/combat systems); `state_hash` 3292 (composition untouched by this ticket).
- `crates/mmd-engine/src/rts/hud.rs` — imports 13-20 (`use super::entity::{BuildingKind, EntityId, EntityKind, EntityStore, ResourceKind, UnitKind};`); `CAMERA_POLY_TINT`/`CAMERA_POLY_PX` 838-843; `push_minimap` 867-895 (order: `push_panel`, map-frame sprite, `let projection`, `let origin`, camera-polygon loop; its doc claims "Draws no entities" — updated in step 5).
- `crates/mmd-engine/src/rts/mod.rs` — export blocks: entity 31-34, hud 39-63, pack 70-76, world 88-92.
- `src/rts_run.rs` — `use mmd_engine::rts::{…}` 83-87; `struct Scratch { frame_buf, cmd_buf }` 734-737; `Scratch` literal 1091-1094; `step_frame` 1658+: `let frame_buf = &mut scratch.frame_buf; let cmd_buf = &mut scratch.cmd_buf;` at top, tick block 1688-1690, `pack_frame_with_options(…)` 1692-1699, `crate::rts_ui::pack_hud(world, session, frame_buf);`, `draw(frame_buf.scene())?;`; test helper `fn test_scratch()` 2442-2444. `step_frame` is the one frame body for frame0, the windowed loop (1583) and `run_offscreen` (1631) — wiring it wires every path. One frame runs at most one tick.
- Tests to update: `crates/mmd-engine/tests/rts_pack.rs` — capacity pin 88-91, `selection_rings_are_procedural` 474-491, `a_selected_building_gets_a_ring_sized_to_its_footprint` 502-517, grid-section trailing `use` 1053, `grid_capacity_covers_max_map_and_selection` comment 1163-1165; `crates/mmd-engine/tests/frame_allocations.rs` — `pack_frame_allocates_nothing` 426-466 (warm-up pin `17 + 1 + 1 + 65 + 5` at 447-451), rts import block 15-21 (has `EntityKind`, `UnitKind`, `OWNER_PLAYER` already); `crates/mmd-engine/tests/rts_hud.rs` — imports 7-21, `CURSOR` 24, helpers `scene()` 26 / `props(frame)` 47.
- Exit-line machinery: `step_frame` prints no per-frame counts; `finish` prints `rts: clean exit …` from hashed endpoints + audio counters only (`src/rts_run.rs:15-60`) — nothing this ticket touches feeds it. Root `tests/rts_acceptance.rs` runs both tracked scripts (`FRAMES = 1600`, `FOCUSED_FRAMES = 300`) and pins tokens + run-vs-run determinism.
- Merge gate: `docs/05-testing.md:95-114`.

## TDD

1. **Red** — step 0 captures the pre-T6 exit-line baselines; step 1 lands every new test and every updated pin. Red = the workspace fails to compile the new surface (`DeathEvent`, `drain_death_events`, `DeathFlashes`, `HP_BAR_*`, `MINIMAP_ENEMY_*`), and the updated pins fail until the code lands. Do not weaken a test to dodge it.
2. **Green** — steps 2-6, minimal code.
3. **Refactor** — keep green; fmt + clippy are gate, not polish.

## Test plan

Run: `cargo test -p mmd-engine --test rts_combat --test rts_pack --test rts_hud --locked` and `cargo test -p mmd-engine --test frame_allocations --locked`.

| Test (exact name) | File | Input | Expect |
| ---- | ---- | ----- | ------ |
| `death_events_drain_once` | `rts_combat.rs` | finished Depot + raw-spawned Ghoul, both killed via `apply_damage` with no tick between | drain yields exactly `[Ghoul/OWNER_ENEMY/[30.5,30.5], Depot/OWNER_PLAYER/[184.0,180.0]]` in kill order; second drain empty; an undrained kill is gone after one tick |
| `death_events_never_enter_the_state_hash` | `rts_combat.rs` | twin worlds, same kill; one drains | hashes equal |
| `bar_hidden_at_full_hp` | `rts_pack.rs` | undamaged unselected soldier | `frame.overlay` empty |
| `bar_shown_damaged_and_selected` | `rts_pack.rs` | damaged Ghoul (25/30) + selected full-HP soldier | overlay = 1 ring + 4 lines; widths `6*tile_w` and fill `48*(25/30)`; both fills green; exact pos over the soldier |
| `bar_color_thresholds` | `rts_pack.rs` | `hp_bar_fill_tint` at 70/50/20 % + both boundaries; 3 packed soldiers at 28/20/8 of 40 | green/yellow/red; boundaries yellow; packed fills in slot order green/yellow/red |
| `building_bar_spans_footprint` | `rts_pack.rs` | raw turret at `[180,180]`, hp 75/150 | 2 overlay lines; backing width `6*tile_w == building_quad_px(6,…)[0]`; fill half-width, yellow; pos above the quad |
| `flash_lasts_twelve_frames` | `rts_pack.rs` | kill a worker; absorb; 12 pack/age cycles | exactly 1 ring each of 12 frames (size `ring_quad_size_px(tw,th,3.0)`, `DEATH_FLASH_TINT`, centred on the death point); frame 12: none, `active_count() == 0` |
| `flash_radius_units_body_buildings_half_footprint` | `rts_pack.rs` | kill Ghoul + raw Depot, absorb, pack once | 2 rings sized for radius 3.0 and 4.0, in event order |
| `minimap_draws_enemy_dots` | `rts_hud.rs` | T2 fixture at tick 0 (2 pre-placed Ghouls) | exactly 2 props instances tinted `MINIMAP_ENEMY_TINT`, `PanelFill` UV, 2×2 px, at `origin + map_to_minimap(cell) - 1` each |
| `selection_rings_are_procedural` (update) | `rts_pack.rs` | 3 selected workers | overlay len 9 = 3 rings + 3 bars×2; every instance ring or line |
| `a_selected_building_gets_a_ring_sized_to_its_footprint` (update) | `rts_pack.rs` | selected HQ | overlay len 3; `overlay[0]` is the radius-6 ring |
| `frame_new_reserves_the_documented_groups` (pin update) | `rts_pack.rs` | fresh frame | overlay capacity `4*MAX_ENTITIES + MAX_GRID_LINES` |
| `pack_frame_allocates_nothing` (rewrite) | `frame_allocations.rs` | selected HQ + rally + ghost + drag + 1 live flash | warm-up count `17+1+2+1+65+5+1`; 600 pack+absorb+pack-flash iterations, 0 allocations |
| `exit_line_unchanged_offscreen` | — (validation step 7.4) | both tracked scripts, before/after diff | `rts: clean exit` lines byte-identical |

Existing suites that must stay green untouched: `rts_hud.rs` (no enemies in tracked scene → 0 dots), `gpu_smoke.rs::the_frame_renders` (17 instances, nothing selected/damaged), all grid tests in `rts_pack.rs` (no selection/damage in them → 0 bars).

## Impl steps

- [ ] 0. Baseline for byte-identity (before touching code)
  - [ ] 0.1 From the repo root, on the T5 commit (pre-T6 tree):
    ```sh
    MMD_WINDOW_HIDDEN=1 cargo run -- rts --frames 1600 --inject-input-file assets/scenarios/rts_acceptance_v1.script | grep '^rts: clean exit' > /tmp/t6_baseline_acceptance.txt
    MMD_WINDOW_HIDDEN=1 cargo run -- rts --frames 300 --inject-input-file assets/scenarios/rts_feedback_polish_v1.script | grep '^rts: clean exit' > /tmp/t6_baseline_focused.txt
    ```
    Step 7.4 diffs against these.
- [ ] 1. Red — new tests + updated pins
  - [ ] 1.1 `crates/mmd-engine/tests/rts_combat.rs` — extend the `use mmd_engine::rts::{…}` block with `DeathEvent, MAX_ENTITIES, OWNER_ENEMY, OWNER_PLAYER` (merge with what T1/T3 left; rustfmt settles order), then append at the end of the file:

    ```rust
    // --- T6: the death-event buffer ------------------------------------------

    #[test]
    fn death_events_drain_once() {
        let mut h = RtsHarness::scene().build().expect("rts scene harness");
        let w0 = first_worker(&h);
        let depot = build_and_finish(&mut h, BuildingKind::Depot, DEPOT_CORNER, w0);
        h.step_exact(1); // settle: any event this setup produced is cleared
        // Raw spawn after the last tick, so its position stays exact.
        let ghoul = h
            .world_mut()
            .entities_mut()
            .spawn(EntityKind::Unit(UnitKind::Ghoul), OWNER_ENEMY, [30.5, 30.5])
            .expect("store has room");

        assert_eq!(h.world_mut().apply_damage(ghoul, OVERKILL), DamageResult::Killed);
        assert_eq!(h.world_mut().apply_damage(depot, OVERKILL), DamageResult::Killed);

        let mut out = Vec::with_capacity(MAX_ENTITIES);
        h.world_mut().drain_death_events(&mut out);
        assert_eq!(out.len(), 2, "two kills, two events, in kill order");
        assert_eq!(
            out[0],
            DeathEvent {
                kind: EntityKind::Unit(UnitKind::Ghoul),
                owner: OWNER_ENEMY,
                center: [30.5, 30.5],
            }
        );
        assert_eq!(
            out[1],
            DeathEvent {
                kind: EntityKind::Building(BuildingKind::Depot),
                owner: OWNER_PLAYER,
                // DEPOT_CORNER (180, 176) + edge 8 / 2.
                center: [184.0, 180.0],
            }
        );
        h.world_mut().drain_death_events(&mut out);
        assert!(out.is_empty(), "a second drain must find nothing");

        // An undrained event does not survive the next tick: offscreen runs
        // that never drain cost nothing and accumulate nothing.
        assert_eq!(h.world_mut().apply_damage(w0, OVERKILL), DamageResult::Killed);
        h.step_exact(1);
        h.world_mut().drain_death_events(&mut out);
        assert!(out.is_empty(), "the tick must clear an unconsumed buffer");
    }

    #[test]
    fn death_events_never_enter_the_state_hash() {
        let mut a = RtsHarness::scene().build().expect("rts scene harness");
        let mut b = RtsHarness::scene().build().expect("rts scene harness");
        let wa = first_worker(&a);
        let wb = first_worker(&b);
        assert_eq!(a.world_mut().apply_damage(wa, OVERKILL), DamageResult::Killed);
        assert_eq!(b.world_mut().apply_damage(wb, OVERKILL), DamageResult::Killed);
        let mut out = Vec::with_capacity(MAX_ENTITIES);
        a.world_mut().drain_death_events(&mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(
            a.state_hash(),
            b.state_hash(),
            "a drained and an undrained buffer must hash identically"
        );
    }
    ```
  - [ ] 1.2 `crates/mmd-engine/tests/rts_pack.rs` — append at the end of the file (the grid section at line 1053 already set the trailing-`use` precedent):

    ```rust
    // ── T6: HP bars and death flashes ────────────────────────────────────────────

    use mmd_engine::rts::{
        DEATH_FLASH_TINT, DeathFlashes, HP_BAR_BACKING_TINT, HP_BAR_GREEN_TINT, HP_BAR_RED_TINT,
        HP_BAR_YELLOW_TINT, OWNER_ENEMY, RTS_SPRITE_SIZE_PX, hp_bar_fill_tint,
    };

    #[test]
    fn bar_hidden_at_full_hp() {
        let mut h = scene();
        // A live, visible, undamaged, unselected soldier: no ring, no bar.
        h.world_mut()
            .entities_mut()
            .spawn(
                EntityKind::Unit(UnitKind::Soldier),
                OWNER_PLAYER,
                [166.5, 170.5],
            )
            .expect("store has room");
        let mut frame = RtsFrame::new();
        pack_frame(h.world(), CURSOR, None, &mut frame);
        assert!(
            frame.overlay.is_empty(),
            "full HP and unselected must draw nothing in the overlay"
        );
    }

    #[test]
    fn bar_shown_damaged_and_selected() {
        let mut h = scene();
        let iso = h.world().iso_view();
        let soldier = h
            .world_mut()
            .entities_mut()
            .spawn(
                EntityKind::Unit(UnitKind::Soldier),
                OWNER_PLAYER,
                [162.5, 166.5],
            )
            .expect("room");
        let ghoul = h
            .world_mut()
            .entities_mut()
            .spawn(EntityKind::Unit(UnitKind::Ghoul), OWNER_ENEMY, [170.5, 166.5])
            .expect("room");
        h.world_mut().selection_mut().insert(soldier);
        // 5 damage through 0 armor: the ghoul sits at 25/30.
        let _ = h.world_mut().apply_damage(ghoul, 5);

        let mut frame = RtsFrame::new();
        pack_frame(h.world(), CURSOR, None, &mut frame);

        // 1 ring (selected soldier), then 2 bars in ascending slot order:
        // the soldier (selected, full HP) and the ghoul (damaged).
        assert_eq!(frame.overlay.len(), 5);
        assert!(frame.overlay[0].is_ring());
        let bars = &frame.overlay[1..];
        for inst in bars {
            assert!(inst.is_diagonal_line(), "a bar line is texture-free: {inst:?}");
            assert_eq!(
                inst.uv_rect[1], DRAG_BOX_THICKNESS_PX,
                "bar thickness is the drag-box constant"
            );
            assert_eq!(inst.size[1], 0.0, "a bar is screen-horizontal");
        }
        // Unit bar width: body diameter (6 cells) at tile_w px per cell.
        let width = 6.0 * iso.tile_w;
        assert_eq!(bars[0].tint, HP_BAR_BACKING_TINT);
        assert_eq!(bars[0].size[0], width);
        assert_eq!(bars[1].tint, HP_BAR_GREEN_TINT, "selected full HP fills green");
        assert_eq!(bars[1].size[0], width, "full HP fills the whole width");
        assert_eq!(bars[2].tint, HP_BAR_BACKING_TINT);
        assert_eq!(bars[2].size[0], width);
        assert_eq!(bars[3].tint, HP_BAR_GREEN_TINT, "25/30 is above 2/3");
        assert_eq!(bars[3].size[0], width * (25.0 / 30.0));
        // Placement: centred over the soldier, 1.5 cells above its sprite top.
        let ground = iso.project(162.5, 166.5);
        assert_eq!(
            bars[0].pos,
            [
                ground[0] - width * 0.5,
                ground[1] - RTS_SPRITE_SIZE_PX[1] - 1.5 * iso.tile_h
            ]
        );
    }

    #[test]
    fn bar_color_thresholds() {
        // 70 % / 50 % / 20 % of a Soldier's 40 max HP: green, yellow, red.
        assert_eq!(hp_bar_fill_tint(28, 40), HP_BAR_GREEN_TINT);
        assert_eq!(hp_bar_fill_tint(20, 40), HP_BAR_YELLOW_TINT);
        assert_eq!(hp_bar_fill_tint(8, 40), HP_BAR_RED_TINT);
        // Both exact boundaries land yellow — no flicker on a threshold.
        assert_eq!(hp_bar_fill_tint(100, 150), HP_BAR_YELLOW_TINT);
        assert_eq!(hp_bar_fill_tint(50, 150), HP_BAR_YELLOW_TINT);

        // …and through a packed frame, not only the table.
        let mut h = scene();
        for (i, hp) in [(0u32, 28u32), (1, 20), (2, 8)] {
            let id = h
                .world_mut()
                .entities_mut()
                .spawn(
                    EntityKind::Unit(UnitKind::Soldier),
                    OWNER_PLAYER,
                    [160.5 + 4.0 * i as f32, 166.5],
                )
                .expect("room");
            let slot = h.world().entities().slot(id).expect("live");
            h.world_mut().entities_mut().set_hp(slot, hp);
        }
        let mut frame = RtsFrame::new();
        pack_frame(h.world(), CURSOR, None, &mut frame);
        assert_eq!(frame.overlay.len(), 6, "three bars, two lines each");
        assert_eq!(frame.overlay[1].tint, HP_BAR_GREEN_TINT);
        assert_eq!(frame.overlay[3].tint, HP_BAR_YELLOW_TINT);
        assert_eq!(frame.overlay[5].tint, HP_BAR_RED_TINT);
    }

    #[test]
    fn building_bar_spans_footprint() {
        let mut h = scene();
        let iso = h.world().iso_view();
        // Raw-spawned finished turret; nav stamping is irrelevant to a pack.
        let turret = h
            .world_mut()
            .entities_mut()
            .spawn(
                EntityKind::Building(BuildingKind::Turret),
                OWNER_PLAYER,
                [180.0, 180.0],
            )
            .expect("room");
        let slot = h.world().entities().slot(turret).expect("live");
        // 75/150: damaged, and squarely inside the yellow band.
        h.world_mut().entities_mut().set_hp(slot, 75);

        let mut frame = RtsFrame::new();
        pack_frame(h.world(), CURSOR, None, &mut frame);
        assert_eq!(frame.overlay.len(), 2, "one bar: backing + fill");
        let quad = building_quad_px(6, iso.tile_w, iso.tile_h);
        let width = 6.0 * iso.tile_w;
        assert_eq!(
            frame.overlay[0].size[0], width,
            "the bar spans the 6-cell footprint edge"
        );
        assert_eq!(
            frame.overlay[0].size[0], quad[0],
            "footprint edge px = the building quad's own width"
        );
        assert_eq!(frame.overlay[1].size[0], width * (75.0 / 150.0));
        assert_eq!(frame.overlay[1].tint, HP_BAR_YELLOW_TINT);
        let ground = iso.project(180.0, 180.0);
        assert_eq!(
            frame.overlay[0].pos,
            [
                ground[0] - width * 0.5,
                ground[1] - quad[1] - 1.5 * iso.tile_h
            ],
            "the bar sits 1.5 cells above the building quad's top"
        );
    }

    #[test]
    fn flash_lasts_twelve_frames() {
        let mut h = scene();
        let w0 = workers(&h)[0];
        let slot = h.world().entities().slot(w0).expect("live");
        let center = h.world().entities().position(slot);
        let iso = h.world().iso_view();
        // 25 damage through 0 armor: exactly lethal for a full-HP worker.
        let _ = h.world_mut().apply_damage(w0, 25);

        let mut flashes = DeathFlashes::new();
        flashes.absorb(h.world_mut());
        assert_eq!(flashes.active_count(), 1);

        let mut frame = RtsFrame::new();
        let expect_size = ring_quad_size_px(iso.tile_w, iso.tile_h, 3.0);
        let ground = iso.project(center[0], center[1]);
        for frame_i in 0..12 {
            pack_frame(h.world(), CURSOR, None, &mut frame);
            flashes.pack(&iso, &mut frame);
            let rings: Vec<_> = frame.overlay.iter().filter(|i| i.is_ring()).collect();
            assert_eq!(rings.len(), 1, "frame {frame_i}: the flash must be present");
            assert_eq!(rings[0].size, expect_size, "radius = the unit's body radius");
            assert_eq!(rings[0].tint, DEATH_FLASH_TINT);
            assert_eq!(
                rings[0].pos,
                [
                    ground[0] - expect_size[0] * 0.5,
                    ground[1] - expect_size[1] * 0.5
                ],
                "centred on the death position"
            );
            flashes.age();
        }
        pack_frame(h.world(), CURSOR, None, &mut frame);
        flashes.pack(&iso, &mut frame);
        assert_eq!(
            frame.overlay.iter().filter(|i| i.is_ring()).count(),
            0,
            "frame 12: gone"
        );
        assert_eq!(flashes.active_count(), 0);
    }

    #[test]
    fn flash_radius_units_body_buildings_half_footprint() {
        let mut h = scene();
        let iso = h.world().iso_view();
        let ghoul = h
            .world_mut()
            .entities_mut()
            .spawn(EntityKind::Unit(UnitKind::Ghoul), OWNER_ENEMY, [170.5, 166.5])
            .expect("room");
        let depot = h
            .world_mut()
            .entities_mut()
            .spawn(
                EntityKind::Building(BuildingKind::Depot),
                OWNER_PLAYER,
                [180.0, 180.0],
            )
            .expect("room");
        let _ = h.world_mut().apply_damage(ghoul, 1_000);
        let _ = h.world_mut().apply_damage(depot, 1_000);

        let mut flashes = DeathFlashes::new();
        flashes.absorb(h.world_mut());
        let mut frame = RtsFrame::new();
        pack_frame(h.world(), CURSOR, None, &mut frame);
        flashes.pack(&iso, &mut frame);
        let rings: Vec<_> = frame.overlay.iter().filter(|i| i.is_ring()).collect();
        assert_eq!(rings.len(), 2, "two deaths, two flashes, in event order");
        assert_eq!(
            rings[0].size,
            ring_quad_size_px(iso.tile_w, iso.tile_h, 3.0),
            "unit: body radius"
        );
        assert_eq!(
            rings[1].size,
            ring_quad_size_px(iso.tile_w, iso.tile_h, 4.0),
            "Depot edge 8: half the footprint"
        );
    }
    ```
  - [ ] 1.3 `crates/mmd-engine/tests/rts_pack.rs`, `selection_rings_are_procedural` (line 474) — replace the two assertions after the `pack_frame` call (`assert_eq!(frame.overlay.len(), 3);` and the `for inst … is_ring` loop) with:

    ```rust
    // 3 rings, then 3 selected full-HP bars at 2 line instances each.
    assert_eq!(frame.overlay.len(), 9);
    assert_eq!(frame.overlay.iter().filter(|i| i.is_ring()).count(), 3);
    for inst in &frame.overlay {
        assert!(
            inst.is_ring() || inst.is_diagonal_line(),
            "a textured instance in the overlay would sample slot 0, not the \
             sheet it was packed for: {inst:?}"
        );
    }
    ```
  - [ ] 1.4 Same file, `a_selected_building_gets_a_ring_sized_to_its_footprint` (line 502) — replace `assert_eq!(frame.overlay.len(), 1);` with:

    ```rust
    assert_eq!(frame.overlay.len(), 3, "one ring, then the selected bar's two lines");
    assert!(frame.overlay[0].is_ring(), "rings pack before bars");
    ```
    (the following `frame.overlay[0].size` assertion stands unchanged).
  - [ ] 1.5 Same file, `frame_new_reserves_the_documented_groups` (line 88-91) — replace the overlay-capacity assertion with:

    ```rust
    assert_eq!(
        frame.overlay.capacity(),
        4 * MAX_ENTITIES + mmd_engine::rts::MAX_GRID_LINES,
        "grid + rings + two bar lines per entity + flashes"
    );
    ```
  - [ ] 1.6 Same file, `grid_capacity_covers_max_map_and_selection` (line 1163) — replace the stale first comment line `// MAX_GRID_LINES + MAX_ENTITIES is the overlay ceiling.` with `// MAX_GRID_LINES + 4 * MAX_ENTITIES is the overlay ceiling.`
  - [ ] 1.7 `crates/mmd-engine/tests/rts_hud.rs` — add `MINIMAP_ENEMY_DOT_PX, MINIMAP_ENEMY_TINT, Prop, prop_uv` to the `use mmd_engine::rts::{…}` block (line 12-19), then append at the end of the file:

    ```rust
    // --- T6: minimap enemy dots ---------------------------------------------------

    #[test]
    fn minimap_draws_enemy_dots() {
        let h = RtsHarness::path(mmd_engine::testkit::fixture_path(
            mmd_engine::testkit::FIXTURE_RTS_COMBAT_V1,
        ))
        .build()
        .expect("combat fixture loads hash-verified");
        let mut frame = RtsFrame::new();
        pack_frame(h.world(), CURSOR, None, &mut frame);
        pack_hud(h.world(), &mut frame);

        let projection = minimap_projection(h.world());
        let origin = [MINIMAP_MAP_RECT[0], MINIMAP_MAP_RECT[1]];
        let dots: Vec<_> = props(&frame)
            .iter()
            .filter(|i| i.tint == MINIMAP_ENEMY_TINT)
            .collect();
        assert_eq!(dots.len(), 2, "two pre-placed ghouls, two dots");
        for (dot, cell) in dots.iter().zip([[20.5_f32, 20.5], [26.5, 20.5]]) {
            let p = projection.map_to_minimap(cell);
            assert_eq!(
                dot.pos,
                [
                    origin[0] + p[0] - MINIMAP_ENEMY_DOT_PX * 0.5,
                    origin[1] + p[1] - MINIMAP_ENEMY_DOT_PX * 0.5
                ],
                "same projection as the camera polygon, centred"
            );
            assert_eq!(dot.size, [MINIMAP_ENEMY_DOT_PX, MINIMAP_ENEMY_DOT_PX]);
            assert_eq!(
                dot.uv_rect,
                prop_uv(Prop::PanelFill),
                "a dot is a tinted panel-fill stamp, like the camera polygon"
            );
        }
    }
    ```
  - [ ] 1.8 `crates/mmd-engine/tests/frame_allocations.rs` — add `DeathFlashes` to the `use mmd_engine::rts::{…}` block (line 15-21), then replace the whole body of `pack_frame_allocates_nothing` (line 426-466) so it reads:

    ```rust
    let _lock = lock_alloc_tests();
    reset_count();

    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let hq = h.world().start_hq().expect("hq");
    h.world_mut().selection_mut().insert(hq);
    assert!(h.world_mut().set_rally(hq, Some(Cell { x: 180, y: 176 })));
    assert!(h.world_mut().begin_placement(BuildingKind::Depot));
    let cursor = [960.0, 540.0];
    let drag = Some(DragBox {
        a: [10.0, 10.0],
        b: [110.0, 60.0],
    });

    // One live death flash rides the measured loop, so the flash pack and
    // the (empty) event drain are measured beside the frame pack. Spawn and
    // kill a sacrifice: the live count is back to the base scene's 17.
    let victim = h
        .world_mut()
        .entities_mut()
        .spawn(
            EntityKind::Unit(UnitKind::Soldier),
            OWNER_PLAYER,
            [170.5, 166.5],
        )
        .expect("store has room");
    let _ = h.world_mut().apply_damage(victim, 1_000);
    let mut flashes = DeathFlashes::new();
    flashes.absorb(h.world_mut());
    let iso = h.world().iso_view();

    // Warm-up outside the scope: whatever the first pack would grow, it grows
    // now. `RtsFrame::new` itself allocates — that is construction, not a
    // frame.
    let mut frame = RtsFrame::new();
    pack_frame(h.world(), cursor, drag, &mut frame);
    flashes.pack(&iso, &mut frame);
    let packed = frame.instance_count();
    assert_eq!(
        packed,
        17 + 1 + 2 + 1 + 65 + 5 + 1,
        "world, ring, the selected HQ's bar, rally, ghost, drag box, flash"
    );

    let guard = MeasureGuard::enter();
    for _ in 0..600 {
        pack_frame(h.world(), cursor, drag, &mut frame);
        flashes.absorb(h.world_mut());
        flashes.pack(&iso, &mut frame);
        std::hint::black_box(frame.instance_count());
    }
    assert_eq!(guard.allocations(), 0, "packing an RTS frame allocated");
    guard.assert_zero();
    drop(guard);

    assert_eq!(
        frame.instance_count(),
        packed,
        "re-packing an unchanged world changed the frame"
    );
    ```
    (No `flashes.age()` in the loop: the one flash must stay live so all 600 iterations exercise the flash path; `age` is `Vec::retain` in place and allocation-free by construction.)
  - [ ] 1.9 `cargo test -p mmd-engine --test rts_pack --test rts_combat --test rts_hud --locked` — expect a **compile failure** naming the missing surface (`DeathEvent`, `drain_death_events`, `DeathFlashes`, `HP_BAR_BACKING_TINT`, `hp_bar_fill_tint`, `MINIMAP_ENEMY_TINT`, …). That is red; proceed.
- [ ] 2. Death-event buffer on `RtsWorld` (`crates/mmd-engine/src/rts/world.rs` + export)
  - [ ] 2.1 Directly after T1's `DamageResult` enum (before `/// The phase-1 RTS game state.`), insert:

    ```rust
    /// One entity death, surfaced for render-side feedback (the death flash).
    ///
    /// Not world state: the buffer holding these clears at the start of every
    /// tick and never enters [`RtsWorld::state_hash`] — two worlds that agree
    /// on their entities agree on their digest whether or not anyone drained
    /// the events. `center` is the entity's position at the moment it died.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct DeathEvent {
        /// What died.
        pub kind: EntityKind,
        /// Who owned it (`OWNER_PLAYER` / `OWNER_ENEMY`).
        pub owner: u8,
        /// Where it stood, in cell space.
        pub center: [f32; 2],
    }
    ```
  - [ ] 2.2 In `struct RtsWorld`, directly after T3's `first_combat_tick: Option<u32>,` field, add:

    ```rust
    /// Deaths since the start of the current tick, for the render side to
    /// drain ([`Self::drain_death_events`]). Cleared at the top of every
    /// tick and bounded by [`MAX_ENTITIES`], so an offscreen run that never
    /// drains costs nothing and accumulates nothing.
    death_events: Vec<DeathEvent>,
    ```
  - [ ] 2.3 In `from_scenario`'s `Ok(Self { … })` literal, directly after T3's `first_combat_tick: None,`, add `death_events: Vec::with_capacity(MAX_ENTITIES),`.
  - [ ] 2.4 In `pub fn tick` (line 1730), directly after `self.tick_index += 1;`, insert:

    ```rust
    // Last tick's death events die here: drained or not, feedback never
    // outlives one tick inside the world.
    self.death_events.clear();
    ```
  - [ ] 2.5 In T1's `fn apply_death(&mut self, id: EntityId, slot: usize, kind: EntityKind)`, insert as the **first statements of the body** (before the `match kind`):

    ```rust
    // Surface the death before any routing mutates the slot. Bounded: a
    // caller that kills without ever ticking cannot grow the buffer.
    if self.death_events.len() < MAX_ENTITIES {
        self.death_events.push(DeathEvent {
            kind,
            owner: self.entities.owner(slot),
            center: self.entities.position(slot),
        });
    }
    ```
  - [ ] 2.6 Directly after `apply_death`'s closing brace, add:

    ```rust
    /// Move every death recorded since the current tick began into `out`
    /// (which is cleared first), emptying the internal buffer — a second
    /// drain in the same tick finds nothing.
    ///
    /// Allocation-free when `out` was reserved to [`MAX_ENTITIES`]; the
    /// events are feedback, not state, and are excluded from
    /// [`Self::state_hash`] by design.
    pub fn drain_death_events(&mut self, out: &mut Vec<DeathEvent>) {
        out.clear();
        out.append(&mut self.death_events);
    }
    ```
  - [ ] 2.7 `crates/mmd-engine/src/rts/mod.rs` — in the `pub use world::{…}` block (line 88-92), add `DeathEvent,` after T1's `DamageResult,`.
- [ ] 3. HP bars in the frame pack (`crates/mmd-engine/src/rts/pack.rs` + export)
  - [ ] 3.1 In pack.rs's `use super::entity::{…}` (line 17), add `RTS_UNIT_BODY_DIAMETER_CELLS` and `max_hp` (rustfmt settles order).
  - [ ] 3.2 Directly after the `GHOST_TINT` const (line ~133), insert:

    ```rust
    /// HP-bar backing tint — near-black, premultiplied, slightly translucent
    /// so the world reads through the empty part of a bar.
    pub const HP_BAR_BACKING_TINT: [f32; 4] = [0.02, 0.02, 0.02, 0.85];
    /// HP-bar fill strictly above 2/3 health.
    pub const HP_BAR_GREEN_TINT: [f32; 4] = [0.05, 0.80, 0.10, 1.0];
    /// HP-bar fill between 1/3 and 2/3 health, both boundaries included.
    pub const HP_BAR_YELLOW_TINT: [f32; 4] = [0.85, 0.75, 0.10, 1.0];
    /// HP-bar fill strictly below 1/3 health.
    pub const HP_BAR_RED_TINT: [f32; 4] = [0.85, 0.10, 0.08, 1.0];
    /// How far above the sprite quad's top edge a bar's centreline sits, in
    /// multiples of the isometric tile height (1.5 cells).
    pub const HP_BAR_RAISE_CELLS: f32 = 1.5;

    /// The fill tint for `hp` of `max` remaining health.
    ///
    /// Integer thresholds, never a float ratio: green strictly above 2/3,
    /// red strictly below 1/3, yellow between — both exact boundaries land
    /// yellow, so a threshold can never flicker on rounding. `max` is never
    /// 0 here: nodes are excluded before any bar is packed.
    pub fn hp_bar_fill_tint(hp: u32, max: u32) -> [f32; 4] {
        let (hp3, max_u) = (3 * u64::from(hp), u64::from(max));
        if hp3 > 2 * max_u {
            HP_BAR_GREEN_TINT
        } else if hp3 < max_u {
            HP_BAR_RED_TINT
        } else {
            HP_BAR_YELLOW_TINT
        }
    }
    ```
  - [ ] 3.3 In `pack_frame_inner`, between the end of section 2b (the selection-ring `for` loop's closing brace, right after the `frame.overlay.push(SpriteInstance::ring(…SELECTION_TINT,));` + `}` lines) and the `// 3. UI, depth-off and textured` comment, insert:

    ```rust
    // 2c. Overlay: HP bars — two texture-free line instances per shown
    //    entity, after the rings so a bar paints over its own ring. Shown =
    //    live, not a node, and damaged (hp < max) ∪ selected. Render-side
    //    derivation only: a bar can no more enter the world hash than a
    //    grid line can.
    let scratch = std::mem::take(&mut frame.scratch);
    for &slot in &scratch {
        let kind = store.kind(slot);
        let (width_cells, quad) = match kind {
            EntityKind::Unit(_) => (RTS_UNIT_BODY_DIAMETER_CELLS, sprite_size),
            EntityKind::Building(b) => {
                let edge = b.footprint_cells();
                (edge as f32, building_quad_px(edge, iso.tile_w, iso.tile_h))
            }
            // A node has no HP semantics: no bar, selected or not.
            EntityKind::Node(_) => continue,
        };
        let hp = store.hp(slot);
        let max = max_hp(kind);
        let selected = store
            .id_at(slot)
            .is_some_and(|id| world.selection().contains(id));
        if hp >= max && !selected {
            continue;
        }
        let p = store.position(slot);
        let ground = iso.project(p[0], p[1]);
        let width = width_cells * iso.tile_w;
        let y = ground[1] - quad[1] - HP_BAR_RAISE_CELLS * iso.tile_h;
        let left = ground[0] - width * 0.5;
        let t = DRAG_BOX_THICKNESS_PX;
        if !quad_is_visible([left, y - t * 0.5], [width, t], iso.view_size) {
            continue;
        }
        frame.overlay.push(SpriteInstance::diagonal_line(
            [left, y],
            [left + width, y],
            t,
            HP_BAR_BACKING_TINT,
        ));
        let fill = width * (hp as f32 / max as f32);
        frame.overlay.push(SpriteInstance::diagonal_line(
            [left, y],
            [left + fill, y],
            t,
            hp_bar_fill_tint(hp, max),
        ));
    }
    frame.scratch = scratch;
    ```
  - [ ] 3.4 Doc + capacity, four small edits in the same file:
    - In `RtsFrame::new`, change the overlay reserve line to `overlay: Vec::with_capacity(4 * MAX_ENTITIES + MAX_GRID_LINES),` and extend the method's doc comment with: `/// The overlay's ceiling: a full grid, a ring per entity, two bar lines per entity, and a death flash per entity.`
    - `RtsFrame.overlay` field doc (line 148-151): `/// Texture-free procedural instances: world grid lines, then selection rings.` → `/// Texture-free procedural instances: world grid lines, selection rings, HP bars, then the app-appended death-flash rings.`
    - `pack_frame_with_options` doc's last line: `/// The overlay is ordered: grid lines (if enabled), then selection rings.` → `/// The overlay is ordered: grid lines (if enabled), selection rings, then HP bars; the app appends death-flash rings after this returns.`
    - Module doc (lines 8-15): in the sentence `… is only honest for texture-free procedural instances: selection rings ([`SpriteInstance::ring`]) and world grid lines ([`SpriteInstance::diagonal_line`]).`, extend the list to `… selection rings and death flashes ([`SpriteInstance::ring`]) and world grid lines and HP bars ([`SpriteInstance::diagonal_line`]).`
  - [ ] 3.5 `crates/mmd-engine/src/rts/mod.rs` — in the `pub use pack::{…}` block (line 70-76), add `HP_BAR_BACKING_TINT, HP_BAR_GREEN_TINT, HP_BAR_RAISE_CELLS, HP_BAR_RED_TINT, HP_BAR_YELLOW_TINT, hp_bar_fill_tint` (merge sorted; rustfmt settles wrapping).
- [ ] 4. `DeathFlashes` (`crates/mmd-engine/src/rts/pack.rs` + export)
  - [ ] 4.1 In pack.rs's `use crate::render::{…}` (line 21-24), add `IsoView`; change `use super::world::RtsWorld;` (line 19) to `use super::world::{DeathEvent, RtsWorld};`.
  - [ ] 4.2 Directly after the `DragBox` struct (line ~262, before `pack_grid`), insert:

    ```rust
    /// Death-flash lifetime, in rendered frames.
    pub const DEATH_FLASH_FRAMES: u8 = 12;
    /// Death-flash ring tint — premultiplied ember red-orange. Deliberately
    /// not the selection green, the hitbox cyan or the minimap enemy red:
    /// same-colour rings meaning different things would be worse than none.
    pub const DEATH_FLASH_TINT: [f32; 4] = [0.95, 0.30, 0.10, 0.80];
    /// Death-flash outer radius in normalised quad units (`0.5` = quad edge).
    pub const DEATH_FLASH_OUTER: f32 = 0.5;
    /// Death-flash inner radius — twice the selection ring's band, so a
    /// flash reads as an event, not as a selection.
    pub const DEATH_FLASH_INNER: f32 = DEATH_FLASH_OUTER - 1.0 / 12.0;

    /// App-owned render feedback: the death flashes currently on screen.
    ///
    /// Not world state — the world only surfaces [`DeathEvent`]s, and how
    /// long a flash lingers is a property of rendered frames, which only the
    /// app counts. Both buffers are reserved once at construction and reused
    /// every frame; nothing here allocates after `new`.
    #[derive(Debug)]
    pub struct DeathFlashes {
        /// Live flashes with their remaining frame counts.
        active: Vec<(DeathEvent, u8)>,
        /// Drain buffer handed to [`RtsWorld::drain_death_events`].
        events: Vec<DeathEvent>,
    }

    impl Default for DeathFlashes {
        fn default() -> Self {
            Self::new()
        }
    }

    impl DeathFlashes {
        /// Reserve both buffers at [`MAX_ENTITIES`].
        pub fn new() -> Self {
            Self {
                active: Vec::with_capacity(MAX_ENTITIES),
                events: Vec::with_capacity(MAX_ENTITIES),
            }
        }

        /// Pull this tick's deaths out of `world` and start a
        /// [`DEATH_FLASH_FRAMES`]-frame flash for each. Bounded: with
        /// [`MAX_ENTITIES`] flashes already live, a further event is dropped
        /// rather than grown into.
        pub fn absorb(&mut self, world: &mut RtsWorld) {
            world.drain_death_events(&mut self.events);
            for &event in &self.events {
                if self.active.len() < MAX_ENTITIES {
                    self.active.push((event, DEATH_FLASH_FRAMES));
                }
            }
        }

        /// Append one procedural ring per live flash to `frame.overlay`,
        /// culled by the same visibility test every packed quad obeys.
        /// Radius: a unit's body radius, a building's half footprint edge —
        /// the same derivation the selection ring uses.
        pub fn pack(&self, iso: &IsoView, frame: &mut RtsFrame) {
            for &(event, _) in &self.active {
                let radius_cells = match event.kind {
                    EntityKind::Unit(u) => u.body_radius_cells(),
                    EntityKind::Building(b) => b.footprint_cells() as f32 * 0.5,
                    // Nodes are indestructible and never emit an event, but
                    // render code stays total rather than trusting that.
                    EntityKind::Node(_) => continue,
                };
                let size = ring_quad_size_px(iso.tile_w, iso.tile_h, radius_cells);
                let ground = iso.project(event.center[0], event.center[1]);
                let pos = [ground[0] - size[0] * 0.5, ground[1] - size[1] * 0.5];
                if !quad_is_visible(pos, size, iso.view_size) {
                    continue;
                }
                frame.overlay.push(SpriteInstance::ring(
                    pos,
                    size,
                    DEATH_FLASH_INNER,
                    DEATH_FLASH_OUTER,
                    DEATH_FLASH_TINT,
                ));
            }
        }

        /// Age every flash by one rendered frame, dropping the expired in
        /// place (`Vec::retain` compacts without allocating).
        pub fn age(&mut self) {
            for f in &mut self.active {
                f.1 -= 1;
            }
            self.active.retain(|&(_, left)| left > 0);
        }

        /// Live flash count — the observability seam tests read.
        pub fn active_count(&self) -> usize {
            self.active.len()
        }
    }
    ```
  - [ ] 4.3 `crates/mmd-engine/src/rts/mod.rs` — in the `pub use pack::{…}` block, add `DEATH_FLASH_FRAMES, DEATH_FLASH_INNER, DEATH_FLASH_OUTER, DEATH_FLASH_TINT, DeathFlashes` (merge sorted).
- [ ] 5. Minimap enemy dots (`crates/mmd-engine/src/rts/hud.rs` + export)
  - [ ] 5.1 In hud.rs's `use super::entity::{…}` (line 16), add `OWNER_ENEMY`.
  - [ ] 5.2 Directly after the `CAMERA_POLY_PX` const (line ~843), insert:

    ```rust
    /// Enemy-unit dot tint on the minimap — a red claimed by nothing else on
    /// the minimap or its chrome (the camera polygon is yellow, panels are
    /// atlas-tinted white, blocked text is a muted `[0.75, 0.28, 0.24]`).
    pub const MINIMAP_ENEMY_TINT: [f32; 4] = [0.90, 0.12, 0.10, 1.0];
    /// Enemy dot edge, in pixels — the camera polygon's stamp size.
    pub const MINIMAP_ENEMY_DOT_PX: f32 = 2.0;
    ```
  - [ ] 5.3 In `push_minimap`'s doc comment (lines 869-872), replace the four lines

    ```rust
    /// Draws no entities, resources, fog or terrain detail (`T12`'s scope): only
    /// the map diamond's chrome and a projected outline of what the camera can
    /// currently see. Clicking/dragging the minimap is the app's pointer router,
    /// not this packer — see `mmd_engine::rts::hud_hit_test`.
    ```

    with

    ```rust
    /// Draws the map diamond's chrome, one dot per live enemy unit, and a
    /// projected outline of what the camera can currently see — still no player
    /// entities, resources, fog or terrain detail. Clicking/dragging the minimap
    /// is the app's pointer router, not this packer — see
    /// `mmd_engine::rts::hud_hit_test`.
    ```

    Then, in the body, between `let origin = …;` and `let corners = …;`, insert:

    ```rust
    // Enemy units, as dots: "where is the attack coming from" at a glance.
    // Same projection as the camera polygon, drawn before it so the camera
    // frame stays the minimap's top element. Clicking a dot is nothing
    // special — the pointer router still sees an ordinary minimap point
    // (`hud_hit_test` is untouched).
    let store = world.entities();
    for slot in 0..store.slot_count() {
        if !store.alive(slot)
            || store.owner(slot) != OWNER_ENEMY
            || !matches!(store.kind(slot), EntityKind::Unit(_))
        {
            continue;
        }
        let p = projection.map_to_minimap(store.position(slot));
        props.push(SpriteInstance::new(
            [
                origin[0] + p[0] - MINIMAP_ENEMY_DOT_PX * 0.5,
                origin[1] + p[1] - MINIMAP_ENEMY_DOT_PX * 0.5,
            ],
            [MINIMAP_ENEMY_DOT_PX, MINIMAP_ENEMY_DOT_PX],
            prop_uv(Prop::PanelFill),
            MINIMAP_ENEMY_TINT,
        ));
    }
    ```
  - [ ] 5.4 `crates/mmd-engine/src/rts/mod.rs` — in the `pub use hud::{…}` block (line 39-63), add `MINIMAP_ENEMY_DOT_PX, MINIMAP_ENEMY_TINT` (sorted beside `MINIMAP_MAP_RECT`).
- [ ] 6. App wiring (`src/rts_run.rs`)
  - [ ] 6.1 In the `use mmd_engine::rts::{…}` block (line 83-87), add `DeathFlashes`.
  - [ ] 6.2 `struct Scratch` (line 734) — add a field after `cmd_buf: Vec<RtsCommand>,`:

    ```rust
    /// Death flashes currently on screen — per-run feedback state, absorbed
    /// after every tick and aged after every rendered frame.
    flashes: DeathFlashes,
    ```
    Then add `flashes: DeathFlashes::new(),` to both `Scratch` literals: the run setup (line 1091-1094) and the unit-test helper `test_scratch()` (line 2442-2444).
  - [ ] 6.3 In `step_frame` (line 1658): after `let cmd_buf = &mut scratch.cmd_buf;` add `let flashes = &mut scratch.flashes;`. Then change the tick/pack sequence

    ```rust
    if !session.ui.sim_paused() {
        world.tick();
    }

    pack_frame_with_options(
    ```
    to

    ```rust
    if !session.ui.sim_paused() {
        world.tick();
    }
    // Deaths → flashes before the pack: a kill on this tick flashes on this
    // very frame. A paused frame drains nothing (the last tick's events were
    // absorbed the frame they happened) but still ages below — a flash lives
    // exactly 12 *rendered* frames.
    flashes.absorb(world);

    pack_frame_with_options(
    ```
    then directly after the `pack_frame_with_options(…);` call's closing `);` and before `crate::rts_ui::pack_hud(world, session, frame_buf);` insert `flashes.pack(&world.iso_view(), frame_buf);`, and directly after `draw(frame_buf.scene())?;` insert `flashes.age();`.
- [ ] 7. Green + gate
  - [ ] 7.1 `cargo fmt --all`, then `cargo test -p mmd-engine --test rts_pack --test rts_combat --test rts_hud --locked` — all green (incl. the updated ring tests and pins).
  - [ ] 7.2 `cargo test -p mmd-engine --test frame_allocations --locked -- --test-threads=1` — `pack_frame_allocates_nothing` green at the new pin with 0 allocations.
  - [ ] 7.3 Full gate: `cargo fmt --all -- --check && cargo test --workspace --locked && cargo clippy --workspace --all-targets --all-features -- -D warnings`.
  - [ ] 7.4 Byte-identity vs the step-0 baselines:
    ```sh
    MMD_WINDOW_HIDDEN=1 cargo run -- rts --frames 1600 --inject-input-file assets/scenarios/rts_acceptance_v1.script | grep '^rts: clean exit' | diff /tmp/t6_baseline_acceptance.txt -
    MMD_WINDOW_HIDDEN=1 cargo run -- rts --frames 300 --inject-input-file assets/scenarios/rts_feedback_polish_v1.script | grep '^rts: clean exit' | diff /tmp/t6_baseline_focused.txt -
    ```
    Both diffs empty.
  - [ ] 7.5 Manual (D14): `cargo run -- rts --scenario assets/scenarios/fixtures/fixture_rts_combat_v1.ron` — ghouls march (T3); watch: green→yellow→red bars over damaged units/buildings, a full bar over anything selected, red-orange rings where things die, red dots crossing the minimap. `git status --porcelain -- assets/scenarios` — empty.
  - [ ] 7.6 Commit as `feat(render): hp bars, death flashes and enemy minimap dots` (sign-off required — the DCO gate walks every commit).

## Outputs

- Files touched: `crates/mmd-engine/src/rts/world.rs` (event buffer only), `crates/mmd-engine/src/rts/pack.rs` (bars + `DeathFlashes` + capacity), `crates/mmd-engine/src/rts/hud.rs` (minimap dots), `crates/mmd-engine/src/rts/mod.rs` (exports), `src/rts_run.rs` (flash wiring); tests: `crates/mmd-engine/tests/{rts_combat.rs, rts_pack.rs, rts_hud.rs, frame_allocations.rs}`. **Not** touched: `minimap.rs`, `selection.rs`, `sim/`, `assets/scenarios/*`, any hash composition.
- Public API T7 consumes: none new beyond `RtsWorld::drain_death_events(&mut self, out: &mut Vec<DeathEvent>)` + `DeathEvent { kind: EntityKind, owner: u8, center: [f32; 2] }` (T7 scripts only read exit tokens; the close doc names the drain). Also newly public (render-side, no cross-ticket consumer): `DeathFlashes` (+ `DEATH_FLASH_*`), `HP_BAR_*`, `hp_bar_fill_tint`, `MINIMAP_ENEMY_TINT`, `MINIMAP_ENEMY_DOT_PX`.

## Validation

- [ ] `cargo test -p mmd-engine --test rts_pack --test rts_combat --test rts_hud --locked` → green, incl. 8 new tests + 3 updated
- [ ] `cargo test -p mmd-engine --test frame_allocations --locked -- --test-threads=1` → `pack_frame_allocates_nothing` green, 0 allocations
- [ ] `cargo test --workspace --locked` → all green; `cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets --all-features -- -D warnings` → clean
- [ ] Step 7.4 exit-line diffs vs pre-T6 baselines → both empty (byte-identical)
- [ ] Manual fixture run (step 7.5): bars, flashes, red minimap dots visible; no tracked-asset diff
- [ ] commit msg draft: `feat(render): hp bars, death flashes and enemy minimap dots`
