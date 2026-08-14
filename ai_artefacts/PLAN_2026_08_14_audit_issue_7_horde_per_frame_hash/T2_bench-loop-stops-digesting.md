# T2: Bench loop stops digesting

**Plan:** `./ai_artefacts/PLAN_2026_08_14_audit_issue_7_horde_per_frame_hash.md`
**Depends:** T1
**Commit outcome:** the frozen benchmark's measured loop renders every frame without a state digest, proven at `run_phase` — the loop the real bench runs, not a helper.

## Context (self-contained)

- Goal: horde `Runtime::tick_and_render` digests the whole simulation every frame
  (`Simulation::state_hash`, O(agent count)). The benchmark discards that digest
  — `crates/mmd-engine/src/bench/runner.rs:318` binds `out` and reads only
  `out.stats.sim_ms`, `out.stats.upload_ms`, `out.agent_count`, `out.groups`.
- This slice: switch the bench's measured/warmup loop to the unhashed frame and
  pin its digest budget at 0.
- Out of scope here: `src/run.rs` (T3 owns the app), `crates/mmd-engine/src/runtime.rs`
  (T1 shipped it), bench policy / report / stats / fence-queue behaviour, any
  bench *number* (phase-0 performance is retired and non-gating —
  no timing may gate anything here), RTS.
- Assumptions in force: bench report bytes are unaffected because the digest was
  already discarded; engine unit tests must not depend on the `testkit` feature.

## Requirements

- `run_phase` calls `runtime.tick_and_render_unhashed()`; no other line of the
  bench changes.
- A unit test in `crates/mmd-engine/src/bench/runner.rs` drives the real
  `run_phase` (dry, no renderer) and asserts the runtime took **0** digests while
  still ticking once per frame.
- Frame count in the test is exact, never wall-clock derived: `Duration::ZERO`
  plus `min_frames`.

## Inputs

- `crates/mmd-engine/src/bench/runner.rs`:
  - top imports already provide `std::time::{Duration, Instant}`,
    `crate::render::{DrawGroup, FRAMES_IN_FLIGHT, SpriteRenderer}`,
    `crate::runtime::{Runtime, RuntimeError}`,
    `super::fence_queue::{CompletedFrame, FenceQueue, InflightFrame}`,
    `super::stats::{SampleBuffer, …}`.
  - `pub fn default_scenario_path() -> PathBuf` (line ~63) — the gate scenario.
  - `fn run_phase(runtime: &mut Runtime, mut renderer: Option<&mut SpriteRenderer>, queue: &mut FenceQueue<BenchFence>, duration: Duration, min_frames: u32, dry: bool, record: bool, inject_frame_alloc: bool, samples: &mut SampleBuffer, poll_scratch: &mut Vec<CompletedFrame>) -> Result<u32, BenchError>`
    (line ~289) — loop condition `while Instant::now() < deadline || frames < min_frames`,
    so `duration = Duration::ZERO` renders exactly `min_frames` frames.
  - `dry = true` makes `submit_frame` synthesize a `BenchFence::Dry` and never
    touch a GPU; `renderer = None` is what `run_scale_point` passes in dry mode.
  - the file currently has **no** `#[cfg(test)] mod tests` (siblings
    `bench/fence_queue.rs:245` and `bench/stats.rs:191` do — same shape).
- **From Depends (T1), already on `main` when this ticket starts:**
  - `pub fn Runtime::tick_and_render_unhashed(&mut self) -> RenderOutput<'_>` —
    same tick, pack and `FrameStats` as `tick_and_render`, no digest.
  - `pub struct mmd_engine::runtime::RenderOutput<'a>` with fields
    `tick_index: u64`, `agent_count: usize`, `paused: bool`,
    `overlay_visible: bool`, `groups: &'a [DrawGroup; ATLAS_COUNT]`,
    `rings: &'a [SpriteInstance]`, `stats: FrameStats` (i.e. `FrameOutput`
    minus `state_hash`).
  - `pub fn Runtime::state_hash_calls(&self) -> u64` — digests taken through this
    runtime; bumped by `Runtime::state_hash` and by `Runtime::tick_and_render`,
    never by `Runtime::tick_and_render_unhashed`, never by
    `runtime.sim().state_hash()`.

## TDD

1. **Red** — add `the_measured_loop_never_digests` (below) first and run it: it
   fails on `assert_eq!(runtime.state_hash_calls(), 0)` with `4` digests, because
   `run_phase` still calls the hashed entry.
2. **Green** — swap the single call at `runner.rs:318` to
   `tick_and_render_unhashed()`.
