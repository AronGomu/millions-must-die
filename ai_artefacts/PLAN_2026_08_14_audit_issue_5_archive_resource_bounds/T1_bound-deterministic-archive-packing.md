# T1: Bound deterministic archive packing

**Plan:** `./ai_artefacts/PLAN_2026_08_14_audit_issue_5_archive_resource_bounds.md`
**Depends:** none
**Commit outcome:** Every prod `mmd-lab` archive pack uses fixed ceilings, iterative traversal, bounded reads/reserves, stable errors, full tiny-limit boundary coverage.

## Context (self-contained)

- Goal: Fix medium-severity audit finding `4d34cb0fdf5b5559`. Candidate trees can currently exhaust trusted coordinator stack/heap.
- Evidence: `tools/mmd-lab/src/archive.rs:174-215` recursively walks without depth/count caps, calls `fs::read`, retains every file, then copies all data into encoded blob. `tools/mmd-lab/src/ssh.rs:69-82` clones blob for fake delivery; `verify_bytes_hash` clones verified bytes again.
- Current clean-tree measurement using existing skip set: depth `5`; visited entries `481`; files `408`; largest file `1_536_044 B`; decoded `7_247_432 B`; metadata/format overhead `21_844 B`; final blob `7_269_276 B`.
- This slice: Bound pack/build resource use without changing archive bytes for accepted inputs.
- Out of scope: F5 numeric validation; transport changes; SSH work; `parse_archive` hardening; new format/version; compression/streaming rewrite; user-set limits; live large payload; unrelated cleanup.
- Assumptions in force: Supported hosts are 64-bit. Existing `MMDARC01` layout stays exact. Skipped entries plus symlinks count as visited because coordinator still enumerates them. Root depth is `0`; direct child depth is `1`.
- No ADR needed: operational ceiling, no format/topology decision. No manual checklist needed: headless deterministic unit coverage.

## Requirements

### Fixed prod contract

Add in `tools/mmd-lab/src/archive.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArchiveLimits {
    pub max_depth: usize,
    pub max_visited_entries: usize,
    pub max_files: usize,
    pub max_file_bytes: u64,
    pub max_decoded_bytes: u64,
    pub max_encoded_bytes: u64,
}

pub const PRODUCTION_ARCHIVE_LIMITS: ArchiveLimits = ArchiveLimits {
    max_depth: 32,
    max_visited_entries: 16_384,
    max_files: 8_192,
    max_file_bytes: 16 * 1024 * 1024,
    max_decoded_bytes: 64 * 1024 * 1024,
    max_encoded_bytes: 80 * 1024 * 1024,
};
```

- `ArchiveLimits` + `PRODUCTION_ARCHIVE_LIMITS`: `pub` inside private binary module. Tests/other crate modules may inspect contract. No CLI flags/env/config.
- Keep public signatures unchanged:

```rust
pub fn build_archive(entries: Vec<ArchiveEntry>) -> Result<ArchiveBlob, ArchiveError>
pub fn pack_tree(root: &Path) -> Result<ArchiveBlob, ArchiveError>
```

- Both public fns call prod-limit seams. Direct `build_archive` must enforce file/data/final limits too.
- Add private test seams used by prod wrappers:

```rust
fn build_archive_with_limits(
    entries: Vec<ArchiveEntry>,
    limits: ArchiveLimits,
) -> Result<ArchiveBlob, ArchiveError>

fn pack_tree_with_limits(
    root: &Path,
    limits: ArchiveLimits,
) -> Result<ArchiveBlob, ArchiveError>

fn pack_tree_with_limits_and_hook<F>(
    root: &Path,
    limits: ArchiveLimits,
    before_file_read: &mut F,
) -> Result<ArchiveBlob, ArchiveError>
where
    F: FnMut(&Path) -> io::Result<()>;
```

- `pack_tree` → `pack_tree_with_limits(root, PRODUCTION_ARCHIVE_LIMITS)`.
- `pack_tree_with_limits` → hook seam with typed no-op closure.
- Hook runs after initial `symlink_metadata` size check, immediately before `File::open` + bounded read. Only growth test injects mutation. No `#[cfg(test)]` branch in prod algorithm.

### Typed stable failures

Extend `ArchiveError`; preserve existing `Io` + `Msg`. Add exact variants/displays:

