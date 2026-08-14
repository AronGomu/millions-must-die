# T2: Repair-disabled invariants and hash equality

**Plan:** `./ai_artefacts/PLAN_2026_08_14_audit_issue_9_shipping_overlap_repair.md`  
**Depends:** T1  
**Commit outcome:** the seed, movement, production and construction paths are pinned by tests that run with the overlap-repair pass never armed — 0 repair passes, 0 body overlaps, every tick — and a repair-armed run of the same world hashes identically to a repair-disabled one; ADR 017 and the phase-1.1 close doc say what the code now does.

## Context (self-contained)

- Goal: `RtsWorld::movement` used to run `repair_body_overlaps` — an N×N penetration search — on every shipping tick, although nothing in the game can produce a penetration; only the testkit hooks can. T1 gated the pass out of the shipping build and behind an armed flag in testkit builds. This slice proves the gate is safe: no normal path ever needed the pass, and having it armed changes no state.
- This slice: repair-disabled invariants across `rts_collision`, `rts_production`, `rts_build`; the armed-vs-disabled state-hash equality proof; the ADR/close-doc updates. No source behaviour change.
- Out of scope here: any further change to `crates/mmd-engine/src/rts/world.rs` behaviour (T1 owns the mechanism; this ticket edits **no** `src/` file); collision/push/deflection redesign; the `body_penetrates_any` call in `step_one_unit` (`world.rs:2536`, a shipping arrival check that stays); horde sim; any wall-clock or perf-number assertion; adding commands to the merge-gate lists in `docs/05-testing.md` / `README.md`; renaming or deleting any existing test.
- Assumptions in force:
  - The work proof is a deterministic count of executed repair passes, never a timing. No perf number may gate a merge in this repo.
  - "Repair-capable vs repair-disabled" is expressed inside one binary: arming the pass before every tick vs never arming it. The two runs must produce byte-identical `state_hash`.
  - Repositioning a body onto its own current position is the arming move: `RtsWorld::force_position_for_test` calls `EntityStore::set_position` (`pub(crate)`, `crates/mmd-engine/src/rts/entity.rs:358-360`, delegating to `set_position_impl` at `:362-366`), which writes `x`/`y` and nothing else — so an identity reposition changes no state and no hash.
  - Doc edits stay inside ADR 017's amendment convention (`6bf6e9a` amended it the same way) and the close doc's existing coverage rows.

### What T1 left behind (spelled out — do not go read T1)

In `crates/mmd-engine/src/rts/world.rs`:

- Two `#[cfg(feature = "testkit")]` fields on `RtsWorld`: `repair_armed: bool` and `repair_runs: u64`, initialised `false` / `0`.
- `#[cfg(feature = "testkit")] pub fn overlap_repair_runs(&self) -> u64` — the count of overlap-repair passes this world has run.
- `pub fn entities_mut(&mut self) -> &mut EntityStore` and `pub fn force_position_for_test(&mut self, id: EntityId, pos: [f32; 2]) -> bool` (both `#[cfg(feature = "testkit")]`) each set `repair_armed = true`. `force_position_for_test` arms only when the id resolves, and returns `false` for a stale id.
- `fn movement` now reads: `collect_unit_bodies()`, then `self.last_tick_error = None;`, then `#[cfg(feature = "testkit")] if self.repair_armed { self.repair_body_overlaps(); if self.last_tick_error.is_some() { return; } self.repair_armed = false; }`, then the unchanged rotated proposal/commit loop. So an unrepairable overlap keeps the flag armed and is re-reported every tick.
- `fn repair_body_overlaps` is `#[cfg(feature = "testkit")]`, increments `self.repair_runs` as its first statement, and no longer clears `last_tick_error`.
- `pub fn last_tick_error(&self) -> Option<TickError>` is unchanged and un-gated; in a shipping build it is always `None`.
- `pub fn body_overlap_count(&self) -> u32` (`world.rs:724`) is unchanged: the shared O(n²) body-safety oracle, an observation seam, never a per-tick path.

