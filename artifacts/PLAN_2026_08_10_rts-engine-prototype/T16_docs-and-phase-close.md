# T16: Docs, ADRs, phase close

**Plan:** `./artifacts/PLAN_2026_08_10_rts-engine-prototype.md`
**Depends:** T15
**Commit outcome:** phase 1 is recorded, indexed and gated — every documented claim is backed by a named test, and every ADR matches what actually shipped.

## Context (self-contained)

- Goal: phase 1 is a thin vertical slice of an RTS engine prototype (camera,
  selection, workers, economy, building, unit production) on a horde-free scene.
  T1–T15 built it. This ticket closes it honestly.
- This slice: docs only, plus two contract tests that make the docs falsifiable.
  No gameplay change, no render change.
- Out of scope here: any behaviour change. If a doc and the code disagree,
  **fix the doc** — unless the code is wrong, in which case fix the code and say
  so explicitly in the commit body.
- Assumptions in force: `no_perf_claim_in_docs` still binds. Nothing written in
  this ticket may state a speed, a frame time, or a throughput. Phase 1 is
  functional scope only, exactly as phase 0 was.

## Requirements

- Three ADRs, already drafted by the plan, reviewed against the as-built code
  and amended where they diverge.
- One architecture page, already drafted, likewise.
- Every index updated: `docs/README.md`, `docs/ADR/README.md`, `docs/DESIGN.md`,
  `docs/CONTEXT.md`, `AGENT.md`, `README.md`.
- A phase-1 close document listing what is proven, what is not, and every known gap.
- `every_system_has_a_test` extended to the six phase-1 systems, resolving each
  claim against real `#[test]` function names.
- `docs/05-testing.md` describing the phase-1 gate.

## Inputs

- **Files to read**
  - `docs/technical-prototype-functional-close.md` — the model for the close doc.
  - `tests/validation_contract.rs` — `every_system_has_a_test`,
    `no_perf_claim_in_docs`, `gate_list_has_no_perf_thresholds`,
    `results_doc_is_superseded_history_not_a_claim`.
  - `docs/05-testing.md`, `docs/README.md`, `docs/ADR/README.md`,
    `docs/CONTEXT.md`, `docs/DESIGN.md`, `AGENT.md`, `README.md`.
  - The three ADRs and the architecture page drafted with this plan:
    - `docs/ADR/013_ADR_phase1_scope_and_rts_entity_model.md`
    - `docs/ADR/014_ADR_movable_camera_texture_table_and_ui_layer.md`
    - `docs/ADR/015_ADR_economy_construction_and_production_determinism.md`
    - `docs/rts-engine-prototype-architecture.html`
- **From Depends (T1–T15) — the shipped surface, spelled out:**
  - New engine modules: `crates/mmd-engine/src/rts/{mod,entity,economy,orders,selection,build,production,pack,hud,world}.rs`,
    `crates/mmd-engine/src/nav/field_pool.rs`,
    `crates/mmd-engine/src/render/{camera,text}.rs`,
    `crates/mmd-engine/src/testkit/rts.rs`.
  - New app modules: `src/{rts_input,rts_script,rts_overlay,rts_run}.rs`.
  - New xtask module: `xtask/src/placeholder_art.rs`.
  - New assets: `assets/scenarios/rts_prototype_v1.{ron,sha256}`,
    `assets/scenarios/rts_acceptance_v1.script`,
    `assets/sprites/generated/rts/{worker,soldier,buildings,props}.png` + manifest,
    `assets/sprites/generated/ui/font.png` + manifest.
  - New generator: `tools/scenegen/gen_rts_scene.py`.
  - New test binaries: `crates/mmd-engine/tests/{camera,nav_pool,ui_text,rts_world,rts_selection,rts_economy,rts_build,rts_production,rts_pack,rts_hud,rts_acceptance}.rs`,
    `tests/{rts_cli_contract,rts_acceptance}.rs`.
  - New gate command:
    `cargo run -- rts --frames 1600 --inject-input-file assets/scenarios/rts_acceptance_v1.script`.
  - Phase-0 invariants that survived and must be stated as such: the horde
    `run --agents 5000 --frames 300` exit-line `hash=` is unchanged, the render
    golden under `lab/goldens/` is byte-unchanged, `atlas_count/direction_count/
    frame_count` are still locked at `4/8/4` for every scenario family,
    `SpriteInstance` is still 48 bytes, `cargo tree -e features | grep -c testkit`
    is still `0`.