3. **Refactor** — none.

## Test plan

| Test | Input | Expect |
| ---- | ----- | ------ |
| `bench::runner::tests::the_measured_loop_never_digests` | gate scenario at 64 agents, `run_phase(dry = true, renderer = None, duration = ZERO, min_frames = 4, record = true)` | returns `4`; `runtime.tick_index() == 4`; `runtime.state_hash_calls() == 0` |
| `benchmark_policy` suite (unchanged) | `run_bench` dry, existing cases | still `12 passed` — report shape and values untouched |

## Impl steps

- [ ] 1. In `crates/mmd-engine/src/bench/runner.rs`, inside `fn run_phase`,
      replace the line

```rust
        let out = runtime.tick_and_render();
```

  with

```rust
        // The bench has no consumer for the state digest — it reads timings,
        // the agent count and the atlas groups, and `submit_frame` uploads the
        // groups only. Hashing every measured frame would charge the frozen
        // ladder for work no measured frame uses.
        let out = runtime.tick_and_render_unhashed();
```

- [ ] 2. Append to the end of `crates/mmd-engine/src/bench/runner.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    /// The measured loop never digests the simulation.
    ///
    /// Drives the real `run_phase` — the same function warmup and every trial
    /// go through — rather than a stand-in. The frame count comes from
    /// `min_frames` with a zero duration, so this pins an exact number of
    /// frames and never waits on a clock.
    #[test]
    fn the_measured_loop_never_digests() {
        let mut runtime = Runtime::load(default_scenario_path(), Some(64)).expect("load");
        let mut queue: FenceQueue<BenchFence> = FenceQueue::new(FRAMES_IN_FLIGHT);
        let mut samples = SampleBuffer::with_capacity(16);
        let mut poll_scratch = Vec::with_capacity(FRAMES_IN_FLIGHT);

        let frames = run_phase(
            &mut runtime,
            None,
            &mut queue,
            Duration::ZERO,
            4,
            true,
            true,
            false,
            &mut samples,
            &mut poll_scratch,
        )
        .expect("dry phase");

        assert_eq!(frames, 4, "min_frames must fix the frame count exactly");
        assert_eq!(
            runtime.tick_index(),
            4,
            "a bench frame must still advance the simulation"
        );
        assert_eq!(
            runtime.state_hash_calls(),
            0,
            "the benchmark digested state it never reads"
        );
    }
}
```

## Outputs

- Files touched: `crates/mmd-engine/src/bench/runner.rs` (one call site + one
  test module).
- Public API change: none. `BenchOptions`, `run_bench`, `BenchmarkReport` and
  every report field are untouched.
- No migration, no config, no asset, no doc change.

## Validation

- [ ] `cargo test -p mmd-engine --lib --locked` → `running 57 tests`,
      `test result: ok. 57 passed; 0 failed; 0 ignored` (baseline was 56)
- [ ] `cargo test -p mmd-engine --lib --locked -- --exact bench::runner::tests::the_measured_loop_never_digests`
      → `running 1 test`, `1 passed; 0 failed`, `56 filtered out`.
      The **full module path** is mandatory with `--exact`: the bare name
      `the_measured_loop_never_digests` matches 0 tests and libtest still exits 0,
      which would make this line prove nothing.
- [ ] `cargo test -p mmd-engine --test benchmark_policy --locked` →
      `running 12 tests`, `12 passed; 0 failed` (unchanged baseline)
- [ ] `cargo test -p mmd-engine --test runtime_frame --locked` → `running 9 tests`,
      `9 passed; 0 failed` (T1's suite stays green)
- [ ] `cargo test -p mmd-engine --test harness --locked` → `running 12 tests`,
      `11 passed; 0 failed; 1 ignored` (unchanged baseline)
- [ ] `MMD_REQUIRE_GPU=1 cargo test --locked --test cli_contract` →
      `running 25 tests`, `25 passed; 0 failed` (unchanged baseline — the app
      still uses the hashed frame until T3)
- [ ] `cargo fmt --all -- --check` → no output, exit 0
- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings` →
      `Finished`, no warnings
- [ ] app functional — `SDL_VIDEODRIVER=offscreen cargo run -q -- run --agents 64 --frames 3`
      exits 0 and still prints
      `hash=e70c9870a98de70f3baccb24d768cd77acb3c487eaba1beb1bf011e1ad3f6835`
      on its `run: clean exit` line (the app path is untouched by this slice)
- [ ] commit msg draft: `perf(bench): stop digesting a state the measured loop never reads`