```rust
#[error("archive limit exceeded: max_depth actual={actual} limit={limit} path={path}")]
DepthLimitExceeded { actual: usize, limit: usize, path: String },
#[error("archive limit exceeded: max_visited_entries actual={actual} limit={limit}")]
VisitedEntriesLimitExceeded { actual: usize, limit: usize },
#[error("archive limit exceeded: max_files actual={actual} limit={limit}")]
FileCountLimitExceeded { actual: usize, limit: usize },
#[error("archive limit exceeded: max_file_bytes actual={actual} limit={limit} path={path}")]
FileSizeLimitExceeded { actual: u64, limit: u64, path: String },
#[error("archive limit exceeded: max_decoded_bytes actual={actual} limit={limit} path={path}")]
DecodedSizeLimitExceeded { actual: u64, limit: u64, path: String },
#[error("archive limit exceeded: max_encoded_bytes actual={actual} limit={limit}")]
EncodedSizeLimitExceeded { actual: u64, limit: u64 },
#[error("archive size overflow: {context}")]
SizeOverflow { context: &'static str },
#[error("archive allocation failed: {context} requested={requested}")]
AllocationFailed { context: &'static str, requested: usize },
```

- Unit tests match variants + all fields; also assert exact `to_string()` for each resource class.
- Resource errors expose normalized archive-relative `/` paths, never canonical absolute root.
- `actual` for streaming limit failures is bounded observation (`limit + 1`), not claimed full file size.
- If one read crosses per-file + aggregate limit simultaneously, `FileSizeLimitExceeded` wins. Stable priority.
- Existing filesystem `Io` text remains OS-defined; docs promise stable resource messages only.
- Existing CLI prefixes/exits remain: `archive` → stderr `archive failed: {ArchiveError}`, exit `1`; `validate`/`validate-runner` → `... archive failed: {ArchiveError}`, exit `3`.

### Bounded deterministic traversal

- Replace recursive `collect_entries` with explicit LIFO `Vec<(PathBuf, usize)>`; root frame depth `0`.
- Stream each `fs::read_dir` iterator. Increment checked global visited count for every successful entry before skip/symlink filtering. Reject attempted `limit + 1` immediately. This bounds per-dir collection even when names are skipped.
- Use `try_reserve(1)` before pushing child paths, stack frames, archive entries. Map failure to `AllocationFailed`; never call `reserve`/`with_capacity` on candidate-derived counts.
- Sort each bounded child vector ascending. Push in reverse → deterministic ascending DFS. Final builder still sorts entry paths, preserving archive bytes.
- Apply depth after skip-name filtering; symlinks stay skipped. Empty dirs at depth `limit` pass; any non-skipped dir/file at depth `limit + 1` fails.
- Check file count before file read. Use checked increments only.
- Preserve existing skip set, UTF-8/path validation, symlink behavior, duplicate rejection, hash, format.

### Bounded race-safe file IO

- Initial `symlink_metadata().len()` may fail early but is not trusted as final size.
- Remaining aggregate budget = `max_decoded_bytes - decoded_so_far` via checked arithmetic.
- Read cap = `min(max_file_bytes, remaining_aggregate) + 1`, using checked add.
- Open file after hook. Read through `Read::take(read_cap)` in fixed `64 KiB` stack chunks.
- Before each `Vec<u8>::extend_from_slice`, call `try_reserve(chunk_len)` and map error. Never use `fs::read`/unbounded `read_to_end`.
- After bounded EOF/cap: test per-file first, aggregate second; update aggregate with `checked_add` only; push entry only after all checks.
- File growth after metadata can consume at most active budget + one byte. Shrink is accepted using bytes actually read.

### Bounded encoding

- Before output allocation/copy, validate file count, each file size, cumulative decoded bytes.
- Compute exact final size using checked arithmetic: `12` byte header + each entry's `4 + path_bytes + 8 + data_bytes`.
- Reject exact computed size `> max_encoded_bytes` before output allocation. Size equal to limit passes.
- Convert final `u64` to `usize` with checked conversion; map failure to `SizeOverflow { context: "archive encoded length" }`.
- `Vec::new()` then `try_reserve_exact(final_len)` once; map failure to `AllocationFailed { context: "archive bytes", requested: final_len }`. Copy only after reserve succeeds.
- Use exact private helpers in prod paths; tests call them with synthetic lengths:

