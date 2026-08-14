# T2: Wire required merge gate + CONTRIBUTING + doc contract

**Plan:** `./ai_artefacts/PLAN_2026_08_14_audit_issue_10_dco_range_gate.md`  
**Depends:** T1  
**Commit outcome:** Required merge gate documents `./scripts/check-dco <trusted-base-sha> <exact-candidate-sha>` first in the fence; CONTRIBUTING states offline range enforcement; `required_gate_contains_dco_check` locks both doc mirrors; full workspace tests green.

## Context (self-contained)

- Goal: mandatory DCO must be part of authoritative offline merge workflow, not honor-system prose only.
- This slice: wire script from T1 into gate docs + CONTRIBUTING + automated doc contract. No script logic change unless a test reveals a doc/script mismatch (then fix doc to match T1 contract).
- Out of scope here: GitHub Actions; rewriting main history; revalidating legacy commits; identity crypto; engine/app behavior; changing T1 exit-code/trailer rules.
- Assumptions in force:
  - T1 left `scripts/check-dco` (mode `100755`) and `tests/dco_range_gate.rs` green.
  - Gate command string **exact** (whitespace-normalized match in contract tests):
    ```text
    ./scripts/check-dco <trusted-base-sha> <exact-candidate-sha>
    ```
  - Place this command as **first line** inside the required-merge-gate ` ```sh ` fence in every mirror that lists the full gate.
  - Mirrors that must match: `docs/05-testing.md`, `README.md` (already enforced pair), plus `AGENT.md` (duplicates same list for agents).
  - `validation_contract.rs` scans `docs/05-testing.md` + `README.md` only for `required_gate_*` — both must contain the exact command.
  - Legacy history: prose must say only commits after trusted base are checked.

## Requirements

### Constant + test in `tests/validation_contract.rs`

1. After existing `AUDIO_GATE_COMMAND` (near line 82), add:
   ```rust
   /// Offline DCO range gate (audit F11 / issue #10). Placeholders are literal
   /// tokens in the doc fence; maintainer substitutes real SHAs at run time.
   const DCO_GATE_COMMAND: &str =
       "./scripts/check-dco <trusted-base-sha> <exact-candidate-sha>";
   ```
2. After `required_gate_contains_audio_check`, add test:
   ```rust
   /// DCO range check stays on the required gate in both doc mirrors.
   #[test]
   fn required_gate_contains_dco_check() {
       for rel in [CONTRACT_DOC, README_DOC] {
           let gate = gate_section(rel);
           assert!(
               gate.commands.iter().any(|c| c == DCO_GATE_COMMAND),
               "{rel} '{GATE_HEADING}' does not list `{DCO_GATE_COMMAND}`; \
                unsigned candidate commits could pass every other gate command"
           );
       }
   }
   ```
3. Do **not** add DCO command to `REQUIRED_COMMANDS` array (that array is the pre-existing minimal spine). Dedicated test is enough — same pattern as `AUDIO_GATE_COMMAND`.

### `docs/05-testing.md` — Required merge gate

Current fence (starts ~line 66):

```sh
cargo fmt --all -- --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --all-features -- -D warnings
nix flake check
cargo run -p xtask -- bootstrap --check
cargo run -p xtask -- shaders --check
cargo run -p xtask -- atlases --check
cargo run -p xtask -- audio --check
cargo run -- run --agents 5000 --frames 300
cargo run -- run --scenario assets/scenarios/collision_mid_v1.ron --frames 300
cargo run -- run --scenario assets/scenarios/collision_sprite_v1.ron --frames 300
cargo run -- rts --frames 1600 --inject-input-file assets/scenarios/rts_acceptance_v1.script
```

**Edit fence** so first command line is exactly:

```sh
./scripts/check-dco <trusted-base-sha> <exact-candidate-sha>
cargo fmt --all -- --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --all-features -- -D warnings
nix flake check
cargo run -p xtask -- bootstrap --check
cargo run -p xtask -- shaders --check
cargo run -p xtask -- atlases --check
cargo run -p xtask -- audio --check
cargo run -- run --agents 5000 --frames 300
cargo run -- run --scenario assets/scenarios/collision_mid_v1.ron --frames 300
cargo run -- run --scenario assets/scenarios/collision_sprite_v1.ron --frames 300
cargo run -- rts --frames 1600 --inject-input-file assets/scenarios/rts_acceptance_v1.script
```

**Add prose** immediately after the closing ` ``` ` of that fence (before the existing paragraph that starts “The last command is the interactive RTS smoke”). Insert new paragraph(s):

