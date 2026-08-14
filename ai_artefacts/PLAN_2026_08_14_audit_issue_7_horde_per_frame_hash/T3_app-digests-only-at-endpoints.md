# T3: App digests only at its endpoints

**Plan:** `./ai_artefacts/PLAN_2026_08_14_audit_issue_7_horde_per_frame_hash.md`
**Depends:** T1
**Commit outcome:** `run` takes two state digests per run — one for the `frame0` line, one for the `clean exit` line (one only, when a scripted quit lands on frame 1) — and every hash it prints is byte-identical to today's.

## Context (self-contained)

- Goal: horde `Runtime::tick_and_render` digested the whole simulation on every
  frame (`Simulation::state_hash`, O(agent count)), but `src/run.rs` prints a hash
  on exactly two lines: `run: frame0 … hash=<64 hex>` and
  `run: clean exit … hash=<64 hex>`. Move the app to endpoint-only digests
  without moving one byte of stdout.
- This slice: the app's frame body and exit line. It is the user-visible half —
  the CLI contract suite is the judge.
- Out of scope here: `crates/mmd-engine/**` (T1 shipped the engine seam, T2 the
  bench), `src/rts_*.rs` and everything RTS (separate, blocked issue), the
  overlay HUD lines, exit codes, `docs/`, `AGENT.md`.
- Assumptions in force: nothing mutates the simulation between the last
  `tick_and_render*` of a run and `finish` — `InputScript::apply` only reaches
  `Runtime::apply_action`, which touches `paused` / `overlay_visible` /
  `hitboxes_visible` and never the sim — so a digest taken inside `finish` is
  bit-identical to the digest the last rendered frame produced, and on the
  quit-before-frame-1 path it is the load-time digest.

## Requirements

- `step_frame` renders through `Runtime::tick_and_render_unhashed` and stores no
  hash at all.
- `RunState` loses `first_hash` and `last_hash`.
- The `frame0` line's hash comes from one `runtime.state_hash()` in `run`, taken
  right after the frame-1 `step_frame` returned `Some`.
- The `clean exit` line's hash comes from one `runtime.state_hash()` inside
  `finish`.
- No new `run: clean exit` printer and no new success path: a render failure, a
  present failure that cannot fall back, an unfired scripted press, or a
  tick/frame drift must still return `Err` and print no exit line.
- Digest budget, proven through the production seams `step_frame` and `finish`:
  0 per rendered frame, 1 for the `frame0` line, 1 for the exit line.

## Inputs

- `src/run.rs` (714 lines) — all edits live here:
  - module doc `# stdout contract` (lines 10-38) — the two hash-bearing lines.
  - `struct RunState` (line ~249) — `frames`, `quit`, `expected_ticks`,
    `first_hash`, `last_hash`.
  - `pub fn run(opts: RunOptions)` — `let initial_hash = runtime.state_hash();`
    (line 296), the `RunState` literal, the `frame0` `println!` (line ~322),
    the `run_offscreen` fallbacks, `release_window`, `finish` call sites.
  - `fn step_frame<D>(runtime, script, state, backend, draw)` (line ~522) —
    `let out = runtime.tick_and_render();` (line 539),
    `if frame == 1 { state.first_hash = out.state_hash; }` (lines 551-553),
    `state.last_hash = out.state_hash;` (line 555).
  - `fn finish(script, state, runtime, backend, mode)` (line ~583) —
    `hex::encode(state.last_hash)` in the exit `println!`.
  - already imported at the top: `mmd_engine::render::{ATLAS_COUNT, DrawGroup,
    RenderError, SpriteInstance, SpriteRenderer, VIEW_HEIGHT, VIEW_WIDTH}`,
    `mmd_engine::runtime::{BoundKey, InputAction, Runtime, RuntimeError,
    action_for_key}`, `hex`. No import needs adding or removing.
  - private helpers reachable from an in-file `#[cfg(test)] mod tests`:
    `workspace_root_or_cwd()`, `InputScript::parse`, `InputScript::default()`,
    `RunState`, `step_frame`, `finish`.
- **From Depends (T1), already on `main` when this ticket starts:**
  - `pub fn Runtime::tick_and_render_unhashed(&mut self) -> RenderOutput<'_>` —
    same tick, same pack, same `FrameStats` as `tick_and_render`, no digest.
  - `pub struct mmd_engine::runtime::RenderOutput<'a>` = `FrameOutput` minus
    `state_hash`: `tick_index: u64`, `agent_count: usize`, `paused: bool`,
    `overlay_visible: bool`, `groups: &'a [DrawGroup; ATLAS_COUNT]`,
    `rings: &'a [SpriteInstance]`, `stats: FrameStats`.
  - `pub fn Runtime::state_hash_calls(&self) -> u64` — digests taken through this
    runtime. Bumped by `Runtime::state_hash` and by `Runtime::tick_and_render`;
    never by `tick_and_render_unhashed`; never by `runtime.sim().state_hash()`,
    which is the uncounted way to read digest bytes in a budget test.
  - `Runtime::tick_only() -> bool` (pre-existing) — ticks without packing or
    digesting; returns `false` when paused.

