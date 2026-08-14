# T1: Bound deterministic archive packing

**Plan:** `./ai_artefacts/PLAN_2026_08_14_audit_issue_5_archive_resource_bounds.md`
**Depends:** none
**Commit outcome:** Every prod `mmd-lab` archive pack uses fixed ceilings, iterative traversal, bounded reads/reserves, stable errors, fixed-format compatibility, full tiny-limit boundary coverage.

## Context (self-contained)

- Goal: Fix medium-severity audit finding `4d34cb0fdf5b5559`. Candidate trees can currently exhaust trusted coordinator stack/heap.
- Evidence: pre-impl `tools/mmd-lab/src/archive.rs:174-215` recursively walks without depth/count caps, calls `fs::read`, retains every file, then copies all data into encoded blob. `tools/mmd-lab/src/ssh.rs:69-82` clones blob for fake delivery; `verify_bytes_hash` clones verified bytes again.
- Exact measurement source: filesystem checkout at plan commit `994d292f7c01a314d16e01302377daf64a96904a`; depth `5`; visited entries `488` under proposed pre-filter rule, including enumerated `.git`, `.pi-subagents`, `.tmp`, `target`; files `410`; largest file `1_536_044 B`; decoded `7_269_289 B`; metadata/format overhead `22_045 B`; final blob `7_291_334 B`.
- This slice: Bound pack/build resource use without changing archive bytes for accepted inputs.
- Out of scope: F5 numeric validation; transport changes; SSH work; `parse_archive` hardening; new format/version; compression/streaming rewrite; user-set limits; live large payload; unrelated cleanup.
- Assumptions in force: Supported hosts are 64-bit. Existing `MMDARC01` layout stays exact. Skipped entries + symlinks count as visited because coordinator enumerates them. Root depth is `0`; direct child depth is `1`.
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

- Both public fns call prod-limit seams. Direct `build_archive` enforces file/data/final limits independently of traversal.
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
- Hook runs after initial `symlink_metadata` + metadata-size checks, immediately before `File::open` + bounded read. Growth test mutates only selected file. No `#[cfg(test)]` branch in prod algorithm.

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

- Unit tests match variants + all fields; assert exact `to_string()` for every resource class.
- Resource errors expose normalized archive-relative `/` paths, never canonical absolute root.
- `actual` for streaming failures is bounded observation (`limit + 1`), not claimed full file size.
- If read crosses per-file + aggregate simultaneously, `FileSizeLimitExceeded` wins.
- Existing filesystem `Io` text remains OS-defined; docs promise stable resource messages only.
- Existing CLI prefixes/exits remain: `archive` → stderr `archive failed: {ArchiveError}`, exit `1`; `validate`/`validate-runner` → `... archive failed: {ArchiveError}`, exit `3`.

### Seam-first compile contract

Before adding red tests, add working low-level helpers, not stubs:

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

- `checked_encoded_add` uses `checked_add`, always maps overflow to `SizeOverflow { context: "archive encoded length" }`.
- `reserve_additional` wraps `try_reserve`; `reserve_exact` wraps `try_reserve_exact`; both map failure to `AllocationFailed { context, requested: additional }`.
- Reserve contexts fixed: `"directory children"`, `"directory stack"`, `"archive entries"`, `"file data"`, `"archive bytes"`.
- Seam-first setup also adds limits/errors/build/pack/hook seams, routes public wrappers through prod const, then leaves legacy unbounded behavior behind seams. Helper/legacy tests compile + pass; behavioral limit tests added next stay red.

### Bounded deterministic traversal

- Replace recursive `collect_entries` with explicit LIFO `Vec<(PathBuf, usize)>`; root frame depth `0`.
- Stream each `fs::read_dir` iterator. Increment checked global visited count for every successful entry before skip/symlink filtering. Reject attempted `limit + 1` immediately. Ordinary dirs, skipped names, files, symlinks all consume budget; symlink target is never traversed.
- Use `reserve_additional(..., 1, ...)` before pushing child paths, stack frames, archive entries. Never call `reserve`/`with_capacity` on candidate-derived counts.
- Sort each bounded child vec ascending. Push in reverse → deterministic ascending DFS. Final builder sorts entry paths, preserving bytes.
- Apply depth after skip-name filtering but before file/dir handling. Empty dir at depth `limit` passes; any non-skipped dir/file at depth `limit + 1` fails with exact normalized relative path. Symlinks remain skipped after consuming visited budget.
- Check file count before hook/open/read. Use checked increments only.
- Preserve skip set, UTF-8/path validation, symlink behavior, duplicate rejection, hash, format.

