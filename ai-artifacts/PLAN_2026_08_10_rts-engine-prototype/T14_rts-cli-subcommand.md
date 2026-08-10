# T14: `rts` CLI subcommand

**Plan:** `./ai-artifacts/PLAN_2026_08_10_rts-engine-prototype.md`
**Depends:** T13
**Commit outcome:** `cargo run -- rts` opens the RTS scene, drives it with mouse and keyboard, and runs headlessly from a scripted input string with a parseable stdout contract.

## Context (self-contained)

- Goal: phase 1 is a thin vertical slice of an RTS engine prototype (camera,
  selection, workers, economy, building, unit production) on a horde-free scene.
- This slice: the app layer. Everything below it exists and is tested; this
  wires SDL events to world calls and gives the whole thing a headless,
  deterministic driver so T15 can assert an end-to-end economy loop.
- Out of scope here: **`run` and `bench` are not touched.** The phase-0 `run`
  subcommand, its 24 CLI-contract cases, `src/run.rs`, `src/input.rs`,
  `src/overlay.rs` and `src/bench.rs` all stay exactly as they are. This ticket
  adds parallel files.
- Assumptions in force: the phase-0 `InputAction` / `BoundKey` contract is
  frozen (`input_actions_are_stable`); RTS input is a separate enum.

## Requirements

- A new `rts` clap subcommand with `--scenario`, `--frames`, `--inject-input`.
- A keyboard + mouse binding table with the same three-column discipline
  `src/input.rs` uses, so the live path and the scripted path cannot drift.
- A script grammar that can express clicks, drags and held keys.
- A stdout contract whose two machine-readable lines are strict `key=value`.
- The same exit codes `run` uses.

## Inputs

- **Files to read**
  - `src/main.rs` — the clap `Commands` enum.
  - `src/run.rs` — the whole file. It is the template: `RunError`,
    `EXIT_ERROR`, `EXIT_NO_GPU`, `resolve_frames`, `InputScript`, `RunState`,
    `step_frame`, `release_window`, `finish`, `workspace_root_or_cwd`, and the
    window claim/release ordering. **Copy the structure; do not edit the file.**
  - `src/input.rs` — the `BINDINGS` table pattern and its four unit tests.
  - `src/overlay.rs` — `format_overlay`.
  - `tests/cli_contract.rs` — the assertion style.
- **From Depends (T6–T13) — spell out, the worker cannot read them:**
  - `mmd_engine::rts::RtsWorld`:
    `load(path) -> Result<Self, RtsWorldError>`, `tick()`, `state_hash()`,
    `tick_index()`, `scenario()`, `entities()`, `resources() -> Resources { crystal, gas }`,
    `supply() -> Supply` (`used()`, `cap()`, `free()`), `selection() -> &Selection`
    (`len()`, `ids()`, `primary()`), `start_hq()`, `nav()`, `iso_view()`,
    `camera()`, `camera_mut()`, `set_pan_dir([f32; 2])`, `pan_dir()`,
    `click_select(&IsoView, [f32; 2]) -> Pick`,
    `shift_click_select(&IsoView, [f32; 2]) -> Pick`,
    `box_select_into_selection(&IsoView, [f32; 2], [f32; 2]) -> usize`,
    `order_move(id, Cell) -> bool`, `order_move_group(&[EntityId], Cell) -> usize`,
    `order_gather(id, node) -> bool`, `order_gather_group(&[EntityId], node) -> usize`,
    `order_build(id, site) -> bool`,
    `placement() -> Placement`, `begin_placement(BuildingKind) -> bool`,
    `cancel_placement()`, `confirm_placement(Cell, EntityId) -> Result<EntityId, PlacementError>`,
    `cancel_construction(site) -> bool`, `is_site(id) -> bool`,
    `enqueue_unit(building, UnitKind) -> Result<(), ProduceError>`,
    `cancel_queued(building, usize) -> bool`, `production_queue(id)`,
    `rally(id) -> Option<Cell>`, `set_rally(id, Option<Cell>) -> bool`.
  - `mmd_engine::rts::{Pick::{Unit, Building, Node, Nothing}, Placement::{None, Pending}, BuildingKind::{Hq, Depot, Barracks}, UnitKind::{Worker, Soldier}, EntityKind, EntityId, is_drag, normalise_rect, ghost_min_corner, DRAG_MIN_PX}`.
  - `mmd_engine::rts::{RtsFrame, DragBox, pack_frame(world, cursor, drag, frame), pack_hud(world, frame), BUILD_MENU}`.
    `RtsFrame::{new, clear, scene, instance_count}`; `RtsFrame` exposes public
    `world: Vec<DrawGroup>`, `overlay: Vec<SpriteInstance>`, `ui: Vec<DrawGroup>`.
    `BUILD_MENU: [(u8, BuildingKind); 3] == [(b'Q', Hq), (b'W', Depot), (b'E', Barracks)]`.
  - `mmd_engine::render::{SpriteRenderer, ScenePass, RenderError, VIEW_WIDTH, VIEW_HEIGHT, Camera, edge_pan_dir, screen_dir_to_cells}`.
    `SpriteRenderer::{new(root, debug), set_depth_params(scale, bias), backend(), ctx, draw_offscreen_scene(scene), draw_to_swapchain_scene(window, scene)}`.
  - `IsoView::{project, unproject, cell_at(sx, sy, width, height) -> Option<Cell>, depth_scale, depth_bias, view_size, tile_w, tile_h}`.
  - The tracked scene is `assets/scenarios/rts_prototype_v1.ron`, 320 × 320.
