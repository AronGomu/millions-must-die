# T6: Systems map, docs, ADR

**Plan:** `./ai-artifacts/PLAN_2026_08_08_zombie-collision.md`
**Depends:** T5
**Commit outcome:** Collision is a named system with a test map the gate enforces, a written decision record, and docs that describe what it does and — just as importantly — what it does not guarantee.

## Context (self-contained)

- Goal: zombies stop passing through each other, via soft separation steering already implemented and shipped in two demo scenes. This ticket makes the claim auditable.
- This slice: docs and the test-enforced coverage map only. **No source file under `crates/mmd-engine/src/**` or `src/**` is edited.** The only Rust file touched is `tests/validation_contract.rs`, which is itself the coverage contract.
- Out of scope here: any behaviour change; renaming or adding a test in a test file; touching a scenario asset.
- Assumptions in force:
  - `every_system_has_a_test` in `tests/validation_contract.rs` resolves the `SCOPE_SYSTEMS` const against a source scan **and** against `docs/technical-prototype-functional-close.md`, in both directions. Both must move in this one commit or the gate goes red.
  - Docs must contain no live performance claim. `no_perf_claim_in_docs` scans `README.md`, `docs/CONTEXT.md`, `docs/05-testing.md`, `docs/technical-prototype-functional-close.md` and `CONTRIBUTING.md` line by line. Do not write a speed number anywhere, retired-marked or not.
  - Soft separation gives **no** hard non-overlap guarantee. Every doc sentence must say so; nothing may claim "agents never overlap".

## Inputs

- `tests/validation_contract.rs`. Relevant existing shape:
  - `struct SystemCoverage { system: &'static str, file: &'static str, tests: &'static [&'static str], gpu_only: &'static [&'static str] }`
  - `const SCOPE_SYSTEMS: &[SystemCoverage]` currently holds **11** entries, in this order: `Simulation — movement, obstacles, recycling`, `Navigation — flow field`, `Scenario loading and hash contract`, `Deterministic test harness`, `Runtime frame loop`, `Render correctness — instance data, projection, whole frame`, `GPU smoke and tracked asset hashes`, `Golden-image comparator`, `Allocation invariant`, `App and CLI lifecycle`, `Merge-gate contract`.
  - `const MIN_SCANNED_TESTS: usize = 125;` — a floor on the total `#[test]` fns discovered across mapped files.
  - `const SCOPE_SYSTEM_COUNT: usize = 11;` — asserted equal to `SCOPE_SYSTEMS.len()`.
  - The `Allocation invariant` entry maps `crates/mmd-engine/tests/frame_allocations.rs` with tests `alloc_invariant_still_enforced`, `warmup_allocation_passes`, `guard_resets_between_trials`, `panic_restores_guard`, `foreign_thread_allocations_do_not_leak_into_a_measure_scope`.
  - Rules enforced per mapped test: the name must exist in the mapped file; a name mapped outside `gpu_only` must **not** be `#[ignore]`d; the close doc must contain the system string verbatim, the file path verbatim, and every mapped test name; and every backticked snake_case token of length ≥ 8 in the close doc's prose must resolve to a test declared in some mapped file.
  - `const REQUIRED_COMMANDS` is a floor, not an exact list — adding commands to a gate section is allowed. `gate_list_has_no_perf_thresholds` scans the `Required merge gate` section of `docs/05-testing.md` and `README.md` for `PERF_THRESHOLD_TOKENS` (`p95`, `p99`, `nmad`, `16.67`, `25 ms`, `frame-time`, `frame time`, `percentile`, `median`, `throughput`, `latency`, `fps`) and for `NON_GATING_COMMANDS` (`bench`, `mmd-lab`, `release-freeze`, `release-check`, `calibrate`, `pilot`, `merge-gate`). The scene commands added below contain none of these.
