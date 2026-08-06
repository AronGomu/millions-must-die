# Handoff — Technical Prototype (`plan/technical-prototype`)

> **SUPERSEDED 2026-08-06 — this handoff is history, not a live instruction.** T33 completed at `9bffc10`; **phase 0 is closed on functional scope**. The shell failure described below is gone (verified). The only outstanding item is opening the unmerged PR (D6), which needs `gh auth login` — see the T33 entries in `.tmp/IMPLEMENT_PROGRESS_technical-prototype.md`, which is the authoritative record. Do **not** resume at T33; per D20, phase 1 does not start.


- Written: 2026-08-06 (session 2, resumed from `.tmp/HANDOFF_technical-prototype.md`)
- Branch: `plan/technical-prototype`
- HEAD at stop: `3450f86` — `docs(progress): record T32 commit sha`, pushed to `origin`
- Plan: `.tmp/IMPLEMENTATION_PLAN_technical_prototype.md`
- Progress (authoritative ticket state): `.tmp/IMPLEMENT_PROGRESS_technical-prototype.md`
- Stopped by: **user request**, mid-T33. Also blocked independently — see "Environment blocker" below. **Not a code failure.** No worker in flight, nothing uncommitted (the T33 worker deliberately left the tree clean).

## Where the plan stands

Phase 0 closes on functional tests of the game systems: chain T28 → T29 → {T30, T31, T32} → T33. The 2026-08-05 amendment #2 retired the whole performance/platform program (T9–T27, `retired (perf deferred)` — kept in history, nothing deleted).

| ID | Title | State | SHA |
| --- | ----- | ----- | --- |
| T28 | Retire perf gating | done | `94a075b` |
| T29 | Deterministic system test harness | done | `1725eae` |
| T30 | Simulation + navigation behaviour | done | `b70fd12` |
| T31 | Render correctness (single host) | done | `fa355d7` |
| T32 | App + CLI behaviour | done | `8ec342c` |
| T33 | Functional phase close | **pending — resume here** | — |

**T33 is the last ticket.** Its deps (T30, T31, T32) are all done. Finishing it closes phase 0.

## Environment blocker — read this first

Shell execution died host-wide part-way through this session and never recovered. Every path returns **exit 1 with no stdout and no stderr**: `Bash` foreground, `Bash` background, `dangerouslyDisableSandbox: true`, `Monitor`, and a freshly-spawned subagent's own shell. It fails for `true` and `echo hello`, not just `cargo`. Confirmed from both the T33 worker and the orchestrator.

Consequences for the next session:

- **Verify the shell works before dispatching T33** (`echo alive`). If it is still dead, T33 cannot be honestly completed — its two tests must be red-then-green and mutation-verified, and the ticket closes only on a full green gate.
- This handoff file and the progress-file updates for T33 **could not be committed**. Commit them manually once the shell returns:
  ```
  git add HANDOFF.md .tmp/IMPLEMENT_PROGRESS_technical-prototype.md
  git commit -m "docs(progress): hand off after T32 for the next session"
  git push origin HEAD
  ```
- Everything through T32 **is** committed and pushed. `3450f86` is safe on `origin`.

## Resume command

```
/implement-plan-aron @.tmp/IMPLEMENTATION_PLAN_technical_prototype.md continue
```

The skill re-reads the progress file, skips `done`, and picks up at T33.

## What this session did

### T31 — Render correctness (`fa355d7`)

New `crates/mmd-engine/tests/render_correctness.rs`: 11 headless tests (`instance_per_alive_agent`, `instances_carry_agent_position_and_animation_uvs`, `packing_is_a_pure_projection_of_sim_state`, `world_to_clip_transform`, `atlas_manifest_matches_generated_pngs`, `golden_drift_fails`, `golden_drift_reports_the_first_differing_pixel`, `diff_rejects_mismatched_buffers`, `unbound_golden_writes_no_diff`, `no_gpu_skips_cleanly`, `device_creation_failure_classifies_as_unavailable`) plus 4 GPU tests that auto-skip without a device (`golden_frame_matches`, `golden_drift_fails_on_gpu`, `world_to_clip_matches_gpu_raster`, `renderer_smoke_device_resize_shutdown`).

