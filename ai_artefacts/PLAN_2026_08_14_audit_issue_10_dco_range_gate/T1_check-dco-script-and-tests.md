# T1: Offline DCO range script + behavioral tests

**Plan:** `./ai_artefacts/PLAN_2026_08_14_audit_issue_10_dco_range_gate.md`  
**Depends:** none  
**Commit outcome:** Tracked `./scripts/check-dco` rejects unsigned / non-descendant / wrong-key ranges and accepts fully signed ranges; proven by temp-git integration tests that stay workspace-green on Windows. No product/game code change. No gate-doc edit yet.

## Context (self-contained)

- Goal: mandatory DCO policy needs offline trusted-base→exact-candidate range enforcement. Today no script inspects commit trailers; unsigned candidates can pass every documented gate cmd.
- This slice: implement checker + automated behavioral tests only.
- Out of scope here: editing `docs/05-testing.md` / `README.md` / `AGENT.md` / `CONTRIBUTING.md`; `tests/validation_contract.rs` gate-list lock; GitHub Actions; history rewrite; email ownership crypto; engine/app source.
- Assumptions in force:
  - Args exactly two positional revs: trusted base, exact candidate. Doc/gate placeholder tokens (not shell metacharacters): `TRUSTED_BASE_SHA` and `EXACT_CANDIDATE_SHA`.
  - Range `base..candidate` via `git rev-list --reverse`.
  - Trailer parse: `git interpret-trailers --parse` on `git log -1 --format=%B <sha>`.
  - Valid trailer = key equals `Signed-off-by` **case-insensitive**, value matches regex below.
  - Wrong keys (`Acked-by`, `Signed-off-bys`, etc.) never count as SOB.
  - Tests never touch this repo’s commits; only `TempDir` fixtures.
  - Legacy main not revalidated unless operator passes ancient base (operator error, not default).
  - Root `tests/` must compile and pass on Windows: no unconditional unix-only APIs; bash required on PATH for non-unix script runs.

## Requirements

### CLI contract — `scripts/check-dco`

- Path: `scripts/check-dco` (repo-relative `./scripts/check-dco`).
- Shebang: `#!/usr/bin/env bash`
- Second line after shebang (comment OK):
  ```bash
  # Offline DCO range gate: TRUSTED_BASE_SHA..EXACT_CANDIDATE_SHA
  ```
- Immediately after header comments: `set -euo pipefail`
- Usage (stderr, exit **2**) — exact line preferred:
  ```text
  usage: scripts/check-dco TRUSTED_BASE_SHA EXACT_CANDIDATE_SHA
  ```
  (argv0 basename form OK: `usage: check-dco TRUSTED_BASE_SHA EXACT_CANDIDATE_SHA` — tests match substring `usage:` + both tokens `TRUSTED_BASE_SHA` and `EXACT_CANDIDATE_SHA`.)
- **Arg count:** if `$#` ≠ 2 → print usage → exit **2**.
- **Resolve revs — `set -e`-safe (locked; never bare git):**
  ```bash
  if ! base_full=$(git rev-parse --verify "${1}^{commit}" 2>/dev/null); then
    echo "error: trusted-base-sha not a commit: ${1}" >&2
    exit 2
  fi
  if ! candidate_full=$(git rev-parse --verify "${2}^{commit}" 2>/dev/null); then
    echo "error: exact-candidate-sha not a commit: ${2}" >&2
    exit 2
  fi
  ```
  Stderr must contain `trusted-base-sha` (bad base) or `exact-candidate-sha` (bad candidate). Exit **2**. Do **not** let bare `git rev-parse` fail under `set -e` (that yields git status ~128 and skips custom message).
- **Ancestor check — `set -e`-safe (locked; never bare git):**
  ```bash
  if ! git merge-base --is-ancestor "$base_full" "$candidate_full"; then
    echo "error: candidate is not a descendant of trusted base (${candidate_full} vs ${base_full})" >&2
    exit 1
  fi
  ```
  Stderr **must** include exact prefix:
  ```text
  error: candidate is not a descendant of trusted base
  ```
  and **must also contain** both full SHAs. Exit **1**. Do **not** bare-call `git merge-base --is-ancestor` under `set -e` (shell would exit 1 with no custom stderr).
- Empty range: if `base_full == candidate_full` → exit **0** (no stdout required). May short-circuit before rev-list.
- Commit list:
  ```bash
  git rev-list --reverse "${base_full}..${candidate_full}"
  ```
- Per commit SOB check:
  1. `body=$(git log -1 --format=%B "$sha")`
  2. `trailers=$(printf '%s' "$body" | git interpret-trailers --parse)`  
     (or `printf '%s\n'`; must not drop final trailer)
  3. For each trailer line `Key: Value` from parse output:
     - Compare key case-insensitively to `signed-off-by` (**exact** key after lowercasing; `signed-off-bys` / `acked-by` must **not** match)
     - Value must match this **POSIX ERE** via `grep -E`:
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
     Use full SHA from `git rev-parse` / rev-list (no short hash). Note: angle brackets here are documentation of the SHA field in the message template only; the printed line is literally `missing Signed-off-by: ` + full hex SHA (no angle brackets around the hash).
