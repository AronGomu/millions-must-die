# Progress: RTS Engine Prototype (Phase 1)

- Goal: prove the custom engine runs real RTS mechanics — camera, selection,
  workers, economy, building, unit production — as one thin vertical slice on a
  new horde-free scene, asserted headlessly and deterministically.
- Plan index: `ai-artifacts/PLAN_2026_08_10_rts-engine-prototype.md`
- Tickets dir: `ai-artifacts/PLAN_2026_08_10_rts-engine-prototype/`
- Workspace: branch `plan/rts-engine-prototype` (base `main` @ `a33d96d`)
- Started: 2026-08-10
- Updated: 2026-08-10

## Success

- [x] `cargo run -- rts --frames 1600 --inject-input-file assets/scenarios/rts_acceptance_v1.script`
      exits 0 after panning the camera, box-selecting workers, gathering BOTH
      resources, finishing a Depot and Barracks, and producing a Soldier under
      supply — final parent run: `tick=1449 crystal=54 gas=74 supply=9/20
      units=8 buildings=3 nodes=10 camera=181.00092,150.99908`; engine acceptance
      5/5 + app acceptance 12/12 green
- [x] The whole phase-0 merge gate stays green — final parent run: fmt clean,
      workspace 980 passed / 0 failed (after one isolated retry of the documented
      HUD GPU flake), clippy clean, `nix flake check` all checks passed, all three
      xtask checks ok, gate scene exit 0 on exact hash
      `864147ca3a0e09f7ebc5762b778fce193e705a2bc943ceaf67acf087581ee881`
- [x] The phase-0 render golden is byte-unchanged — final guard diff empty and
      `golden_frame_matches` passed without regeneration
- [x] Every tracked phase-0 asset is byte-unchanged — final guard diff against
      `main` empty for phase-0 fixture tree, zombie atlases and manifest
- [x] `testkit` stays feature-gated out of the shipping binary — final
      `cargo tree -e features | grep -c testkit` = 0
- [x] `ai-artifacts/manual_test_checklist.md` covers every shipped phase-1 ticket
      and the post-review fixes — T1–T16 sections plus `## Post-review fixes`

## Out of scope

Combat / weapons / damage / health / enemy AI; the zombie horde inside the RTS
scene; zoom, minimap, fog of war, save/load, menus, sound; any performance number
(`no_perf_claim_in_docs` still binds); a second player or AI opponent; real art;
cross-platform verification.

## Status

