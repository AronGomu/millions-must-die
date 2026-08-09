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

- [x] 1. `graphify query "population constants and scenario caps"` and
      `graphify query "hitbox ring quad size projection"` to orient.
- [x] 2. Item 1 — the population sweep, including the window title and the
      scenegen fix. — validate: the three-spelling grep returns only allowlisted
      residue; `cargo test --workspace --locked` green.
      All 14 named live sites corrected, plus **five** the ticket's list did not
      reach but which are the same defect: `testkit/fixtures.rs` x2 (the two
      collision-scene doc comments — `10 000 agents`, `1.25-cell` / `3.75-cell`
      body, `30 px sprite`, every figure stale), and three live claims on
      architecture pages — `docs/simulation-navigation-architecture.html`
      (`<h1>Flow Field + 50k-Agent SoA</h1>`, SVG note `capacity = 50k / 100k`),
      `docs/sprite-renderer-architecture.html` (`50k compact instances`) and
      `docs/technical-prototype-architecture.html` (SVG note
      `Simulation SoA · 50k · fixed tick`). The last three were **missed by the
      first pass of this ticket and caught by review**; recorded here rather
      than quietly folded in.
      Scenegen now reproduces both tracked `.ron`s byte-identically
      (`git status` clean after a run) and its new `MAX_LIVE_AGENTS` assert was
      proven to fire.

      **The re-run sweep and its real residue.** The ticket's stated allowlist
      is not sufficient — run against it the sweep returns 21 lines, not a
      handful. Every extra hit is the same frozen bench/lab family the ticket
      already allowlists in part, so the allowlist is **widened here explicitly**
      rather than left implicit in a worker's report. Command:

      ```sh
      git grep -nIE '(50|100)[ _]?(000|k)\b' -- . \
        | grep -vE '^(\.tmp/|ai-artifacts/|Cargo\.lock)' \
        | grep -vE '^(crates/mmd-engine/src/bench/|crates/mmd-engine/tests/benchmark_policy\.rs|tools/mmd-lab/|lab/|docs/lab/|schemas/)' \
        | grep -vE '^(docs/technical-prototype-results\.md|docs/ADR/00)' \
        | grep -vE '^(crates/mmd-engine/src/testkit/rng\.rs|tests/validation_contract\.rs|assets/scenarios/technical_prototype_v1\.ron)'
      ```

      Additions to the ticket's allowlist, each with its reason:
      `crates/mmd-engine/src/bench/report.rs` and
      `crates/mmd-engine/tests/benchmark_policy.rs` (the same frozen
      `production-v1` ladder as `bench/policy.rs`, which the ticket does
      allowlist); `docs/lab/**` and `schemas/**` (the frozen validation-lab
      machinery); `assets/scenarios/technical_prototype_v1.ron` (the hits are
      obstacle **cell indices** — `49957`, `100000` — not populations);
      `ai-artifacts/**` (plan history, of which the ticket already allowlists
      two directories); `.tmp/**` (untracked-by-intent scratch owned by other
      agents).

      **Residue after all edits — 6 lines, all legitimate:**
      - `crates/mmd-engine/src/render/renderer.rs:42` — `MAX_INSTANCES`, the GPU
        buffer capacity the ticket explicitly says to keep. Comment rewritten.
      - `docs/05-testing.md:168,170` — the two frozen bench-ladder rows, both
        already marked "frozen, **not a gate**".
      - `docs/local-validation-lab-architecture.html`,
        `docs/sprite-renderer-architecture.html`,
        `docs/technical-prototype-architecture.html` — one hit each, all naming
        a **frozen perf-ladder tier** (`50k p95/p99 miss`, `100k stretch miss`,
        `100k perf miss → report only`), never a live population. These are the
        same class as the `docs/05-testing.md` rows above. On
        `technical-prototype-architecture.html` the surrounding "Success" card
        was additionally marked *"original phase-0 criteria — superseded"*,
        because renumbering `50k at 1080p` in place would have manufactured a
        **false current criterion** — the `p95 ≤ 16.67 ms` lines beside it are
        retired, so the honest fix is to mark the card as history, not to update
        its number.
- [x] 3. Item 2 — pin the mass scale ratio + a two-class digest. — validate: new
      test fails under both mutations, passes on the real code.
      Evidence: `MUTANT-A` (`scale = mass[j] as f32`) and `MUTANT-B`
      (`scale = inv_mi`) each failed both
      `a_heavier_neighbour_pushes_a_lighter_one_harder` and
      `mass_classes_change_the_bodied_digest`; reverted, 42/42 green.
      `BODIED_TWO_CLASS_STACK_HASH = b4ea0656…1ab35b`.
