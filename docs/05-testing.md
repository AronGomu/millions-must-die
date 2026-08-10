# Testing Strategy

Prototype-first development. Each prototype answers one question before
continuing.

Phase 0 proves the game and its systems *work* — every system covered by
automated behavioural tests, and the scene runs end to end. It does not prove
how fast they run, and it proves nothing about any host other than the
development one. This document is the single source of truth for what gates a
merge; anything not listed under [Required merge gate](#required-merge-gate)
gates nothing.

Phase 0 is **closed on that functional scope**. The system → test map, and
every known gap in it, are recorded in the
[functional close](technical-prototype-functional-close.md). Two tests in
`tests/validation_contract.rs` keep this document and its siblings honest:
`every_system_has_a_test` fails when a system named in scope loses the test
that proves it, and `no_perf_claim_in_docs` fails when a live doc states a
speed measurement without marking it retired or unmeasured.

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
cargo run -- run --agents 5000 --frames 300
cargo run -- run --scenario assets/scenarios/collision_mid_v1.ron --frames 300
cargo run -- run --scenario assets/scenarios/collision_sprite_v1.ron --frames 300
cargo run -- rts --frames 1600 --inject-input-file assets/scenarios/rts_acceptance_v1.script
```

The last command is the phase-1 interactive smoke: one tracked script selects
the starting workers, puts them on a crystal node, builds a Depot and a
Barracks and produces a Worker and a Soldier. It asserts behaviour and an exit
code — it consumes no measurement number, and a scripted event that never fires
fails it.

The three `run` commands before it are the phase-0 interactive smokes: the
5 000-agent gate scene and both collision demo scenes must start, tick and exit
cleanly. All three
carry the same body — a radius of exactly half a sprite width, measured in cell
space, where the contact test lives. What the demo scenes add is the rest of the
tuning surface, which the gate scene leaves at its identity values:
`collision_mid_v1` spreads the separation pass over four phases, and
`collision_sprite_v1` runs a thinner crowd over two push-priority classes, which
is what makes the asymmetric push observable rather than merely configured.

Hard success is **all tests green**: deterministic simulation and navigation
behaviour, render correctness on the development host, app and CLI lifecycle,
contract hashes, and the allocation invariant.

## Render correctness — development host only

`cargo test --workspace --locked` covers rendering in three layers
(`crates/mmd-engine/tests/render_correctness.rs`):

| Layer | Needs a GPU? | What it proves |
| --- | --- | --- |
| Instance data | no | one instance per alive agent, in the atlas bucket the sim names, centred on the agent's world position, carrying that agent's `(dir, frame)` UV rect |
| Projection | mirror: no / oracle: yes | `render::world_to_clip` states where a world corner lands in clip space; a GPU raster probe requires the real `shaders/sprite.hlsl` to agree |
| Whole frame | yes | the committed host golden, compared exactly |

Atlas manifest ↔ generated PNG consistency is checked in both directions: the
positive load in `render_correctness.rs`, and the tamper case in
`gpu_smoke.rs::tracked_atlas_hashes_are_enforced`.

### Scope of the golden claim

Goldens are **host-scoped**. `lab/goldens/linux-vulkan/` proves that *this*
development backend, on the adapter recorded in its manifest, renders the
expected frame. It is not a cross-backend or cross-platform claim: the
comparator refuses a foreign golden outright instead of diffing it, and the
`windows-d3d12` / `macos-metal` families stay `placeholder-deferred-hw` — they
document intent and can never pass a comparison.

The comparison is exact (`max_channel_delta == 0`). Should a bounded tolerance
ever be introduced after reviewed native evidence, it would cover `f32` and
driver variation **on that one host** — never a second backend.

### Regenerating the golden

Regeneration is an explicit, reviewed step — one command:

```sh
MMD_UPDATE_GOLDEN=1 cargo test -p mmd-engine --test gpu_golden -- --ignored update_host_golden
```

It rewrites `lab/goldens/<family>/{golden.png,manifest.json}`. Review the image
diff before committing: a regenerated golden re-baselines the gate, so it must
be a deliberate decision, never a way to make a red test go green.

When a frame drifts, the failure writes a reviewable artifact to
`target/golden-diffs/<test>/` — `actual.png`, a magenta-on-black `diff.png`
mask, and `diff.json` (differing pixel count, worst channel delta, first
differing pixel). A passing comparison writes nothing.

### Hosts without a GPU

Every GPU-bound case obtains its renderer through a helper that **skips** —
prints and returns, counted as a pass — when the host has no GPU device at all,
so the suite stays runnable in a headless shell. The skip is deliberately
narrow: only `RenderError::DeviceUnavailable` (SDL init or device creation
failed) skips. A drifted atlas, a rejected software adapter, or a broken group
contract fails, because treating those as "headless" would hole the gate.

`cargo test` hides a passing test's output, so a skipped run and a verified run
print the same `ok`. **On a host that is supposed to have a GPU — which is what
the development host is — run the gate with the skip disabled:**

```sh
MMD_REQUIRE_GPU=1 cargo test --workspace --locked
```

With that set, any skip becomes a failure naming the capability that was
missing. Leave it unset only where the absence of a GPU is expected.

Known narrowness, recorded rather than papered over: a host whose only Vulkan
ICD is a software rasterizer (lavapipe) *does* create a device and is then
rejected by the adapter gate, so it fails instead of skipping. Reclassifying it
would also excuse the macOS "never MoltenVK" rejection, which must stay a hard
failure — and the two cannot be told apart on a host that has neither.

## App and CLI lifecycle

`tests/cli_contract.rs` drives the built binary as a subprocess. The engine
tests prove the systems behave; these prove the *app* wires them — that
`--frames N` is honoured, Space pauses, F1 is inert, Esc releases the GPU
window and returns 0, and every rejection is a message rather than a panic.

The `run` command prints a `key=value` contract on stdout, opening with a
`run: frame0 …` line and closing with `run: clean exit …` (state hash, tick,
frames rendered, and the quit/paused/overlay flags). A run that cannot defend
those claims prints no exit line and fails instead. The full shape is
documented at the top of `src/run.rs`.

Exit codes:

| Code | Meaning |
| --- | --- |
| 0 | ran to the frame budget, or quit, and shut down cleanly |
| 1 | actionable failure — bad flag value, bad scenario, render failure |
| 2 | clap usage error |
| 3 | this host has no usable GPU device |

Code 3 is what lets a headless shell tell "no device here" apart from a defect
without scraping the message: cases that need a device skip on 3 and stay loud
on everything else, the same policy — and the same `MMD_REQUIRE_GPU=1`
override — as the render tests above.

Runs with no keyboard drive the bindings through `--inject-input FRAME:KEY`
(1-based frames; `esc`, `f1`, `space`). Scripted presses resolve through the
same key→action mapping the live SDL path uses, and a press that never fires
turns the run into a failure — otherwise a test asserting "nothing changed"
would pass because nothing was ever pressed.

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
  per-frame allocation; it says nothing about throughput. A measure scope is
  armed per-thread, so allocations made by a thread that never entered it are
  invisible to it — nothing in this project hands frame work to another thread,
  and `foreign_thread_allocations_do_not_leak_into_a_measure_scope` pins that
  boundary.
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
