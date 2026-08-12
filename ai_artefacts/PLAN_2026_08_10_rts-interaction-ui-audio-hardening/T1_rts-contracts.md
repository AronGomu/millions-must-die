# T1: Lock phase-1.1 RTS contracts

**Plan:** `./ai_artefacts/PLAN_2026_08_10_rts-interaction-ui-audio-hardening.md`  
**Depends:** none  
**Commit outcome:** RTS map/body/speed contracts compile, current 320×320 scene validates, phase-0 `sim/` remains byte-unchanged.

## Context (self-contained)

- Goal: start phase 1.1 with exact engine constants used by every later collision, camera, HUD, and acceptance slice.
- This slice: separate RTS body from horde scenario radius; allow RTS maps up to 512×512; triple worker/soldier speed.
- Out of scope here: picking, collision resolution, nav inflation, formations, UI, settings, audio.
- Assumptions in force: RTS player/future-enemy units use 3-cell circular bodies; horde `sim/` stays soft-overlap at existing speed; tracked RTS scene stays 320×320.
- Decisions: `docs/ADR/016_ADR_phase1_1_scope_and_input_geometry.md`, `docs/ADR/017_ADR_rts_hard_collision_navigation_and_formations.md`.

## Requirements

- Add `RTS_MAX_MAP_EDGE: u32 = 512` and `RTS_MAX_MAP_CELLS: u32 = 262_144` in `crates/mmd-engine/src/scenario.rs`.
- Replace exact RTS-family 320×320 validator with: width/height each `1..=512`; checked product `<=262_144`; existing cell size 4, sprite size 48, zero horde counts stay locked.
- Keep `assets/scenarios/rts_prototype_v1.ron` at 320×320. Existing asset hash still proves exact shipped map.
- Add in `crates/mmd-engine/src/rts/entity.rs`:
  ```rust
  pub const RTS_UNIT_BODY_RADIUS_CELLS: f32 = 3.0;
  pub const RTS_UNIT_BODY_DIAMETER_CELLS: f32 = 6.0;

  impl UnitKind {
      pub const fn body_radius_cells(self) -> f32;
  }
  ```
  `Worker` and `Soldier` both return `3.0`. Exhaustive match forces future kinds to decide.
- Set `WORKER_SPEED_CELLS_PER_SEC = 30.0`; `SOLDIER_SPEED_CELLS_PER_SEC = 24.0` in `crates/mmd-engine/src/rts/orders.rs`.
- Set tracked RTS scene `collision_radius_q8: 768` (3 cells) so pre-T2 rings/picking stay functional during this commit. Treat it as compatibility metadata only: shipping RTS collision reads `UnitKind::body_radius_cells`, not scenario horde radius.
- Update `tools/scenegen/gen_rts_scene.py`, `assets/scenarios/rts_prototype_v1.ron`, `.sha256` together.
- Do not edit `crates/mmd-engine/src/sim/` or any phase-0 fixture/hash.

## Inputs

- `crates/mmd-engine/src/scenario.rs`: `validate_rts`, `RTS_SCENE_V1`, `COLLISION_Q8`.
- `crates/mmd-engine/src/rts/entity.rs`: `UnitKind`.
- `crates/mmd-engine/src/rts/orders.rs`: `WORKER_SPEED_CELLS_PER_SEC`, `SOLDIER_SPEED_CELLS_PER_SEC`, `unit_speed`.
- `tools/scenegen/gen_rts_scene.py`: canonical RTS scene generator.
- `crates/mmd-engine/tests/scenario_contract.rs`, `crates/mmd-engine/tests/rts_world.rs`.
- **From Depends:** none.

## TDD

1. **Red** — add cap/body/speed tests first; add a temp 512×512 RTS scenario accepted test and 513×320 rejected test.
2. **Green** — add constants/validator/methods; regenerate tracked scene + sidecar; change no unrelated behavior.
3. **Refactor** — remove old exact-320 validator branch only after current scene still passes exact asset checks.