Mutation verification: 25 injected regressions, 25 killed. The one initial survivor (single-corner quad shrink) was replaced with a uniform half-scale mutation that kills.

**Two real defects found and fixed, not merely tested:** destroying a window still claimed by the GPU device left a dangling swapchain → SIGSEGV on shutdown. `sdl3` 0.18.4 exposes no release, so `GpuContext::release_window` was added (it drains the device first) and paired at every exit in `src/run.rs`, including paths that previously returned through `?` with a live window. Separately, `event_pump()?` sat between claim and release — the same hazard on an error path — and was hoisted above the claim.

### T32 — App + CLI behaviour (`8ec342c`)

`tests/cli_contract.rs` extended from 3 to 24 cases, plus 4 unit tests in the new `src/input.rs`: frame budget offscreen + windowed (1/6/9), agent override, pause freeze vs control run, pause-from-frame-1, overlay inertness (exact HUD line count), quit release + no-signal exit, quit-before-frame-1, quit cancels its own frame, multi-entry scripts, unfired-press rejection, env budget (valid/malformed/zero), headless default, missing/corrupt/drifted/invalid scenario, missing sidecar, `--agents 0` and over-cap, `--frames 0`, malformed scripts, exit codes 1/2/3.

**Three defects found and fixed in that diff:** the windowed loop checked its budget at the tail, so `--frames 1` rendered 2 frames; a parallel first-frame body meant a frame-1 overlay/quit was ignored and the documented contract did not match reality (frame 1 now uses the same body); and the first "state hash unchanged" guard could never fire, because `state_hash` digests `tick_index` — replaced with a tick/frame lockstep guard that can.

Mutation verification: 35 mutants, 32 killed. **Three survive by construction, not by weak tests** (each bounded in a test comment) — see the gap list below.

New deps on the app package: `hex` (normal — replaces a hand-rolled encoder, already present transitively) and `sha2` (**dev-only**, lets tests write scenarios with honest sidecar hashes). `docs/05-testing.md` gained an "App and CLI lifecycle" section (stdout contract + exit-code table + injection).

## T33 — prep already done (a retry starts warm)

The worker completed its read-only investigation before the shell died. Carry this forward:

**Test corpus located** — 10 engine test binaries plus 2 app-level: `tests/{cli_contract,validation_contract}.rs` and `crates/mmd-engine/tests/{simulation,flow_field,harness,scenario_contract,render_correctness,runtime_frame,gpu_smoke,gpu_golden,frame_allocations,benchmark_policy}.rs`, with helpers in `crates/mmd-engine/tests/common/mod.rs`. Named tests already harvested for navigation (11 in `flow_field.rs`), simulation/movement/recycle (15 in `simulation.rs`), and render correctness (15 in `render_correctness.rs`). CLI lifecycle (24 cases) and the allocation invariant still need a name sweep — one `grep` once the shell returns.

**Design conclusion for the two required tests:**

- `every_system_has_a_test` should hold the scope-system list as a `const` **in test code** and resolve each doc-mapped test name against a source scan of `#[test]` fns across the repo (plus the file column it claims), so deleting or renaming a mapped test fails it. It must not be a doc-restates-itself check.
- `no_perf_claim_in_docs` should scan a `const LIVE_DOCS` set (README, roadmap, `05-testing`, the new close doc) and require every line carrying a perf-metric token to also carry a retirement/negation marker. `docs/technical-prototype-results.md` and the ADRs stay **out** of that set as designated history, matching what `validation_contract.rs::results_doc_is_superseded_history_not_a_claim` already guards.
- Both must **complement, not duplicate**, T28's `gate_list_has_no_perf_thresholds`, which already parses the `Required merge gate` section.

**Impl step 3 of T33** ("update progress file") is the **orchestrator's** job, not the worker's — the worker leaves that box unchecked.

## Constraints that bind every remaining ticket