## Exact design — no decisions left

### 1. Review the drafted decision records against reality

For each of ADR 013, 014, 015 and the architecture page: read every factual
claim and check it against the code as shipped. Where they differ, edit the
document and add a bullet under its **Consequences** heading beginning
`**Corrected during implementation.**` naming what changed and why. Do not
silently rewrite — the ADR set's own rule is
*"New decision → new ADR. Changed decision → superseding ADR; do not rewrite
history silently."*

At minimum, verify these ten claims, each of which a ticket flagged as a place
where implementation could legitimately diverge:

1. `ATLAS_SLOT_COUNT == 9` and the five phase-1 slot ids (T2).
2. The phase-0 golden is byte-unchanged (T2).
3. `IsoView::with_center_cell` recomputes `depth_bias` (T4).
4. Zoom is **not** shipped (T4).
5. The RTS scene declares `hard_agent_count: 0` (T5).
6. `NAV_FIELD_SLOTS == 8` with LRU eviction (T7).
7. `EXTRA_BUILDERS_SPEED_UP == false` (T10).
8. Supply is **reserved at enqueue**, and `Supply::used` is **recomputed** every
   tick, not incremented (T11).
9. The `overlay` layer carries procedural rings only; textured depth-off content
   lives in `ui` (T2, T12).
10. The single documented allocation exception is a flow-field **miss** growing
    the reused scratch heap (T7).

### 2. `docs/rts-engine-prototype-functional-close.md`

New file, modelled on `docs/technical-prototype-functional-close.md`, with these
sections:

- **What phase 1 proves** — the six systems, each with the test binary and the
  named tests that prove it.
- **What phase 1 does not prove** — no combat, no enemy AI, no zoom, no minimap,
  no fog of war, no save/load, no menus, no sound, no second faction, no
  balance pass, no real art, **no performance number of any kind**, no
  cross-platform verification (Linux/Vulkan dev host only).
- **System → test map** — a table with columns `System | Test binary | Named tests`.
  Every row's test names must exist; the contract test below enforces it.
- **Known gaps** — carried forward from the tickets, at minimum:
  - Placeholder art everywhere; a real art pass will change every atlas hash.
  - `EXTRA_BUILDERS_SPEED_UP == false` — additive build speed deferred.
  - The Soldier has no weapon, no health and no combat behaviour.
  - Buildings cannot be destroyed, so `Supply::revoke_cap` is unexercised in
    play and covered only by unit tests.
  - The flow-field pool is 8 slots; a player issuing more than 8 distinct live
    destinations will thrash it. No test asserts a thrash is harmless beyond
    correctness.
  - The camera has no zoom, no minimap and no edge-scroll acceleration.
  - Nodes are never removed when depleted; they remain as depleted scenery.
  - A worker holding cargo when its drop-off disappears keeps the cargo forever.
  - Determinism is same-host, same-binary; `f32` is not claimed bit-identical
    across builds.
  - Any flakiness observed while running the new suites in parallel, named
    explicitly, alongside the phase-0 pair `warmup_allocation_passes` /
    `panic_restores_guard` if they are still flaky.

### 3. Extend `tests/validation_contract.rs`

`every_system_has_a_test` currently holds a `const` list of phase-0 systems in
test code and resolves each doc-mapped test name against a source scan of
`#[test]` functions. Extend that list with six phase-1 entries:

```rust
// (system name, test binary path, one representative test that must exist)
("camera",           "crates/mmd-engine/tests/camera.rs",         "cell_at_returns_the_cell_a_sprite_was_packed_from"),
("selection",        "crates/mmd-engine/tests/rts_selection.rs",  "box_selects_every_own_unit_inside"),
("workers",          "crates/mmd-engine/tests/rts_economy.rs",    "a_full_round_trip_banks_crystal"),
("economy",          "crates/mmd-engine/tests/rts_economy.rs",    "the_node_loses_exactly_the_carried_amount"),
("building",         "crates/mmd-engine/tests/rts_build.rs",      "a_depot_finishes_in_its_documented_time"),
("unit production",  "crates/mmd-engine/tests/rts_production.rs", "queueing_cannot_exceed_the_cap"),
```

