# T1: Offline DCO range script + behavioral tests

**Plan:** `./ai_artefacts/PLAN_2026_08_14_audit_issue_10_dco_range_gate.md`  
**Depends:** none  
**Commit outcome:** Tracked `./scripts/check-dco` rejects unsigned / non-descendant ranges and accepts fully signed ranges; proven by temp-git integration tests. No product/game code change. No gate-doc edit yet.

## Context (self-contained)

- Goal: mandatory DCO policy needs offline trusted-base→exact-candidate range enforcement. Today no script inspects commit trailers; unsigned candidates can pass every documented gate cmd.
- This slice: implement checker + automated behavioral tests only.
- Out of scope here: editing `docs/05-testing.md` / `README.md` / `AGENT.md` / `CONTRIBUTING.md`; `tests/validation_contract.rs` gate-list lock; GitHub Actions; history rewrite; email ownership crypto; engine/app source.
- Assumptions in force:
  - Args exactly two positional SHAs/revs: trusted base, exact candidate.
  - Range `base..candidate` via `git rev-list --reverse`.
  - Trailer parse: `git interpret-trailers --parse` on `git log -1 --format=%B <sha>`.
  - Valid trailer = key equals `Signed-off-by` **case-insensitive**, value matches regex below.
  - Tests never touch this repo’s commits; only `TempDir` fixtures.
  - Legacy main not revalidated unless operator passes ancient base (operator error, not default).

## Requirements

### CLI contract — `scripts/check-dco`

- Path: `scripts/check-dco` (repo-relative `./scripts/check-dco`).
- Shebang: `#!/usr/bin/env bash`
- Second line after shebang comment OK: `# Offline DCO range gate: trusted-base..exact-candidate`
- Immediately: `set -euo pipefail`
- Usage (stderr, exit **2**):
  ```text
  usage: scripts/check-dco <trusted-base-sha> <exact-candidate-sha>
  ```
  (argv0 basename form OK: `usage: check-dco <trusted-base-sha> <exact-candidate-sha>` — tests match substring `usage:` + both placeholders.)
- Resolve revs with:
  ```bash
  git rev-parse --verify "${1}^{commit}"
  git rev-parse --verify "${2}^{commit}"
  ```
  On failure → stderr `error: trusted-base-sha not a commit: <arg>` or `error: exact-candidate-sha not a commit: <arg>` → exit **2**.
- Ancestor check:
  ```bash
  git merge-base --is-ancestor "$base_full" "$candidate_full"
  ```
  Fail → stderr exactly one line prefix:
  ```text
  error: candidate is not a descendant of trusted base
  ```
  Line **must also contain** both full SHAs (order: candidate then base, or both present). Exit **1**.
- Empty range: if `base_full == candidate_full` → exit **0** (no stdout required).
- Commit list:
  ```bash
  git rev-list --reverse "${base_full}..${candidate_full}"
  ```
- Per commit SOB check:
  1. `body=$(git log -1 --format=%B "$sha")`
  2. `trailers=$(printf '%s' "$body" | git interpret-trailers --parse)`  
     (or `printf '%s\n'`; must not drop final trailer)
  3. For each trailer line `Key: Value` from parse output:
     - Compare key case-insensitively to `signed-off-by`
     - Value must match this **POSIX ERE** via `grep -E` (locked impl sketch below):
       ```text
       ^.+ <[^[:space:]<>]+@[^[:space:]<>]+>$
       ```
       Meaning: non-empty name, one ASCII space, then `<email>` with one `@` and no whitespace/`<>` inside email.
       Accepted value example: `DCO Tester <dco-tester@example.com>`
       Full trailer canonical form: `Signed-off-by: Your Name <your.email@example.com>`
     - Locked bash check body:
       ```bash
       key="${line%%:*}"
       value="${line#*: }"
       key_lc=$(printf '%s' "$key" | tr '[:upper:]' '[:lower:]')
       if [[ "$key_lc" == "signed-off-by" ]] \
         && printf '%s' "$value" | grep -Eq '^.+ <[^[:space:]<>]+@[^[:space:]<>]+>$'; then
         ok=1
       fi
       ```
  4. At least one matching trailer → commit OK.
  5. Else record failure; stderr one line:
     ```text
     missing Signed-off-by: <full40+sha>
     ```
     Use full SHA from `git rev-parse` / rev-list (no short hash).
- After scanning **all** commits: if any missing → exit **1**; else exit **0**.
- Operate on **current working directory’s git repo** (tests set `Command::current_dir(temp)`). Do **not** hardcode product repo path.
- No network. No `git rebase` / `git filter-branch` / `git commit --amend` of caller history.
- File mode: executable bit set in git (`chmod +x` + `git update-index --chmod=+x scripts/check-dco` on add).

### Tests — `tests/dco_range_gate.rs`

New integration test file (auto-picked by cargo as `dco_range_gate`).

Helper fixture (private in test file):

- `TempDir` via `tempfile::TempDir`
- `git init` inside
- `git config user.name "DCO Tester"`
- `git config user.email "dco-tester@example.com"`
- `git config commit.gpgsign false`
- Helper `commit(msg_body: &str) -> String` full SHA:
  - write/change file `tracked.txt`
  - `git add tracked.txt`
  - `git -c commit.gpgsign=false commit -F -` with provided full message body (allows controlling trailers; do **not** always pass `-s`)