- `docs/technical-prototype-functional-close.md` — structure: `## What "closed" means here` (line 18), `## System → test map` (line 30, the table starts line 38), `### Evidence behind the render rows` (64), `### How these tests were validated` (86), `## Known gaps — all non-blocking` (103) with numbered `### 1.` … `### 9.` subsections, `## Phase-1 backlog` (199), `## Related` (215).
- `docs/05-testing.md` — owns the `## Required merge gate` section and its fenced command list.
- `README.md` — mirrors the gate list; links `docs/05-testing.md` at lines 11, 39, 64, 76.
- `docs/README.md` — the docs index, with a `## Phase 0 architecture` list of HTML pages.
- `docs/ADR/README.md` — the ADR index, ending with "New decision → new ADR."
- `docs/ADR/003_ADR_simulation_and_flow_field.md` — contains the now-false line `- Overlap allowed. No agent collision/separation/spatial neighbor grid.` and a `- Superseded by: —` header field.
- `docs/simulation-navigation-architecture.html` — its "Not present" card currently lists `<li>Collision/separation</li>` and `<li>Agent spatial partition</li>`, both of which are now false.
- `docs/DESIGN.md`, `docs/GLOSSARY.md`, `AGENT.md`.
- Already written by the plan author, do **not** rewrite, only link:
  - `docs/ADR/009_ADR_agent_separation_and_collision.md`
  - `docs/agent-collision-architecture.html`
- **From Depends (T3/T4/T5), quoted because the worker cannot read those tickets** — the test names now declared in `crates/mmd-engine/tests/separation.rs`:
  `spatial_bins_hold_every_agent_exactly_once`, `spatial_bucket_order_is_ascending_agent_index`, `spatial_bin_size_covers_two_radii`, `spatial_bin_size_never_drops_below_one_cell`, `spatial_clamps_positions_outside_the_world`, `spatial_rebuild_is_repeatable`, `separation_of_a_pair_is_equal_and_opposite`, `separation_is_capped_at_eight_neighbours`, `separation_ignores_agents_beyond_contact`, `coincident_agents_separate_on_the_first_tick`, `separation_is_reproducible`, `separation_keeps_the_step_length`, `a_released_stack_spreads_apart`, `a_bodyless_scenario_walks_the_flow_only_path`, `a_fixture_scenario_reports_its_body`, `sprite_scene_pulls_agents_out_of_deep_overlap`, `mid_scene_reports_its_tuning`, `collision_scene_agents_never_enter_an_obstacle`. None is `#[ignore]`d.
  Newly declared in `crates/mmd-engine/tests/frame_allocations.rs`: `spatial_rebuild_allocates_nothing`, `a_collision_tick_allocates_nothing`.
  Renamed in `crates/mmd-engine/tests/simulation.rs`: `aggregate_progress_is_monotone` → `aggregate_progress_never_stalls` (it was never mapped and is not named in the close doc, so no map edit follows from it).
  Shipped scenes: `assets/scenarios/collision_mid_v1.ron` (10 000 agents, body radius 1.25 cells) and `assets/scenarios/collision_sprite_v1.ron` (1 200 agents, body radius 3.75 cells = 15 px = half a sprite).

## Requirements

- `SCOPE_SYSTEMS` gains a twelfth entry for collision, and `SCOPE_SYSTEM_COUNT` becomes `12`.
- The `Allocation invariant` entry gains the two new allocation tests.
- `MIN_SCANNED_TESTS` is raised to the number the scanner actually reports — measured, not guessed.
- The close doc's table gains the matching row and the amended allocation row.
- A new known-gap subsection records that separation is steering, with no hard non-overlap guarantee.
- The gate command list in both `docs/05-testing.md` and `README.md` gains the two demo scenes.
- ADR 003's superseded line is marked, ADR 009 is indexed, and the architecture page is linked.

## Exact text to write

New `SystemCoverage` entry, inserted **after** the `Simulation — movement, obstacles, recycling` entry:

