# T13: HUD

**Plan:** `./ai-artifacts/PLAN_2026_08_10_rts-engine-prototype.md`
**Depends:** T3, T12
**Commit outcome:** the frame carries an on-screen HUD — resource and supply readout, selection panel, production status and build menu — drawn from the tracked font and prop sheets.

## Context (self-contained)

- Goal: phase 1 is a thin vertical slice of an RTS engine prototype (camera,
  selection, workers, economy, building, unit production) on a horde-free scene.
  A prototype you cannot read the resources of is not demonstrable, which is why
  this plan bought a bitmap-font layer.
- This slice: pure layout. It reads world state and appends instances to the
  frame's UI groups. No input, no device, no world mutation.
- Out of scope here: SDL, the CLI, any gameplay rule. Do not change any
  `RtsWorld` system.
- Assumptions in force: allocation-free. Numbers are formatted into fixed stack
  buffers, never into a `String`.

## Requirements

- One entry point that appends the whole HUD to an existing frame.
- Every number formatted without allocating.
- Layout constants named and public, so a test can assert a panel's rect rather
  than a pixel it happened to observe.
- The HUD renders identically whether or not anything is selected — it never
  panics on an empty selection or a stale id.

## Inputs

- **Files to read**
  - `crates/mmd-engine/src/render/text.rs`, `crates/mmd-engine/src/rts/pack.rs`.
  - `crates/mmd-engine/src/rts/{economy,production,build,selection,entity,world}.rs`.
- **From Depends (T3) — spell out, the worker cannot read T3:**
  ```rust
  // mmd_engine::render::text
  pub const GLYPH_W_PX: f32 = 8.0;
  pub const GLYPH_H_PX: f32 = 8.0;
  pub const GLYPH_TRACKING_PX: f32 = 0.0;
  pub const FONT_REPLACEMENT: u8 = b'?';
  pub fn glyph_uv_rect(byte: u8) -> [f32; 4];
  pub fn text_width(text: &str, scale: f32) -> f32;
  /// `pos` is the TOP-LEFT of the first glyph cell, in screen pixels.
  /// Uppercases ASCII. A space advances and pushes NO instance. Returns the
  /// advance width. Allocation-free when `out` has capacity.
  pub fn push_text(out: &mut Vec<SpriteInstance>, text: &str, pos: [f32; 2], scale: f32, tint: [f32; 4]) -> f32;
  pub fn begin_text_group(group: &mut DrawGroup);
  ```
  Glyph advance is `(GLYPH_W_PX + GLYPH_TRACKING_PX) * scale`; quad size is
  `[GLYPH_W_PX * scale, GLYPH_H_PX * scale]`. Real glyphs exist for space,
  `0`–`9`, `A`–`Z` and `. , : / - + ( ) % [ ] < > ! ?`; everything else in
  ASCII 32..=127 draws a fallback box.
- **From Depends (T12) — spell out, the worker cannot read T12:**
  ```rust
  // mmd_engine::rts
  pub enum Prop { SelectionRing=0, PlacementOk=1, PlacementBad=2, RallyFlag=3,
                  CrystalIcon=4, GasIcon=5, SupplyIcon=6, PanelFill=7 }
  pub fn prop_uv(prop: Prop) -> [f32; 4];
  pub struct RtsFrame { pub world: Vec<DrawGroup>, pub overlay: Vec<SpriteInstance>, pub ui: Vec<DrawGroup> }
  impl RtsFrame { pub fn new() -> Self; pub fn clear(&mut self); pub fn scene(&self) -> ScenePass<'_>; pub fn instance_count(&self) -> usize; }
  pub struct DragBox { pub a: [f32; 2], pub b: [f32; 2] }
  pub fn pack_frame(world: &RtsWorld, cursor: [f32; 2], drag: Option<DragBox>, frame: &mut RtsFrame);
  ```
  `RtsFrame::ui` holds **exactly two** groups: index `0` with
  `atlas_id == SLOT_RTS_PROPS (7)` and index `1` with
  `atlas_id == SLOT_UI_FONT (8)`. `pack_frame` has already filled `ui[0]` with
  the rally flags, the placement ghost and the drag box; the HUD **appends**.
  The UI layer draws last with the depth test off, so later pushes land on top.
  `Prop::PanelFill` is an opaque 32 × 32 cell — the quad to stretch for any panel.
  `RtsWorld::iso_view()` gives the current projection; `render::{VIEW_WIDTH = 1920, VIEW_HEIGHT = 1080}`.