```rust
fn checked_encoded_add(total: u64, amount: u64) -> Result<u64, ArchiveError>
fn reserve_additional<T>(
    values: &mut Vec<T>,
    additional: usize,
    context: &'static str,
) -> Result<(), ArchiveError>
fn reserve_exact<T>(
    values: &mut Vec<T>,
    additional: usize,
    context: &'static str,
) -> Result<(), ArchiveError>
```

- `checked_encoded_add` always uses `SizeOverflow { context: "archive encoded length" }`.
- Reserve contexts fixed: `"directory children"`, `"directory stack"`, `"archive entries"`, `"file data"`, `"archive bytes"`. `reserve_additional` wraps `try_reserve`; `reserve_exact` wraps `try_reserve_exact`.
- No huge allocation. Valid input blob/hash must remain byte-identical to baseline.

### Memory rationale

- Current blob `7_269_276 B`; max file `1_536_044 B`. Chosen caps provide 9×–11× byte headroom.
- Pack peak contract: retained decoded data `≤64 MiB` + encoded blob `≤80 MiB` + bounded path/entry metadata.
- Transport evidence: `FakeAgent::deliver_and_collect` makes one `80 MiB` remote clone; `verify_bytes_hash` can make one second `80 MiB` clone. Conservative combined accounting: `64 + 3×80 = 304 MiB`; actual source vec drops before transport.
- Keep final `80 MiB`, not `64 MiB`, because paths + `12 B` per-file framing count beyond decoded bytes. Final cap independently stops metadata overhead.

## Inputs

- `tools/mmd-lab/src/archive.rs`: all prod code + unit tests touched.
- `tools/mmd-lab/src/ssh.rs:69-82`: read-only clone evidence; do not edit.
- `tools/mmd-lab/src/main.rs:1585-1602,1865-1871,2305-2311,2444-2450,2586-2592`: existing `pack_tree` callers; unchanged API means no edits.
- `docs/lab/local-validation.md:68-75`: archive contract docs touched.
- `schemas/lab-archive-v1.schema.json`: read-only. Logical format unchanged; no schema edit.
- **From Depends:** none.

## TDD

1. **Seam-first compile setup** — add `ArchiveLimits`, prod const, typed variants, private seam signatures, hook plumbing; move legacy bodies behind seams without limits yet. Route both public fns through prod const. Run `cargo check -p mmd-lab --locked`; expect exit `0`. This avoids red-test compile failures.
2. **Red** — add/update exact 14-test archive module below. Keep fixtures ≤ `64 B`; use sparse-free temp files. Run focused cmd; expect exit `101`, no compile error, new limit assertions failing against legacy unbounded behavior.
3. **Green** — add iterative traversal, bounded read, checked math, `try_reserve`, exact errors. Run focused cmd; expect `running 14 tests`, `14 passed; 0 failed`.
4. **Refactor** — remove recursive `collect_entries`; keep helpers private/single-use; preserve accepted bytes. Run focused + crate validation.
5. **Docs** — publish fixed ceilings, counting rules, memory envelope, stable resource errors, unchanged CLI exits in `docs/lab/local-validation.md`. No manual checklist.

## Test plan

All tests live in `tools/mmd-lab/src/archive.rs::tests`. Existing test count `3`; final count exactly `14` (`3` retained/updated + `11` new).

