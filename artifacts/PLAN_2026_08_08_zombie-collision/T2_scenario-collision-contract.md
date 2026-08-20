# T2: Scenario collision contract

**Plan:** `./artifacts/PLAN_2026_08_08_zombie-collision.md`
**Depends:** T1
**Commit outcome:** Every scenario — gate scene, all four fixtures, inline grids — carries a body radius and a separation strength as validated, hash-tracked data. Simulation behaviour is byte-identical because nothing reads the new fields yet.

## Context (self-contained)

- Goal: zombies stop passing through each other, via **soft separation steering** — a repulsion vector summed into the flow-field vector before the single move. Body radius is per-scenario data, not a hardcoded constant.
- This slice: the data contract only. Add two fields to the scenario, validate them, lock the gate scene's values, update every tracked `.ron` and its `.sha256` sidecar. No simulation change — the sim gains its collision parameter in T4.
- Out of scope here: `crates/mmd-engine/src/sim/**` (do not touch), `crates/mmd-engine/src/nav/**`, the renderer, any new scenario file, any CLI flag.
- Assumptions in force:
  - Fields are `u32` **Q8 fixed point** (256 = one cell, or the value 1.0), *not* `f32`, because `Scenario` and `ScenarioSpec` both `#[derive(..., PartialEq, Eq)]` and `f32` is not `Eq`. `q as f32 / 256.0` is exact — the divisor is a power of two.
  - Fields are **required** in the RON (no `#[serde(default)]`): a stale scenario must fail loudly rather than quietly run with no collision.
  - Inline `GridSpec` grids default to radius 0 so the surgical unit tests in `simulation.rs` stay bit-identical after T4.
  - Tracked fixtures get radius `32` (0.125 cell). Small on purpose: `fixture_dense_v1` has one-cell-wide corridors and `agents_reach_destination` asserts 90% arrivals within 400 ticks.

## Requirements

- `ScenarioSpec` and `Scenario` gain `collision_radius_q8: u32` and `separation_strength_q8: u32`, placed between `frame_count` and `obstacle_cells`.
- Public accessors on `Scenario`: `collision_radius_q8`, `collision_radius_cells`, `separation_strength_q8`, `separation_strength`.
- Validation rejects: radius above the cap, strength above the cap, and strength > 0 paired with radius == 0.
- `technical_prototype_v1` locks both values exactly, alongside its other frozen constants.
- All five tracked `.ron` files carry the fields and all five `.sha256` sidecars match.
- The whole suite stays green with **no behavioural drift** — the state hash after N ticks is unchanged, because nothing consumes the fields yet.

## Inputs

- `crates/mmd-engine/src/scenario.rs` (470 lines) — the contract. Relevant existing shape:
  - `pub struct ScenarioSpec { version, width, height, cell_size_px, sprite_size_px, hard_agent_count, stretch_agent_count, seed, destination, spawn_cells, atlas_count, direction_count, frame_count, obstacle_cells }`, `#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]`.
  - `pub struct Scenario { ... same fields, private ... }`, `#[derive(Debug, Clone, PartialEq, Eq)]`.
  - `Scenario::from_spec(doc: ScenarioSpec) -> Result<Self, ScenarioError>` calls, in order: `validate_version_and_dims(&doc, cells)?`, `validate_counts(&doc)?`, seed check, spawn-empty check, `normalize_obstacles`, obstacle-ratio check, destination check, spawn checks, `flood_reachable`.
  - `fn validate_version_and_dims(doc, cells)` holds a `let checks = [(doc.width, V1_WIDTH, "width"), ...]` array of `(u32, u32, &str)` applied only to `TECHNICAL_PROTOTYPE_V1`.
  - Existing V1 consts: `V1_WIDTH = 480`, `V1_HEIGHT = 270`, `V1_CELL_PX = 4`, `V1_SPRITE_PX = 30`, `V1_HARD_AGENTS = 50_000`, `V1_STRETCH_AGENTS = 100_000`, `V1_ATLASES = 4`, `V1_DIRS = 8`, `V1_FRAMES = 4`, `V1_DEST_X = 240`, `V1_DEST_Y = 135`.
  - `pub enum ScenarioError` is a `thiserror::Error` with variants including `InvalidDimension(String)`.