- [x] 4. Item 3 — the √2 ring fix, the claim corrections, the tangency
      assertion, goldens eyeballed. — validate: `MMD_REQUIRE_GPU=1 cargo test
      -p mmd-engine --test render_correctness` green with 0 skips.
      Derivation re-verified independently and numerically before editing; it
      agrees with the ticket's. Quad is now `[√2·r·tw, √2·r·th]` =
      `67.88225 × 33.941124 px` on the tracked scenes. GPU run: 29 passed,
      `grep -c '^SKIP '` = 0. New `the_rings_of_two_touching_bodies_are_tangent`
      fails under the restored pre-T10 formula with
      `centres are 1.4142 radii apart, not 2`; reverted, green.
      **No golden moved** — `git status lab/` clean; the golden scene is
      `static_demo_groups()` and is ring-free. Offscreen PNG rendered before and
      after and looked at: at exactly contact distance the old rings visibly
      intersect, the new ones are tangent.
      Gate digest re-run after the change:
      `hash=864147ca3a0e09f7ebc5762b778fce193e705a2bc943ceaf67acf087581ee881`.
- [x] 5. Item 4 — fix the clamp test. — validate: fails under the clamp mutant.
      Evidence: `let hi = if bx1 >= self.cols { bx0 } else { bx1 };` →
      `bin_row_clamps_to_the_last_column` failed `left: [6] right: [6, 7]`;
      reverted, green.
- [x] 6. Item 5 — fix the stamp-wrap test. — validate: fails with the guard
      deleted. Evidence: guard at `spatial.rs:128` deleted →
      `spatial_survives_a_stamp_wrap` panicked (`index out of bounds: the len
      is 5 but the index is 5` inside `rebuild`'s counting sort) and was the
      **only** failure in the 42-test suite; restored, green.
- [x] 7. Item 6 — the automated gate-digest pin. — validate: runs in default
      `cargo test --workspace`, no GPU, asserts `864147ca…1ee881`.
      `the_gate_scene_walk_is_pinned_to_its_published_digest` in
      `crates/mmd-engine/tests/separation.rs`, next to `BODIED_STACK_HASH`.
      **Full 300-tick walk at 5 000 agents, not shortened** — the whole
      `separation` suite is 1.05 s. Not `#[ignore]`d, no feature gate, no GPU.
      Gap proven closed: `return true;` in place of the `diagonal_clear` corner
      check (`sim/tick.rs:213`) leaves `a_bodied_scenario_is_pinned_to_a_golden_digest`,
      `one_mass_class_leaves_every_agent_equal`,
      `mass_classes_change_the_bodied_digest` and
      `a_bodyless_scenario_walks_the_flow_only_path` **all green** and fails
      only the new pin; reverted, 43/43 green.
- [x] 8. Item 7 — extend the shader-define test to the GLSL. — validate: fails
      if a GLSL literal is edited alone. Both proven, one at a time:
      `sprite.vert.glsl` epsilon → `0.000030517578125` failed
      (`left: 3.0517578e-5 right: 1.5258789e-5`); `sprite.frag.glsl`
      `texel.a - 0.5` → `0.25` failed (`left: 0.25 right: 0.5`). Both reverted,
      `git status shaders/` clean, test green. Docstring corrected from "exists
      twice" to three copies.
- [x] 9. Item 8 — the pool ordering comment / edge. — validate: SAFETY comment
      claims exactly what the code proves; thread-invariance tests green.
      **Both halves done.** (a) `shutdown` now has an edge that does not depend
      on the barrier: the worker spins on `publish` until the epoch differs from
      the one it last ran, so the acquire load provably *reads* the release
      bump, and the shutdown arm is the only other exit — it can no longer fall
      through to `unsafe { *shared.job.get() }` on a stale `false`. (b) the
      inbound `completed` direction is now described as corroborating, with
      `done.wait()` named as the primitive that actually orders it — no
      overstated claim left. New `a_pool_dropped_before_its_first_tick_shuts_down`
      and `a_pool_dropped_after_a_tick_shuts_down` cover the two UB paths the
      reviewer named. `threads_do_not_change_the_walk` and
      `threads_do_not_change_an_amortised_walk` green over 10 consecutive runs;
      pool unit tests green over 15.
- [x] 10. Item 9 — the architecture page's render half. — validate: page
      mentions the projection, the depth order and the ring pass; no external
      asset reference; no perf token.
      Grep counts, was → now: `isometric` 0 → 5, `depth` 0 → 18, `hitbox` 0 → 2,
      `screen` 0 → 3, `projection` 0 → 5, `cutout` 0 → 4. External asset refs:
      none. Perf tokens (`PERF_THRESHOLD_TOKENS` + `EXTRA_PERF_CLAIM_TOKENS`):
      none. Tag balance checked against the pre-edit file: both `unclosed: []`.
      Also corrected two now-false claims the page carried: "so is the renderer"
      (untouched) and "the flow field, the renderer and the shaders are outside
      this plan entirely".
- [x] 11. Item 10 — reconcile the plan index. — validate: no surviving claim of
      a retuned `SCALE_COUNTS` or of `shaders/debug_ring.hlsl`.
      Retune table rows now read **unchanged / not shipped**, with the two rows
      that *did* ship (`TEST_SHORT_SCALE_COUNTS`, `TEST_SHORT_GATE_AGENT_COUNT`)
      added and D1's reasoning quoted from `bench/policy.rs`'s own header. Both
      surviving `debug_ring` hits (lines 45, 215) are explicit **negations**;
      `grep -n 'debug_ring'` shows no remaining assertion. T10 added to the
      mermaid flowchart, the ticket order table and the Tickets list.
- [x] 12. Append the residual-risk list to Outputs.
- [x] 13. `graphify update .` — `Rebuilt: 3908 nodes, 7537 edges, 250
      communities`. `graphify-out/` is gitignored and absent from
      `git status`; it is never staged.

## Outputs

All ten review findings closed. Nothing from the "Out of scope" list was
touched; it is carried into Outputs verbatim below, as accepted residual risk.

### What shipped, per item

1. **Population sweep (blocker, scope).** All 14 named live sites corrected, plus
   two the reviewer's list did not reach but which are the same defect in the
   same files: `testkit/fixtures.rs`'s two collision-scene doc comments
   (`10 000 agents`, `1.25-cell body`, `3.75-cell body`, `30 px sprite` — every
   one stale). Functional half: the window title no longer names a population
   the CLI refuses (`millions_must_die — flow-field horde`), and
   `tools/scenegen/gen_collision_scenes.py` now regenerates both tracked scenes
   **byte-identically** (`git status` clean after a run) instead of emitting a
   `.ron` the loader rejects — it was stale on `stretch_agent_count`,
   `sprite_size_px`, both radii, `hard_agent_count`, and was missing all three
   phase-0.5 knobs outright, so fixing only the one named line would have left
   it still producing an unloadable file. It now carries a `MAX_LIVE_AGENTS`
   self-check, proven to fire. `MAX_INSTANCES` keeps its value; only its comment
   changed. `collision.rs`'s `MAX_NEIGHBOURS` justification was **re-derived**,
   not renumbered: `5000 / 127 = 39.37`, i.e. 47 spawn cells of 40 and 80 of 39.
2. **Mass scale (blocker, tests).** The 4:1 ratio and both individual magnitudes
   are pinned against a push derived from the falloff rather than from the pass,
   and `BODIED_TWO_CLASS_STACK_HASH` gives the multi-class walk the same class of
   guard the one-class walk has.
3. **The ring (correctness).** `√2` divided out of both axes. The derivation was
   re-checked independently and numerically before editing and agrees with the
   ticket's. Three claim sites corrected (`runtime.rs`, `render_correctness.rs`,
   ADR 012, which now records the resolution rather than an accepted follow-up),
   plus the two stale manual-checklist entries, corrected **in place**. No
   golden moved; no state hash moved.