- Helper `run_check(base: &str, cand: &str) -> Output` runs:
  ```rust
  Command::new(repo_root().join("scripts/check-dco"))
      .current_dir(fixture_path)
      .args([base, cand])
      .output()
  ```
- `repo_root()` = `PathBuf::from(env!("CARGO_MANIFEST_DIR"))`
- Never call check-dco against the real worktree for policy cases.

## Inputs

- No existing `scripts/` dir — create it.
- `tempfile` in root `Cargo.toml` `[dev-dependencies]` (already present).
- Host tools: `git` on PATH (already required for repo work), `bash`.
- **From Depends:** none.

## TDD

1. **Red** — add `tests/dco_range_gate.rs` with tests below. Run targeted test cmd → fail because `scripts/check-dco` missing (or not executable) / wrong behavior.
2. **Green** — add minimal `scripts/check-dco` implementing contract; mark executable in git index.
3. **Refactor** — only if needed; keep green. No extra features.

## Test plan

| Test fn | Input | Expect |
| --- | --- | --- |
| `script_is_tracked_and_executable` | `scripts/check-dco` path under `CARGO_MANIFEST_DIR` | path is file; Unix mode has owner-exec bit (`mode & 0o100 != 0`) |
| `signed_range_exits_zero` | root signed (`-s` or manual valid SOB) → child signed | exit code `Some(0)`; range base=root cand=child |
| `unsigned_commit_exits_nonzero_and_names_hash` | root signed → child message **without** SOB | exit code `Some(1)`; stderr contains `missing Signed-off-by:`; stderr contains full child SHA |
| `mixed_range_reports_only_unsigned` | root signed → mid unsigned → tip signed | exit `Some(1)`; stderr names mid SHA; stderr does **not** require naming tip/root as missing |
| `trailer_without_email_rejected` | commit body ends with `Signed-off-by: nobody` (no `<email>`) | exit `Some(1)`; stderr `missing Signed-off-by:` + that SHA |
| `lowercase_signed_off_by_key_accepted` | trailer key `signed-off-by: DCO Tester <dco-tester@example.com>` | exit `Some(0)` (case-insensitive key) |
| `non_descendant_candidate_exits_nonzero` | two sibling commits from same root; pass base=A cand=B where B not descendant of A | exit `Some(1)`; stderr contains `not a descendant` (or exact prefix `error: candidate is not a descendant of trusted base`) |
| `equal_base_and_candidate_exits_zero` | single signed commit; base=cand=that SHA | exit `Some(0)` |
| `usage_without_args_exits_two` | zero args | exit `Some(2)`; stderr contains `usage:` |
| `unresolvable_base_exits_two` | base=`not-a-real-sha`, cand=valid | exit `Some(2)`; stderr contains `trusted-base-sha` |

Fixture build notes:

- **signed commit message** example:
  ```text
  signed child

  Signed-off-by: DCO Tester <dco-tester@example.com>
  ```
- **unsigned**: single line `unsigned child` only.
- **siblings for non-descendant**: after root, branch `git checkout -b other` + commit B; `git checkout main` + commit A; invoke with base=A cand=B.
- Disable signing: always `-c commit.gpgsign=false`.
- Init: `git init -b main` (or `git init` + ensure branch exists).

## Impl steps

- [ ] 1. Create `tests/dco_range_gate.rs` with fixture helpers + all test fns from Test plan. → verify red: `cargo test --locked --test dco_range_gate` fails (missing script or assertions).
- [ ] 2. Create dir `scripts/`. Write `scripts/check-dco` per CLI contract (bash, `set -euo pipefail`, two args, rev-parse, merge-base --is-ancestor, rev-list, interpret-trailers --parse, regex value check, aggregate missing lines). → verify file exists.
- [ ] 3. `chmod +x scripts/check-dco` then `git add scripts/check-dco` and `git update-index --chmod=+x scripts/check-dco` so index mode is `100755`. → verify `git ls-files -s scripts/check-dco` shows `100755`.
- [ ] 4. Manually smoke inside a throwaway temp repo (optional but recommended): signed range exit 0; unsigned exit 1 + hash. → verify matches Test plan.
- [ ] 5. Run `cargo test --locked --test dco_range_gate`. → verify all tests in file pass.
- [ ] 6. Run `cargo test --workspace --locked`. → verify full suite still green (no product regressions).
- [ ] 7. Confirm no edits under `crates/`, `src/`, `docs/`, `README.md`, `CONTRIBUTING.md`, `AGENT.md`, `tests/validation_contract.rs`. → verify `git diff --name-only` only script + new test (+ Cargo.lock only if accidentally touched — do **not** change lock intentionally).

## Outputs

- Created: `scripts/check-dco` (mode `100755`)
- Created: `tests/dco_range_gate.rs`
- Public behavior: `./scripts/check-dco <trusted-base-sha> <exact-candidate-sha>` offline range DCO gate
- No migrate/config
- Docs still lack gate wiring (T2)

## Validation

- [ ] `cargo test --locked --test dco_range_gate` — all named tests pass
- [ ] `cargo test --workspace --locked` — pass
- [ ] `git ls-files -s scripts/check-dco` — starts with `100755`
- [ ] manual: in temp repo, unsigned child → exit 1 and stderr lists full SHA; signed child → exit 0
- [ ] app functional — game paths untouched; no broken binary from this slice
- [ ] commit msg draft: `feat(ops): add offline DCO range checker`
