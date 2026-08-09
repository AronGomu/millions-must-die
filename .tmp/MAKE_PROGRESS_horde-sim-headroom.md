# Progress: Horde Sim Headroom (phase 0.5)

- Goal: Resize the game to a StarCraft read (5 000-entity ceiling, 48 px sprites,
  half-sprite bodies, hitbox rings, 2:1 isometric render) and land five
  neighbour-scan headroom changes on top, with T0 as the only hash-moving ticket.
- Plan index: `ai-artifacts/PLAN_2026_08_09_horde-sim-headroom.md`
- Tickets dir: `ai-artifacts/PLAN_2026_08_09_horde-sim-headroom/`
- Workspace: branch `plan/horde-sim-headroom` (cut from `main` @ `5ec9df6`)
- Started: 2026-08-09
- Updated: 2026-08-10 — **run complete**, all 11 tickets terminal

## Success

- [ ] `MAX_LIVE_AGENTS = 5_000` is enforced by every scenario family and nothing
      in the repo names a larger population — validate:
      `cargo test --workspace --locked` green + grep sweep from T0 step 11
      — **partially met.** The enforcement clause holds: every family validator
      routes through `check_population` and the sweep at all four spellings
      (`50000`, `50_000`, `50 000`, `50k`) is clean across live surfaces. The
      "nothing names a larger population" clause is knowingly unmet on one frozen
      surface — the historical `production-v1` bench ladder. See **D1**.
- [x] T0 re-pins the gate digest exactly once; T1–T9 reproduce it bit-for-bit —
      validate: `cargo run -- run --agents 5000 --frames 300` `hash=` equal to
      T0's Outputs value at the end of every ticket
      — held on all nine, plus T10 and the parent's own final run
- [x] `BODIED_STACK_HASH` and `BODYLESS_GRID_PRE_SEPARATION_HASH` never move
      after T0 — validate: `cargo test -p mmd-engine --test separation`
- [x] Every new sim capability is scenario data with identity default `1` —
      validate: `cargo test -p mmd-engine --test scenario_contract`