4. **Clamp test.** Now queries a populated row, and asserts its own premise so it
   cannot silently go vacuous again.
5. **Stamp-wrap test.** Rewritten to construct the real hazard (two wraps, not
   one). It is the *only* test in the 42-test suite that catches the deleted
   guard.
6. **Gate digest.** Pinned by a test, in the default `cargo test --workspace`,
   no GPU, **at the real scene and the real 300 ticks** — no shortened walk was
   needed; the whole `separation` suite runs in about a second.
7. **Shader mirrors.** Extended to both GLSL literals, each proven to fail alone.
8. **Pool ordering.** `shutdown` given a barrier-independent edge, and the
   inbound counter's argument downgraded to what it actually proves.
9. **Architecture page.** Render half added; two false claims in the existing
   text corrected.
10. **Plan index.** Reconciled.

### Accepted residual risk — carried verbatim from the ticket's out-of-scope list

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

### New residual risk introduced by this ticket

- The worker's publish-epoch wait is a **spin**, not a park. It is bounded by the
  memory model's eventual-visibility guarantee and in practice runs zero
  iterations, because the barrier has already ordered both stores by the time it
  is reached — but it is a busy-wait on paper and worth knowing about if the pool
  is ever driven by something other than a barrier.
- The three-spelling population grep needed a wider allowlist than the ticket
  gave. The widened allowlist is now written into Impl step 2 with a reason per
  entry, rather than left in a worker's report.