### Bounded race-safe file IO

- Initial `symlink_metadata().len()` may reject early but is not trusted as final size.
- Remaining aggregate budget = `max_decoded_bytes - decoded_so_far` via checked arithmetic.
- Read cap = `min(max_file_bytes, remaining_aggregate) + 1`, using checked add.
- Open file after hook. Read through `Read::take(read_cap)` in fixed `64 KiB` stack chunks.
- Before each `Vec<u8>::extend_from_slice`, call `reserve_additional(..., chunk_len, "file data")`. Never use `fs::read`/unbounded `read_to_end`.
- After bounded EOF/cap: test per-file first, aggregate second; update aggregate with `checked_add`; push entry only after all checks.
- Growth after metadata consumes at most active budget + one byte. Shrink uses bytes actually read.

### Exact bounded builder order

`build_archive_with_limits` performs these phases in exact order:

1. **Count:** reject `entries.len() > max_files` + representation overflow before iteration/sort. `FileCountLimitExceeded.actual` is attempted count.
2. **Checked preflight in input order:** validate each path; check path/data format conversions; reject per-file overflow; checked-add cumulative decoded bytes; checked-add exact encoded length from `12` byte header + each `4 + path_bytes + 8 + data_bytes`; apply per-file → decoded → encoded error priority. No output allocation/copy.
3. **Sort:** sort entries by path only after bounded count/byte preflight; reject duplicate adjacent paths.
4. **Bounded reserve/copy:** checked-convert exact final `u64` to `usize`, mapping failure to `SizeOverflow { context: "archive encoded length" }`; `Vec::new()` then `reserve_exact(..., final_len, "archive bytes")` once; copy framing/path/data only after successful reserve.

- Exact encoded size equal to `max_encoded_bytes` passes; size `limit + 1` fails before sort/output allocation.
- Direct builder tests independently prove count, per-file, decoded, encoded at limit + limit+1. Pack tests independently prove early collection/read enforcement.
- No huge allocation. Fixed tiny vector must remain byte-identical to pre-change format.

### Memory rationale + docs wording

- Fixed caps bound retained decoded payload to `64 MiB`, encoded blob to `80 MiB`, packed files to `8_192`, visited entries to `16_384`, depth to `32`.
- `64 MiB + 3 × 80 MiB = 304 MiB` is only major byte-buffer subtotal across pack/fake-transport/hash clone paths. Never call it exact total peak or process RAM cap.
- Path/entry metadata is structurally bounded by visited/file/depth caps + host filesystem path/name limits, not separately byte-capped. Allocator capacity, hashing state, std/OS metadata add overhead. Docs state this honest envelope.
- Final `80 MiB` remains independent from `64 MiB` because paths + `12 B` per-entry framing exceed decoded payload.

## Inputs

- `tools/mmd-lab/src/archive.rs`: all prod code + unit tests touched.
- `tools/mmd-lab/src/ssh.rs:69-82`: read-only clone evidence; do not edit.
- `tools/mmd-lab/src/main.rs:1585-1602,1865-1871,2305-2311,2444-2450,2586-2592`: existing callers; unchanged API means no edits.
- `docs/lab/local-validation.md:68-75`: archive contract docs touched.
- `schemas/lab-archive-v1.schema.json`: read-only. Format unchanged; no schema edit.
- **From Depends:** none.

## TDD