| Test | Tiny input | Exact expect |
| --- | --- | --- |
| `deterministic_hash_same_entries` | Existing reversed `a.txt`/`b.txt` entries | Same bytes/hash; parsed order `a.txt`, `b.txt`; now passes bounded public `build_archive` |
| `hash_mismatch_detected` | Existing one-byte blob | Existing hash mismatch behavior unchanged |
| `pack_tree_roundtrip` | Existing `hello.txt`, `sub/x.txt`, skipped `target`; pack twice | Two blobs equal; parsed paths exactly `['hello.txt', 'sub/x.txt']`; data exact; no skipped path |
| `depth_limit_accepts_limit_and_rejects_limit_plus_one` | `max_depth=2`; empty dir at depth 2, then depth 3 | First pass; second `DepthLimitExceeded { actual: 3, limit: 2, path: "a/b/c" }`; exact display |
| `visited_entry_limit_accepts_limit_and_rejects_limit_plus_one` | Limit 2; two then three zero-byte root files; all other caps high | Two pass; third `VisitedEntriesLimitExceeded { actual: 3, limit: 2 }`; skipped-name entry also counts via second assertion/case |
| `file_count_limit_accepts_limit_and_rejects_limit_plus_one` | File limit 2; two then three zero-byte files | Two pass; third `FileCountLimitExceeded { actual: 3, limit: 2 }`; exact display |
| `per_file_limit_accepts_limit_and_rejects_limit_plus_one` | Limit 4; one file 4 B then 5 B | 4 passes; 5 returns `FileSizeLimitExceeded { actual: 5, limit: 4, path: "x" }`; exact display |
| `decoded_limit_accepts_limit_and_rejects_limit_plus_one` | Aggregate 4; `a` 2 B + `b` 2 B, then `b` 3 B; per-file cap 8 | 4 passes; 5 returns `DecodedSizeLimitExceeded { actual: 5, limit: 4, path: "b" }`; exact display |
| `final_encoded_limit_accepts_limit_and_rejects_limit_plus_one` | `build_archive_with_limits([path="a", data=[]])`; exact encoded len 25 | Max 25 passes with 25-byte blob; max 24 returns `EncodedSizeLimitExceeded { actual: 25, limit: 24 }`; exact display |
| `metadata_overhead_counts_toward_final_encoded_limit` | One zero-byte `a`; decoded max 0; encoded max 24 | Decoded bound passes; 25-byte framing/path total fails encoded variant. Proves aggregate overhead accounting |
| `growth_after_metadata_is_stopped_at_limit_plus_one` | File starts 4 B; hook appends 1 B after metadata; file/aggregate max 4 | Hook runs once; bounded read returns file-size variant at observed 5 B; no unbounded read/allocation |
| `deterministic_first_failure_follows_sorted_order` | Oversized `z/bad` + `a/bad`; pack twice | Both errors equal; path always `a/bad`. Proves sorted iterative order/error selection |
| `encoded_size_overflow_is_bounded_archive_error` | `checked_encoded_add(u64::MAX, 1)` | `SizeOverflow { context: "archive encoded length" }`; no payload allocation |
| `reservation_failure_is_bounded_archive_error` | `reserve_additional(&mut Vec::<u8>::new(), usize::MAX, "archive bytes")` | `AllocationFailed { context: "archive bytes", requested: usize::MAX }`; exact display; no giant allocation/panic |

## Impl steps

- [ ] 1. In `tools/mmd-lab/src/archive.rs`, import `std::fs::File` + `std::io::Read`; add exact `ArchiveLimits` + `PRODUCTION_ARCHIVE_LIMITS` declarations after existing constants.
- [ ] 2. Add exact eight `ArchiveError` variants/displays; retain `Io` + `Msg` behavior.
- [ ] 3. Add private `build_archive_with_limits`; route unchanged `build_archive` through prod const.
- [ ] 4. Add private `pack_tree_with_limits` + `pack_tree_with_limits_and_hook`; route unchanged `pack_tree` through prod const.
- [ ] 5. Run seam-first `cargo check -p mmd-lab --locked`; fix only compile issues in touched file.
- [ ] 6. Add/update exact 14 unit tests from test table using `tempfile`; add one test helper returning tiny `ArchiveLimits` with each test overriding only named bound.
- [ ] 7. Run red focused test cmd; record exit `101`; confirm failures are missing enforcement, not compile/setup faults.
- [ ] 8. Replace recursive `collect_entries` with bounded sorted iterative DFS; count every enumerated entry; use checked counters + `try_reserve`.
- [ ] 9. Add archive-relative path helper preserving `/` normalization; use it in resource path fields.
- [ ] 10. Add bounded file-read helper using initial metadata check, test hook, `File::open`, `Read::take(limit+1)`, fixed `64 KiB` buffer, per-chunk `try_reserve`.
- [ ] 11. Add checked encoded-size preflight; validate direct entries; reserve exact output with `try_reserve_exact` before copying.
- [ ] 12. Remove recursive `collect_entries`; verify no remaining `fs::read(&path)` or candidate-sized `Vec::with_capacity`/`reserve` in pack/build path.
- [ ] 13. Update `docs/lab/local-validation.md` `## Archive`: exact defaults/table, counting/depth definitions, 304 MiB conservative clone accounting, stable error form, unchanged exit semantics, no override.
- [ ] 14. Run focused tests, formatter, crate tests, clippy. Fix only issue-scope failures.
- [ ] 15. Inspect `git diff --check` + scoped diff; ensure only three requested files changed.
- [ ] 16. Commit implementation as `fix(lab): bound candidate archive resources` with DCO sign-off when implementation job requests commit.

