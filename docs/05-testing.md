# Testing Strategy

Prototype-first development. Each prototype answers one question before
continuing.

Phase 0 proves the game and its systems *work* — every system covered by
automated behavioural tests, and the scene runs end to end. It does not prove
how fast they run. This document is the single source of truth for what gates a
merge; anything not listed under [Required merge gate](#required-merge-gate)
gates nothing.

## Required merge gate

Every command here is deterministic and behavioural. None consumes a
measurement number, and none may be replaced by one.

```sh
cargo fmt --all -- --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --all-features -- -D warnings
nix flake check
cargo run -p xtask -- bootstrap --check
cargo run -p xtask -- shaders --check
cargo run -p xtask -- atlases --check
cargo run -- run --agents 50000 --frames 300
```

Hard success is **all tests green**: deterministic simulation and navigation
behaviour, render correctness on the development host, app and CLI lifecycle,
contract hashes, and the allocation invariant. The last command is the
interactive smoke — the 50k scene must start, tick, and exit cleanly.

## Retired: performance gating

Performance measurement moved out of phase 0 to a later **optimization phase**
on the finished game (plan amendment 2026-08-05 #2, user-directed). Nothing
below gates a merge any more. Nothing was deleted — it is frozen in place so
the optimization phase can reuse it, and it must be re-validated before that
reuse.

Retired by name:

| Retired | Where it still lives | Status |
| --- | --- | --- |
| 50k frame-time gate (median p95 ≤ 16.67 ms, p99 ≤ 25 ms) | `crates/mmd-engine/src/bench/` | frozen, **not a gate** |
| `nmad ≤ 0.03` trial-noise bound | `crates/mmd-engine/src/bench/stats.rs` | frozen, **not a gate** |
| 1k/10k/50k/100k scale curve as an acceptance artifact | `crates/mmd-engine/src/bench/` | frozen, **not a gate** |
| Windows/D3D12 and macOS/Metal platform lanes | `docs/platform/`, `lab/manifests/` | frozen, **not a gate** |
| 3-host validation lab, runner contracts, recovery/attestation, candidate + exact-hash aggregate gates | `tools/mmd-lab/`, `lab/`, `docs/lab/` | frozen, **not a gate** |
| Relative calibration, pilot baselines, release proofs | `tools/mmd-lab/`, `schemas/*baseline*`, `schemas/*pilot*`, `schemas/*release*` | frozen, **not a gate** |

Frozen means: the code still compiles, its own unit tests still run on every
merge (that is what proves it still compiles), and nothing it emits decides
whether a change may merge.

`bench` stays available as a developer tool — **not a gate; optimization
phase**:

```sh
cargo run -- bench --help
cargo run -- bench --test-policy --dry-cpu   # short policy, no GPU
```

### Kept, because it is correctness rather than speed

- Deterministic simulation, flow-field, and scenario contracts with their hash
  checks.
- Renderer output correctness (golden images) on the development host only —
  these prove the development backend renders the expected frame, never that
  another platform does.
- The zero-allocation-per-frame contract, reframed as a **code-health
  invariant** under a short deterministic policy
  (`crates/mmd-engine/tests/frame_allocations.rs`). It catches accidental
  per-frame allocation; it says nothing about throughput.
- Reproducible builds and toolchain checks (`nix flake check`, xtask
  `--check`).

### History

Measurements taken before the retirement are kept as history, claiming
nothing: [technical prototype results](technical-prototype-results.md)
(superseded), with raw evidence at
`lab/releases/evidence/linux-vulkan-bench-production-v1.json`.

Design records that defined the retired acceptance criteria are marked
superseded and kept for history:
[ADR 001](ADR/001_ADR_technical_prototype_scope_and_acceptance.md),
[ADR 005](ADR/005_ADR_benchmark_measurement_and_baselines.md),
[ADR 006](ADR/006_ADR_native_platform_and_reference_hardware.md),
[ADR 007](ADR/007_ADR_local_validation_lab_and_security.md).
