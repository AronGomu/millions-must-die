# T0: StarCraft-scale cap and body

**Plan:** `./ai-artifacts/PLAN_2026_08_09_horde-sim-headroom.md`
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
- `crates/mmd-engine/src/bench/policy.rs`: `SCALE_COUNTS` →
  `[500, 1_000, 2_500, 5_000]`, `GATE_AGENT_COUNT` → `5_000`,
  `STRETCH_AGENT_COUNT` → `5_000`, with a comment recording that the ceiling
  collapses the stretch tier onto the gate. `crates/mmd-engine/tests/benchmark_policy.rs`
  (lines ~166, ~167, ~302) follows. **No threshold value changes**;
  `gate_list_has_no_perf_thresholds` must stay green.
- `tools/mmd-lab/src/gate.rs` fixture strings that name `100_000` / `100000`
  (lines ~634, ~681, ~684, ~687) are retuned to `5000` so the frozen lab stops
  naming a population above the ceiling.
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

- [ ] 1. `graphify query "scenario agent count validation caps"` and
      `graphify explain "collision_radius_q8"` to confirm nothing outside the files
      listed in Inputs reads the constants.
- [ ] 2. Add `MAX_LIVE_AGENTS` to `scenario.rs`. Write the six new tests. Run — red.
- [ ] 3. Delete `COLLISION_SCENE_MAX_AGENTS`; route
      `validate_collision_scene_dims` through `MAX_LIVE_AGENTS`.
- [ ] 4. Add the ceiling check to `validate_fixture_dims` and to the v1 path. Prefer
      one shared `fn check_population(hard, stretch) -> Result<(), ScenarioError>`
      called by all three so a fourth family cannot skip it.
- [ ] 5. Change `V1_HARD_AGENTS`, `V1_STRETCH_AGENTS`, `V1_SPRITE_PX`,
      `V1_COLLISION_RADIUS_Q8`.
- [ ] 6. Edit the three `.ron` scenes. Only the six scalar fields change — spawn
      lists, obstacle lists, seeds and destinations are untouched.
- [ ] 7. Regenerate the three sidecars:
      ```sh
      for f in assets/scenarios/technical_prototype_v1 \
               assets/scenarios/collision_mid_v1 \
               assets/scenarios/collision_sprite_v1; do
        sha256sum "$f.ron" | cut -d' ' -f1 > "$f.sha256"
      done
      ```
- [ ] 8. Retune `bench/policy.rs` + `benchmark_policy.rs`. Change counts only; leave
      `GATE_P95_MS`, `GATE_P99_MS`, `NMAD_LIMIT` and every duration alone.
- [ ] 9. Retune the `tools/mmd-lab/src/gate.rs` fixture strings.
- [ ] 10. Rename the three tests; update `tests/validation_contract.rs` and the two
       close-doc rows.
- [ ] 11. Sweep the commands: `AGENT.md`, `README.md`, `docs/05-testing.md`,
       `HANDOFF.md`, `docs/platform/macos-bootstrap.md`,
       `docs/lab/gpu-profiling.md`, `src/main.rs`.
       Verify afterwards that
       `grep -rn "50000\|50_000\|100_000\|100000" --include=*.rs --include=*.md .`
       returns hits **only** in `docs/technical-prototype-results.md`, `docs/ADR/`,
       `ai-artifacts/PLAN_2026_08_08_zombie-collision/`, `.tmp/`, `.pi-subagents/`
       and `Cargo.lock` — all designated history or untracked notes.
- [ ] 12. Run the gate smoke, capture the new `hash=`, and record it in Outputs.
- [ ] 13. `graphify install` (clears the 0.9.36/0.9.37 skill warning), then
       `graphify update .`.

## Outputs

- `MAX_LIVE_AGENTS = 5_000`, enforced in one place, reachable from all three
  scenario families.
- Three retuned scenes + sidecars; four fixtures byte-identical.
- A frozen bench policy that no longer names a population above the ceiling.
- Six new contract tests; three renamed tests.
- **The re-pinned gate digest** — fill in on completion:
  - `cargo run -- run --scenario assets/scenarios/technical_prototype_v1.ron --frames 300` →
    `hash=________________________________________________________________`
  - This value is what T1–T9 must reproduce. Previous value (superseded):
    `f647e7f590ed5814e4e61388e23836dfacb980217fb1762542ec3abfe85549b3`.
- `MIN_SCANNED_TESTS` raised to match the new scanned total.

## Validation

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo test --workspace --locked` — green, including the six new tests
- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- [ ] `nix flake check`
- [ ] `cargo run -p xtask -- bootstrap --check`
- [ ] `cargo run -p xtask -- shaders --check`
- [ ] `cargo run -p xtask -- atlases --check`
- [ ] `cargo run -- run --agents 5000 --frames 300` — exits 0; `hash=` captured
      into Outputs
- [ ] `cargo run -- run --scenario assets/scenarios/collision_mid_v1.ron --frames 300` — exits 0
- [ ] `cargo run -- run --scenario assets/scenarios/collision_sprite_v1.ron --frames 300` — exits 0
- [ ] `cargo run -- run --agents 5001 --frames 1` — exits non-zero, message names the cap
- [ ] `cargo tree -e features | grep -c testkit` → 0
- [ ] `git diff --stat assets/scenarios/fixtures/` → empty
- [ ] `no_perf_claim_in_docs` green after the `LIVE_DOCS` edits
- [ ] `graphify update .` run; `git status` shows no `graphify-out/` entry
- [ ] manual check: `cargo run -- run --agents 5000` — units read as chunky
      StarCraft-scale sprites, and two agents pressed together are edge-to-edge
      rather than overlapping art. Esc to quit.
