# T1: Gate repair behind testkit arming

**Plan:** `./ai_artefacts/PLAN_2026_08_14_audit_issue_9_shipping_overlap_repair.md`  
**Depends:** none  
**Commit outcome:** the shipping RTS tick compiles no overlap-repair pass at all; a `testkit` build runs the pass only on ticks after `force_position_for_test` or `entities_mut` armed it, keeps repairing forced overlaps, and re-reports an unrepairable one every tick until it is fixed.

## Context (self-contained)

- Goal: `RtsWorld::movement` runs `repair_body_overlaps` — an N×N penetration search over every live unit — on **every** shipping tick, although nothing in the game can produce a penetration. Only two testkit hooks can. Gate it out of shipping, keep the testkit repair.
- This slice: the whole mechanism plus its forced-overlap/unrepairable coverage. T2 adds the repair-disabled invariants, the hash-equality proof and the docs.
- Out of scope here: any change to the push chain, deflection, formation, seeding, production or construction; the `body_penetrates_any` call in `step_one_unit` (`crates/mmd-engine/src/rts/world.rs:2536`) — that is a shipping arrival check and **stays exactly as it is**; `nearest_free_body_center` (F9's fast path landed at `af728fe`, do not revert); merge-gate command lists; any wall-clock or perf-number assertion; renaming or deleting any existing test or public fn.
- Assumptions in force:
  - Gating is `#[cfg(feature = "testkit")]` **plus** a testkit-only armed flag. `cfg` alone would leave the pass live in every integration test binary (they all compile with `testkit`), so T2 could never prove the normal paths hold with repair disabled. The flag alone would leave the branch and the whole pass compiled into the shipping binary.
  - Both testkit hooks arm the pass. `entities_mut` hands out a raw `&mut EntityStore` whose later writes cannot be observed, so taking that borrow is treated as "this world may now be penetrating".
  - Arming is sticky until a pass ends clean: the flag clears only when `last_tick_error` is `None` after the pass.
  - `last_tick_error = None` moves out of `repair_body_overlaps` and into the top of `movement`, so its meaning survives in a build with no repair pass. In shipping it is then always `None`.
  - Neither new field is hashed (`state_hash`, `world.rs:2695`), so no valid-state hash moves.
- Verified facts this ticket relies on (checked against the tree at HEAD `af728feb216c2b8660db3480fbb74b0b27a54d3c`, branch `audit/9-shipping-overlap-repair`):
  - `mmd-engine` features: `default = ["gpu", "testkit"]`, and the workspace dependency is `mmd-engine = { path = "crates/mmd-engine", default-features = false, features = ["gpu"] }` (`Cargo.toml:15`), so the shipping binary never sees `testkit`.
  - `cargo tree -e features | grep -c testkit` currently prints `0`, and `cargo tree -e features | grep -c 'mmd-engine feature "gpu"'` prints `1` (non-vacuity: the graph really does contain the engine).
  - `cargo build -p mmd-engine --no-default-features --features gpu --locked` and `cargo clippy -p mmd-engine --no-default-features --features gpu --lib --locked -- -D warnings` are both clean today. `--all-targets` cannot be used in that configuration: `crates/mmd-engine/tests/*` consume `mmd_engine::testkit`.
  - `crates/mmd-engine/tests/rts_collision.rs` currently declares **17** tests (`running 17 tests` → `16 passed; 0 failed; 1 ignored`; the ignored one is the child-process fixture `print_collision_hash_for_child_process`, `#[ignore]`d at `rts_collision.rs:625`). This ticket adds **1** → **18** (`17 passed; 0 failed; 1 ignored`).
  - Nothing in `crates/mmd-engine/src/` or `src/` calls `entities_mut` or `force_position_for_test` — the only callers are integration tests. So no shipping or seeding path can arm the pass, which is what makes T2's `overlap_repair_runs() == 0` invariants reachable.
  - Baseline test counts elsewhere, unchanged by this ticket: `rts_build` 42, `rts_production` 44, `rts_radius_nav` 10, `rts_acceptance` 7, all fully passing.
  - The canonical RTS collision state hash at HEAD is `7e40af444dc7010a1b910c3e852fad8f4ffa76f0d108004043b94d27504f6b8e`. It must not move.
  - **Both `#[cfg]` placements this ticket needs are legal Rust and clippy-clean in both feature configurations** — checked on a throwaway `edition = "2024"` crate with the same shape as R1/R4: `#[cfg(…)]` on a struct field, on a *struct-expression* field inside a `Self { … }` literal, and on a bare `if` **statement** inside a fn all build under `--features testkit` and under `--no-default-features`, and `cargo clippy --no-default-features -- -D warnings` reports nothing against them. Do not "work around" the statement attribute with a helper fn or an `#[cfg]`'d wrapper method — write it exactly as R4 gives it.
  - **`cfg`-gating `repair_body_overlaps` strands nothing.** `nearest_free_body_center` keeps three shipping callers — initial spawn relocation (`world.rs:631`), completion evacuation (`world.rs:1780`) and production placement (`world.rs:1858`) — and `body_radius` (`world.rs:2409`) is called from the sweep, push chain and deflection. `repair_body_overlaps` is the only fn that becomes testkit-only, and it is gated, so `--no-default-features --features gpu` raises no `dead_code`.
  - A bare short test name really does resolve in these binaries — they declare their tests at file top level, with no wrapping `mod`, so libtest registers e.g. `forced_overlap_is_repaired`, not `some_mod::forced_overlap_is_repaired`. Confirmed at HEAD: `-- --exact --ignored print_collision_hash_for_child_process` prints `running 1 test`, not `running 0 tests`.
  - `crates/mmd-engine/tests/common/mod.rs` already provides `one_free_centre_spec()` (a 48×48 pocket scene whose grid holds **exactly one** legal body centre) and `pub const ONE_FREE_CENTRE: [f32; 2] = [23.5, 9.5]`, which the single seeded worker occupies. `rts_production.rs:16-17` shows the exact `mod common;` / `use common::{...};` form. `common/mod.rs` carries `#![allow(dead_code)]`, so importing it into another binary warns about nothing.
  - `mmd_engine::rts` re-exports `TickError` (`crates/mmd-engine/src/rts/mod.rs:71`).
  - `EntityStore::spawn(&mut self, kind: EntityKind, owner: u8, pos: [f32; 2]) -> Option<EntityId>` (`crates/mmd-engine/src/rts/entity.rs:198`) is reachable as `world.entities_mut().spawn(...)` (pattern at `crates/mmd-engine/tests/rts_build.rs:821-826`). It returns an `Option`, so the new test's `.expect(...)` is `Option::expect` — do not add a `Result` import or an `is_ok()` check.
  - `RtsHarness::spec(one_free_centre_spec()).build()` seeds **exactly one** worker standing on `ONE_FREE_CENTRE`; `rts_production.rs:865-879` already asserts both (`center_blocked` has exactly one free entry, one live worker).

## Requirements

### R1 — two testkit-only fields on `RtsWorld`

Path: `crates/mmd-engine/src/rts/world.rs`.

Insert immediately **after** the existing `last_tick_error` field (doc `world.rs:207-209`, field line `world.rs:210`, which reads `last_tick_error: Option<TickError>,`):

```rust
    /// Testkit only: has a raw store mutation happened since the last clean
    /// overlap-repair pass? Nothing in the game can merge two bodies — every
    /// path that places a unit (seeding, movement, production, the push off a
    /// finished building) already respects them — so the repair pass exists
    /// solely for [`Self::force_position_for_test`] and [`Self::entities_mut`]
    /// and runs only when one of them has armed it. The shipping tick compiles
    /// no repair pass at all.
    ///
    /// Sticky on failure: an overlap the grid cannot repair leaves this set, so
    /// the pass retries and re-reports [`TickError::UnrepairableOverlap`] every
    /// tick instead of going quiet after one.
    #[cfg(feature = "testkit")]
    repair_armed: bool,
    /// Testkit only: how many overlap-repair passes have actually run. The
    /// deterministic work count the repair-disabled invariants assert on —
    /// never a timing.
    #[cfg(feature = "testkit")]
    repair_runs: u64,
```

Add the matching initialisers in the `Ok(Self { ... })` literal, immediately after `last_tick_error: None,` (`world.rs:686`) — `#[cfg]` on a struct-expression field is stable and is what keeps the literal compiling in both configurations:

```rust
            #[cfg(feature = "testkit")]
            repair_armed: false,
            #[cfg(feature = "testkit")]
            repair_runs: 0,
```

### R2 — the observability accessor

Insert directly after `pub fn last_tick_error` (`world.rs:775-780`):

```rust
    /// Testkit only: how many overlap-repair passes this world has run.
    ///
    /// The pass is armed by [`Self::force_position_for_test`] and
    /// [`Self::entities_mut`] and by nothing else, so a run that touches
    /// neither must report `0` — that is the observation the repair-disabled
    /// invariants assert on, and it is a work count, never a timing.
    #[cfg(feature = "testkit")]
    pub fn overlap_repair_runs(&self) -> u64 {
        self.repair_runs
    }
```

### R3 — both testkit hooks arm the pass

`entities_mut` (`world.rs:750-756`) becomes:

```rust
    /// Direct entity-store mutation. A test hook: hard unit collision makes a
    /// raw position or spawn a world invariant, so shipping code routes
    /// through [`RtsWorld`]'s own systems instead.
    ///
    /// Taking this borrow **arms the overlap-repair pass** ([`Self::repair_armed`]):
    /// the writes it hands out cannot be observed from here, so a raw store
    /// mutation is assumed to be able to merge two bodies. The next tick's
    /// repair pass is what clears one.
    #[cfg(feature = "testkit")]
    pub fn entities_mut(&mut self) -> &mut EntityStore {
        self.repair_armed = true;
        &mut self.entities
    }
```

`force_position_for_test` (`world.rs:758-773`) becomes:

```rust
    /// Test-only: place a live entity anywhere, **including on top of another
    /// body**.
    ///
    /// The one supported way to construct a penetrating world state. Nothing
    /// in the game can produce one, so this hook **arms the overlap-repair
    /// pass** ([`Self::repair_armed`]) and the next tick's pass is what must
    /// clear it, reporting [`Self::last_tick_error`] when it cannot. `false`
    /// for a stale id, and a stale id arms nothing.
    #[cfg(feature = "testkit")]
    pub fn force_position_for_test(&mut self, id: EntityId, pos: [f32; 2]) -> bool {
        let Some(slot) = self.entities.slot(id) else {
            return false;
        };
        self.entities.set_position(slot, pos);
        self.repair_armed = true;
        true
    }
```

### R4 — `movement` drops the unconditional pass

Replace the head of `fn movement` (`world.rs:2107-2115`) — current text:

```rust
    fn movement(&mut self) {
        self.collect_unit_bodies();
        self.repair_body_overlaps();
        if self.last_tick_error.is_some() {
            // The world was handed a penetration nothing could repair. Moving
            // anyone now would build on an illegal state; hold last tick's
            // positions and let the caller see `last_tick_error`.
            return;
        }
```

with:

```rust
    fn movement(&mut self) {
        self.collect_unit_bodies();
        // Always describes the last tick and never an older one, in every
        // build — the repair pass below is the only thing that can set it,
        // and the shipping build has no repair pass.
        self.last_tick_error = None;
        #[cfg(feature = "testkit")]
        if self.repair_armed {
            self.repair_body_overlaps();
            if self.last_tick_error.is_some() {
                // The world was handed a penetration nothing could repair.
                // Moving anyone now would build on an illegal state; hold last
                // tick's positions, stay armed so the next tick retries, and
                // let the caller see `last_tick_error`.
                return;
            }
            self.repair_armed = false;
        }
```

Everything below (`let n = self.unit_scratch.len();` onward) is unchanged.

### R5 — `repair_body_overlaps` becomes testkit-only and counts itself

Replace the doc block and signature at `world.rs:2150-2164`. The doc block currently opens `/// Phase 2: move any unit that starts the tick merged into another body ...`; the body opens with `self.last_tick_error = None;`, which **moves out** (it now lives in `movement`, R4). New head:

```rust
    /// Phase 2, **testkit only**: move any unit that starts the tick merged
    /// into another body to the nearest legal free cell centre, in the same
    /// rotated order phase 3 walks.
    ///
    /// Only an explicitly invalid state reaches this, and only a test can
    /// build one: every in-game path that places a unit (seeding, movement,
    /// production, the push off a finished building) already respects bodies,
    /// so the shipping tick compiles this pass out entirely rather than
    /// scanning every pair of live bodies for a penetration that cannot exist.
    /// [`Self::force_position_for_test`] and [`Self::entities_mut`] arm it;
    /// nothing else does. When a penetration *is* found, the first penetrating
    /// unit in the rotated order is the one relocated — its partner is then no
    /// longer penetrating and is left alone, so a pair costs one relocation,
    /// not two.
    ///
    /// A unit with nowhere legal to go stashes [`TickError::UnrepairableOverlap`]
    /// rather than letting the tick complete with a merged pair unreported.
    /// The caller then leaves [`Self::repair_armed`] set, so the pass retries
    /// on the next tick.
    #[cfg(feature = "testkit")]
    fn repair_body_overlaps(&mut self) {
        self.repair_runs += 1;
        let n = self.unit_scratch.len();
```

The rest of the fn body (`if n < 2 { return; }` through the closing brace) is **unchanged**.

`fn body_penetrates_any` (`world.rs:2192-2198`) is **not** gated: `step_one_unit` calls it on the shipping arrival check (`world.rs:2536`).

### R6 — `TickError` doc tells the truth about who can reach it

`crates/mmd-engine/src/rts/world.rs`, variant `UnrepairableOverlap` (doc `world.rs:166-171`, `#[error(...)]` at `:172`, variant at `:173`). Replace that doc comment with:

```rust
    /// A tick started with two unit bodies merged and the grid had no legal
    /// free centre to repair one of them into. Only
    /// [`RtsWorld::force_position_for_test`] and a raw store mutation through
    /// [`RtsWorld::entities_mut`] can produce that, and both arm a repair pass
    /// the shipping build does not compile — so a shipping tick never sets
    /// this. The movement system is skipped for that tick rather than
    /// compounding an illegal state, the repair stays armed so the next tick
    /// retries, and the overlap is reported here rather than silently kept.
    #[error("a merged unit body could not be repaired: no legal free position exists")]
    UnrepairableOverlap,
```

### R7 — the movement doc's phase list

`world.rs` doc block above `fn movement` (the numbered list at `world.rs:2065-2072`; item 2 is `world.rs:2067-2070`). Replace list item 2 — currently:

```
    /// 2. repair any penetration the world was handed
    ///    ([`Self::repair_body_overlaps`]) — nothing in the game can produce
    ///    one, only [`Self::force_position_for_test`] and a raw store
    ///    mutation can;
```

with:

```
    /// 2. **testkit builds only, and only when a test hook armed it**: repair
    ///    any penetration the world was handed
    ///    ([`Self::repair_body_overlaps`]) — nothing in the game can produce
    ///    one, only [`Self::force_position_for_test`] and a raw store mutation
    ///    can, so the shipping tick skips straight from phase 1 to phase 3;
```

Then, in the same doc block, the induction paragraph's parenthetical "(phase 2 guarantees it)" becomes "(no in-game path can hand this system a merged pair, and phase 2 repairs the ones a test hook forces)". Change nothing else in that block.

## Inputs

- Files to read: `crates/mmd-engine/src/rts/world.rs` (lines 160-215, 680-690, 744-782, 2050-2200, 2520-2545, 2690-2720), `crates/mmd-engine/tests/rts_collision.rs`, `crates/mmd-engine/tests/common/mod.rs` (pocket-scene section at the end).
- **From Depends:** none — this is the first slice.

## TDD

1. **Red** — write both test changes first (`an_unrepairable_overlap_is_reported_on_every_tick`, and the new assertion inside `forced_overlap_is_repaired`). They fail to compile: `overlap_repair_runs` does not exist yet. That compile failure **is** the red.
2. **Green** — apply R1-R7. Suite green with the counts below.
3. **Refactor** — none planned. Do not touch the push chain, deflection or `nearest_free_body_center`.

### Test edits — exact code

In `crates/mmd-engine/tests/rts_collision.rs`:

1. Extend the `mmd_engine::rts` import list (`rts_collision.rs:16-19`) with `TickError` — final list, alphabetical as the file already keeps it:

```rust
use mmd_engine::rts::{
    EntityId, EntityKind, OWNER_PLAYER, Order, RTS_UNIT_BODY_DIAMETER_CELLS,
    RTS_UNIT_BODY_RADIUS_CELLS, TickError, UnitKind, moving_circle_hits_point, units_overlap,
};
```

2. Add the shared pocket scene right after `use mmd_engine::testkit::RtsHarness;` (`rts_collision.rs:21`):

```rust

mod common;
use common::{ONE_FREE_CENTRE, one_free_centre_spec};
```

3. Add this assertion to the **existing** `forced_overlap_is_repaired` (`rts_collision.rs:567-596`), immediately after the existing `assert_eq!(h.world().last_tick_error(), None, ...)` block (`rts_collision.rs:579-583`):

```rust
    assert_eq!(
        h.world().overlap_repair_runs(),
        1,
        "the forced overlap must have armed exactly one repair pass"
    );
```

Do **not** rename that test: `docs/rts-interaction-ui-audio-hardening-functional-close.md` names it, and `tests/validation_contract.rs::phase1_1_close_names_only_real_tests` fails on a renamed test.

4. Add this test immediately after `forced_overlap_is_repaired`, before the `// Determinism` banner (the `// ---…` rule at `rts_collision.rs:598`, `// Determinism` at `:599`):

```rust
/// An overlap the grid cannot repair is reported on **every** tick, not just
/// the first: the pass stays armed until it ends clean, so a world holding an
/// unrepairable penetration never quietly resumes moving.
///
/// The scene's grid holds exactly one legal body centre, which the seeded
/// worker already stands on, so a second body forced onto the same point has
/// nowhere at all to go.
#[test]
fn an_unrepairable_overlap_is_reported_on_every_tick() {
    let mut h = RtsHarness::spec(one_free_centre_spec())
        .build()
        .expect("one-free-centre scene");
    let seeded = workers(&h);
    assert_eq!(seeded.len(), 1, "the pocket scene seeds one worker");
    assert_eq!(
        pos(&h, seeded[0]),
        ONE_FREE_CENTRE,
        "the seeded worker must stand on the grid's only legal body centre"
    );

    let twin = h
        .world_mut()
        .entities_mut()
        .spawn(
            EntityKind::Unit(UnitKind::Worker),
            OWNER_PLAYER,
            ONE_FREE_CENTRE,
        )
        .expect("spawn a second worker on the same point");
    assert!(
        units_overlap(pos(&h, seeded[0]), RADIUS, pos(&h, twin), RADIUS),
        "the forced state must actually be illegal, or this proves nothing"
    );

    for tick in 1..=2u64 {
        h.step_exact(1);
        assert_eq!(
            h.world().last_tick_error(),
            Some(TickError::UnrepairableOverlap),
            "tick {tick} must report the overlap it could not repair"
        );
        assert_eq!(
            h.world().overlap_repair_runs(),
            tick,
            "the pass must stay armed and retry on tick {tick}"
        );
        assert_eq!(
            pos(&h, seeded[0]),
            ONE_FREE_CENTRE,
            "an unrepairable tick moves nobody"
        );
        assert_eq!(
            pos(&h, twin),
            ONE_FREE_CENTRE,
            "an unrepairable tick moves nobody"
        );
    }
}
```

## Test plan

| Test | Input | Expect |
| --- | --- | --- |
| `forced_overlap_is_repaired` (edited) | two workers forced 0.5 cells apart, one tick | no overlap, `last_tick_error() == None`, repaired unit on an unblocked centre, `overlap_repair_runs() == 1` |
| `an_unrepairable_overlap_is_reported_on_every_tick` (new) | one-legal-centre pocket scene, second worker raw-spawned on the occupied centre, two ticks | `last_tick_error() == Some(TickError::UnrepairableOverlap)` after each tick, `overlap_repair_runs()` = 1 then 2, both bodies still at `ONE_FREE_CENTRE` |
| `head_on_units_never_penetrate`, `crossing_units_cannot_tunnel`, `idle_units_are_collision_bodies`, `all_rts_owners_collide`, push-chain cases, `priority_rotates_deterministically` (unchanged) | as today | still green: each uses `force_position_for_test` or `entities_mut`, so the pass is armed exactly where it used to run |
| `hard_collision_is_reproducible` (unchanged) | 4 contended workers, 400 ticks, in-process + child process | same hash in and across processes, and that hash is still `7e40af44…4f6b8e` |
| no-testkit lib build + clippy | `--no-default-features --features gpu` | compiles clean with `-D warnings`; no dead-code warning, because `repair_body_overlaps` is `cfg`-gated and `body_penetrates_any` is still used by `step_one_unit` |

## Impl steps

- [ ] 1. `cd` to the worktree `/tmp/make-audit-aron-2026_08_14_millions-must-die_e7fe9dee0277/issue-9`, confirm `git rev-parse --abbrev-ref HEAD` prints `audit/9-shipping-overlap-repair` and `git status --porcelain` is empty.
- [ ] 2. Record the pre-change canonical hash: `MMD_RTS_COLLISION_CHILD_HASH=1 cargo test -p mmd-engine --locked --test rts_collision -- --exact --ignored --nocapture print_collision_hash_for_child_process` → must print `running 1 test` and `MMD_RTS_COLLISION_HASH=7e40af444dc7010a1b910c3e852fad8f4ffa76f0d108004043b94d27504f6b8e`.
- [ ] 3. Edit `crates/mmd-engine/tests/rts_collision.rs`: import `TickError`, add `mod common; use common::{ONE_FREE_CENTRE, one_free_centre_spec};`, add the `overlap_repair_runs() == 1` assertion to `forced_overlap_is_repaired`, add `an_unrepairable_overlap_is_reported_on_every_tick` (all four snippets under **TDD**, verbatim).
- [ ] 4. Run `cargo test -p mmd-engine --locked --test rts_collision` → must fail to compile with `no method named 'overlap_repair_runs'`. That is the red.
- [ ] 5. Apply R1: add `repair_armed` and `repair_runs` after `last_tick_error` (`world.rs:210`) and their two `#[cfg]`-attributed initialisers after `last_tick_error: None,` (`world.rs:686`).
- [ ] 6. Apply R2: add `overlap_repair_runs` after `pub fn last_tick_error` (`world.rs:780`).
- [ ] 7. Apply R3: `self.repair_armed = true;` in `entities_mut` and in `force_position_for_test` (after the successful `set_position`, before `true`), with the doc changes.
- [ ] 8. Apply R4: rewrite the head of `fn movement` (`world.rs:2107-2115`) exactly as given.
- [ ] 9. Apply R5: `#[cfg(feature = "testkit")]` on `fn repair_body_overlaps`, replace its doc block, delete its `self.last_tick_error = None;` line, add `self.repair_runs += 1;` as the first statement. Leave `body_penetrates_any` untouched.
- [ ] 10. Apply R6 and R7 (doc text only).
- [ ] 11. `cargo fmt --all` then run the validation list below, top to bottom.
- [ ] 12. Commit with DCO: `git commit -s -m "perf(rts): arm the overlap repair pass only for a testkit forced overlap"`.

## Outputs

- Files touched: `crates/mmd-engine/src/rts/world.rs`, `crates/mmd-engine/tests/rts_collision.rs`.
- Behaviour change: no shipping tick runs an overlap-repair scan; a testkit tick runs it only when armed; `last_tick_error` is always `None` in shipping.
- New public API (testkit only): `RtsWorld::overlap_repair_runs() -> u64`.
- No migration, no config, no scenario/asset change, no hash change.

## Validation

- [ ] `cargo fmt --all -- --check` → exit 0, no output.
- [ ] `cargo test -p mmd-engine --locked --test rts_collision` → `running 18 tests` … `test result: ok. 17 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out`.
- [ ] `cargo test -p mmd-engine --locked --test rts_collision -- --exact an_unrepairable_overlap_is_reported_on_every_tick` → `running 1 test` … `1 passed; 0 failed; 0 ignored; 0 measured; 17 filtered out`. A `running 0 tests` line here means the name drifted — treat it as a failure, not a pass.
- [ ] `cargo test -p mmd-engine --locked --test rts_collision -- --exact forced_overlap_is_repaired` → `running 1 test` … `1 passed; 0 failed; 0 ignored; 0 measured; 17 filtered out`.
- [ ] `cargo test -p mmd-engine --locked --test rts_radius_nav --test rts_build --test rts_production --test rts_acceptance` → `running 10 tests` / `running 42 tests` / `running 44 tests` / `running 7 tests`, each `test result: ok.` with `0 failed; 0 ignored`.
- [ ] Hash unmoved: `MMD_RTS_COLLISION_CHILD_HASH=1 cargo test -p mmd-engine --locked --test rts_collision -- --exact --ignored --nocapture print_collision_hash_for_child_process` → `running 1 test` and `MMD_RTS_COLLISION_HASH=7e40af444dc7010a1b910c3e852fad8f4ffa76f0d108004043b94d27504f6b8e`. A different hex is a regression: stop and fix, do not re-baseline.
- [ ] Shipping build has no repair pass: `cargo build -p mmd-engine --no-default-features --features gpu --locked` → `Finished`, zero warnings.
- [ ] `cargo clippy -p mmd-engine --no-default-features --features gpu --lib --locked -- -D warnings` → exit 0. (Never `--all-targets` in this configuration: `tests/` needs `testkit`.)
- [ ] Shipping feature graph excludes testkit, non-vacuously: `test "$(cargo tree -e features | grep -c testkit)" = 0 && test "$(cargo tree -e features | grep -c 'mmd-engine feature "gpu"')" = 1` → exit 0.
- [ ] `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` → exit 0.
- [ ] `cargo test --workspace --locked` → every binary `ok`, `0 failed`.
- [ ] App functional — no broken path from this slice: `cargo run -- rts --frames 1600 --inject-input-file assets/scenarios/rts_acceptance_v1.script` → exit line still ends `body_overlaps=0 ui_page=gameplay music_starts=1 voice_select=8 voice_order=9 voice_reject=1 sfx_ui=8 keyboard_pan=78`. (Needs SDL3 + a display; on a headless host record it as skipped and say so.)
- [ ] commit msg draft: `perf(rts): arm the overlap repair pass only for a testkit forced overlap`