## Endpoint pins (captured on `e7fe9dee0277`, `SDL_VIDEODRIVER=offscreen`, `backend=vulkan`)

`Simulation::state_hash` is a same-platform digest, so these are host-stable
values for this worktree — they must come out byte-identical after the change.

| Run | Line | Hash |
| --- | --- | --- |
| `run --agents 64 --frames 3` | `frame0` (`tick=1`) | `c3ab606f5b16424775a369a8f82156dd237300dce4f13270c8d26a47f5e865b0` |
| `run --agents 64 --frames 3` | `clean exit` (`tick=3`) | `e70c9870a98de70f3baccb24d768cd77acb3c487eaba1beb1bf011e1ad3f6835` |
| `run --agents 64 --frames 1` | `frame0` and `clean exit` (both `tick=1`) | `c3ab606f5b16424775a369a8f82156dd237300dce4f13270c8d26a47f5e865b0` |
| `run --agents 64 --frames 3 --inject-input 1:esc` | `clean exit` only (`tick=0 frames=0 quit=true`), no `frame0` line | `9cd3a6751e5eb5772c5edba159abbfce4f1b43fa83bacd84012de8c966b8dcd3` |
| `run --agents 64 --frames 4 --inject-input 1:space` | `frame0` and `clean exit` (both `tick=0`, `paused=true`, `frames=4`) | `9cd3a6751e5eb5772c5edba159abbfce4f1b43fa83bacd84012de8c966b8dcd3` |
| `run --agents 5000 --frames 300` | `frame0` (`tick=1`) | `fa0cf6bdbb3b093c98019d0332a7ac497b0d5f134426807edabe8f51afd1959b` |
| `run --agents 5000 --frames 300` | `clean exit` (`tick=300 frames=300`) | `864147ca3a0e09f7ebc5762b778fce193e705a2bc943ceaf67acf087581ee881` |

## TDD

1. **Red** — add the four tests below to a new `#[cfg(test)] mod tests` at the end
   of `src/run.rs`. Before the source edits they fail:
   `rendered_frames_never_digest_state` reports 5 digests instead of 0,
   `a_whole_run_digests_at_its_two_endpoints` reports 4 instead of 2,
   `a_quit_before_the_first_frame_digests_once` reports 0 instead of 1 (the
   pre-change code digests at load, before `RunState` is built).
2. **Green** — apply impl steps 1-6 verbatim.
3. **Refactor** — none. Do not touch the overlay HUD block, the pace sleep, the
   window claim/release order, or any error path.

## Test plan

| Test | Input | Expect |
| ---- | ----- | ------ |
| `rendered_frames_never_digest_state` | 5 `step_frame` calls, no-op draw, empty script | `state.frames == 5`, `state.expected_ticks == 5`, `runtime.tick_index() == 5`, `runtime.state_hash_calls() == 0` |
| `the_frame0_hash_is_the_state_frame_one_produced` | 1 `step_frame`, then `runtime.state_hash()`; second runtime stepped once with `tick_only()` | the two digests are equal; driven runtime's count is `1` |
| `a_whole_run_digests_at_its_two_endpoints` | 1 `step_frame` → `state_hash()` → 2 more `step_frame` → `finish(…, "offscreen")` | `finish` returns `Ok`; count `== 2`; the frame-1 digest differs from `runtime.sim().state_hash()` at exit |
| `a_quit_before_the_first_frame_digests_once` | script `1:esc`, one `step_frame` → `Ok(None)`, then `finish` | `state.quit`, `state.frames == 0`, `runtime.tick_index() == 0`, count `== 1` |

## Impl steps

- [ ] 1. In `src/run.rs`, in the module doc, replace

```rust
//! `hash` is the simulation state hash: on the `frame0` line after the first
//! frame, on the exit line after the last. The `sim=`/`upload=` fields are for
```

  with

```rust
//! `hash` is the simulation state hash: on the `frame0` line after the first
//! frame, on the exit line after the last. It is digested at those two moments
//! and nowhere else — the digest walks every agent, and no frame in between has
//! a consumer for one. A run that quits before frame 1 prints neither a
//! `frame0` line nor a first digest, and pays for exactly one.
//! The `sim=`/`upload=` fields are for
```