In `crates/mmd-engine/tests/rts_collision.rs`: `mod common; use common::{ONE_FREE_CENTRE, one_free_centre_spec};` is already present, `TickError` is already imported, `forced_overlap_is_repaired` already asserts `overlap_repair_runs() == 1`, and `an_unrepairable_overlap_is_reported_on_every_tick` already exists. The file declares **18** tests after T1 (`running 18 tests` → `17 passed; 0 failed; 1 ignored`; the ignored one is the child-process fixture `print_collision_hash_for_child_process`).

### Verified facts this ticket relies on (tree at `af728fe` plus T1)

- Test counts before this ticket: `rts_collision` 18, `rts_build` 42, `rts_production` 44, `rts_radius_nav` 10, `rts_acceptance` 7, root `validation_contract` 14. This ticket adds 2 + 1 + 1 → `rts_collision` **20**, `rts_build` **43**, `rts_production` **45**; the rest are unchanged.
- The canonical RTS collision state hash is `7e40af444dc7010a1b910c3e852fad8f4ffa76f0d108004043b94d27504f6b8e` and must not move.
- A bare short test name resolves under `--exact` in these binaries: they declare their tests at file top level with no wrapping `mod`, so libtest registers `production_never_runs_overlap_repair`, not `some_mod::production_never_runs_overlap_repair`. The `running 1 test` line in each validation row below is therefore the real check — a `running 0 tests` line means the name drifted, and it still exits 0.
- `crates/mmd-engine/tests/rts_collision.rs` helpers already in the file: `harness(spawns)`, `scattered_spawns(n)`, `workers(&h)`, `pos(&h, id)`, `dist(a, b)`, `assert_no_overlap(&h, ctx)`, `const RADIUS`, `const DIAMETER`, `const W`/`H`, and `fn canonical_hash() -> String` (4 workers from `scattered_spawns(4)`, `order_move_group` to `Cell { x: 20, y: 20 }`, `step_exact(400)`, `state_hash_hex()`), which the cross-process determinism test uses.
- `crates/mmd-engine/tests/rts_build.rs` already has `const CLEAR_CORNER: Cell = Cell { x: 180, y: 176 };` (`:30`) and `fn assert_every_body_is_legal(h: &RtsHarness)` (`:829`), and imports `BuildingKind`. A Depot's footprint is 8×8, so `CLEAR_CORNER` covers cells `180..188 × 176..184`. `completion_evacuates_every_overlapping_body` (`:852`) documents that the builder ends up standing against that footprint's east edge, i.e. the plain build path already exercises completion evacuation without any raw spawn.
- `crates/mmd-engine/tests/rts_production.rs` already imports `WORKER_PRODUCE_TICKS`, `EntityKind`, `UnitKind`, and has `fn unit_positions(h)` (`:739`); `new_spawn_joins_same_tick_collision` (`:957`) shows the enqueue form `h.world_mut().enqueue_unit(hq, UnitKind::Worker).is_ok()` and `h.world().start_hq().expect("hq")`, and asserts the tracked scene seeds 6 workers (`:961`). `WORKER_PRODUCE_TICKS == 300` (`rts_production.rs:72`), so R3's `2 * WORKER_PRODUCE_TICKS + 240` = 840 ticks clears two sequential heads with room to spare — the sibling above needs `WORKER_PRODUCE_TICKS + 60` for one.
- `RtsWorld::order_gather_group(&mut self, ids: &[EntityId], node: EntityId) -> Result<usize, FormationError>` (`crates/mmd-engine/src/rts/world.rs:1163`), so `.is_ok()` is the right check. `EntityKind::Node(ResourceKind)` (`crates/mmd-engine/src/rts/entity.rs:69`) is the node kind; `rts_acceptance.rs:270` shows the exact `h.ids_of_kind(EntityKind::Node(ResourceKind::Crystal))[0]` form.
- `RtsHarness::step_exact` is a plain `for _ in 0..ticks { self.world.tick(); }` (`crates/mmd-engine/src/testkit/rts.rs:87-92`), so R2's 400 × `step_exact(1)` is tick-for-tick identical to one `step_exact(400)` — which is what lets the armed run be compared against `canonical_hash()`.
- Nothing in `crates/mmd-engine/src/` or `src/` calls `entities_mut` or `force_position_for_test`; the only callers are integration tests. That is why seeding, movement, production and construction can assert `overlap_repair_runs() == 0`.
- `crates/mmd-engine/tests/rts_build.rs` already declares `mod common;` (`:802`), and `rts_collision.rs` gained it in T1 — neither needs a new one here.
- The tracked scene (`assets/scenarios/rts_prototype_v1.ron`) seeds 6 workers, `start_crystal: 300`, `start_gas: 100`, `start_supply_cap: 10` — room for exactly 2 more produced workers (8 ≤ 10) at 50 crystal each.
- `tests/validation_contract.rs::phase1_1_close_names_only_real_tests` walks every backticked test-looking name in `docs/rts-interaction-ui-audio-hardening-functional-close.md` and fails if the named test does not exist in the binary its row names, or is `#[ignore]`d. Adding real, un-ignored names to a row is safe; renaming an existing one is not.