- **The two pool drop tests cannot falsify the memory ordering, and say so.**
  The pre-fix worker — a single unconditional `shutdown` load falling through to
  the job cell — passes them, and so does a deliberately UB variant that reads
  the null `Job` on the shutdown path. That is not a hole in the tests but a
  property of the defect: it is a memory-model bug, invisible on any machine
  where `Barrier` is `Mutex` + `Condvar`. Falsifying it needs Miri or a weakly
  ordered host, and the ticket forbids a new dependency. Both docstrings state
  this limit in-code instead of implying coverage that does not exist. What the
  tests *do* pin: termination within 5 s, and that a shutdown wakeup neither
  advances `completed` nor moves the output buffers.
- **`shaders/glsl/*.glsl` ↔ `shaders/generated/*.spv` is still unbound** — on
  the out-of-scope list, so deferred, but now stated *in the test itself* rather
  than only in `shaders/generated/README.md`. Smallest closing fix, recorded for
  whoever takes it: scan the `.spv` `OpConstant` words for the f32 bit patterns
  of `ISO_DEPTH_EPSILON` (`0x37800000`) and the alpha cutoff (`0x3F000000`) and
  assert both appear.
- **`T7_docs-adr-and-system-map.md:94` and
  `T9_isometric-projection-and-depth.md:352` still describe the √2 ring as
  shipped-as-specified and as an open follow-up.** Not fixed here: this ticket's
  worker was permitted to write to exactly one plan file (the plan index, for
  item 10), and those are sibling ticket files. The **normative** record is
  correct — ADR 012 carries the resolution and the plan index is reconciled — so
  what is stale is the per-ticket history only. Needs one line appended to each
  by an actor with write access; flagged rather than closed.

- The gate digest, unchanged and now **pinned by a test** rather than by prose:
  `the_gate_scene_walk_is_pinned_to_its_published_digest`.

## Validation

All boxes below were run on this tree after the final mutation (the review
fixes), not before it.

- [x] `cargo fmt --all -- --check` — clean, no output.
- [x] `cargo test --workspace --locked` — green. 35 `test result: ok` blocks,
      **0** `test result: FAILED`.
- [x] `MMD_REQUIRE_GPU=1 cargo test -p mmd-engine --test render_correctness` —
      green, `grep -c '^SKIP '` = 0.
      `29 passed; 0 failed; 0 ignored`, `grep -c '^SKIP '` → **0**, so
      `golden_frame_matches`, `rings_are_never_occluded` and
      `a_ring_and_a_sprite_share_a_pass` really ran on the Vulkan device rather
      than skipping.
- [x] `cargo clippy --workspace --all-targets --all-features -- -D warnings` —
      clean.
- [x] `nix flake check` — `all checks passed!`
- [x] `cargo run -p xtask -- bootstrap --check` / `shaders --check` /
      `atlases --check` —
      `bootstrap: ok (SDL 3.4.12 / sdl3 0.18.4 / sdl3-sys 0.6.7; ...)`,
      `shaders: ok (spirv+dxil+metallib; ...)`,
      `atlases: ok (4 png + manifest)` — still exactly 4 atlases.
- [x] `cargo run -- run --agents 5000 --frames 300` — `hash=864147ca3a0e09f7ebc5762b778fce193e705a2bc943ceaf67acf087581ee881`
      Full line: `run: clean exit mode=window backend=vulkan tick=300 frames=300
      hash=864147ca3a0e09f7ebc5762b778fce193e705a2bc943ceaf67acf087581ee881
      quit=false paused=false overlay=false hitboxes=true`. Reproduced **three
      times**: before any edit, immediately after the ring change, and after the
      review fixes.
- [x] `cargo test -p mmd-lab --test merge_gate` — green, `5 passed`.
- [x] `no_perf_claim_in_docs` green; `every_system_has_a_test` green
      (`validation_contract` 7/7).
- [x] the three-spelling population grep returns only allowlisted residue —
      6 lines, each named and justified in Impl step 2 above. The allowlist the
      ticket gave was insufficient; the widened one is written into step 2
      rather than left in a worker's report.
- [x] `graphify update .` run; `git status` shows no `graphify-out/` entry —
      `Rebuilt: 3908 nodes, 7537 edges, 250 communities`;
      `git status --short | grep -i graphify` → no output.
- [ ] manual: rings are tangent, not overlapping, for two agents at contact —
      recorded in `ai-artifacts/manual_test_checklist.md`
      **Left unchecked deliberately — this is a windowed/visual check and a
      headless worker must not claim it.** Recorded as the first item of
      `## T10 review-fixes` in `ai-artifacts/manual_test_checklist.md`, which is
      where a human closes it.
      The *automatable* half was done and is not a substitute for the box: the
      contact-pair scene was rendered offscreen to PNG under both the pre-T10
      and the shipped formula and the images were looked at — before, the two
      ellipses of a pair at exactly contact distance visibly **intersect**;
      after, they **touch without crossing**. The numeric guard is
      `the_rings_of_two_touching_bodies_are_tangent`, which fails at
      `1.4142 radii apart, not 2` under the old formula.