| ID  | Title                              | File                                     | State   | Evidence | Note |
| --- | ---------------------------------- | ---------------------------------------- | ------- | -------- | ---- |
| T1  | Placeholder atlas families         | `.../T1_placeholder-atlas-families.md`   | done    | `01dfdf2` — `cargo test -p xtask` 37 pass (17 new); full gate green; `xtask atlases` idempotent (empty `git status`); phase-0 atlases + manifest byte-unchanged vs `main` | tier standard |
| T2  | Renderer texture table + ScenePass | `.../T2_texture-table-and-scene-pass.md` | done    | `391d136` — `MMD_REQUIRE_GPU=1 cargo test --workspace --locked` 35 suites ok, `render_correctness` 44 pass; `golden_frame_matches` green with `MMD_UPDATE_GOLDEN` unset; `git diff --stat main -- lab/goldens/` empty; 6/6 mutations killed | tier deep (renderer × atlas, golden byte-identity) |
| T3  | Bitmap text packer                 | `.../T3_bitmap-text-packer.md`           | done    | `b974155` — `cargo test -p mmd-engine --test ui_text` 18/18; 2 GPU tests green under `MMD_REQUIRE_GPU=1`, skip-clean without a driver; full gate green; golden diff empty; 6/6 mutations killed | tier standard |
| T4  | Panning camera + unprojection      | `.../T4_camera-and-unprojection.md`      | done    | `41e6299` — `cargo test -p mmd-engine --test camera` 20/20; full gate green; `run --agents 5000 --frames 300` hash `864147ca…` identical before/after (stash-verified); golden diff empty; 7/7 mutations killed | tier standard |
| T5  | `rts_prototype_v1` scenario        | `.../T5_rts-scenario-family.md`          | done    | `6a05593` — `--test scenario_contract` 64/64; full gate green; gate-scene hash `864147ca…` bit-identical (stash-verified); phase-0 fixtures + goldens byte-unchanged vs `main`; 6/6 mutations killed | tier standard |
| T6  | Entity store + `RtsWorld`          | `.../T6_entity-store-and-world.md`       | done    | `3f47853` (+ `9d27063` box fix) — `--test rts_world` 32/32; parent re-ran the whole gate independently: 38 test-result lines all `0 failed`, `nix flake check` passed, gate hash `864147ca…`, `cargo tree -e features \| grep -c testkit` = 0 | tier standard |
| T7  | Field pool + unit movement         | `.../T7_field-pool-and-movement.md`      | done    | `efe318c` — `nav_pool` 13, `rts_world` 48, `frame_allocations` 12 all `0 failed`; gate hash `864147ca…` UNCHANGED despite the `sim/tick.rs` edit; golden green without regeneration; 7/8 mutations killed, #3 proven an equivalent mutant | tier deep (nav × rts) |
| T8  | Selection                          | `.../T8_selection.md`                    | done    | `3ec22d9` — `--test rts_selection` 31/31, `--test frame_allocations` 13/13; full gate green; gate hash `864147ca…` unchanged; golden green without regeneration; 8/8 mutations killed | tier standard |
| T9  | Gather loop, two resources         | `.../T9_gather-loop.md`                  | done    | `7d1b90c` (+ `5722420` box fix) — `--test rts_economy` 30/30, `rts_world` 48/48, `frame_allocations` 14/14; parent re-ran the whole gate: 0 FAILED lines, `nix flake check` passed, gate hash `864147ca…`, guards empty; 7/8 mutations killed, #6 proven equivalent | tier standard |
| T10 | Building placement + construction  | `.../T10_building-placement.md`          | done    | `772588b` — `--test rts_build` 37/37, `rts_economy` 30/30, `frame_allocations` 15/15; full gate green twice (plain and `MMD_REQUIRE_GPU=1`); gate hash `864147ca…`; golden green without regeneration; 9/9 mutations killed | tier standard |
| T11 | Production + supply                | `.../T11_production-and-supply.md`       | done    | `c8b5b33` — `--test rts_production` 39/39, `frame_allocations` 16/16; parent re-ran the whole gate: 829 passed / 0 failed, `nix flake check` passed, gate hash `864147ca…`, guards empty; 8/8 mutations killed | tier deep (repair) |
| T12 | World render packing               | `.../T12_world-render-packing.md`        | done    | `f009e3c` (+ `cbd89e9` box fix) — `--test rts_pack` 36/36, `frame_allocations` 17/17; both new GPU cases ran under `MMD_REQUIRE_GPU=1` and skip cleanly without a driver; gate hash `864147ca…`; golden green without regeneration; 9/9 mutations killed | tier deep (render × rts × camera × selection) |
| T13 | HUD                                | `.../T13_hud.md`                         | done    | `8195112` (+ `01f33b6` box fix) — `--test rts_hud` 30/30, `rts_pack` 36/36, `frame_allocations` 18/18; 45 test binaries no failures; gate hash `864147ca…`; golden green without regeneration; 8/8 mutations killed | tier standard |
| T14 | `rts` CLI subcommand               | `.../T14_rts-cli-subcommand.md`          | done    | `ba0dfc7` (+ `9ca4d5c` box fix) — `--test rts_cli_contract` 30/30, `cli_contract` 24/24 untouched; `cargo run -- rts --frames 600` exit 0; gate hash `864147ca…`; guards empty; testkit feature count 0 | tier standard |
| T15 | End-to-end acceptance              | `.../T15_end-to-end-acceptance.md`       | done    | `eff7864` (+ `a82e4cd`) — parent re-ran the acceptance command: exit 0, `crystal=86 gas=50 supply=9/20 units=8 buildings=3 nodes=10`; `rts_acceptance` 3/3 engine + 10/10 app; gate hash `864147ca…`; guards empty | tier deep (whole-system integration) |
| T16 | Docs, ADRs, phase close            | `.../T16_docs-and-phase-close.md`        | done    | `54b01bf` (+ `d2055f9`) — `validation_contract` 10/10; 5/5 doc mutations killed; full gate + acceptance green; post-review blockers fixed in `a974a3c` and independently re-reviewed with 6/6 fix verdicts PASS | tier deep (final honesty review) |