- **From the RTS world (T6–T12) — the state the HUD reads:**
  - `RtsWorld::resources() -> Resources { crystal: u32, gas: u32 }`.
  - `RtsWorld::supply() -> Supply` with `used() -> u32`, `cap() -> u32`, `free() -> u32`.
  - `RtsWorld::selection() -> &Selection` with `len()`, `is_empty()`,
    `ids() -> &[EntityId]` ascending by slot, `primary() -> Option<EntityId>`.
  - `RtsWorld::entities() -> &EntityStore` with `slot(id)`, `kind(slot)`,
    `progress(slot)`, `progress_target(slot)`, `amount(slot)`, `carry(slot) -> Option<(ResourceKind, u32)>`.
  - `EntityKind::{Unit(UnitKind), Building(BuildingKind), Node(ResourceKind)}`,
    `UnitKind::{Worker, Soldier}`, `BuildingKind::{Hq, Depot, Barracks}`,
    `ResourceKind::{Crystal, Gas}`.
  - `RtsWorld::production_queue(id) -> Option<&ProductionQueue>` with `len()`,
    `head() -> Option<UnitKind>`, `progress() -> u32`, `entries() -> &[UnitKind]`;
    `PRODUCTION_QUEUE_CAP == 5`.
  - `rts::{produce_ticks(UnitKind), unit_cost(UnitKind), building_cost(BuildingKind), build_ticks(BuildingKind)}`;
    `WORKER_COST = (50, 0)`, `SOLDIER_COST = (50, 25)`, `HQ_COST = (400, 0)`,
    `DEPOT_COST = (100, 0)`, `BARRACKS_COST = (150, 25)`.
  - `RtsWorld::placement() -> Placement::{None, Pending { kind }}`.
  - `RtsWorld::is_site(id) -> bool`, `RtsWorld::rally(id) -> Option<Cell>`.

## Exact design — no decisions left

### `crates/mmd-engine/src/rts/hud.rs`

```rust
/// Top resource bar.
pub const TOP_BAR_RECT: [f32; 4] = [0.0, 0.0, 1920.0, 40.0];        // x, y, w, h
/// Bottom command panel.
pub const BOTTOM_PANEL_RECT: [f32; 4] = [0.0, 920.0, 1920.0, 160.0];
/// Selection block inside the bottom panel.
pub const SELECTION_RECT: [f32; 4] = [16.0, 936.0, 560.0, 128.0];
/// Production block inside the bottom panel.
pub const PRODUCTION_RECT: [f32; 4] = [608.0, 936.0, 640.0, 128.0];
/// Build-menu block inside the bottom panel.
pub const BUILD_MENU_RECT: [f32; 4] = [1280.0, 936.0, 624.0, 128.0];

/// Icon edge in the top bar.
pub const ICON_PX: f32 = 32.0;
/// Text scale in the top bar (24 px glyphs).
pub const TOP_TEXT_SCALE: f32 = 3.0;
/// Text scale in the bottom panel (16 px glyphs).
pub const PANEL_TEXT_SCALE: f32 = 2.0;
/// Line height in the bottom panel.
pub const PANEL_LINE_PX: f32 = 22.0;

/// Panel tint, premultiplied. The sheet cell already carries the alpha; this
/// keeps the tint neutral so the panel colour lives in exactly one place.
pub const PANEL_TINT: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
/// Normal HUD text.
pub const TEXT_TINT: [f32; 4] = [0.90, 0.92, 0.96, 1.0];
/// Text for something the player cannot currently afford or do.
pub const TEXT_TINT_BLOCKED: [f32; 4] = [0.75, 0.28, 0.24, 1.0];
/// Text for a hotkey letter.
pub const TEXT_TINT_HOTKEY: [f32; 4] = [0.95, 0.80, 0.25, 1.0];

/// The build menu's three rows, in display order, with their hotkey letters.
///
/// The letters are the HUD's copy of the binding, and `T14` asserts they match
/// the app's keyboard table — a menu that says `Q` while the key is `B` is worse
/// than no menu.
pub const BUILD_MENU: [(u8, BuildingKind); 3] = [
    (b'Q', BuildingKind::Hq),
    (b'W', BuildingKind::Depot),
    (b'E', BuildingKind::Barracks),
];

/// Longest decimal a `u32` needs, plus room for a `/` pair.
pub const NUM_BUF: usize = 12;

/// Format `v` into `buf` and return the written slice as a `&str`.
///
/// No allocation: the HUD runs every frame and a `String` per number would put
/// four heap allocations inside the frame path.
pub fn fmt_u32(buf: &mut [u8; NUM_BUF], v: u32) -> &str;

/// Format `a/b` into `buf`.
pub fn fmt_ratio(buf: &mut [u8; NUM_BUF], a: u32, b: u32) -> &str;

/// A one-word name for an entity kind, for the selection panel.
pub fn kind_label(kind: EntityKind) -> &'static str;   // "WORKER", "SOLDIER", "HQ", "DEPOT", "BARRACKS", "CRYSTAL", "GAS"

/// Append the whole HUD to `frame`.
///
/// Appends to `frame.ui[0]` (props: panels and icons) and `frame.ui[1]` (font).
/// Call **after** [`pack_frame`], which owns the world-space half of the UI
/// layer. Allocation-free.
pub fn pack_hud(world: &RtsWorld, frame: &mut RtsFrame);
```

