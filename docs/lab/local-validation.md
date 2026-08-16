# Local validation lab (trusted coordinator) — FROZEN, NON-GATING

> **RETIRED as a gate (2026-08-05, T28).** The validation lab and everything
> under `docs/lab/` are frozen in place for a later optimization phase: the
> code still builds and its unit tests still run, but nothing it emits gates a
> merge. Current gate:
> [testing strategy](../05-testing.md#required-merge-gate). Re-validate before
> reuse.

Normative policy: ADR 002, ADR 005, ADR 007 (005 and 007 superseded in part by
T28). This doc is operator-facing.

## Trust boundary

| Trusted | Untrusted |
| --- | --- |
| Installed `$HOME/.local/bin/mmd-lab` | Candidate/PR source tree |
| `$HOME/.config/mmd-lab/trusted-tools.toml` digest | Candidate-generated bench stats/reports |
| External recovery + network controls | Host cleanup initiated by candidate |
| Coordinator recompute from raw samples | Claimed p95/p99/verdict fields |

Operational commands **must** use the installed absolute binary after `self-check`.  
Dev unit/integration tests may use workspace `cargo test -p mmd-lab`.

## Install (trusted tree only)

```bash
# Build/install from trusted main (or reviewed release commit), not from PR worktree.
cargo install --path tools/mmd-lab --root "$HOME/.local" --locked

# Or from a just-built binary:
"$HOME/.local/bin/mmd-lab" install \
  --bin "$HOME/.local/bin/mmd-lab" \
  --manifest "$HOME/.config/mmd-lab/trusted-tools.toml"
```

`install` copies the running binary (if invoked that way) or use `cargo install` then:

```bash
# After cargo install, record digest from the installed path:
$HOME/.local/bin/mmd-lab install
```

Manifest lives **outside** the candidate tree. Do not commit home paths or secrets.

## Self-check

```bash
$HOME/.local/bin/mmd-lab self-check \
  --manifest "$HOME/.config/mmd-lab/trusted-tools.toml"
```

Checks:

1. Manifest exists and `binary_path` is absolute.
2. Running process path canonicalizes to that install path.
3. SHA-256 of running binary matches manifest.

Failure → **no dispatch**.

## Modes

| Mode | Flag | Dirty worktree |
| --- | --- | --- |
| PR / merge gate | `--mode pr` | **Rejected** |
| Local experiment | `--mode local-dev` | Allowed (explicit) |

## Archive

Coordinator builds a deterministic `MMDARC01` content-addressed archive (SHA-256 of full bytes). Remote agents must verify the digest **before** executing candidate code.

```bash
$HOME/.local/bin/mmd-lab archive --root . --out-dir /tmp/mmd-archives
```

Archive packing uses fixed, non-configurable ceilings: traversal depth `32`, visited entries `16,384`, packed files `8,192`, one file `16 MiB`, retained decoded file data `64 MiB`, and final encoded archive `80 MiB`. Root depth is `0`; a direct child is depth `1`. Every successfully enumerated entry consumes the visited-entry budget before name skipping or symlink filtering, including ordinary directories, skipped names, files, and symlinks. Symlink targets are never traversed. Accepted inputs keep the exact existing `MMDARC01` byte format and hash.

The major byte-buffer subtotal across packing, fake transport, and hash-verification clone paths is bounded by `64 MiB + 3 × 80 MiB = 304 MiB`. This is not an exact peak or a process RAM cap. Path and entry metadata remain structurally bounded by visited/file/depth ceilings plus host filesystem path/name limits, but are not separately byte-capped. Allocator capacity, hashing state, standard-library state, and OS metadata add overhead.

Resource failures use stable messages beginning `archive limit exceeded:`, `archive size overflow:`, or `archive allocation failed:`. Filesystem `io:` details remain OS-defined. `mmd-lab archive` still reports `archive failed: ...` and exits `1`; `validate` and `validate-runner` still report their existing `... archive failed: ...` prefixes and exit `3`. No CLI flag, environment variable, or config setting overrides archive ceilings.

## Fake 3-agent matrix (no physical hosts)

```bash
$HOME/.local/bin/mmd-lab self-check --manifest "$HOME/.config/mmd-lab/trusted-tools.toml"

$HOME/.local/bin/mmd-lab validate \
  --mode local-dev \
  --root . \
  --fake-fixtures lab/fixtures/fake-agents \
  --retain-dir "$HOME/.local/share/mmd-lab/runs" \
  --summary-out /tmp/mmd-pr-summary.md
```

Without `--fake-fixtures`, built-in synthetic pass agents (`ubuntu-ref`, `windows-ref`, `macos-ref`) run.

Example config: `lab/config.example.toml`.

## Coordinator verdict

1. Deliver archive; stop on remote hash mismatch (no exec).
2. Parse host evidence as **untrusted** input.
3. Recompute 50k trial p95/p99 (Hyndman–Fan type 7) + median/MAD from raw `frame_service_ms`.
4. Apply absolute gates (p95 ≤ 16.67 ms, p99 ≤ 25 ms, NMAD ≤ 3%, alloc=0, drain/FIF caps).
5. Reject claimed stats that disagree with recompute.
6. Emit PR summary bound to exact `archive_sha256` (+ optional git commit).

Host/source identity may be attested later; **perf truth is coordinator-verified evidence**.

## Retention

Full JSON evidence under retain dir keyed by `sha256-<archive>`. Ordinary merge retention target: 90 days (config). Release/calibration retention is indefinite (later tickets).

## Schemas

- `schemas/lab-archive-v1.schema.json`
- `schemas/lab-host-manifest-v1.schema.json`
- `schemas/lab-raw-sample-v1.schema.json`
- `schemas/lab-host-evidence-v1.schema.json`
- `schemas/benchmark-report-v2.schema.json` (current app bench; candidate claim surface)
- `schemas/benchmark-report-v1.schema.json` (historical reports)
