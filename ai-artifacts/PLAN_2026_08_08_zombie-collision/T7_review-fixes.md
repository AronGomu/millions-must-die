# T7: Reviewer fixes — wedging blocker, validator coherence, test teeth, doc accuracy

**Plan:** `./ai-artifacts/PLAN_2026_08_08_zombie-collision.md`
**Depends:** T1–T6 (all done and pushed: fc3ab67, aa81daf, 06b63a0, a507487, 50dce68, 9be3092)
**Commit outcome:** No agent is ever permanently wedged by the separation blend; the claims the docs publish are the claims the tests actually prove.

## Context (self-contained)

T1–T6 shipped agent-agent soft separation steering: a repulsion vector summed into
the flow-field vector before the single move, with per-scenario Q8 body radius and
separation strength, a zero-alloc `SpatialGrid`, two demo scenes, and a docs/systems-map
update. All six tickets are committed and the merge gate is green.

A four-dimension deep review (correctness, security, scope-drift, tests) then found
one behavioural blocker, two unearned-claim blockers, and a set of should-fix gaps.
This ticket closes them. It is the last ticket in the plan.

Out of scope here: the neighbour-cap scan-order lockstep (accepted residual risk R1);
the crafted-scenario O(n²) DoS (residual risk R3 — see "Explicitly NOT in this ticket");
any renderer, sprite, atlas, shader, or golden change; any performance number anywhere;
any new CLI flag; combat, damage, unit stats, selection, camera.

## Requirements

1. The blended step obeys the same admissibility rule as a flow-field step. No agent
   can be steered into a walkable-but-unreachable corner pocket and frozen there.
2. A `collision_scene_v1` cannot declare a body and then silently disable separation.
3. Every claim published in `docs/technical-prototype-functional-close.md`, `docs/ADR/009_*`,
   `docs/agent-collision-architecture.html`, `docs/DESIGN.md`, and the architecture HTML
   is one the shipped tests actually prove.
4. The tests named as covering a behaviour actually fail when that behaviour is removed.

## Inputs

- `crates/mmd-engine/src/sim/tick.rs` — the blend and the walkability fallback live at
  ~`:88-116`; `position_walkable` at ~`:141-149` tests only the destination cell of the centre.
- `crates/mmd-engine/src/nav/flow_field.rs` — `diagonal_clear` at ~`:281-286` is the rule
  the flow field itself applies; a cell with `cost >= COST_UNREACHABLE` gets a zero vector
  at ~`:260-262`.
- `crates/mmd-engine/src/sim/collision.rs` — `CollisionParams::enabled()` ~`:107-109`,
  `MAX_SEPARATION_NEIGHBORS`, `SEPARATION_DIR16`, the contact cutoff at ~`:168`.
- `crates/mmd-engine/src/scenario.rs` — `validate_collision`, `validate_collision_scene_dims`
  (~`:530-534`), `validate_version_and_dims` V1 locked list.
- `crates/mmd-engine/tests/separation.rs` — 19 tests; `separation_never_wedges_an_agent_against_a_wall`
  ~`:461`, `separation_ignores_agents_beyond_contact` ~`:188`, `BODYLESS_GRID_PRE_SEPARATION_HASH` ~`:413`.
- `crates/mmd-engine/tests/scenario_contract.rs` — 22 tests.

### Digests pinned in the tree today (these WILL move — see step 8)

- gate scene `cargo run -- run --agents 50000 --frames 300` → `130e3047228c4813156d68641567971cda4ab8ef3f4e5e8c71d7088c7f1e8ba7`
  → **after the fix**: `f647e7f590ed5814e4e61388e23836dfacb980217fb1762542ec3abfe85549b3`
- `collision_mid_v1` 300 frames → `9b0691550b2a0b3af0a4d58c15662d2631cadf8ad5c8a65e402422facd633e91`
  → **after the fix**: `861ccf228a673c8a3c74718ed3891c0462aabbed426d9f434d87ea81182d1988`
- `collision_sprite_v1` 300 frames → `1909d6c085f74b3490a5cb0548b7b5744b68df605e7357a55a57aa6986b8223d`
  → **after the fix**: `0d13037832c37a90ec628f8ac9b94d23100ce1365405d11d7d4fb5546fec90d3`
- `BODYLESS_GRID_PRE_SEPARATION_HASH` = `e110a2bfba692f92ce6f2924991b881efa5075c6a72dce9b08fa9b984245ff85`
  — this one is on the radius-0 path and **must NOT change**. It is your control: if it moves,
  your fix has leaked into the bodyless path and is wrong.

