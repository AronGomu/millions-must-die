# AGENTS.md

Single context-init file for this repo. Read this first.

## Project

Millions Must Die — offline-first PC RTS for Steam, custom Rust engine.
Fortress defense against massive enemy hordes (target population cap: 500),
pixel art inspired by StarCraft: Brood War and Stronghold, Agartha-inspired
underground setting. Core pillar: mechanical RTS gameplay — clone StarCraft 1
feel before innovating. Full vision/roadmap: `docs/CONTEXT.md`.

## Status

Phase 0 (technical prototype) is closed on functional scope (`9bffc10`):
every game system has automated behavioural tests, the 5 000-agent gate scene
runs end-to-end on Linux/Vulkan (5 000 = `scenario::MAX_LIVE_AGENTS`).
Performance/benchmarking is explicitly retired and frozen for a later
optimization phase — no perf number may gate a merge.
Branch `plan/technical-prototype` is pushed to `origin`; PR to `main` not yet
opened.
Phase 0.5 (`plan/horde-sim-headroom`) adds three scenario-gated simulation
knobs — `separation_phases`, `mass_class_count`, `separation_threads` — each
defaulting to the identity value 1; see ADR 010 and ADR 011.

Phase 1 (RTS engine prototype, branch `plan/rts-engine-prototype`) is closed on
functional scope: camera, selection, workers, economy, building and unit
production ship as a thin vertical slice on the horde-free `rts_prototype_v1`
scene (320 × 320, `hard_agent_count: 0`), driven by the new `rts` subcommand.
Phase 0's contracts are untouched — same exit-line hash, same render golden,
same `atlas_count: 4` scenario contract. No combat, no enemy AI, no zoom, no
balance pass, and performance is still unmeasured. What it proves and does not:
`docs/rts-engine-prototype-functional-close.md`; decisions in ADR 013, 014, 015.

Phase 1.1 (interaction/UI/audio hardening, branch
`plan/rts-interaction-ui-audio-hardening`) is closed on functional scope:
visible pick geometry, hard RTS bodies (radius 3 cells) with radius-aware
static navigation and formations, persistent settings, three window modes, an
aspect-fit logical canvas, a projected camera frontier, a StarCraft-shaped HUD
with minimap and command card, a pause/settings menu, and deterministic
generated audio. Same 1,600-frame gate smoke, now driving all of it. Hard
collision is **RTS-only**; the horde keeps ADR 009's soft separation. Nothing
on the gate claims a window appeared or a sound was heard — those are human
checklist items (`artifacts/manual_test_checklist.md`). What it proves and
does not: `docs/rts-interaction-ui-audio-hardening-functional-close.md`;
decisions in ADR 016–020.

Feedback polish (branch `plan/rts-feedback-polish`) extends phase 1.1 on user
feedback: framed control states with a `MENU`/`CLOSE MENU` pair, live sliders
and typed numeric settings fields, per-bus mute labels, a scrolled settings
body, a persisted default-on world grid, assisted building placement, a
sprite ∪ footprint building pick with an exact-line card (six lines then, seven
since phase 2 added HP), positional QWE/ASD/ZXC command keys, an exact
pure-green drag box, and an order-scoped gather-worker
collision policy. Decision: ADR 021, which narrows ADR 017's invariant (see
the collision constraint below) and amends ADR 018–020 forward. What it proves
and does not — including the `rts_economy` node-click regression this branch
introduced at `af16e7c` and then fixed by ranking exact pickshapes above a
building's sprite quad: `docs/rts-feedback-polish-functional-close.md`.

