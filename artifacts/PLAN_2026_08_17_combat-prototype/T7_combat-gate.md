# T7: Combat gate

**Plan:** `./artifacts/PLAN_2026_08_17_combat-prototype.md`
**Depends:** T6
**Commit outcome:** `rts_prototype_v1.ron` carries enemies (hundreds across waves) with regenerated sha256; a new tracked combat script drives spawn → march → fight → deaths through the shipped binary; exit line carries combat tokens; both phase-1 scripts re-baselined once — all in ONE commit so the gate is never red between commits.

## Context (self-contained)

- Goal: Phase 2 Combat Prototype — weapons, damage, turrets, enemy AI. Success = this ticket's script + tokens green on the full merge gate.
- This slice: the proof. Everything works (T1–T6) on fixture scenes; this puts enemies into the shared gate scene and pins the end-to-end run.
- Out of scope here: docs beyond the gate line (T8), any engine behavior change (a system bug found here → reopen the owning ticket, don't patch here). `sim/` frozen — the 5000-agent exit line, collision scenes, goldens, `atlas_count: 4` must not move.
- Assumptions in force (user-settled): early contact allowed inside the old scripts' 1600-frame windows; enemies spawn OUTSIDE direct contact range so combat provably begins by marching (`first_combat_tick` strictly after the first spawn tick); both old scripts' asserted counts + scene sha256 re-baselined ONCE in this commit; hundreds scale = 300–800 total enemies; sandbox — no win/lose, tokens carry the outcome.

### Deviation from the settled list (supervisor-approved 2026-08-17)

**`pre_placed` is empty — the gate scene is waves-only.** Proof: T3's enemy AI marches every idle enemy from tick 1 at 0.3 cells/tick, the map's longest reachable path to the HQ is ~230 cells, and 10 Ghouls at max(1, 5−2) = 3 damage per 30-tick cooldown kill the 400-HP HQ by ~tick 1170 — inside both old scripts' windows (canonical exits ≈ tick 1519), gutting un-re-baselineable phase-1 pins (`buildings=3`, supply cap 20, the whole economy). Waves at tick 3000+ keep both old windows provably enemy-free (their asserted counts move by exactly zero), make "combat begins by marching" *stronger* (`first_combat_tick > 3000` pinned), and still deliver hundreds (400 total). **T2's `pre_placed` feature stays covered by T2's own fixture tests** (`fixture_rts_combat_v1.ron`, `pre_placed_ghouls_spawn_at_start`); nothing here retests it.

### Detailer decisions (recorded — do not reopen)

- **D1 — wave table (validator-checked sizing, all cells verified against the RON's obstacle list):** `spawn_points: [(14, 304), (306, 304)]` (far SW / SE corners, both unblocked, 9×9 neighbourhoods clear, straight-line corridor to the base spot-checked obstacle-free); waves `(3000, 12, 0)`, `(4100, 150, 0)`, `(4160, 150, 1)`, `(4220, 88, 1)` — total **400** (in 300–800, ≤ 1200 cap, sorted, counts ≥ 1, spawn_point < 2). Tick-0 distances are infinite (no enemies exist); at first spawn (tick 3000) the nearest enemy is ~186 cells from every player entity — soldier 24 / turret 36 / Ghoul 8 all vastly exceeded.
- **D2 — timeline math:** SW corner → defence line at ~(150–161, 186–195) ≈ 186 cells ≈ 620 ticks → wave-1 contact ≈ tick 3480–3650 (turret range 36 fires first, ≈ 3500). Hordes (4100/4160/4220) all spawn before the script's quit (frame 4460 → tick 4459) but their nearest body stays > 70 cells from the turret at exit — they are spawned, counted, and never fought. `enemies_spawned=400`, `hq_alive=1` by construction.
- **D3 — kill-class attribution is structural, not token-level.** The exit line carries only aggregate `kills`. The script makes each required class the only source in range of its victims: the A-moved soldier (ordered engagement, `Order::AttackMove`), the rally-parked Idle soldier (auto-acquire), and the turret each cover the one corridor wave 1 walks. The ticket pins aggregate totals; the classes are proven by the script's construction + the pinned `first_combat_tick`.
- **D4 — the combat script uses positional keys** (`key:e`/`key:a`/`key:c`/`key:q`). The "pointer card only" rule was the *phase-1.1 script's own* rule (its header says so); T4 D3 confirmed `key:a` = `ExecuteSlot(3)` parses today. Zero new script vocabulary.
- **D5 — exit-line print site is single.** The ticket brief named an "`src/rts_script.rs` quit path" — inspection shows every clean exit flows through `finish()` in `src/rts_run.rs` (one `println!`, line 1908); `rts_script.rs` is touched only for a parse-pin test. Three sync sites total: doc contract (`src/rts_run.rs:21-63`), `finish()` (1908), `exit_line_tests` (2875).
- **D6 — `hq_alive` derivation:** `u8::from(world.start_hq().is_some_and(|id| world.entities().contains(id)))` — the starting HQ's id survives its death, `contains` is generation-checked.
- **D7 — re-baseline outcome: the old scripts' asserted counts do not move at all.** First spawn 3000 > canonical exit ≈ 1519 > focused exit 72. "Re-baseline" = sidecar regen + verification runs + NEW zero-pins (`phase1_scripts_report_bloodless_combat_tokens`: both old runs must read `kills=0 losses=0 enemies_spawned=0 first_combat_tick=none hq_alive=1`). Any old count moving is an upstream bug — reopen the owning ticket.
- **D8 — long-horizon engine tests repoint to a byte-identical baseline fixture.** 8 tests step the tracked scene past tick 3000 with economy assertions that marching Ghouls would eventually break (contact ≈ 3670+): `rts_economy.rs` × 4 (`the_worker_keeps_cycling` 6000, `six_workers_on_one_node_all_deliver` 4000, `the_economy_is_reproducible` 4000, `click_path_gather_banks_crystal` 6000) and `rts_nav_staleness.rs` × 4 (`a_walking_unit_re_paths_when_a_building_blocks_its_route` 4000, `an_evicted_field_does_not_hang_a_gather` 6000, `an_evicted_field_does_not_hang_a_build` 4000, `a_unit_caught_in_a_finished_footprint_can_still_gather` 6000). New `assets/scenarios/fixtures/fixture_rts_baseline_v1.ron` = byte-for-byte copy of the pre-edit scene (its sidecar therefore equals today's scene hash `b1fb663afd19d9181f028f0d163c5421500f5e5599722ba7254f3e6ede29e2ab`), so every pinned number keeps its meaning. Tests at exactly ≤ 3000 ticks stay on the scene: the wave fires on their final tick, 186+ cells away, and nothing they assert can move (verified per test; reproducibility pairs compare two identical runs).
- **D9 — `tests/rts_cli_contract.rs` needs zero edits.** Full-file inspection: every tracked-scene run there is ≤ 3000 frames; the single 3000-frame case (`a_right_click_on_a_node_starts_gathering`) asserts only `crystal > 300`, untouched by 12 Ghouls spawning 186 cells away on its final tick. The `rts --scenario` escape hatch exists (`src/main.rs:58-60`, default at `src/rts_run.rs:1015-1018`) but no test needs it.
- **D10 — engine canonical acceptance (`crates/mmd-engine/tests/rts_acceptance.rs`, 3520 ticks) survives unedited:** at tick 3520 wave-1 Ghouls are ~156 cells in (≈ (132, 202) / (192, 197)), > 12 cells from every node a worker works and > 140 from the HQ; first possible combat ≈ 3670 > 3520. Verified by running the suite in step 3.3.
- **D11 — frame budget 4500, script quits at frame 4460** (exit `frames=4459`, `tick=4459` — no pause in this script; the quit-frame convention `N:quit → frames=N−1` is pinned by `rts_cli_contract.rs`). All waves ≤ 4220 < 4459. `RUN_DEADLINE` in `tests/rts_acceptance.rs` rises 120 s → 300 s (4500 frames ≈ 2.8× the 1600-frame run that fits 120 s).
- **D12 — screen-coordinate formula for authoring** (derived from three known anchors in the phase-1.1 script and verified against all of them): for ground cell `(x, y)` at the starting camera, `screen_x = 960 + 4·((x−166) − (y−166))`, `screen_y = 540 + 2·((x−166) + (y−166))`; a placement cursor cell maps to ghost min corner `cursor − edge/2` (verified: Depot cursor (184,180) → min (180,176)).
- **D13 — exact-token pinning is observe-then-pin:** `combat_tokens_exact` is written in step 6 with the numbers read off a verified run (invariants checked first by `combat_begins_by_marching`), then two-run determinism locks them. Expected values from D2's math: `kills=12`, `losses∈{1,2}`, `enemies_spawned=400`, `first_combat_tick≈3480–3650`, `hq_alive=1`, `body_overlaps=0`.

## Requirements

- Scene edit (`assets/scenarios/rts_prototype_v1.ron`, 320×320, HQ min (160,160) edge 12): append the `enemies` block of D1 inside `rts: Some((…))` after `gas_nodes`; regenerate `assets/scenarios/rts_prototype_v1.sha256` (64 hex + newline — sidecar format verified via `xxd`).
- Baseline fixture: `assets/scenarios/fixtures/fixture_rts_baseline_v1.ron` + `.sha256` = pre-edit scene bytes (D8), created BEFORE the scene edit, same commit.
- Exit line (`src/rts_run.rs` only — D5): append **exactly, in this order, after `show_grid=<bool>`**:
  `kills=<u32> losses=<u32> enemies_spawned=<u32> first_combat_tick=<u32|none> hq_alive=<0|1>`
  from `RtsWorld::kills()`, `losses()`, `enemies_spawned()`, `first_combat_tick()` (T2/T3 getters) and D6's HQ liveness. Single spaces, no spaces inside a value.
- New tracked script `assets/scenarios/rts_combat_v1.script` (quit frame 4460): economy opening (box-select, crystal gather), Barracks (E-key ghost at min (146,176) — the phase-1.1 script's proven site), rally-parked Soldiers at the defence post (150,195), one soldier A-moved deeper (ordered class), sacrifice worker walked to (140,200), Turret built at min (158,186) astride the corridor (6×6 rect verified obstacle-free), wave 1 destroyed, hordes spawned-not-fought, `quit`. Full content in step 5.1.
- `tests/rts_acceptance.rs`: consts + shared-run plumbing + 6 new tests (invariants, token order, exact pins, two-process determinism, anti-vacuity, old-script bloodless zero-pins) + `RUN_DEADLINE` 300 s.
- `src/rts_script.rs`: `the_tracked_combat_script_parses` test (mirror of the existing pin — a typo must fail in unit tests, not minutes into a GPU run).
- `crates/mmd-engine/tests/{rts_economy.rs, rts_nav_staleness.rs}`: D8's 8 repoints via a local `baseline_scene()` helper; `crates/mmd-engine/tests/scenario_contract.rs`: `enemy_block_optional_old_scenes_parse` re-anchored (baseline fixture = no enemies; gate scene = D1's block, pinned).
- Gate docs: `docs/05-testing.md` **and** its `README.md` mirror each gain, after the existing rts line in the fenced block:
  `cargo run -- rts --frames 4500 --inject-input-file assets/scenarios/rts_combat_v1.script`
  (`tests/validation_contract.rs` requires the gate lists to be a superset of its pinned commands — adding a line passes; no edit there, T8 may promote the line to a pinned smoke.)
- Phase-0 contracts untouched: 5000-agent line, collision scenes, goldens, `atlas_count: 4`.

## Inputs (all inspected this branch, 2026-08-17)

- **Scene** `assets/scenarios/rts_prototype_v1.ron`: `start_crystal: 300`, `start_gas: 100`, `start_supply_cap: 10`, `hq_cell: (160,160)`, crystal nodes (140,150)(146,146)(152,142)(180,142)(186,146)(192,150)(150,190)(182,190), gas (136,168)(196,168), worker spawn cells (162..167, 178). Current sidecar hash `b1fb663a…e2ab`. Geometry verified by script: spawn corners + turret rect + park/sacrifice/A-move cells all obstacle-free.
- **Exit-line sites** `src/rts_run.rs`: stdout contract doc lines 21–63 (`(f)` token list ends `…settings_scroll_px=<n> show_grid=<bool>`); `finish()` lines 1853–1941 — checks (unfired entries, tick-vs-frames), audio line, `final_hash`, the one clean-exit `println!` at 1908 whose args end `…, session.ui.settings_scroll_px.round() as u32, session.settings.gameplay.show_grid,`; `count_entities` 1944; `exit_line_tests::exit_line_reports_live_show_grid` 2867–2884 (duplicated format string at 2875). `RtsWorld` getters available: `start_hq()` (world.rs:960), `entities().contains(id)`, `kills()/losses()/first_combat_tick()` (T3), `enemies_spawned()` (T2).
- **Script grammar** `src/rts_script.rs`: `FRAME:KIND[:ARGS]`, kinds `key/pan/panup/move/lclick/sclick/rclick/drag/wheel/quit`, `#` comments, newline = `;`, frames 1-based; `parse_file_text` rejects empty; tests mod at ~300 with `the_tracked_script_parses` as the template (path via `env!("CARGO_MANIFEST_DIR")`).
- **Key/card facts** (T4/T5 shipped): `KEY_BINDINGS` row-major `QWEASDZXC` → `ExecuteSlot(0..9)`; worker card slot 2 = Barracks, slot 3 = Turret (cost 75c, `TURRET_BUILD_TICKS=180`); armed card slot 3 = Attack (arms A-mode; ground click → `cmd_attack_move`), slot 4 = Stop; producer card slot 8 = Set Rally (`key:c`), slot 0 = produce (`key:q`); Soldier 50c/25g/360 ticks, Barracks 150c/25g/300 ticks; Ghoul 30 HP/0 armor, 18 c/s = 0.3 cells/tick, range 8, dmg 5/30t; Soldier range 24, 6/15t; Turret range 36, 10/20t; HQ 400 HP/armor 2.
- **`tests/rts_acceptance.rs`** (1076 lines): `SCRIPT`:23, `FRAMES`:25, `FOCUSED_SCRIPT`:33, `FOCUSED_FRAMES`:35; `Cli` helpers `exit_line`:70, `exit_field`:77, `exit_u32`:84, `final_hash`:102, `assert_success`:131; `rts()`:222 (offscreen + env scrub), `acceptance_run`:235, shared/second OnceLock pattern 256–302, `RUN_DEADLINE`:303, `run_to_completion`:305, `or_skip`:352; exact old pins: `buildings=3`:397, `units≥8`:410, supply cap `"20"`:507, `body_overlaps=0`:559/585, `voice_order=9`:601, `music_starts=1`/`voice_select=8`/`voice_reject=1`/`sfx_ui=8`:721–731, audio-line pins 750–767; anti-vacuity pattern `the_acceptance_run_fires_every_entry`:800; focused pins 900–1010.
- **`tests/rts_cli_contract.rs`** (2125 lines): budgets ≤ 3000 (max: `a_right_click_on_a_node_starts_gathering` at 836, crystal-only assert); `the_exit_line_reports_every_counter`:1028 (key list is a subset check — new tokens don't break it); quit-frame convention `3:quit → frames=2`:689–724.
- **Long-horizon engine tests**: `rts_economy.rs` — imports line 6–11 (`use mmd_engine::testkit::RtsHarness;`), `the_worker_keeps_cycling`:371 (6000), `six_workers_on_one_node_all_deliver`:578 (4000), `the_economy_is_reproducible`:595 (two harnesses, 4000), `click_path_gather_banks_crystal`:678 (6000); `rts_nav_staleness.rs` — `a_walking_unit_re_paths…`:122 (4000), `an_evicted_field_does_not_hang_a_gather`:180 (6000), `an_evicted_field_does_not_hang_a_build`:202 (4000), `a_unit_caught_in_a_finished_footprint_can_still_gather`:384 (6000). Engine canonical acceptance total 3520 ticks (`crates/mmd-engine/tests/rts_acceptance.rs:44-55`).
- **Testkit**: `RtsHarness::{scene, path, spec}` (`testkit/rts.rs:64-80`; `path` → `Scenario::load_verified` — needs the sidecar); `fixture_path(name)` appends `.ron` (`testkit/fixtures.rs:40`); `rts_scene_path()`:68.
- **`scenario_contract.rs`**: `enemy_block_optional_old_scenes_parse` (T2) currently asserts the tracked scene has `enemies.is_none()` — must be re-anchored here (D8/step 2.6).
- **Gate docs**: `docs/05-testing.md` fenced block lines 100–114 (rts line 113); `README.md` block 52–66 (rts line 66). `tests/validation_contract.rs`: `gate_section` asserts `commands.len() >= REQUIRED` + named commands present (224–266) — supersets are legal.
- **Sidecar regen** (format verified: 64 lowercase hex + `\n`): `sha256sum <file> | cut -c1-64 > <sidecar>`.
- Capacity: 400 enemies + ≤ 500-supply army + buildings + 10 nodes ≈ 920 ≪ `MAX_ENTITIES` 2048.

## TDD

1. **Red** — step 1's invariant tests land first and fail (no script file → exit 1 → `assert_success` panics; no tokens → `exit_field` panics).
2. **Green** — fixture + scene + tokens + script (steps 2–5) until the invariant tests pass; then observe-and-pin exact values (step 6).
3. **Refactor** — keep green; two-process determinism + bloodless zero-pins lock the whole surface.

## Test plan

| Test (exact fn name) | File | Input | Expect |
| ---- | ---- | ----- | ------ |
| `the_combat_script_runs_clean` | `tests/rts_acceptance.rs` | shared combat run | exit 0, exactly one clean-exit line, `quit=true`, no "never fired" |
| `combat_tokens_close_the_exit_line_in_order` | same | same run | the 5 token prefixes occupy positions `show_grid`+1 … +5 and `hq_alive=` ends the line |
| `combat_begins_by_marching` | same | same run | `first_combat_tick` ≠ `none` and `> 3000`; `enemies_spawned == 400`; `kills ≥ 1`; `losses ≥ 1`; `hq_alive == 1` |
| `combat_tokens_exact` | same | same run | pinned observed values (D13) incl. `body_overlaps == 0` |
| `combat_run_is_cross_process_deterministic` | same | shared + second run | whole `exit_line()` strings equal (covers hash + every token) |
| `the_combat_run_fires_every_entry` | same | shared run + 30-frame rerun | clean; short budget exits 1 saying "never fired" |
| `phase1_scripts_report_bloodless_combat_tokens` | same | canonical + focused shared runs | both: `kills=0 losses=0 enemies_spawned=0`, `first_combat_tick=none`, `hq_alive=1` |
| `the_tracked_combat_script_parses` | `src/rts_script.rs` | tracked combat script | parses non-empty |
| `exit_line_reports_live_show_grid` (extended) | `src/rts_run.rs` | format-string pin | contains all 5 new `token={}`s; ends with the 5-token tail |
| `enemy_block_optional_old_scenes_parse` (re-anchored) | `crates/mmd-engine/tests/scenario_contract.rs` | baseline fixture + gate scene | fixture: `enemies.is_none()`; scene: `pre_placed` empty, 2 spawn points, wave total 400, first `at_tick` 3000 |
| 8 repointed tests (names in D8, assertions untouched) | `rts_economy.rs` / `rts_nav_staleness.rs` | baseline fixture | green with identical pinned numbers |
| old acceptance + focused suites | `tests/rts_acceptance.rs` | both old scripts | every existing pinned count unchanged (D7) |
| full gate | `docs/05-testing.md` | every line incl. the new one | green |

## Impl steps

- [ ] 1. **Red — combat scaffolding in `tests/rts_acceptance.rs`** (invariant tests only; exact pins land in step 6)
  - [ ] 1.1 After `FOCUSED_FRAMES` (line 35) add:
    ```rust
    /// The tracked combat script (T7), relative to the crate root.
    const COMBAT_SCRIPT: &str = "assets/scenarios/rts_combat_v1.script";
    /// Frame budget the merge gate gives it. The script quits at frame 4460.
    const COMBAT_FRAMES: &str = "4500";
    /// Tick the gate scene's first enemy wave fires on (first `at_tick` in
    /// `assets/scenarios/rts_prototype_v1.ron`).
    const FIRST_SPAWN_TICK: u32 = 3000;
    /// Every enemy the gate scene scripts, across all four waves.
    const TOTAL_ENEMIES: u32 = 400;
    ```
  - [ ] 1.2 Line 303: change `const RUN_DEADLINE: Duration = Duration::from_secs(120);` to `Duration::from_secs(300)` and extend its doc: the combat run renders 4500 frames, ~2.8× the canonical 1600.
  - [ ] 1.3 In `impl Cli`, after `final_hash` (line 102), add:
    ```rust
    /// The `first_combat_tick` exit token: `none`, or the tick number.
    fn exit_first_combat(&self) -> Option<u32> {
        match self.exit_field("first_combat_tick") {
            "none" => None,
            raw => Some(raw.parse().unwrap_or_else(|e| {
                panic!("{self}\n`first_combat_tick={raw}` is neither `none` nor a number: {e}")
            })),
        }
    }
    ```
  - [ ] 1.4 After `focused_run()` (line ~247) add `combat_run()`, `shared_combat_run(case)`, `second_combat_run(case)` — copy the `focused_run`/`shared_focused_run`/`second_acceptance_run` bodies verbatim, swapping consts (`COMBAT_FRAMES`/`COMBAT_SCRIPT`) and the skip labels ("the tracked combat run" / "a second tracked combat run").
  - [ ] 1.5 New section `// T7: the combat gate run` at end of file with `the_combat_script_runs_clean`: shared run → `assert_success()`, exactly one `rts: clean exit` line, `exit_field("quit") == "true"`, `!combined().contains("never fired")`.
  - [ ] 1.6 `combat_tokens_close_the_exit_line_in_order`:
    ```rust
    let tokens: Vec<&str> = cli.exit_line().split_whitespace().collect();
    let at = |prefix: &str| {
        tokens.iter().position(|t| t.starts_with(prefix))
            .unwrap_or_else(|| panic!("{cli}\nexit line has no `{prefix}`"))
    };
    let base = at("show_grid=");
    for (offset, prefix) in
        ["kills=", "losses=", "enemies_spawned=", "first_combat_tick=", "hq_alive="]
            .iter().enumerate()
    {
        assert_eq!(at(prefix), base + 1 + offset, "{cli}\n`{prefix}` out of pinned order");
    }
    assert_eq!(base + 6, tokens.len(), "{cli}\nhq_alive must end the exit line");
    ```
  - [ ] 1.7 `combat_begins_by_marching`: `let first = cli.exit_first_combat().unwrap_or_else(|| panic!("{cli}\nthe combat run never fought"));` then `assert!(first > FIRST_SPAWN_TICK, …)`, `assert_eq!(cli.exit_u32("enemies_spawned"), TOTAL_ENEMIES, …)`, `assert!(cli.exit_u32("kills") >= 1, …)`, `assert!(cli.exit_u32("losses") >= 1, …)`, `assert_eq!(cli.exit_u32("hq_alive"), 1, …)`.
  - [ ] 1.8 `the_combat_run_fires_every_entry`: copy `the_focused_run_fires_every_entry` (line 1050), swap in `COMBAT_SCRIPT` and a `--frames 30` short run → `assert_actionable_failure().assert_says(&["never fired"])`.
  - [ ] 1.9 Red check: `cargo test --test rts_acceptance the_combat -- --nocapture` — the new tests fail (missing script → exit 1).

- [ ] 2. **Enemy-free baseline fixture + repoints** (MUST precede the scene edit — it copies today's bytes)
  - [ ] 2.1 From the repo root:
    ```sh
    cp assets/scenarios/rts_prototype_v1.ron assets/scenarios/fixtures/fixture_rts_baseline_v1.ron
    sha256sum assets/scenarios/fixtures/fixture_rts_baseline_v1.ron | cut -c1-64 > assets/scenarios/fixtures/fixture_rts_baseline_v1.sha256
    test "$(cat assets/scenarios/fixtures/fixture_rts_baseline_v1.sha256)" = "b1fb663afd19d9181f028f0d163c5421500f5e5599722ba7254f3e6ede29e2ab" && echo baseline-ok
    ```
  - [ ] 2.2 `crates/mmd-engine/tests/rts_economy.rs`: change the testkit import (line 11) to `use mmd_engine::testkit::{RtsHarness, fixture_path};` and add beside the helpers at the top:
    ```rust
    /// The enemy-free twin of the tracked scene, for horizons that cross its
    /// first wave tick (3000): `fixture_rts_baseline_v1.ron` is a
    /// byte-identical copy taken immediately before the combat gate scripted
    /// enemies into the scene, so every pinned number below keeps its meaning.
    fn baseline_scene() -> RtsHarness {
        RtsHarness::path(fixture_path("fixture_rts_baseline_v1"))
            .build()
            .expect("baseline scene harness")
    }
    ```
  - [ ] 2.3 Swap `RtsHarness::scene().build().expect(…)` → `baseline_scene()` in exactly: `the_worker_keeps_cycling` (:372), `six_workers_on_one_node_all_deliver` (:579), `the_economy_is_reproducible` (:596 **and** :597 — both harnesses), `click_path_gather_banks_crystal` (:~683). No assertion changes.
  - [ ] 2.4 `crates/mmd-engine/tests/rts_nav_staleness.rs`: same import change + same `baseline_scene()` helper (test crates cannot share it).
  - [ ] 2.5 Swap in exactly: `a_walking_unit_re_paths_when_a_building_blocks_its_route` (:123), `an_evicted_field_does_not_hang_a_gather` (:181), `an_evicted_field_does_not_hang_a_build` (:203), `a_unit_caught_in_a_finished_footprint_can_still_gather` (:385). The 2000/3000-tick cases in this file stay on `scene()`.
  - [ ] 2.6 `crates/mmd-engine/tests/scenario_contract.rs`, `enemy_block_optional_old_scenes_parse`: replace the body —
    ```rust
    // The pre-combat scene shape, preserved byte-for-byte as the baseline
    // fixture: parses hash-verified with no enemies.
    let baseline =
        Scenario::load_verified(mmd_engine::testkit::fixture_path("fixture_rts_baseline_v1"))
            .expect("baseline fixture loads hash-verified");
    assert!(baseline.rts().expect("rts block").enemies.is_none());

    // The gate scene itself now scripts the combat invasion (T7): waves
    // only, exactly the numbers the acceptance run pins.
    let scene = Scenario::load_verified(mmd_engine::testkit::rts_scene_path())
        .expect("tracked rts scene still loads hash-verified");
    let enemies = scene.rts().expect("rts block").enemies.as_ref()
        .expect("the combat gate scene scripts enemies");
    assert!(enemies.pre_placed.is_empty(), "waves-only by design (T7)");
    assert_eq!(enemies.spawn_points.len(), 2);
    assert_eq!(enemies.waves.iter().map(|w| w.count).sum::<u32>(), 400);
    assert_eq!(enemies.waves.first().map(|w| w.at_tick), Some(3000));
    ```
    (Name kept; the second half goes red until step 3.)
  - [ ] 2.7 `cargo test -p mmd-engine --test rts_economy --test rts_nav_staleness` — green (fixture bytes == scene bytes today).

- [ ] 3. **Scene edit + sidecar regen**
  - [ ] 3.1 `assets/scenarios/rts_prototype_v1.ron` — inside `rts: Some((…))`, immediately after the `gas_nodes: [ … ],` block, insert:
    ```ron
    enemies: Some((
      pre_placed: [],
      spawn_points: [
        (x: 14, y: 304),
        (x: 306, y: 304),
      ],
      waves: [
        (at_tick: 3000, count: 12, spawn_point: 0),
        (at_tick: 4100, count: 150, spawn_point: 0),
        (at_tick: 4160, count: 150, spawn_point: 1),
        (at_tick: 4220, count: 88, spawn_point: 1),
      ],
    )),
    ```
  - [ ] 3.2 `sha256sum assets/scenarios/rts_prototype_v1.ron | cut -c1-64 > assets/scenarios/rts_prototype_v1.sha256`
  - [ ] 3.3 Verification battery (D8/D9/D10 — expect zero edits beyond 2.6 going green): `cargo test -p mmd-engine --test scenario_contract --test rts_acceptance --test rts_economy --test rts_nav_staleness --test rts_production --test rts_radius_nav` then `cargo test --test rts_cli_contract`. Any failure here is an upstream bug or a missed pin — stop and reopen the owning ticket, do not patch behavior here.

- [ ] 4. **Exit tokens in `src/rts_run.rs`** (three sync sites, D5)
  - [ ] 4.1 Stdout-contract doc (lines 21–27): change the last `(f)` line pair to
    ```text
    //!      keyboard_pan=<n> settings_scroll_px=<n> show_grid=<bool> kills=<n> \
    //!      losses=<n> enemies_spawned=<n> first_combat_tick=<n|none> hq_alive=<0|1>  (f)
    ```
    and append to the `(f)` bullet's prose: `…and then the combat gate's five tokens (T7): kills/losses from the world's combat counters, enemies_spawned (cumulative spawn odometer — pre-placed plus waves, never decremented on death), first_combat_tick (none while the run is bloodless), and hq_alive — whether the scene's starting HQ is still alive.`
  - [ ] 4.2 In `finish()`, before the `println!` at 1908, add:
    ```rust
    let first_combat = world
        .first_combat_tick()
        .map_or_else(|| "none".to_string(), |t| t.to_string());
    // The *starting* HQ's liveness: its id survives the building's death and
    // `contains` is generation-checked, so a reused slot cannot lie.
    let hq_alive = u8::from(
        world
            .start_hq()
            .is_some_and(|id| world.entities().contains(id)),
    );
    ```
    then extend the format string's tail from `show_grid={}"` to `show_grid={} kills={} losses={} enemies_spawned={} first_combat_tick={} hq_alive={}"` and append the args after `session.settings.gameplay.show_grid,`:
    ```rust
    world.kills(),
    world.losses(),
    world.enemies_spawned(),
    first_combat,
    hq_alive,
    ```
  - [ ] 4.3 `exit_line_tests` (2867–2884): update the duplicated `format_str` to the new full string and extend the assertions:
    ```rust
    for token in [
        "show_grid={}", "kills={}", "losses={}", "enemies_spawned={}",
        "first_combat_tick={}", "hq_alive={}",
    ] {
        assert!(format_str.contains(token), "exit line format must include {token}");
    }
    assert!(format_str.ends_with(
        "kills={} losses={} enemies_spawned={} first_combat_tick={} hq_alive={}"
    ));
    ```
  - [ ] 4.4 Smoke: `SDL_VIDEODRIVER=offscreen MMD_WINDOW_HIDDEN=1 cargo run -- rts --frames 5 | tail -2` — exit line ends `… kills=0 losses=0 enemies_spawned=0 first_combat_tick=none hq_alive=1`.

- [ ] 5. **The combat script**
  - [ ] 5.1 Create `assets/scenarios/rts_combat_v1.script` with exactly:
    ```text
    # Phase-2 combat gate: economy -> army -> waves -> fight -> deaths, on the
    # shared gate scene (waves at ticks 3000/4100/4160/4220 from the far south
    # corners). Frames are 1-based; nothing here pauses, so tick = frame - 1.
    #
    # Coordinates are screen pixels at the starting camera: ground cell (x,y)
    # maps to (960 + 4*((x-166)-(y-166)), 540 + 2*((x-166)+(y-166))).
    #
    # Positional keys are deliberate (the pointer-card rule was the phase-1.1
    # script's own): E = worker slot 2 (Barracks), A = worker slot 3 (Turret)
    # / armed slot 3 (Attack), C = producer slot 8 (Set Rally), Q = slot 0.

    1:move:960,540
    5:drag:850,520,1000,600        # box-select the six starting workers
    10:rclick:897,411              # all six gather the NW crystal node - war chest

    # Detach three specialists while the group is still near the spawn cells
    # (the same departure timing the phase-1.1 script proved).
    12:lclick:906,563              # worker over spawn cell (165,178): turret builder
    14:rclick:848,559              # park it at ~(156,184), beside the turret site
    16:lclick:916,568              # worker over spawn cell (167,178): the sacrifice
    18:rclick:720,557              # walk it to (140,200), astride the enemy approach
    22:lclick:896,558              # worker over spawn cell (162,178): Barracks builder
    24:key:e                       # worker card slot 2 = Barracks ghost
    26:move:840,542
    27:lclick:840,542              # place the Barracks at min corner (146,176)
    28:rclick:840,542              # send the selected worker onto its own site

    # Army: rally first, then two Soldiers (supply 6 + 2x2 = 10 = the cap).
    400:lclick:840,542             # select the finished Barracks
    402:key:c                      # producer slot 8 = Set Rally
    404:lclick:780,567             # rally at (150,195): the defence post
    406:key:q                      # queue Soldier 1 (50c / 25g)
    410:key:q                      # queue Soldier 2

    # Ordered engagement: one soldier attack-moves deeper into the corridor;
    # the other stays Idle at the rally (the auto-acquire class).
    1400:lclick:780,567            # select one soldier at the rally post
    1402:key:a                     # armed card slot 3 = Attack (arms A-mode)
    1404:lclick:740,565            # A-move to ~(145,200): Order::AttackMove

    # Turret astride the corridor, built by the parked worker.
    1500:lclick:848,559            # select the parked builder at (156,184)
    1502:key:a                     # worker card slot 3 = Turret ghost (75c)
    1504:move:848,577
    1505:lclick:848,577            # place at min corner (158,186)
    1506:rclick:848,577            # send the builder onto its own site

    # Waves fire at ticks 3000/4100/4160/4220; wave 1 (12 ghouls) reaches the
    # defence ~tick 3500-3650: turret + A-moved soldier + idle soldier destroy
    # it, the sacrificed worker dies to ghoul melee. The hordes (388) spawn
    # before the exit and never reach the base.
    4440:key:f1                    # overlay on for the final frames
    4460:quit
    ```
  - [ ] 5.2 `src/rts_script.rs`, tests mod — after `the_tracked_script_parses` add its twin:
    ```rust
    /// The committed combat script is on the merge gate too; a typo in it
    /// must fail here, not four minutes into a GPU run.
    #[test]
    fn the_tracked_combat_script_parses() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("assets/scenarios/rts_combat_v1.script");
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        let script = RtsScript::parse_file_text(&text)
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert!(!script.entries.is_empty(), "{} parsed to nothing", path.display());
    }
    ```
  - [ ] 5.3 Iterate offscreen until every entry fires and the invariants hold:
    ```sh
    SDL_VIDEODRIVER=offscreen MMD_WINDOW_HIDDEN=1 cargo run -- rts --frames 4500 \
      --inject-input-file assets/scenarios/rts_combat_v1.script | tail -3
    ```
    Iteration guidance (nudge, never redesign): a selection click that misses → shift the click ±4 px x / ±2 px y (one cell = 4×2 px) or ±10 frames; `losses=0` → move the sacrifice cell ~10 cells further out along the corridor, e.g. (130,210) → click `680,565` (still inside turret range 36, outside soldier range); soldier-select at 1400 missing → the two rally formation slots straddle (150,195), probe `776,565`/`784,569`; enqueue rejected at 406/410 → push both `key:q` +200 frames (income timing); `hq_alive=0` (leakers) → move the A-move goal 3 cells north-east toward the HQ. Every entry MUST fire — the binary exits 1 naming any that didn't.
  - [ ] 5.4 From the verified run's exit line record: `kills`, `losses`, `first_combat_tick`, and confirm `enemies_spawned=400`, `hq_alive=1`, `body_overlaps=0`, `first_combat_tick > 3000`, `frames=4459`, `tick=4459`.

- [ ] 6. **Pin + determinism + bloodless zero-pins** (`tests/rts_acceptance.rs`)
  - [ ] 6.1 `combat_tokens_exact` — the observed values from 5.4 as hard equalities (D13 expects `kills=12`, `losses∈{1,2}`, `first_combat_tick≈3480–3650`):
    ```rust
    assert_eq!(cli.exit_u32("kills"), /* observed */, "{cli}");
    assert_eq!(cli.exit_u32("losses"), /* observed */, "{cli}");
    assert_eq!(cli.exit_u32("enemies_spawned"), 400, "{cli}");
    assert_eq!(cli.exit_first_combat(), Some(/* observed */), "{cli}");
    assert_eq!(cli.exit_u32("hq_alive"), 1, "{cli}");
    assert_eq!(cli.exit_u32("body_overlaps"), 0, "{cli}\nADR 021 holds under combat");
    ```
  - [ ] 6.2 `combat_run_is_cross_process_deterministic` — shared + second run, `assert_eq!(a.exit_line(), b.exit_line(), …)` (one string compare covers hash and every token).
  - [ ] 6.3 `phase1_scripts_report_bloodless_combat_tokens` — for both `shared_acceptance_run` and `shared_focused_run`: `kills=0`, `losses=0`, `enemies_spawned=0`, `exit_first_combat()==None`, `hq_alive=1` (the old windows are provably combat-free; this is the re-baseline's teeth).
  - [ ] 6.4 `cargo test --test rts_acceptance` — everything green, old pins untouched.

- [ ] 7. **Gate docs (both mirrors)**
  - [ ] 7.1 `docs/05-testing.md` fenced gate block (line ~113): after the `rts --frames 1600 …rts_acceptance_v1.script` line add
    `cargo run -- rts --frames 4500 --inject-input-file assets/scenarios/rts_combat_v1.script`
  - [ ] 7.2 `README.md` gate block (line ~66): same line, same position.
  - [ ] 7.3 `cargo test --test validation_contract` — green (supersets are legal; the pinned-command lists are untouched).

- [ ] 8. **Full gate + one-commit proof**
  - [ ] 8.1 `cargo fmt --all -- --check && cargo test --workspace --locked && cargo clippy --workspace --all-targets --all-features -- -D warnings`
  - [ ] 8.2 `nix flake check` + the four xtask `--check`s + the three `run` smokes + both rts gate lines (1600-frame acceptance, 4500-frame combat) — every line of `docs/05-testing.md`.
  - [ ] 8.3 Single-commit proof, then commit:
    ```sh
    test "$(sha256sum assets/scenarios/rts_prototype_v1.ron | cut -c1-64)" = "$(cat assets/scenarios/rts_prototype_v1.sha256)" && echo scene-sidecar-ok
    git status --porcelain   # exactly the Outputs list below, staged together
    ```
    Commit msg: `feat(rts): combat on the gate scene with scripted end-to-end proof`

## Outputs

- Files: `assets/scenarios/rts_prototype_v1.ron` + `.sha256` (enemies block, regenerated sidecar), **new** `assets/scenarios/fixtures/fixture_rts_baseline_v1.ron` + `.sha256` (pre-edit bytes), **new** `assets/scenarios/rts_combat_v1.script`, `src/rts_run.rs` (3 exit-line sites), `src/rts_script.rs` (parse-pin test only), `tests/rts_acceptance.rs` (consts, `RUN_DEADLINE` 300 s, helpers, 7 new tests), `crates/mmd-engine/tests/rts_economy.rs` + `rts_nav_staleness.rs` (baseline repoints ×4 each), `crates/mmd-engine/tests/scenario_contract.rs` (re-anchored old-scene test), `docs/05-testing.md` + `README.md` (one gate line each). ALL IN ONE COMMIT.
- Public behavior: exit line grows exactly 5 tokens in pinned order after `show_grid`; gate grows one line (both doc mirrors).
- NOT touched: any engine `src` under `crates/mmd-engine/src`, `sim/`, `tests/rts_cli_contract.rs`, `tests/validation_contract.rs`, old tracked scripts, sprites, goldens.

## Validation

- [ ] `test "$(sha256sum assets/scenarios/rts_prototype_v1.ron | cut -c1-64)" = "$(cat assets/scenarios/rts_prototype_v1.sha256)"` — true; same check for the baseline fixture sidecar (must equal `b1fb663a…e2ab`)
- [ ] `SDL_VIDEODRIVER=offscreen MMD_WINDOW_HIDDEN=1 cargo run -- rts --frames 4500 --inject-input-file assets/scenarios/rts_combat_v1.script | tail -1` — clean exit with `enemies_spawned=400 … hq_alive=1`, `first_combat_tick` > 3000, `kills ≥ 1`, `losses ≥ 1`, `body_overlaps=0`
- [ ] `cargo run -- rts --frames 1600 --inject-input-file assets/scenarios/rts_acceptance_v1.script` — old exit tokens unchanged plus `kills=0 losses=0 enemies_spawned=0 first_combat_tick=none hq_alive=1`
- [ ] Full merge gate from `docs/05-testing.md`, every line incl. the new combat line, locally green
- [ ] `git diff --stat` shows scene + sidecar + fixture + script + tests + docs moved in the SAME commit
- [ ] commit msg draft: `feat(rts): combat on the gate scene with scripted end-to-end proof`