## The blocker, with a measured repro

`position_walkable` tests only the destination cell. The pre-change code always stepped
along `(vx, vy)`, which `derive_vectors` restricts to grid directions that already passed
`diagonal_clear`, so a corner cut was unreachable. The blend makes the step an arbitrary
unit vector and removes that restriction. When the diagonal target is walkable but
*unreachable*, its descent vector is `(0,0)` forever and the agent never moves, never
arrives, never recycles. The wall fallback never fires, because the blended step *was*
walkable.

Measured by the reviewer:

- `technical_prototype_v1` contains **227** walkable cells that are unreachable under the
  no-corner-cut rule yet diagonally adjacent to a reachable cell.
- Shipped gate scene, 50 000 agents: agents parked in a zero-vector cell = 2 @ tick 200,
  25 @ 600, 53 @ 1000, 117 @ 1400, **119 @ 2000**, monotonically rising.
- Same geometry, 20 000 agents, 1500 ticks: separation **off** → 0 frozen; separation
  **on** (radius 0.3984, strength 1.0) → 13 frozen.
- Minimal repro: 8×8 grid, destination (7,7), obstacles `(0,1)` and `(1,0)` so `(0,0)` is
  walkable but corner-locked. Agent 0 at `(1.02, 1.02)`, agent 1 at `(1.60, 1.60)`,
  `with_collision(256, 2560)`. Tick 1 → agent 0 at `(0.92572, 0.92572)` = cell `(0,0)`;
  after 500 further ticks its position is bit-identical. `vector_at(0,0) == (0.0, 0.0)`,
  `cost_at(0,0) == 4294967294`.

This contradicts the contract T6 already wrote into `docs/DESIGN.md`: *"When the blended
step would leave the walkable area, the agent falls back to the pure descent step rather
than being wedged in place."*

**The fix contract:** a blended step that moves the agent's centre diagonally between
cells is admissible only if both shared cardinal neighbours are clear — the same rule
`diagonal_clear` applies in the flow field. An inadmissible blended step falls back to
the pure descent step. Implement it against the existing `diagonal_clear` semantics;
do not invent a second rule. This also closes the cosmetic variant where a corner-cutting
move whose target *is* reachable still sweeps its segment through a blocked cell.

## TDD

1. **Red** — write the minimal-repro test first, from the numbers above. It must fail
   on the current tree with the agent frozen at cell `(0,0)`.
2. **Green** — apply the admissibility rule in `tick.rs`.
3. **Red** — add the strength-0 validator test; it must fail before the validator change.
4. **Green** — add the validator rule.
5. **Refactor** — none beyond what the fixes require.

## Impl steps

- [x] 1. Add a failing test to `crates/mmd-engine/tests/separation.rs` reproducing the
      8×8 corner-pocket wedge verbatim from the numbers above — validate: it fails on the
      current tree, agent 0 frozen at cell `(0,0)` across 500 ticks.
- [x] 2. Apply the diagonal-admissibility rule to the blended step in
      `crates/mmd-engine/src/sim/tick.rs`, falling back to the pure descent step when the
      blended step is inadmissible — validate: step 1's test goes green.
- [x] 3. Give `separation_never_wedges_an_agent_against_a_wall` real teeth. Today deleting
      the fallback retry at `tick.rs:106-116` leaves all 19 separation tests **plus**
      `simulation`, `flow_field`, `harness`, `scenario_contract`, `runtime_frame` green —
      65 tests, zero failures — because the test only requires L1 displacement `> 1e-4`
      after 200 ticks while the measured minimum is `20.9` (200 000× slack). Rewrite it so
      deleting the fallback FAILS it — validate: delete the fallback locally, confirm the
      test fails, restore, confirm green. Report both observations.
- [x] 4. Make `separation_ignores_agents_beyond_contact` exercise the cutoff it is named
      for. Today the far agents sit four bins outside the 3×3 scan window, so they are
      never candidates; deleting `if d2 >= contact2 { continue; }` from `collision.rs:168`
      leaves it green. Place the far agent inside the same or an adjacent bin, just past
      `2 * radius` — validate: delete the cutoff locally, confirm the test fails, restore,
      confirm green. (Without the cutoff `w = (contact - d) * inv_contact` goes negative
      and agents *attract* at medium range — that is the regression being pinned.)
- [x] 5. Reject `separation_strength_q8 == 0` for the `collision_scene_v1` family in
      `validate_collision_scene_dims`. Today a collision scene may declare a body and set
      strength 0, which validates while `CollisionParams::enabled()` returns false and the
      separation pass is entirely off — validate: a new `scenario_contract` test rejects it.