Phase 2 (combat prototype, branch `plan/combat-prototype`) is closed on
functional scope: an enemy faction (`OWNER_ENEMY = 1`, melee `UnitKind::Ghoul`)
spawns from scenario waves into the gate scene — 400 across the scripted run,
not the phase-3 horde — marches on **one** objective cell over one shared
pooled flow field and melees the first player thing in range; `Idle` and
`AttackMove` auto-acquire, plain `Move`/`Gather`/`Build`/`Follow` never fire;
damage is instant-hit `max(1, damage - armor)` with no projectile entities;
units despawn, buildings (HQ included) are destructible with
queue-cancel-no-refund, and a worker-built Turret (75 crystal, no supply)
auto-fires — silently, a named gap. The same branch folded in feedback round 2:
an 8-cell build grid buildings snap to while units keep moving in true cells, a
bounded `STALLED_SITE_TICKS = 180` give-up for a site that can never evacuate,
order status text, target rings, move markers with dashed bearings, follow
orders, entity rally points, and an untimed sandbox scene deliberately kept off
the gate. Enemies are ordinary hard bodies under the ADR 021 policy; the gather
exception is not widened and the horde's soft separation is untouched. Sandbox
— no win/lose: the outcome rides five exit tokens (`kills`, `losses`,
`enemies_spawned`, `first_combat_tick`, `hq_alive`) pinned by the tracked
`assets/scenarios/rts_combat_v1.script`, and both phase-1 scripts plus the
scene sha256 were re-baselined (the phase-1 close docs describe the pre-enemy
runs). The scripted defence loses on its pinned numbers and that is recorded,
not tuned away. What it proves and does not:
`docs/combat-prototype-functional-close.md`; decisions in ADR 022–025.

## Workspace layout