`fmt_u32`: write digits backwards into the buffer, then
`std::str::from_utf8(&buf[i..]).expect("ascii digits")`. `0` writes `"0"`.

### `pack_hud` layout — exact

**A. Top bar.** One `PanelFill` quad at `TOP_BAR_RECT`. Then three columns, each
`icon` + one space + `text`, at `y = 4.0` for the icon and
`y = TOP_BAR_RECT[1] + (TOP_BAR_RECT[3] - GLYPH_H_PX * TOP_TEXT_SCALE) * 0.5`
for the text:

| column | icon x | icon | text x | text |
| ------ | ------ | ---- | ------ | ---- |
| Crystal | `16.0` | `Prop::CrystalIcon` | `56.0` | `fmt_u32(resources().crystal)` |
| Gas | `280.0` | `Prop::GasIcon` | `320.0` | `fmt_u32(resources().gas)` |
| Supply | `544.0` | `Prop::SupplyIcon` | `584.0` | `fmt_ratio(supply().used(), supply().cap())` |

Supply text is `TEXT_TINT_BLOCKED` when `supply().free() == 0`, else `TEXT_TINT`.

**B. Bottom panel.** One `PanelFill` quad at `BOTTOM_PANEL_RECT`.

**C. Selection block** at `SELECTION_RECT`, `PANEL_TEXT_SCALE`, one line per
`PANEL_LINE_PX`:

- Line 0: `"SELECTED "` + `fmt_u32(selection().len())`. When the selection is
  empty this is `"SELECTED 0"` and the block ends here.
- Line 1: `kind_label(kind_of_primary)`.
- Line 2, by primary kind:
  - `Unit(Worker)` with cargo → `"CARRYING "` + label + `" "` + amount;
    without cargo → `"CARRYING NOTHING"`.
  - `Unit(Soldier)` → `"IDLE"` (no combat exists yet; saying anything else would
    be a claim the code cannot support).
  - `Building(b)` under construction → `"BUILDING "` +
    `fmt_u32(progress * 100 / progress_target)` + `"%"`.
  - `Building(b)` finished → `"READY"`.
  - `Node(res)` → `"REMAINING "` + `fmt_u32(amount)`.
- Line 3, buildings only, when a rally point is set:
  `"RALLY "` + x + `","` + y.

**D. Production block** at `PRODUCTION_RECT`, only when the primary selection is
a finished building with a non-empty queue:

- Line 0: `"PRODUCING "` + `kind_label(Unit(head))`.
- Line 1: `fmt_u32(progress * 100 / produce_ticks(head))` + `"%"`.
- Line 2: `"QUEUE "` + `fmt_ratio(queue.len(), PRODUCTION_QUEUE_CAP)`.

**E. Build menu** at `BUILD_MENU_RECT`, always, one row per `BUILD_MENU` entry:

`"[Q] HQ 400C"` — the hotkey letter in `TEXT_TINT_HOTKEY`, the rest in
`TEXT_TINT` when `resources().covers(building_cost(kind))` and
`TEXT_TINT_BLOCKED` when it does not. Gas is appended only when the cost's gas
is nonzero: `"[E] BARRACKS 150C 25G"`.

The pending placement's row is prefixed `">"` instead of `" "` so the ghost's
identity is readable without the mouse.

Each row's text is assembled with **multiple `push_text` calls**, not with
`format!` — that is what keeps the HUD allocation-free.

### Panel quads

```rust
/// One stretched `Prop::PanelFill` quad.
fn push_panel(out: &mut Vec<SpriteInstance>, rect: [f32; 4], tint: [f32; 4]) {
    out.push(SpriteInstance::new(
        [rect[0], rect[1]], [rect[2], rect[3]], prop_uv(Prop::PanelFill), tint,
    ));
}
```

## TDD