```rust
    SystemCoverage {
        system: "Collision — agent separation and neighbour bins",
        file: "crates/mmd-engine/tests/separation.rs",
        tests: &[
            "spatial_bins_hold_every_agent_exactly_once",
            "spatial_bucket_order_is_ascending_agent_index",
            "spatial_clamps_positions_outside_the_world",
            "separation_of_a_pair_is_equal_and_opposite",
            "separation_is_capped_at_eight_neighbours",
            "coincident_agents_separate_on_the_first_tick",
            "separation_keeps_the_step_length",
            "a_released_stack_spreads_apart",
            "a_bodyless_scenario_walks_the_flow_only_path",
            "sprite_scene_pulls_agents_out_of_deep_overlap",
            "collision_scene_agents_never_enter_an_obstacle",
        ],
        gpu_only: &[],
    },
```

Amended `Allocation invariant` test list:

```rust
        tests: &[
            "alloc_invariant_still_enforced",
            "warmup_allocation_passes",
            "guard_resets_between_trials",
            "panic_restores_guard",
            "foreign_thread_allocations_do_not_leak_into_a_measure_scope",
            "spatial_rebuild_allocates_nothing",
            "a_collision_tick_allocates_nothing",
        ],
```

New close-doc table row, inserted after the `Simulation — movement, obstacles, recycling` row (one line, pipes as shown):

```md
| Collision — agent separation and neighbour bins | `spatial_bins_hold_every_agent_exactly_once`, `spatial_bucket_order_is_ascending_agent_index`, `spatial_clamps_positions_outside_the_world`, `separation_of_a_pair_is_equal_and_opposite`, `separation_is_capped_at_eight_neighbours`, `coincident_agents_separate_on_the_first_tick`, `separation_keeps_the_step_length`, `a_released_stack_spreads_apart`, `a_bodyless_scenario_walks_the_flow_only_path`, `sprite_scene_pulls_agents_out_of_deep_overlap`, `collision_scene_agents_never_enter_an_obstacle` | `crates/mmd-engine/tests/separation.rs` |
```

Amended close-doc `Allocation invariant` row — append to its `Proven by` cell:

```md
, `spatial_rebuild_allocates_nothing`, `a_collision_tick_allocates_nothing`
```

New known-gap subsection, appended after `### 9. Frozen measurement code rots until re-validated` and before `## Phase-1 backlog`:

```md
### 10. Collision is steering, not resolution

Agents separate by *steering*: each sums a repulsion vector from the
neighbours overlapping its body, that sum is added to the flow-field descent
vector, and the agent walks the blended heading at its unchanged speed. Nothing
in the tick forbids an overlap, and no test claims one is impossible — the
proven claims are that a coincident pair splits, that a released stack spreads,
and that deep overlap on the sprite-scale scene collapses by an order of
magnitude within 300 ticks. Under crowd pressure, and especially in the jam that
forms at the destination cell, bodies do interpenetrate. That is the accepted
behaviour of the chosen model, recorded in
[ADR 009](ADR/009_ADR_agent_separation_and_collision.md).

Two further narrowings live here rather than in the code. Each agent
accumulates at most eight neighbours per tick, so a very deep stack is pushed
apart over several ticks instead of one. And when the blended step would leave
the walkable area, the agent falls back to the pure descent step — separation
may never wedge an agent the field alone could have moved, which means a wall
can win against a crowd and let bodies compress against it.
```

Gate command list — append these two lines to the fenced block under
`## Required merge gate` in **both** `docs/05-testing.md` and `README.md`:

```sh
cargo run -- run --scenario assets/scenarios/collision_mid_v1.ron --frames 300
cargo run -- run --scenario assets/scenarios/collision_sprite_v1.ron --frames 300
```

Prose to add directly under that fenced block in `docs/05-testing.md`:

```md
The last three commands are the interactive smokes: the 50k scene and both
collision demo scenes must start, tick and exit cleanly. The demo scenes are
what make agent separation observable — the gate scene's body radius is a
fraction of its sprite, because 50 000 sprites cannot be laid out on one screen
without overlapping.
```

ADR 003 — change its header line `- Superseded by: —` to:

```md
- Superseded by: [ADR 009](009_ADR_agent_separation_and_collision.md), in part — the "no agent collision" decision only
```