1. **Seam-first compile setup** — add `ArchiveLimits`, prod const, typed variants, three fully working low-level helpers, private build/pack/hook seams; route public wrappers; move legacy bodies behind seams without behavioral limits. Run `cargo check -p mmd-lab --locked`; expect exit `0`, no warnings/errors.
2. **Red** — add/update exact 17-test archive module below. Keep materialized fixtures ≤ `64 B`; use sparse-free temp files. Run literal cmd `cargo test -p mmd-lab archive::tests --locked`; expect exit `101`, `running 17 tests`, summary `FAILED. 5 passed; 12 failed; 0 ignored; 0 measured; 136 filtered out`. Passing: three legacy regressions + overflow helper + dual-reserve helper. Failing from missing behavioral enforcement: depth, visited, four direct-builder limits, three pack limits, growth, metadata overhead, deterministic first failure. Any compile error or different pass/fail count → stop + repair seam/tests before Green.
3. **Green** — add exact traversal/read/builder enforcement. Run same focused cmd; expect exit `0`, `running 17 tests`, `17 passed; 0 failed; 0 ignored; 0 measured; 136 filtered out`.
4. **Refactor** — remove recursive `collect_entries`; keep helpers private; run focused + crate validation.
5. **Docs** — publish fixed ceilings, counting/depth rules, honest buffer/metadata envelope, stable resource errors, unchanged CLI exits in `docs/lab/local-validation.md`. No manual checklist.

## Test plan

All tests live in `tools/mmd-lab/src/archive.rs::tests`. Existing count `3`; final count exactly `17` (`3` retained/strengthened + `14` new). One test fn may contain multiple exact boundary subcases.

| Test | Seam/input | Exact expect |
| --- | --- | --- |
| `deterministic_hash_same_entries` | Public `build_archive`; reversed `a.txt=a`, `b.txt=b` | Exact 48-byte hex `4d4d4441524330310000000200000005612e74787400000000000000016100000005622e747874000000000000000162`; SHA-256 `ff1cee1ba99c0f3f2cbd6749bc4755e2d0b7e59b8f03f47a977a0f2f67e06295`; parsed order `a.txt`, `b.txt` |
| `hash_mismatch_detected` | Existing one-byte blob | Existing hash mismatch unchanged |
| `pack_tree_roundtrip` | Public pack twice; `hello.txt`, `sub/x.txt`, skipped `target` | Blobs equal; paths exactly `["hello.txt", "sub/x.txt"]`; data exact |
| `depth_limit_accepts_limit_and_rejects_deep_file` | Pack seam, max depth 2; empty `a/b`, then file `a/b/c` | Empty depth-2 dir passes; file returns `DepthLimitExceeded { actual: 3, limit: 2, path: "a/b/c" }` + exact display |
| `visited_limit_counts_dirs_skips_and_symlinks` | Pack seam, max visited 2; empty ordinary dir + symlink to external dir/file; then skipped `target` | First passes empty archive, proving dir + symlink count exactly 2 + symlink target never traversed; third enumerated skipped dir returns actual 3/limit 2. Symlink setup uses cfg-specific std API; Unix assertions always run on merge host |
| `builder_file_count_limit_accepts_limit_and_rejects_limit_plus_one` | Direct builder seam, max files 2; two then three empty entries | Two pass; three → `FileCountLimitExceeded { actual: 3, limit: 2 }` + exact display |
| `pack_file_count_rejects_before_second_read` | Hooked pack seam, max files 1; two sorted files | Error actual 2/limit 1; hook/read counter exactly 1 → attempted second file rejected pre-read |
| `builder_per_file_limit_accepts_limit_and_rejects_limit_plus_one` | Direct builder seam, max file 4; data 4 then 5 | 4 passes; 5 → file error actual 5/limit 4/path `x` + exact display |
| `pack_per_file_limit_accepts_limit_and_rejects_limit_plus_one` | Pack seam, file `x` 4 then 5 | 4 passes; 5 → same file variant/fields; oversized metadata rejects before hook/open |
| `builder_decoded_limit_accepts_limit_and_rejects_limit_plus_one` | Direct builder seam, decoded max 4; `a` 2 + `b` 2 then 3, per-file 8 | 4 passes; 5 → decoded error actual 5/limit 4/path `b` + exact display |
| `pack_decoded_limit_accepts_limit_and_rejects_limit_plus_one` | Pack seam, same tiny files/caps | 4 passes; 5 → same decoded variant/fields, independently proving pack collection path |
| `builder_encoded_limit_accepts_limit_and_rejects_limit_plus_one` | Direct builder seam `[path="a", data=[]]`; exact blob 25 | Encoded max 25 passes with 25-byte blob; max 24 → encoded error actual 25/limit 24 + exact display |
| `metadata_overhead_counts_toward_encoded_limit` | Direct builder seam, zero-byte `a`; decoded max 0; encoded max 24 | Decoded bound passes; exact 25-byte framing/path total fails encoded variant |
| `growth_after_metadata_obeys_active_budget_plus_one` | Hooked pack seam: (a) 4→5 B with both max 4; (b) prior `a`=3 B, aggregate max 4, per-file max 8, `b` starts 1 B then grows | (a) file error actual 5. (b) remaining aggregate 1 < per-file 8; bounded read observes 2 B, returns `DecodedSizeLimitExceeded { actual: 5, limit: 4, path: "b" }`; hook runs selected mutation once per subcase |
| `deterministic_first_failure_follows_sorted_order` | Pack seam, oversized `z/bad` + `a/bad`; pack twice | Both errors same fields/display; path always `a/bad` |
| `encoded_size_overflow_is_bounded_archive_error` | `checked_encoded_add(u64::MAX, 1)` | `SizeOverflow { context: "archive encoded length" }`; exact display; no payload allocation |
| `reservation_helpers_fail_without_allocation` | Empty `Vec<u8>`; call each of `reserve_additional` + `reserve_exact` with `usize::MAX` | Both → exact `AllocationFailed { context: "archive bytes", requested: usize::MAX }`; len/capacity remain 0; no allocation/panic |