- **Facts you must not rediscover** — from `src/run.rs`:
  - `pub const EXIT_ERROR: u8 = 1;` `pub const EXIT_NO_GPU: u8 = 3;`
    clap usage errors exit `2`.
  - The window must be released from the device **before** it is dropped
    (`ctx.release_window(&w)`), on every path including error paths, or the
    device is left with a dangling swapchain (a real SIGSEGV, fixed in T31).
  - The event pump must be acquired **before** the window is claimed, so no `?`
    sits between a claim and its release.
  - The frame budget is checked at the **top** of the loop, because frame 1 is
    already rendered before the loop is entered.
  - `MMD_RUN_FRAMES` / `MMD_RUN_ONCE` govern `run`; this ticket uses
    **`MMD_RTS_FRAMES` / `MMD_RTS_ONCE`** so the two commands cannot be
    accidentally cross-configured.

## Exact design — no decisions left

### `src/rts_input.rs`

```rust
/// RTS commands, produced by both the live SDL path and the script path.
///
/// Deliberately a separate enum from `mmd_engine::runtime::InputAction`: that
/// one is pinned by `input_actions_are_stable` and belongs to the phase-0 horde
/// viewer. Extending it would put RTS discriminants into a contract that has
/// nothing to do with them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RtsCommand {
    Quit,
    TogglePause,
    ToggleOverlay,
    CancelPlacement,
    /// Open a build ghost.
    Build(BuildingKind),
    /// Queue a unit at the primary selected building.
    Produce(UnitKind),
    /// Set the primary selected building's rally point to the cursor's cell.
    SetRally,
    /// Begin holding a pan direction. Components in `-1..=1`, screen space.
    PanStart([f32; 2]),
    /// Stop holding it.
    PanStop([f32; 2]),
    /// Pointer moved to a screen position.
    Move([f32; 2]),
    /// Plain left click / release at a screen position.
    LeftClick([f32; 2]),
    /// Additive (shift) left click.
    ShiftClick([f32; 2]),
    /// Left drag from a to b.
    Drag([f32; 2], [f32; 2]),
    /// Right click — the context order.
    RightClick([f32; 2]),
}

/// Bound key, its SDL keycode, the `--inject-input` name, and the command.
///
/// One table so the three views of a binding cannot drift — the same discipline
/// `src/input.rs` uses for the horde viewer.
const KEY_BINDINGS: &[(Keycode, &str, RtsCommand)] = &[
    (Keycode::Escape, "esc",   RtsCommand::Quit),
    (Keycode::Space,  "space", RtsCommand::TogglePause),
    (Keycode::F1,     "f1",    RtsCommand::ToggleOverlay),
    (Keycode::X,      "x",     RtsCommand::CancelPlacement),
    (Keycode::Q,      "q",     RtsCommand::Build(BuildingKind::Hq)),
    (Keycode::W,      "w",     RtsCommand::Build(BuildingKind::Depot)),
    (Keycode::E,      "e",     RtsCommand::Build(BuildingKind::Barracks)),
    (Keycode::A,      "a",     RtsCommand::Produce(UnitKind::Worker)),
    (Keycode::S,      "s",     RtsCommand::Produce(UnitKind::Soldier)),
    (Keycode::R,      "r",     RtsCommand::SetRally),
];

/// Held pan keys. **Arrow keys only** — `W`, `A`, `S` and `E` are already build
/// and production hotkeys, and a WASD pan would silently shadow half the build
/// menu.
const PAN_BINDINGS: &[(Keycode, &str, [f32; 2])] = &[
    (Keycode::Left,  "left",  [-1.0,  0.0]),
    (Keycode::Right, "right", [ 1.0,  0.0]),
    (Keycode::Up,    "up",    [ 0.0, -1.0]),
    (Keycode::Down,  "down",  [ 0.0,  1.0]),
];

pub fn command_from_keycode(key: Keycode) -> Option<RtsCommand>;
pub fn command_from_name(name: &str) -> Option<RtsCommand>;
pub fn pan_from_keycode(key: Keycode) -> Option<[f32; 2]>;
pub fn pan_from_name(name: &str) -> Option<[f32; 2]>;
pub fn key_names() -> String;
```

