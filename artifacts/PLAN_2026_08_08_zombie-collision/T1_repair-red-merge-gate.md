# T1: Repair the red merge gate

**Plan:** `./artifacts/PLAN_2026_08_08_zombie-collision.md`
**Depends:** none
**Commit outcome:** `cargo test --workspace --locked` is green again — `tests/validation_contract.rs` reads the doc that exists (`docs/CONTEXT.md`) instead of the one that was deleted.

## Context (self-contained)

- Goal: add agent-agent soft separation ("zombie collision") to the fixed-tick simulation. Every later ticket validates itself with "the merge gate is green", so the gate must actually be green first.
- This slice: pure repair, no feature work. The branch already carries a staged doc consolidation — `docs/00-vision.md`, `docs/02-prototype-roadmap.md`, `docs/03-mvp.md`, `docs/04-design-decisions.md` were deleted and folded into `docs/CONTEXT.md` — but `tests/validation_contract.rs` still points at `docs/02-prototype-roadmap.md` and panics reading it.
- Verified failure on this working tree, before any change:
  ```text
  test gate_docs_state_perf_gating_is_retired ... FAILED
  test no_perf_claim_in_docs ... FAILED
  thread panicked at tests/validation_contract.rs:82:55:
  read /home/aron/projects/millions_must_die/docs/02-prototype-roadmap.md: No such file or directory (os error 2)
  test result: FAILED. 5 passed; 2 failed
  ```
- Out of scope here: any collision/simulation/scenario/renderer change; restoring the deleted docs; editing `docs/CONTEXT.md` prose; touching the retired-perf policy itself.
- Assumptions in force: `docs/CONTEXT.md` is the replacement roadmap doc and already contains the substring `05-testing.md` and a retirement-marked perf line — both verified below, do not add prose to satisfy them.

## Requirements

- `ROADMAP_DOC` in `tests/validation_contract.rs` resolves to a file that exists.
- The same path is written **once**. Today it is written twice: once as the `ROADMAP_DOC` const (line ~25) and once as a bare string literal inside `gate_docs_state_perf_gating_is_retired` (line ~283). The literal must be replaced by the const, or the next doc rename reintroduces exactly this bug.
- No other test behaviour changes. No doc content changes.

## Inputs

- `tests/validation_contract.rs` — the only source file edited.
- `docs/CONTEXT.md` — read-only. Relevant lines (verified):
  - line 35: `the development host. Performance is **unmeasured** and was never a phase-0` — contains no perf token, so the scanner ignores it.
  - line 36: `criterion: frame-time gating is retired, along with the` — contains the token `frame-time` **and** the retirement marker `retired`, so `line_states_a_live_perf_claim` returns false. Safe.
  - line ~41: `[functional close](technical-prototype-functional-close.md). What gates a merge:` followed by `[testing strategy](05-testing.md)` — satisfies the `05-testing.md` link requirement of `gate_docs_state_perf_gating_is_retired`.
- Scanner rules that matter (do not change them):
  - `LIVE_DOCS` (line ~750) includes `ROADMAP_DOC`; every line of each live doc is scanned for a perf token, and a token line must also carry one of `RETIREMENT_MARKERS` = `retired`, `retirement`, `superseded`, `not a gate`, `gates nothing`, `unmeasured`, `no longer`, `says nothing`, `claims nothing`, `claiming nothing`, `not proven`, `no performance claim`.
  - The per-doc anti-vacuity floor in `no_perf_claim_in_docs` applies only to `CONTRACT_DOC` (≥2 token lines) and `README_DOC` (≥1). `ROADMAP_DOC` has no floor, so `docs/CONTEXT.md` needs no added prose.
- **From Depends:** none — this is the first ticket.

## User interaction to frontload

No package install, account, or key is needed for the whole plan. Two host facts, recorded here so no later ticket blocks on them:

- `TODO(user)` — `AGENT.md` instructs `graphify update .` after code changes, but the CLI is not installed on this host (`graphify: command not found`). No ticket in this plan runs it. Install it separately if the knowledge graph should stay current.
- The development host has a GPU (Linux / Vulkan / RTX 5060 Ti per `docs/technical-prototype-functional-close.md`). Run the full gate with `MMD_REQUIRE_GPU=1` so GPU cases fail instead of silently skipping.

## TDD

1. **Red** — run the existing suite and capture the two named failures. No new test is written: `gate_docs_state_perf_gating_is_retired` and `no_perf_claim_in_docs` **are** the red tests, and they are already mapped in `SCOPE_SYSTEMS` under `Merge-gate contract`.
   ```sh
   cargo test --test validation_contract 2>&1 | tail -20
   ```
   Expect: `test result: FAILED. 5 passed; 2 failed`, both panics naming `docs/02-prototype-roadmap.md`.
2. **Green** — repoint the const, dedupe the literal, rerun.
3. **Refactor** — none. Keep the diff to two lines plus a comment.

## Test plan

| Test | Input | Expect |
| ---- | ----- | ------ |
| `gate_docs_state_perf_gating_is_retired` | `docs/CONTEXT.md` present, contains `05-testing.md` | passes |
| `no_perf_claim_in_docs` | `docs/CONTEXT.md` scanned as a live doc | passes; line 36 is token-with-marker, not a live claim |
| `perf_claim_scanner_catches_what_it_is_meant_to` | unchanged fixtures | still passes (proves the scanner was not loosened to go green) |
| `every_system_has_a_test` | unchanged map | still passes |
| whole workspace | — | `cargo test --workspace --locked` green |

## Impl steps

- [x] 1. Run `cargo test --test validation_contract 2>&1 | tail -20` and save the failing output as the red baseline.
- [x] 2. In `tests/validation_contract.rs`, change the const at line ~25 from `const ROADMAP_DOC: &str = "docs/02-prototype-roadmap.md";` to `const ROADMAP_DOC: &str = "docs/CONTEXT.md";`.
- [x] 3. Update that const's doc comment to read: `/// Doc that states what each phase claims (roadmap + vision + MVP scope, consolidated).`
- [x] 4. In `gate_docs_state_perf_gating_is_retired` (line ~283), replace the bare literal `"docs/02-prototype-roadmap.md"` inside the `for rel in [...]` array with `ROADMAP_DOC`, so the array reads `[README_DOC, ROADMAP_DOC, RESULTS_DOC, "CONTRIBUTING.md"]`.
- [x] 5. Add one comment line above that array: `// Paths come from the consts above — a doc rename must break in one place, not two.`
- [x] 6. Run `cargo test --test validation_contract` and confirm `7 passed; 0 failed`.
- [x] 7. Run `cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets --all-features -- -D warnings`.
- [x] 8. Run the full gate (see Validation) and record that it is green — this is the baseline every later ticket compares against.

## Outputs

- Files touched: `tests/validation_contract.rs` (const value, its doc comment, one array element, one comment).
- Public API / behaviour change: none. Test-only.
- Migrate / config: none.

## Validation

- [x] `cargo test --test validation_contract` → `test result: ok. 7 passed; 0 failed`
- [x] `cargo fmt --all -- --check` → exit 0, no output
- [x] `cargo clippy --workspace --all-targets --all-features -- -D warnings` → exit 0
- [x] `MMD_REQUIRE_GPU=1 cargo test --workspace --locked` → all binaries pass, no skips
- [x] `cargo run -p xtask -- bootstrap --check` → exit 0
- [x] `cargo run -p xtask -- shaders --check` → exit 0
- [x] `cargo run -p xtask -- atlases --check` → exit 0
- [x] `cargo run -- run --agents 50000 --frames 300` → exit 0, prints a `run: clean exit ...` line
- [x] app functional — no broken path from this slice (test-only edit)
- [ ] commit msg draft: `fix(tests): point the gate scan at docs/CONTEXT.md after the doc consolidation`
