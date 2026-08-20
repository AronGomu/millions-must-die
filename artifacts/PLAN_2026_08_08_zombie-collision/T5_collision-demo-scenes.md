# T5: `collision_scene_v1` demo scenes

**Plan:** `./artifacts/PLAN_2026_08_08_zombie-collision.md`
**Depends:** T4
**Commit outcome:** Two runnable, hash-tracked scenes show collision at two scales — 10 000 agents with a body box, and 1 200 agents whose 30 px sprites genuinely keep off each other.

## Context (self-contained)

- Goal: zombies stop passing through each other, via soft separation steering already wired into the tick. Body radius is scenario data.
- This slice: make the effect visible. The gate scene's body radius is 0.398 cell ≈ 1.6 px against a 30 px sprite, so at 50 000 agents the sprites still overlap heavily — that is geometry, not a defect: 50 000 sprites × 900 px = 45 000 000 px against a 2 073 600 px screen. Sprite-accurate separation therefore needs a low-count scene, and the middle ground deserves one too.
- Out of scope here: touching `technical_prototype_v1` or any `fixture_*` file; changing `crates/mmd-engine/src/sim/**`; docs and the systems map (T6); any renderer change.
- Assumptions in force:
  - No scenario generator exists in-tree; fixtures and the gate scene are tracked assets with `.sha256` sidecars. These two ship the same way, with their generator committed for provenance. **No new `--check` gate.**
  - Both scenes reuse the gate scene's screen geometry (480×270 cells at 4 px, 30 px sprites, destination at the centre) so the three tunings are visually comparable.
  - Jam at the funnel is accepted behaviour.

## Requirements

- A third scenario version family, `collision_scene_v1`, that locks the screen geometry and the destination but leaves the population and body radius free within caps.
- A collision scene must declare a nonzero body radius — a bodyless "collision scene" is an authoring mistake.
- Two tracked scenes with matching sidecars.
- The generator is committed and rerunnable, and rerunning it must reproduce byte-identical files.
- A behavioural test proving the sprite-scale scene actually pulls its agents out of deep overlap.

## Inputs

- `crates/mmd-engine/src/scenario.rs`. Existing shape that this ticket extends:
  - `pub const TECHNICAL_PROTOTYPE_V1: &str = "technical_prototype_v1";`, `pub const FIXTURE_VERSION_PREFIX: &str = "fixture_";`
  - V1 consts `V1_WIDTH = 480`, `V1_HEIGHT = 270`, `V1_CELL_PX = 4`, `V1_SPRITE_PX = 30`, `V1_ATLASES = 4`, `V1_DIRS = 8`, `V1_FRAMES = 4`, `V1_DEST_X = 240`, `V1_DEST_Y = 135`
  - `fn validate_version_and_dims(doc, cells)` opens with `if doc.version.starts_with(FIXTURE_VERSION_PREFIX) { return validate_fixture_dims(doc, cells); }` then `if doc.version != TECHNICAL_PROTOTYPE_V1 { return Err(ScenarioError::UnsupportedVersion(..)); }` then a `checks` array of `(u32, u32, &str)` triples.
  - `fn validate_counts(doc)` locks `atlas_count/direction_count/frame_count` for every family and locks `hard_agent_count/stretch_agent_count` for everything that is not a fixture, via `let is_fixture = doc.version.starts_with(FIXTURE_VERSION_PREFIX);` and `renderer.iter().chain(workload.iter().filter(|_| !is_fixture))`.
  - `Scenario::from_spec` applies the exact-20% obstacle ratio only when `doc.version == TECHNICAL_PROTOTYPE_V1`; every other family only needs at least one free cell.