### `src/rts_script.rs`

```rust
/// Scripted input for runs with no keyboard or mouse.
///
/// Grammar: `FRAME:KIND[:ARGS]` entries separated by **`;`**, because
/// coordinates already use `,`. Frames are 1-based and name the frame the event
/// lands *before*, matching `--inject-input` on the `run` command.
///
/// | KIND      | ARGS            | meaning |
/// | --------- | --------------- | ------- |
/// | `key`     | `<name>`        | one press of a bound key |
/// | `pan`     | `<name>`        | press a pan key (hold until `panup`) |
/// | `panup`   | `<name>`        | release a pan key |
/// | `move`    | `X,Y`           | move the pointer |
/// | `lclick`  | `X,Y`           | left click |
/// | `sclick`  | `X,Y`           | additive (shift) left click |
/// | `rclick`  | `X,Y`           | right click |
/// | `drag`    | `X0,Y0,X1,Y1`   | left drag |
///
/// Example:
/// `10:move:960,540;12:lclick:960,540;60:key:w;90:lclick:1000,600;200:key:esc`
#[derive(Debug, Default)]
pub struct RtsScript { /* private */ }

impl RtsScript {
    /// Every rejection names the offending entry — a script is typed by hand and
    /// a silent misparse would make the run prove nothing.
    pub fn parse(spec: &str) -> Result<Self, String>;
    /// Commands scheduled for `frame`, in script order, appended to `out`.
    /// Returns `true` when one of them was `Quit`; entries queued behind a
    /// `Quit` on the same frame stay unfired.
    pub fn drain_frame(&mut self, frame: u64, out: &mut Vec<RtsCommand>) -> bool;
    /// Entries the run never reached, in script order.
    pub fn unfired(&self) -> Vec<String>;
}
```

Coordinates parse as `f32` and are rejected when non-finite or negative.
Frame `0` is rejected with the same "frames are 1-based" message `run` uses.

### `src/rts_run.rs`

```rust
#[derive(Debug, Clone, Default)]
pub struct RtsOptions {
    pub scenario: Option<PathBuf>,
    pub frames: Option<u64>,
    pub inject_input: Option<String>,
}

pub fn run(opts: RtsOptions) -> Result<(), crate::run::RunError>;
```