The test must fail when a named test is deleted or renamed. Verify that by
renaming one and watching it go red.

Add a second contract test:

```rust
/// Every test the phase-1 close document maps to a system must exist.
///
/// The close doc's `System -> Test binary -> Named tests` table is a claim about
/// the repo, and a claim nothing checks rots within a ticket. This parses that
/// table out of the markdown and resolves every name against a scan of `#[test]`
/// functions in the named binary.
#[test]
fn phase1_close_doc_names_only_real_tests() { … }
```

Add `docs/rts-engine-prototype-functional-close.md` and
`docs/rts-engine-prototype-architecture.html` to `no_perf_claim_in_docs`'s
`LIVE_DOCS` set. Confirm the test still passes — and that it **fails** if you
temporarily insert the sentence "the RTS scene renders at 144 fps".

### 4. Index and prose updates

| File | Change |
| ---- | ------ |
| `docs/README.md` | new `## Phase 1 architecture` section linking the architecture page and the close doc |
| `docs/ADR/README.md` | append entries 13, 14, 15; add a "supersedes in part" note if ADR 012's fixed-camera line is now stale (it is — 014 supersedes it) |
| `docs/CONTEXT.md` | roadmap item 1 gains a status paragraph in the same voice as item 0's: what closed, what it proves, what it does not, with links |
| `docs/DESIGN.md` | new `## RTS entity model` section: entity store, orders, flow-field pool, economy/supply, grid placement; links to ADR 013/014/015 and the architecture page. Also record that "Unlimited unit selection" is implemented as `MAX_SELECTION == MAX_ENTITIES` |
| `docs/05-testing.md` | the phase-1 gate command (added in T15) gets its prose paragraph; a `## Phase 1 scope` section states that phase 1 closes on functional scope with performance still unmeasured |
| `AGENT.md` | **Status** section rewritten for phase 1; **Workspace layout** gains `crates/mmd-engine/src/rts/`; **Build / test / dev commands** gains the `rts` gate command; **Architectural constraints** gains: no per-entity pathfinding for player units either (flow-field pool), supply reserved at enqueue, `Supply::used` recomputed not incremented, the `overlay`-vs-`ui` layer rule |
| `README.md` | one paragraph and one command (`cargo run -- rts`) so a reader can see the prototype |
| `docs/GLOSSARY.md` | via the project's glossary skill: entries for entity, order, flow-field pool, drop-off, footprint, ghost, site, supply, rally point, scene pass, UI layer |

## TDD

1. **Red** — write `phase1_close_doc_names_only_real_tests` and the six new
   `every_system_has_a_test` rows first, against docs that do not exist yet.
   Watch them fail.
2. **Green** — write the close doc and the index updates until they pass.
3. **Refactor** — none expected. Keep green.

## Test plan

| Test | Input | Expect |
| ---- | ----- | ------ |
| `every_system_has_a_test` (extended) | the six new rows | green |
| `every_system_has_a_test_fails_on_a_renamed_test` (manual, in Impl steps) | rename `box_selects_every_own_unit_inside` | red |
| `phase1_close_doc_names_only_real_tests` | the close doc's table | green |
| `phase1_close_doc_fails_on_a_fictional_test` (manual) | add a row naming `test_that_does_not_exist` | red |
| `no_perf_claim_in_docs` (extended `LIVE_DOCS`) | the two new docs | green |
| `no_perf_claim_in_docs_fails_on_an_inserted_claim` (manual) | insert "144 fps" into the close doc | red |
| `gate_list_has_no_perf_thresholds` | the gate block now carrying the `rts` command | green |
| `adr_index_lists_every_adr_file` | new test: `docs/ADR/*.md` minus `README.md` vs the numbered list in `README.md` | every file is linked, and every link resolves |
| `every_doc_link_resolves` | new test: every relative markdown link under `docs/` | the target exists |

**Mutation verification (mandatory).** Inject, confirm red, revert, confirm green:
1. Rename one mapped test → kills `every_system_has_a_test`.
2. Add a fictional row to the close doc's table → kills `phase1_close_doc_names_only_real_tests`.
3. Insert a frame-time sentence into the close doc → kills `no_perf_claim_in_docs`.
4. Remove ADR 014 from `docs/ADR/README.md`'s list → kills `adr_index_lists_every_adr_file`.
5. Point a `docs/README.md` link at a missing file → kills `every_doc_link_resolves`.

