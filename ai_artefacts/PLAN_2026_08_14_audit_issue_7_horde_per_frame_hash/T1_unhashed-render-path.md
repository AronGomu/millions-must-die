# T1: Unhashed render path + digest counter

**Plan:** `./ai_artefacts/PLAN_2026_08_14_audit_issue_7_horde_per_frame_hash.md`
**Depends:** none
**Commit outcome:** `Runtime` offers an unhashed frame (`tick_and_render_unhashed` → `RenderOutput`) and a digest budget readout (`state_hash_calls`); the hashed `FrameOutput` API and every existing caller behave exactly as before.

## Context (self-contained)

- Goal: horde `Runtime::tick_and_render` digests the whole sim every frame
  (`crates/mmd-engine/src/runtime.rs:465`), yet only the app's two endpoint frames
  consume a digest and the frozen bench consumes none. This plan removes the
  per-frame digest without changing one byte of stdout or of the hashed API.
- This slice: the engine seam only. It **adds** the unhashed path and the counter
  that later tickets assert on. No caller switches over here — T2 moves the bench,
  T3 moves the app.
- Out of scope here: `src/run.rs`, `crates/mmd-engine/src/bench/runner.rs`,
  `crates/mmd-engine/src/testkit/**` (`Harness::render_frame` keeps returning the
  hashed `FrameOutput`), RTS (`crates/mmd-engine/src/rts/**`, `src/rts_*.rs`),
  the digest bytes of `Simulation::state_hash`, any doc under `docs/`.
- Assumptions in force: digest bytes frozen; the counter must be visible without
  the `testkit` feature (the app crate takes `mmd-engine` with
  `default-features = false, features = ["gpu"]`, and
  `cargo tree -e features | grep -c testkit` must stay `0`).

## Requirements

- `FrameOutput` keeps all 8 public fields, including `state_hash`.
  `Runtime::tick_and_render` keeps its signature and behaviour.
- New `pub struct RenderOutput<'a>` = `FrameOutput` minus `state_hash`.
- New `pub fn tick_and_render_unhashed(&mut self) -> RenderOutput<'_>` — same tick,
  same pack, same `FrameStats`, zero digests.
- Both entries delegate to one private `fn tick_and_pack(&mut self) -> FrameStats`.
- New `state_hash_calls: AtomicU64` field on `Runtime`, bumped in
  `Runtime::state_hash`, read by `pub fn state_hash_calls(&self) -> u64`.
- `Runtime::sim().state_hash()` stays the **uncounted** way to read digest bytes;
  every count assertion in this repo must use it, never `Runtime::state_hash`.

## Inputs

- `crates/mmd-engine/src/runtime.rs` — `FrameStats` (line ~124), `FrameOutput`
  (line ~139), `struct Runtime` (line ~165), `from_scenario` `Ok(Self { … })`
  (line ~256), `state_hash` (line ~362), `pack_groups` (line ~406),
  `tick_and_render` (line ~434).
- `crates/mmd-engine/tests/runtime_frame.rs` — 6 existing tests; helpers
  `gate_scenario()`, `bodied_scenario()`, `visible_count`, `visible_rings`;
  already imports `mmd_engine::render::{ATLAS_COUNT, IsoView, SpriteInstance}`
  and `mmd_engine::runtime::{BoundKey, InputAction, Runtime, action_for_key}`.
- Existing pub API this ticket must not disturb: `Runtime::sim() -> &Simulation`,
  `Simulation::state_hash() -> [u8; 32]` (both ungated `pub`),
  `Runtime::tick_only()`, `Runtime::pack_groups()`.

## TDD

1. **Red** — add the three tests below to
   `crates/mmd-engine/tests/runtime_frame.rs`. They must fail to compile
   (`tick_and_render_unhashed` / `state_hash_calls` do not exist yet) — that is
   the red state for this slice.
2. **Green** — apply the `runtime.rs` edits verbatim from *Impl steps*.
3. **Refactor** — none. Do not touch the 6 pre-existing tests.

## Test plan