- `crates/mmd-engine/src/testkit/fixtures.rs` — holds `FIXTURE_DIR`, `GATE_SCENARIO`, the four `FIXTURE_*` names, `ALL_FIXTURES`, `fixture_path(name)`, `gate_scenario_path()`. Re-exported from `crates/mmd-engine/src/testkit/mod.rs` by an explicit `pub use fixtures::{...}` list that must be extended.
- `crates/mmd-engine/src/testkit/mod.rs` — `ScenarioSource::path(path)` loads and hash-verifies any scenario file; `Harness::builder(source)` is the generic entry point.
- **From Depends (T2), quoted:**
  - Scenario fields `collision_radius_q8: u32` and `separation_strength_q8: u32` sit between `frame_count` and `obstacle_cells` in both `ScenarioSpec` and the `.ron` files.
  - `pub const COLLISION_Q8: u32 = 256;`, `MAX_COLLISION_RADIUS_Q8 = 2_048`, `MAX_SEPARATION_STRENGTH_Q8 = 2_560`.
  - `ScenarioError::InvalidCollision(String)` exists; `fn validate_collision(doc: &ScenarioSpec)` is called from `from_spec` right after `validate_counts(&doc)?`.
  - `Scenario::collision_radius_q8()`, `collision_radius_cells()`, `separation_strength_q8()`, `separation_strength()`.
  - Sidecar format: lowercase hex SHA-256 of the `.ron` bytes plus one trailing `\n`.
- **From Depends (T4), quoted:**
  - `mmd_engine::sim::CollisionParams` with `radius_cells: f32`, `strength: f32`, `enabled()`, `bin_size_cells()`, `from_scenario(&Scenario)`.
  - `Simulation::collision(&self) -> CollisionParams`, reachable in tests as `h.sim().collision()`.
  - `crates/mmd-engine/tests/separation.rs` exists and holds 15 tests.
  - The tick blends `v = flow + strength * separation`, renormalises, and falls back to the pure descent step when the blended step is not walkable.

## Scene specification — exact

Both scenes: `width: 480`, `height: 270`, `cell_size_px: 4`, `sprite_size_px: 30`,
`destination: (x: 240, y: 135)`, `atlas_count: 4`, `direction_count: 8`,
`frame_count: 4`, `separation_strength_q8: 256`, `stretch_agent_count: 20000`.

| File | version | `hard_agent_count` | `collision_radius_q8` | radius | seed |
| --- | --- | --- | --- | --- | --- |
| `assets/scenarios/collision_mid_v1.ron` | `collision_scene_v1` | `10000` | `320` | 1.25 cells = 5 px | `7355608251463129073` |
| `assets/scenarios/collision_sprite_v1.ron` | `collision_scene_v1` | `1200` | `960` | 3.75 cells = 15 px = half a sprite | `4411920776318552601` |

Spawn cells (both scenes, 128 of them): `x = 2`, `y = 8, 10, 12, … 262`.

Obstacles (both scenes, 4 800 cells = 3.7%): 4×4 pillars. For every
`by in range(0, 270, 18)` and `bx in range(0, 480, 24)`, block the sixteen cells
`(bx + dx, by + dy)` for `dx in 0..4`, `dy in 0..4`, offset by `+12` in x and
`+9` in y — i.e. x in `bx+12 ..= bx+15`, y in `by+9 ..= by+12`, dropping any
cell that falls outside the grid. This keeps the destination free (240 is not
congruent to 12..15 mod 24) and every spawn free (x = 2 is never a pillar
column), and leaves 20-cell corridors so the field stays fully connected.

Density check, recorded so the tuning is not mistaken for arbitrary: free cells
= 129 600 − 4 800 = 124 800. Mid scene body area = 10 000 × π × 1.25² ≈ 49 100
cells (39%). Sprite scene = 1 200 × π × 3.75² ≈ 53 000 cells (42%). Both sit
well under the 90.7% hex-packing limit, so neither scene is gridlocked by
construction — while the funnel at the destination still jams, as intended.

## Code to add — copy verbatim

In `crates/mmd-engine/src/scenario.rs`, beside `FIXTURE_VERSION_PREFIX`:

```rust
/// Version id for the collision demo family: the gate scene's screen geometry
/// and destination, with a free population and a free body radius.
///
/// It exists because [`TECHNICAL_PROTOTYPE_V1`] freezes the 50k/100k workload
/// and the exact-20% obstacle ratio, and the `fixture_` caps (65 536 cells)
/// cannot express a full-screen scene.
pub const COLLISION_SCENE_V1: &str = "collision_scene_v1";

/// A collision scene stays legible: it demonstrates bodies, it is not a second
/// horde workload.
pub const COLLISION_SCENE_MAX_AGENTS: u32 = 20_000;
```

New validator, placed directly after `validate_fixture_dims`:

```rust
/// Screen geometry is locked so the demo scenes stay comparable with the gate
/// scene; population and body radius are the point of the family and stay free
/// within caps.
fn validate_collision_scene_dims(doc: &ScenarioSpec) -> Result<(), ScenarioError> {
    let locked = [
        (doc.width, V1_WIDTH, "width"),
        (doc.height, V1_HEIGHT, "height"),
        (doc.cell_size_px, V1_CELL_PX, "cell_size_px"),
        (doc.sprite_size_px, V1_SPRITE_PX, "sprite_size_px"),
        (doc.destination.x, V1_DEST_X, "destination.x"),
        (doc.destination.y, V1_DEST_Y, "destination.y"),
    ];
    for (got, want, name) in locked {
        if got != want {
            return Err(ScenarioError::InvalidDimension(format!(
                "{name}: got {got}, want {want}"
            )));
        }
    }
    if doc.hard_agent_count == 0 {
        return Err(ScenarioError::InvalidDimension(
            "hard_agent_count must be > 0".into(),
        ));
    }
    if doc.stretch_agent_count < doc.hard_agent_count {
        return Err(ScenarioError::InvalidDimension(format!(
            "stretch_agent_count {} below hard_agent_count {}",
            doc.stretch_agent_count, doc.hard_agent_count
        )));
    }
    for (got, name) in [
        (doc.hard_agent_count, "hard_agent_count"),
        (doc.stretch_agent_count, "stretch_agent_count"),
    ] {
        if got > COLLISION_SCENE_MAX_AGENTS {
            return Err(ScenarioError::InvalidDimension(format!(
                "collision scene {name} {got} exceeds cap {COLLISION_SCENE_MAX_AGENTS}"
            )));
        }
    }
    if doc.collision_radius_q8 == 0 {
        return Err(ScenarioError::InvalidCollision(
            "a collision scene must declare a nonzero collision_radius_q8".into(),
        ));
    }
    Ok(())
}
```

Wire it into `validate_version_and_dims`, immediately after the fixture branch
and before the `TECHNICAL_PROTOTYPE_V1` check:

```rust
    if doc.version == COLLISION_SCENE_V1 {
        return validate_collision_scene_dims(doc);
    }
```

In `validate_counts`, replace the single `is_fixture` binding and its use with:

```rust
    // Families that pick their own population: the small fixtures, and the
    // collision demo scenes whose whole purpose is a different agent count.
    let free_workload = doc.version.starts_with(FIXTURE_VERSION_PREFIX)
        || doc.version == COLLISION_SCENE_V1;
    let checks = renderer
        .iter()
        .chain(workload.iter().filter(|_| !free_workload));
```

In `crates/mmd-engine/src/testkit/fixtures.rs`, append:

```rust
/// Full-screen collision demo: 10 000 agents with a 1.25-cell body.
pub const COLLISION_MID_SCENE: &str = "assets/scenarios/collision_mid_v1.ron";
/// Full-screen collision demo at sprite scale: 1 200 agents with a 3.75-cell
/// body, so a 30 px sprite keeps clear of its neighbours.
pub const COLLISION_SPRITE_SCENE: &str = "assets/scenarios/collision_sprite_v1.ron";
/// Both collision demo scenes, for suites that assert across the family.
pub const ALL_COLLISION_SCENES: &[&str] = &[COLLISION_MID_SCENE, COLLISION_SPRITE_SCENE];

/// Absolute path to a scenario given workspace-relative.
pub fn scene_path(rel: &str) -> PathBuf {
    crate::workspace_root().join(rel)
}
```

and extend the re-export list in `crates/mmd-engine/src/testkit/mod.rs` to:

```rust
pub use fixtures::{
    ALL_COLLISION_SCENES, ALL_FIXTURES, COLLISION_MID_SCENE, COLLISION_SPRITE_SCENE,
    FIXTURE_CORRIDOR_V1, FIXTURE_DENSE_V1, FIXTURE_DIR, FIXTURE_SMALL_V1, FIXTURE_WALLED_V1,
    GATE_SCENARIO, fixture_path, gate_scenario_path, scene_path,
};
```

## Generator — commit at `tools/scenegen/gen_collision_scenes.py`

```python
#!/usr/bin/env python3
"""Generate the tracked collision demo scenes and their sha256 sidecars.

Deterministic and idempotent: rerunning it must leave `git status` clean.
The scenes are tracked assets, like the gate scene and the fixtures; this
script exists for provenance, not as a build step.

Usage (from the workspace root):
    python3 tools/scenegen/gen_collision_scenes.py
"""

import hashlib
import pathlib

WIDTH, HEIGHT = 480, 270
CELL_PX, SPRITE_PX = 4, 30
DEST = (240, 135)

SCENES = [
    # (filename, hard_agents, collision_radius_q8, seed)
    ("collision_mid_v1.ron", 10_000, 320, 7355608251463129073),
    ("collision_sprite_v1.ron", 1_200, 960, 4411920776318552601),
]


def obstacle_cells():
    cells = set()
    for by in range(0, HEIGHT, 18):
        for bx in range(0, WIDTH, 24):
            for dy in range(4):
                for dx in range(4):
                    x, y = bx + 12 + dx, by + 9 + dy
                    if x < WIDTH and y < HEIGHT:
                        cells.add(x + y * WIDTH)
    return sorted(cells)


def spawn_cells():
    return [(2, y) for y in range(8, 264, 2)]


def render(name, hard, radius_q8, seed, obstacles, spawns):
    lines = [
        "(",
        f'  version: "collision_scene_v1",',
        f"  width: {WIDTH},",
        f"  height: {HEIGHT},",
        f"  cell_size_px: {CELL_PX},",
        f"  sprite_size_px: {SPRITE_PX},",
        f"  hard_agent_count: {hard},",
        "  stretch_agent_count: 20000,",
        f"  seed: {seed},",
        f"  destination: (x: {DEST[0]}, y: {DEST[1]}),",
        "  spawn_cells: [",
    ]
    lines += [f"    (x: {x}, y: {y})," for x, y in spawns]
    lines += [
        "  ],",
        "  atlas_count: 4,",
        "  direction_count: 8,",
        "  frame_count: 4,",
        f"  collision_radius_q8: {radius_q8},",
        "  separation_strength_q8: 256,",
        "  obstacle_cells: [",
    ]
    for i in range(0, len(obstacles), 20):
        lines.append("    " + ",".join(str(c) for c in obstacles[i : i + 20]) + ",")
    lines += ["  ],", ")", ""]
    return "\n".join(lines)


def main():
    out_dir = pathlib.Path("assets/scenarios")
    assert out_dir.is_dir(), "run from the workspace root"
    obstacles = obstacle_cells()
    spawns = spawn_cells()
    dest_idx = DEST[0] + DEST[1] * WIDTH
    assert dest_idx not in set(obstacles), "destination must be free"
    blocked = set(obstacles)
    for x, y in spawns:
        assert x + y * WIDTH not in blocked, f"spawn ({x}, {y}) must be free"
    for name, hard, radius_q8, seed in SCENES:
        text = render(name, hard, radius_q8, seed, obstacles, spawns)
        path = out_dir / name
        path.write_text(text)
        digest = hashlib.sha256(path.read_bytes()).hexdigest()
        path.with_suffix(".sha256").write_text(digest + "\n")
        print(f"{path}: agents={hard} radius_q8={radius_q8} sha256={digest}")


if __name__ == "__main__":
    main()
```

## TDD

