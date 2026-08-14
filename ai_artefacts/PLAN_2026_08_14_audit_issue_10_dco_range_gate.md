# Plan: audit issue #10 DCO range gate

## Goal

Add tracked offline DCO range checker `./scripts/check-dco TRUSTED_BASE_SHA EXACT_CANDIDATE_SHA`. Wire into authoritative required merge gate + CONTRIBUTING. Prove unsigned range fails (names hashes) and signed range passes. Success = script + contract tests green; gate docs list exact cmd; legacy main history never revalidated/rewritten.

## Scope

- In: `scripts/check-dco`; `tests/dco_range_gate.rs`; gate lock in `tests/validation_contract.rs`; fence + prose in `docs/05-testing.md`; mirror fences in `README.md` + `AGENT.md` (same command list already mirrored; leave stale → agent/doc drift); CONTRIBUTING DCO/PR prose; automated tests only (no GitHub Actions).
- Out: other audit findings; CI redesign; identity/email crypto beyond trailer presence; rewriting/revalidating existing unsigned main history; GPG/SSH commit signatures; GitHub App/status checks; changes to game/engine code.

## Assumptions

- Autonomous defaults (no grill).
- Trailer presence only: valid `Signed-off-by: Name <email>` via `git interpret-trailers --parse` + documented value regex. No email ownership proof.
- Range = commits reachable from candidate not from base: `git rev-list --reverse <base>..<candidate>` (base exclusive, candidate inclusive). Empty range (base == candidate) → exit 0.
- Trusted base chosen by maintainer (typical: `git merge-base main <candidate>` or main tip that is ancestor). Script never walks full root→HEAD unless operator passes root.
- Put DCO cmd **first** in required-gate fence → fail-fast before long cargo/nix work.
- Exit codes: `0` pass; `1` policy fail (missing trailer / non-descendant); `2` usage / unresolvable rev.
- Missing trailers: print **all** offenders then exit 1 (no fail-fast on first).
- Success silent (no required stdout).
- Tests use **throwaway temp git repos** only (`tempfile::TempDir`); never rewrite this worktree history.
- `README.md` must gain same fence line as `docs/05-testing.md` because `required_gate_*` tests already scan both.
- `AGENT.md` merge-gate block updated same way (duplicate list; SSoT remains `docs/05-testing.md`).
- No new ADR: ADR 008 already requires DCO; this adds offline enforcement only.
- No HTML plan / architecture docs (caller override).
- Bash script (`#!/usr/bin/env bash`, `set -euo pipefail`) matches `lab/provision/*` style.
- `tempfile` already root `[dev-dependencies]` → integration tests may use it.
- **Gate / docs / contract command string is shell-safe** (no unquoted `<…>` metacharacters). Locked exact token:
  ```text
  ./scripts/check-dco TRUSTED_BASE_SHA EXACT_CANDIDATE_SHA
  ```
  Same string in `docs/05-testing.md`, `README.md`, `AGENT.md`, `CONTRIBUTING.md`, `DCO_GATE_COMMAND`, and tests. Prose says maintainer replaces tokens with real SHAs at run time. Angle brackets remain only inside commit-message trailer examples (`Signed-off-by: Name <email>`), never as sh-fence argv placeholders.
- **Root tests workspace-green on Windows:** unix-only APIs under `#[cfg(unix)]`; non-unix script spawn via `Command::new("bash").arg(script)` (bash required on PATH for behavioral script tests; fail with clear message if bash missing). Mode-bit fs assert unix-only; index mode `100755` via `git ls-files -s` asserted on all platforms.
- **`set -e`-safe control flow locked** in T1 CLI contract: rev-parse and merge-base ancestor checks wrapped; never bare failing git for those paths.
- **CONTRIBUTING insert:** single anchor after DCO license fence closing ` ``` `, before `## Pull requests`.
- **Wrong-key trailer negative test required** (`Acked-by` / `Signed-off-bys` only → fail).

## Ticket flowchart

```mermaid
flowchart TD
  T1[T1: check-dco script + fixture tests] --> T2[T2: gate docs + contract lock]
```

## Ticket order

| ID | Title | Depends | Commit outcome | File |
| --- | --- | --- | --- | --- |
| T1 | Offline DCO range script + behavioral tests | — | `./scripts/check-dco` enforces signed range; temp-repo tests green on unix + non-unix | `PLAN_2026_08_14_audit_issue_10_dco_range_gate/T1_check-dco-script-and-tests.md` |
| T2 | Wire required merge gate + CONTRIBUTING + doc contract | T1 | Gate docs list shell-safe DCO cmd; `required_gate_contains_dco_check` locks it | `PLAN_2026_08_14_audit_issue_10_dco_range_gate/T2_gate-docs-and-contract.md` |

## Tickets

- [T1: Offline DCO range script + behavioral tests](PLAN_2026_08_14_audit_issue_10_dco_range_gate/T1_check-dco-script-and-tests.md) — depends: none
- [T2: Wire required merge gate + CONTRIBUTING + doc contract](PLAN_2026_08_14_audit_issue_10_dco_range_gate/T2_gate-docs-and-contract.md) — depends: T1

## Repair log (plan review F11)

Resolved from `F11-plan-review-{scope,exec,security}.md` before impl:

1. Shell-safe gate tokens (`TRUSTED_BASE_SHA` / `EXACT_CANDIDATE_SHA`) — exec blocker + security should-fix.
2. Windows workspace-green: `#[cfg(unix)]` mode/shebang; non-unix `bash` spawn — exec blocker.
3. Locked `set -e`-safe `if !` wrappers for rev-parse / merge-base — exec + security should-fix.
4. Single CONTRIBUTING anchor (after DCO license fence, before `## Pull requests`) — exec should-fix.
5. Wrong-key trailer negative test — security should-fix.
6. Index mode `100755` via `git ls-files -s` (all platforms) + unix fs exec bit — exec note.
7. Soft choices locked: manual SOB fixtures (never `-s`); `git init -b main`; AGENT DCO bullet required.