| Test | Input | Expect |
| ---- | ----- | ------ |
| `the_unhashed_frame_reports_the_same_frame` | two runtimes on the gate scene at 256 agents, 3 frames, one hashed one unhashed | identical `tick_index`/`agent_count`/`paused`/`overlay_visible`/atlas groups/rings each frame, identical `sim().state_hash()`; hashed count `3`, unhashed count `0` |
| `a_hashed_frame_digests_exactly_once` | 5 hashed frames, then one `state_hash()`, then two `sim().state_hash()` reads | count `5`, then `6`, then still `6` |
| `unhashed_frames_never_digest` | 5 unhashed frames | count `0`, `tick_index() == 5`, `sim().state_hash()` moved off the tick-0 digest |

## Impl steps

- [ ] 1. In `crates/mmd-engine/src/runtime.rs`, extend the std imports at the top
      of the file to:

```rust
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;
```

- [ ] 2. Directly **after** the `pub struct FrameOutput<'a> { … }` block, add:

```rust
/// Result of one runtime frame, without the state digest.
///
/// [`FrameOutput`] minus `state_hash`. `Simulation::state_hash` walks every
/// agent, so a frame whose caller has no consumer for the digest must not pay
/// for one — the interactive app digests at its two endpoints only, and the
/// frozen benchmark never does.
#[derive(Debug, Clone, Copy)]
pub struct RenderOutput<'a> {
    pub tick_index: u64,
    pub agent_count: usize,
    pub paused: bool,
    pub overlay_visible: bool,
    pub groups: &'a [DrawGroup; ATLAS_COUNT],
    /// Hitbox rings for this frame — same contract as [`FrameOutput::rings`].
    pub rings: &'a [SpriteInstance],
    pub stats: FrameStats,
}
```

- [ ] 3. In `pub struct Runtime`, add one field immediately after the
      `ring_instances: Vec<SpriteInstance>,` field:

```rust
    /// Full-state digests taken through this runtime (see
    /// [`Self::state_hash_calls`]). Not feature-gated: the app crate links this
    /// library without `testkit`, and its own tests pin this budget.
    state_hash_calls: AtomicU64,
```

- [ ] 4. In `from_scenario`, in the final `Ok(Self { … })`, add
      `state_hash_calls: AtomicU64::new(0),` immediately after `ring_instances,`.

- [ ] 5. Replace the existing `state_hash` accessor

```rust
    pub fn state_hash(&self) -> [u8; 32] {
        self.sim.state_hash()
    }
```

  with:

```rust
    /// Digest the full simulation state. Counted by [`Self::state_hash_calls`].
    ///
    /// O(agent count). Read the bytes off the simulation
    /// (`runtime.sim().state_hash()`) when the read must not move that budget.
    pub fn state_hash(&self) -> [u8; 32] {
        self.state_hash_calls.fetch_add(1, Ordering::Relaxed);
        self.sim.state_hash()
    }

    /// How many full-state digests were taken through this runtime.
    ///
    /// The digest is the one per-frame cost with no intermediate consumer, so
    /// the count is a budget a test can pin: one per [`Self::tick_and_render`],
    /// none per [`Self::tick_and_render_unhashed`], plus every explicit
    /// [`Self::state_hash`].
    pub fn state_hash_calls(&self) -> u64 {
        self.state_hash_calls.load(Ordering::Relaxed)
    }
```

- [ ] 6. Replace the whole `pub fn tick_and_render(&mut self) -> FrameOutput<'_> { … }`
      body (from `/// One frame: optional sim tick + rebuild draw groups into reused buffers.`
      down to its closing brace) with these three items, in this order:

