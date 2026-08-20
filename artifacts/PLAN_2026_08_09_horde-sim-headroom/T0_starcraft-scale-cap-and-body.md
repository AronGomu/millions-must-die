# T0: StarCraft-scale cap and body

**Plan:** `./artifacts/PLAN_2026_08_09_horde-sim-headroom.md`
**Depends:** none
**Commit outcome:** `5 000` is the absolute simultaneous-entity ceiling of this
engine, enforced by the scenario contract; sprites are 48 px and bodies are
exactly half a sprite; the gate-scene digest is re-pinned **once** and frozen
for every ticket after this one.

## Context (self-contained)

- Decision of 2026-08-09: *make the game feel closer to StarCraft — fewer
  entities, but bigger*. The swarm/horde read is bought with body scale and
  density, not with population.
- This slice: the resize, and nothing else. Contract constants, three tracked
  `.ron` scenes + sidecars, the frozen bench ladder, every doc and command that
  names 50 000 or 100 000, and three test names that bake `50000` into their
  identity.
- **This is the only ticket in the plan allowed to move a hash.** It moves the
  gate scene's run digest exactly once. T1–T9 must reproduce T0's value.
  It moves **no** inline `GridSpec` digest — do not touch `GridSpec` defaults.
- Out of scope here: the three headroom knobs (T1), any change to
  `sim/tick.rs`, `sim/spatial.rs` or `accumulate_separation` (T2–T6), the
  hitbox ring (T8), the isometric projection (T9). No performance number in any
  doc, test name or commit message.
- Assumptions in force:
  - The `fixture_*` scenes are **not** retuned. Their bodies
    (`collision_radius_q8: 32`) are what the arrival-tick constants in
    `crates/mmd-engine/tests/simulation.rs` are calibrated against, and their
    populations are already far below the ceiling.
  - `technical_prototype_v1` is retuned **in place**, keeping its version id.
    No `technical_prototype_v2` is created.
  - `.sha256` sidecars are bare lowercase hex, trimmed.
  - Perf stays retired. The bench ladder is edited only so a frozen policy
    stops *naming* a population we refuse to run; it stays frozen and
    non-gating, and no threshold changes.
  - `graphify` is installed. Orient with `graphify query` before reading source;
    run `graphify update .` as the last validation step.
- **`TODO(user)` — RESOLVED by the orchestrator, 2026-08-09.**
  `plan/zombie-collision` (`108230a`) is already an ancestor of `main`
  (`git merge-base --is-ancestor plan/zombie-collision main` → true), and
  `origin/main` == local `main` (`5ec9df6`). The branch
  `plan/horde-sim-headroom` is cut from that `main` by pre-flight and already
  exists when this ticket starts. **Do not create or switch branches** — just
  work on the current one.

## Requirements

- `crates/mmd-engine/src/scenario.rs` gains
  `pub const MAX_LIVE_AGENTS: u32 = 5_000;` with a doc comment stating it is the
  absolute simultaneous-entity ceiling of the engine and that nothing above it
  is run, tested or benchmarked.
- Every validator rejects `hard_agent_count` or `stretch_agent_count` above
  `MAX_LIVE_AGENTS`, in **every** family — v1, `collision_scene_v1`, `fixture_*`.
  The error names the offending field, the value and the cap.
- `COLLISION_SCENE_MAX_AGENTS` is removed; `validate_collision_scene_dims` uses
  `MAX_LIVE_AGENTS`. (Keeping a second, larger cap would let a collision scene
  declare 20 000.)
- `FIXTURE_MAX_AGENTS` stays `4_096` — already under the ceiling, and it carries
  a different intent (fixtures stay cheap).
- Locked v1 constants change:
  - `V1_HARD_AGENTS`: `50_000` → `5_000`
  - `V1_STRETCH_AGENTS`: `100_000` → `5_000`
  - `V1_SPRITE_PX`: `30` → `48`
  - `V1_COLLISION_RADIUS_Q8`: `102` → `1_536`
- `MAX_COLLISION_RADIUS_Q8` stays `2_048`; `1_536 < 2_048` must hold and a test
  must say so, so a later radius bump cannot silently exceed the cap.
- The three full-screen `.ron` scenes are retuned and their sidecars
  regenerated:
  | Scene | `sprite_size_px` | `collision_radius_q8` | `hard` | `stretch` |
  | ----- | ---------------- | --------------------- | ------ | --------- |
  | `technical_prototype_v1` | 48 | 1 536 | 5 000 | 5 000 |
  | `collision_mid_v1` | 48 | 1 536 | 5 000 | 5 000 |
  | `collision_sprite_v1` | 48 | 1 536 | 1 200 | 5 000 |
  `separation_strength_q8` is unchanged (`256`) everywhere.