and change the decision bullet `- Overlap allowed. No agent collision/separation/spatial neighbor grid.` to:

```md
- ~~Overlap allowed. No agent collision/separation/spatial neighbor grid.~~ Superseded by [ADR 009](009_ADR_agent_separation_and_collision.md): agents now carry a scenario-declared body radius and steer apart through a uniform neighbour grid. Every other decision in this record stands.
```

`docs/ADR/README.md` — append to the numbered list:

```md
9. [Agent separation + collision](009_ADR_agent_separation_and_collision.md)
```

and add above the list, after the existing supersession paragraph:

```md
ADR 003 is **superseded in part** by ADR 009 (2026-08-08): agents no longer pass
freely through each other. Its navigation and movement decisions still stand.
```

`docs/README.md` — add to the `## Phase 0 architecture` list, after the
simulation + navigation entry:

```md
- [Agent collision](agent-collision-architecture.html)
```

`docs/DESIGN.md` — add a `## Agent collision` section stating: bodies are
scenario data in Q8 fixed point; the model is soft separation steering, not
resolution; the neighbour index is a preallocated uniform grid rebuilt per tick
with no allocation; the coincidence tie-break is a 16-entry table keyed on the
index pair; each agent accumulates at most eight neighbours; and the blended
step falls back to the pure descent step rather than wedging. Link
`ADR/009_ADR_agent_separation_and_collision.md` and
`agent-collision-architecture.html`.

`docs/GLOSSARY.md` — add entries for **body radius**, **contact distance**
(twice the body radius), **separation strength**, **Q8**, **neighbour bin**,
**coincidence tie-break**, and **collision scene**. Keep each to one or two
sentences and match the file's existing formatting.

`AGENT.md` — in `## Architectural constraints`, replace the bullet
`- No per-enemy pathfinding — navigation via flow fields.` with:

```md
- No per-enemy pathfinding — navigation via flow fields. Agent-agent collision is *soft separation steering* layered on top: a repulsion sum bends the descent vector, it never resolves an overlap, and no code or doc may claim agents cannot overlap.
```

## TDD

1. **Red** — bump `SCOPE_SYSTEM_COUNT` to `12` first, before adding the entry, and run `cargo test --test validation_contract`. It must fail with "the phase-0 scope list changed size". That failure proves the guard is live before it is satisfied.
2. **Green** — add the entry, the doc row, the amended allocation lists, then re-measure `MIN_SCANNED_TESTS` by the procedure below.
3. **Refactor** — none.

## Measuring `MIN_SCANNED_TESTS` — do not guess

- [ ] Temporarily set `const MIN_SCANNED_TESTS: usize = 100_000;`
- [ ] Run `cargo test --test validation_contract every_system_has_a_test 2>&1 | grep 'source scan found'`
- [ ] The panic message reads `source scan found only N #[test] fns ...`. Set `MIN_SCANNED_TESTS` to exactly that `N`.
- [ ] Re-run and confirm the test passes.

## Test plan

| Test | Input | Expect |
| ---- | ----- | ------ |
| `every_system_has_a_test` | 12-entry map + amended close doc | passes; the collision row resolves in both directions |
| `every_system_has_a_test` (negative rehearsal) | temporarily misspell one mapped collision test name | fails naming the missing test — then revert. This proves the new row is actually checked rather than decorative |
| `no_perf_claim_in_docs` | amended `docs/05-testing.md`, `README.md`, close doc | passes; none of the new prose states a speed number |
| `gate_list_has_no_perf_thresholds` | amended gate lists in both docs | passes; the two new commands carry no threshold token and no non-gating tool name |
| `gate_docs_state_perf_gating_is_retired` | unchanged retirement prose | still passes |
| `results_doc_is_superseded_history_not_a_claim` | untouched | still passes |

## Impl steps