## Requirements

### R1 — repair-disabled invariant on seed + movement

Add to `crates/mmd-engine/tests/rts_collision.rs`, immediately after `an_unrepairable_overlap_is_reported_on_every_tick` and **before** the `// Determinism` banner. Extend the `mmd_engine::rts` import list with `ResourceKind` first, so it reads:

```rust
use mmd_engine::rts::{
    EntityId, EntityKind, OWNER_PLAYER, Order, RTS_UNIT_BODY_DIAMETER_CELLS,
    RTS_UNIT_BODY_RADIUS_CELLS, ResourceKind, TickError, UnitKind, moving_circle_hits_point,
    units_overlap,
};
```

```rust
// ---------------------------------------------------------------------------
// The repair pass is not part of the game
// ---------------------------------------------------------------------------

/// The tracked scene, seeded and then walked as a gathering group, never arms
/// the overlap-repair pass — and never merges two bodies either.
///
/// This is the invariant the shipping build depends on: it compiles no repair
/// pass at all, so if seeding or movement could hand the world a penetration,
/// nothing would clear it. `overlap_repair_runs() == 0` is the proof that the
/// pass never had anything to do here; `body_overlap_count() == 0` is the
/// proof that this is because there was no overlap, not because the oracle is
/// blind.
#[test]
fn movement_never_runs_overlap_repair() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let group = workers(&h);
    assert_eq!(group.len(), 6, "the tracked scene seeds six workers");
    assert_eq!(
        h.world().body_overlap_count(),
        0,
        "the seed path handed the world a merged pair"
    );
    assert_eq!(
        h.world().overlap_repair_runs(),
        0,
        "seeding must not need the repair pass"
    );

    let node = h.ids_of_kind(EntityKind::Node(ResourceKind::Crystal))[0];
    assert!(
        h.world_mut().order_gather_group(&group, node).is_ok(),
        "the whole group must take the gather order"
    );

    for tick in 1..=600u64 {
        h.step_exact(1);
        assert_eq!(
            h.world().body_overlap_count(),
            0,
            "tick {tick} ended with a merged pair"
        );
        assert_eq!(
            h.world().overlap_repair_runs(),
            0,
            "tick {tick} ran the overlap repair pass on a normal movement path"
        );
    }
}
```

### R2 — armed vs disabled: identical state hash

Add directly after R1's test, still before the `// Determinism` banner:

```rust
/// A repair-capable run and a repair-disabled run of the same world end in the
/// same state, hash for hash.
///
/// Arming the pass on a valid world is a no-op — that is exactly why the
/// shipping build may omit it. The armed run re-arms before every tick (an
/// identity reposition writes the body's own coordinates back, which changes
/// no state), so the only difference between the two worlds is whether the
/// pass ran at all: 400 times against 0.
#[test]
fn arming_overlap_repair_on_a_valid_world_changes_no_state() {
    fn contended_run() -> RtsHarness {
        let mut h = harness(scattered_spawns(4));
        let w = workers(&h);
        assert_eq!(w.len(), 4);
        assert_eq!(
            h.world_mut().order_move_group(&w, Cell { x: 20, y: 20 }),
            Ok(4)
        );
        h
    }

    let mut disabled = contended_run();
    disabled.step_exact(400);
    assert_eq!(
        disabled.world().overlap_repair_runs(),
        0,
        "a run that touches no test hook must never run the repair pass"
    );

    let mut armed = contended_run();
    let first = workers(&armed)[0];
    for _ in 0..400 {
        let p = pos(&armed, first);
        assert!(
            armed.world_mut().force_position_for_test(first, p),
            "the arming reposition must accept a live id"
        );
        armed.step_exact(1);
    }
    assert_eq!(
        armed.world().overlap_repair_runs(),
        400,
        "the armed run must have run the pass on every tick"
    );

    assert_eq!(
        disabled.state_hash_hex(),
        armed.state_hash_hex(),
        "the repair pass changed a valid world's state; the shipping build \
         omits it, so it must change nothing"
    );
    assert_eq!(
        disabled.state_hash_hex(),
        canonical_hash(),
        "the repair-disabled run must still be the canonical contended run the \
         cross-process determinism test hashes"
    );
}
```