```markdown
`./scripts/check-dco <trusted-base-sha> <exact-candidate-sha>` is the offline
DCO range gate. Maintainer sets `trusted-base-sha` to a commit already on
trusted `main` that is an ancestor of the candidate (typical:
`git merge-base main <exact-candidate-sha>`) and `exact-candidate-sha` to the
**exact** commit under test. The checker walks only
`trusted-base-sha..exact-candidate-sha` (base exclusive, candidate inclusive):
every commit in that range must carry a valid `Signed-off-by: Name <email>`
trailer (`git interpret-trailers --parse`, value must include `<email@domain>`).
Missing trailers exit nonzero and print each offending full hash. A candidate
that is not a descendant of the trusted base exits nonzero. Equal base and
candidate (empty range) exits zero. Legacy history on `main` is **not**
revalidated and must not be rewritten to satisfy this gate.
`required_gate_contains_dco_check` keeps the command on this list.
```

Keep existing audio/smoke prose intact. No perf tokens.

### `README.md` — Required merge gate mirror

Same fence edit under `### Required merge gate` (~line 53): insert
`./scripts/check-dco <trusted-base-sha> <exact-candidate-sha>` as **first** fence line. No long prose required in README (SSoT is testing doc); optional one-line note under fence is OK but not required.

### `AGENT.md` — Build / test merge gate block

Same first-line insert in the merge-gate ` ```sh ` block (~line 66). Optionally append one short bullet after the block:

```markdown
- DCO: `./scripts/check-dco <trusted-base-sha> <exact-candidate-sha>` on candidate range only; see `docs/05-testing.md`.
```

Do not claim AGENT is SSoT — existing text already points at `docs/05-testing.md`.

### `CONTRIBUTING.md`

1. Under `## Developer Certificate of Origin (DCO) 1.1`, after the sentence `Use \`git commit -s\` (or equivalent). Sign-off certifies:` **or** after the DCO license block closing fence — add short enforcement note **before** `## Pull requests`:

```markdown
Maintainer enforces sign-off on the candidate commit range with the offline
checker documented in the [required merge gate](docs/05-testing.md#required-merge-gate):

```sh
./scripts/check-dco <trusted-base-sha> <exact-candidate-sha>
```

Only commits after the trusted base are checked. Do not rewrite legacy `main`
history for DCO.
```

2. In `## Pull requests` step 3 (exact-commit / full gate sentence), ensure it still points at required merge gate (already does). Optionally tighten to:

```markdown
3. Maintainer reviews, fetches the **exact** commit, runs the full [required merge gate](docs/05-testing.md#required-merge-gate) locally (including `./scripts/check-dco` on trusted-base→exact-candidate), posts a concise pass summary, and merges **only** that tested hash.
```

Use that tightened step 3 wording.

### Non-goals for this ticket

- Do not add GitHub workflow files.
- Do not amend/rebase existing main commits.
- Do not change `scripts/check-dco` behavior except bugfix if T1 tests fail after doc work (should not).
- Do not edit ADR 008 (policy already accepted).

## Inputs

- From T1 (must exist before starting):
  - `scripts/check-dco` — executable offline checker; args `<trusted-base-sha> <exact-candidate-sha>`; exit 0 signed/empty; exit 1 missing SOB or non-descendant; exit 2 usage/bad rev; stderr `missing Signed-off-by: <fullsha>` per offender.
  - `tests/dco_range_gate.rs` — behavioral coverage green via `cargo test --locked --test dco_range_gate`.