1. **Red** — write every test below: headless in a new
   `crates/mmd-engine/tests/rts_hud.rs`, the two GPU cases appended to
   `crates/mmd-engine/tests/gpu_smoke.rs`. Watch them fail.
2. **Green** — implement.
3. **Refactor** — none expected. Keep green.

## Test plan

Helper the tests need, written once in `rts_hud.rs`:
`fn glyphs(frame: &RtsFrame) -> &[SpriteInstance] { &frame.ui[1].instances }` and
`fn text_at(frame: &RtsFrame, pos: [f32; 2], scale: f32) -> String` — walks the
font group's instances from `pos` in advance-width steps and maps each
`uv_rect` back through `glyph_uv_rect` to a byte, so a test asserts **the string
the HUD drew**, not a pixel it hoped for. Spaces read back as `' '` from the gap.

| Test | Input | Expect |
| ---- | ----- | ------ |
| `fmt_u32_covers_zero_and_the_max` | `0`, `1`, `4_294_967_295` | `"0"`, `"1"`, `"4294967295"` |
| `fmt_ratio_joins_with_a_slash` | `(6, 10)` | `"6/10"` |
| `kind_label_covers_every_kind` | all 7 | 7 distinct non-empty uppercase labels |
| `hud_appends_and_does_not_clear` | `pack_frame` then `pack_hud` | the ghost/drag instances `pack_frame` pushed are still present |
| `hud_uses_only_the_two_ui_groups` | after `pack_hud` | `frame.ui.len() == 2`, `world` and `overlay` unchanged |
| `the_top_bar_shows_the_stock` | fresh scene | `text_at([56, ..], TOP_TEXT_SCALE) == "300"`, gas column `"100"` |
| `the_top_bar_shows_supply_as_a_ratio` | fresh scene | supply column `"6/10"` |
| `supply_turns_red_at_the_cap` | fill supply to the cap | the supply glyphs carry `TEXT_TINT_BLOCKED` |
| `supply_is_normal_below_the_cap` | fresh scene | `TEXT_TINT` |
| `an_empty_selection_says_zero` | nothing selected | selection line 0 is `"SELECTED 0"`, and no line 1 exists |
| `a_selected_worker_names_itself` | select one worker | line 1 is `"WORKER"` |
| `a_carrying_worker_reports_its_cargo` | `set_carry(slot, Some((Gas, 8)))` | line 2 is `"CARRYING GAS 8"` |
| `an_empty_handed_worker_says_so` | no cargo | `"CARRYING NOTHING"` |
| `a_selected_site_reports_its_percentage` | Depot at 90 of 180 ticks | `"BUILDING 50%"` |
| `a_finished_building_says_ready` | the HQ | `"READY"` |
| `a_selected_node_reports_its_remainder` | a full crystal node | `"REMAINING 1500"` |
| `a_rally_point_is_shown` | `set_rally(hq, Some(Cell{x:200,y:210}))`, select the HQ | line 3 is `"RALLY 200,210"` |
| `no_rally_means_no_line_three` | rally cleared | line 3 absent |
| `the_production_block_is_absent_without_a_queue` | HQ selected, empty queue | no glyphs inside `PRODUCTION_RECT` |
| `the_production_block_reports_the_head` | enqueue a Worker, step 150 | `"PRODUCING WORKER"` then `"50%"` then `"QUEUE 1/5"` |
| `the_build_menu_lists_three_rows` | any state | three rows at `BUILD_MENU_RECT`, `PANEL_LINE_PX` apart |
| `the_build_menu_shows_costs` | fresh scene | `"[Q] HQ 400C"`, `"[W] DEPOT 100C"`, `"[E] BARRACKS 150C 25G"` |
| `an_unaffordable_row_is_red` | fresh scene (`300` crystal) | the HQ row's non-hotkey glyphs carry `TEXT_TINT_BLOCKED`; the Depot row's carry `TEXT_TINT` |
| `the_hotkey_letter_is_tinted_separately` | any row | the `Q`/`W`/`E` glyph carries `TEXT_TINT_HOTKEY` |
| `the_pending_row_is_marked` | `begin_placement(Depot)` | the Depot row starts `">"`, the others `" "` |
| `the_panels_are_drawn_before_the_text` | after `pack_hud` | the two `PanelFill` quads appear in `ui[0]`, and every glyph is in `ui[1]`, which draws after |
| `the_hud_never_panics_on_a_stale_primary` | select a worker, despawn it directly, `pack_hud` without ticking | no panic; the selection line reports the count it has |
| `pack_hud_allocates_nothing` | in `frame_allocations.rs`, `MeasureGuard` around 600 `pack_frame` + `pack_hud` pairs | zero allocations |
| `pack_hud_does_not_mutate_the_world` | hash before/after 100 calls | equal |
| `the_hud_renders_legible_pixels` (GPU) | pack + `draw_offscreen_readback_scene` | more than 200 pixels inside `TOP_BAR_RECT` differ from the panel colour (i.e. glyphs are visible) |
| `the_hud_draws_over_the_world` (GPU) | camera on the base so units overlap the bottom panel | every pixel sampled inside `BOTTOM_PANEL_RECT` at a non-glyph position equals the panel colour |

