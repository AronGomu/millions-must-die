# Commands

Every command runnable in this repo. Source of truth for the gate stays
`docs/05-testing.md`; this page is the flat index.

Toolchain: Rust 1.95.0 pinned by `rust-toolchain.toml`. On NixOS enter
`nix develop` first. All commands run from repo root.

## Required merge gate

All must pass, in this order:

```sh
./scripts/check-dco TRUSTED_BASE_SHA EXACT_CANDIDATE_SHA
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

`check-dco` takes real SHAs: base = `git merge-base main <candidate>`
(exclusive), candidate = exact commit under test (inclusive).

## Game binary (`millions_must_die`)

```sh
cargo run -- run                                    # interactive horde scene (5000 agents)
cargo run -- run --agents N                         # agent count override
cargo run -- run --scenario PATH.ron                # scenario override
cargo run -- run --frames N                         # auto-exit after N frames
cargo run -- run --inject-input '4:space,20:esc'    # scripted keys (esc/f1/space), 1-based frames
cargo run -- rts                                    # interactive RTS prototype
cargo run -- rts --scenario PATH.ron
cargo run -- rts --frames N
cargo run -- rts --inject-input 'FRAME:KIND[:ARGS];...'
cargo run -- rts --inject-input-file PATH.script    # mutually exclusive with --inject-input
cargo run -- bench [--output R.json] [--scenario P.ron] [--test-policy] [--dry-cpu] [--inject-frame-alloc]
```

`bench` is frozen and gates nothing — perf is retired from phase 0.

Tracked scenarios: `technical_prototype_v1.ron`, `collision_mid_v1.ron`,
`collision_sprite_v1.ron`, `rts_prototype_v1.ron`.
Tracked input scripts: `rts_acceptance_v1.script`,
`rts_feedback_polish_v1.script` (both under `assets/scenarios/`).

## xtask (bootstrap / reproducibility)

Each has a `--check` mode used as a gate; without `--check` it regenerates.

```sh
cargo run -p xtask -- bootstrap [--check]
cargo run -p xtask -- shaders   [--check]   # shaders/sprite.hlsl -> shaders/generated/
cargo run -p xtask -- atlases   [--check]
cargo run -p xtask -- audio     [--check]   # assets/audio/generated/*.wav + manifest
```

## Tests

```sh
cargo test --workspace --locked                     # full suite (gate)
cargo test -p mmd-engine                            # engine tests only
cargo test -p mmd-engine --test NAME                # one engine test binary
cargo test --test NAME                              # one app test binary
MMD_REQUIRE_GPU=1 cargo test --workspace            # disable headless skip on a real GPU
MMD_UPDATE_GOLDEN=1 cargo test -p mmd-engine --test gpu_golden -- --ignored update_host_golden
```

Golden regeneration is explicit and reviewed — never run it to make a red gate
green.

Engine test binaries (`crates/mmd-engine/tests/`): `benchmark_policy`,
`camera`, `display_viewport`, `flow_field`, `frame_allocations`, `gpu_golden`,
`gpu_smoke`, `harness`, `nav_pool`, `render_correctness`, `rts_acceptance`,
`rts_build`, `rts_collision`, `rts_economy`, `rts_formation`, `rts_hud`,
`rts_minimap`, `rts_nav_staleness`, `rts_pack`, `rts_production`,
`rts_radius_nav`, `rts_selection`, `rts_world`, `runtime_frame`,
`scenario_contract`, `separation`, `simulation`, `ui_text`.

App test binaries (`tests/`): `cli_contract`, `dco_range_gate`,
`rts_acceptance`, `rts_cli_contract`, `validation_contract`.

## Lint / format / build

```sh
cargo fmt --all
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo build --workspace --locked
cargo build --release
cargo tree -e features | grep -c testkit          # must print 0 (testkit out of shipping build)
```

## Nix

```sh
nix develop            # dev shell with pinned toolchain + SDL3 deps
nix flake check        # checks: rustc-version, toolchain-file (gate)
```

## Scripts

```sh
./scripts/check-dco BASE_SHA CANDIDATE_SHA          # offline DCO range gate
```

## Lab CLI (`tools/mmd-lab`) — frozen, non-gating

Runs only from trusted `main`; PR code is untrusted input.

```sh
cargo run -p mmd-lab -- doctor [--runner ubuntu]
cargo run -p mmd-lab -- attest-ubuntu  --manifest M.toml --observed O.json
cargo run -p mmd-lab -- attest-windows --manifest M.toml --observed O.json
cargo run -p mmd-lab -- attest-macos   --manifest M.toml --observed O.json
cargo run -p mmd-lab -- ubuntu-recover-simulate  --image-manifest I.toml --runner-manifest R.toml --attest-fixture F.json
cargo run -p mmd-lab -- windows-recover-simulate --image-manifest I.toml --runner-manifest R.toml --attest-fixture F.json
cargo run -p mmd-lab -- macos-recover-simulate   --mdm-profile P.json --runner-manifest R.toml --attest-fixture F.json
cargo run -p mmd-lab -- install
cargo run -p mmd-lab -- self-check
cargo run -p mmd-lab -- archive
cargo run -p mmd-lab -- validate
cargo run -p mmd-lab -- validate-runner
cargo run -p mmd-lab -- calibrate
cargo run -p mmd-lab -- pilot-synth
cargo run -p mmd-lab -- pilot-assemble
cargo run -p mmd-lab -- release-freeze
cargo run -p mmd-lab -- release-check
```

Recover/simulate subcommands are dry-run only: no PXE, DISM or disk writes.
Add `--help` to any of them for the full flag set.

Host provisioning scripts (run on the lab hosts, not here):
`lab/provision/{ubuntu,macos}/{attest,recover,run-candidate}.sh` and
`lab/provision/windows/{attest,recover,run-candidate}.ps1`.

## graphify

```sh
graphify query "<question>"
graphify path "<A>" "<B>"
graphify explain "<concept>"
graphify update .          # after modifying code (AST-only, no API cost)
```

## Git conventions

```sh
git commit -s              # DCO sign-off required on every commit
```

No online CI / GitHub Actions — forbidden, see `SECURITY.md`.
