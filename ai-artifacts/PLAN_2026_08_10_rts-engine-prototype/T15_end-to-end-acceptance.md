# T15: End-to-end acceptance

**Plan:** `./ai-artifacts/PLAN_2026_08_10_rts-engine-prototype.md`
**Depends:** T14
**Commit outcome:** one tracked script drives select → gather → build → produce end to end, asserted twice — once headlessly through the engine, once through the shipped binary.

## Context (self-contained)

- Goal: phase 1 is a thin vertical slice of an RTS engine prototype (camera,
  selection, workers, economy, building, unit production) on a horde-free scene.
  This ticket is what makes "the slice works" a falsifiable claim rather than a
  collection of unit tests that each pass alone.
- This slice: a committed input script, a file-driven flag to run it, an
  engine-level milestone test, and a CLI-level test. It adds the acceptance run
  to the merge gate.
- Out of scope here: new gameplay, new rendering, tuning any constant. If a
  milestone cannot be met, the fix is the script's timing, not the game's
  numbers — unless a genuine defect is found, in which case fix it and say so in
  the commit body.
- Assumptions in force: no performance claim. This run gates on **behaviour and
  exit code only**; it must not assert a duration.

## Requirements

- `--inject-input-file <PATH>` on the `rts` subcommand, with a newline- and
  comment-tolerant form of the existing grammar.
- A tracked acceptance script.
- An engine-level test asserting each milestone by world state.
- A CLI-level test asserting the same milestones from the exit line.
- The merge gate in `docs/05-testing.md` gains the acceptance command.

## Inputs

- **Files to read**
  - `src/rts_run.rs`, `src/rts_script.rs`, `src/rts_input.rs`, `src/main.rs`.
  - `tests/rts_cli_contract.rs`.
  - `crates/mmd-engine/src/rts/*`.