- After scanning **all** commits: if any missing → exit **1**; else exit **0**.
- Operate on **current working directory’s git repo** (tests set `Command::current_dir(temp)`). Do **not** hardcode product repo path.
- No network. No `git rebase` / `git filter-branch` / `git commit --amend` of caller history.
- File mode: executable bit set in git index (`chmod +x` + `git update-index --chmod=+x scripts/check-dco` on add). Index mode must be `100755`.

### Tests — `tests/dco_range_gate.rs`

New integration test file (auto-picked by cargo as `dco_range_gate`).

Helper fixture (private in test file):

- `TempDir` via `tempfile::TempDir`
- `git init -b main` inside (locked; do **not** use plain `git init` without `-b main`)
- `git config user.name "DCO Tester"`
- `git config user.email "dco-tester@example.com"`
- `git config commit.gpgsign false`
- Helper `commit(msg_body: &str) -> String` full SHA:
  - write/change file `tracked.txt`
  - `git add tracked.txt`
  - `git -c commit.gpgsign=false commit --cleanup=verbatim -F -` with provided full message body (allows controlling trailers)
  - **Never** pass `-s` / `--signoff`; always embed trailers manually in `msg_body` so fixtures are deterministic across git templates
- Helper `run_check(base: &str, cand: &str) -> std::process::Output` — **platform spawn locked**:

  ```rust
  fn run_check(fixture: &Path, base: &str, cand: &str) -> std::process::Output {
      let script = repo_root().join("scripts/check-dco");
      #[cfg(unix)]
      {
          // Shebang exec path (unix).
          std::process::Command::new(&script)
              .current_dir(fixture)
              .args([base, cand])
              .output()
              .unwrap_or_else(|e| panic!("spawn scripts/check-dco: {e}"))
      }
      #[cfg(not(unix))]
      {
          // Windows/other: CreateProcess does not honor shebang.
          // bash required on PATH for these behavioral tests.
          std::process::Command::new("bash")
              .arg(&script)
              .current_dir(fixture)
              .args([base, cand])
              .output()
              .unwrap_or_else(|e| {
                  panic!(
                      "spawn bash scripts/check-dco failed: {e}; \
                       bash must be on PATH to run dco_range_gate tests on non-unix"
                  )
              })
      }
  }
  ```

- `repo_root()` = `PathBuf::from(env!("CARGO_MANIFEST_DIR"))`
- Never call check-dco against the real worktree for policy cases.
- **Unix-only APIs:** any `std::os::unix::fs::PermissionsExt` / mode-bit fs check **must** sit under `#[cfg(unix)]`. Never import `PermissionsExt` unconditionally in this root test file.

## Inputs

- No existing `scripts/` dir — create it.
- `tempfile` in root `Cargo.toml` `[dev-dependencies]` (already present).
- Host tools: `git` on PATH (already required for repo work); `bash` on PATH (unix via shebang env; non-unix tests require `bash` on PATH — documented fail message above).
- **From Depends:** none.

## TDD

1. **Red** — add `tests/dco_range_gate.rs` with tests below. Run targeted test cmd → fail because `scripts/check-dco` missing (or not executable) / wrong behavior.
2. **Green** — add minimal `scripts/check-dco` implementing contract; mark executable in git index.
3. **Refactor** — only if needed; keep green. No extra features.

## Test plan

| Test fn | Input | Expect |
| --- | --- | --- |
| `script_is_tracked_and_executable` | `scripts/check-dco` under `CARGO_MANIFEST_DIR` | (1) path is file; (2) `git -C $CARGO_MANIFEST_DIR ls-files -s -- scripts/check-dco` stdout has one line starting with `100755` and containing `scripts/check-dco` (**all platforms**); (3) **`#[cfg(unix)]` only:** fs mode has owner-exec bit (`PermissionsExt`, `mode & 0o100 != 0`). Non-unix: skip fs mode assert only — still require (1)+(2). |
| `signed_range_exits_zero` | root signed (manual valid SOB in body) → child signed (manual SOB) | exit code `Some(0)`; range base=root cand=child |
| `unsigned_commit_exits_nonzero_and_names_hash` | root signed → child message **without** SOB | exit code `Some(1)`; stderr contains `missing Signed-off-by:`; stderr contains full child SHA |
| `mixed_range_reports_only_unsigned` | root signed → mid unsigned → tip signed | exit `Some(1)`; stderr names mid SHA; stderr does **not** require naming tip/root as missing |
| `trailer_without_email_rejected` | commit body ends with `Signed-off-by: nobody` (no `<email>`) | exit `Some(1)`; stderr `missing Signed-off-by:` + that SHA |
| `wrong_key_trailer_rejected` | commit body ends with **only** non-SOB trailers — cover **both** in one test via two commits in range **or** two sub-assertions / two fixture commits checked separately: (a) `Acked-by: DCO Tester <dco-tester@example.com>` only; (b) `Signed-off-bys: DCO Tester <dco-tester@example.com>` only (plural spoof). Each must exit `Some(1)` with `missing Signed-off-by:` + that commit’s full SHA. Locked approach: build root signed; child A with only `Acked-by:…`; assert fail on root..A; then child B with only `Signed-off-bys:…` from root (or from a fresh signed base); assert fail on base..B. Minimum: one test fn that fails both shapes. |
| `lowercase_signed_off_by_key_accepted` | trailer key `signed-off-by: DCO Tester <dco-tester@example.com>` | exit `Some(0)` (case-insensitive key) |
| `non_descendant_candidate_exits_nonzero` | two sibling commits from same root; pass base=A cand=B where B not descendant of A | exit `Some(1)`; stderr contains exact prefix `error: candidate is not a descendant of trusted base`; stderr contains both full SHAs |
| `equal_base_and_candidate_exits_zero` | single signed commit; base=cand=that SHA | exit `Some(0)` |
| `usage_without_args_exits_two` | zero args (spawn script with no args; same platform spawn rules as `run_check` but empty args) | exit `Some(2)`; stderr contains `usage:`; stderr contains `TRUSTED_BASE_SHA`; stderr contains `EXACT_CANDIDATE_SHA` |
| `unresolvable_base_exits_two` | base=`not-a-real-sha`, cand=valid | exit `Some(2)`; stderr contains `trusted-base-sha`; exit code is **2** (not 128) |