- [ ] 2. Replace the `RunState` declaration

```rust
/// Everything the exit line reports, accumulated as the run proceeds.
struct RunState {
    /// Frames actually rendered (frame 1 is the offscreen proof frame).
    frames: u64,
    quit: bool,
    /// Ticks the frames rendered so far should have produced — one per
    /// unpaused frame.
    expected_ticks: u64,
    first_hash: [u8; 32],
    last_hash: [u8; 32],
}
```

  with

```rust
/// Everything the exit line reports, accumulated as the run proceeds.
///
/// Carries no state hash: the two lines that print one digest the runtime at
/// the moment they print it, so there is nothing to keep in step here.
struct RunState {
    /// Frames actually rendered (frame 1 is the offscreen proof frame).
    frames: u64,
    quit: bool,
    /// Ticks the frames rendered so far should have produced — one per
    /// unpaused frame.
    expected_ticks: u64,
}
```

- [ ] 3. In `pub fn run`, replace

```rust
    let initial_hash = runtime.state_hash();
    let mut state = RunState {
        frames: 0,
        quit: false,
        expected_ticks: 0,
        first_hash: initial_hash,
        last_hash: initial_hash,
    };
```

  with

```rust
    let mut state = RunState {
        frames: 0,
        quit: false,
        expected_ticks: 0,
    };
```

- [ ] 4. In `pub fn run`, in the `frame0` `println!`, replace the argument
      `hex::encode(state.first_hash),` with

```rust
        // First of the run's two digests. Taken here rather than inside the
        // frame body: nothing has touched the simulation since frame 1 packed,
        // so this is the state frame 1 produced.
        hex::encode(runtime.state_hash()),
```

- [ ] 5. In `fn step_frame`, replace

```rust
    let out = runtime.tick_and_render();
```

  with

```rust
    // Unhashed: a rendered frame has no consumer for the state digest, and the
    // digest walks every agent. The two lines that print one take their own.
    let out = runtime.tick_and_render_unhashed();
```

  and replace

```rust
    if frame == 1 {
        state.first_hash = out.state_hash;
    }
    state.frames = frame;
    state.last_hash = out.state_hash;
```

  with

```rust
    state.frames = frame;
```

- [ ] 6. In `fn finish`, in the exit `println!`, replace the argument
      `hex::encode(state.last_hash),` with

```rust
        // Second of the run's two digests — and the only one on a run that quit
        // before frame 1. Nothing mutates the simulation after the last
        // rendered frame (`apply_action` reaches pause/overlay/hitboxes only),
        // so this is the state the last frame left behind.
        hex::encode(runtime.state_hash()),
```