## Impl steps

- [ ] 1. In `tools/mmd-lab/src/archive.rs`, import `std::fs::File` + `std::io::Read`; add exact `ArchiveLimits` + prod const.
- [ ] 2. Add eight exact `ArchiveError` variants/displays; retain `Io` + `Msg`.
- [ ] 3. Add fully working `checked_encoded_add`, `reserve_additional`, `reserve_exact` helpers with fixed mappings.
- [ ] 4. Add private `build_archive_with_limits`; route unchanged public builder through prod const; temporarily retain legacy encoder behavior.
- [ ] 5. Add private pack + hook seams; route unchanged public packer through prod const; hook immediately before legacy file read.
- [ ] 6. Run `cargo check -p mmd-lab --locked`; require exit `0` before tests.
- [ ] 7. Add/update exact 17 tests from table + tiny-limits helper. Run red literal cmd; require exact 5-pass/12-fail behavioral-red shape above, no compile errors.
- [ ] 8. Replace recursive collector with bounded sorted iterative DFS; count every enumerated entry; use checked counters + reserve helper.
- [ ] 9. Add archive-relative path helper preserving `/`; apply depth to files + dirs; never traverse symlink target.
- [ ] 10. Add pre-read file-count/metadata checks + bounded file helper using remaining aggregate, hook, `File::open`, `Read::take`, `64 KiB` buffer, reserve helper.
- [ ] 11. Implement builder exact order: count → checked per-entry/decoded/encoded preflight → sort/duplicate check → checked conversion/exact reserve/copy.
- [ ] 12. Remove recursive collector; verify no `fs::read(&path)`, unbounded `read_to_end`, or candidate-sized infallible reserve in pack/build.
- [ ] 13. Update `docs/lab/local-validation.md` archive section: exact defaults, counting/depth, 304 MiB major-buffer subtotal (explicitly not total peak), structurally bounded non-byte-quantified metadata, stable resource errors, unchanged exits, no overrides.
- [ ] 14. Run focused tests, formatter, crate tests, clippy. Fix only issue-scope failures.
- [ ] 15. Inspect `git diff --check` + scoped diff; exactly two impl files changed.
- [ ] 16. Commit impl as `fix(lab): bound candidate archive resources` with DCO sign-off only when impl job requests commit.

## Outputs

- `tools/mmd-lab/src/archive.rs`: bounded prod defaults, iterative traversal, bounded IO/encoding, stable errors, 17 archive tests.
- `docs/lab/local-validation.md`: operator contract.
- Public behavior: unchanged archive bytes/hash under limits; deterministic rejection above limits.
- Public API: unchanged fn signatures; new `ArchiveLimits`, prod const, resource variants. Private tiny-limit/hook seams only.
- Exactly 2 impl changed files. No deps, schema migration, CLI flags, format version, manual checklist.

## Validation