- `.` (root) — app binary crate: `cargo run -- run` (game), `bench` (frozen, non-gating). Entry `src/main.rs`.
- `crates/mmd-engine` — engine library: sim, nav (flow fields, `nav/field_pool.rs`), render (SDL3/GPU sprite renderer, `render/camera.rs`, `render/text.rs`), `runtime.rs`, `scenario.rs`, `alloc_guard.rs`, `testkit/` (headless deterministic test harness, excluded from shipping build).
- `crates/mmd-engine/src/rts/` — the RTS world: `entity.rs` (SoA store, generational ids), `orders.rs`, `economy.rs`, `build.rs`, `production.rs`, `selection.rs` (pick geometry), `combat.rs` (per-kind weapon table + surface-distance range rule; the combat system itself lives in `world.rs`), `static_nav.rs` (body-inflated navigation mask + sweeps), `collision.rs`, `formation.rs`, `pack.rs`, `hud.rs`, `minimap.rs` (minimap projection + `hud_hit_test`), `world.rs` (`RtsWorld::tick`'s fixed system order). Separate from `sim/`, which stays frozen.
- `src/rts_*.rs` — the app's `rts` subcommand: input table, script injection (incl. the `quit` token), overlay, run loop, plus phase-1.1's app-side halves: `rts_settings.rs` (persisted schema-1 settings), `rts_window.rs` (window modes, focus, pointer grab), `rts_ui.rs` (pause/settings menu FSM), `rts_feedback.rs` (audio events + buses), `rts_audio.rs` (SDL audio sink).
- `assets/audio/generated/` — seven generated MIT-0 placeholder WAVs + manifest, produced and verified by `cargo run -p xtask -- audio [--check]`. No copyrighted audio may enter this repo.
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
cargo run -- rts --frames 4500 --inject-input-file assets/scenarios/rts_combat_v1.script
```
- DCO: `./scripts/check-dco TRUSTED_BASE_SHA EXACT_CANDIDATE_SHA` on candidate range only; substitute real SHAs for the two tokens; see `docs/05-testing.md`.
- The `rts` line is the interactive RTS smoke: one tracked script drives select → gather → build (command card) → produce → minimap jump → pause menu → settings edit → `quit`, and asserts an exit line carrying `body_overlaps=0 ui_page=gameplay music_starts=1 voice_select=8 voice_order=9 voice_reject=1 sfx_ui=8 keyboard_pan=78`, then the feedback-polish tokens `settings_scroll_px=<n> show_grid=<bool>`. A second tracked script, `assets/scenarios/rts_feedback_polish_v1.script`, drives the chrome path (menu → positional `q` → Settings → slider → grid → wheel → Back → Close Menu) and is asserted by `tests/rts_acceptance.rs`. `audio --check` regenerates every tracked WAV + manifest and byte-compares. The combat line is the phase-2 smoke: `assets/scenarios/rts_combat_v1.script` drives economy → forward Turret → Barracks → rallied Soldiers → attack-move into a marching wave, and asserts the combat tokens `kills=2 losses=5 enemies_spawned=400 first_combat_tick=3691 hq_alive=1` on the exit line. Single source of truth for the gate: `docs/05-testing.md`.
- Toolchain: Rust 1.95.0 pinned via `rust-toolchain.toml`. Linux/NixOS: `nix develop` / `nix flake check`. Windows/macOS: rustup from `rust-toolchain.toml`.
- On a host with a real GPU, run tests with `MMD_REQUIRE_GPU=1` to disable the headless skip. Known and pre-existing: two tests that deliberately force `SDL_VIDEODRIVER=dummy` (`dummy_driver_run_does_not_touch_settings`, `no_rts_run_creates_the_real_user_config`) fail while that variable is set, because `or_skip` turns their intended `EXIT_NO_GPU` skip into a failure. The documented gate command, without the variable, is green.
- Golden regeneration (explicit, reviewed): `MMD_UPDATE_GOLDEN=1 cargo test -p mmd-engine --test gpu_golden -- --ignored update_host_golden`.
- No online CI/GitHub Actions (forbidden — see `SECURITY.md`). PRs require DCO sign-off (`git commit -s`); maintainer reviews and merges only the exact tested commit hash.

## Architectural constraints (load-bearing — don't break these)

- No per-frame allocations in simulation (enforced by `alloc_guard.rs`, per-thread `MeasureGuard`).
- No per-enemy pathfinding — navigation via flow fields. **This extends to player units**: they descend a pooled field from `nav::FieldPool` (`NAV_FIELD_SLOTS = 8`, exact LRU, ties to the lowest slot), never a per-unit path. Formation slots are bounded terminal steering around one anchor field, not pathfinding.
- Collision is **two different contracts, and every doc must name which**. Horde `sim/`: *soft separation steering* — a repulsion sum bends the descent vector, never resolves an overlap, and no code or doc may claim horde agents cannot overlap (ADR 009).
- The other contract: `rts/` **hard bodies** — radius 3 cells, contact distance 6, sequential proposal/commit with a continuous sweep, plus bounded push-aside (`MAX_PUSH_DEPTH = 3`, `MAX_PUSHED_BODIES = 8`, one push per body per tick, all-or-nothing) and a ±45°/±90° deflection fallback.
- The RTS invariant is **narrow, and never state it unconditionally** (ADR 021 narrowing ADR 017): a tick may leave two RTS unit bodies merged only when both are active gather workers, or when that exact pair is inside its bounded 12-attempt gather-exit transition. Every other pair is repaired, or counted by `body_overlaps` and reported through `TickError::UnrepairableOverlap` — so `body_overlaps=0` is a *policy* claim, not a raw-geometry one. Static terrain, map edges, nodes and finished buildings stay hard for everyone, gathering workers included.
- Supply is **reserved at enqueue**, never charged at completion, and `Supply::used` is **recomputed** every tick from live units plus queue reservations — never incremented at a call site.
- Render layers: `ScenePass::overlay` is depth-off and binds texture slot 0, so it is honest **only for procedural rings**. Every textured depth-off element — placement tiles, drag box, rally flag, icons, panel fill, glyphs — must be a `ui` draw group, or it samples the zombie atlas.
- `sim/` is frozen for phase 1. Its entire phase-1 diff is two visibility keywords (`dir_from_vector` → `pub`, `step_admissible` → `pub(crate)`) plus one `#[cfg(test)]` re-export; the 5 000-agent exit-line hash must not move.
- Seeded, clock-free headless entry points: `mmd_engine::testkit::Harness` for the horde sim and `mmd_engine::testkit::RtsHarness` for the RTS world — those two and no others. `testkit` is feature-gated out of the shipping binary (`cargo tree -e features | grep -c testkit` must be 0).
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
- `.dev/` : Future implementation resources. Contains bugs.md, feedback.md, ideas.md, decisions/. Round-2 user feedback (the phase-2 items) lives in the repo-root `feedback.md`, not here.
- `artifacts/` : Documents generated by agents.