## Test plan

| Test | Input | Expect |
| --- | --- | --- |
| `rts_map_edge_cap_is_512` | 512×512 + 513×320 RTS specs | first valid; second `InvalidRts` |
| `tracked_rts_scene_remains_320_by_320` | tracked scene | exact 320×320 |
| `rts_unit_body_radius_is_three_cells` | Worker, Soldier | radius 3; diameter 6 |
| `rts_unit_speeds_are_tripled` | Worker, Soldier | 30, 24 |
| `rts_scene_compat_radius_matches_three_cells` | tracked scene | `collision_radius_q8 == 768`; RTS body source remains `UnitKind` |
| existing phase-0 hash tests | phase-0 assets | unchanged |

## Impl steps

- [x] 1. Add red scenario cap/current-scene tests in `crates/mmd-engine/tests/scenario_contract.rs`. Validated: `cargo test -p mmd-engine --locked --test scenario_contract rts_` showed `rts_map_edge_cap_is_512` and `rts_scene_compat_radius_matches_three_cells` FAILED before impl (compile succeeded, assertions failed).
- [x] 2. Add red body/speed tests in `crates/mmd-engine/tests/rts_world.rs`. Validated: `cargo test -p mmd-engine --locked --test rts_world rts_unit_` failed to compile (`RTS_UNIT_BODY_RADIUS_CELLS` / `body_radius_cells` not found) before impl.
- [x] 3. Add `RTS_MAX_MAP_EDGE`/`RTS_MAX_MAP_CELLS`; generalize only RTS-family dimension validation. Validated: `rts_map_edge_cap_is_512` passes (512x512 accepted, 513x320 rejected).
- [x] 4. Add RTS body constants + exhaustive `UnitKind::body_radius_cells`. Validated: `rts_unit_body_radius_is_three_cells` passes.
- [x] 5. Change only RTS unit speed constants to 30/24. Validated: `rts_unit_speeds_are_tripled` passes.
- [x] 6. Update scenegen `collision_radius_q8` field to 768 (3 cells, per Requirements/Test-plan); regenerate RON + SHA-256 using existing scenegen workflow. Validated: `python3 tools/scenegen/gen_rts_scene.py` rerun is idempotent (no further diff); `rts_scene_compat_radius_matches_three_cells` passes.
- [x] 7. Run targeted tests; confirm `git diff -- crates/mmd-engine/src/sim` empty. Validated: `git diff --exit-code -- crates/mmd-engine/src/sim` exit 0; `cargo test --workspace --locked` all green (two downstream tests — `rts_nav_staleness::a_walking_unit_re_paths_when_a_building_blocks_its_route`, `rts_selection::the_pick_radius_is_the_body_radius` — updated to match the intentional speed/radius constants).

## Outputs

- Modified: `crates/mmd-engine/src/scenario.rs`, `crates/mmd-engine/src/rts/entity.rs`, `crates/mmd-engine/src/rts/orders.rs`, `tools/scenegen/gen_rts_scene.py`, tracked RTS RON/sidecar, two test files.
- Public API: constants + `UnitKind::body_radius_cells()` above.
- Behavior: RTS specs cap at 512×512; current map unchanged; RTS units report 3-cell bodies and 30/24 speed.
- Config/migration: tracked RTS sidecar changes because compatibility radius becomes 768.

## Validation

- [x] `cargo test -p mmd-engine --locked --test scenario_contract rts_` — 15 passed.
- [x] `cargo test -p mmd-engine --locked --test rts_world rts_unit_` — 2 passed.
- [x] `cargo check --workspace --all-targets --all-features --locked` — clean.
- [x] `git diff --exit-code -- crates/mmd-engine/src/sim` — exit 0, empty.
- [x] app functional: `cargo run -- rts --frames 3` — clean exit, tick=3 frames=3.
- [x] commit msg draft: `feat(rts): separate player body and map contracts from horde sim`
