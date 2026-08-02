# Millions Must Die

Offline-first PC RTS focused on fortress defense against massive enemy hordes.

## Status

Phase 0 technical prototype workspace. App CLI shells (`run`, `bench`) only; simulation and renderer land in later tickets.

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

```sh
cargo run -- run --help
cargo run -- bench --help
cargo run -p mmd-lab -- --help
cargo run -p xtask -- --help
cargo run -p xtask -- bootstrap --check
cargo run -p xtask -- shaders --check
cargo run -p xtask -- atlases --check
cargo test --workspace --locked
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
nix flake check
```

Pinned SDL3 source + crate versions: [`third_party/`](third_party/). Offline shader blobs: [`shaders/`](shaders/). Native SDL build caches stay outside Git (`MMD_NATIVE_CACHE` / `~/.cache/mmd/native`).

## Documentation

See [design documentation](docs/README.md).

## Contributing / governance

- Public contributions welcome under inbound=outbound MIT-0 + **DCO 1.1** sign-off.
- Details: [`CONTRIBUTING.md`](CONTRIBUTING.md), [`SECURITY.md`](SECURITY.md).
- No online CI (no GitHub Actions). Maintainer runs local multi-host gates before merge.
- Manual GitHub branch rules (default branch): PR required; force-push/delete/auto-merge blocked; owner-only merge.