- [x] 6. Add the missing validator tests — strength above `MAX_SEPARATION_STRENGTH_Q8` is
      refused (today replacing that whole check with `if false` leaves all 22 tests green),
      and both cap **boundary** values are accepted (today flipping either `>` to `>=`
      leaves all 22 green) — validate: `cargo test -p mmd-engine --test scenario_contract` green.
- [x] 7. Pin the bodied path with a golden digest, mirroring `BODYLESS_GRID_PRE_SEPARATION_HASH`.
      Today rotating `SEPARATION_DIR16` by one position, or raising `MAX_SEPARATION_NEIGHBORS`
      from 8 to 16, leaves all 19 separation tests green — `separation_is_reproducible` only
      compares two runs inside one process, so it cannot see a change that moves both.
      Do this **after** step 2, so the pinned value is the post-fix one — validate: rotate
      `SEPARATION_DIR16` locally, confirm the new test fails, restore, confirm green.
- [x] 8. Re-measure the three scene digests and update every place they are pinned (tests,
      docs, ticket files). They WILL move — the fix changes bodied trajectories. Confirm
      `BODYLESS_GRID_PRE_SEPARATION_HASH` did **not** move — validate: report all four values.
- [x] 9. Re-measure the sprite-scene deep-pair samples at ticks 1/100/200/300 and confirm
      `deep_at_300 * 2 <= deep_before` still holds and no sample after tick 1 exceeds tick 1.
      **If the 2× bar no longer holds, do NOT loosen it — report `failed` with the numbers.**
- [x] 10. Correct the unearned "order of magnitude" claim. `docs/technical-prototype-functional-close.md:~207`
      and `docs/ADR/009_ADR_agent_separation_and_collision.md:~138` both publish that deep
      overlap "collapses by an order of magnitude within 300 ticks". The test asserts a 2×
      bar and the measured reduction is 2.66×. Restate both to the claim the test actually
      makes, using your step-9 numbers — validate: grep both files, no "order of magnitude".
- [x] 11. Correct the ADR text that records a deletion which never happened.
      `docs/ADR/009_*.md:~109-112` and `docs/agent-collision-architecture.html` both say the
      strict "mean routing cost falls every tick" contract "is false and was re-derived".
      It was deliberately KEPT and still passes (`crates/mmd-engine/tests/simulation.rs:~387`),
      with `aggregate_progress_never_stalls` added alongside — validate: both files describe
      what shipped.
- [x] 12. Fix `docs/DESIGN.md:~13`
- [x] 12b. (added) The T6 fallback sentence in `docs/DESIGN.md` and
      `docs/technical-prototype-functional-close.md` described only the
      leaves-the-walkable-area half of the rule, which step 2 widened. Restate
      both to the shipped rule — validate: neither page states a narrower
      contract than `tick.rs::step_admissible` enforces. — "Spatial partition: Uniform grid (post-phase-0; deferred
      until a gameplay system consumes it)" is false; the grid is rebuilt every tick. The same
      file gained an "## Agent collision" section 15 lines below, so the page currently
      contradicts itself — validate: grep, no "deferred" claim about the grid.
- [x] 13. Fix the stale boundary card in `docs/technical-prototype-architecture.html` — it
      still lists "No collision/separation" as a phase-0 boundary. Its sibling
      `docs/simulation-navigation-architecture.html` was already repaired in T6; this parent
      page, linked from `docs/DESIGN.md` and `docs/README.md`, was missed. **Change only the
      boundary card.** Do not touch the pre-existing perf claims elsewhere in that file —
      they predate this work, the file sits outside `LIVE_DOCS`, and they are logged as a
      separate residual risk — validate: grep, boundary card no longer denies collision.
