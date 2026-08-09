# T7: ADRs, architecture doc, system map

**Plan:** `./ai-artifacts/PLAN_2026_08_09_horde-sim-headroom.md`
**Depends:** T6, T9
**Commit outcome:** Phase 0.5 is a written decision — three ADRs, an architecture
page, an enforced system→test map, and a glossary that names the new vocabulary.

## Context (self-contained)

- Goal: buy simulation headroom in the per-agent neighbour scan
  (`crates/mmd-engine/src/sim/collision.rs`) without spending a behavioural
  guarantee, on a game resized to StarCraft scale. T0 resized it, T1–T6 shipped
  the simulation work, T8–T9 shipped the render work; this ticket writes it all
  down.
- This slice: docs and the contract test that keeps them honest. The repo's own
  rule (`docs/ADR/README.md`) is *"New decision → new ADR. Changed decision →
  superseding ADR; do not rewrite history silently."* Three of this plan's five
  changes are new decisions and two of them **reject** a recommendation from the
  research dossier — those rejections are the most valuable thing to record,
  because the next reader will find the same recommendation and try it again.
- Out of scope here: any engine change. If a test fails in this ticket, fix the
  doc or the map, not the code.
- Assumptions in force:
  - `docs/technical-prototype-functional-close.md` is in `LIVE_DOCS` in
    `tests/validation_contract.rs`. **Every line added to it must avoid every
    perf token**, or `no_perf_claim_in_docs` turns red. Banned substrings
    include `p95`, `p99`, `16.67`, `frame time`, `frame-time`, `percentile`,
    `median`, `throughput`, `latency`, `fps`, `frames per second`, `per second`,
    `hertz`, ` hz`, `millisecond`, `microsecond`, `nanosecond`, `real-time`,
    `realtime`, **`faster`**, **`slower`**, `benchmark result`, and any
    `<digits> ms`. Write about *what the code does*, never about how quickly.
  - ADRs and the new architecture HTML are outside `LIVE_DOCS`, but this plan
    keeps them free of speed claims too. Nothing in this repo has been measured
    since phase-0 close.
  - T1–T6 add **no** new system: every test they write lives in a file
    `SCOPE_SYSTEMS` already maps. T8 and T9 **do** — `SCOPE_SYSTEM_COUNT` goes
    `12` → `14`, gaining "Debug hitbox overlay" and "Isometric projection and
    depth order".
  - `graphify` **is** installed. Orient with `graphify query "<question>"`;
    run `graphify update .` as the last validation step. `graphify-out/` is
    gitignored, so the refresh never appears in the diff.

## Requirements