1. **Red** — add the five tests below. They fail: `COLLISION_SCENE_V1` does not exist and the scene files are absent.
2. **Green** — add the consts, the validator, the `validate_counts` change, the testkit exports, then run the generator.
3. **Refactor** — none.

## Test plan

In `crates/mmd-engine/tests/scenario_contract.rs`:

| Test | Input | Expect |
| ---- | ----- | ------ |
| `collision_scenes_load_and_verify` | `Scenario::load_verified(scene_path(COLLISION_MID_SCENE))` and the sprite scene | both `Ok`; mid has `hard_agent_count() == 10_000` and `collision_radius_q8() == 320`; sprite has `1_200` and `960`; both report `version() == COLLISION_SCENE_V1` |
| `collision_scene_locks_the_screen_geometry` | the mid scene's parsed `ScenarioSpec` with `width` forced to `481` | `Err(ScenarioError::InvalidDimension(msg))`, `msg.contains("width")` |
| `collision_scene_refuses_a_bodyless_scene` | same spec with `collision_radius_q8: 0` and `separation_strength_q8: 0` | `Err(ScenarioError::InvalidCollision(msg))`, `msg.contains("nonzero collision_radius_q8")` |
| `collision_scene_caps_its_population` | same spec with `hard_agent_count: COLLISION_SCENE_MAX_AGENTS + 1` and `stretch_agent_count: COLLISION_SCENE_MAX_AGENTS + 1` | `Err(ScenarioError::InvalidDimension(msg))`, `msg.contains("exceeds cap")` |
| `v1_geometry_stays_frozen_against_the_fixture_relaxation` (existing) | — | still passes: the new family must not loosen V1 |

In `crates/mmd-engine/tests/separation.rs`:

| Test | Input | Expect |
| ---- | ----- | ------ |
| `sprite_scene_pulls_agents_out_of_deep_overlap` | `Harness::builder(ScenarioSource::path(scene_path(COLLISION_SPRITE_SCENE))).build()`; count "deep" pairs (centre distance `< radius_cells`, i.e. half contact) at ticks 1, 100, 200 and 300 | `deep_before > 0` (they start stacked ~9 per spawn cell); `deep_at_300 * 2 <= deep_before`; no sample taken after tick 1 exceeds `deep_before`; every position finite |
| `mid_scene_reports_its_tuning` | `Harness` on the mid scene | `alive_count() == 10_000`; `h.sim().collision().enabled()`; `(radius_cells - 1.25).abs() < 1e-6`; `(strength - 1.0).abs() < 1e-6` |
| `collision_scene_agents_never_enter_an_obstacle` | mid scene, 200 ticks, sampling every tick via `common::Tracker` | `t.obstacle_samples() == 0` and `t.bounds_violations() == 0` |

`sprite_scene_pulls_agents_out_of_deep_overlap` is O(n²) at n = 1 200 (719 400
pairs per sample) — that is fine and deliberate: the claim is about pairs, so count
pairs rather than approximate.

### CORRECTION (parent, after the first T5 attempt) — read this before writing the test

The original row asserted `deep_after * 10 <= deep_before` (a 90 % reduction).
**That constant was authored, not measured, and it is not achievable against this
ticket's own locked scene spec.** Measured decay on the real
`collision_sprite_v1.ron`, unmodified engine, same harness:

```text
tick    1 -> 16229      tick  200 ->  7030      tick  600 ->  4960
tick   50 -> 14386      tick  250 ->  6511      tick  900 ->  5315
tick  100 -> 10920      tick  300 ->  6104      tick 1200 ->  6248
```

The count falls to ~38 % of its tick-1 value, plateaus, and then **rises** past
tick ~600 as agents recycle to the spawn cells and restack — which is the
destination-funnel jam this plan already accepts as real behaviour (plan A6).
No tick horizon reaches 10 %.

The bar is therefore `deep_at_300 * 2 <= deep_before` — deep overlap must at
least **halve** — plus the new "no later sample exceeds `deep_before`" clause,
which is what actually catches the rise-again pathology inside the 300-tick
window the scene is specified for.

Two rules on this correction, both binding:

- **Do not tune the scene, the radius, the strength, the spawn layout, or the
  tick count to make a number pass.** The scene spec above is locked. If the
  2× bar is not met, that is a real finding — report `failed` with the measured
  numbers. Do not loosen the constant again.
- **Do not add a radius-0 control arm to this test.** `validate_collision_scene_dims`
  correctly rejects `collision_radius_q8 == 0` for the `collision_scene_v1`
  family, and that rule must not be loosened. The causal claim — that separation,
  not flow-field dispersion, is what unstacks agents — is already proven at unit
  scale by T4's `a_released_stack_spreads_apart` and
  `coincident_agents_separate_on_the_first_tick`, both green. This test's job is
  only to show the effect is material at sprite scale.

`collision_scene_agents_never_enter_an_obstacle` uses the shared `Tracker`, so
`crates/mmd-engine/tests/separation.rs` must gain `mod common;` at the top of
the file (directly under the `//!` header, before the `use` lines) and
`use common::Tracker;`. That module already exists at
`crates/mmd-engine/tests/common/mod.rs`, is compiled per test binary, and
carries `#![allow(dead_code)]` for exactly this reason. The imports this ticket
adds are `use mmd_engine::testkit::{COLLISION_MID_SCENE, COLLISION_SPRITE_SCENE, ScenarioSource, scene_path};`.

## Impl steps

- [x] 1. Add the five new tests above to `crates/mmd-engine/tests/scenario_contract.rs` and `crates/mmd-engine/tests/separation.rs`; run `cargo test -p mmd-engine --test scenario_contract` and confirm red.
- [x] 2. Add `COLLISION_SCENE_V1` and `COLLISION_SCENE_MAX_AGENTS` to `crates/mmd-engine/src/scenario.rs`, verbatim.
- [x] 3. Add `fn validate_collision_scene_dims` verbatim, directly after `fn validate_fixture_dims`.
- [x] 4. Insert the `COLLISION_SCENE_V1` branch into `validate_version_and_dims`, after the fixture branch.
- [x] 5. Replace the `is_fixture` binding in `validate_counts` with the `free_workload` version given above.
- [x] 6. Append `COLLISION_MID_SCENE`, `COLLISION_SPRITE_SCENE`, `ALL_COLLISION_SCENES` and `fn scene_path` to `crates/mmd-engine/src/testkit/fixtures.rs`.
- [x] 7. Replace the `pub use fixtures::{...}` list in `crates/mmd-engine/src/testkit/mod.rs` with the extended version given above.
- [x] 8. Create `tools/scenegen/gen_collision_scenes.py` with the script above, verbatim, and `chmod +x` it.
- [x] 9. Run `python3 tools/scenegen/gen_collision_scenes.py` from the workspace root; expect two lines of output, each with a 64-hex digest.
- [x] 10. Run it a second time and confirm `git status --short assets/scenarios` shows the same two `.ron` and two `.sha256` files as before and no further churn — the generator must be idempotent.
- [x] 11. Run `cargo test -p mmd-engine --test scenario_contract --test separation` → green, with `sprite_scene_pulls_agents_out_of_deep_overlap` written against the **CORRECTION** section above (2× bar + no-later-sample-exceeds clause), not the original 10× bar. Evidence: `scenario_contract` 22 passed / 0 failed; `separation` 19 passed / 0 failed. Measured deep pairs `[(1, 16229), (100, 10920), (200, 7030), (300, 6104)]` — `6104 * 2 = 12208 <= 16229`, and no sample after tick 1 exceeds 16229.
- [x] 12. Run `cargo run -- run --scenario assets/scenarios/collision_sprite_v1.ron --frames 300` and confirm exit 0 with a `run: clean exit ...` line.
- [x] 13. Run `cargo run -- run --scenario assets/scenarios/collision_mid_v1.ron --frames 300` and confirm the same.
- [x] 14. Run the full validation list below. Every line is checked except the interactive window check, which a headless worker cannot perform.

## Outputs