### R3 — repair-disabled invariant on the production path

Add to `crates/mmd-engine/tests/rts_production.rs`, immediately after `new_spawn_joins_same_tick_collision` (ends `:993`):

```rust
/// Production places a finished unit itself, on the nearest free legal body
/// centre, so a producing run never needs the overlap-repair pass — which the
/// shipping build does not compile at all.
#[test]
fn production_never_runs_overlap_repair() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let hq = h.world().start_hq().expect("hq");
    assert_eq!(
        h.world().body_overlap_count(),
        0,
        "the scene starts clean"
    );
    assert_eq!(
        h.world().overlap_repair_runs(),
        0,
        "seeding must not need the repair pass"
    );
    assert!(h.world_mut().enqueue_unit(hq, UnitKind::Worker).is_ok());
    assert!(h.world_mut().enqueue_unit(hq, UnitKind::Worker).is_ok());

    for tick in 1..=(2 * WORKER_PRODUCE_TICKS as u64 + 240) {
        h.step_exact(1);
        assert_eq!(
            h.world().body_overlap_count(),
            0,
            "tick {tick} ended with a merged pair"
        );
        assert_eq!(
            h.world().overlap_repair_runs(),
            0,
            "tick {tick} ran the overlap repair pass on the production path"
        );
    }
    assert_eq!(
        h.ids_of_kind(EntityKind::Unit(UnitKind::Worker)).len(),
        8,
        "both queued workers must have been produced, or this run proved nothing"
    );
}
```

### R4 — repair-disabled invariant on the construction path

Add to `crates/mmd-engine/tests/rts_build.rs`, immediately after `completion_evacuates_every_overlapping_body` (ends `:894`):

```rust
/// Construction places bodies itself — the builder walks in under the movement
/// system, and completion evacuates whatever the finished footprint would
/// swallow — so a build run never needs the overlap-repair pass, which the
/// shipping build does not compile at all.
///
/// The sibling above owns the evacuation *behaviour*; it forces its extra
/// bodies in through the raw store hook, which is exactly what arms the repair
/// pass, so it cannot make this claim. This one drives the plain path: one
/// builder, one Depot, no test hook at all.
#[test]
fn a_completion_never_runs_overlap_repair() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let builder = first_worker(&h);
    assert_eq!(
        h.world().overlap_repair_runs(),
        0,
        "seeding must not need the repair pass"
    );

    assert!(h.world_mut().begin_placement(BuildingKind::Depot));
    let site = h
        .world_mut()
        .confirm_placement(CLEAR_CORNER, builder)
        .expect("confirm");

    for tick in 1..=2_000u64 {
        h.step_exact(1);
        assert_eq!(
            h.world().body_overlap_count(),
            0,
            "tick {tick} ended with a merged pair"
        );
        assert_eq!(
            h.world().overlap_repair_runs(),
            0,
            "tick {tick} ran the overlap repair pass on the construction path"
        );
    }

    assert!(!h.world().is_site(site), "the Depot never finished");
    assert_every_body_is_legal(&h);
}
```

### R5 — ADR 017 amendment

Append to the end of `docs/ADR/017_ADR_rts_hard_collision_navigation_and_formations.md` (currently 220 lines; add a blank line, then):