```rust
    /// Tick (unless paused) and repack into the reused buffers, returning the
    /// frame's timings. The one body both frame entries share, so a hashed and
    /// an unhashed frame can never drift apart on what they simulate or pack.
    fn tick_and_pack(&mut self) -> FrameStats {
        let t0 = Instant::now();

        let sim_ms = if self.paused {
            0.0
        } else {
            let s = Instant::now();
            self.sim.tick();
            s.elapsed().as_secs_f64() * 1000.0
        };

        let u0 = Instant::now();
        self.pack_groups();
        let upload_ms = u0.elapsed().as_secs_f64() * 1000.0;
        let total_ms = t0.elapsed().as_secs_f64() * 1000.0;

        let stats = FrameStats {
            sim_ms,
            upload_ms,
            total_ms,
        };
        self.last_stats = stats;
        stats
    }

    /// One frame: optional sim tick + rebuild draw groups into reused buffers,
    /// with the state digest. Costs one [`Self::state_hash`].
    pub fn tick_and_render(&mut self) -> FrameOutput<'_> {
        let stats = self.tick_and_pack();
        let state_hash = self.state_hash();

        FrameOutput {
            tick_index: self.sim.tick_index(),
            agent_count: self.sim.agent_count(),
            paused: self.paused,
            overlay_visible: self.overlay_visible,
            groups: &self.groups,
            rings: &self.ring_instances,
            stats,
            state_hash,
        }
    }

    /// [`Self::tick_and_render`] without the digest — same tick, same pack,
    /// same [`FrameStats`], zero [`Self::state_hash`] calls.
    pub fn tick_and_render_unhashed(&mut self) -> RenderOutput<'_> {
        let stats = self.tick_and_pack();

        RenderOutput {
            tick_index: self.sim.tick_index(),
            agent_count: self.sim.agent_count(),
            paused: self.paused,
            overlay_visible: self.overlay_visible,
            groups: &self.groups,
            rings: &self.ring_instances,
            stats,
        }
    }
```

- [ ] 7. Append the three tests to `crates/mmd-engine/tests/runtime_frame.rs`
      (end of file), verbatim:

```rust
/// The unhashed frame is the hashed frame minus the digest — not a second,
/// looser frame body.
///
/// Two runtimes on the same scene, stepped side by side: everything a caller
/// can observe about the frame must match, and the two simulations must still
/// be in the same state afterwards. The digests are read off the *simulation*
/// rather than through `Runtime::state_hash`, which is counted — an observation
/// must not spend the budget this test asserts.
#[test]
fn the_unhashed_frame_reports_the_same_frame() {
    let mut hashed = Runtime::load(gate_scenario(), Some(256)).expect("load");
    let mut unhashed = Runtime::load(gate_scenario(), Some(256)).expect("load");

    for frame in 1..=3u64 {
        let (h_tick, h_agents, h_paused, h_overlay, h_groups, h_rings, h_digest) = {
            let out = hashed.tick_and_render();
            (
                out.tick_index,
                out.agent_count,
                out.paused,
                out.overlay_visible,
                out.groups
                    .iter()
                    .map(|g| g.instances.clone())
                    .collect::<Vec<Vec<SpriteInstance>>>(),
                out.rings.to_vec(),
                out.state_hash,
            )
        };
        let (u_tick, u_agents, u_paused, u_overlay, u_groups, u_rings) = {
            let out = unhashed.tick_and_render_unhashed();
            (
                out.tick_index,
                out.agent_count,
                out.paused,
                out.overlay_visible,
                out.groups
                    .iter()
                    .map(|g| g.instances.clone())
                    .collect::<Vec<Vec<SpriteInstance>>>(),
                out.rings.to_vec(),
            )
        };

        assert_eq!(h_tick, frame, "the hashed frame must tick once per frame");
        assert_eq!(
            (u_tick, u_agents, u_paused, u_overlay),
            (h_tick, h_agents, h_paused, h_overlay),
            "frame {frame}: the unhashed frame reports a different frame"
        );
        assert_eq!(u_groups, h_groups, "frame {frame}: atlas groups differ");
        assert_eq!(u_rings, h_rings, "frame {frame}: hitbox rings differ");
        assert_eq!(
            unhashed.sim().state_hash(),
            hashed.sim().state_hash(),
            "frame {frame}: the two simulations diverged"
        );
        assert_eq!(
            h_digest,
            hashed.sim().state_hash(),
            "frame {frame}: the hashed frame reported a digest of some other state"
        );
    }

    assert_eq!(
        unhashed.state_hash_calls(),
        0,
        "the unhashed path digested anyway"
    );
    assert_eq!(
        hashed.state_hash_calls(),
        3,
        "the hashed path must digest exactly once per frame"
    );
}

/// Budget of the hashed path: one digest per frame, one per explicit call, and
/// none for reading the bytes off the simulation.
#[test]
fn a_hashed_frame_digests_exactly_once() {
    let mut rt = Runtime::load(gate_scenario(), Some(64)).expect("load");
    for _ in 0..5 {
        let _ = rt.tick_and_render();
    }
    assert_eq!(rt.state_hash_calls(), 5, "one digest per hashed frame");

    let explicit = rt.state_hash();
    assert_eq!(rt.state_hash_calls(), 6, "an explicit digest is counted");

    // The uncounted channel every budget test observes through.
    assert_eq!(rt.sim().state_hash(), explicit);
    assert_eq!(rt.sim().state_hash(), explicit);
    assert_eq!(
        rt.state_hash_calls(),
        6,
        "reading the digest off the simulation must not spend the budget"
    );
}

/// The unhashed path still simulates — it just never digests.
#[test]
fn unhashed_frames_never_digest() {
    let opening = Runtime::load(gate_scenario(), Some(64))
        .expect("load")
        .sim()
        .state_hash();

    let mut rt = Runtime::load(gate_scenario(), Some(64)).expect("load");
    for _ in 0..5 {
        let _ = rt.tick_and_render_unhashed();
    }

    assert_eq!(rt.tick_index(), 5, "the unhashed frame must still tick");
    assert_ne!(
        rt.sim().state_hash(),
        opening,
        "five unhashed frames left the state where it started"
    );
    assert_eq!(
        rt.state_hash_calls(),
        0,
        "the unhashed path must take no digest at all"
    );
}
```