- ADR 010 records amortisation, bin stamping and push priority — including the
  two rejections (Ericson's min-corner 2×2; BioDynaMo's O(#agents) rebuild).
- ADR 011 records the worker pool and what it cost the allocation invariant.
- **ADR 012** records the StarCraft-scale decision and the render work it
  forced: the `MAX_LIVE_AGENTS = 5_000` ceiling, the 48 px sprite with a
  half-sprite body, retuning `technical_prototype_v1` in place rather than
  minting a v2, the hitbox ring as a shader branch rather than a new shader
  family or a fifth atlas, and isometric depth via alpha-tested cutout rather
  than a per-frame sort. Each rejected alternative is named with its reason —
  those are the entries the next reader will otherwise retry.
- `docs/ADR/README.md` lists all three and states how they relate to ADR 009
  (separation) and ADR 004 (the sprite renderer, whose "no per-frame depth sort"
  line ADR 012 supersedes in part).
- `docs/horde-sim-headroom-architecture.html` exists, dark-mode by default,
  self-contained (no external asset), and shows the tick's pass order, the three
  knobs, and the render path from cell space through the isometric projection to
  the depth-ordered draw.
- `docs/technical-prototype-functional-close.md` names every new test in the
  right system row.
- `tests/validation_contract.rs` maps the same names, and `MIN_SCANNED_TESTS`
  matches the real total.
- `docs/GLOSSARY.md` defines the new vocabulary.

## Inputs

- **From Depends (T9, `51c73dc`) — what landed and what this ticket must write
  down:**
  - The world→screen projection is 2:1 isometric at the **render layer only**.
    `git diff --stat crates/mmd-engine/src/sim/` is empty across T9 — that is
    exactly what keeps every pinned state hash alive, and is the decision ADR 012
    should record as the reason the resize could ship without a sim rewrite.
  - **The depth test is `GREATER` with a clear of `0`, NOT `LESS`/clear-1.** The
    T9 ticket's three depth statements were mutually inconsistent; under `LESS`
    the horde renders back-to-front (observed: 4482 wrong pixels). Any doc that
    repeats the `LESS` framing is wrong — write the shipped convention.
  - Depth correctness with alpha comes from an alpha-test `clip()`, not a sort,
    so the 4-atlas batching survives and `SpriteInstance` stays 48 bytes
    (`instance_layout_is_stable` green and byte-unmodified). The two
    normalisation scalars live in `FrameUniforms::_pad`.
  - A depth key of exactly `0.0` was being discarded rather than sorted last,
    which would have blanked an entire frame on a degenerate map. Fixed by
    flooring sprite depth at one D16 quantum. The `GreaterOrEqual` alternative
    was deliberately rejected: it admits the tie and re-blinds the occlusion
    test.
  - The camera is a **fixed** offset centring the destination cell, plus an AABB
    reject for quads fully outside the view. Scrolling, edge-pan, zoom and
    selection are Phase 1 — say so, so a reader does not mistake the fixed view
    for a limitation nobody noticed.
  - **Known follow-up, record it:** the ring ellipse ships at
    `[2r·tile_w, 2r·tile_h]` as the ticket specified, which is √2 larger than the
    exact projection of a circular body onto the isometric floor.
    ***Superseded by T10 (`736c803`)*** — the review ruled it a defect, not an
    accepted approximation, and T10 divided both axes by `√2`. ADR 012 carries
    the resolution; this line is kept only as per-ticket history.
  - **Two residual risks to state plainly:** (a) the frozen phase-0 bench ladder
    is **no longer comparable at any rung** — the measured frame changed four
    ways at once (population, body size, projection, an extra pipeline); (b) the
    merge gate's *pixel* limb is tautological, because the Ubuntu fixture is a
    byte-copy of the golden it is compared against.

- **From Depends (T8, `d5c0ba9`) — more to document:**
  - The shader canonical hash is pinned in **five** manifests (three obvious ones
    plus `lab/fixtures/{windows,macos}-candidate/golden/manifest.json`, read live
    by the mmd-lab merge gate). T8 added
    `every_tracked_manifest_pins_the_live_shader_and_atlas` to enforce all of
    them on every host.
  - **Open gap worth recording as a known risk:** nothing binds the `.spv` blobs
    to `sprite.hlsl`. `xtask` checks recorded hashes only, so a future edit could
    re-pin `canonical_sha256` against stale blobs and pass every offline gate.
    The repo also tracks no GLSL mirror despite building the Linux SPIR-V from
    one. Not fixable inside this plan — write it down.
  - **Native blob debt:** DXIL and metallib slots are deferred placeholders on
    this host; the Windows and macOS reference hosts owe a native rebuild.
  - The ring reuses `SpriteInstance` verbatim via a sentinel in `uv_rect.x`, so
    the pinned 48-byte layout and `atlas_count: 4` both survive — that is the
    decision ADR 012 should record.

- **From Depends (T6) — corrections this ticket MUST make, discovered during
  implementation:**
  - **ADR 011 is now factually wrong in one consequence bullet.** It says the
    pool's re-entrancy check is "a debug assertion". It is not: `debug_assert!`
    compiled the guard swap out in release, leaving a data race reachable from
    100% safe code (`Simulation` is `pub`, `Clone`, `Send`), so T6 promoted it to
    a real `assert!`. Correct that bullet to say a real assertion, and keep ADR
    011's existing "clones share one pool" decision — the live assertion is the
    mechanism that enforces it.
  - **Record the panic protocol.** A panic in any separation chunk used to
    abandon the rendezvous: the other participants blocked on `done` forever, and
    a panicking ticker then blocked in `Drop` behind them, hanging the process
    mid-unwind — a failed assertion would have surfaced as a stalled merge gate,
    not a red test. T6 fixed it with catch-flag-rendezvous-rethrow and pinned it
    with a test that injects `phases = 0`. ADR 011 should state that the pool
    rendezvous is panic-safe and why.
  - **Record the ordering.** Job publication no longer rests on `Barrier`'s
    undocumented ordering; T6 added explicit release-acquire `publish` /
    `completed` pairs in both directions.
  - **Two residual risks to write down, not to fix here:** (a) no `cargo miri`
    and no `loom` on the pinned 1.95.0 stable toolchain, so every soundness claim
    in `crates/mmd-engine/src/sim/pool.rs` is prose plus tests, not
    machine-checked; (b) if `thread::spawn` fails partway through
    `SeparationPool::new`, already-spawned workers park forever — the fix is an
    `Option<Self>` + condvar redesign, out of this plan's scope.
- **From Depends (T0) — one more residual to document:** `cargo run -- bench`
  **without** `--test-policy` now hits the live ceiling at its second tier,
  because `BenchPolicy::production()` keeps the frozen phase-0 ladder while
  `test_short()` was split onto its own `test-short-v1` ladder. The perf tool is
  retired and non-gating, so this was left rather than redesigned. Say so
  somewhere a reader will find it.

- **Inherited from T0 — the pinned gate digest (do not recompute, do not
  re-derive):**
  `hash=864147ca3a0e09f7ebc5762b778fce193e705a2bc943ceaf67acf087581ee881`
  produced by `cargo run -- run --agents 5000 --frames 300` on commit `df309d3`.
  Every ticket after T0 must reproduce this value byte for byte. A different
  `hash=` means the plan's core invariant is broken — stop and report `failed`
  rather than re-pinning it. The pre-T0 value
  `f647e7f590ed5814e4e61388e23836dfacb980217fb1762542ec3abfe85549b3` is
  superseded and must never reappear.
  T0 also delivered: `MAX_LIVE_AGENTS = 5_000` enforced by a single
  `check_population` at the validator dispatcher (not per family),
  `COLLISION_SCENE_MAX_AGENTS` removed, the three tracked scenes retuned to
  48 px sprites / `collision_radius_q8: 1_536` with fresh `.sha256` sidecars,
  the four `fixture_*` scenes byte-identical, `MIN_SCANNED_TESTS` at `174`, and
  `BenchPolicy::test_short()` split onto its own `test-short-v1` ladder while
  `production()` keeps the frozen phase-0 ladder verbatim.

- `docs/ADR/README.md` — the index, and the two "superseded in part" notes at
  the top.
- `docs/ADR/009_ADR_agent_separation_and_collision.md` — the format to follow:
  `# ADR NNN: Title`, then `- Status:`, `- Date:`, `- Supersedes in part:`, then
  `## Context`, `## Decision`, `## Consequences`.
- `docs/technical-prototype-functional-close.md` — the `## System → test map`
  table at line ~38. Rows to edit: **Collision — agent separation and neighbour
  bins** (line ~41), **Scenario loading and hash contract** (line ~43),
  **Allocation invariant** (line ~49).
- `tests/validation_contract.rs` — `SCOPE_SYSTEMS` (line ~350),
  `MIN_SCANNED_TESTS` (line ~516, currently `168`), `SCOPE_SYSTEM_COUNT`
  (line ~520, `12`), `LIVE_DOCS` (line ~770).
- `docs/GLOSSARY.md`, `docs/DESIGN.md`, `AGENT.md`.
- `docs/agent-collision-architecture.html` — the existing architecture page,
  for visual house style.
- **From Depends (T1–T6, all landed).** The exact test names to map:

  `crates/mmd-engine/tests/separation.rs` (system *Collision — agent separation
  and neighbour bins*), twenty new names:
  `spatial_bin_counts_match_the_bucket_lengths`,
  `spatial_reuses_bins_without_clearing_them`,
  `spatial_survives_a_stamp_wrap`,
  `separation_phases_of_one_is_the_identity`,
  `an_amortised_agent_keeps_its_repulsion_between_phases`,
  `the_grid_rebuilds_once_per_phase_cycle`,
  `an_amortised_stack_still_spreads`,
  `bin_row_matches_bin_by_bin_order`,
  `bin_row_clamps_to_the_last_column`,
  `bin_row_is_empty_off_the_grid`,
  `a_lone_agent_accumulates_no_repulsion`,
  `one_mass_class_leaves_every_agent_equal`,
  `mass_is_assigned_round_robin_by_index`,
  `a_heavier_neighbour_pushes_a_lighter_one_harder`,
  `mass_classes_change_the_bodied_digest`,
  `threads_do_not_change_the_walk`,
  `threads_do_not_change_an_amortised_walk`,
  `a_single_thread_spawns_no_workers`,
  `a_pool_spawns_one_fewer_worker_than_participants`,
  `the_pool_shuts_down_cleanly`.

  `crates/mmd-engine/tests/frame_allocations.rs` (system *Allocation
  invariant*), one new name:
  `a_threaded_collision_tick_allocates_nothing`.

  `crates/mmd-engine/tests/scenario_contract.rs` (system *Scenario loading and
  hash contract*), seven new names:
  `tracked_scenes_declare_the_identity_tuning`,
  `zero_separation_phases_is_rejected`,
  `separation_phases_above_the_cap_is_rejected`,
  `mass_classes_above_the_cap_is_rejected`,
  `separation_threads_above_the_cap_is_rejected`,
  `a_bodyless_scenario_may_not_tune_separation`,
  `the_gate_scene_pins_the_identity_tuning`.

  Engine facts to record: three scenario fields
  (`separation_phases` ≤ 16, `mass_class_count` ≤ 8, `separation_threads` ≤ 16),
  all identity `1`; `collision_mid_v1` runs 4 phases; `collision_sprite_v1` runs
  2 mass classes; every tracked scene runs 1 thread; `SpatialGrid` carries
  `counts` / `count_stamp` / `stamp` and exposes `bin_count` and
  `agents_in_bin_row`; `Simulation` carries `mass`, `inv_mass`, `grid_rebuilds`
  and an optional `Arc<SeparationPool>`; `alloc_guard` gained
  `arm_worker() -> WorkerArm`.

## TDD

Docs have no unit test, so the contract test *is* the test.

1. **Red** — add the 28 names to `SCOPE_SYSTEMS` first, without touching the
   close doc. `every_system_has_a_test` fails with *"CLOSE_DOC does not name …"*.
2. **Green** — add the same names to the close doc's three rows; fix
   `MIN_SCANNED_TESTS`; write the ADRs, the architecture page and the glossary
   entries.
3. **Refactor** — none.

## Check plan

| Check | Input | Expect |
| ---- | ---- | ---- |
| `every_system_has_a_test` | `cargo test -p millions_must_die --test validation_contract` | green — const list and close doc agree in both directions |
| `no_perf_claim_in_docs` | same | green — the close-doc edit carries no perf token |
| `gate_docs_state_perf_gating_is_retired` | same | green — unchanged |
| ADR index resolves | `docs/ADR/README.md` | links to `010_…md` and `011_…md` open |
| Architecture page | open `docs/horde-sim-headroom-architecture.html` | renders dark by default, no network request, no broken layout |

## Impl steps

- [x] 1. `docs/ADR/010_ADR_separation_amortisation_and_push_priority.md`
      **already exists**, authored alongside the plan with
      `Status: Proposed — accepted when plan/horde-sim-headroom T7 lands`.
      Read it end to end against the code T1–T6 actually shipped, correct any
      drift (field names, caps, which scene carries which knob, rejected
      alternatives), then change the status line to `- Status: Accepted`.
      Its content must still cover, and these are the parts most likely to have
      drifted:
      `## Context` states that navigation is not the cost — the field is
      sublinear in agent count and steering is near-linear — so the whole plan
      targets one loop. `## Decision` covers three parts:
      **(a) amortisation** — `separation_phases`, one cadence for the scan and
      the grid rebuild, strided bucketing, `1` is the identity, bounded
      staleness, Reynolds' `skipThink` of 8–10 as the precedent, and Graham's
      distinction: `skipThink` does **less** work, it does not spread the same
      work thinner;
      **(b) bin stamping** — what `counts` / `count_stamp` / `stamp` buy, and the
      **rejection**: BioDynaMo's O(#agents) rebuild needs either a per-bin linked
      list (which destroys the bucket contiguity `agents_in_bin_row` depends on)
      or a per-tick sort of touched bins, and with the gate scene's bin-to-agent
      ratio there is no reason to expect that trade to pay — and perf is retired,
      so it cannot be settled by measurement;
      **(c) push priority** — the mass byte, `mass[j] / mass[i]`, one class is
      exactly `1.0` and therefore bit-exact, Froblins' goal-sink deadlock as the
      motivation, and SC2 5.0.15's allied push priority as the shipped
      precedent.
      `## Consequences` must include: **the min-corner rejection.** Ericson's
      2×2 window needs each pair enumerated once; `accumulate_separation` is a
      gather, so a 2×2 window misses every neighbour one bin lower on either
      axis, and converting to a symmetric scatter would destroy the
      index-disjointness the worker pool depends on. The transferable half —
      one contiguous run per row — shipped instead and is bit-exact.
      State plainly that no number in this ADR is a measurement of this engine.
- [x] 2. `docs/ADR/011_ADR_parallel_separation_and_the_allocation_invariant.md`
      **already exists**, also `Status: Proposed`. Same treatment: verify
      against the shipped pool, correct drift, flip to `- Status: Accepted`.
      Its content must still cover:
      `## Context` — the pass is already pure, index-disjoint and read-only in
      its inputs; the movement loop is not, because `recycle_one` advances a
      shared cursor. `## Decision` —
      persistent pool sized by `separation_threads`, spawned at construction
      because thread creation allocates; two `Barrier`s, which do not; the
      ticking thread is participant 0; contiguous index ranges, so every output
      element has exactly one writer and no partial sum crosses a boundary,
      which is why the result does not depend on thread count; **no new
      dependency** — `rayon`'s bridge carries no allocation-free guarantee and
      `alloc_guard` is a merge gate; one `unsafe impl Send for Job` with its
      safety argument quoted. `## Consequences` — the allocation invariant is
      **stronger**, not weaker: workers arm themselves via
      `alloc_guard::arm_worker`, so their allocations are counted rather than
      invisible, and the module doc's "must be revisited" paragraph is now paid
      off. Record the two live limitations: cloned `Simulation`s share one pool
      and must not tick concurrently (debug-asserted), and every tracked scene
      stays at one thread so the merge gate reproduces on any host.
- [x] 3. Edit `docs/ADR/README.md`: append
      `10. [Separation amortisation, bin stamping + push priority](010_ADR_separation_amortisation_and_push_priority.md)`
      and
      `11. [Parallel separation + the allocation invariant](011_ADR_parallel_separation_and_the_allocation_invariant.md)`
      to the numbered list, and add above it:
      *"ADR 009 is **supplemented** by ADR 010 and ADR 011 (2026-08-09): the
      separation model is unchanged; how often it runs, how it is indexed, how
      it is weighted and which threads run it are recorded there."*
- [x] 4. `docs/horde-sim-headroom-architecture.html` **already exists**, authored
      alongside the plan. Read it against the shipped code and correct any
      drift — field names, caps, which tracked scene carries which knob, the
      pass order in the first SVG, the barrier handshake in the second. It is
      already self-contained (one inline `<style>`, no external stylesheet,
      script, font or image), dark by default with a
      `@media (prefers-color-scheme: light)` override, and carries a *What this
      page does not claim* panel. Keep all four properties.
- [x] 5. Edit `docs/technical-prototype-functional-close.md`, row **Collision —
      agent separation and neighbour bins**: append the twenty
      `separation.rs` names from Inputs, comma-separated, each in backticks,
      keeping the existing names and the trailing
      `` | `crates/mmd-engine/tests/separation.rs` | `` cell.
- [x] 6. Same file, row **Scenario loading and hash contract**: append the seven
      `scenario_contract.rs` names. Row **Allocation invariant**: append
      `a_threaded_collision_tick_allocates_nothing`.
- [x] 7. Same file, in `## What "closed" means here`, add one row:
      `| Simulation headroom knobs | phase 0.5 — separation cadence, push priority and worker count are scenario data; every scene at the identity tuning walks a bit-identical path, proven by pinned digests |`.
      Re-read the banned-token list in Context before saving this line.
- [x] 8. Edit `tests/validation_contract.rs`: add the 28 T1–T6 names to the
      three matching `SystemCoverage` entries in `SCOPE_SYSTEMS`, plus the 17
      T8/T9 names — 13 in two **new** entries ("Debug hitbox overlay",
      "Isometric projection and depth order", both filed under
      `render_correctness.rs`) and 4 more appended to the existing Runtime
      frame loop / Allocation invariant / App and CLI lifecycle entries whose
      files they land in. Left `gpu_only` empty for all of them. **Deviation
      from this step's literal "stays 12" text, logged in the worker's final
      report:** `SCOPE_SYSTEM_COUNT` changed `12` → `14`, per this ticket's own
      Assumptions-in-force bullet (line ~36) and the parent's hard constraint
      3 — both say T8 and T9 add real systems and the count must move; this
      step's instruction not to move it predates T8/T9 landing.
- [x] 9. Fix `MIN_SCANNED_TESTS`. Temporarily set it to `usize::MAX`, ran
      `cargo test -p millions_must_die --test validation_contract every_system_has_a_test`,
      read the real total (277) out of the panic message, then set the
      constant to that number.
- [x] 10. Edit `docs/GLOSSARY.md`, adding one entry each for: **separation
      phase**, **push priority / mass class**, **bin stamp**, **row window**,
      **separation pool**, **identity tuning**. Keep each to the file's existing
      one-or-two-sentence house style.
- [x] 11. Edit `AGENT.md` under `## Status`: add one sentence — *"Phase 0.5
      (`plan/horde-sim-headroom`) adds three scenario-gated simulation knobs —
      `separation_phases`, `mass_class_count`, `separation_threads` — each
      defaulting to the identity value 1; see ADR 010 and ADR 011."* Do not
      touch the performance paragraph.
- [x] 12. Add the new architecture page to whichever list in `docs/README.md`
      enumerates the architecture HTML pages, matching the existing entries.
- [x] 13. Run validation.

## Outputs

- Verified against the shipped code, corrected for drift, and flipped from
  `Proposed` to `Accepted`:
  `docs/ADR/010_ADR_separation_amortisation_and_push_priority.md`,
  `docs/ADR/011_ADR_parallel_separation_and_the_allocation_invariant.md`.
  Verified and corrected: `docs/horde-sim-headroom-architecture.html`. All three
  were authored alongside the plan and are already in the tree.
- **New:** `docs/ADR/012_ADR_starcraft_scale_and_isometric_render.md`, authored
  in this ticket against the shipped T0/T8/T9 code.
- Edited: `docs/ADR/README.md`, `docs/README.md`, `docs/DESIGN.md`,
  `docs/technical-prototype-functional-close.md`, `docs/GLOSSARY.md`,
  `AGENT.md`, `tests/validation_contract.rs`.
- **Regenerated:** `ai-artifacts/PLAN_2026_08_09_horde-sim-headroom.html` from
  the updated plan markdown — it is stale from the moment T0 lands until this
  ticket refreshes it.
- Public API / behaviour change: none.

## Validation

- [x] `cargo fmt --all -- --check` — clean, no output
- [x] `cargo test -p millions_must_die --test validation_contract` — green (7
      passed), including `every_system_has_a_test` and `no_perf_claim_in_docs`
- [x] `cargo test --workspace --locked` — green (35 `test result: ok` blocks,
      0 failed, exit 0)
- [x] `cargo clippy --workspace --all-targets --all-features -- -D warnings` —
      clean
- [ ] manual check — `xdg-open docs/horde-sim-headroom-architecture.html`:
      renders dark by default, no layout overflow, no network request — this
      needs a real browser window; left unchecked per the manual-box rule.
      Static evidence (no `http(s)://`, no external `<script src>`/`<link
      rel="stylesheet">`, `color-scheme: dark` as the default `:root`) is
      recorded in `ai-artifacts/manual_test_checklist.md` § T7.
- [x] manual check — every link in `docs/ADR/README.md` resolves — verified by
      resolving all twelve `NNN_ADR_*.md` targets (001–012) against the
      filesystem; all present
- [x] `nix flake check` — "all checks passed!"
- [x] app functional — no code touched; `cargo run -- run --agents 5000 --frames 300` exits 0
- [x] the gate smoke `hash=` equals the T0 pinned digest, byte for byte —
      `hash=864147ca3a0e09f7ebc5762b778fce193e705a2bc943ceaf67acf087581ee881`
- [x] `graphify update .` run (graph refresh; `graphify-out/` is gitignored) —
      "Rebuilt: 3879 nodes, 7496 edges, 254 communities"
- [x] commit msg draft: `docs(sim): record the phase-0.5 scale, headroom and isometric decisions`
