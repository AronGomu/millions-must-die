# Local validation lab (trusted coordinator)

Normative policy: ADR 002, ADR 005, ADR 007. This doc is operator-facing.

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
- `schemas/benchmark-report-v1.schema.json` (app bench; candidate claim surface)