- `crates/mmd-engine/src/testkit/mod.rs` — `GridSpec` and `impl From<GridSpec> for ScenarioSpec`.
- `crates/mmd-engine/tests/scenario_contract.rs` — `fn fixture_spec() -> ScenarioSpec` at line ~211 is the baseline every negative test mutates via `..fixture_spec()`. It is the **only** literal `ScenarioSpec` that names every field; the rest use struct-update syntax and need no edit.
- The five tracked scenarios; each has `frame_count: 4,` on its own line immediately before `obstacle_cells:`:
  - `assets/scenarios/technical_prototype_v1.ron` (line 142)
  - `assets/scenarios/fixtures/fixture_small_v1.ron` (line 14)
  - `assets/scenarios/fixtures/fixture_corridor_v1.ron` (line 14)
  - `assets/scenarios/fixtures/fixture_dense_v1.ron` (line 14)
  - `assets/scenarios/fixtures/fixture_walled_v1.ron` (line 14)
- Sidecar format (verified): lowercase hex SHA-256 of the `.ron` bytes plus a single trailing `\n`. The loader `.trim()`s it.
- `tests/cli_contract.rs` needs **no** edit: it derives its throwaway scenarios from the real gate-scene bytes (`gate_scenario_bytes()`) and a `text.replace("seed: 5570183490285100849,", "seed: 0,")`, so it inherits the new fields automatically.
- **From Depends (T1):** `tests/validation_contract.rs` now reads `docs/CONTEXT.md` via `ROADMAP_DOC`; the whole workspace suite is green. Nothing else from T1 is consumed.

## Exact values to write

| Scenario | `collision_radius_q8` | cells | px | `separation_strength_q8` |
| --- | --- | --- | --- | --- |
| `assets/scenarios/technical_prototype_v1.ron` | `102` | 0.3984375 | 1.59 | `256` |
| `assets/scenarios/fixtures/fixture_small_v1.ron` | `32` | 0.125 | 0.5 | `256` |
| `assets/scenarios/fixtures/fixture_corridor_v1.ron` | `32` | 0.125 | 0.5 | `256` |
| `assets/scenarios/fixtures/fixture_dense_v1.ron` | `32` | 0.125 | 0.5 | `256` |
| `assets/scenarios/fixtures/fixture_walled_v1.ron` | `32` | 0.125 | 0.5 | `256` |
| `GridSpec::new` default (inline grids) | `0` | 0 | 0 | `0` |
| `fixture_spec()` in `scenario_contract.rs` | `64` | 0.25 | 1.0 | `256` |

## Code to add — copy verbatim

In `crates/mmd-engine/src/scenario.rs`, near the other public consts:

```rust
/// Q8 fixed-point scale for collision data: 256 units = one cell, or the
/// scalar value 1.0. Integer because [`Scenario`] derives `Eq`; exact in `f32`
/// because the divisor is a power of two.
pub const COLLISION_Q8: u32 = 256;
/// Largest body radius a scenario may declare: 8 cells (32 px at 4 px/cell).
pub const MAX_COLLISION_RADIUS_Q8: u32 = 2_048;
/// Largest separation weight a scenario may declare: 10.0.
pub const MAX_SEPARATION_STRENGTH_Q8: u32 = 2_560;
```

Private V1 locks, beside `V1_FRAMES`:

```rust
const V1_COLLISION_RADIUS_Q8: u32 = 102;
const V1_SEPARATION_STRENGTH_Q8: u32 = 256;
```

New error variant on `ScenarioError`:

```rust
#[error("invalid collision config: {0}")]
InvalidCollision(String),
```

New validator, called from `from_spec` immediately after `validate_counts(&doc)?;`:

```rust
/// Body radius and separation weight are bounded, and a weight without a body
/// is an authoring mistake rather than a silent no-op.
fn validate_collision(doc: &ScenarioSpec) -> Result<(), ScenarioError> {
    if doc.collision_radius_q8 > MAX_COLLISION_RADIUS_Q8 {
        return Err(ScenarioError::InvalidCollision(format!(
            "collision_radius_q8: got {}, max {MAX_COLLISION_RADIUS_Q8}",
            doc.collision_radius_q8
        )));
    }
    if doc.separation_strength_q8 > MAX_SEPARATION_STRENGTH_Q8 {
        return Err(ScenarioError::InvalidCollision(format!(
            "separation_strength_q8: got {}, max {MAX_SEPARATION_STRENGTH_Q8}",
            doc.separation_strength_q8
        )));
    }
    if doc.collision_radius_q8 == 0 && doc.separation_strength_q8 != 0 {
        return Err(ScenarioError::InvalidCollision(
            "separation_strength_q8 is set but collision_radius_q8 is 0; a \
             weight without a body pushes nothing"
                .into(),
        ));
    }
    Ok(())
}
```

Accessors on `impl Scenario`, beside `frame_count()`:

```rust
pub fn collision_radius_q8(&self) -> u32 {
    self.collision_radius_q8
}

/// Body radius in cells. Exact: the Q8 divisor is a power of two.
pub fn collision_radius_cells(&self) -> f32 {
    self.collision_radius_q8 as f32 / COLLISION_Q8 as f32
}

pub fn separation_strength_q8(&self) -> u32 {
    self.separation_strength_q8
}

/// Separation weight relative to the unit flow vector. Exact, as above.
pub fn separation_strength(&self) -> f32 {
    self.separation_strength_q8 as f32 / COLLISION_Q8 as f32
}
```

V1 lock — extend the existing `checks` array in `validate_version_and_dims`:

```rust
(
    doc.collision_radius_q8,
    V1_COLLISION_RADIUS_Q8,
    "collision_radius_q8",
),
(
    doc.separation_strength_q8,
    V1_SEPARATION_STRENGTH_Q8,
    "separation_strength_q8",
),
```

`GridSpec` in `crates/mmd-engine/src/testkit/mod.rs` — two new public fields plus a builder:

```rust
/// Body radius in 1/256 cell. `0` (the default) leaves separation inert, so an
/// inline grid stays a pure flow-field test unless it opts in.
pub collision_radius_q8: u32,
/// Separation weight in 1/256 (256 = 1.0). Must be 0 when the radius is 0.
pub separation_strength_q8: u32,
```

```rust
/// Opt an inline grid into collision.
pub fn with_collision(mut self, radius_q8: u32, strength_q8: u32) -> Self {
    self.collision_radius_q8 = radius_q8;
    self.separation_strength_q8 = strength_q8;
    self
}
```

`GridSpec::new` sets both to `0`; `impl From<GridSpec> for ScenarioSpec` forwards both.

## Editing the tracked scenarios

Run once from the workspace root. It inserts the two fields after the `frame_count: 4,` line and rewrites each sidecar:

```sh
python3 - <<'PY'
import hashlib, pathlib
targets = {
    "assets/scenarios/technical_prototype_v1.ron": (102, 256),
    "assets/scenarios/fixtures/fixture_small_v1.ron": (32, 256),
    "assets/scenarios/fixtures/fixture_corridor_v1.ron": (32, 256),
    "assets/scenarios/fixtures/fixture_dense_v1.ron": (32, 256),
    "assets/scenarios/fixtures/fixture_walled_v1.ron": (32, 256),
}
for rel, (radius, strength) in targets.items():
    p = pathlib.Path(rel)
    text = p.read_text()
    assert "collision_radius_q8" not in text, f"{rel} already patched"
    needle = "  frame_count: 4,\n"
    assert text.count(needle) == 1, f"{rel}: frame_count anchor not unique"
    text = text.replace(
        needle,
        needle
        + f"  collision_radius_q8: {radius},\n"
        + f"  separation_strength_q8: {strength},\n",
    )
    p.write_text(text)
    digest = hashlib.sha256(p.read_bytes()).hexdigest()
    p.with_suffix(".sha256").write_text(digest + "\n")
    print(f"{rel}: radius={radius} strength={strength} sha256={digest}")
PY
```

Expected output: five lines, each ending in a 64-character hex digest.

## TDD

1. **Red** — add the four new tests below to `crates/mmd-engine/tests/scenario_contract.rs` first. They fail to compile (the fields do not exist), which is the red state for a contract change.
2. **Green** — add fields, validator, accessors, V1 locks, `GridSpec` fields, then run the patch script so the tracked assets match.
3. **Refactor** — none beyond keeping `validate_collision` a single free function next to `validate_counts`.

