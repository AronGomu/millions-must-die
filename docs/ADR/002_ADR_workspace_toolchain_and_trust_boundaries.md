# ADR 002: Workspace, Toolchain + Trust Boundaries

- Status: Accepted
- Date: 2026-08-02
- Superseded by: —

## Context

Current repo = one Rust binary scaffold. Prototype needs engine code, app modes, reproducible build tools, local multi-host validation. Public PR code must not control coordinator/reset path.

## Decision

Cargo workspace keeps root package as app. Members:

- Root `millions_must_die`: one binary; `run` + `bench`.
- `crates/mmd-engine`: scenario, nav, sim, renderer, benchmark core.
- `tools/mmd-lab`: trusted local coordinator.
- `xtask`: SDL/shader/atlas bootstrap + reproducibility checks.

Trust rules:

- `mmd-lab` installed from trusted `main`.
- Candidate archive never supplies executed coordinator/reset code.
- Engine/app candidate remains untrusted input.
- Root `src/main.rs` retained; no relocation/delete needed.

Toolchain:

- Exact Rust `1.95.0` via `rust-toolchain.toml`.
- Nix flake for Linux/NixOS dev.
- `rustup` bootstrap on Windows/macOS.
- `Cargo.lock` committed.
- Native SDL/shader tool versions + digests pinned.
- No network fetch in Cargo `build.rs`.

## Consequences

Positive:

- Engine testable without window.
- One player-facing dev binary.
- Trusted coordinator boundary explicit.
- Existing root scaffold evolves surgically.

Negative:

- Multiple packages increase initial config.
- Trusted lab install/update needs operator process.
- Nix + rustup produce 2 bootstrap paths.

## Rejected alternatives

- One binary for app/bench/matrix/bootstrap: candidate could modify trusted lab path.
- Shell coordinator: cross-platform fragmentation.
- Virtual workspace moving root app: needless file churn.
- Floating stable Rust: compiler drift contaminates perf baselines.

## Validation

- Workspace metadata test.
- CLI help contract tests.
- `cargo test --workspace --locked`.
- `nix flake check`.
- Trusted lab version/source hash printed in every report.

## References

- `docs/01-technical-architecture.md`
- `docs/technical-prototype-architecture.html`
- `docs/local-validation-lab-architecture.html`