```md
## Amendment (F10: the overlap repair pass is testkit-only)

The movement system's phase 2 — "repair any penetration the world was handed" —
ran on **every** tick, scanning every ordered pair of live unit bodies before a
single unit moved. It was unreachable work by this ADR's own rules: seeding,
movement, the push chain, production placement and completion evacuation all
place bodies body-safely, and the decision above already states that raw store
mutation is crate-private/testkit-only. The only two mutations that can hand
the world a merged pair are `RtsWorld::force_position_for_test` and
`RtsWorld::entities_mut`, and neither exists without the `testkit` feature.

The pass is therefore `#[cfg(feature = "testkit")]` and, inside a testkit
build, runs only on ticks where one of those two hooks armed it
(`repair_armed`). Arming is sticky until a pass ends clean, so an overlap the
grid cannot repair still stashes `TickError::UnrepairableOverlap` and is
re-reported every tick instead of going quiet after one. `last_tick_error` is
now cleared at the top of `movement` rather than inside the pass, so it still
describes the last tick and never an older one in a build that has no pass at
all; in a shipping build it is always `None`.

What this does not change: the hard-body invariant, its induction, the
no-epsilon rule, the arrival check's own `body_penetrates_any` call (a
per-mover check inside `step_one_unit`, not a per-tick pair scan), and every
valid-state hash — a repair-capable run and a repair-disabled run of the same
contended world hash identically
(`rts_collision::arming_overlap_repair_on_a_valid_world_changes_no_state`).

The claim is pinned by deterministic work counts, never timings:
`RtsWorld::overlap_repair_runs()` counts executed passes, and the seed,
movement, production and construction runs assert it stays `0`
(`rts_collision::movement_never_runs_overlap_repair`,
`rts_production::production_never_runs_overlap_repair`,
`rts_build::a_completion_never_runs_overlap_repair`), while
`rts_collision::forced_overlap_is_repaired` and
`rts_collision::an_unrepairable_overlap_is_reported_on_every_tick` keep the
forced-overlap behaviour green.
```

### R6 — phase-1.1 close doc coverage rows

`docs/rts-interaction-ui-audio-hardening-functional-close.md`, coverage table. Append names to the end of the existing name list in three rows, changing nothing else on those lines:

- row `hard bodies — contact, sweep, push chain, deflection` (`:93`, file `crates/mmd-engine/tests/rts_collision.rs`): append `` , `an_unrepairable_overlap_is_reported_on_every_tick`, `movement_never_runs_overlap_repair`, `arming_overlap_repair_on_a_valid_world_changes_no_state` ``
- row `body-safe production — nearest free spawn, wait and resume` (`:96`, file `crates/mmd-engine/tests/rts_production.rs`): append `` , `production_never_runs_overlap_repair` ``
- row `body-safe construction — atomic evacuation or wait` (`:97`, file `crates/mmd-engine/tests/rts_build.rs`): append `` , `a_completion_never_runs_overlap_repair` ``

Do not rename or remove any name already in those rows.

## Inputs

- Files to read: `crates/mmd-engine/tests/rts_collision.rs` (helpers `harness` `:70`, `scattered_spawns` `:82`, `workers` `:91`, `pos` `:95`, `assert_no_overlap` `:107`; `canonical_hash` at `:609`, in the Determinism section — Rust item order is free, so the two new tests may sit above it and still call it), `crates/mmd-engine/tests/rts_production.rs:939-993`, `crates/mmd-engine/tests/rts_build.rs:800-895`, `docs/ADR/017_...md` (tail), `docs/rts-interaction-ui-audio-hardening-functional-close.md` (coverage table).
- **From Depends (T1):** the API and behaviour listed under "What T1 left behind" above — in particular `RtsWorld::overlap_repair_runs() -> u64` (`#[cfg(feature = "testkit")]`), `repair_armed` arming in `entities_mut` and `force_position_for_test`, the `#[cfg(feature = "testkit")] if self.repair_armed { ... }` block in `movement`, and `self.repair_runs += 1;` as the first statement of `repair_body_overlaps`.

## TDD

