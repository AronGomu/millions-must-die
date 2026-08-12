# Millions Must Die

Offline-first PC RTS focused on fortress defense against massive enemy hordes.

## Status

Phase 0 technical prototype: **closed on functional scope — the game systems are proven to behave correctly.**

The 5 000-agent flow-field gate scene runs on Linux/Vulkan (5 000 is the engine's live simultaneous-agent ceiling, `scenario::MAX_LIVE_AGENTS`). Phase 0 closed on automated behavioural tests of every game system, not on speed: **performance is unmeasured and gates nothing**, and no verification exists for any platform other than the development host. Frame-time gating, the cross-platform matrix, and the multi-host validation lab are retired to a later optimization phase on the finished game — the code stays in-tree, frozen and non-gating.

What phase 0 proves, what it does not, and every known gap: [docs/technical-prototype-functional-close.md](docs/technical-prototype-functional-close.md). What gates a merge: [docs/05-testing.md](docs/05-testing.md). Earlier measurements, kept as history and claiming nothing: [docs/technical-prototype-results.md](docs/technical-prototype-results.md) (superseded).

Phase 1 RTS engine prototype: **closed on functional scope.** A thin vertical slice through six systems — a camera you pan, units you select, workers that gather, an economy that banks, buildings you place, units you produce — on a horde-free scene, with phase 0's contracts untouched. There is no combat, no enemy AI, no zoom and no balance pass, and performance remains unmeasured.

Phase 1.1 interaction, UI and audio hardening: **closed on functional scope.** What you click is now what the tests click — visible sprite geometry, RTS unit bodies that cannot merge, formations, a StarCraft-shaped HUD with a minimap and a command card, persistent settings, three window modes, and deterministic generated audio. Hard collision is RTS-only: the 5 000-agent horde keeps its soft separation and may still overlap. Nothing on the gate claims a window appeared or a sound was heard. What it proves, what it does not, and every known gap: [docs/rts-interaction-ui-audio-hardening-functional-close.md](docs/rts-interaction-ui-audio-hardening-functional-close.md).

See it run:

```sh
cargo run -- rts --frames 1600 --inject-input-file assets/scenarios/rts_acceptance_v1.script
```

That is the tracked acceptance script: it selects the starting workers, puts them on a crystal and a gas node, builds a Depot and a Barracks through the command card, produces a Worker and a Soldier, jumps the camera from the minimap, opens the pause menu and Settings, and quits. Drop `--inject-input-file` to drive it yourself — arrows or screen edges pan, left-click and drag select, right-click orders, `Q`/`W`/`E` open a build ghost (HQ / Depot / Barracks) and `X` cancels it, `A`/`S` queue a Worker / Soldier, `R` sets a rally point, the gear icon or `Escape` opens the pause menu. What phase 1 proves, what it does not, and every known gap: [docs/rts-engine-prototype-functional-close.md](docs/rts-engine-prototype-functional-close.md).

## License

Authored project code, docs, shaders, and generated placeholder assets: **[MIT-0](LICENSE)**.

Name and logo are reserved — see [`TRADEMARKS.md`](TRADEMARKS.md). Third-party dependencies keep their own terms.

## Workspace

| Path | Role |
| --- | --- |
| `.` (`millions_must_die`) | App binary: `run`, `rts`, `bench` |
| `crates/mmd-engine` | Engine library |
| `tools/mmd-lab` | Trusted local lab CLI: `doctor`, `validate` |
| `xtask` | Bootstrap / reproducibility tasks |

## Toolchain

- Rust **1.95.0** via [`rust-toolchain.toml`](rust-toolchain.toml)
- Linux/NixOS: [`flake.nix`](flake.nix) (`nix develop`, `nix flake check`)
- Windows/macOS: `rustup` toolchain from `rust-toolchain.toml`

## Development

### Required merge gate

Everything that must pass before a merge, and nothing else. Defined by
[docs/05-testing.md](docs/05-testing.md).

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

### Developer tools (not gates)

```sh
cargo run -- run --help
cargo run -- bench --help    # benchmark harness — not a gate; optimization phase
cargo run -p mmd-lab -- --help
cargo run -p xtask -- --help
python3 tools/scenegen/gen_collision_scenes.py    # regenerates the tracked collision demo scenes + their .sha256 sidecars
```

`gen_collision_scenes.py` is deterministic and idempotent — rerunning it must
leave `git status` clean. It exists for provenance, not as a build step: the
demo scenes are tracked assets like the gate scene and the fixtures.

`bench`, `mmd-lab` and the `lab/` fixtures are frozen in place for the later
optimization phase. They still build and their unit tests still run, but
nothing they emit decides whether a change may merge — see
[Retired: performance gating](docs/05-testing.md#retired-performance-gating).

Pinned SDL3 source + crate versions: [`third_party/`](third_party/). Offline shader blobs: [`shaders/`](shaders/). Native SDL build caches stay outside Git (`MMD_NATIVE_CACHE` / `~/.cache/mmd/native`).

## Documentation

See [design documentation](docs/README.md).

## Contributing / governance

- Public contributions welcome under inbound=outbound MIT-0 + **DCO 1.1** sign-off.
- Details: [`CONTRIBUTING.md`](CONTRIBUTING.md), [`SECURITY.md`](SECURITY.md).
- No online CI (no GitHub Actions). Maintainer runs the [required merge gate](docs/05-testing.md#required-merge-gate) locally before merge.
- Manual GitHub branch rules (default branch): PR required; force-push/delete/auto-merge blocked; owner-only merge.