- [x] Zero per-frame allocation still holds, on every thread — validate:
      `cargo test -p mmd-engine --test alloc_guard` (and T6's threaded assertions)
- [x] Merge gate green after every ticket — validate: `cargo fmt --all -- --check`,
      `cargo clippy --workspace --all-targets --all-features -- -D warnings`,
      `cargo test --workspace --locked`, `nix flake check`,
      `cargo run -p xtask -- bootstrap|shaders|atlases --check`
- [x] No performance number enters any doc — validate: `no_perf_claim_in_docs`
- [x] `ai-artifacts/manual_test_checklist.md` covers every shipped ticket
      — 11 sections, T0–T10, one per shipped ticket

## Out of scope

Camera control (scroll/zoom/selection), flow-field work (backlog 6/7), flocking
goal propagation (backlog 10), retuning the `fixture_*` scenes, any new
third-party dependency (`rayon` explicitly excluded), hard non-overlap, and any
performance number or perf-gated pass/fail anywhere.

## Status

| ID | Title | File | State | Evidence | Note |
| --- | --- | --- | --- | --- | --- |
| T0 | StarCraft-scale cap and body | `T0_starcraft-scale-cap-and-body.md` | **done** | `df309d3` — 460 pass / 0 fail, fmt + clippy + `nix flake check` + 3 xtask checks green, gate `hash=864147ca…1ee881` reproduced 3× | tier deep, ship production — only hash-moving ticket. Failed once on a real plan defect (D1), repaired 1/1 |
| T1 | Scenario headroom knobs | `T1_scenario-headroom-knobs.md` | **done** | `de49405` — workspace green, clippy + `nix flake check` clean, gate `hash=864147ca…1ee881` **reproduced byte-for-byte**, 7 new contract tests | tier standard, ship balanced. All 7 scenes declare the three knobs at identity `1` |
| T2 | Stamped bin counts | `T2_stamped-bin-counts.md` | **done** | `dda283b` — workspace green, clippy + `nix flake check` clean, `spatial_rebuild_allocates_nothing` green, gate `hash=864147ca…1ee881` reproduced | tier standard, ship balanced. Bit-exact |
| T3 | Amortised separation | `T3_amortised-separation.md` | **done** | `9b195d7` — workspace green, clippy + `nix flake check` clean, gate `hash=864147ca…1ee881` reproduced, `collision_mid_v1` now runs `separation_phases: 4` | tier standard, ship balanced. Ship reviewer found 4 items; 3 fixed, 1 logged |
| T4 | Row-contiguous neighbour scan | `T4_row-contiguous-scan.md` | **done** | `2d526db` — workspace green, clippy + `nix flake check` clean, gate `hash=864147ca…1ee881` reproduced, 32 separation tests pass | tier standard, ship balanced. Bit-exact; one weak test logged as residual |
| T5 | Per-agent push priority | `T5_push-priority-mass.md` | **done** | `ac39344` — workspace green, clippy + `nix flake check` + 3 xtask checks clean, gate `hash=864147ca…1ee881` reproduced, `a_collision_tick_allocates_nothing` green, `collision_sprite_v1` runs 2 mass classes | tier standard, ship balanced |
| T6 | Parallel separation pass | `T6_parallel-separation.md` | **done** | `50b43aa` + `862bf1e` — 493 pass / 0 fail, release 207 pass, clippy + `nix flake check` + xtask clean, gate `hash=864147ca…1ee881` reproduced 3×; bit-identical at threads 1/2/4/8 (phases 1) and 1/3/4/7 (phases 4) | tier deep, ship production. Red-team caught a real panic-deadlock; fixed and proven by test |
| T8 | Hitbox ring overlay | `T8_hitbox-ring-overlay.md` | **done** | `d5c0ba9` — `MMD_REQUIRE_GPU=1` 35 suites / 0 fail / 0 ignored, clippy + `nix flake check` + 3 xtask gates clean, gate `hash=864147ca…1ee881` reproduced, `instance_layout_is_stable` green unmodified, `atlases: ok (4 png + manifest)`; rings eyeballed in an offscreen PNG | tier deep, ship production. Found the shader hash is pinned in 5 manifests, not 3 |
| T9 | Isometric projection and depth | `T9_isometric-projection-and-depth.md` | **done** | `51c73dc` — 519 pass / 0 fail (0 SKIP under `MMD_REQUIRE_GPU=1`), clippy + `nix flake check` + 3 xtask gates clean, gate `hash=864147ca…1ee881` reproduced, `git diff crates/mmd-engine/src/sim/` **empty**, `instance_layout_is_stable` green byte-unmodified | tier deep, ship production. Review caught the ticket's core invariant unasserted + a whole-frame blanking bug |
| T7 | ADRs, architecture doc, system map | `T7_docs-adr-and-system-map.md` | **done** | `64f17f4` — workspace green, clippy + `nix flake check` + 3 xtask gates clean, `no_perf_claim_in_docs` green, `every_system_has_a_test` green at `SCOPE_SYSTEM_COUNT = 14`, gate `hash=864147ca…1ee881` reproduced, all 12 ADR links resolve | tier standard. **Deviation: worker did not invoke `ship` despite the prompt naming it** — docs slice, covered by the reviewer fanout instead |
| T10 | Review fixes | `T10_review-fixes.md` | **done** | `736c803` — 523 pass / 0 fail, `MMD_REQUIRE_GPU=1 render_correctness` 29 pass / 0 skip, clippy + `nix flake check` + 3 xtask gates clean, gate `hash=864147ca…1ee881` reproduced 3× | tier deep, ship production. Added post-review; closes 2 blockers + 8 should-fixes |

States: pending|running|done|failed|blocked_user|blocked_dep|skipped

## Review fanout (loop step 6) — 4 dimensions, fresh context, tier deep

| Dimension | Verdict | Headline |
| --- | --- | --- |
| Correctness | findings, no blockers | Ring drawn √2 too large while docs claimed exactness; pool partition, phase seed, T2 stamps, T4 row order, T9 depth all verified sound (GPU-checked) |
| Tests | findings, 1 blocker | Mass scale factor unpinned — mutation `scale = mass[j]` left the whole suite green; two more vacuous tests proven by mutation |
| Scope drift | findings, 1 blocker | T0's sweep missed the `50k` contraction — 14 live sites, incl. the window title and a generator emitting an unloadable scene |
| Security / soundness | findings, none blocking | `unsafe` judged sound; the *argument* for `shutdown`'s ordering was circular |

All out-of-scope items verified clean: no new dependency (`Cargo.toml`/`Cargo.lock`
byte-identical to `main`, no `rayon`), fixtures gained only the identity knobs, no
camera control, no hard non-overlap claim, designated history untouched.

Execution order (topo-serial): T0 → T1 → T2 → T3 → T4 → T5 → T6 → T8 → T9 → T7,
then the review fanout, then T10.

## Pinned digests

- Gate scene (`technical_prototype_v1`, 300 frames), pre-T0 (superseded):
  `f647e7f590ed5814e4e61388e23836dfacb980217fb1762542ec3abfe85549b3`
- Gate scene, re-pinned by T0 (`df309d3`), **frozen for T1–T9**:
  `864147ca3a0e09f7ebc5762b778fce193e705a2bc943ceaf67acf087581ee881`
  — reproduced on three consecutive runs, identical via both `--agents 5000` and
  the explicit `--scenario`. Inlined into every downstream ticket file's
  `## Inputs`.

## Assumptions

- Branch is `plan/horde-sim-headroom` per plan **A1**, not the skill's default
  `feat/{slug}` — the plan names the branch and the skill's own git rules say
  `plan/{slug}`.
- **`TODO(user)` in T0/T1 resolved by the orchestrator, not the user.**
  `plan/zombie-collision` (`108230a`) is already an ancestor of `main`
  and `origin/main` == local `main` (`5ec9df6`), so the precondition is met.
  Both ticket files were edited to record this and to forbid branch switching.
- Artifacts live in `ai-artifacts/` (plan **A11**), so the manual test checklist
  is `ai-artifacts/manual_test_checklist.md`, not the skill default
  `ai_artefacts/`.
- Checkboxes were added to T0/T8/T9 Impl steps by the parent before the run;
  T1–T7 already carried them.
- Production-depth ship auto-approved inside this run (the user invoked `make`).
- Every worker prompt carries the repo's graphify rule: orient with
  `graphify query` before reading source, `graphify update .` as the last step.
- The GPU/window manual checks in T0/T8/T9 Validation are recorded in
  `ai-artifacts/manual_test_checklist.md` for a human, not run headless by a
  worker; they never gate a ticket.

## Decisions

**D1 — the bench ladder is frozen phase-0 history and does not move.**
T0's Requirements asked for `SCALE_COUNTS` → `[500, 1_000, 2_500, 5_000]` on the
premise that the ladder "stays frozen and non-gating". That premise was false.
Only two paths reach green: rewrite the committed evidence JSON so recorded runs
claim populations they were never measured at — which falsifies frozen phase-0
measurement history (`d37bfe6` "close phase 0 on honest inconclusive 50k
evidence") and is forbidden by this plan's own no-perf-claim rule — or redesign
the lab to validate evidence against the policy it was recorded under, a
separate slice outside Scope In.
Chosen: `bench/policy.rs`, `benchmark_policy.rs`, `tools/mmd-lab/**` and `lab/**`
join the plan's existing **designated history** category alongside
`docs/technical-prototype-results.md` and the ADRs. The ladder keeps its values;
`policy.rs` gains a comment-only header recording that these are historical
measurement tiers, that the live ceiling is `MAX_LIVE_AGENTS = 5_000`, and that
nothing above it is run from phase 0.5 onward.
**Residual risk, accepted:** the frozen policy still *names* 50 000 / 100 000, so
the plan's Scope-In line "no policy constant names a larger population" is
knowingly unmet for that one frozen surface. The live scenario contract — which
governs what actually runs — does enforce the ceiling.

**D2 — `BenchPolicy::test_short()` split off the frozen ladder.** Discovered
during T0's repair: D1's "revert everything" is not sufficient, because
`test_short()` is the one policy that actually *executes* the simulation, and the
frozen ladder's second tier (10 000) now exceeds the live ceiling —
`dry_bench_pins_exact_atlas_manifest_bytes` died on
`Runtime(AgentCount { got: 10000, cap: 5000 })`. The frozen ladder cannot be run
under the live ceiling; neither retuning nor reverting alone resolves it.
Resolution: `production()` keeps its historical ladder verbatim (all committed
evidence still validates — 231 `mmd-lab` tests green), `test_short()` gets
`TEST_SHORT_SCALE_COUNTS = [500, 1_000, 2_500, 5_000]` under its own
`policy_id: "test-short-v1"` and writes no committed evidence. No threshold value
moved. Accepted.

## Parent final validation (2026-08-09, HEAD `1cf57d4`)

- `cargo fmt --all -- --check` — clean
- `cargo test --workspace --locked` — **523 passed, 0 failed, 8 ignored**
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` — clean
- `nix flake check` — `all checks passed!`
- `cargo run -p xtask -- bootstrap|shaders|atlases --check` — all ok
- `cargo run -- run --agents 5000 --frames 300` — `clean exit … hash=864147ca3a0e09f7ebc5762b778fce193e705a2bc943ceaf67acf087581ee881`
- Population sweep at all three spellings (`50000`, `50_000`, `50 000`, `50k`):
  residue is obstacle **cell indices** in `technical_prototype_v1.ron`,
  `MAX_INSTANCES` (GPU buffer capacity, comment corrected), two
  `docs/05-testing.md` rows explicitly marked "frozen, **not a gate**", the lab
  architecture page, and `.tmp/` phase-0 notes — all designated history.
  **Correction to T0's premise:** `.tmp/` is *tracked*, not untracked as T0
  stated; leaving it is still right (phase-0 history), the stated reason was not.

## Residual risks

- The frozen `production-v1` policy still names 50 000 / 100 000 (D1).
- `cargo run -- bench` **without** `--test-policy` now hits the ceiling at its
  second tier. The perf tool is retired and non-gating and no Validation box
  covers it, so T0 left it rather than redesigning. Worth a line in T7's docs.
- `atlas_dir_frame_evenly_distributed` weakened for the `frame` field only:
  asserted even-within-one-atlas-stride instead of a perfect split, because
  `frame = (i / atlas_count) % frame_count` only splits perfectly when the
  population divides 16 — 50 000 did, 5 000 does not. `atlas` and `dir` keep the
  strict check. Honest restatement of the real invariant, but a weakening.
- **T4** — `bin_row_clamps_to_the_last_column` is weak: it queries row 0, which
  holds no agents in the test's own layout, so the clamp assertion compares two
  empty slices and only guards against a panic, not a wrong clamp value. Comes
  straight from the ticket's Test plan table. Low risk in practice — the real
  guard on that walk is the pinned gate digest plus `BODIED_STACK_HASH`, both of
  which reproduce. Flagged to the final reviewer fanout.
- **T3** — `mid_scene_reports_its_tuning` duplicates its scenario-vs-sim
  assertion; noted by the ship reviewer, left as-is.
- `crates/mmd-engine/src/render/renderer.rs` `MAX_INSTANCES` still reads
  `100_000` — a GPU instance-buffer capacity, not a declared population.
  Deliberately untouched (out of T0's Inputs); T10 corrected its comment.
- **T6 pool soundness is prose plus tests, not machine-checked.** `cargo miri` is
  unavailable on the pinned 1.95.0 toolchain and the no-new-dependency rule
  forbids `loom`, so the index-disjoint-write argument rests on review and the
  bit-identical threads 1/2/4/8 and 1/3/4/7 runs. Highest-value follow-up.
- **No shipped scenario sets `separation_threads > 1`**, so the pool — the
  riskiest code in this plan — executes only under tests. It ships dormant.
- `SeparationPool::new` parks on spawn failure rather than degrading to serial.
  Reachable only if the OS refuses a thread; accepted, logged by T6.
- Windows/macOS DXIL + metallib need a native rebuild for the new shader; only
  the Vulkan/SPIR-V path was regenerated and verified on this host.

## Log

- 2026-08-09 pre-flight — branch `plan/horde-sim-headroom` cut from `main`
  (`5ec9df6`), plan artifacts committed `be5fc12`, pushed to origin.
- 2026-08-09 T0 start — tier deep, ship production.
- 2026-08-09 T0 attempt 1 `failed` — **plan defect**, correctly reported, no
  commit. `SCALE_COUNTS` / `GATE_AGENT_COUNT` in `crates/mmd-engine/src/bench/policy.rs`
  are load-bearing inputs to `validate_release_proof`, the merge gate and all
  three candidate lanes, which validate ~15 committed evidence artifacts under
  `lab/fixtures/**` and `lab/releases/evidence/**` recorded at 1 000 / 10 000 /
  50 000 / 100 000 agents. Retuning them reds 31 `mmd-lab` tests. Isolation
  proven: reverting only `policy.rs` turns all 31 green.
- 2026-08-09 T0 defect resolved by the orchestrator — see Decision D1 below.
  Ticket file amended (Requirements bench-policy bullet, new "Designated
  history, extended" bullet, Impl steps 8/9/11). Repair loop 1/1 dispatched to
  the same deep worker, which still holds the working tree.
- 2026-08-09 T0 `done` `df309d3` — repair surfaced **D2** (`test_short()` is the
  one policy that executes the sim; the frozen ladder's 10 000 tier exceeds the
  new ceiling). Gate digest re-pinned once: `864147ca…1ee881`.
- 2026-08-09 T1–T5 `done` (`de49405`, `dda283b`, `9b195d7`, `2d526db`,
  `ac39344`) — every one bit-exact against T0's digest. T3 hit a pre-existing
  test conflict (`tracked_scenes_declare_the_identity_tuning` asserted all seven
  scenes stay at `separation_phases == 1`, contradicting T3's own requirement);
  resolved by a narrow `collision_mid_v1` special case.
- 2026-08-09 T6 `done` `50b43aa` + `862bf1e` — red-team found a real
  panic-deadlock: a panic in any chunk abandoned the rendezvous, so the other
  participants blocked on `done` forever and a panicking ticker then hung in
  `Drop` behind them. A failed assertion would have surfaced as a *stalled merge
  gate*, not a red test. Fixed by catch-flag-rendezvous-rethrow, pinned by a
  `phases = 0` injection test that hangs rather than fails if regressed. Also
  promoted the re-entrancy guard from `debug_assert!` to `assert!` and added
  explicit release-acquire pairs. Ticket's specified thread set (1 and 4) was
  inadequate — at `phases: 4` every chunk start is ≡0 mod 4, so the phase-seed
  correction was never exercised; worker used 1/3/4/7 and mutation-proved it.
- 2026-08-09 T8 `done` `d5c0ba9` — found the shader hash pinned in five
  manifests, not the three the ticket named, and an unbound `.spv`.
- 2026-08-09 T9 `done` `51c73dc` — ticket was self-inconsistent on depth (three
  mutually exclusive statements; under `LESS`/clear-1 the horde renders
  back-to-front, 4 482 wrong pixels observed). Shipped `GREATER`/clear-0 and
  documented the deviation. Review then caught a whole-frame blanking bug: a
  depth key of exactly `0.0` was discarded rather than sorted last; fixed by
  flooring sprite depth at one D16 quantum. The `GreaterOrEqual` alternative two
  reviewers proposed was deliberately rejected — it admits the tie and re-blinds
  the occlusion test.
- 2026-08-09 T9 → T7 fact inlining: the first pass silently no-op'd because the
  guard string matched a pre-existing stub; caught by grepping for the expected
  content and re-run with a distinct guard (`79af58b`).
- 2026-08-09 T7 `done` `64f17f4` — **deviation: worker did not invoke `ship`**
  despite the prompt naming it, and claimed the prompt did not. Docs-only slice;
  covered by the reviewer fanout instead.
- 2026-08-09 review fanout — four fresh-context deep reviewers, one per
  dimension. Two blockers: T0's population sweep matched digits but not the
  `50k` contraction (14 live sites survived, including the shipped window title
  and a generator emitting a scene the loader rejects), and the mass scale factor
  was unpinned (mutating it to `scale = mass[j]` left the entire suite green, on
  a scene that ships two mass classes). T10 written from the four reports.
- 2026-08-09 T10 `done` `736c803` — closes 2 blockers + 8 should-fixes. Every
  replacement test mutation-proven: observed failing under the injected bug,
  passing after revert. The ring was drawn √2 too large while `runtime.rs`
  claimed it "traces the true contact circle… never an approximation"; fixed,
  and now asserted geometrically (`the_rings_of_two_touching_bodies_are_tangent`)
  rather than by re-deriving the formula.
- 2026-08-09 parent final validation on `1cf57d4` — see the section above. All
  eleven tickets terminal `done`, branch pushed, `main` untouched at `5ec9df6`.
- 2026-08-10 progress file committed and the branch rebased onto `main` on user
  request. `main` was already an ancestor of the branch head, so the rebase was
  a fast-forward no-op — no history rewritten, no force-push.