1. **Red** — write R1-R4's four tests first. They are new claims about landed behaviour, so prove they can fail before trusting them: temporarily change `if self.repair_armed {` to `if true {` in `crates/mmd-engine/src/rts/world.rs` (`fn movement`), run the four tests, and check each fails on its `overlap_repair_runs()` assertion — R1/R3/R4 blow up on their first loop iteration (`left: 1, right: 0`), R2 on `disabled.world().overlap_repair_runs()` (`left: 400, right: 0`). The two forced-overlap tests (`forced_overlap_is_repaired`, `an_unrepairable_overlap_is_reported_on_every_tick`) keep passing under that edit, because it restores exactly today's always-on behaviour — so the four new tests must be the *only* failures. **Revert that edit** with `git checkout -- crates/mmd-engine/src/rts/world.rs` before going on; `git diff --stat crates/mmd-engine/src/rts/world.rs` must then print nothing.
2. **Green** — with the revert in place, the four tests pass unchanged. No `src/` edit is part of this ticket.
3. **Refactor** — none. Add R5 and R6 doc text, then re-run `validation_contract`.

## Test plan

| Test | Input | Expect |
| --- | --- | --- |
| `rts_collision::movement_never_runs_overlap_repair` | tracked scene, 6 workers on a gather group order, 600 single ticks | `body_overlap_count() == 0` and `overlap_repair_runs() == 0` at seed and after every tick |
| `rts_collision::arming_overlap_repair_on_a_valid_world_changes_no_state` | 4 contended workers to one cell, 400 ticks, once unarmed and once armed before every tick | counters 0 vs 400; both `state_hash_hex()` equal, and equal to `canonical_hash()` |
| `rts_production::production_never_runs_overlap_repair` | tracked scene, 2 workers queued at the HQ, 840 single ticks | counters stay 0, no overlap on any tick, 8 workers alive at the end |
| `rts_build::a_completion_never_runs_overlap_repair` | tracked scene, one builder, Depot confirmed at `CLEAR_CORNER`, 2 000 single ticks | counters stay 0, no overlap on any tick, site finished, every body legal |
| `rts_collision::forced_overlap_is_repaired`, `an_unrepairable_overlap_is_reported_on_every_tick` (unchanged) | forced overlaps through the testkit hooks | still green — forced-overlap repair coverage is untouched by this ticket |
| `validation_contract` (root, unchanged) | close doc + ADR text | 14 tests still pass; every name added in R6 resolves to a real, un-ignored test |
| anti-vacuity run | `if self.repair_armed` → `if true`, temporarily | exactly the four new tests fail, each on an `overlap_repair_runs()` assertion (`left: 1, right: 0`; R2 `left: 400, right: 0`); the edit is then reverted |

## Impl steps

- [ ] 1. `cd` to the worktree `/tmp/make-audit-aron-2026_08_14_millions-must-die_e7fe9dee0277/issue-9`; confirm `git rev-parse --abbrev-ref HEAD` prints `audit/9-shipping-overlap-repair`, `git status --porcelain` is empty, and T1's commit is `HEAD`.
- [ ] 2. Add `ResourceKind` to the `mmd_engine::rts` import list in `crates/mmd-engine/tests/rts_collision.rs` and append R1's and R2's tests before the `// Determinism` banner.
- [ ] 3. Append R3's test to `crates/mmd-engine/tests/rts_production.rs` after `new_spawn_joins_same_tick_collision`.
- [ ] 4. Append R4's test to `crates/mmd-engine/tests/rts_build.rs` after `completion_evacuates_every_overlapping_body`.
- [ ] 5. Anti-vacuity: change `if self.repair_armed {` to `if true {` in `fn movement` (`crates/mmd-engine/src/rts/world.rs`), run `cargo test -p mmd-engine --locked --test rts_collision --test rts_production --test rts_build`, confirm exactly the four new tests fail and each fails on an `overlap_repair_runs()` assertion (`left: 1, right: 0` for R1/R3/R4; `left: 400, right: 0` for R2), then `git checkout -- crates/mmd-engine/src/rts/world.rs` and confirm `git diff --stat crates/mmd-engine/src/rts/world.rs` prints nothing.
- [ ] 6. Append R5's amendment section to `docs/ADR/017_ADR_rts_hard_collision_navigation_and_formations.md`.
- [ ] 7. Apply R6's three row edits in `docs/rts-interaction-ui-audio-hardening-functional-close.md`.
- [ ] 8. `cargo fmt --all`, then run the validation list below, top to bottom.
- [ ] 9. Commit with DCO: `git commit -s -m "test(rts): prove the shipping paths never need the overlap repair pass"`.