- The four `fixture_*` scenes are byte-identical afterwards. Their sidecars are
  not regenerated.
- The gate smoke becomes `cargo run -- run --agents 5000 --frames 300` in:
  `AGENT.md`, `README.md`, `docs/05-testing.md`, `HANDOFF.md`,
  `docs/platform/macos-bootstrap.md`, `docs/lab/gpu-profiling.md`, and the
  `--agents` doc comment in `src/main.rs` (line ~33, "default: scenario hard
  count = 50000").
- **`crates/mmd-engine/src/bench/policy.rs` — AMENDED by the orchestrator,
  2026-08-09. Do NOT retune the ladder.** The original requirement
  (`SCALE_COUNTS` → `[500, 1_000, 2_500, 5_000]`, `GATE_AGENT_COUNT` and
  `STRETCH_AGENT_COUNT` → `5_000`) rested on a false premise. The first T0
  attempt proved it: `SCALE_COUNTS` / `GATE_AGENT_COUNT` are **load-bearing
  inputs** to `validate_release_proof`, the merge gate and all three candidate
  lanes, which validate ~15 committed evidence artifacts under `lab/fixtures/**`
  and `lab/releases/evidence/**` recorded at 1 000 / 10 000 / 50 000 / 100 000
  agents. Retuning the constants reds 31 `mmd-lab` tests; isolation was proven
  (reverting only `policy.rs` turns all 31 green again). Greening it would need
  either rewriting the committed evidence JSON — which **falsifies frozen
  phase-0 measurement history** (`d37bfe6` "close phase 0 on honest inconclusive
  50k evidence") and is forbidden by this plan's own no-perf-claim rule — or a
  policy-versioning redesign of the lab, which is a separate slice and is not in
  this plan's Scope In.
  **Resolution: `bench/policy.rs` and the `mmd-lab` evidence are designated
  phase-0 history, exactly like `docs/technical-prototype-results.md` and the
  ADRs.** They record the policy (`policy_id: "production-v1"`) that phase 0's
  evidence was measured under; that policy cannot retroactively become something
  else. Therefore:
  - `SCALE_COUNTS`, `GATE_AGENT_COUNT`, `STRETCH_AGENT_COUNT` keep their
    historical values. Revert any edit to them.
  - `crates/mmd-engine/tests/benchmark_policy.rs`, `tools/mmd-lab/src/gate.rs`
    and `tools/mmd-lab/src/release.rs` are reverted to `HEAD` for the same
    reason — their fixture strings mirror the frozen ladder.
  - Instead, add a **comment only** at the head of the ladder constants in
    `bench/policy.rs` recording that (a) this policy is frozen phase-0 history
    and the populations it names are historical measurement tiers, not live
    targets, (b) the engine's live simultaneous-entity ceiling is
    `scenario::MAX_LIVE_AGENTS = 5_000`, and (c) nothing above that ceiling is
    run, tested or benchmarked from phase 0.5 onward. The comment must contain
    **no** performance number and no `PERF_THRESHOLD_TOKENS` /
    `EXTRA_PERF_CLAIM_TOKENS` token.
  - No threshold value changes anywhere; `gate_list_has_no_perf_thresholds`
    stays green, and `cargo test -p mmd-lab` stays green.
  Residual risk, accepted and logged: the frozen policy still *names* 50 000 and
  100 000. The plan's Scope-In line "no policy constant names a larger
  population" is knowingly not met for this one frozen-history surface, because
  the only ways to meet it are dishonest or out of scope. The live contract —
  which is what actually governs what runs — does enforce the ceiling.
- Three tests are renamed so no test identity contains a population we no longer
  run, and `tests/validation_contract.rs` (lines ~360, ~361, ~425) plus the
  close-doc map rows follow:
  | Old | New | File |
  | --- | --- | ---- |
  | `population_stays_50000` | `population_stays_at_the_cap` | `crates/mmd-engine/tests/simulation.rs:114` |
  | `determinism_holds_for_50k_agents` | `determinism_holds_at_the_cap` | `crates/mmd-engine/tests/simulation.rs:530` |
  | `builds_50000_instances` | `builds_one_instance_per_agent` | `crates/mmd-engine/tests/runtime_frame.rs:44` |
- `docs/technical-prototype-functional-close.md` is in `LIVE_DOCS`: its rows 23,
  40 and 45 change, and the edit must contain no token from
  `PERF_THRESHOLD_TOKENS` + `EXTRA_PERF_CLAIM_TOKENS`.
- `docs/technical-prototype-results.md` and the ADRs are designated history —
  **leave their 50k references alone**; they record what phase 0 did.
  `.tmp/` and `.pi-subagents/` are untracked working notes — leave them.
- **Designated history, extended (orchestrator, 2026-08-09).** The same rule
  covers `crates/mmd-engine/src/bench/policy.rs`,
  `crates/mmd-engine/tests/benchmark_policy.rs`, `tools/mmd-lab/**` and
  `lab/**` — the frozen `production-v1` policy and the evidence recorded under
  it. Leave every population they name alone. Also out of the sweep, and NOT
  defects: `crates/mmd-engine/src/render/renderer.rs` `MAX_INSTANCES` (a GPU
  instance-buffer capacity, not a declared population — not in this ticket's
  Inputs, do not touch it), `tests/validation_contract.rs` ~893 (a deliberate
  bad-doc fixture feeding `perf_claim_scanner_catches_what_it_is_meant_to` —
  editing it would weaken the scanner's own test),
  `crates/mmd-engine/src/testkit/rng.rs` ~38 (the FNV prime `0x100_0000_01B3`,
  a grep false positive), `Cargo.lock`, and this plan's own directory
  `artifacts/PLAN_2026_08_09_horde-sim-headroom*` plus
  `artifacts/PLAN_2026_08_08_zombie-collision/`.
- The gate smoke's `hash=` on the `clean exit` line is captured and written into
  this ticket's Outputs as the new pinned digest.

## Inputs

- `crates/mmd-engine/src/scenario.rs` — constants (lines ~34–62),
  `validate_version_and_dims` (~346), `validate_counts` (~384),
  `validate_collision` (~423), `validate_fixture_dims` (~448),
  `validate_collision_scene_dims` (~493).
- `crates/mmd-engine/src/bench/policy.rs` — lines ~8, ~11, ~14.
- `crates/mmd-engine/tests/benchmark_policy.rs` — lines ~166, ~167, ~302.
- `crates/mmd-engine/tests/scenario_contract.rs` — where the new cap tests go
  (`collision_radius_above_the_cap_is_refused` at ~107 is the shape to copy).
- `crates/mmd-engine/tests/simulation.rs` — ~114, ~530.
- `crates/mmd-engine/tests/runtime_frame.rs` — ~44.
- `tests/validation_contract.rs` — ~360, ~361, ~425, `MIN_SCANNED_TESTS` (~516).
- `tests/cli_contract.rs` — `absurd_agent_count_rejected` (~862),
  `agent_count_override_is_respected` (~567).
- `tools/mmd-lab/src/gate.rs` — ~634, ~681, ~684, ~687.
- Scenes: `assets/scenarios/technical_prototype_v1.ron`,
  `assets/scenarios/collision_mid_v1.ron`,
  `assets/scenarios/collision_sprite_v1.ron` (+ their `.sha256`).
- Docs listed under Requirements.
- **From Depends:** none — this is the first ticket.

**Existing signatures this ticket must preserve:**

```rust
// crates/mmd-engine/src/scenario.rs
pub const COLLISION_Q8: u32 = 256;
pub const MAX_COLLISION_RADIUS_Q8: u32 = 2_048;
pub const MAX_SEPARATION_STRENGTH_Q8: u32 = 2_560;
pub const FIXTURE_MAX_CELLS: u32 = 65_536;
pub const FIXTURE_MAX_AGENTS: u32 = 4_096;
impl Scenario {
    pub fn collision_radius_q8(&self) -> u32;
    pub fn collision_radius_cells(&self) -> f32;
    pub fn hard_agent_count(&self) -> u32;
    pub fn stretch_agent_count(&self) -> u32;
}
```

`COLLISION_SCENE_MAX_AGENTS` is the one public constant that goes; grep for it
before deleting (`crates/mmd-engine/tests/scenario_contract.rs` references it).

## TDD

1. **Red** — add the six tests below. They fail on the current tree: the cap
   constant does not exist, and the scenes still declare the old geometry.
2. **Green** — change the constants, retune the three `.ron` scenes, regenerate
   the three sidecars, sweep the docs and commands, rename the three tests.
3. **Refactor** — collapse the two population caps into one path; make sure the
   cap error message is produced in exactly one place.

## Test plan

| Test | Where | Asserts |
| ---- | ----- | ------- |
| `population_above_the_ceiling_is_refused` | `scenario_contract.rs` | a spec with `hard_agent_count: 5_001` fails with an error naming `MAX_LIVE_AGENTS`; `5_000` passes |
| `stretch_above_the_ceiling_is_refused` | `scenario_contract.rs` | same for `stretch_agent_count`, in the v1, collision and fixture families |
| `the_ceiling_binds_every_scenario_family` | `scenario_contract.rs` | iterating the three family validators, none accepts `5_001` — a new family cannot forget the cap |
| `a_body_is_half_a_sprite` | `scenario_contract.rs` | for all three tracked full-screen scenes, `collision_radius_cells() * cell_size_px * 2 == sprite_size_px` — the "no art overlap at contact" property, stated as arithmetic |
| `the_locked_radius_is_under_the_radius_cap` | `scenario_contract.rs` | `V1_COLLISION_RADIUS_Q8 < MAX_COLLISION_RADIUS_Q8` |
| `every_tracked_scene_is_within_the_ceiling` | `scenario_contract.rs` | loads all seven tracked scenes and asserts both counts `<= MAX_LIVE_AGENTS` |
| `absurd_agent_count_rejected` (existing) | `tests/cli_contract.rs` | still passes — `--agents 5001` now trips the stretch cap |
| `population_stays_at_the_cap` (renamed) | `simulation.rs` | unchanged behaviour, new name |
| `determinism_holds_at_the_cap` (renamed) | `simulation.rs` | unchanged behaviour, new name |
| `builds_one_instance_per_agent` (renamed) | `runtime_frame.rs` | unchanged behaviour, new name; the hardcoded `50_000` expectation becomes the scenario's hard count |

## Impl steps

- [x] 1. `graphify query "scenario agent count validation caps"` and
      `graphify explain "collision_radius_q8"` to confirm nothing outside the files
      listed in Inputs reads the constants. — both run; the follow-up
      `grep -rn "COLLISION_SCENE_MAX_AGENTS\|V1_HARD_AGENTS\|…"` shows the
      constants are read only by `scenario.rs`, `scenario_contract.rs` and
      `testkit/mod.rs` (all in Inputs).
- [x] 2. Add `MAX_LIVE_AGENTS` to `scenario.rs`. Write the six new tests. Run — red.
      — `cargo test -p mmd-engine --test scenario_contract` → `26 passed; 5 failed`;
      failures are `a_body_is_half_a_sprite`,
      `every_tracked_scene_is_within_the_ceiling`,
      `population_above_the_ceiling_is_refused`,
      `stretch_above_the_ceiling_is_refused`,
      `the_ceiling_binds_every_scenario_family`. The sixth,
      `the_locked_radius_is_under_the_radius_cap`, is a regression guard that
      must hold before and after (`102 < 2_048`, later `1_536 < 2_048`).
- [x] 3. Delete `COLLISION_SCENE_MAX_AGENTS`; route
      `validate_collision_scene_dims` through `MAX_LIVE_AGENTS`. — constant gone
      from `scenario.rs`; `collision_scene_caps_its_population` now asserts
      `exceeds MAX_LIVE_AGENTS` and passes.
- [x] 4. Add the ceiling check to `validate_fixture_dims` and to the v1 path. Prefer
      one shared `fn check_population(hard, stretch) -> Result<(), ScenarioError>`
      called by all three so a fourth family cannot skip it. — implemented as a
      single `check_population(doc)` call at the top of
      `validate_version_and_dims`, after version recognition and *before* family
      dispatch. Stronger than three call sites: a fourth family inherits the
      ceiling instead of having to remember it.
      `the_ceiling_binds_every_scenario_family` passes.
- [x] 5. Change `V1_HARD_AGENTS`, `V1_STRETCH_AGENTS`, `V1_SPRITE_PX`,
      `V1_COLLISION_RADIUS_Q8`. — `5_000 / 5_000 / 48 / 1_536`;
      `loads_v1_scene` + `gate_scene_locks_its_collision_tuning` pass.
- [x] 6. Edit the three `.ron` scenes. Only the six scalar fields change — spawn
      lists, obstacle lists, seeds and destinations are untouched. —
      `git diff -U0 assets/scenarios/*.ron` shows exactly 11 changed lines, all
      scalars; no spawn/obstacle/seed/destination line moved.
- [x] 7. Regenerate the three sidecars — all three scenes load hash-verified
      (`collision_scenes_load_and_verify`, `loads_v1_scene` pass):
      ```sh
      for f in assets/scenarios/technical_prototype_v1 \
               assets/scenarios/collision_mid_v1 \
               assets/scenarios/collision_sprite_v1; do
        sha256sum "$f.ron" | cut -d' ' -f1 > "$f.sha256"
      done
      ```
- [x] 8. **REWRITTEN by the orchestrator — the defect the first attempt found is
      resolved in Requirements.** Do NOT retune the ladder. Instead:
      `git checkout HEAD -- crates/mmd-engine/src/bench/policy.rs
      crates/mmd-engine/tests/benchmark_policy.rs tools/mmd-lab/src/gate.rs
      tools/mmd-lab/src/release.rs`, then add the **comment-only** header to the
      ladder constants in `bench/policy.rs` described in Requirements (frozen
      phase-0 history / live ceiling is `MAX_LIVE_AGENTS` / nothing above it is
      run). No constant, threshold or fixture string changes.
      — validate: `cargo test -p mmd-lab --no-fail-fast` green, and
      `git diff HEAD --stat -- crates/mmd-engine/src/bench/policy.rs
      crates/mmd-engine/tests/benchmark_policy.rs tools/mmd-lab/` shows a change
      to `policy.rs` comments only.
      — DONE, with ONE necessary deviation, flagged rather than hidden.
      Revert done: `git diff HEAD --stat -- tools/mmd-lab/` is **empty**, and
      `cargo test -p mmd-lab --no-fail-fast` → **231 passed / 0 failed**.
      Comment-only header added at the ladder; no constant, threshold or fixture
      string changed there.
      **Deviation — the diff is NOT comments-only.** A fact neither the ticket
      nor the first attempt had: `BenchPolicy::test_short()` reused the frozen
      ladder, and it is the one policy that *executes* the simulation. With the
      ladder reverted, `dry_bench_pins_exact_atlas_manifest_bytes` died on
      `Runtime(AgentCount { got: 10000, cap: 5000 })` — the frozen ladder cannot
      be run under the live ceiling. Resolved by splitting the two policies, the
      smallest change that keeps both truths:
      `production()` keeps the historical ladder verbatim, so every committed
      evidence artifact still validates; `test_short()` gains its own
      `TEST_SHORT_SCALE_COUNTS = [500, 1_000, 2_500, 5_000]` and
      `TEST_SHORT_GATE_AGENT_COUNT = 5_000`. `test_short` has
      `policy_id: "test-short-v1"` and writes no committed evidence, so nothing
      historical depends on the tiers it names. No threshold value changed.
      Touched: `bench/policy.rs`, `bench/mod.rs` (re-export),
      `tests/benchmark_policy.rs` (the one assertion that pinned `test_short` to
      the frozen ladder; it now also asserts every smoke tier is `<= MAX_LIVE_AGENTS`).
- [x] 9. ~~Retune the `tools/mmd-lab/src/gate.rs` fixture strings.~~ **DROPPED by
      the orchestrator** — folded into step 8's revert. The frozen lab's fixture
      strings mirror the frozen ladder; they are phase-0 history, not live text.
      — validate: `git diff HEAD --stat -- tools/mmd-lab/` empty.
- [x] 10. Rename the three tests; update `tests/validation_contract.rs` and the two
       close-doc rows. — `every_system_has_a_test` passes (24/24 in
       `validation_contract`), so the map, the source scan and the close doc agree.
- [x] 11. Sweep the commands: `AGENT.md`, `README.md`, `docs/05-testing.md`,
       `HANDOFF.md`, `docs/platform/macos-bootstrap.md`,
       `docs/lab/gpu-profiling.md`, `src/main.rs`.
       **Sweep done. Grep criterion AMENDED by the orchestrator** — the
       allowlist is the "Designated history, extended" bullet in Requirements.
       Every residue enumerated below falls inside that allowlist, so this step
       is satisfied; re-run the grep after step 8's revert and confirm nothing
       outside the allowlist remains.
       Swept to `--agents 5000`: the seven listed files, plus
       `docs/platform/windows-bootstrap.md` and `third_party/README.md` (the
       ticket lists only the macOS platform doc, but Context says "every doc and
       command that names 50 000"), plus the comments in `src/run.rs:583` and
       `tests/cli_contract.rs:59`. `tests/cli_contract.rs` stretch-cap strings
       became `stretch cap of 5000` / `1..=5000`.
       Re-run after step 8's revert, excluding exactly the amended allowlist
       (`docs/technical-prototype-results.md`, `docs/ADR/`, `artifacts/`,
       `.tmp/`, `.pi-subagents/`, `bench/policy.rs`, `tests/benchmark_policy.rs`,
       `tools/mmd-lab/**`, `lab/**`, `render/renderer.rs`,
       `tests/validation_contract.rs`, `testkit/rng.rs`):
       **0 residual hits.**
- [x] 12. Run the gate smoke, capture the new `hash=`, and record it in Outputs.
       — `cargo run -- run --agents 5000 --frames 300` → exit 0,
       `mode=window backend=vulkan tick=300 frames=300`, groups `[1250 x 4]`,
       `hash=864147ca3a0e09f7ebc5762b778fce193e705a2bc943ceaf67acf087581ee881`.
       Captured off a fully green tree (460 passed / 0 failed) and reproduced on
       three consecutive runs. Written into Outputs.
- [x] 13. `graphify install` (clears the 0.9.36/0.9.37 skill warning), then
       `graphify update .`. — both run; `git status` shows no `graphify-out/`
       entry (gitignored).

## Outputs

- `MAX_LIVE_AGENTS = 5_000`, enforced in one place, reachable from all three
  scenario families.
- Three retuned scenes + sidecars; four fixtures byte-identical.
- A frozen bench policy that no longer names a population above the ceiling.
- Six new contract tests; three renamed tests.
- **The re-pinned gate digest** — fill in on completion:
  - `cargo run -- run --scenario assets/scenarios/technical_prototype_v1.ron --frames 300` →
    `hash=864147ca3a0e09f7ebc5762b778fce193e705a2bc943ceaf67acf087581ee881`
  - Reproduced on three consecutive runs, and byte-identical via both
    `--agents 5000` and the explicit `--scenario …/technical_prototype_v1.ron`.
  - This value is what T1–T9 must reproduce. Previous value (superseded):
    `f647e7f590ed5814e4e61388e23836dfacb980217fb1762542ec3abfe85549b3`.
- `MIN_SCANNED_TESTS` raised to match the new scanned total.

## Validation

- [x] `cargo fmt --all -- --check` — clean, no diff
- [x] `cargo test --workspace --locked` — green, including the six new tests:
      **460 passed / 0 failed** across the workspace (`--no-fail-fast`)
- [x] `cargo clippy --workspace --all-targets --all-features -- -D warnings` — exit 0, no warnings
- [x] `nix flake check` — `all checks passed!`
- [x] `cargo run -p xtask -- bootstrap --check` — ok
- [x] `cargo run -p xtask -- shaders --check` — ok
- [x] `cargo run -p xtask -- atlases --check` — ok
- [x] `cargo run -- run --agents 5000 --frames 300` — exits 0; `hash=` captured
      into Outputs: `864147ca3a0e09f7ebc5762b778fce193e705a2bc943ceaf67acf087581ee881`
      (`mode=window backend=vulkan tick=300 frames=300`, groups `[1250 x 4]`)
- [x] `cargo run -- run --scenario assets/scenarios/collision_mid_v1.ron --frames 300` — exits 0
- [x] `cargo run -- run --scenario assets/scenarios/collision_sprite_v1.ron --frames 300` — exits 0
- [x] `cargo run -- run --agents 5001 --frames 1` — exit 1:
      `run failed: --agents 5001 exceeds the scenario's stretch cap of 5000: pick a
      count in 1..=5000, or use a scenario with a larger cap`
- [x] `cargo tree -e features | grep -c testkit` → `0`
- [x] `git diff --stat assets/scenarios/fixtures/` → empty (four fixtures byte-identical)
- [x] `no_perf_claim_in_docs` green after the `LIVE_DOCS` edits — also
      `perf_claim_scanner_catches_what_it_is_meant_to`, `every_system_has_a_test`
      and `gate_list_has_no_perf_thresholds` green
- [x] `graphify update .` run; `git status` shows no `graphify-out/` entry
- [ ] manual check: `cargo run -- run --agents 5000` — units read as chunky
      StarCraft-scale sprites, and two agents pressed together are edge-to-edge
      rather than overlapping art. Esc to quit.
      **Intentionally left unchecked — windowed GPU check, needs a human at a
      display.** Recorded as a human step in `artifacts/manual_test_checklist.md`
      under `## T0 starcraft-scale-cap-and-body`. Not run headless, and not
      allowed to gate this ticket.