- **From Depends (T14) — spell out, the worker cannot read T14:**
  - The `rts` subcommand exists:
    `cargo run -- rts [--scenario PATH] [--frames N] [--inject-input SPEC]`,
    with env budget `MMD_RTS_FRAMES` / `MMD_RTS_ONCE`, and exit codes
    `0` clean, `1` actionable failure, `2` clap usage, `3` no GPU.
  - Script grammar: `FRAME:KIND[:ARGS]` entries separated by **`;`**, frames
    1-based, naming the frame the event lands *before*. Kinds:
    `key:<name>`, `pan:<name>`, `panup:<name>`, `move:X,Y`, `lclick:X,Y`,
    `sclick:X,Y`, `rclick:X,Y`, `drag:X0,Y0,X1,Y1`.
    Key names: `esc space f1 x q w e a s r`; pan names: `left right up down`.
  - Key semantics: `q/w/e` open the HQ / Depot / Barracks ghost, `x` cancels it,
    `a`/`s` queue a Worker / Soldier at the primary selected building, `r` sets
    that building's rally to the cursor cell, `space` pauses, `f1` toggles a
    one-line stdout HUD, `esc` quits.
  - Mouse semantics: `lclick` confirms a pending ghost (using the first selected
    live player Worker, else the world's first) and otherwise selects; `sclick`
    is an additive select and never confirms a ghost; `drag` box-selects own
    units; `rclick` is the context order (node → gather, site → build,
    finished building → move to its approach cell, ground → move) and, while a
    ghost is pending, **cancels the ghost and issues no order**.
  - The exit line is:
    ```text
    rts: clean exit mode=<offscreen|window> backend=<b> tick=<t> frames=<n> \
         hash=<64 hex> quit=<bool> paused=<bool> crystal=<n> gas=<n> \
         supply=<used>/<cap> units=<n> buildings=<n> nodes=<n> selected=<n>
    ```
    strictly `key=value`, single-spaced, no spaces inside a value.
  - A scripted entry that never fires makes the run fail with exit `1`.
- **From the world (T5–T13) — the numbers the milestones depend on:**
  - Scene `assets/scenarios/rts_prototype_v1.ron`: 320 × 320 cells,
    `cell_size_px: 4` (so `tile_w = 8.0`, `tile_h = 4.0`), `sprite_size_px: 48`.
    HQ min corner `(160, 160)` edge `12` → centre `[166.0, 166.0]`.
    Six workers at `(162..=167, 178)`. Crystal nodes
    `(140,150) (146,146) (152,142) (180,142) (186,146) (192,150) (150,190) (182,190)`;
    gas nodes `(136,168) (196,168)`. Start `crystal 300 / gas 100`,
    `start_supply_cap 10`. Obstacles follow
    `(x*7 + y*13) % 97 == 0` **excluded** within Chebyshev 28 of `(165, 165)`,
    so the square `x, y in 137..=193` is guaranteed obstacle-free.
  - Costs: Depot `100C`, Barracks `150C 25G`, Worker `50C`, Soldier `50C 25G`.
    Build ticks: Depot `180`, Barracks `300`. Produce ticks: Worker `300`,
    Soldier `360`. Supply grants: HQ `10`, Depot `10`. Supply costs: Worker `1`,
    Soldier `2`. `WORKER_CARRY_CAPACITY = 8`, `GATHER_TICKS = 60`.
  - Footprints: HQ `12`, Depot `8`, Barracks `10`.
    `ghost_min_corner(cell, edge) == cell - edge / 2` (saturating).
  - The camera starts centred on the HQ centre, so
    `iso_view().origin == [960.0, -124.0]` and
    `project(cx, cy) == [960.0 + (cx - cy) * 4.0, -124.0 + (cx + cy) * 2.0]`.
    **Re-derive this in the test rather than trusting it**; the engine-level
    test below asserts each screen coordinate against `project` before the CLI
    test consumes it.

## Exact design — no decisions left

### `--inject-input-file`

Added to the `Rts` clap variant:

```rust
/// Read the scripted input from a file instead of the command line.
///
/// The file uses the same grammar as `--inject-input`, plus: a newline is an
/// entry separator exactly like `;`, blank lines are skipped, and everything
/// from a `#` to the end of a line is a comment. Mutually exclusive with
/// `--inject-input`; giving both is an error rather than a silent precedence
/// rule.
#[arg(long, value_name = "PATH", conflicts_with = "inject_input")]
inject_input_file: Option<PathBuf>,
```

`RtsScript` gains:

```rust
/// Strip comments and normalise newlines to `;`, then [`Self::parse`].
pub fn parse_file_text(text: &str) -> Result<Self, String>;
```

### Tracked script: `assets/scenarios/rts_acceptance_v1.script`

Screen coordinates, each derived from
`project(cx, cy) = [960 + (cx - cy) * 4, -124 + (cx + cy) * 2]`:

| target | cell centre | screen |
| ------ | ----------- | ------ |
| worker box, top-left | — | `(860, 530)` |
| worker box, bottom-right | — | `(950, 600)` |
| crystal node `(140,150)` | `(140.5, 150.5)` | `(920, 458)` |
| HQ centre | `(166.0, 166.0)` | `(960, 540)` |
| Depot ghost cell `(184,180)` → min `(180,176)` | `(184.5, 180.5)` | `(976, 606)` |
| Barracks ghost cell `(151,181)` → min `(146,176)` | `(151.5, 181.5)` | `(840, 542)` |

Both footprints lie inside the obstacle-free square `137..=193` and overlap
neither the HQ (`160..=171`) nor any node.

```text
# Phase-1 acceptance: select -> gather -> build -> produce.
# Frames are 1-based. Coordinates are screen pixels at the starting camera.

1:move:960,540
5:drag:860,530,950,600         # box-select the six starting workers
10:rclick:920,458              # send them all to the crystal node

# --- Depot ---
60:lclick:896,558              # select one worker (spawn cell 162,178)
65:key:w                       # Depot ghost
70:move:976,606
75:lclick:976,606              # place it at min corner (180,176)

# --- a Worker from the HQ ---
120:lclick:960,540             # select the HQ
125:key:a                      # queue a Worker

# --- Barracks ---
400:lclick:906,563             # select a worker near the crystal run
405:key:e                      # Barracks ghost
410:move:840,542
415:lclick:840,542             # place it at min corner (146,176)

# --- a Soldier from the Barracks ---
900:lclick:840,542             # select the finished Barracks
905:key:s                      # queue a Soldier

1400:key:f1                    # overlay on for the final frames
1450:key:esc
```

If a milestone does not land, **retime the script**, do not retune the game.
The one exception: a genuine defect, which must be fixed and named in the commit
body.

### Engine-level milestone test

New `crates/mmd-engine/tests/rts_acceptance.rs`. It drives `RtsWorld` through
the **same** sequence using world calls rather than screen events, so a failure
says which system broke rather than which pixel moved:

```rust
/// The acceptance run, asserted milestone by milestone.
///
/// Deliberately not driven from the script file: this test owns the *meaning*
/// of the run, and `tests/rts_acceptance.rs` in the app crate owns the fact that
/// the shipped binary reproduces it. Two failures in different places mean
/// different things, which is the whole reason both exist.
#[test]
fn the_full_economy_loop_runs_end_to_end() { … }
```

Milestones, each a separate assertion with its own message:

| # | after | assert |
| - | ----- | ------ |
| 1 | box-select | `selection().len() == 6` |
| 2 | gather order | all six orders are `Order::Gather { .. }` |
| 3 | 600 ticks | `resources().crystal > 300` |
| 4 | place the Depot | `resources().crystal` fell by exactly `100`; `is_site(depot)` |
| 5 | 400 ticks | `is_site(depot) == false`; `supply().cap() == 20` |
| 6 | Depot finished | every Depot footprint cell is blocked in `nav().blocked()` |
| 7 | queue a Worker at the HQ | `resources().crystal` fell by exactly `50`; `supply().used() == 7` |
| 8 | 300 ticks | a seventh `Unit(Worker)` exists |
| 9 | place the Barracks | `crystal` fell by `150`, `gas` fell by `25` |
| 10 | 600 ticks | `is_site(barracks) == false` |
| 11 | queue a Soldier | `Err` is not returned; `supply().used()` rose by `2` |
| 12 | 400 ticks | exactly one `Unit(Soldier)` exists |
| 13 | end | `supply().used() <= supply().cap()` |
| 14 | end | `tick_index()` equals the ticks stepped (nothing silently paused) |

Plus:

```rust
/// The same run, twice, must land on the same hash.
#[test]
fn the_acceptance_run_is_reproducible() { … }

/// Every screen coordinate the tracked script uses must project to the cell it
/// claims. This is what keeps the script and the scene from drifting apart:
/// move a node and this fails, instead of the CLI run silently ordering nobody.
#[test]
fn the_script_coordinates_hit_what_they_name() {
    // reads assets/scenarios/rts_acceptance_v1.script, extracts every X,Y,
    // and asserts iso_view().cell_at(x, y, 320, 320) is the documented cell.
}
```

### CLI-level test

New `tests/rts_acceptance.rs` (app crate):

| Test | Input | Expect |
| ---- | ----- | ------ |
| `the_tracked_script_runs_clean` | `rts --frames 1600 --inject-input-file assets/scenarios/rts_acceptance_v1.script` | exit `0`, one `clean exit` line, `quit=true` |
| `the_acceptance_run_builds_two_buildings` | same | `buildings=3` (HQ + Depot + Barracks) |
| `the_acceptance_run_produces_a_soldier` | same | `units=` at least `8` (6 start + 1 Worker + 1 Soldier) |
| `the_acceptance_run_earns_crystal` | same | `crystal=` greater than `0` and the run spent `300` on buildings, so income happened |
| `the_acceptance_run_raises_the_supply_cap` | same | `supply=` right-hand side is `20` |
| `the_acceptance_run_is_deterministic` | run twice | identical `hash=` |
| `the_acceptance_run_fires_every_entry` | same | exit `0` — an unfired entry would exit `1` |
| `inject_input_and_file_are_mutually_exclusive` | both flags | exit `2` |
| `a_missing_script_file_is_actionable` | `--inject-input-file /nope` | exit `1`, message names the path |
| `comments_and_blank_lines_are_ignored` | a temp script with `#` comments and blank lines equivalent to a one-line `--inject-input` | identical `hash=` from both forms |

### Gate

`docs/05-testing.md`'s **Required merge gate** block gains, as the last line:

```sh
cargo run -- rts --frames 1600 --inject-input-file assets/scenarios/rts_acceptance_v1.script
```

with one sentence of prose below the block: it is the phase-1 interactive smoke;
it asserts behaviour and an exit code and consumes no measurement number.

## TDD

1. **Red** — write `crates/mmd-engine/tests/rts_acceptance.rs` and
   `tests/rts_acceptance.rs` first, and commit the script file empty enough to
   fail. Watch them fail.
2. **Green** — add `--inject-input-file`, write the script, retime until every
   milestone lands.
3. **Refactor** — none expected. Keep green.

## Test plan

Both tables above are the test plan. In addition:

| Test | Input | Expect |
| ---- | ----- | ------ |
| `parse_file_text_strips_comments` (unit, `src/rts_script.rs`) | `"1:key:esc # go\n\n# nothing\n"` | one entry |
| `parse_file_text_accepts_semicolons_too` (unit) | `"1:key:esc;2:key:esc"` on one line | two entries |
| `parse_file_text_rejects_a_bad_entry` (unit) | `"1:jump:1\n"` | `Err` naming `jump` |
| `the_tracked_script_parses` (unit) | the committed file's text | `Ok` |

**Mutation verification (mandatory).** Inject, confirm red, revert, confirm green:
1. Move the Depot's ghost click 40 px right (into a node) → kills milestone 4 and `the_acceptance_run_builds_two_buildings`.
2. `DEPOT_SUPPLY_GRANT = 0` → kills milestone 5 and `the_acceptance_run_raises_the_supply_cap`.
3. `WORKER_CARRY_CAPACITY = 0` → kills milestone 3.
4. Drop `--inject-input-file`'s comment stripping → kills `parse_file_text_strips_comments` and `the_tracked_script_runs_clean`.
5. Shift the whole camera start by 10 cells → kills `the_script_coordinates_hit_what_they_name`.
6. Make `rclick` on a node issue a move instead of a gather → kills milestone 2 and 3.

## Impl steps

- [x] 1. Add `parse_file_text` to `src/rts_script.rs` with its four unit tests.
- [x] 2. Add `--inject-input-file` to the `Rts` clap variant in `src/main.rs`, with `conflicts_with = "inject_input"`.
- [x] 3. Thread it through `RtsOptions` and `src/rts_run.rs`; a read failure is `RunError::Failed` naming the path.
- [x] 4. Create `crates/mmd-engine/tests/rts_acceptance.rs` with the 14 milestones, the reproducibility test and the coordinate test. Watch them fail.
- [x] 5. Create `tests/rts_acceptance.rs` with the ten CLI cases. Watch them fail.
- [x] 6. Write `assets/scenarios/rts_acceptance_v1.script` from the block above.
- [x] 7. Run both test files; retime the script (frames only) until every milestone lands. Record every retiming in the commit body.
- [x] 8. Add the acceptance command to `docs/05-testing.md`'s required merge gate.
- [x] 9. Run the mutation list; record kills in the commit body.
- [x] 10. Run the full validation block.

## Outputs

- **Files created**
  - `assets/scenarios/rts_acceptance_v1.script`
  - `crates/mmd-engine/tests/rts_acceptance.rs`
  - `tests/rts_acceptance.rs`
- **Files edited**
  - `src/main.rs`, `src/rts_run.rs`, `src/rts_script.rs`
  - `docs/05-testing.md`
- **Public API added:** `--inject-input-file`, `RtsScript::parse_file_text`.
- **Behaviour change:** the merge gate gains one command.
- **Migration / config:** none.

## Validation

- [x] `cargo fmt --all -- --check`
- [x] `cargo test -p mmd-engine --test rts_acceptance` — all green
- [x] `cargo test --test rts_acceptance` — all green
- [x] `MMD_REQUIRE_GPU=1 cargo test --workspace --locked`
- [x] `VK_DRIVER_FILES=/nonexistent cargo test --workspace --locked`
- [x] `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- [x] `nix flake check`
- [x] `cargo run -p xtask -- bootstrap --check ; cargo run -p xtask -- shaders --check ; cargo run -p xtask -- atlases --check`
- [x] `cargo run -- run --agents 5000 --frames 300` — exit 0, exit-line `hash=` unchanged
- [x] `cargo run -- run --scenario assets/scenarios/collision_mid_v1.ron --frames 300` — exit 0
- [x] `cargo run -- run --scenario assets/scenarios/collision_sprite_v1.ron --frames 300` — exit 0
- [x] `cargo run -- rts --frames 1600 --inject-input-file assets/scenarios/rts_acceptance_v1.script` — exit 0
- [x] app functional — no broken path from this slice
- [x] commit msg draft: `test(rts): prove the full economy loop end to end`