- `docs/05-testing.md` is the single source of truth for the merge gate.
- `tests/validation_contract.rs` is a contract test that **fails** if any required-path command reintroduces a frame-time/nmad threshold. **No new test may assert on timing.**
- `crates/mmd-engine/src/bench/` and `tools/mmd-lab/` are **frozen in place** — they compile and keep their own unit tests, but gate nothing. Do not delete, move, or re-gate them.
- `testkit` is a Cargo feature genuinely excluded from shipping builds (`cargo tree -e features | grep -c testkit` → 0). Keep it that way.
- Tracked fixtures `assets/scenarios/fixtures/{fixture_small_v1,fixture_corridor_v1,fixture_dense_v1,fixture_walled_v1}.{ron,sha256}`, registered in `ALL_FIXTURES`. Any new fixture follows the same `.sha256` contract and the validation rules in `crates/mmd-engine/src/scenario.rs`.
- **A test that only asserts "no panic" is not proof.** T30 mutation-verified 12 tests, T31 25, T32 35 — each pass caught a genuinely weak test. T33 must do the same for both of its tests, in both directions.
- Do not weaken or delete any existing test to make the close look cleaner.

## Known gaps to carry into the T33 close doc

- **No performance claim is valid anywhere.** The honest statement is "performance unmeasured; deferred to the optimization phase". `docs/technical-prototype-results.md` is superseded — history, not a claim.
- **No cross-platform verification.** Goldens are host-scoped (Linux/Vulkan, RTX 5060 Ti). Windows/macOS are deferred hardware, out of phase-0 scope; the deferred-hardware backlog (T9/T10/T16/T19/T22) stays open.
- **Flaky under full parallel load:** `warmup_allocation_passes`, `panic_restores_guard` (allocator counter cross-talk, T12 scope). Pass isolated. Must be fixed or isolated before they mask a real regression — do not weaken them.
- **Diagonal corner case** (recorded by T30, not a defect today): the movement step samples only the destination cell, so a corner-adjacent diagonal step could in principle land in a different neighbour than the field intended and wedge permanently. `no_agent_is_stuck_against_an_obstacle` is the guard. 0 occurrences observed.
- **Shipping CLI accepts a `fixture_*` scenario via `--scenario`**, drawing a small world into the fixed 1080p view. Cosmetic; the obvious fix sits in the T28-frozen `bench::runner`, left alone deliberately.
- `Runtime` retains ~1.6 MiB of nav data per instance, duplicating what `Simulation` copies. No consumer holds more than one Runtime; `Arc<FlowField>` if it ever matters.
- **T31 limits:** goldens stay at `lab/goldens/<family>/`, not the plan's `assets/goldens/` (deferred-platform placeholder families and the frozen `lab/` layout bind to that path — recorded in the plan). A lavapipe-only host **fails rather than skips**, because reclassifying would also excuse the macOS never-MoltenVK policy rejection and the two are indistinguishable on a host with neither. `gpu_golden.rs::host_offscreen_matches_tracked_golden` is now a near-duplicate of `golden_frame_matches` but stays `#[ignore]` and frozen.
- **T32 limits — three mutants survive by construction:** (1) removing `ctx.release_window()` while keeping its report — the T31 use-after-free does not reproduce deterministically at CLI level, so the engine-level claim/release contract stays owned by `render_correctness.rs`; (2) disabling the tick/frame lockstep guard — redundant defensive code on the *interactive* command that no test drives; (3) `is_device_unavailable()` → `true`, unobservable from the CLI on a GPU host, with both observable directions killed by the two exit-code mutants.
- **Retired bench/lab code rots if left untouched** — it still compiles and its unit tests still run, but it must be re-validated before the optimization phase reuses it.
- `f32` ≠ bit-identical across builds; determinism claims are same-host, same-binary.

## Current gate (all green at `8ec342c`, verified by the T32 worker)

```
cargo fmt --all -- --check
MMD_REQUIRE_GPU=1 cargo test --workspace --locked      # 34 binaries, 0 failures
VK_DRIVER_FILES=/nonexistent cargo test --workspace --locked   # no DISPLAY; 34 binaries, GPU cases skip
cargo clippy --workspace --all-targets --all-features -- -D warnings
nix flake check
cargo run -p xtask -- bootstrap --check ; shaders --check ; atlases --check
cargo run -- run --agents 50000 --frames 300           # exit 0, mode=window tick=300 frames=300
cargo tree -e features | grep -c testkit               # 0
```

Nothing was run after `8ec342c` — the shell died before T33 executed a single command.