## Test plan

All new tests go in `crates/mmd-engine/tests/scenario_contract.rs`.

| Test | Input | Expect |
| ---- | ----- | ------ |
| `gate_scene_locks_its_collision_tuning` | `Scenario::load_verified(gate_scenario_path())` | `collision_radius_q8() == 102`, `separation_strength_q8() == 256`, `(collision_radius_cells() - 0.398_437_5).abs() < f32::EPSILON`, `(separation_strength() - 1.0).abs() < f32::EPSILON` |
| `v1_rejects_a_retuned_collision_radius` | `ScenarioSpec` from the gate scene bytes with `collision_radius_q8` forced to `103` | `Err(ScenarioError::InvalidDimension(msg))` where `msg.contains("collision_radius_q8")` |
| `collision_radius_above_the_cap_is_refused` | `ScenarioSpec { collision_radius_q8: MAX_COLLISION_RADIUS_Q8 + 1, ..fixture_spec() }` | `Err(ScenarioError::InvalidCollision(msg))`, `msg.contains("collision_radius_q8")` |
| `separation_strength_without_a_body_is_refused` | `ScenarioSpec { collision_radius_q8: 0, separation_strength_q8: 256, ..fixture_spec() }` | `Err(ScenarioError::InvalidCollision(msg))`, `msg.contains("pushes nothing")` |
| `fixture_scenarios_are_hash_verified` (existing, `harness.rs`) | patched `.ron` + regenerated sidecars | still passes — proves the sidecars were regenerated, not forgotten |
| `loads_v1_scene` (existing) | patched gate scene | still passes |
| `determinism_holds_for_50k_agents` (existing) | — | still passes; behaviour must not drift in this ticket |

**Non-drift is checked by hand, not by a test.** No in-tree test can compare
against "the behaviour before this commit", and inventing one that compares two
runs of the *same* build would only restate determinism. Instead, record the
gate-scene state hash on the parent commit and again after the change — the
exact procedure is in Validation. It must be **identical**; T4 is the ticket
where it is allowed to change.

## Impl steps

- [x] 1. In `crates/mmd-engine/tests/scenario_contract.rs`, add the four new tests listed above; run `cargo test -p mmd-engine --test scenario_contract` and confirm it fails to compile (red). — confirmed: 14 compile errors (missing fields/variant), see report.
- [x] 2. In `crates/mmd-engine/src/scenario.rs`, add the three public consts `COLLISION_Q8`, `MAX_COLLISION_RADIUS_Q8`, `MAX_SEPARATION_STRENGTH_Q8` verbatim from above.
- [x] 3. Add the private consts `V1_COLLISION_RADIUS_Q8 = 102` and `V1_SEPARATION_STRENGTH_Q8 = 256` beside `V1_FRAMES`.
- [x] 4. Add `collision_radius_q8: u32` and `separation_strength_q8: u32` to `ScenarioSpec`, between `frame_count` and `obstacle_cells`, each with a one-line doc comment naming its unit.
- [x] 5. Add the same two fields to `Scenario` in the same position.
- [x] 6. Add the `InvalidCollision(String)` variant to `ScenarioError` verbatim from above.
- [x] 7. Add `fn validate_collision(doc: &ScenarioSpec) -> Result<(), ScenarioError>` verbatim from above, placed directly after `fn validate_counts`.
- [x] 8. In `Scenario::from_spec`, insert `validate_collision(&doc)?;` on the line immediately after `validate_counts(&doc)?;`.
- [x] 9. In `Scenario::from_spec`'s `Ok(Self { ... })` construction, forward both new fields from `doc`.
- [x] 10. Add the four accessors verbatim from above to `impl Scenario`, immediately after `pub fn frame_count`.
- [x] 11. Extend the `checks` array in `validate_version_and_dims` with the two tuples given above.
- [x] 12. In `crates/mmd-engine/src/testkit/mod.rs`, add the two `GridSpec` fields, initialise both to `0` in `GridSpec::new`, add `with_collision`, and forward both in `impl From<GridSpec> for ScenarioSpec`.
- [x] 13. In `crates/mmd-engine/tests/scenario_contract.rs`, add `collision_radius_q8: 64,` and `separation_strength_q8: 256,` to the `fixture_spec()` literal.
- [x] 14. Run the `python3` patch script above from the workspace root; confirm five lines of output, each with a 64-hex digest. — confirmed: 5 lines printed, each sha256 65 bytes (64 hex + \n).
- [x] 15. Run `git diff --stat assets/scenarios` and confirm exactly ten files changed (five `.ron`, five `.sha256`). — confirmed: "10 files changed, 15 insertions(+), 5 deletions(-)".
- [x] 16. Before committing, capture the non-drift evidence. With the working tree dirty, run `cargo run -- run --agents 2000 --frames 120 | grep 'clean exit'` and save the `hash=` value as **after**. Then `git stash push --include-untracked`, run the same command, save it as **before**, and `git stash pop`. The two hashes must be identical. — confirmed identical: `hash=1eae534eab58abf31e139cbe399cf17b73ffe00cf02408e1df236ded7519e52b` both before and after.
- [x] 17. Run `cargo test -p mmd-engine --test scenario_contract --test harness --test simulation` and confirm green. — 18+11+15 passed after one repair (see Assumptions in worker report).
- [x] 18. Run the full validation list below.