- Edit targets:
  - `tests/validation_contract.rs` (`AUDIO_GATE_COMMAND` neighborhood; after `required_gate_contains_audio_check`)
  - `docs/05-testing.md` (`## Required merge gate` fence + following prose)
  - `README.md` (`### Required merge gate` fence)
  - `AGENT.md` (merge gate command block)
  - `CONTRIBUTING.md` (DCO section + PR step 3)
- **From Depends:** paths/behavior above; worker must not re-read T1 file if missing — if script absent, stop and restore T1 first.

## TDD

1. **Red** — add `DCO_GATE_COMMAND` + `required_gate_contains_dco_check` only. Run:
   ```sh
   cargo test --locked --test validation_contract required_gate_contains_dco_check
   ```
   Expect **FAIL** on both `docs/05-testing.md` and `README.md` (command absent).
2. **Green** — edit the three fences + CONTRIBUTING prose as specified. Re-run test → PASS.
3. **Refactor** — none expected.

## Test plan

| Test | Input | Expect |
| --- | --- | --- |
| `required_gate_contains_dco_check` (new) | gate sections of `docs/05-testing.md`, `README.md` | each `gate.commands` contains exact `DCO_GATE_COMMAND` string after whitespace normalize |
| `required_gate_contains_audio_check` (existing) | same | still pass |
| `required_gate_keeps_phase_smokes` (existing) | same | still pass |
| `gate_list_has_no_perf_thresholds` (existing) | same | still pass (DCO prose/cmd introduce no perf tokens) |
| full `dco_range_gate` (from T1) | script | still pass |

## Impl steps

- [ ] 1. Add `DCO_GATE_COMMAND` + `required_gate_contains_dco_check` to `tests/validation_contract.rs`. → verify red: `cargo test --locked --test validation_contract required_gate_contains_dco_check` fails.
- [ ] 2. Insert `./scripts/check-dco <trusted-base-sha> <exact-candidate-sha>` as first fence line in `docs/05-testing.md` Required merge gate. → verify string present.
- [ ] 3. Insert DCO prose paragraph after that fence in `docs/05-testing.md` (wording under Requirements). → verify mentions exclusive range, no legacy rewrite, `required_gate_contains_dco_check`.
- [ ] 4. Insert same first fence line in `README.md` Required merge gate. → verify.
- [ ] 5. Insert same first fence line in `AGENT.md` merge-gate block; optional one-line DCO bullet. → verify.
- [ ] 6. Edit `CONTRIBUTING.md` DCO enforcement note + PR step 3 per Requirements. → verify links to `docs/05-testing.md#required-merge-gate` and mentions `./scripts/check-dco`.
- [ ] 7. Run:
   ```sh
   cargo test --locked --test validation_contract required_gate_contains_dco_check
   cargo test --locked --test validation_contract required_gate_
   cargo test --locked --test dco_range_gate
   cargo test --workspace --locked
   ```
   → verify all pass.
- [ ] 8. Sanity: `rg -n "check-dco" docs/05-testing.md README.md AGENT.md CONTRIBUTING.md tests/validation_contract.rs` shows expected hits; no `.github/workflows` added; `git log -1 --oneline` not rewritten. → verify.

## Outputs

- Modified: `tests/validation_contract.rs`, `docs/05-testing.md`, `README.md`, `AGENT.md`, `CONTRIBUTING.md`
- Unchanged behavior of `scripts/check-dco` (T1)
- Public/docs behavior: merge gate requires offline DCO range check with two SHA args
- No migration

## Validation

- [ ] `cargo test --locked --test validation_contract required_gate_contains_dco_check` — pass
- [ ] `cargo test --locked --test validation_contract required_gate_` — pass (audio + smokes + dco)
- [ ] `cargo test --locked --test dco_range_gate` — pass
- [ ] `cargo test --workspace --locked` — pass
- [ ] `rg -n '^\./scripts/check-dco <trusted-base-sha> <exact-candidate-sha>$' docs/05-testing.md README.md AGENT.md` — 3 hits
- [ ] manual read: CONTRIBUTING states range-only enforcement + no legacy rewrite
- [ ] app functional — no game code touched
- [ ] commit msg draft: `docs(ops): require offline DCO range check in merge gate`