- [ ] 7. Append to the end of `src/run.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    /// The gate scene at a small population: CPU only, no device needed.
    fn test_runtime() -> Runtime {
        let scenario = workspace_root_or_cwd().join("assets/scenarios/technical_prototype_v1.ron");
        Runtime::load(&scenario, Some(64)).expect("load gate scenario")
    }

    fn fresh_state() -> RunState {
        RunState {
            frames: 0,
            quit: false,
            expected_ticks: 0,
        }
    }

    /// Stand-in for the offscreen/swapchain draw. The frame body under test is
    /// the real `step_frame`; only the device is replaced.
    fn noop_draw(
        _groups: &[DrawGroup; ATLAS_COUNT],
        _rings: &[SpriteInstance],
    ) -> Result<(), RenderError> {
        Ok(())
    }

    /// A rendered frame must not digest: no line prints a per-frame hash.
    #[test]
    fn rendered_frames_never_digest_state() {
        let mut runtime = test_runtime();
        let mut script = InputScript::default();
        let mut state = fresh_state();

        for frame in 1..=5u64 {
            let report = step_frame(&mut runtime, &mut script, &mut state, "test", noop_draw)
                .expect("frame body")
                .expect("a frame was rendered");
            assert_eq!(report.tick, frame, "a frame must advance the simulation");
        }

        assert_eq!(state.frames, 5);
        assert_eq!(state.expected_ticks, 5);
        assert_eq!(runtime.tick_index(), 5);
        assert_eq!(
            runtime.state_hash_calls(),
            0,
            "the frame body digested a state nothing prints"
        );
    }

    /// The `frame0` line reports the state frame 1 produced.
    ///
    /// Checked against a second runtime advanced one tick through `tick_only`,
    /// so the expectation does not flow through the code under test.
    #[test]
    fn the_frame0_hash_is_the_state_frame_one_produced() {
        let mut driven = test_runtime();
        let mut script = InputScript::default();
        let mut state = fresh_state();
        step_frame(&mut driven, &mut script, &mut state, "test", noop_draw)
            .expect("frame body")
            .expect("a frame was rendered");

        let printed = driven.state_hash();

        let mut independent = test_runtime();
        assert!(independent.tick_only(), "the control run must tick");
        assert_eq!(
            printed,
            independent.sim().state_hash(),
            "the frame0 line reports a state frame 1 did not produce"
        );
        assert_eq!(
            driven.state_hash_calls(),
            1,
            "the frame0 line owes exactly one digest"
        );
    }

    /// A whole run digests twice: once for `frame0`, once for the exit line.
    #[test]
    fn a_whole_run_digests_at_its_two_endpoints() {
        let mut runtime = test_runtime();
        let mut script = InputScript::default();
        let mut state = fresh_state();

        step_frame(&mut runtime, &mut script, &mut state, "test", noop_draw)
            .expect("frame body")
            .expect("a frame was rendered");
        let frame0_hash = runtime.state_hash();
        for _ in 0..2 {
            step_frame(&mut runtime, &mut script, &mut state, "test", noop_draw)
                .expect("frame body")
                .expect("a frame was rendered");
        }

        finish(&mut script, &state, &runtime, "test", "offscreen").expect("clean exit");

        assert_eq!(
            runtime.state_hash_calls(),
            2,
            "a run may digest at its two endpoints and nowhere else"
        );
        // Read off the simulation: `Runtime::state_hash` is counted, and this
        // observation must not spend the budget just asserted.
        assert_ne!(
            frame0_hash,
            runtime.sim().state_hash(),
            "the exit line must report the final state, not frame 1's"
        );
    }

    /// A scripted quit on frame 1 renders nothing, so it pays for one digest —
    /// the exit line's — and never reaches the `frame0` line.
    #[test]
    fn a_quit_before_the_first_frame_digests_once() {
        let mut runtime = test_runtime();
        let mut script = InputScript::parse("1:esc").expect("script");
        let mut state = fresh_state();

        let rendered = step_frame(&mut runtime, &mut script, &mut state, "test", noop_draw)
            .expect("frame body");
        assert!(rendered.is_none(), "a quit cancels its own frame");
        assert!(state.quit);
        assert_eq!(state.frames, 0);
        assert_eq!(runtime.tick_index(), 0);

        finish(&mut script, &state, &runtime, "test", "offscreen").expect("clean exit");

        assert_eq!(
            runtime.state_hash_calls(),
            1,
            "a run that rendered nothing owes exactly the exit line's digest"
        );
    }
}
```

## Outputs

- Files touched: `src/run.rs` only.
- Public API change: none. `RunOptions`, `RunError`, `EXIT_ERROR`, `EXIT_NO_GPU`
  and every stdout line keep their exact shape and bytes.
- Behaviour change: digests per run drop from `frames + 1` to `2` (`1` when a
  scripted quit lands on frame 1). No timing claim is made or gated.
- No migration, no config, no asset change.

## Validation

Every line below is a pass/fail check with a pinned count — no timing, no
eyeballing.

- [ ] `cargo test -p millions_must_die --bin millions_must_die --locked` →
      `running 99 tests`, `test result: ok. 99 passed; 0 failed; 0 ignored`
      (baseline was 95; this ticket adds 4)
- [ ] `cargo test -p millions_must_die --bin millions_must_die --locked -- --exact run::tests::rendered_frames_never_digest_state run::tests::the_frame0_hash_is_the_state_frame_one_produced run::tests::a_whole_run_digests_at_its_two_endpoints run::tests::a_quit_before_the_first_frame_digests_once`
      → `running 4 tests`, `4 passed; 0 failed`, `95 filtered out`.
      The **full module path** (`run::tests::…`) is mandatory with `--exact`:
      the bare test names match 0 tests and libtest still exits 0, which would
      make this line prove nothing.
- [ ] `MMD_REQUIRE_GPU=1 cargo test --locked --test cli_contract` →
      `running 25 tests`, `test result: ok. 25 passed; 0 failed; 0 ignored`.
      `MMD_REQUIRE_GPU=1` is required: without it a host with no device turns
      every case into a silent skip that still exits 0.
- [ ] `MMD_REQUIRE_GPU=1 cargo test --locked --test cli_contract run_exits_after_n_frames -- --exact`
      → `running 1 test`, `1 passed; 0 failed`, `24 filtered out`
      (`run_exits_after_n_frames` is a top-level test fn, so the bare name is the
      exact name here)
