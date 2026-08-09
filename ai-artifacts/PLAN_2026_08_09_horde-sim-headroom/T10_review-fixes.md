# T10: Review fixes

**Plan:** `./ai-artifacts/PLAN_2026_08_09_horde-sim-headroom.md`
**Depends:** T0–T9 (all landed)
**Commit outcome:** the two blockers and eight should-fix findings from the
four-dimension review are closed; the written record stops claiming things the
code does not do.

## Context (self-contained)

- T0–T9 have all landed on `plan/horde-sim-headroom` (HEAD `64f17f4`). A
  fresh-context reviewer fanout at deep tier then reviewed `main..HEAD` along
  four dimensions: correctness, tests, scope-drift, security/soundness.
- **This ticket fixes what they found.** It adds no new capability. Everything
  below is either a defect against a requirement an earlier ticket already
  accepted, or a written claim that is false.
- **The pinned gate digest is
  `864147ca3a0e09f7ebc5762b778fce193e705a2bc943ceaf67acf087581ee881`**
  (`cargo run -- run --agents 5000 --frames 300`). Items 1, 2, 4, 5, 6, 7, 8, 9
  and 10 below must not move it. **Item 3 (the ring radius) is a render-layer
  change and must not move it either** — the ring is drawn from simulation
  state, it does not feed back into it.
- `BODIED_STACK_HASH` and `BODYLESS_GRID_PRE_SEPARATION_HASH` must not move.
- The reviewers verified a great deal as sound; **do not re-litigate any of it**:
  the pool's range partition and `unsafe impl Send`, the phase-seed formula, T2's
  stamp read-guard, T4's row concatenation order, T5's one-class IEEE exactness,
  T9's occlusion/cull/depth (machine-verified on a real GPU), and the fact that
  T8 and T9 touched no sim code.

## Requirements

### 1. [BLOCKER, scope] Finish T0's population sweep — 14 live sites

T0 Impl step 11 recorded "0 residual hits", but its grep pattern
(`50000|50_000|100_000|100000`) matches neither the `50k` contraction nor the
space-separated `50 000`. These survive and are **not** on T0's
designated-history allowlist. Two are functional, not prose:

- `src/run.rs:350` — **the shipped binary's window title** is
  `millions_must_die — moving 50k`, naming a population `--agents 5001` is now
  rejected for. Retitle it without a population figure (or with the live one).
- `tools/scenegen/gen_collision_scenes.py:51` — emits
  `stretch_agent_count: 20000`, so **the generator now produces a `.ron` the
  loader rejects** (`COLLISION_SCENE_MAX_AGENTS` was folded onto
  `MAX_LIVE_AGENTS = 5_000`). Fix the emitted value and, if the script has a
  self-check, make it assert against the cap.

Prose sites, all to be corrected to the live ceiling:
`src/run.rs:1`, `README.md:9`, `AGENT.md:16`, `docs/CONTEXT.md:34`,
`docs/05-testing.md:39`, `docs/05-testing.md:42`,
`crates/mmd-engine/src/sim/collision.rs:21`,
`crates/mmd-engine/src/testkit/mod.rs:72`,
`crates/mmd-engine/src/testkit/mod.rs:316`,
`crates/mmd-engine/src/testkit/fixtures.rs:46`, `tests/cli_contract.rs:206`,
`docs/agent-collision-architecture.html:28,33,72`.