Reuses `crate::run::{RunError, EXIT_ERROR, EXIT_NO_GPU}` — one exit-code
contract for the whole binary.

**Command application**, in one function so the live and scripted paths are
identical:

```rust
/// Per-frame interactive state the world does not own.
struct RtsSession {
    cursor: [f32; 2],
    /// Left button pressed at this position, if it is down.
    press: Option<[f32; 2]>,
    drag: Option<DragBox>,
    /// Currently held pan directions, summed and clamped per axis.
    pan_held: [f32; 2],
    paused: bool,
    overlay_visible: bool,
    quit: bool,
    /// Reused scratch for `order_*_group`.
    group: Vec<EntityId>,
}

fn apply(world: &mut RtsWorld, session: &mut RtsSession, cmd: RtsCommand);
```

`apply`'s exact semantics:

- `Quit` → `session.quit = true`.
- `TogglePause` / `ToggleOverlay` → flip the flag.
- `CancelPlacement` → `world.cancel_placement()`.
- `Build(kind)` → `world.begin_placement(kind)`; the return value is ignored
  (an unaffordable build simply leaves no ghost, which the HUD already shows).
- `Produce(unit)` → resolve `world.selection().primary()`; if it is a building,
  `world.enqueue_unit(id, unit)`; the `Err` is dropped (the HUD reports it).
- `SetRally` → primary selection, if a building:
  `world.set_rally(id, world.iso_view().cell_at(cursor.x, cursor.y, w, h))`.
- `PanStart(d)` / `PanStop(d)` → add / subtract into `session.pan_held`, then
  clamp each axis to `-1.0..=1.0`, then `world.set_pan_dir(pan_held)`.
- `Move(p)` → `session.cursor = p`; if `session.press` is `Some(a)` and
  `is_drag(a, p)`, set `session.drag = Some(DragBox { a, b: p })`.
  Then edge pan: `world.set_pan_dir(clamp_sum(pan_held, edge_pan_dir(p, view_size)))`.
- `LeftClick(p)` → if a ghost is pending:
  `cell_at(p)` → `ghost_min_corner(cell, kind.footprint_cells())` →
  pick a builder (`selection().ids()` first live player Worker; else the world's
  first live player Worker) → `confirm_placement(min, builder)`, ignoring the
  `Err`. Otherwise `world.click_select(&view, p)`.
- `ShiftClick(p)` → `world.shift_click_select(&view, p)` (a shift-click never
  confirms a placement).
- `Drag(a, b)` → `world.box_select_into_selection(&view, a, b)`.
- `RightClick(p)` — the **context order**, resolved in this order:
  1. `pick_at(world, &view, p)` is `Pick::Node(n)` → `order_gather_group(sel, n)`.
  2. `Pick::Building(b)` and `world.is_site(b)` → `order_build` for each selected
     worker.
  3. `Pick::Building(b)` and not a site → `order_move_group(sel, approach cell)`.
  4. otherwise → `cell_at(p)` → `order_move_group(sel, cell)`.
  A right click with an empty selection is a no-op. A right click while a ghost
  is pending **cancels the ghost** instead, and issues no order.

The live SDL loop maps `Event::MouseButtonDown/Up { mouse_btn, x, y }`,
`Event::MouseMotion { x, y }`, `Event::KeyDown/KeyUp { keycode }` into the same
`RtsCommand`s. Shift state comes from `pump.keyboard_state()` at button-up time.
A left button-up produces `Drag(a, b)` when `is_drag(a, b)`, else `LeftClick(b)`.

### stdout contract