- [ ] `MMD_REQUIRE_GPU=1 cargo test --locked --test cli_contract -- --exact pause_freezes_state pause_from_the_first_frame_is_not_a_stalled_run overlay_toggle_is_inert hitbox_toggle_is_scriptable quit_before_the_first_frame_renders_nothing`
      → `running 5 tests`, `5 passed; 0 failed`, `20 filtered out`
- [ ] `cargo test -p mmd-engine --test runtime_frame --locked` → `running 9 tests`,
      `9 passed; 0 failed` (T1's suite, unchanged)
- [ ] `cargo test -p mmd-engine --lib --locked` → `running 57 tests`, `57 passed`
      after T2 (`56 passed` if T2 has not landed yet)
- [ ] `cargo test -p mmd-engine --test harness --locked` → `running 12 tests`,
      `11 passed; 0 failed; 1 ignored`
- [ ] `cargo test -p millions_must_die --test rts_cli_contract --locked -- --test-threads=1`
      → `running 56 tests`, `56 passed; 0 failed`.
      Single-threaded on purpose: this suite has a **pre-existing** parallel
      flake on the audit host (`no_rts_run_creates_the_real_user_config` and
      `dummy_driver_run_does_not_touch_settings` both write the real user config
      and fail together under the default thread count on `e7fe9dee0277`, before
      any change in this plan).
- [ ] `MMD_REQUIRE_GPU=1 cargo test --workspace --locked 2>&1 | grep "test result:" | grep -v " 0 failed"`
      → prints nothing except, possibly, the pre-existing `rts_cli_contract`
      line above; every other suite must show `0 failed`
- [ ] Endpoint bytes, `--frames 3` (must reproduce the pins exactly):
      `SDL_VIDEODRIVER=offscreen cargo run -q -- run --agents 64 --frames 3`
      → exit 0, `run: frame0 tick=1 hash=c3ab606f5b16424775a369a8f82156dd237300dce4f13270c8d26a47f5e865b0`,
      `run: clean exit mode=offscreen … tick=3 frames=3 hash=e70c9870a98de70f3baccb24d768cd77acb3c487eaba1beb1bf011e1ad3f6835 quit=false paused=false overlay=false hitboxes=true`
- [ ] Endpoint bytes, `--frames 1`:
      `SDL_VIDEODRIVER=offscreen cargo run -q -- run --agents 64 --frames 1`
      → both hashes `c3ab606f5b16424775a369a8f82156dd237300dce4f13270c8d26a47f5e865b0`
- [ ] Quit path (no `frame0` line, one digest):
      `SDL_VIDEODRIVER=offscreen cargo run -q -- run --agents 64 --frames 3 --inject-input 1:esc`
      → exit 0, no `frame0` line,
      `run: clean exit mode=offscreen … tick=0 frames=0 hash=9cd3a6751e5eb5772c5edba159abbfce4f1b43fa83bacd84012de8c966b8dcd3 quit=true`
- [ ] Paused path:
      `SDL_VIDEODRIVER=offscreen cargo run -q -- run --agents 64 --frames 4 --inject-input 1:space`
      → exit 0, both hashes
      `9cd3a6751e5eb5772c5edba159abbfce4f1b43fa83bacd84012de8c966b8dcd3`,
      exit line `tick=0 frames=4 … paused=true`
- [ ] Gate smoke (windowed on a headed host, offscreen otherwise):
      `cargo run -- run --agents 5000 --frames 300` → exit 0,
      `frame0 … hash=fa0cf6bdbb3b093c98019d0332a7ac497b0d5f134426807edabe8f51afd1959b`,
      `clean exit … tick=300 frames=300 hash=864147ca3a0e09f7ebc5762b778fce193e705a2bc943ceaf67acf087581ee881`
- [ ] Failure paths still fail: `cargo run -q -- run --agents 0` → exit 1 and **no**
      `run: clean exit` line; `cargo run -q -- run --frames 3 --inject-input 9:esc`
      → exit 1, message `--inject-input entries never fired: 9:esc`, no exit line
- [ ] `grep -c '"run: clean exit' src/run.rs` → `1` (the exit line keeps exactly
      one printer; no error path gained a clean exit). Note the leading quote in
      the pattern: `grep -c "run: clean exit" src/run.rs` returns `2`, because
      the module doc quotes the same line.
- [ ] `cargo fmt --all -- --check` → no output, exit 0
- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings` →
      `Finished`, no warnings
- [ ] `cargo tree -e features | grep -c testkit` → `0` (shipping binary still
      free of the harness; `grep -c` exiting 1 on a zero count is the passing case)
- [ ] commit msg draft: `perf(run): digest the horde state at the two endpoints that print it`