States: pending|running|done|failed|blocked_user|blocked_dep|skipped

## Assumptions

- Branch is `plan/rts-engine-prototype`, matching the repo's existing
  `plan/technical-prototype` / `plan/horde-sim-headroom` / `plan/zombie-collision`
  convention. The `make` skill's pre-flight text says `feat/{slug}`, but its own
  Auto-decide table and Git-rules section say `plan/{slug}`; repo convention and
  the majority of the skill agree, so `plan/` wins.
- Artefact dir is `ai-artifacts/` (hyphen), the repo's spelling, not the skill's
  `ai_artefacts/`. Progress ledger lives in `.tmp/`, matching the three previous
  runs' ledgers already tracked there.
- Commit trailer convention is taken from `git log`: `Co-Authored-By:` and no
  `Signed-off-by:`, because no commit currently on `main` carries a DCO trailer.
- No PR is opened. The plan does not ask for one.
- No user interaction is required by any ticket: T1 states explicitly that there
  is no account, API key or package to install beyond the pinned toolchain, and a
  grep across all sixteen ticket files finds no `TODO(user)`.
- Tier escalation to `deep` is applied only where a ticket genuinely spans
  subsystems (T2, T7, T12, T15) or is a repair attempt. The rest run `standard`,
  because the plan was written at `deep` and leaves workers no design decisions.