## Outputs

- Files touched: `crates/mmd-engine/tests/rts_collision.rs`, `crates/mmd-engine/tests/rts_production.rs`, `crates/mmd-engine/tests/rts_build.rs`, `docs/ADR/017_ADR_rts_hard_collision_navigation_and_formations.md`, `docs/rts-interaction-ui-audio-hardening-functional-close.md`.
- No `src/` change, no public API change, no behaviour change, no hash change, no migration, no config.

## Validation

- [ ] `cargo fmt --all -- --check` → exit 0, no output.
- [ ] `cargo test -p mmd-engine --locked --test rts_collision` → `running 20 tests` … `test result: ok. 19 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out`.
- [ ] `cargo test -p mmd-engine --locked --test rts_collision -- --exact movement_never_runs_overlap_repair` → `running 1 test` … `1 passed; 0 failed; 0 ignored; 0 measured; 19 filtered out`. `running 0 tests` means the name drifted — a failure, not a pass.
- [ ] `cargo test -p mmd-engine --locked --test rts_collision -- --exact arming_overlap_repair_on_a_valid_world_changes_no_state` → `running 1 test` … `1 passed; 0 failed; 0 ignored; 0 measured; 19 filtered out`.
- [ ] `cargo test -p mmd-engine --locked --test rts_production -- --exact production_never_runs_overlap_repair` → `running 1 test` … `1 passed; 0 failed; 0 ignored; 0 measured; 44 filtered out`.
- [ ] `cargo test -p mmd-engine --locked --test rts_build -- --exact a_completion_never_runs_overlap_repair` → `running 1 test` … `1 passed; 0 failed; 0 ignored; 0 measured; 42 filtered out`.
- [ ] `cargo test -p mmd-engine --locked --test rts_collision --test rts_radius_nav --test rts_build --test rts_production --test rts_acceptance` → `running 20 tests` (19 passed, 1 ignored), `running 10 tests`, `running 43 tests`, `running 45 tests`, `running 7 tests`, every binary `test result: ok.` with `0 failed`.
- [ ] Hash unmoved: `MMD_RTS_COLLISION_CHILD_HASH=1 cargo test -p mmd-engine --locked --test rts_collision -- --exact --ignored --nocapture print_collision_hash_for_child_process` → `running 1 test` and `MMD_RTS_COLLISION_HASH=7e40af444dc7010a1b910c3e852fad8f4ffa76f0d108004043b94d27504f6b8e`. A different hex is a regression: stop and fix, never re-baseline.
- [ ] `cargo test --locked --test validation_contract` → `running 14 tests` … `14 passed; 0 failed` (this is what checks the R6 rows and every backticked test name in the close doc).
- [ ] Shipping build still has no repair pass: `cargo build -p mmd-engine --no-default-features --features gpu --locked` → `Finished`, zero warnings; `cargo clippy -p mmd-engine --no-default-features --features gpu --lib --locked -- -D warnings` → exit 0. (Never `--all-targets` there: `tests/` needs `testkit`.)
- [ ] Shipping feature graph excludes testkit, non-vacuously: `test "$(cargo tree -e features | grep -c testkit)" = 0 && test "$(cargo tree -e features | grep -c 'mmd-engine feature "gpu"')" = 1` → exit 0.
- [ ] `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` → exit 0.
- [ ] `cargo test --workspace --locked` → every binary `ok`, `0 failed`.
- [ ] App functional — no broken path from this slice: `cargo run -- rts --frames 1600 --inject-input-file assets/scenarios/rts_acceptance_v1.script` → exit line still ends `body_overlaps=0 ui_page=gameplay music_starts=1 voice_select=8 voice_order=9 voice_reject=1 sfx_ui=8 keyboard_pan=78`. (Needs SDL3 + a display; on a headless host record it as skipped and say so.)
- [ ] commit msg draft: `test(rts): prove the shipping paths never need the overlap repair pass`