- [ ] 1. In `tests/validation_contract.rs`, change `SCOPE_SYSTEM_COUNT` from `11` to `12`; run `cargo test --test validation_contract` and confirm the size assertion fires (red).
- [ ] 2. Insert the new `SystemCoverage` entry verbatim, after the `Simulation — movement, obstacles, recycling` entry.
- [ ] 3. Replace the `Allocation invariant` entry's `tests:` list with the seven-name version above.
- [ ] 4. In `docs/technical-prototype-functional-close.md`, insert the new table row verbatim after the simulation row.
- [ ] 5. Append the two new test names to the `Allocation invariant` row's `Proven by` cell.
- [ ] 6. Append the `### 10. Collision is steering, not resolution` subsection verbatim, after gap 9 and before `## Phase-1 backlog`.
- [ ] 7. Run the `MIN_SCANNED_TESTS` measurement procedure above and set the constant to the reported number.
- [ ] 8. Run `cargo test --test validation_contract` → 7 passed.
- [ ] 9. Rehearse the negative: misspell one collision test name in `SCOPE_SYSTEMS`, run the test, confirm it fails naming that test, then revert the misspelling and confirm green again.
- [ ] 10. Append the two scene commands to the fenced gate block in `docs/05-testing.md`, then add the prose paragraph beneath it.
- [ ] 11. Append the same two commands to the gate block in `README.md`.
- [ ] 12. Edit `docs/ADR/003_ADR_simulation_and_flow_field.md`: the `Superseded by` header line and the overlap bullet, both verbatim from above.
- [ ] 13. Edit `docs/ADR/README.md`: the supersession paragraph and the list entry.
- [ ] 14. Add the architecture link to `docs/README.md`.
- [ ] 15. Add the `## Agent collision` section to `docs/DESIGN.md`.
- [ ] 16. Add the seven glossary entries to `docs/GLOSSARY.md`.
- [ ] 17. Replace the flow-field bullet in `AGENT.md` with the version above.
- [ ] 18. In `docs/simulation-navigation-architecture.html`, remove `<li>Collision/separation</li>` and `<li>Agent spatial partition</li>` from the "Not present" card, and add to that page's nav paragraph: ` · <a href="agent-collision-architecture.html">Agent collision</a>`. Leave everything else on that page alone.
- [ ] 19. Re-read every sentence added in steps 4–18 and confirm none states a speed, a frame time, or a hard non-overlap guarantee.
- [ ] 20. Run the full validation list below.

## Outputs

- Files touched: `tests/validation_contract.rs`, `docs/technical-prototype-functional-close.md`, `docs/05-testing.md`, `README.md`, `docs/README.md`, `docs/DESIGN.md`, `docs/GLOSSARY.md`, `docs/ADR/README.md`, `docs/ADR/003_ADR_simulation_and_flow_field.md`, `docs/simulation-navigation-architecture.html`, `AGENT.md`.
- Public API change: none.
- Behaviour change: none. The merge gate gains two smoke commands.
- Migrate / config: none.

## Validation

- [ ] `cargo test --test validation_contract` → 7 passed
- [ ] `cargo fmt --all -- --check` → exit 0
- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings` → exit 0
- [ ] `MMD_REQUIRE_GPU=1 cargo test --workspace --locked` → green
- [ ] `nix flake check` → exit 0
- [ ] `cargo run -p xtask -- bootstrap --check` / `shaders --check` / `atlases --check` → exit 0
- [ ] `cargo run -- run --agents 50000 --frames 300` → exit 0
- [ ] `cargo run -- run --scenario assets/scenarios/collision_mid_v1.ron --frames 300` → exit 0
- [ ] `cargo run -- run --scenario assets/scenarios/collision_sprite_v1.ron --frames 300` → exit 0
- [ ] manual check: `grep -c 'agent-collision-architecture.html' docs/README.md` → `1`
- [ ] manual check: `grep -c 'Collision/separation' docs/simulation-navigation-architecture.html` → `0`
- [ ] manual check: the negative rehearsal in impl step 9 actually failed before being reverted
- [ ] app functional — no broken path from this slice
- [ ] commit msg draft: `docs(collision): map agent separation as a phase-0 system and record the decision`