## Impl steps

- [x] 1. Read ADR 013, 014, 015 and the architecture page; check all ten flagged claims against the shipped code.
- [x] 2. Amend each document where it diverges, adding a `**Corrected during implementation.**` bullet under Consequences.
- [x] 3. Write `docs/rts-engine-prototype-functional-close.md` with all four sections and the full system → test table.
- [x] 4. Add the six phase-1 rows to `every_system_has_a_test` in `tests/validation_contract.rs`.
- [x] 5. Write `phase1_close_doc_names_only_real_tests` in the same file.
- [x] 6. Add the two new docs to `no_perf_claim_in_docs`'s `LIVE_DOCS`.
- [x] 7. Write `adr_index_lists_every_adr_file` and `every_doc_link_resolves`.
- [x] 8. Update `docs/README.md`, `docs/ADR/README.md`, `docs/CONTEXT.md`, `docs/DESIGN.md`, `docs/05-testing.md`.
- [x] 9. Update `AGENT.md`'s Status, Workspace layout, Build/test/dev commands and Architectural constraints sections.
- [x] 10. Update `README.md` with the `cargo run -- rts` line.
- [x] 11. Run the project's glossary skill (`.claude/skills/make-glossary-aron/SKILL.md`) to add the eleven new terms to `docs/GLOSSARY.md`.
- [x] 12. Run `graphify update .` so the knowledge graph covers the new modules.
      (5 305 nodes, 10 498 edges, 296 communities; `graphify-out/` is gitignored, so nothing to stage.)
- [x] 13. Run the mutation list; record kills in the commit body.
      (5 injected, 5 killed, 0 survivors; each reverted and re-confirmed green.)
- [x] 14. Run the full validation block.

## Outputs

- **Files created**
  - `docs/rts-engine-prototype-functional-close.md`
- **Files edited**
  - `docs/ADR/{013,014,015}_*.md`, `docs/rts-engine-prototype-architecture.html`
  - `docs/{README,ADR/README,CONTEXT,DESIGN,05-testing,GLOSSARY}.md`
  - `AGENT.md`, `README.md`
  - `tests/validation_contract.rs`
  - `graphify-out/` (regenerated)
- **Public API added:** none.
- **Behaviour change:** none. Docs and contract tests only.
- **Migration / config:** none.

## Validation

- [x] `cargo fmt --all -- --check`
- [x] `cargo test --test validation_contract` — all green (10 passed)
- [x] `MMD_REQUIRE_GPU=1 cargo test --workspace --locked` — 48 binaries, 0 failures
- [x] `VK_DRIVER_FILES=/nonexistent cargo test --workspace --locked` — 48 binaries, 0 failures
- [x] `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- [x] `nix flake check` — all checks passed
- [x] `cargo run -p xtask -- bootstrap --check`
- [x] `cargo run -p xtask -- shaders --check`
- [x] `cargo run -p xtask -- atlases --check` — 4 zombie + 4 rts + 1 ui
- [x] `cargo run -- run --agents 5000 --frames 300` — exit 0, `hash=864147ca3a0e09f7ebc5762b778fce193e705a2bc943ceaf67acf087581ee881`
- [x] `cargo run -- run --scenario assets/scenarios/collision_mid_v1.ron --frames 300` — exit 0
- [x] `cargo run -- run --scenario assets/scenarios/collision_sprite_v1.ron --frames 300` — exit 0
- [x] `cargo run -- rts --frames 1600 --inject-input-file assets/scenarios/rts_acceptance_v1.script` — exit 0
- [x] `cargo tree -e features | grep -c testkit` — `0`
- [x] `git diff --stat HEAD -- lab/goldens/` — **empty**; `golden_frame_matches` passes without `MMD_UPDATE_GOLDEN=1`
- [x] every checkbox in `docs/05-testing.md`'s required merge gate passes, in order, from a clean tree
- [x] app functional — no broken path from this slice
- [x] commit msg draft: `docs(rts): record the phase-1 decisions and close the slice` — landed as `54b01bf`
