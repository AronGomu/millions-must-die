# Millions Must Die

Offline-first PC RTS focused on fortress defense against massive enemy hordes.

## Status

Phase 0 technical prototype: **closed on functional scope — the game systems are proven to behave correctly.**

The 50k-agent flow-field scene runs on Linux/Vulkan. Phase 0 closed on automated behavioural tests of every game system, not on speed: **performance is unmeasured and gates nothing**, and no verification exists for any platform other than the development host. Frame-time gating, the cross-platform matrix, and the multi-host validation lab are retired to a later optimization phase on the finished game — the code stays in-tree, frozen and non-gating.

What phase 0 proves, what it does not, and every known gap: [docs/technical-prototype-functional-close.md](docs/technical-prototype-functional-close.md). What gates a merge: [docs/05-testing.md](docs/05-testing.md). Earlier measurements, kept as history and claiming nothing: [docs/technical-prototype-results.md](docs/technical-prototype-results.md) (superseded).

## License

Authored project code, docs, shaders, and generated placeholder assets: **[MIT-0](LICENSE)**.

Name and logo are reserved — see [`TRADEMARKS.md`](TRADEMARKS.md). Third-party dependencies keep their own terms.

## Workspace

| Path | Role |
| --- | --- |
| `.` (`millions_must_die`) | App binary: `run`, `bench` |
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
cargo run -- run --agents 50000 --frames 300
cargo run -- run --scenario assets/scenarios/collision_mid_v1.ron --frames 300
cargo run -- run --scenario assets/scenarios/collision_sprite_v1.ron --frames 300
```

### Developer tools (not gates)

```sh
cargo run -- run --help
cargo run -- bench --help    # benchmark harness — not a gate; optimization phase
cargo run -p mmd-lab -- --help
cargo run -p xtask -- --help
```

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