## Outputs

- Files touched: `crates/mmd-engine/src/runtime.rs`,
  `crates/mmd-engine/tests/runtime_frame.rs`.
- New public API (additive only):
  - `pub struct mmd_engine::runtime::RenderOutput<'a>` with fields
    `tick_index: u64`, `agent_count: usize`, `paused: bool`,
    `overlay_visible: bool`, `groups: &'a [DrawGroup; ATLAS_COUNT]`,
    `rings: &'a [SpriteInstance]`, `stats: FrameStats`; `#[derive(Debug, Clone, Copy)]`.
  - `pub fn Runtime::tick_and_render_unhashed(&mut self) -> RenderOutput<'_>`
  - `pub fn Runtime::state_hash_calls(&self) -> u64`
- Behaviour change for existing callers: none. `Runtime::state_hash` and
  `Runtime::tick_and_render` return the same bytes as before; they now also bump
  a counter. `Harness::render_frame` keeps returning `FrameOutput` and therefore
  keeps digesting once per frame — deliberate, it is a test-harness API.
- No migration, no config, no asset change.

## Validation

- [ ] `cargo test -p mmd-engine --test runtime_frame --locked` → `running 9 tests`,
      `test result: ok. 9 passed; 0 failed; 0 ignored`
- [ ] `cargo test -p mmd-engine --test runtime_frame --locked -- --exact the_unhashed_frame_reports_the_same_frame a_hashed_frame_digests_exactly_once unhashed_frames_never_digest`
      → `running 3 tests`, `3 passed; 0 failed`, `6 filtered out`
      (these are top-level test fns, so `--exact` matches them by their bare
      names; never pass a bare name to `--exact` for a test that lives inside a
      module — it matches 0 tests and still exits 0)
- [ ] `cargo test -p mmd-engine --test harness --locked` → `running 12 tests`,
      `11 passed; 0 failed; 1 ignored` (unchanged baseline — `harness.rs:404`
      drives `Runtime::tick_and_render` directly)
- [ ] `cargo test -p mmd-engine --lib --locked` → `running 56 tests`, `56 passed`
      (unchanged baseline)
- [ ] `cargo test -p mmd-engine --test benchmark_policy --locked` →
      `running 12 tests`, `12 passed` (unchanged baseline)
- [ ] `cargo test -p millions_must_die --bin millions_must_die --locked` →
      `running 95 tests`, `95 passed` (unchanged baseline)
- [ ] `MMD_REQUIRE_GPU=1 cargo test --locked --test cli_contract` →
      `running 25 tests`, `25 passed` (unchanged baseline; `MMD_REQUIRE_GPU=1`
      turns a silent no-GPU skip into a failure)
- [ ] `cargo fmt --all -- --check` → no output, exit 0
- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings` →
      `Finished`, no warnings
- [ ] app functional — no caller switched paths in this slice; the shipping
      binary's frame body is byte-for-byte the previous one
- [ ] commit msg draft: `feat(engine): offer a frame that skips the state digest`