## Outputs

- `tools/mmd-lab/src/archive.rs`: bounded prod defaults, iterative traversal, bounded IO/encoding, stable errors, 14 archive tests.
- `docs/lab/local-validation.md`: operator contract.
- Public behavior: unchanged archive bytes/hash under limits; deterministic rejection above limits.
- Public API: unchanged fn signatures; new `ArchiveLimits`, `PRODUCTION_ARCHIVE_LIMITS`, resource variants. Private tiny-limit/hook seams only.
- No deps, schema migration, CLI flags, format version, manual checklist.

## Validation

- [ ] Compile-green seam: `cargo check -p mmd-lab --locked` → exit `0`; no warnings/errors.
- [ ] Focused: `cargo test -p mmd-lab archive::tests --locked` → exit `0`; `running 14 tests`; `14 passed; 0 failed`.
- [ ] Crate: `cargo test -p mmd-lab --locked` → exit `0`; all suites report `0 failed`; archive module contributes exactly 14 passing tests.
- [ ] Format: `cargo fmt --all -- --check` → exit `0`; no diff.
- [ ] Lint: `cargo clippy -p mmd-lab --all-targets --all-features --locked -- -D warnings` → exit `0`; no diagnostics.
- [ ] Static bound check: `rg -n 'collect_entries|fs::read\(&path\)|read_to_end' tools/mmd-lab/src/archive.rs` → exit `1`, no matches.
- [ ] Reserve audit: `rg -n 'Vec::with_capacity|\.reserve\(' tools/mmd-lab/src/archive.rs` → one existing `parse_archive` `Vec::with_capacity(count)` match only; parser hardening remains explicit scope-out. No pack/build match.
- [ ] Diff hygiene: `git diff --check` → exit `0`.
- [ ] Scope: `git diff --name-only` → exactly `docs/lab/local-validation.md`, `tools/mmd-lab/src/archive.rs` during impl; plan artifacts absent from impl branch if tickets executed separately.
- [ ] App functional: existing `archive`, `validate`, `validate-runner` compile unchanged; accepted-tree archive hash remains deterministic.
- [ ] Manual check: not applicable; no UI/live transport.
- [ ] Commit msg draft: `fix(lab): bound candidate archive resources`

## Acceptance boxes

- [ ] Depth `32`, visited `16_384`, files `8_192`, file `16 MiB`, decoded `64 MiB`, encoded `80 MiB` fixed in one prod const.
- [ ] Every prod `build_archive`/`pack_tree` path uses injected-limit seam.
- [ ] Traversal iterative + bounded; no recursion proof needed.
- [ ] Reads consume at most active bound + 1 despite metadata race.
- [ ] Every candidate-sized growth uses checked math + `try_reserve`.
- [ ] Exact limit passes; limit + 1 fails for all six resource classes.
- [ ] Framing/path overhead independently bounded by final encoded cap.
- [ ] Deterministic valid order/hash/roundtrip retained; deterministic first resource error proven.
- [ ] No test allocates payload > `64 B`; overflow/reserve faults synthetic.
- [ ] Docs state ceilings, semantics, clone envelope, errors/exits.

## Residual risks

- `parse_archive` still trusts encoded count/lengths enough to allocate decoded entries. Dormant `#[allow(dead_code)]`; parser/extractor hardening explicitly out of F6 pack scope. Must be bounded before future untrusted extraction exposure.
- `verify_bytes_hash` still clones input. Prod pack cap bounds it; arbitrary future callers must uphold same cap or refactor transport/verification.
- Filesystem path replacement between `symlink_metadata` + `File::open` remains existing TOCTOU class. Bounded read prevents size exhaustion; symlink-race containment is separate scope.
- `try_reserve` prevents allocator abort on explicit vector growth; Rust/std/path/OS internals may still allocate small bounded metadata.