```text
rts: backend=<b> adapter=<a> view=<w>x<h> scenario=<path> (engine <v>)
rts: frame0 tick=<t> hash=<64 hex> world=[<n>,<n>,<n>] overlay=<n> ui=[<n>,<n>]   (a)
rts: offscreen draw ok (backend=<b>)                                              (a)
rts: window <w>x<h> claimed; Esc quit, Space pause, F1 overlay, X cancel, \
     Q/W/E build, A/S produce, R rally, arrows pan                                (b)
<one HUD line per frame while the overlay is on>                                  (c)
rts: released window                                                              (b)
rts: clean exit mode=<offscreen|window> backend=<b> tick=<t> frames=<n> \
     hash=<64 hex> quit=<bool> paused=<bool> crystal=<n> gas=<n> \
     supply=<used>/<cap> units=<n> buildings=<n> nodes=<n> selected=<n>
```

- (a) absent when a scripted quit lands on frame 1.
- (b) printed whenever a window was claimed.
- (c) from a new `src/rts_overlay.rs::format_rts_overlay(world, frames) -> String`,
  a single `key=value` line: `rts: hud tick=<t> crystal=<n> gas=<n> supply=<u>/<c> sel=<n> ghost=<none|hq|depot|barracks>`.

The `frame0` and `clean exit` lines are strictly `key=value` separated by single
spaces, with no spaces inside a value — the same rule `run` follows.

`finish` performs the same self-check `run` does, adapted: every unpaused frame
owes exactly one tick, and every scripted entry must have fired. Failing either
returns `RunError::Failed` instead of printing `clean exit`.

### `src/main.rs`

```rust
/// Run the phase-1 RTS engine prototype scene.
Rts {
    /// Scenario path (default: assets/scenarios/rts_prototype_v1.ron)
    #[arg(long)]
    scenario: Option<PathBuf>,
    /// Auto-exit after N frames (CI/smoke). Omit for interactive.
    #[arg(long)]
    frames: Option<u64>,
    /// Scripted input for runs with no keyboard or mouse:
    /// `FRAME:KIND[:ARGS]` entries separated by `;`.
    #[arg(long, value_name = "FRAME:KIND[:ARGS];...")]
    inject_input: Option<String>,
}
```

plus `mod rts_input; mod rts_overlay; mod rts_run; mod rts_script;`.

## TDD

1. **Red** — unit tests in `src/rts_input.rs` and `src/rts_script.rs`, plus a new
   `tests/rts_cli_contract.rs`. Watch them fail.
2. **Green** — implement.
3. **Refactor** — none expected. Keep green.

## Test plan

`tests/rts_cli_contract.rs` follows `tests/cli_contract.rs`: run the binary with
`SDL_VIDEODRIVER=offscreen`, parse stdout, assert exit codes.