**Mutation verification (mandatory).** Inject, confirm red, revert, confirm green:
1. `pack_hud` calls `frame.clear()` first → kills `hud_appends_and_does_not_clear`.
2. Supply printed as `used` only → kills `the_top_bar_shows_supply_as_a_ratio`.
3. `TEXT_TINT_BLOCKED` never applied → kills `an_unaffordable_row_is_red` and `supply_turns_red_at_the_cap`.
4. Percentage computed `progress / target * 100` in integers → kills `a_selected_site_reports_its_percentage` (always `0%`).
5. Panels pushed into `ui[1]` → kills `the_panels_are_drawn_before_the_text`.
6. `BUILD_MENU` letters changed to `A/S/D` → kills `the_build_menu_shows_costs`; T14 adds the binding cross-check.
7. `fmt_u32(0)` writes an empty slice → kills `fmt_u32_covers_zero_and_the_max`.
8. Gas suffix always appended → kills `the_build_menu_shows_costs` (`"[W] DEPOT 100C 0G"`).

## Impl steps

- [ ] 1. Create `crates/mmd-engine/src/rts/hud.rs` with the constants block verbatim.
- [ ] 2. Add `mod hud;` and the re-exports to `crates/mmd-engine/src/rts/mod.rs`.
- [ ] 3. Create `crates/mmd-engine/tests/rts_hud.rs`, write the `glyphs` / `text_at` helpers, then every test from the table. Watch them fail.
- [ ] 4. Implement `fmt_u32`, `fmt_ratio`, `kind_label`, `push_panel`.
- [ ] 5. Implement `pack_hud` section A (top bar).
- [ ] 6. Implement section B (bottom panel).
- [ ] 7. Implement section C (selection block), all five primary-kind branches.
- [ ] 8. Implement section D (production block).
- [ ] 9. Implement section E (build menu) with the three tints and the pending marker.
- [ ] 10. Append the two GPU tests to `crates/mmd-engine/tests/gpu_smoke.rs`.
- [ ] 11. Add `pack_hud_allocates_nothing` to `crates/mmd-engine/tests/frame_allocations.rs`.
- [ ] 12. Run the mutation list; record kills in the commit body.
- [ ] 13. Run the full validation block.

## Outputs

- **Files created**
  - `crates/mmd-engine/src/rts/hud.rs`
  - `crates/mmd-engine/tests/rts_hud.rs`
- **Files edited**
  - `crates/mmd-engine/src/rts/mod.rs`
  - `crates/mmd-engine/tests/{gpu_smoke,frame_allocations}.rs`
- **Public API added:** `rts::{pack_hud, fmt_u32, fmt_ratio, kind_label, BUILD_MENU, NUM_BUF, TOP_BAR_RECT, BOTTOM_PANEL_RECT, SELECTION_RECT, PRODUCTION_RECT, BUILD_MENU_RECT, ICON_PX, TOP_TEXT_SCALE, PANEL_TEXT_SCALE, PANEL_LINE_PX, PANEL_TINT, TEXT_TINT, TEXT_TINT_BLOCKED, TEXT_TINT_HOTKEY}`.
- **Behaviour change:** a packed frame now carries a HUD.
- **Migration / config:** none.

## Validation

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo test -p mmd-engine --test rts_hud` — all green
- [ ] `cargo test -p mmd-engine --test rts_pack` — all green
- [ ] `cargo test -p mmd-engine --test frame_allocations` — all green
- [ ] `MMD_REQUIRE_GPU=1 cargo test --workspace --locked`
- [ ] `VK_DRIVER_FILES=/nonexistent cargo test --workspace --locked` — GPU cases skip cleanly
- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- [ ] `nix flake check`
- [ ] `git diff --stat HEAD -- lab/goldens/` — **empty**
- [ ] `cargo run -- run --agents 5000 --frames 300` — exit 0, exit-line `hash=` unchanged
- [ ] app functional — no broken path from this slice
- [ ] commit msg draft: `feat(rts): draw the on-screen resource and command HUD`