Notes:
- `docs/05-testing.md:42` ("because 50 000 sprites cannot be laid out on one
  screen without overlapping") is now factually dead — T0 made the body exactly
  half a sprite. Rewrite, do not just renumber.
- `collision.rs:21` justifies `MAX_NEIGHBOURS` with a 50 000-agent seeding
  figure. Re-derive the justification at the live ceiling or state it
  qualitatively; **do not invent a number you have not computed.**
- `docs/agent-collision-architecture.html` asserts `30×30` sprites and a table
  row `technical_prototype_v1 | 50 000 | 102 | 0.398 cell ≈ 1.6 px` — every
  number T0 changed. Correct the page.
- `README.md`, `AGENT.md`, `docs/05-testing.md` are in `LIVE_DOCS`: every edit
  must avoid `PERF_THRESHOLD_TOKENS` + `EXTRA_PERF_CLAIM_TOKENS`.
- **Then re-run the sweep with a pattern that catches all three spellings**, e.g.
  `grep -rniE '(50|100)[ _]?(000|k)\b'` over tracked files, and record the
  residue. Legitimate remaining hits: `crates/mmd-engine/src/bench/policy.rs`
  and its comment header, `tools/mmd-lab/**`, `lab/**`,
  `schemas/benchmark-report-v*.json`, `schemas/release-proof-v1.json`,
  `docs/technical-prototype-results.md`, `docs/ADR/00*`,
  `ai-artifacts/PLAN_2026_08_08_zombie-collision/`,
  `ai-artifacts/PLAN_2026_08_09_horde-sim-headroom*`, `Cargo.lock`,
  `crates/mmd-engine/src/testkit/rng.rs` (the FNV prime), and
  `tests/validation_contract.rs` (the deliberate bad-doc fixture).
  `crates/mmd-engine/src/render/renderer.rs` `MAX_INSTANCES` stays at its value
  (a GPU buffer capacity) but **its doc comment saying "hard 50k; stretch 100k"
  is stale — fix the comment, keep the constant.**

### 2. [BLOCKER, tests] Pin the mass scale factor

`crates/mmd-engine/tests/separation.rs:804`
(`a_heavier_neighbour_pushes_a_lighter_one_harder`) asserts only
`|sep0| > |sep1|`, which holds for the correct scale `mass[j] * inv_mass[i]`
(4:1), for `mass[j]` alone (2:1) and for `inv_mass[i]` alone (2:1). Proven by
mutation: replacing the scale with `mass[j] as f32` leaves the **entire
`mmd-engine` suite green**, and `collision_sprite_v1` ships
`mass_class_count: 2`, so the shipped scene's physics can be silently wrong.

Add an assertion that pins the **ratio**, not just the ordering — a two-class
pair at a known separation should give a 4:1 magnitude ratio, asserted with an
explicit tolerance. `mass_classes_change_the_bodied_digest` is an `assert_ne!`
and is satisfied by any wrong scale; **add an `assert_eq!` digest pin for a
two-class run** so the multi-class walk has the same class of guard the
one-class walk has.

Your new test must FAIL under the mutation `let scale = mass[j] as f32;` and
under `let scale = inv_mi;`. Prove both by running them, then revert.

### 3. [should-fix, correctness] The ring is drawn √2 too large

`crates/mmd-engine/src/runtime.rs:91` (`ring_quad_size_px`) returns
`[2·r·tile_w, 2·r·tile_h]`. The simulation's contact test is Euclidean in cell
space, and the projection `[[tw/2, -tw/2], [th/2, th/2]]` maps a radius-`r`
circle to a screen ellipse with semi-axes `r·tw/√2` and `r·th/√2` — the matrix
`M·Mᵀ` is diagonal (`[[tw²/2, 0], [0, th²/2]]`), so the image ellipse is
**axis-aligned in screen space** and an axis-aligned quad is still the right
primitive. For the tracked scenes (`r = 6`, `tw = 8`, `th = 4`) the true ellipse
is `67.9 × 33.9 px`; the drawn one is `96 × 48 px`.

Consequence: two agents at exactly contact distance render with **overlapping**
rings instead of tangent ones, so the overlay cannot be used to judge contact —
the one thing it exists for, and what T8's requirement ("a ring at its *real*
body radius") demands.

- Divide both axes by `√2`. The shader needs no change.
- **Correct the false claims** this exposed:
  - `crates/mmd-engine/src/runtime.rs:486-489` says the ring "traces the true
    contact circle lying on the isometric floor, never an approximation of it".
    After the fix that is true — verify it is, and keep the wording honest.
  - `crates/mmd-engine/tests/render_correctness.rs:565-568` claims "the ring
    shows the radius the simulation actually separates on".
  - `docs/ADR/012_ADR_starcraft_scale_and_isometric_render.md:101-105` records
    the √2 gap as an accepted follow-up. Replace that with the resolution.
- **Add the assertion the existing tests lack:** they re-derive the expectation
  from the same formula (`render_correctness.rs:639`) and never assert tangency.
  Assert the *geometric* property — two bodies at exactly `2r` cells apart have
  tangent rings — so the formula cannot drift again.
- Regenerate any moved GPU golden, **look at the images before committing
  them**, and say in Outputs that they moved and why.

### 4. [should-fix, tests] `bin_row_clamps_to_the_last_column` is vacuous

`crates/mmd-engine/tests/separation.rs:155` queries row 0, which holds no agents
in the test's own fixture, so it compares two empty slices and only guards
against a panic. Proven by mutation twice; a clamp-path-only mutant at
`crates/mmd-engine/src/sim/spatial.rs:220` passes the whole suite. Aggravating:
`accumulate_separation_range` pre-clamps, so this branch is reachable **only**
from this test — nothing else covers it at all.

Query a populated row (row 2 in that fixture). Verify your fixed test fails
under `let hi = if bx1 >= self.cols { bx0 } else { bx1 };`, then revert.

### 5. [should-fix, tests] `spatial_survives_a_stamp_wrap` is vacuous

`crates/mmd-engine/tests/separation.rs:210` wraps on a *fresh* grid, where every
`counts[b]` is 0, so `count_stamp[b] == stamp == 0` resolves correctly by
coincidence. Proven by execution: deleting the guard at
`crates/mmd-engine/src/sim/spatial.rs:128` leaves the test passing.

Rewrite it to construct the actual hazard: populate bins under a stamp of `0`,
then wrap back to `0`, and assert the counts do not resurrect. Verify it fails
with the guard deleted, then restore the guard.

### 6. [should-fix, tests] Nothing automated pins the gate digest

`864147ca…1ee881` appears only in plan markdown, the manual checklist and
ADR 012 — zero occurrences under `crates/`, `tests/`, `src/`, `tools/`. Every
ticket's "digest reproduced" is a re-typed human observation. A change that
alters the gate scene's 300-tick walk while leaving the two 32-agent synthetic
digests intact ships green.

Add a test that runs the gate scene headless for its pinned tick count and
asserts the digest. Constraints:
- It must run in the default `cargo test --workspace` — not `#[ignore]`d, not
  behind a feature the merge gate does not enable. If its runtime makes that
  unacceptable, say so explicitly in Outputs and pin a shorter but still
  gate-scene-scale walk rather than skipping the guard.
- It must not require a GPU.
- Put the constant somewhere a future ticket will find it, next to
  `BODIED_STACK_HASH`.

### 7. [should-fix, tests] `shader_defines_match_their_rust_mirrors` checks the wrong file

`crates/mmd-engine/tests/render_correctness.rs:956` parses
`#define MMD_DEPTH_EPSILON` / `MMD_ALPHA_CUTOFF` out of `shaders/sprite.hlsl`,
but the blob the app and every GPU test load is
`shaders/generated/sprite.{vert,frag}.spv`, built from `shaders/glsl/*.glsl`,
which hard-code `0.0000152587890625` (`sprite.vert.glsl:41`) and `0.5`
(`sprite.frag.glsl:28`) as bare literals. So the constant exists three times and
the test pins two — edit the HLSL and the Rust mirror together, forget the GLSL,
and the test whose whole job is catching that stays green.

Extend it to also parse and pin the GLSL literals. Its docstring ("exists once
in Rust … and once in the shader") is now false — correct it.

### 8. [should-fix, security] The pool's ordering argument is circular

`crates/mmd-engine/src/sim/pool.rs:186-203` claims the `publish` / `completed`
release-acquire pairs supply happens-before edges *independent of* the
`Barrier`. An acquire load only synchronizes-with a release store if it reads
that store's value, and only the barrier's internal mutex guarantees that.
`shutdown` (`pool.rs:379`) has **no independent edge at all**: were its
`Acquire` load to observe `false`, the worker falls through to
`unsafe { *shared.job.get() }` and either builds a slice from a null pointer
(pool created and dropped without a tick) or writes through a dangling one
(dropped after a tick) — UB, not the hang the comment anticipates.

The reviewer judged this **not exploitable** — `std::sync::Barrier` is
`Mutex` + `Condvar` and does establish the edge — and could not construct a
trigger. But it is invisible on x86 TSO and ADR 006 names an M4 as reference
hardware.

Close the gap: either give `shutdown` an edge that does not depend on the
barrier, or state plainly in the SAFETY comment that the barrier is the
synchronising primitive and the counters are corroborating rather than
independent. **Do not leave a comment that overstates what the code proves.**
Whichever you choose, `threads_do_not_change_the_walk` and
`threads_do_not_change_an_amortised_walk` must stay green.

### 9. [should-fix, scope] The architecture page has no render half

`docs/horde-sim-headroom-architecture.html` has a **zero-byte diff** across the
whole branch, yet T7's Outputs claim it was "verified and corrected", and its own
requirement demands it show "the render path from cell space through the
isometric projection to the depth-ordered draw". Grep counts in the shipped
file: `isometric` 0, `depth` 0, `hitbox` 0, `screen` 0 — T8 and T9, over half
the branch's code, are absent.

Add the render half: cell space → 2:1 projection → depth key → depth-ordered
draw with alpha-test cutout, plus the ring's shared-pass sentinel. Keep the page
self-contained (no external CSS/JS/fonts) and free of speed claims, matching the
existing page's conventions.

### 10. [should-fix, scope] The plan index asserts three things that did not ship

`ai-artifacts/PLAN_2026_08_09_horde-sim-headroom.md`:
- Lines ~279-280, the retune table: `SCALE_COUNTS` → `[500, 1_000, 2_500,
  5_000]` and `GATE_AGENT_COUNT`/`STRETCH_AGENT_COUNT` → `5_000`. **Not
  shipped** — decision D1 kept the frozen phase-0 ladder and added a comment-only
  header instead. Reconcile the table with what shipped and point at the
  reasoning.
- Lines ~44-45 and A20 (~line 209): the ring "on a second pipeline
  (`shaders/debug_ring.hlsl`)". **Not shipped** — it is a branch inside
  `shaders/sprite.hlsl` via a `uv_rect.x` sentinel, same pipeline, same pass
  (`a_ring_and_a_sprite_share_a_pass`). No `shaders/debug_ring.hlsl` exists.
- Also add `T10` to the ticket order table and the Tickets list.

## Out of scope — log, do not fix

Record these in Outputs as accepted residual risk. Do **not** spend the ticket
on them:

- `pool.rs:330-334` — `debug_assert!(completed >= handles.len())` is vacuous
  after tick 1 (`completed` is monotone and never reset). The `Acquire` load
  still does its real memory-edge job; only the assertion is dead.
- `pool.rs:295-335` — `in_use` is not RAII, so a panic between the `swap` and
  the `store(false)` latches it and every later tick misdiagnoses as
  "two threads share one pool". Not UB.
- `pool.rs:401-418` — the worker's `arm_worker()` covers `run_chunk` only, not
  `gate.wait()` / `done.wait()` / `completed.fetch_add`. True today (futex-backed
  `Barrier` does not allocate) but unproven; and *deleting* the arm is
  undetectable, since no worker allocates and every assertion is `== 0`.
- `SeparationPool::new` — a `thread::spawn` failure partway through parks the
  already-spawned workers forever. Needs an `Option<Self>` + condvar redesign.
- `CollisionParams` has `pub` fields and `Simulation::new_custom` is `pub`, so
  the scenario bounds do not bind the programmatic path: `mass_classes = 256`
  truncates a mass to `0` → `inv_mass = inf` → NaN positions; `threads = u32::MAX`
  attempts 4·10⁹ spawns. Not reachable from any `.ron`.
- `crates/mmd-engine/src/scenario.rs:547-556` — the "knob on a pass that never
  runs" guard keys on `collision_radius_q8 == 0` only, but the pass is also
  skipped when `separation_strength_q8 == 0`, so a scene can declare itself
  threaded and amortised and do nothing.
- `crates/mmd-engine/src/render/renderer.rs:391-401` — `set_depth_params` is
  opt-in; a caller who forgets it normalises depth over 1080 and ties every
  sprite past that band. Both in-tree callers do call it.
- With `separation_phases > 1` the `window <= 1` early-out reads *stale* bin
  membership against a *live* position, so a recycled agent can take the
  early-out and write `sep = 0` where the full walk would have pushed. One tick,
  one agent. **T4's bit-exactness claim is therefore conditional on
  `phases == 1`** — say so wherever it is stated.
- `MIN_SCANNED_TESTS` 168 → 277 is inflated: `every_system_has_a_test` counts
  `declared_tests(entry.file)` per entry and `render_correctness.rs` is named by
  three entries, so its 28 tests count 84.
- The merge gate's pixel limb is tautological — the Ubuntu fixture is a
  byte-copy of the golden. **Pre-existing on `main`**, preserved not introduced.
- `shaders/glsl/` ↔ `shaders/generated/*.spv` has no recompile-and-compare check.
  Documented in `shaders/generated/README.md`.
- `BenchPolicy::production()` names populations the validator now refuses, so
  that ladder is unrunnable. Frozen history per D1.
- No shipped scenario sets `separation_threads > 1`, so the pool executes only
  under tests (plan **A9**, deliberate).
- After T9, "contact distance is one full sprite width" is direction-dependent
  on screen; the equality holds in cell space, where T0 measured it.

## Impl steps

- [ ] 1. `graphify query "population constants and scenario caps"` and
      `graphify query "hitbox ring quad size projection"` to orient.
- [ ] 2. Item 1 — the population sweep, including the window title and the
      scenegen fix. — validate: the three-spelling grep returns only allowlisted
      residue; `cargo test --workspace --locked` green.
- [ ] 3. Item 2 — pin the mass scale ratio + a two-class digest. — validate: new
      test fails under both mutations, passes on the real code.
- [ ] 4. Item 3 — the √2 ring fix, the claim corrections, the tangency
      assertion, goldens eyeballed. — validate: `MMD_REQUIRE_GPU=1 cargo test
      -p mmd-engine --test render_correctness` green with 0 skips.
- [ ] 5. Item 4 — fix the clamp test. — validate: fails under the clamp mutant.
- [ ] 6. Item 5 — fix the stamp-wrap test. — validate: fails with the guard
      deleted.
- [ ] 7. Item 6 — the automated gate-digest pin. — validate: runs in default
      `cargo test --workspace`, no GPU, asserts `864147ca…1ee881`.
- [ ] 8. Item 7 — extend the shader-define test to the GLSL. — validate: fails
      if a GLSL literal is edited alone.
- [ ] 9. Item 8 — the pool ordering comment / edge. — validate: SAFETY comment
      claims exactly what the code proves; thread-invariance tests green.
- [ ] 10. Item 9 — the architecture page's render half. — validate: page
      mentions the projection, the depth order and the ring pass; no external
      asset reference; no perf token.
- [ ] 11. Item 10 — reconcile the plan index. — validate: no surviving claim of
      a retuned `SCALE_COUNTS` or of `shaders/debug_ring.hlsl`.
- [ ] 12. Append the residual-risk list to Outputs.
- [ ] 13. `graphify update .`.

## Outputs

- Ten review findings closed; the residual list recorded above carried into
  Outputs verbatim on completion.
- The gate digest, unchanged and now **pinned by a test** rather than by prose.

## Validation

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo test --workspace --locked` — green
- [ ] `MMD_REQUIRE_GPU=1 cargo test -p mmd-engine --test render_correctness` —
      green, `grep -c '^SKIP '` = 0
- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- [ ] `nix flake check`
- [ ] `cargo run -p xtask -- bootstrap --check` / `shaders --check` /
      `atlases --check`
- [ ] `cargo run -- run --agents 5000 --frames 300` — `hash=864147ca3a0e09f7ebc5762b778fce193e705a2bc943ceaf67acf087581ee881`
- [ ] `cargo test -p mmd-lab --test merge_gate` — green
- [ ] `no_perf_claim_in_docs` green; `every_system_has_a_test` green
- [ ] the three-spelling population grep returns only allowlisted residue
- [ ] `graphify update .` run; `git status` shows no `graphify-out/` entry
- [ ] manual: rings are tangent, not overlapping, for two agents at contact —
      recorded in `ai-artifacts/manual_test_checklist.md`
