# Merge workflow — exact-hash 3-host gate (T24)

One trusted command decides whether a candidate may merge: the aggregate gate
over all three native lanes (Ubuntu/Vulkan, Windows/D3D12, macOS/Metal).
There are no path filters and no exceptions — every merge, every path.

## Trust model

- The coordinator binary is the trusted side. It must pass `self-check`
  (absolute path + digest against `$HOME/.config/mmd-lab/trusted-tools.toml`)
  before any dispatch. `--skip-self-check` exists for dev tests only.
- Candidate reports are untrusted input. The coordinator recomputes every
  policy verdict (percentiles, MAD noise, alloc/drain/queue caps, golden
  diff) from raw evidence; a candidate- or lane-reported "pass" can never
  override the recompute.
- Host/source identity is attested; candidate behavior is not
  cryptographically attested. Manual review + the reset model stay required
  (see plan "Risks / stop rules").

## Owner runbook

1. Refresh the trusted coordinator from trusted `main` when it changed:
   `cargo install --path tools/mmd-lab --root "$HOME/.local" --locked`
   then `$HOME/.local/bin/mmd-lab install`.
2. `$HOME/.local/bin/mmd-lab self-check` — refuse to continue on failure.
3. Check out the exact candidate commit; the worktree must be clean
   (`validate` in `pr` mode rejects dirty trees).
4. Run the aggregate gate:

   ```sh
   $HOME/.local/bin/mmd-lab validate --commit <exact-hash>
   ```

   The command packs one content-addressed archive, runs all three candidate
   lanes against it, and prints the deterministic PR summary.
5. Read the summary. Merge **only** when the decision line is
   `**merge-exact-hash**`, and merge **exactly** the tested hash. Any new
   push — even a rebase — is a different hash and needs a new gate run.
6. Paste the summary into the PR as the merge record. With `--retain-dir`,
   full evidence JSON + archive + summary are retained per archive hash
   (ordinary runs 90 days; release/calibration runs forever).

## Verdict semantics

- **Fail-fast is false.** All lanes are collected before the verdict; the
  summary lists every blocking reason across all lanes.
- **Missing lane blocks.** All three native lanes are required.
- **Mixed source hashes block.** Every lane must have run the exact same
  content-addressed archive; the summary binds that hash.
- **One inconclusive blocks.** Noise (normalized MAD > 3%) is not a pass.
- **Stale manifest blocks.** Evidence written under an outdated
  evidence/host-manifest schema is rejected.
- **Forged summaries block.** Claimed stats that disagree with the
  coordinator recompute are surfaced and the merge is refused.
- **50k blocks; 1k/10k/100k are required evidence but nonblocking.** The
  scale rows must exist for every lane and are recorded in the summary;
  only the 50k absolute gate (median p95 ≤ 16.67 ms, median p99 ≤ 25 ms)
  and the correctness gates decide.
- **Reset is part of the verdict.** A lane whose post-run external restore
  did not start is an error, not a pass.

Exit codes: `0` merge-exact-hash, `1` blocked (fail), `2` blocked
(inconclusive), `3` blocked (protocol/tool error).

## Hardware deferral status (2026-08-05)

This gate currently runs in fixture/fake-transport scope: lane evidence,
readbacks, and recovery drills are protocol simulations, and the Windows and
macOS goldens are committed fixture goldens (`[deferred-hw]`). The real
3-host reset → native run → reset cycles remain deferred until the physical
lab exists; until then a pass is "Linux-verified pipeline + fixture-verified
protocol", never a full-confidence 3-OS claim.