- [ ] Seam compile: `cargo check -p mmd-lab --locked` → exit `0`; no warnings/errors.
- [ ] Red evidence before enforcement: `cargo test -p mmd-lab archive::tests --locked` → exit `101`; `running 17 tests`; `FAILED. 5 passed; 12 failed; 0 ignored; 0 measured; 136 filtered out`; exact 12 behavioral tests named in TDD step 2 fail, no compile failures.
- [ ] Green focused: `cargo test -p mmd-lab archive::tests --locked` → exit `0`; `running 17 tests`; `17 passed; 0 failed; 0 ignored; 0 measured; 136 filtered out`.
- [ ] Crate: `cargo test -p mmd-lab --locked` → exit `0`; all suites `0 failed`; archive module contributes 17 passes.
- [ ] Format: `cargo fmt --all -- --check` → exit `0`; no diff.
- [ ] Lint: `cargo clippy -p mmd-lab --all-targets --all-features --locked -- -D warnings` → exit `0`; no diagnostics.
- [ ] Static bound check: `rg -n 'collect_entries|fs::read\(&path\)|read_to_end' tools/mmd-lab/src/archive.rs` → exit `1`, no matches.
- [ ] Reserve audit: `rg -n 'Vec::with_capacity|\.reserve\(' tools/mmd-lab/src/archive.rs` → one existing `parse_archive` `Vec::with_capacity(count)` only; no pack/build match.
- [ ] Baseline literal: `rg -n 'ff1cee1ba99c0f3f2cbd6749bc4755e2d0b7e59b8f03f47a977a0f2f67e06295|4d4d4441524330310000000200000005612e74787400000000000000016100000005622e747874000000000000000162' tools/mmd-lab/src/archive.rs` → exit `0`; both fixed oracles present.
- [ ] Diff hygiene: `git diff --check` → exit `0`.
- [ ] Scope: `git diff --name-only` → exactly `docs/lab/local-validation.md`, `tools/mmd-lab/src/archive.rs` during impl.
- [ ] Existing `archive`, `validate`, `validate-runner` call sites compile unchanged; accepted fixed vector proves format/hash compatibility.
- [ ] Manual check: not applicable; no UI/live transport.

## Acceptance boxes

- [ ] Depth `32`, visited `16_384`, files `8_192`, file `16 MiB`, decoded `64 MiB`, encoded `80 MiB` fixed in one prod const.
- [ ] Every prod builder/packer uses injected-limit seam.
- [ ] Helper impls compile + work before red behavioral tests; red shape is 5 pass/12 fail, not compile-red.
- [ ] Builder order is count → checked per-entry/decoded/encoded preflight → sort → bounded reserve/copy.
- [ ] Direct builder at limit/+1 proven for file count, per-file, decoded, encoded.
- [ ] Pack seam independently proven for pre-read count, per-file, decoded, growth.
- [ ] Traversal iterative + bounded; ordinary dirs/skips/symlinks count; symlink target never traversed; deep file exact path/error proven.
- [ ] Reads consume at most active bound + 1; prior-decoded aggregate-smaller-than-file case returns aggregate actual `limit + 1`.
- [ ] Every candidate-sized growth uses checked math + fallible reserve; both reserve helpers fail synthetically without allocation.
- [ ] Fixed 48-byte vector + SHA-256 pins pre-change `MMDARC01` output.
- [ ] No materialized test payload > `64 B`; overflow/reserve faults synthetic.
- [ ] Exactly 17 archive tests pass; impl diff changes exactly 2 files.
- [ ] Docs distinguish `304 MiB` major-buffer subtotal from total process peak + disclose metadata/allocator/std/OS overhead.

## Residual risks

- `parse_archive` trusts encoded count/lengths enough to allocate decoded entries. Dormant `#[allow(dead_code)]`; parser/extractor hardening remains outside F6 pack scope. Bound before future untrusted extraction exposure.
- `verify_bytes_hash` clones input. Current prod pack cap bounds it; arbitrary future callers must uphold cap or refactor.
- Filesystem replacement between `symlink_metadata` + `File::open` remains TOCTOU. Bounded read prevents byte exhaustion; containment/FIFO blocking is separate scope.
- `try_reserve` covers explicit vec growth. Path/entry count/depth are bounded, but their byte footprint follows host filesystem + allocator behavior; no exact process-RAM ceiling is claimed.
