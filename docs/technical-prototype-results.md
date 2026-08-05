# Technical Prototype Results (Phase 0)

**Scope claim (2026-08-05): 50k gate NOT verified — evidence inconclusive on a
noisy host; native cross-platform matrix deferred.**

This document records what phase 0 actually proved, what it did not, and where
every piece of raw evidence lives. Under the Hardware deferral policy
(2026-08-05, user-directed) the full-confidence 3-OS claim is **unavailable**:
no statement here implies native Windows or macOS proof. Under the T27
release-proof rule, noisy 50k evidence may freeze **neither** a pass nor a
failure — so **no release proof is frozen** and
`lab/releases/technical-prototype-v1.json` does not exist.

## What was proven (Linux)

- 50k flow-field agents simulate, upload, and render at 1920x1080 through the
  SDL3 GPU Vulkan backend on Linux; the interactive app runs the scene
  (`run --agents 50000`, 300 frames, clean exit).
- Real locked production bench policy (`production-v1`: 4 counts x 10 s warmup
  + 7 x 60 s trials) executed end-to-end on a real GPU; report committed as raw
  evidence.
- Zero-allocation contract holds under the production policy: **0 project Rust
  allocations inside measured frames at every scale count** (1k/10k/50k/100k).
- Full locally runnable gate set passes on the exact candidate commit:
  fmt, clippy `-D warnings`, workspace tests, `nix flake check`, xtask
  bootstrap/shaders/atlases `--check`, trusted `mmd-lab self-check`,
  fixture-mode merge gate (`mmd-lab validate --mode local-dev`, 3/3 lanes).

## What was NOT proven

- **The 50k absolute gate.** Recorded medians sit ~10x under the limits, but
  trial-to-trial noise at 50k exceeds the locked `nmad <= 0.03` bound, so the
  run is `inconclusive` by policy — not a pass, not a failure. No verdict may
  be derived from it, and none was.
- Any native Windows/macOS behaviour (see the deferred-hw ledger).

## Measured scale curve — Linux/Vulkan, production policy

Host: Linux x86_64, NVIDIA GeForce RTX 5060 Ti (**developer GPU — not the
locked RX 6400 reference target**; see caveats). Candidate commit
`82f162cce248d8af7286b21175de4b4b6183c9e4`.

| Agents | Blocking | median p95 (ms) | median p99 (ms) | nmad p95 | nmad p99 | Rust allocs | Verdict |
| ------ | -------- | --------------- | --------------- | -------- | -------- | ----------- | ------- |
| 1 000 | no | 0.052 | 0.097 | 0.0866 | 0.3257 | 0 | recorded |
| 10 000 | no | 0.341 | 0.439 | 0.0579 | 0.1384 | 0 | recorded |
| 50 000 | **yes** | 1.670 | 1.981 | 0.0653 | 0.1388 | 0 | **inconclusive** |
| 100 000 | no | 3.151 | 3.271 | 0.0034 | 0.0162 | 0 | recorded |

### 50k absolute gate verdict (the phase gate)

**Inconclusive — rerun required on a quiet host.** Report verdict:
`inconclusive`, reason `50k normalized MAD over 3% (nmad_p95=0.0653
nmad_p99=0.1388)`.

The recorded 50k medians (p95 1.670 ms, p99 1.981 ms) are far under the limits,
and per-trial medians were stable (sim 1.061-1.077 ms, gpu queue latency
1.594-1.636 ms); the instability is in the p95/p99 tail (1.543-1.923 ms across
the seven trials). 100k in the same run was quiet (nmad_p95 0.0034), which
points at host interference rather than an engine property — a screen
capture/encode session (OBS) plus a Wayland compositor and browser were live
on the GPU during the run. Reruns to obtain quiet-host evidence were stopped by
owner decision (2026-08-05); the gate stays open.

Limits (locked, unchanged): median p95 <= 16.67 ms, median p99 <= 25 ms,
nmad <= 0.03. No threshold or tolerance was changed in this phase close.

### Relative gates

**Disabled.** No enabled baselines exist. The T26 pilot pipeline was proven on
synthetic data only; synthetic provenance can never enable a baseline
(T25/T26 rule), and the real 150-run physical pilot is `deferred-hw`.

## Raw evidence