| Test | Input | Expect |
| ---- | ----- | ------ |
| `keyboard_and_script_agree` (unit) | every `KEY_BINDINGS` row | `command_from_keycode(kc) == command_from_name(name)` |
| `pan_keyboard_and_script_agree` (unit) | every `PAN_BINDINGS` row | same |
| `each_key_binds_one_command` (unit) | the table | no two rows share a command |
| `no_key_is_both_a_command_and_a_pan` (unit) | both tables | keycode sets are disjoint; name sets are disjoint |
| `the_build_menu_matches_the_bindings` (unit) | `rts::BUILD_MENU` vs `KEY_BINDINGS` | for each `(letter, kind)`, `command_from_name(&letter.to_ascii_lowercase())` is `Build(kind)` |
| `unbound_keys_are_rejected` (unit) | `Keycode::Z`, `"f9"`, `""` | `None` |
| `script_parses_every_kind` (unit) | one entry of each of the 8 kinds | `Ok`, 8 entries |
| `script_rejects_frame_zero` (unit) | `0:key:esc` | `Err` naming the entry |
| `script_rejects_an_unknown_kind` (unit) | `1:jump:1,2` | `Err` naming `jump` |
| `script_rejects_an_unknown_key` (unit) | `1:key:f9` | `Err` listing valid names |
| `script_rejects_bad_coordinates` (unit) | `1:lclick:a,2`, `1:lclick:-1,2`, `1:lclick:1` | `Err` for each |
| `script_rejects_an_empty_entry` (unit) | `1:key:esc;;2:key:esc` | `Err` |
| `quit_stops_the_frames_sweep` (unit) | `3:key:esc;3:key:space` | `drain_frame(3)` returns `true`, emits only `Quit`, and `space` stays unfired |
| `rts_runs_headless_and_exits_clean` | `rts --frames 30` | exit 0, one `clean exit` line, `mode=offscreen` |
| `frame0_line_reports_three_world_groups` | `rts --frames 1` | `world=[` has exactly 3 comma-separated numbers, `ui=[` exactly 2 |
| `frame0_hash_is_sixty_four_hex` | `rts --frames 1` | matches `^[0-9a-f]{64}$` |
| `the_run_is_deterministic` | the same command twice | identical `clean exit` `hash=` |
| `frames_zero_is_rejected` | `rts --frames 0` | exit 1, message names `--frames` |
| `env_frame_budget_is_honoured` | `MMD_RTS_FRAMES=5 rts` | `frames=5` |
| `a_malformed_env_budget_is_rejected` | `MMD_RTS_FRAMES=abc rts` | exit 1, message names `MMD_RTS_FRAMES` |
| `run_and_rts_budgets_are_independent` | `MMD_RUN_FRAMES=7 rts --frames 3` | `frames=3`, and no complaint about `MMD_RUN_FRAMES` |
| `a_missing_scenario_is_actionable` | `rts --scenario /nope.ron` | exit 1, message names the path |
| `a_phase0_scenario_is_refused` | `rts --scenario assets/scenarios/technical_prototype_v1.ron` | exit 1, message says the scene carries no rts block |
| `pause_freezes_the_tick` | `rts --frames 20 --inject-input 2:key:space` | `tick=1`, `frames=20`, `paused=true` |
| `the_overlay_prints_one_line_per_frame` | `--frames 10 --inject-input 1:key:f1` | exactly 10 `rts: hud ` lines |
| `the_overlay_is_off_by_default` | `--frames 10` | zero `rts: hud ` lines |
| `a_quit_on_frame_one_renders_nothing` | `--inject-input 1:key:esc` | no `frame0` line, `frames=0`, `quit=true`, exit 0 |
| `an_unfired_entry_fails_the_run` | `--frames 3 --inject-input 50:key:esc` | exit 1, message lists `50:esc` |
| `a_click_selects_a_worker` | move + lclick on a worker's projected position, `--frames 20` | `selected=1` |
| `a_drag_selects_the_group` | `drag` over the six spawn cells | `selected=6` |
| `a_right_click_moves_the_selection` | select then right-click 30 cells away, `--frames 900` | at least one unit's position changed (assert via a second run with no order producing a different `hash=`) |
| `a_right_click_on_a_node_starts_gathering` | select workers, right-click a node, `--frames 3000` | `crystal=` above `300` |
| `arrow_keys_pan_the_camera` | `1:pan:right` held for 60 frames | `hash=` differs from an unpanned run |
| `pan_stops_on_key_up` | `1:pan:right;10:panup:right`, 100 frames | `hash=` equals a run that panned only 9 frames |
| `w_opens_the_depot_ghost` | `1:key:w`, `--frames 5`, overlay on | the HUD line reports `ghost=depot` |
| `x_cancels_the_ghost` | `1:key:w;3:key:x` | `ghost=none` |
| `a_left_click_places_the_ghost` | `1:key:w` then a click on clear ground, `--frames 2500` | `buildings=` rose by 1 |
| `a_right_click_cancels_the_ghost_without_ordering` | `1:key:w;3:rclick:...` | `ghost=none`, and the `hash=` equals a run that only pressed `x` |
| `a_produces_a_worker_at_the_hq` | click the HQ, `key:a`, `--frames 400` | `units=` rose by 1, `crystal=250` |
| `the_exit_line_reports_every_counter` | any clean run | the line carries `crystal=`, `gas=`, `supply=`, `units=`, `buildings=`, `nodes=`, `selected=` |
| `the_window_banner_lists_every_binding` | source assertion in a unit test | the banner string contains every `KEY_BINDINGS` name and `arrows` |
| `no_gpu_exits_with_code_three` | `VK_DRIVER_FILES=/nonexistent rts --frames 1` | exit `3` |
| `usage_error_exits_with_code_two` | `rts --nope` | exit `2` |