- Files touched: `crates/mmd-engine/src/scenario.rs`, `crates/mmd-engine/src/testkit/fixtures.rs`, `crates/mmd-engine/src/testkit/mod.rs`, `crates/mmd-engine/tests/scenario_contract.rs`, `crates/mmd-engine/tests/separation.rs`, `tools/scenegen/gen_collision_scenes.py` (new), `assets/scenarios/collision_mid_v1.ron` (new), `assets/scenarios/collision_mid_v1.sha256` (new), `assets/scenarios/collision_sprite_v1.ron` (new), `assets/scenarios/collision_sprite_v1.sha256` (new).
- Public API added (quoted verbatim by T6):
  - `mmd_engine::scenario::COLLISION_SCENE_V1: &str = "collision_scene_v1"`
  - `mmd_engine::scenario::COLLISION_SCENE_MAX_AGENTS: u32 = 20_000`
  - `mmd_engine::testkit::COLLISION_MID_SCENE`, `COLLISION_SPRITE_SCENE`, `ALL_COLLISION_SCENES`, `scene_path(rel: &str) -> PathBuf`
- Behaviour change: two new runnable scenes. No existing scenario changes.
- Migrate / config: none.

## Validation

- [x] `cargo test -p mmd-engine --test scenario_contract` → green, four new tests visible (22 total, 22 passed)
- [x] `cargo test -p mmd-engine --test separation` → all pass (19 incl. the corrected deep-overlap test): `test result: ok. 19 passed; 0 failed; 0 ignored`
- [x] `cargo fmt --all -- --check` → exit 0
- [x] `cargo clippy --workspace --all-targets --all-features -- -D warnings` → exit 0
- [x] `MMD_REQUIRE_GPU=1 cargo test --workspace --locked` → green, exit 0; every `test result:` line reports `0 failed`. Ignored tests are the pre-existing host-GPU/SDL3 golden set, the `print_state_hash_for_child_process` entry point and the `alloc_guard` doctest — this slice adds no ignored test.
- [x] `cargo run -- run --agents 50000 --frames 300` → exit 0 (the gate scene is untouched); hash `130e3047228c4813156d68641567971cda4ab8ef3f4e5e8c71d7088c7f1e8ba7` reproduced — superseded by T7 (corner-pocket fix): `f647e7f590ed5814e4e61388e23836dfacb980217fb1762542ec3abfe85549b3`
- [x] `cargo run -- run --scenario assets/scenarios/collision_sprite_v1.ron --frames 300` → exit 0, `run: clean exit ... tick=300 frames=300 hash=1909d6c085f74b3490a5cb0548b7b5744b68df605e7357a55a57aa6986b8223d` — superseded by T7 (corner-pocket fix): `0d13037832c37a90ec628f8ac9b94d23100ce1365405d11d7d4fb5546fec90d3`
- [x] `cargo run -- run --scenario assets/scenarios/collision_mid_v1.ron --frames 300` → exit 0, `run: clean exit ... tick=300 frames=300 hash=9b0691550b2a0b3af0a4d58c15662d2631cadf8ad5c8a65e402422facd633e91` — superseded by T7 (corner-pocket fix): `861ccf228a673c8a3c74718ed3891c0462aabbed426d9f434d87ea81182d1988`
- [ ] manual check: `cargo run -- run --scenario assets/scenarios/collision_sprite_v1.ron` in a window — the 1 200 sprites must be visibly separated rather than merged into blobs, except where they pile at the centre destination. Esc to quit. **Not run — headless worker, no interactive window check performed.**
- [x] manual check: rerunning the generator leaves the working tree clean (confirmed twice, same 4 files, same digests, no churn)
- [x] app functional — no broken path from this slice: all three `cargo run -- run` invocations reach `run: clean exit` with exit 0, the gate-scene hash is unmoved, and the full workspace suite is green.
- [x] committed on `plan/zombie-collision` as `feat(scenario): add collision_mid_v1 and collision_sprite_v1 demo scenes` (the parent's wording supersedes the `feat(assets)` draft above), 11 files, pre-commit hooks honoured, no `--no-verify`.