- T2 plan defects, resolved in-scope by the worker and accepted by the parent:
  (a) the ticket cited a GPU auto-skip helper in `tests/common/mod.rs` that does
  not exist — the real helper is `renderer_or_skip` in `render_correctness.rs`,
  and integration binaries cannot share a private helper, so the new GPU tests
  went there instead of `gpu_smoke.rs` (whose device cases are `#[ignore]` and
  would never have run under the ticket's own validation command);
  (b) the ticket called `props.png` cell (1,3) "the opaque panel-fill cell", but
  that cell is uniformly alpha-200 translucent — the alpha-255 assertion moved to
  cell (1,0), the blue diamond icon;
  (c) the ticket's `ui_layer_draws_over_the_world` case carried no overlay, so
  its own mandatory mutation 1 survived — an overlay ring was added so the
  mutation is actually killed.
- T2 added two things beyond the ticket's listed public API, both forced by the
  mutation list: `SpriteRenderer::pack_capacity()` (the only observation seam for
  the private `pack_scratch` capacity guard) and a free `texture_at` fn rather
  than a `&self` method (the pass holds `&mut self.depth` for its lifetime).
- The `tests/common/mod.rs` GPU auto-skip helper cited by T2 and T3 does not
  exist anywhere in the repo. The real pattern is private `renderer_or_skip` /
  `gpu_guard` fns replicated per integration-test binary, gated on
  `MMD_REQUIRE_GPU`; `#[ignore]`d GPU tests never run under this plan's own
  validation commands, so `#[ignore]` is not an acceptable substitute. Only T2
  and T3 cite the phantom path (grep-verified), so no later ticket inherits it.
- Workers reach ship's `locally-verified` bar by running the ticket's own
  TDD → impl → mutation → full-gate loop directly. T3 and T5 noted they executed
  that checklist rather than invoking `ship` as a separate tool. The gate that
  actually binds — real captured output for every Validation command before any
  commit — was met in both cases, so this is recorded, not treated as a failure.

## Decisions

- **D1 — a second sanctioned `sim/` visibility promotion (T7).** The ticket
  permits exactly one edit to the frozen phase-0 `sim/`: promoting
  `dir_from_vector`. The mandated agreement test between the RTS mover and the
  horde's step rule needs `sim::tick::step_admissible` to be nameable too.
  Authorised as `pub(crate)` (not `pub`, so it stays off the public API), with a
  `#[cfg(test)]`-gated re-export, on three conditions: visibility keyword only,
  no signature or body change, and the 5 000-agent gate hash must not move. All
  three held — `hash=864147ca3a0e09f7ebc5762b778fce193e705a2bc943ceaf67acf087581ee881`
  is unchanged. T16 must reconcile the ADRs against this.

## Residual risks

- **T12 — `RtsFrame::{world, ui}` are `pub` by ticket mandate**, so a caller that
  reorders or replaces groups breaks `world_group`'s `slot - SLOT_RTS_WORKER`
  index arithmetic. Pinned by a test, not by the type system.
- **T12 — a placement ghost near a map edge** tiles footprint cells outside the
  grid. `placement_valid` still refuses the placement as BAD, so it is a cosmetic
  overdraw on off-map ground, not an illegal build.
- **T12 — `pack_frame` moves its scratch out and back**, so a panic mid-pack
  leaves it empty and costs one reallocation on the next call. Same pattern as
  the pre-existing `box_select_into_selection`.
- **`VK_DRIVER_FILES=/nonexistent` behaves inconsistently on this host.** T13
  observed every GPU case still running instead of skipping; T14 observed it
  reliably forcing exit 3 for both `run` and `rts`. Same host, same week,
  opposite results, and neither worker could reconcile them. Earlier tickets
  cited this env var as proof their GPU tests skip cleanly on a driverless
  machine — that proof is therefore unreliable. What is NOT in doubt: the tests
  pass with a real device, and `MMD_REQUIRE_GPU=1` forces them to run. The
  skip-cleanly-without-a-GPU claim must be re-checked on a genuinely driverless
  host before any doc leans on it.
- **`gpu_smoke::the_hud_draws_over_the_world` is flaky** (T12/T13 surface, found
  by T14). Observed failing 3/3 by the T14 worker at commit `01f33b6` with no
  T14 code in the tree — pixel (700,950) read back `[0,0,0,0]` in both the
  world-only and world+HUD captures — and passing 2/2 on an independent parent
  re-run at the same commit minutes later. Cause not diagnosed. A GPU readback
  test that flips with transient device state is a real robustness defect in the
  test; it is not a regression in shipped behaviour and was out of T14's scope to
  fix. Owner: the T12/T13 render surface. This is the most likely source of a
  spurious red gate for the next person who runs the full suite.
- **T14 serialises its GPU-heavy subprocess tests behind a file-local `Mutex`**,
  after observing `VK_ERROR_DEVICE_LOST` under full 16-way parallelism. T15 hit
  the same wall and cut its own eight parallel GPU subprocesses down to two. In
  both cases the contention is avoided, not fixed: `VK_ERROR_DEVICE_LOST` under
  heavy parallel GPU test load is a property of this host that the suite now
  steers around.
- **`README.md`'s own gate block was not updated by T15** — the ticket named only
  `docs/05-testing.md`. T16 owns the index sweep and should catch it.
- **The acceptance script's exit line reports a state hash**
  (`cd1d28ce…` on the parent's run) that nothing currently pins. It is stable
  across the runs observed, but no test asserts it, so a silent behavioural drift
  in the RTS slice would not be caught the way the phase-0 `864147ca…` hash
  catches drift in the horde scene.

- ~~**T7 — stale cached flow field.**~~ Fixed post-review in `a974a3c`: orders
  now carry `{slot, epoch}`, re-acquire after eviction/invalidation, relocate
  deterministically if a completed footprint traps them, and the seeded HQ is
  stamped at load. Independent follow-up review passed all four correctness
  fixes. Keep this struck-through entry as provenance for why the epoch exists.
- **Epoch/LRU counter wrap is only theoretically handled.** Follow-up review
  found `FieldPool`'s epoch helper wraps but its LRU clock uses checked `+=` and
  can panic in debug after `u64::MAX` acquires; release wrap also loses exact LRU
  ordering, and an ancient handle can ABA-match after a full epoch cycle. No
  practical run can approach this count, so it is not a phase-close blocker.
- **Acceptance-process test gaps remain outside the post-review fix scope:**
  `a_depot_finishes_in_its_documented_time` proves completion within 2 000 ticks,
  not exact tick; world-level production tests do not prove `production_system`
  advances exactly once per tick (only `ProductionQueue::advance` is exact);
  final acceptance has ample supply (9/20), so supply enforcement is proven by
  `queueing_cannot_exceed_the_cap`, not by the vertical-slice run itself.
- **Phase-0 coverage reverse check is diluted by the combined phase-0/phase-1
  test corpus.** A deleted phase-0 test could theoretically be masked by a
  same-named phase-1 test, and the combined `MIN_SCANNED_TESTS` floor can hide
  phase-0 shrinkage behind phase-1 additions. Logged by scope reviewer; not
  fixed because it was explicitly outside the one blocker-fix worker's scope.
- **Renderer `group_ranges` reserves nine entries but does not bound group count.**
  Current RTS frames use five total groups, so no allocation occurs; a future
  caller passing more than nine can grow it during draw.
- **Knowledge graph update not run:** required command `graphify update .` failed
  with exact shell error `graphify: command not found`. `graphify-out/` remained
  unchanged; code and docs validation do not depend on it.

## Log

- 2026-08-10 pre-flight — branch `plan/rts-engine-prototype` cut from `main` @ `a33d96d`,
  pushed to origin. Baseline `cargo check --workspace --all-targets` clean.
- 2026-08-10 `6cf9402` — plan index, 16 ticket files, ADR 013/014/015 and the
  architecture page committed as plan scaffolding.
- 2026-08-10 T1 done — `01dfdf2`, pushed. 32/32 boxes checked. Five-item mutation
  list run both directions and recorded in the commit body. Worker flagged one
  honest gap: the TDD red phase was not independently observed (tests and impl
  were authored in one file, then run together); the mutation kills are the
  substitute evidence that the tests are load-bearing. Accepted — a stronger
  check than the one skipped.
- 2026-08-10 T2 start — tier `deep`.
- 2026-08-10 T2 done — `391d136`, pushed. 32/32 boxes checked. Ship terminal
  `locally-verified` at depth `production` (confirmation auto-approved by this
  skill, as designed). Golden byte-identity independently re-verified by the
  parent: `git diff --stat main -- lab/goldens/` empty. Worker resolved three
  plan defects in-scope rather than inventing scope — see Assumptions above.
- 2026-08-10 T3 done — `b974155`, pushed. 22/22 boxes checked. 6/6 mutations
  killed and reverted byte-identical. Same `tests/common/mod.rs` plan defect as
  T2, resolved the same way.
- 2026-08-10 T4 start — tier `standard`.
- 2026-08-10 T4 done — `41e6299`, pushed. 19/19 boxes checked. 7/7 mutations
  killed. The `IsoView` origin is now movable without moving the phase-0 scene:
  the worker pinned this by re-running the 5 000-agent gate scene across a
  `git stash` and showing the run hash is character-identical, which is stronger
  than the golden-diff check alone.
- 2026-08-10 T5 start — tier `standard`.
- 2026-08-10 T5 done — `6a05593`, pushed. 29/29 boxes checked. New tracked scene
  `assets/scenarios/rts_prototype_v1.{ron,sha256}` plus its generator
  `tools/scenegen/gen_rts_scene.py`; every phase-0 fixture byte-unchanged, as
  the `#[serde(default)]` assumption required. Two plan defects handled: the
  ticket's literal `rts_scene_path()` body resolved to a nonexistent path
  (fixed per its own doc comment), and adding a tracked `.ron` tripped the
  pre-existing `TRACKED_SCENES` inventory test (one-line const list updated).
- 2026-08-10 T6 start — tier `standard`. Parent re-verified T6's `From Depends`
  block against the shipped `scenario.rs`: `RTS_PROTOTYPE_V1`, the `RtsSpec`
  field list, `Scenario::rts()`, the three footprint constants, `MAX_SUPPLY_CAP`
  and `testkit::rts_scene_path()` all match the ticket as written. No drift to
  patch.
- 2026-08-10 T6 done — `3f47853`, pushed. New `crates/mmd-engine/src/rts/`
  (`mod`, `entity`, `economy`, `world`) plus `testkit/rts.rs`. The worker left
  Impl step 14 ("run the full validation block") unchecked while checking all ten
  Validation lines, so the parent re-ran the entire merge gate itself rather than
  accept the claim: fmt clean, clippy clean, 38 test-result lines all `0 failed`,
  `nix flake check` "all checks passed!", three `xtask --check` ok, gate scene
  exit 0 on `hash=864147ca3a0e09f7ebc5762b778fce193e705a2bc943ceaf67acf087581ee881`
  — the same hash T4 and T5 reported, so the phase-0 scene is bit-stable six
  tickets in. Box then flipped with that evidence inline (`9d27063`).
- 2026-08-10 T7 start — tier `deep` (spans `nav` and `rts`).
- 2026-08-10 T7 — worker escalated a genuine design conflict to the parent rather
  than inventing its way past it: the mandated
  `rts_step_admissible_agrees_with_the_sim` test is unreachable under the
  ticket's "`dir_from_vector` is the only `sim/` edit" rule, because
  `sim::tick::step_admissible` is module-private and no caller can see both
  functions. Parent authorised a second visibility-only promotion to
  `pub(crate)` (decision D1 below) and explicitly rejected the worker's option C
  — transcribing sim's rule into the test as a reference copy — because that
  test would assert the copy agrees with the copy, killing the mutation while
  providing zero drift protection.
- 2026-08-10 T7 done — `efe318c`, pushed. 28/28 boxes checked. The `sim/` diff is
  exactly two visibility keywords plus one `#[cfg(test)]` re-export, verified by
  the parent with `git diff main -- crates/mmd-engine/src/sim/`: 10 insertions,
  3 deletions, no behaviour line touched. Gate hash still `864147ca…`, which is
  the evidence that the promotion changed nothing.
- 2026-08-10 T8 start — tier `standard`.
- 2026-08-10 T8 done — `3ec22d9`, pushed. 23/23 boxes checked. 8/8 mutations
  killed. Selection is allocation-free by construction: `pick_at` / `box_select`
  walk slots with an inline `alive()` check instead of collecting live slots
  first, which is what lets them satisfy the ticket's frozen free-function
  signatures and the `alloc_guard` invariant at the same time.
- 2026-08-10 T9 start — tier `standard`. Parent re-verified T9's `From Depends`
  block against `rts/mod.rs`: the orders, field-pool, entity and economy export
  lists all match the ticket as written.
- 2026-08-10 T9 done — `7d1b90c`, pushed. 24/24 boxes checked after the parent
  settled step 14 the same way as T6, by re-running the full gate (`5722420`).
  Two things worth keeping: the worker *strengthened* a test rather than accept a
  surviving mutant — `mining_takes_the_documented_time` only checked end state, so
  an early-finish mutant survived it; and it proved mutant #6 genuinely equivalent
  (`GATHER_REACH_CELLS` 2.0 > `ARRIVAL_RADIUS_CELLS` 1.5, HQ footprint half-width
  6) with the arithmetic in the commit body instead of faking a kill.
- 2026-08-10 T10 start — tier `standard`.
- 2026-08-10 T10 done — `772588b`, pushed. 24/24 boxes checked, step 14 included
  this time. 9/9 mutations killed. Two edits reached back into earlier tickets'
  code and both were justified rather than drive-by: the mover's generic
  arrival-radius check was early-stopping `Gather` and `Build` orders as well as
  `Move` (latent since T7, exposed by this ticket's mandated approach-cell
  change), and T9's `the_gather_loop_allocates_nothing` was rewritten tick-by-tick
  with an early break because its fixed 2 000-tick snapshot became timing-fragile
  once approach cells moved. Both kept `rts_economy` green, which is the
  ticket's own stated validation requirement.
- 2026-08-10 T11 start — tier `standard`.
- 2026-08-10 T11 first attempt failed before validation: provider returned exact
  error `401 {"type":"error","error":{"type":"authentication_error","message":"OAuth access token has been revoked."},"request_id":null}` after partial source/test edits. No commit, no push. Partial diff retained for the one authorised repair worker, escalated to tier `deep` on a different provider/model.
- 2026-08-10 T11 repair done — `c8b5b33`, pushed, by a `deep` worker on GPT-5.6
  Sol that audited and completed the partial tree rather than restarting. 27/27
  boxes checked. Its report was far terser than the other workers', so the parent
  re-ran the entire gate rather than accept the summary: 829 passed / 0 failed
  across the workspace (`rts_production` 39/39, `frame_allocations` 16/16),
  `nix flake check` "all checks passed!", three `xtask --check` ok, gate scene
  exit 0 on `hash=864147ca3a0e09f7ebc5762b778fce193e705a2bc943ceaf67acf087581ee881`,
  phase-0 guard diff empty. The infra failure cost one attempt and no work.
- 2026-08-10 T12 start — tier `deep` (spans render, rts, camera and selection).
- 2026-08-10 T12 done — `f009e3c`, pushed. 23/23 boxes checked. The load-bearing
  T2 constraint held: `overlay` carries only procedural `SpriteInstance::ring`
  values, and every textured depth-off element (placement tiles, rally flag,
  icons, panel) sits in a slot-7 `ui` group. The worker split the final
  checkbox flip into its own commit `cbd89e9` rather than amend an already-pushed
  `f009e3c` — correct call, and it matches this run's no-force-push rule.
- 2026-08-10 T13 start — tier `standard`.
- 2026-08-10 T13 done — `8195112`, pushed, with the commit-subject box flipped in
  `01f33b6` on the same no-force-push reasoning T12 used. 25/25 boxes checked,
  8/8 mutations killed. The worker rejected its own first version of
  `the_hud_draws_over_the_world`: it had compared panel colour across two screen
  positions, which is invalid because the panel art is textured and
  semi-transparent rather than flat, and replaced it with a before/after
  comparison at a single pixel — the robust form of the same claim.
- 2026-08-10 T14 start — tier `standard`.
- 2026-08-10 T14 — worker escalated a red `MMD_REQUIRE_GPU=1 cargo test
  --workspace --locked` before publishing, having proved by `git stash -u` that
  `gpu_smoke::the_hud_draws_over_the_world` failed identically at the pristine
  base commit, 3/3 runs, pixel (700,950) reading `[0,0,0,0]`. The parent re-ran
  it independently with the worker's diff in the tree and it PASSED 2/2 —
  `gpu_smoke` alone 13 passed / 0 failed, and the whole workspace 945 passed /
  0 failed. Both observations stand: the test is flaky on this host, not
  regressed. See residual risks. The worker was right to hold rather than
  reinterpret "every Validation command passed" on its own authority.
- 2026-08-10 T14 done — `ba0dfc7`, pushed, commit-subject box in `9ca4d5c`.
  23/23 boxes checked. `cargo run -- rts` now runs the prototype scene, and this
  is the first ticket a human can drive interactively.
- 2026-08-10 T15 start — tier `deep` (whole-system integration).
- 2026-08-10 T15 done — `eff7864`, pushed, commit-subject box in `a82e4cd`.
  24/24 boxes checked. The parent re-ran the headline acceptance command itself
  rather than accept the report:
  `cargo run -- rts --frames 1600 --inject-input-file assets/scenarios/rts_acceptance_v1.script`
  exits 0 on
  `tick=1449 frames=1449 quit=true crystal=86 gas=50 supply=9/20 units=8 buildings=3 nodes=10 selected=1`
  — supply cap grown 10 → 20 by a built Depot, three buildings standing, eight
  units from six starting workers. `rts_acceptance` is 3/3 in the engine and
  10/10 in the app. The plan's headline success criterion is met and
  independently confirmed.
- 2026-08-10 T15 — the worker reduced, but did not eliminate, the
  `VK_ERROR_DEVICE_LOST` flakiness: it had been launching eight parallel
  1 600-frame GPU subprocesses, and now shares one run across five read-only CLI
  cases behind a `OnceLock`. Two failures in five full-suite runs before, 6/6
  clean after. Assertions were not weakened to get there.
- 2026-08-10 T16 start — tier `standard`.
- 2026-08-10 T16 done — `54b01bf`, pushed, commit-subject box in `d2055f9`.
  32/32 boxes checked. Phase 1 recorded honestly, `validation_contract` 10/10,
  full gate green, acceptance still green.
- 2026-08-10 final review fanout — correctness, tests and scope-regression at
  tier `deep`. Two correctness blockers found and empirically reproduced:
  (1) finishing a building invalidates field keys but leaves live orders riding
  stale slots, freezing walkers at the new wall; (2) slot eviction hangs Gather
  and Build forever, worse than the already-recorded Move hazard. Same root:
  orders cache a raw pool slot with no epoch/generation. Also found: units caught
  inside a finishing footprint are bricked; starting HQ is never stamped into
  nav; acceptance does not pan camera or gather gas despite claiming both.
- 2026-08-10 post-review fix worker — provider rate-limited during mutation
  checks after writing the requested implementation/tests. Exact error:
  `429 {"type":"error","error":{"type":"rate_limit_error","message":"This request would exceed your account's rate limit. Please try again later."},"request_id":"req_011CduJnBwvVw1ZL97DEk1BQ"}`.
  No commit, no push. Partial diff retained; continuation moved to a different
  provider/model at tier `deep`.
- 2026-08-10 post-review fixes done — `a974a3c`, pushed. Red evidence captured
  first for all four nav failures: finished-building Move pinned, Gather banked
  nothing after eviction, Build never finished after eviction, trapped unit did
  not escape, seeded HQ remained walkable. Fix adds field epochs and coherent
  re-acquisition for every transit order, deterministic blocked-cell relocation,
  initial HQ stamping, and acceptance pan + gas steps/assertions. Pan-off and
  gas-off mutations both killed the acceptance tests.
- 2026-08-10 independent post-fix review — first reviewer rate-limited; second
  provider completed read-only review. Verdict: all six requested fixes PASS,
  no blockers. One practical note: no alloc regression found, but the alloc test
  does not deliberately trap a unit, so an allocation inserted specifically in
  `nearest_unblocked_cell` would evade that gate.
- 2026-08-10 final parent validation — first workspace run hit the documented
  `gpu_smoke::the_hud_draws_over_the_world` flake at pixel (700,950), with both
  readbacks `[0,0,0,0]`. Isolated `MMD_REQUIRE_GPU=1 ... --test gpu_smoke`
  immediately passed 13/13, then the exact full gate passed: 980 tests / 0 failed,
  fmt + clippy + nix + all xtasks green, golden green, phase-0 byte guard empty,
  testkit feature count 0, gate hash `864147ca…`, strengthened acceptance exit 0
  on `crystal=54 gas=74 supply=9/20 units=8 buildings=3 camera=181.00092,150.99908`.
- 2026-08-10 graph refresh attempted per `AGENT.md`: `graphify update .` failed
  with exact error `/etc/profiles/per-user/aron/bin/bash: line 1: graphify:
  command not found`. Logged as residual; not a code or merge-gate failure.