| Artifact | Location |
| --- | --- |
| Raw Linux production bench report (kept forever) | `lab/releases/evidence/linux-vulkan-bench-production-v1.json` (sha256 `31916575623bd7f78406859b49833ae323068147ed5bcb85b5cdfe8f690c47c2`) |
| Release proof manifest | **not frozen** — inconclusive evidence may freeze no status |
| Proof validator | `tools/mmd-lab/src/release.rs`; CLI `mmd-lab release-freeze` / `release-check` |
| Proof schema | `schemas/release-proof-v1.schema.json` |
| GPU profiler runbook | `docs/lab/gpu-profiling.md` |

To close the gate once quiet-host evidence exists:

```sh
# 1. rerun the production bench with no capture/compositor load on the GPU
cargo build --release && ./target/release/millions_must_die bench \
  --output lab/releases/evidence/linux-vulkan-bench-production-v1.json

# 2. freeze + verify (only succeeds on decisive evidence)
cargo run -p mmd-lab -- release-freeze \
  --bench-report lab/releases/evidence/linux-vulkan-bench-production-v1.json \
  --commit <exact-hash> --out lab/releases/technical-prototype-v1.json
cargo run -p mmd-lab -- release-check \
  --proof lab/releases/technical-prototype-v1.json --commit <exact-hash> \
  --bench-report lab/releases/evidence/linux-vulkan-bench-production-v1.json
```

## Deferred-hw ledger (honest record of what did NOT run)

| Item | State |
| --- | --- |
| Quiet-host 50k gate evidence | **open** — owner stopped reruns 2026-08-05; gate unverified |
| Windows/D3D12 native lane (build, golden, bench, gate) | `deferred-hw` — no physical Windows ref PC (T9/T16/T17 fixture-only) |
| macOS/Metal native lane (build, golden, bench, gate) | `deferred-hw` — no M4 Mac / MDM lab (T10/T19/T22/T23 fixture-only) |
| Real 3-host reset/run/reset merge gate | `deferred-hw` — T24 gate proven in fixture/fake-transport mode only |
| Physical 150-run pilot + enabled relative baselines | `deferred-hw` — T25/T26 synthetic proof only; baselines disabled |
| Linux RenderDoc capture | procedure documented (`docs/lab/gpu-profiling.md`), not captured |
| Windows RenderDoc capture | `deferred-hw` — procedure documented, unverified |
| Xcode Metal capture | `deferred-hw` — procedure documented, unverified |
| RX 6400 / entry-level reference hardware measurements | `deferred-hw` — bench ran on a developer RTX 5060 Ti |
| Trusted-install (`$HOME/.local/bin/mmd-lab`) refresh | awaits promotion of this candidate; workspace binary used with `--skip-self-check` |

## Caveats and known issues

- **Developer GPU, not reference hardware.** The RTX 5060 Ti is far above the
  locked entry-level RX 6400 target. Even a future pass here would not predict
  a pass on reference hardware; the reference-host verdict stays open until the
  deferred lanes run.
- **Noisy host.** The measurement host ran a screen capture/encode session, a
  Wayland compositor and a browser on the same GPU. Phase-0 measurement design
  assumes a quiet host; this run violated that assumption and the noise gate
  caught it.
- **Commit binding is coordinator assertion.** `benchmark-report-v2` carries
  no git commit field; a proof's `candidate_commit` would bind evidence to a
  commit by trusted-coordinator assertion (bench binary built from a clean tree
  at that commit), not cryptographically.
- **Known flaky tests under full parallel load.**
  `warmup_allocation_passes` and `panic_restores_guard`
  (`crates/mmd-engine/tests/frame_allocations.rs`) can fail when the whole
  workspace test suite runs at high parallelism (allocator-counting tests are
  cross-talk sensitive); they pass in isolation and when the
  `frame_allocations` test binary runs alone. Not fixed in this close to avoid
  scope creep; tracked as a known issue.
- **GPU metric is a proxy.** `gpu_queue_latency` is submit-to-fence latency,
  not true GPU execution time; true-GPU timing comes from manual profiler
  captures (runbook above).
- The close commit adds docs and the honest-absence proof test on top of the
  benched candidate commit `82f162c`; the bench binary was built from the clean
  tree at that commit.

## Phase outcome

**Phase 0 is not closed as passed.** The engine slice, bench harness,
zero-allocation contract, validation lab and gate machinery are built and
locally green, and the 50k scene runs with medians roughly an order of
magnitude inside the frame-time limits — but the one blocking measurement
(50k absolute gate) has no decisive evidence, and the Windows/macOS lanes were
never run on hardware. The honest status is **open**: no pass claim, no
failure claim, and no frozen release proof. What remains is one quiet-host
bench rerun (Linux scope) plus the deferred-hw backlog (full 3-OS scope).
