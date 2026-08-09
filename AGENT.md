# AGENT.md

Single context-init file for this repo. Read this first.

## Project

Millions Must Die — offline-first PC RTS for Steam, custom Rust engine.
Fortress defense against massive enemy hordes (target population cap: 500),
pixel art inspired by StarCraft: Brood War and Stronghold, Agartha-inspired
underground setting. Core pillar: mechanical RTS gameplay — clone StarCraft 1
feel before innovating. Full vision/roadmap: `docs/CONTEXT.md`.

## Status

Phase 0 (technical prototype) is closed on functional scope (`9bffc10`):
every game system has automated behavioural tests, 50k-agent scene runs
end-to-end on Linux/Vulkan. Performance/benchmarking is explicitly retired
and frozen for a later optimization phase — no perf number may gate a merge.
Branch `plan/technical-prototype` is pushed to `origin`; PR to `main` not yet
opened. Phase 1 not started. Details: `.tmp/IMPLEMENT_PROGRESS_technical-prototype.md`.

## Workspace layout

- `.` (root) — app binary crate: `cargo run -- run` (game), `bench` (frozen, non-gating). Entry `src/main.rs`.
- `crates/mmd-engine` — engine library: sim, nav (flow fields), render (SDL3/GPU sprite renderer), `runtime.rs`, `scenario.rs`, `alloc_guard.rs`, `testkit/` (headless deterministic test harness, excluded from shipping build).
- `tools/mmd-lab` — trusted local lab CLI (`doctor`, `validate`, plus frozen cross-host validation/calibration/release code). Frozen/non-gating since phase-0 close but still builds.
- `xtask` — bootstrap/reproducibility tasks: `bootstrap`, `shaders`, `atlases` (each has a `--check` mode used as a merge gate).
- `lab/` — data for lab tooling: `baselines`, `fixtures`, `goldens/` (host-scoped render goldens), `manifests`, `provision`, `releases`.
- `shaders/` — `sprite.hlsl` source plus `generated/` compiled output, checked by `xtask shaders --check`.
- `assets/` — game assets incl. tracked scenario fixtures (`assets/scenarios/fixtures/*`) with `.sha256` sidecar contracts.
- `schemas/` — JSON schemas for lab/report/baseline/calibration data formats (mostly tied to the frozen perf-lab machinery).
- `third_party/` — pinned third-party build info (`versions.toml`) — e.g. SDL3 pinned exactly, no caret floats.

## Build / test / dev commands

Merge gate (must all pass):
```sh
cargo fmt --all -- --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --all-features -- -D warnings
nix flake check
cargo run -p xtask -- bootstrap --check
cargo run -p xtask -- shaders --check
cargo run -p xtask -- atlases --check
cargo run -- run --agents 5000 --frames 300
```
- Toolchain: Rust 1.95.0 pinned via `rust-toolchain.toml`. Linux/NixOS: `nix develop` / `nix flake check`. Windows/macOS: rustup from `rust-toolchain.toml`.
- On a host with a real GPU, run tests with `MMD_REQUIRE_GPU=1` to disable the headless skip.
- Golden regeneration (explicit, reviewed): `MMD_UPDATE_GOLDEN=1 cargo test -p mmd-engine --test gpu_golden -- --ignored update_host_golden`.
- No online CI/GitHub Actions (forbidden — see `SECURITY.md`). PRs require DCO sign-off (`git commit -s`); maintainer reviews and merges only the exact tested commit hash.

## Architectural constraints (load-bearing — don't break these)

- No per-frame allocations in simulation (enforced by `alloc_guard.rs`, per-thread `MeasureGuard`).
- No per-enemy pathfinding — navigation via flow fields. Agent-agent collision is *soft separation steering* layered on top: a repulsion sum bends the descent vector, it never resolves an overlap, and no code or doc may claim agents cannot overlap.
- `mmd_engine::testkit::Harness` is the single seeded, clock-free headless entry point; `testkit` is feature-gated out of the shipping binary (`cargo tree -e features | grep -c testkit` must be 0).
- Determinism: cross-process determinism proven (test binary re-execs itself, compares hashes); seed 0 is canonical.
- Render goldens are host-scoped (`lab/goldens/<family>/`), exact-match comparison; never a cross-platform/cross-backend claim.
- Performance/benchmarking is retired for phase 0 — no perf number may gate a merge; a doc claiming a live speed number is a bug (`no_perf_claim_in_docs` test enforces this).
- Trust boundary: PR code is untrusted candidate input; `mmd-lab` only runs from trusted `main`; no GitHub Actions workflows permitted.

## Governance

`CODEOWNERS` (sole maintainer `@AronGomu`), `SECURITY.md` (pre-production trust boundary), `CONTRIBUTING.md` (MIT-0, DCO sign-off, no CLA, no online CI).

## graphify

This project has a knowledge graph at `graphify-out/` with god nodes, community structure, and cross-file relationships.

Rules:
- For codebase questions, first run `graphify query "<question>"` when `graphify-out/graph.json` exists. Use `graphify path "<A>" "<B>"` for relationships and `graphify explain "<concept>"` for focused concepts. These return a scoped subgraph, usually much smaller than GRAPH_REPORT.md or raw grep output.
- If `graphify-out/wiki/index.md` exists, use it for broad navigation instead of raw source browsing.
- Read `graphify-out/GRAPH_REPORT.md` only for broad architecture review or when query/path/explain do not surface enough context.
- After modifying code, run `graphify update .` to keep the graph current (AST-only, no API cost).

## Glossary

Read and activate `.claude/skills/make-glossary-aron/SKILL.md` — maintains `docs/GLOSSARY.md`, shared vocabulary between user and agents.

## Directories

- `docs/` : Project documentation. Contains CONTEXT.md, DESIGN.md, GLOSSARY.md, 05-testing.md, ADR/.
- `.dev/` : Future implementation resources. Contains bugs.md, feedback.md, ideas.md, decisions/.
- `ai-artifacts/` : Documents generated by agents.