**Mutation verification (mandatory).** Inject, confirm red, revert, confirm green:
1. Swap two rows of `KEY_BINDINGS`'s keycode column → kills `keyboard_and_script_agree`.
2. Bind pan to `W/A/S/D` → kills `no_key_is_both_a_command_and_a_pan`.
3. Change `BUILD_MENU`'s letters in `rts::hud` → kills `the_build_menu_matches_the_bindings`.
4. Check the frame budget at the loop tail → kills `rts_runs_headless_and_exits_clean` only if a test pins `frames=N` exactly; `env_frame_budget_is_honoured` does.
5. Drop the unfired-entry check in `finish` → kills `an_unfired_entry_fails_the_run`.
6. Apply `Quit` but keep sweeping the frame → kills `quit_stops_the_frames_sweep`.
7. `RightClick` while pending issues an order instead of cancelling → kills `a_right_click_cancels_the_ghost_without_ordering`.
8. Use `MMD_RUN_FRAMES` instead of `MMD_RTS_FRAMES` → kills `run_and_rts_budgets_are_independent`.
9. Skip `ctx.release_window` on the error path → no test catches it; add `the_window_is_released_before_it_drops` asserting the `released window` line appears on both the clean and the present-failure path, and record the result.

## Impl steps

- [x] 1. Create `src/rts_input.rs` with both binding tables and the four lookups, plus its five unit tests.
- [x] 2. Create `src/rts_script.rs` with `RtsScript` and its unit tests.
- [x] 3. Create `src/rts_overlay.rs` with `format_rts_overlay`.
- [x] 4. Create `src/rts_run.rs`: `RtsOptions`, `RtsSession`, `apply`, `resolve_frames` (on `MMD_RTS_FRAMES` / `MMD_RTS_ONCE`), `step_frame`, `finish`, `run`.
- [x] 5. Copy `src/run.rs`'s window claim / event-pump / release ordering exactly; do not invent a new one.
- [x] 6. Add the `Rts` variant and the four `mod` lines to `src/main.rs`.
- [x] 7. Create `tests/rts_cli_contract.rs` and write every integration test from the table. Watch them fail.
- [x] 8. Implement until green.
- [x] 9. Run the mutation list; record kills in the commit body.
- [x] 10. Run the full validation block.

## Outputs

- **Files created**
  - `src/rts_input.rs`, `src/rts_script.rs`, `src/rts_overlay.rs`, `src/rts_run.rs`
  - `tests/rts_cli_contract.rs`
- **Files edited**
  - `src/main.rs` (four `mod` lines and one clap variant)
- **Public API added:** the `rts` subcommand and its stdout contract.
- **Behaviour change:** a new command. `run` and `bench` are byte-for-byte
  unchanged in behaviour.
- **Migration / config:** new env vars `MMD_RTS_FRAMES`, `MMD_RTS_ONCE`.

## Validation

- [x] `cargo fmt --all -- --check`
- [x] `cargo test --test rts_cli_contract` — all green
- [x] `cargo test --test cli_contract` — all 24 phase-0 cases still green
- [x] `MMD_REQUIRE_GPU=1 cargo test --workspace --locked`
- [x] `VK_DRIVER_FILES=/nonexistent cargo test --workspace --locked`
- [x] `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- [x] `nix flake check`
- [x] `cargo run -- rts --frames 600` — exit 0, one `clean exit` line
- [x] `cargo run -- run --agents 5000 --frames 300` — exit 0, exit-line `hash=` unchanged
- [x] `cargo run -- run --scenario assets/scenarios/collision_mid_v1.ron --frames 300` — exit 0
- [x] `cargo tree -e features | grep -c testkit` — `0`
- [x] app functional — no broken path from this slice
- [ ] commit msg draft: `feat(app): add the rts subcommand with scripted headless input`