- [x] 14. Add the supersession marker to `docs/ADR/001_ADR_technical_prototype_scope_and_acceptance.md:~28`,
      whose "Explicit exclusions" still lists "Collision, separation, dynamic obstacles,
      per-agent paths" with `Status: Accepted` and no `Superseded by:` field. ADR 003 and
      `docs/ADR/README.md` already got theirs in T6. Follow the repo's own rule at
      `docs/ADR/README.md:~24` ("Changed decision → superseding ADR; do not rewrite history
      silently") — validate: ADR 001 and ADR 009 no longer contradict each other.
- [x] 15. Fix `docs/ADR/009_*.md:~57` — the heading "**Three shipped tunings**" introduces a
      four-row table (gate, mid, sprite, `fixture_*`). `docs/agent-collision-architecture.html`
      states the same data correctly as "One model, four radii" — validate: count matches table.
- [x] 16. Fix `docs/ADR/009_*.md:~113-114` — "Every state hash changes. Nothing in the tree
      pins one as a literal, so this is observable at the CLI rather than a test edit" is
      falsified by `BODYLESS_GRID_PRE_SEPARATION_HASH` in `separation.rs:~413`, and will be
      doubly false after step 7 — validate: the sentence describes what is actually pinned.
- [x] 17. Bring the close doc's claim table (`docs/technical-prototype-functional-close.md:~23`)
      up to date with the merge gate T6 widened to three interactive scene runs; the table
      still names only the 50k command — validate: table matches the gate block in
      `README.md` and `docs/05-testing.md`.
- [x] 18. Mention `tools/scenegen/gen_collision_scenes.py` in `README.md`'s "Developer tools
      (not gates)" section — it is currently discoverable only by `find` — validate: grep README.
- [x] 19. Update `MIN_SCANNED_TESTS` in `tests/validation_contract.rs` to the newly measured
      count if the tests you added change it. **Measure it, do not guess** — validate: bump it
      by 1 locally and confirm the "scanner is broken" message fires, then set the real value.

## Explicitly NOT in this ticket

- **R3 (security, accepted):** a crafted `collision_scene_v1` with 20 000 agents on a single
  spawn cell and a tiny radius can degenerate the neighbour scan to ~n² pair evaluations per
  tick and hang the process. The reviewer's suggested validator fix — reject
  `collision_radius_q8 < 128` — is **not viable**: every shipped scenario is below it
  (fixtures 32, gate scene 102). The real fix is a per-bin visit budget, which is an
  algorithm change that moves every digest again and would force the T4/T5 contracts to be
  re-derived. Out of plan scope. Do not attempt it.
- **R1 (accepted):** the 8-slot neighbour cap is spent in bin-scan order, visiting up/left
  bins before the agent's own bin.
- Pre-existing unmarked perf claims in `docs/technical-prototype-architecture.html`.
- `MIN_SCANNED_TESTS` being a floor on the total across files rather than per-file.

## Validation

- [x] `cargo test -p mmd-engine --test separation` → green — 21 passed, 0 failed (was 19)
- [x] `cargo test -p mmd-engine --test scenario_contract` → green — 25 passed, 0 failed (was 22)
- [x] `cargo test --test validation_contract` → green, including `every_system_has_a_test` — 7 passed
- [x] `cargo fmt --all -- --check` → exit 0
- [x] `cargo clippy --workspace --all-targets --all-features -- -D warnings` → exit 0
- [x] `MMD_REQUIRE_GPU=1 cargo test --workspace --locked` → green, no new skips — every binary
      0 failed; the only ignore is the pre-existing `alloc_guard::CountingAllocator` doctest
- [x] `cargo run -p xtask -- bootstrap --check` / `shaders --check` / `atlases --check` → exit 0
- [x] `cargo run -- run --agents 50000 --frames 300` → exit 0, NEW hash
      `f647e7f590ed5814e4e61388e23836dfacb980217fb1762542ec3abfe85549b3`
- [x] both new scene runs → exit 0; `collision_mid_v1`
      `861ccf228a673c8a3c74718ed3891c0462aabbed426d9f434d87ea81182d1988`,
      `collision_sprite_v1` `0d13037832c37a90ec628f8ac9b94d23100ce1365405d11d7d4fb5546fec90d3`
- [x] `BODYLESS_GRID_PRE_SEPARATION_HASH` unchanged at `e110a2bf…` —
      `a_bodyless_scenario_walks_the_flow_only_path` green, literal untouched
- [x] gate scene frozen-agent count is 0 where it was 119 @ tick 2000 — probed at 50 000
      agents: 0 / 0 / 0 / 0 / 0 at ticks 200 / 600 / 1000 / 1400 / 2000. The probe was
      validated by removing the diagonal arm, which reproduced the reviewer's series
      exactly (2 / 25 / 53 / 117 / 119), then restored. Probe deleted before commit.
- [x] all 7 scenario `.sha256` sidecars still match their `.ron` — none regenerated
- [x] commit msg draft: `fix(sim): stop the separation blend wedging agents in corner pockets`

### Deep-pair samples, sprite scene (step 9)

`[(1, 16229), (100, 10920), (200, 7011), (300, 5919)]` — monotone, no sample above the
tick-1 baseline, and `5919 * 2 = 11838 <= 16229`, so the 2x bar holds (2.74x measured).
