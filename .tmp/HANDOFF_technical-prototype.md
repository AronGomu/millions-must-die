# Handoff — Technical Prototype (plan/technical-prototype)

- Written: 2026-08-06
- Branch: `plan/technical-prototype` — clean, in sync with `origin`
- HEAD: `a907402`
- Plan: `.tmp/IMPLEMENTATION_PLAN_technical_prototype.md`
- Progress: `.tmp/IMPLEMENT_PROGRESS_technical-prototype.md` (authoritative ticket state)
- Stopped by: user request mid-run, after T30 committed + pushed. **Not a failure.** No worker was running, no work in flight, nothing uncommitted.

## Where the plan stands

The 2026-08-05 amendment #2 retired the whole performance/platform program (T9–T27, `retired (perf deferred)` — kept in history, nothing deleted). Phase 0 now closes on **functional tests of the game systems**, chain T28 → T29 → {T30, T31, T32} → T33.

| ID | Title | State | SHA |
| --- | ----- | ----- | --- |
| T28 | Retire perf gating | done | `94a075b` |
| T29 | Deterministic system test harness | done | `1725eae` |
| T30 | Simulation + navigation behaviour | done | `b70fd12` |
| T31 | Render correctness (single host) | **pending — resume here** | — |
| T32 | App + CLI behaviour | pending | — |
| T33 | Functional phase close | pending (needs T30+T31+T32) | — |

T31 and T32 both depend only on T29, so either may go first. The orchestrator runs them **serially** (implement-plan-aron: parallel off, one writer per cwd). T33 is last.

## Resume command

```
/implement-plan-aron @.tmp/IMPLEMENTATION_PLAN_technical_prototype.md continue
```

The skill re-reads the progress file, skips `done`, and picks up at T31.

## What the next session must know

**Constraints inherited from T28 — these bind every remaining ticket:**

- `docs/05-testing.md` is the single source of truth for the merge gate.
- `tests/validation_contract.rs` is a contract test that **fails** if any required-path command reintroduces a frame-time/nmad threshold. No new test may assert on timing.
- `crates/mmd-engine/src/bench/` and `tools/mmd-lab/` are **frozen in place** — they compile and keep their own unit tests, but gate nothing. Do not delete, move, or re-gate them. (The plan's `TODO(user)` on disposition was auto-decided as *(a) freeze in place*, the plan's own recommended option; logged under Assumptions in the progress file.)

**The harness T31/T32/T33 must build on (from T29):**

- `mmd_engine::testkit::{Harness, ...}` — `crates/mmd-engine/src/testkit/{mod,rng,fixtures}.rs`, driven by `crates/mmd-engine/tests/harness.rs`.
- Seeded sub-streams via `rng("<label>")` — labelled so future systems can't correlate.
- `testkit` is a Cargo feature genuinely excluded from shipping builds (`cargo tree` shows 0 occurrences in the binary).
- Fixture scenarios are tracked with sidecar hashes: `assets/scenarios/fixtures/{fixture_small_v1,fixture_corridor_v1,fixture_dense_v1,fixture_walled_v1}.{ron,sha256}`, registered in `ALL_FIXTURES`. Any new fixture must follow the same `.sha256` contract and the fixture validation rules in `crates/mmd-engine/src/scenario.rs` (size caps, renderer contract) — enforced by negative tests in `tests/scenario_contract.rs`.
- Shared behavioural helpers live in `crates/mmd-engine/tests/common/mod.rs` (`cell_of`, `assert_positions_finite_and_in_bounds`, `agents_in_obstacles`, `Tracker`).

**Hard requirement carried by the plan's risk list:** a test that only asserts "no panic" is not proof. T30 mutation-verified all 12 of its tests (patch engine source → run one test → require failure → revert). T31/T32 must do the same for their invariants — that first pass caught one genuinely weak T30 test that had to be strengthened.

## Known gaps to carry forward (not regressions)

- **Flaky under full parallel load:** `warmup_allocation_passes` and `panic_restores_guard` (allocator counter cross-talk, T12 scope). Pass isolated. Listed in the plan's active risk list; must be fixed or isolated before they mask a real regression. Do not weaken them.
- **Diagonal corner case (recorded by T30, not a defect today):** the movement step samples only the destination cell, so a corner-adjacent diagonal step could in principle land in a different neighbour than the field intended and wedge permanently. `no_agent_is_stuck_against_an_obstacle` is the guard that would catch it. 0 occurrences across all four fixtures and the 50k scene.
- **Shipping CLI accepts a `fixture_*` scenario via `--scenario`**, drawing a small world into the fixed 1080p view. Cosmetic. The obvious fix sits in `bench::runner`, which T28 froze — left alone deliberately.
- `Runtime` retains ~1.6 MiB of nav data per instance, duplicating what `Simulation` copies. No consumer holds more than one Runtime; `Arc<FlowField>` if it ever matters.
- **No performance claim is valid anywhere.** The honest statement is "performance unmeasured; deferred to the optimization phase". `docs/technical-prototype-results.md` is marked superseded — history, not a claim.
- No cross-platform verification. Goldens are host-scoped (Linux/Vulkan, RTX 5060 Ti). Windows/macOS remain deferred hardware, now also out of phase-0 scope.

## Current gate (all green at `a907402`)

```
cargo fmt --all -- --check
cargo test --workspace --locked          # 32 test binaries, 0 failures, ~45 s
cargo clippy --workspace --all-targets --all-features -- -D warnings
nix flake check
cargo run -p xtask -- bootstrap --check ; shaders --check ; atlases --check
cargo run -- run --agents 50000 --frames 300   # clean Vulkan exit
```