## Outputs

- Files touched: `crates/mmd-engine/src/scenario.rs`, `crates/mmd-engine/src/testkit/mod.rs`, `crates/mmd-engine/tests/scenario_contract.rs`, five `assets/scenarios/**/*.ron`, five `assets/scenarios/**/*.sha256`.
- Public API change (quoted verbatim by T3/T4/T5):
  - `mmd_engine::scenario::COLLISION_Q8: u32`
  - `mmd_engine::scenario::MAX_COLLISION_RADIUS_Q8: u32`
  - `mmd_engine::scenario::MAX_SEPARATION_STRENGTH_Q8: u32`
  - `mmd_engine::scenario::ScenarioError::InvalidCollision(String)`
  - `Scenario::collision_radius_q8(&self) -> u32`
  - `Scenario::collision_radius_cells(&self) -> f32`
  - `Scenario::separation_strength_q8(&self) -> u32`
  - `Scenario::separation_strength(&self) -> f32`
  - `ScenarioSpec { .., collision_radius_q8: u32, separation_strength_q8: u32, .. }`
  - `GridSpec::with_collision(self, radius_q8: u32, strength_q8: u32) -> GridSpec`
- Behaviour change: none. Data only.
- Migrate: every scenario `.ron` outside the repo is now invalid until it declares both fields — intended.

## Validation

- [x] `cargo test -p mmd-engine --test scenario_contract` → green, four new tests visible in the output — 18 passed incl. gate_scene_locks_its_collision_tuning, v1_rejects_a_retuned_collision_radius, collision_radius_above_the_cap_is_refused, separation_strength_without_a_body_is_refused.
- [x] `cargo test -p mmd-engine --test harness` → green (`fixture_scenarios_are_hash_verified` proves the sidecars) — 11 passed, 1 ignored (unrelated child-process test).
- [x] `cargo fmt --all -- --check` → exit 0 — confirmed (after one `cargo fmt --all` pass).
- [x] `cargo clippy --workspace --all-targets --all-features -- -D warnings` → exit 0 — confirmed, "Finished `dev` profile".
- [x] `MMD_REQUIRE_GPU=1 cargo test --workspace --locked` → green — confirmed, no FAILED/error across full workspace output, exit 0.
- [x] `cargo run -- run --agents 50000 --frames 300` → exit 0, `run: clean exit ...` printed — confirmed: `hash=2517e839224048345fe891312d97ae000fd76523b43954d1042348eea75b8783`, exit 0.
- [x] manual check: `grep -A2 'frame_count' assets/scenarios/technical_prototype_v1.ron` shows `collision_radius_q8: 102,` and `separation_strength_q8: 256,` — confirmed.
- [x] manual check: the before/after `hash=` values from impl step 16 are **identical** — this ticket adds data, it must not move an agent — confirmed identical: `1eae534eab58abf31e139cbe399cf17b73ffe00cf02408e1df236ded7519e52b`.
- [x] app functional — no broken path from this slice — `cargo run -- run --agents 50000 --frames 300` completed clean exit as shown above.
- [x] commit msg draft: `feat(scenario): declare agent body radius and separation strength as scenario data`