Fixture build notes (locked):

- **signed commit message** always manual (never `git commit -s`):
  ```text
  signed child

  Signed-off-by: DCO Tester <dco-tester@example.com>
  ```
- **unsigned**: single line `unsigned child` only (no trailer block).
- **Acked-by only**:
  ```text
  acked only

  Acked-by: DCO Tester <dco-tester@example.com>
  ```
- **Signed-off-bys only** (plural spoof):
  ```text
  plural spoof

  Signed-off-bys: DCO Tester <dco-tester@example.com>
  ```
- **siblings for non-descendant**: after root, `git checkout -b other` + commit B; `git checkout main` + commit A; invoke with base=A cand=B.
- Disable signing: always `-c commit.gpgsign=false`.
- Init: **`git init -b main` only**.

## Impl steps

- [ ] 1. Create `tests/dco_range_gate.rs` with fixture helpers (`git init -b main`, manual SOB commits, platform-split `run_check`) + **all** test fns from Test plan including `wrong_key_trailer_rejected`. → verify red: `cargo test --locked --test dco_range_gate` fails (missing script or assertions). Confirm file compiles on non-unix shape (no bare `PermissionsExt` import).
- [ ] 2. Create dir `scripts/`. Write `scripts/check-dco` per CLI contract (bash, `set -euo pipefail`, two args, **wrapped** rev-parse exit 2, **wrapped** merge-base --is-ancestor exit 1 + exact stderr, rev-list, interpret-trailers --parse, regex value check, key must be exactly `signed-off-by` case-insensitive, aggregate missing lines). → verify file exists.
- [ ] 3. `chmod +x scripts/check-dco` then `git add scripts/check-dco` and `git update-index --chmod=+x scripts/check-dco` so index mode is `100755`. → verify `git ls-files -s scripts/check-dco` starts with `100755`.
- [ ] 4. Manually smoke inside a throwaway temp repo (optional but recommended): signed range exit 0; unsigned exit 1 + hash; `Acked-by`-only exit 1; bad rev exit 2 with custom stderr (not 128); non-descendant exit 1 with custom stderr. → verify matches Test plan.
- [ ] 5. Run `cargo test --locked --test dco_range_gate`. → verify all tests in file pass.
- [ ] 6. Run `cargo test --workspace --locked`. → verify full suite still green (no product regressions). On Windows hosts this must stay green: mode assert cfg’d; spawn uses bash.
- [ ] 7. Confirm no edits under `crates/`, `src/`, `docs/`, `README.md`, `CONTRIBUTING.md`, `AGENT.md`, `tests/validation_contract.rs`. → verify `git diff --name-only` only script + new test (+ Cargo.lock only if accidentally touched — do **not** change lock intentionally).

## Outputs

- Created: `scripts/check-dco` (index mode `100755`)
- Created: `tests/dco_range_gate.rs`
- Public behavior: `./scripts/check-dco TRUSTED_BASE_SHA EXACT_CANDIDATE_SHA` offline range DCO gate (operator substitutes real SHAs for the two tokens)
- No migrate/config
- Docs still lack gate wiring (T2)

## Validation

- [ ] `cargo test --locked --test dco_range_gate` — all named tests pass
- [ ] `cargo test --workspace --locked` — pass
- [ ] `git ls-files -s scripts/check-dco` — starts with `100755`
- [ ] manual: in temp repo, unsigned child → exit 1 and stderr lists full SHA; signed child → exit 0; `Acked-by`-only → exit 1; unresolvable base → exit 2 (not 128) + `trusted-base-sha` on stderr
- [ ] app functional — game paths untouched; no broken binary from this slice
- [ ] commit msg draft: `feat(ops): add offline DCO range checker`
