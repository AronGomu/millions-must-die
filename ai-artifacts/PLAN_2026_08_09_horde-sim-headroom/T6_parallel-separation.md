# T6: Parallel separation pass

**Plan:** `./ai-artifacts/PLAN_2026_08_09_horde-sim-headroom.md`
**Depends:** T3, T4, T5
**Commit outcome:** The separation pass runs on a persistent worker pool sized
by the scenario, producing a **bit-identical** result at any thread count, still
allocating nothing per tick — on every thread, now provably.

## Context (self-contained)

- Goal: buy simulation headroom in the per-agent neighbour scan
  (`crates/mmd-engine/src/sim/collision.rs`) without spending a behavioural
  guarantee. This is the last engine ticket of five.
- This slice: `accumulate_separation_phase` reads `x`, `y`, `mass`, `inv_mass`
  and the grid — all immutable — and writes only `sep_x[i]` / `sep_y[i]`. It is
  already a pure, index-disjoint, read-only-input pass: the textbook parallel
  for-loop, and the most expensive loop in the tick. Every primary source that
  succeeded at this used one shape — read last frame's state immutably, write to
  a disjoint buffer, synchronise at the boundary (AoE IV's MAW: *"one task in a
  task group to have write access"*; Naughty Dog's rotating FrameParams:
  *"No locks needed as each stage works on a unique instance"*). Factorio is the
  cautionary tale: they parallelised belts and trains and got something *"slower
  than the non-parallel solution"* because threads were *"invalidating each
  others cache all the time."*
- Out of scope here — hard fences:
  - **Do not parallelise the movement loop in `tick::step`.** It mutates
    `sim.x[i]` / `sim.y[i]` in place and, via `recycle_one`, advances the shared
    `sim.recycle_cursor` — a sequential dependency that would assign different
    spawn slots under any nondeterministic ordering. Only the separation pass
    moves.
  - **Do not add a dependency.** `rayon` is not added; its parallel bridge has
    no documented allocation-free guarantee and `alloc_guard.rs` enforces zero
    per-frame allocation as a merge gate.
  - Do not spawn threads inside a tick. `std::thread::scope` allocates a stack
    per call; the pool is created once, at `Simulation` construction.
  - No scenario file changes, no renderer, no shader, no flow field.
  - **No performance number in any doc, test name or commit message.** Perf
    gating is retired. This ticket is accepted on three behavioural claims:
    identical output at any thread count, zero allocation on every thread, and a
    clean shutdown.
- Assumptions in force:
  - Thread count must never change results. Every `sep_x[i]` is written by
    exactly one thread, from immutable inputs, in an unchanged intra-agent
    order. No floating-point reassociation occurs because no partial sum crosses
    a thread. This is asserted directly, not argued.
  - `alloc_guard.rs` currently states in its own module doc: *"Introducing a
    worker thread on the frame path means this guard must be revisited before it
    can still claim 'zero allocations per frame'."* This ticket pays that debt
    rather than deferring it. Arming the workers makes their allocations
    **counted**, so the invariant gets stronger.
  - Every tracked scene keeps `separation_threads: 1`. The threaded path is
    proven by inline `GridSpec` tests only, so the merge gate reproduces on a
    host with any core count.

## Requirements

- A scenario declaring `separation_threads: T > 1` gets a pool of `T - 1` worker
  threads, spawned once in `Simulation::new_custom`; the ticking thread is the
  `T`-th participant.
- `separation_threads: 1` spawns nothing and runs the pass inline, exactly as
  before.
- Work split: participant `w` of `T` owns the contiguous agent range
  `[w * n / T, (w + 1) * n / T)`, and within it visits the indices of the
  current phase.
- Output is bit-identical for every `T`.
- Nothing on the tick path allocates, on any thread.
- Dropping the `Simulation` shuts the pool down and joins every worker; no
  thread outlives it, no test hangs.

## Inputs

- **Inherited from T0 — the pinned gate digest (do not recompute, do not
  re-derive):**
  `hash=864147ca3a0e09f7ebc5762b778fce193e705a2bc943ceaf67acf087581ee881`
  produced by `cargo run -- run --agents 5000 --frames 300` on commit `df309d3`.
  Every ticket after T0 must reproduce this value byte for byte. A different
  `hash=` means the plan's core invariant is broken — stop and report `failed`
  rather than re-pinning it. The pre-T0 value
  `f647e7f590ed5814e4e61388e23836dfacb980217fb1762542ec3abfe85549b3` is
  superseded and must never reappear.
  T0 also delivered: `MAX_LIVE_AGENTS = 5_000` enforced by a single
  `check_population` at the validator dispatcher (not per family),
  `COLLISION_SCENE_MAX_AGENTS` removed, the three tracked scenes retuned to
  48 px sprites / `collision_radius_q8: 1_536` with fresh `.sha256` sidecars,
  the four `fixture_*` scenes byte-identical, `MIN_SCANNED_TESTS` at `174`, and
  `BenchPolicy::test_short()` split onto its own `test-short-v1` ladder while
  `production()` keeps the frozen phase-0 ladder verbatim.

- `crates/mmd-engine/src/sim/collision.rs` — the pass to split.
- `crates/mmd-engine/src/sim/agents.rs` — `struct Simulation`
  (`#[derive(Debug, Clone)]`), `new_custom`, the `sep_x` / `sep_y` / `mass` /
  `inv_mass` / `grid` fields.
- `crates/mmd-engine/src/sim/tick.rs` — the `if collision_on` block.
- `crates/mmd-engine/src/sim/mod.rs` — module list and re-exports.
- `crates/mmd-engine/src/alloc_guard.rs` — the private helpers
  `fn counting_here() -> bool` and `fn set_counting_here(enabled: bool)`, the
  `COUNTING` thread-local, `MeasureGuard`, and the "Thread scope" section of the
  module doc that this ticket rewrites.
- `crates/mmd-engine/tests/frame_allocations.rs` —
  `a_collision_tick_allocates_nothing` (line ~219),
  `foreign_thread_allocations_do_not_leak_into_a_measure_scope` (line ~143),
  and `lock_alloc_tests()` (line ~29), the mutex every test in that file takes.
- `crates/mmd-engine/tests/separation.rs` — `stacked_collision_grid(agents)`
  (line ~244).
- **From Depends (T1–T5, all landed):**
  - `CollisionParams` is
    `{ radius_cells: f32, strength: f32, phases: u32, mass_classes: u32, threads: u32 }`.
    `threads` is validated to `1..=16`, forced to `1` on a bodyless scene and on
    `technical_prototype_v1`, and reachable in the tick as `sim.collision.threads`.
  - `GridSpec::with_separation_threads(u32)` exists.
  - The pass is
    ```rust
    pub fn accumulate_separation_phase(
        x: &[f32], y: &[f32], grid: &SpatialGrid, radius_cells: f32,
        mass: &[u8], inv_mass: &[f32],
        phases: u32, phase: u32,
        sep_x: &mut [f32], sep_y: &mut [f32],
    )
    ```
    Its per-agent loop is
    `let step = phases as usize; let mut i = phase as usize; while i < n { … i += step; }`;
    it early-outs when the 3×3 window holds one agent; it scans rows via
    `grid.agents_in_bin_row(bx0, bx1, cy)`; it scales each contribution by
    `mass[j] as f32 * inv_mass[i]`.
  - `Simulation` carries `mass: Vec<u8>` and `inv_mass: Vec<f32>`, plus
    `grid_rebuilds: u64`, and the testkit accessors `separation_of(index)`,
    `grid_rebuild_count()`, `mass_of(index)`.
  - `tick::step` rebuilds the grid when `tick_index % phases == 0` and calls
    `accumulate_separation_phase` every tick with
    `phase = tick_index % phases`.
  - Pinned digest that must not move, in
    `crates/mmd-engine/tests/separation.rs`:
    `BODIED_STACK_HASH = "81f958301624ff2033e797032d7cff8fd59344036263fdc5c6e13f00b5c80e8b"`.

## TDD

- [x] 1. **Red** — write `threads_do_not_change_the_walk`,
   `a_single_thread_spawns_no_workers`, `the_pool_shuts_down_cleanly` and
   `a_threaded_collision_tick_allocates_nothing`. All four fail to compile
   (`worker_thread_count` missing) or fail outright (`with_separation_threads`
   reaching the sim changes nothing yet, so the pool-count assertion fails).
   *Criterion:* `cargo test -p mmd-engine --test separation` fails with
   `no method named worker_thread_count`.
- [x] 2. **Green** — add the range-limited pass, the `alloc_guard` worker arm, the
   pool module, then wire `new_custom` and `tick::step`.
   *Criterion:* the four new tests pass.
- [x] 3. **Refactor** — none. Keep `accumulate_separation_phase` as a thin wrapper
   over the range form so T3's and T5's tests keep compiling.
   *Criterion:* `crates/mmd-engine/tests/separation.rs` calls to
   `accumulate_separation` are unedited and green.

## Test plan

| Test | Input | Expect |
| ---- | ---- | ---- |
| `threads_do_not_change_the_walk` | `stacked_collision_grid(512)` built four times with `.with_separation_threads(1 / 2 / 4 / 8)`, 200 ticks each | all four `state_hash_hex()` equal — **the load-bearing claim** |
| `threads_do_not_change_an_amortised_walk` | same, plus `.with_separation_phases(4)`, threads 1 and 4 | both `state_hash_hex()` equal |
| `a_single_thread_spawns_no_workers` | `stacked_collision_grid(32)` (default 1 thread) | `sim().worker_thread_count() == 0` |
| `a_pool_spawns_one_fewer_worker_than_participants` | `.with_separation_threads(4)` | `sim().worker_thread_count() == 3` |
| `the_pool_shuts_down_cleanly` | build with `.with_separation_threads(4)`, tick 10, `drop(h)`, build again, tick 10 | test returns; no hang, no panic |
| `a_threaded_collision_tick_allocates_nothing` | `frame_allocations.rs`: `.with_separation_threads(4)`, take `lock_alloc_tests()`, warm up one tick, then measure 10 ticks | allocation count `== 0` |
| `a_collision_tick_allocates_nothing` | unchanged | still green, unedited |
| `foreign_thread_allocations_do_not_leak_into_a_measure_scope` | unchanged | still green, unedited — a thread that never arms is still invisible |
| `a_bodied_scenario_is_pinned_to_a_golden_digest` | unchanged | still green, unedited |

## Impl steps

- [x] 1. In `crates/mmd-engine/src/sim/collision.rs`, add a range-limited form
      and demote `accumulate_separation_phase` to a wrapper over it:
      ```rust
      /// As [`accumulate_separation_phase`], restricted to agent indices in
      /// `lo..hi`.
      ///
      /// Splitting by index range is what makes this pass safe to run on
      /// several threads: each output element is written by exactly one caller,
      /// from immutable inputs, in the same intra-agent order it would have had
      /// serially. No partial sum crosses a range boundary, so no floating-point
      /// reassociation is possible and the result does not depend on how the
      /// range was cut.
      ///
      /// # Panics
      /// If `phases == 0`, `phase >= phases`, or `hi > x.len()`.
      #[allow(clippy::too_many_arguments)]
      pub fn accumulate_separation_range(
          x: &[f32], y: &[f32], grid: &SpatialGrid, radius_cells: f32,
          mass: &[u8], inv_mass: &[f32],
          phases: u32, phase: u32, lo: usize, hi: usize,
          sep_x: &mut [f32], sep_y: &mut [f32],
      ) { /* … */ }
      ```
- [x] 2. Inside it, replace the loop seed with the first index of this phase at
      or after `lo`:
      ```rust
      let step = phases as usize;
      let mut i = lo + ((phase as usize + step - lo % step) % step);
      while i < hi {
          // … body unchanged, verbatim …
          i += step;
      }
      ```
      Keep every `debug_assert!` and both new `assert!`s from T3, and add
      `assert!(hi <= x.len() && lo <= hi, "range {lo}..{hi} out of bounds");`.
- [x] 3. Make `accumulate_separation_phase` forward with `lo = 0, hi = x.len()`.
      Leave `accumulate_separation` forwarding to it. Re-export
      `accumulate_separation_range` from `crates/mmd-engine/src/sim/mod.rs`.
- [x] 4. In `crates/mmd-engine/src/alloc_guard.rs`, add — mirroring however
      `MeasureGuard` already saves and restores the flag via
      `counting_here()` / `set_counting_here()`:
      ```rust
      /// Arm allocation counting on this thread without snapshotting or
      /// resetting the process-wide counter.
      ///
      /// [`MeasureGuard`] is for the thread that *reads* a delta. This is for a
      /// worker thread that must not allocate at all: arming it makes any
      /// allocation it does make land in the same process-wide counter the
      /// measuring thread reads, so a zero-allocation assertion covers the
      /// worker too. Unarmed, a worker's allocations would simply be invisible.
      pub struct WorkerArm {
          prev: bool,
          _not_send: PhantomData<*const ()>,
      }

      /// Arm this thread until the returned guard drops.
      pub fn arm_worker() -> WorkerArm { /* … */ }

      impl Drop for WorkerArm { /* restore `prev` */ }
      ```
- [x] 5. In the same file, rewrite the last paragraph of the "Thread scope"
      module doc. Replace *"Introducing a worker thread on the frame path means
      this guard must be revisited before it can still claim 'zero allocations
      per frame'."* with:
      *"The separation pass does hand work to worker threads
      (`crate::sim::pool`). Each worker arms itself with [`arm_worker`] for the
      duration of its chunk, so an allocation there is counted into the same
      process-wide number the measuring thread reads — the claim still holds,
      and it now covers the workers. A thread that never arms is still
      invisible, which is what `foreign_thread_allocations_do_not_leak_into_a_measure_scope`
      pins."*
- [x] 6. Create `crates/mmd-engine/src/sim/pool.rs`. Add `mod pool;` to
      `crates/mmd-engine/src/sim/mod.rs` (private — the pool is not public API).
      Contents:
      ```rust
      //! Persistent worker pool for the separation pass.
      //!
      //! Spawned once per `Simulation`, never per tick: creating a thread
      //! allocates, and `crate::alloc_guard` forbids that on the frame path.
      //! Synchronisation is two `Barrier`s, which allocate nothing to wait on.

      use std::cell::UnsafeCell;
      use std::sync::atomic::{AtomicBool, Ordering};
      use std::sync::{Arc, Barrier};
      use std::thread::JoinHandle;

      use super::spatial::SpatialGrid;

      #[derive(Debug, Clone, Copy)]
      struct Job {
          x: *const f32,
          y: *const f32,
          mass: *const u8,
          inv_mass: *const f32,
          grid: *const SpatialGrid,
          sep_x: *mut f32,
          sep_y: *mut f32,
          n: usize,
          radius_cells: f32,
          phases: u32,
          phase: u32,
          participants: usize,
      }

      // SAFETY: a `Job` is published by the ticking thread and read by the
      // workers only between the two barrier waits below. Every pointer refers
      // to a `Vec` owned by the `Simulation` that owns this pool, and that
      // `Simulation` is borrowed mutably for the whole of `tick::step`, so
      // nothing can move or free the buffers while a job is live. `sep_x` and
      // `sep_y` are written through disjoint index ranges — participant `w`
      // touches only `[w * n / T, (w + 1) * n / T)` — so no two threads ever
      // write the same element.
      unsafe impl Send for Job {}
      unsafe impl Sync for Job {}
      ```
      plus `struct Shared { gate: Barrier, done: Barrier, job: UnsafeCell<Job>, shutdown: AtomicBool, in_use: AtomicBool }`,
      `unsafe impl Sync for Shared {}` with its own SAFETY note, and
      `pub(super) struct SeparationPool { shared: Arc<Shared>, handles: Vec<JoinHandle<()>> }`.
- [x] 7. `SeparationPool::new(participants: usize) -> Self`: assert
      `participants >= 2`; build both barriers with `Barrier::new(participants)`;
      spawn `participants - 1` workers, each running:
      ```rust
      loop {
          shared.gate.wait();
          if shared.shutdown.load(Ordering::Acquire) {
              break;                       // do NOT touch `done` on the way out
          }
          let job = unsafe { *shared.job.get() };
          let _arm = crate::alloc_guard::arm_worker();
          run_chunk(&job, worker_index);
          drop(_arm);
          shared.done.wait();
      }
      ```
- [x] 8. `fn run_chunk(job: &Job, w: usize)` rebuilds safe slices from the raw
      pointers and calls `accumulate_separation_range`:
      ```rust
      let lo = w * job.n / job.participants;
      let hi = (w + 1) * job.n / job.participants;
      // SAFETY: see `unsafe impl Send for Job`. `lo..hi` is this participant's
      // exclusive range.
      unsafe {
          super::collision::accumulate_separation_range(
              std::slice::from_raw_parts(job.x, job.n),
              std::slice::from_raw_parts(job.y, job.n),
              &*job.grid,
              job.radius_cells,
              std::slice::from_raw_parts(job.mass, job.n),
              std::slice::from_raw_parts(job.inv_mass, job.n),
              job.phases, job.phase, lo, hi,
              std::slice::from_raw_parts_mut(job.sep_x, job.n),
              std::slice::from_raw_parts_mut(job.sep_y, job.n),
          );
      }
      ```
- [x] 9. `pub(super) fn run(&self, job: Job)` — the ticking thread's entry:
      ```rust
      debug_assert!(
          !self.shared.in_use.swap(true, Ordering::AcqRel),
          "two threads ticked simulations sharing one separation pool"
      );
      unsafe { *self.shared.job.get() = job; }
      self.shared.gate.wait();       // release workers; job is published
      run_chunk(&job, 0);            // the ticking thread is participant 0
      self.shared.done.wait();       // every chunk written
      self.shared.in_use.store(false, Ordering::Release);
      ```
- [x] 10. `impl Drop for SeparationPool`: set `shutdown` with
      `Ordering::Release`, call `self.shared.gate.wait()` once to release the
      parked workers, then `for h in self.handles.drain(..) { let _ = h.join(); }`.
      Do **not** wait on `done` here — the workers break before reaching it.
- [x] 11. In `crates/mmd-engine/src/sim/agents.rs`, add to `struct Simulation`:
      ```rust
      /// Worker pool for the separation pass; `None` when the scenario asked
      /// for one participant. Behind an `Arc` because `Simulation` is `Clone`
      /// — clones share one pool, and ticking two of them concurrently is
      /// rejected by a debug assertion in `SeparationPool::run`.
      pub(super) pool: Option<std::sync::Arc<super::pool::SeparationPool>>,
      ```
      In `new_custom`, build it:
      ```rust
      let pool = if collision.enabled() && collision.threads > 1 {
          Some(std::sync::Arc::new(super::pool::SeparationPool::new(
              collision.threads as usize,
          )))
      } else {
          None
      };
      ```
      and add `pool,` to the struct literal.
- [x] 12. Add the testkit accessor beside `mass_of`:
      ```rust
      /// Worker threads this simulation spawned. `0` when the pass runs inline.
      #[cfg(feature = "testkit")]
      pub fn worker_thread_count(&self) -> usize {
          self.pool.as_ref().map_or(0, |p| p.worker_count())
      }
      ```
      with `pub(super) fn worker_count(&self) -> usize { self.handles.len() }` on
      `SeparationPool`.
- [x] 13. In `crates/mmd-engine/src/sim/tick.rs`, replace the
      `accumulate_separation_phase(...)` call with a branch:
      ```rust
      match sim.pool.clone() {
          Some(pool) => pool.run(super::pool::job_for(sim, phases as u32, phase as u32)),
          None => super::collision::accumulate_separation_phase(
              &sim.x, &sim.y, &sim.grid, collision.radius_cells,
              &sim.mass, &sim.inv_mass,
              phases as u32, phase as u32,
              &mut sim.sep_x, &mut sim.sep_y,
          ),
      }
      ```
      with `pub(super) fn job_for(sim: &mut Simulation, phases: u32, phase: u32) -> Job`
      in `pool.rs` filling the pointers from the sim's vectors. Cloning the
      `Arc` is what releases the borrow on `sim` before the pointers are taken;
      it is a refcount bump, not an allocation.
- [x] 14. Add the six new tests: `threads_do_not_change_the_walk`,
      `threads_do_not_change_an_amortised_walk`,
      `a_single_thread_spawns_no_workers`,
      `a_pool_spawns_one_fewer_worker_than_participants` and
      `the_pool_shuts_down_cleanly` in
      `crates/mmd-engine/tests/separation.rs`; and
      `a_threaded_collision_tick_allocates_nothing` in
      `crates/mmd-engine/tests/frame_allocations.rs`, taking
      `lock_alloc_tests()` first like every other test in that file.
- [x] 15. Run validation, then run the determinism test ten times in a row:
      `for i in $(seq 10); do cargo test -p mmd-engine --test separation threads_do_not_change_the_walk || break; done`.
      A race shows up as an intermittent failure, and one green run does not
      rule it out.

## Outputs

- Touched: new `crates/mmd-engine/src/sim/pool.rs`;
  `crates/mmd-engine/src/sim/mod.rs`,
  `crates/mmd-engine/src/sim/collision.rs`,
  `crates/mmd-engine/src/sim/agents.rs`,
  `crates/mmd-engine/src/sim/tick.rs`,
  `crates/mmd-engine/src/alloc_guard.rs`,
  `crates/mmd-engine/tests/separation.rs`,
  `crates/mmd-engine/tests/frame_allocations.rs`.
- Public API added: `sim::accumulate_separation_range`;
  `alloc_guard::{WorkerArm, arm_worker}`;
  `#[cfg(feature = "testkit")] Simulation::worker_thread_count`.
- Behaviour change: none observable. Output is bit-identical at every thread
  count; only tracked scenes with `separation_threads > 1` would use a pool, and
  none do.
- Migration / config: none. No new dependency; `Cargo.lock` is untouched.

## Validation

- [x] `cargo fmt --all -- --check`
- [x] `cargo test --workspace --locked` — green
- [x] `cargo test -p mmd-engine --test separation` — green, with
      `a_bodied_scenario_is_pinned_to_a_golden_digest` **unedited**
- [x] `cargo test -p mmd-engine --test frame_allocations` — green, with
      `a_collision_tick_allocates_nothing` and
      `foreign_thread_allocations_do_not_leak_into_a_measure_scope` **unedited**
- [x] `for i in $(seq 10); do cargo test -p mmd-engine --test separation threads_do_not_change_the_walk || break; done` — 10/10 green
- [x] `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- [x] `cargo build -p mmd-engine --no-default-features --features gpu`
- [x] `cargo tree -e features | grep -c testkit` — `0` for the shipping build
- [x] `cargo run -- run --agents 5000 --frames 300` — exits 0
- [x] the gate smoke `hash=` equals the T0 pinned digest, byte for byte
- [x] `graphify update .` run (graph refresh; `graphify-out/` is gitignored)
- [x] `nix flake check`
- [x] the three xtask `--check` merge gates — green. (`cargo xtask --check` is
      not a real command in this repo; AGENT.md:42-44 gives the actual form:
      `cargo run -p xtask -- bootstrap --check`, `... shaders --check`,
      `... atlases --check`.)
- [ ] app functional — every scene loads, ticks and exits; no thread outlives the
      process. *Criterion:* windowed/GPU box — recorded in
      `ai-artifacts/manual_test_checklist.md` under `## T6 parallel-separation`,
      left unchecked here; not run headless, does not gate this ticket
- [x] commit msg draft: `feat(sim): run the separation pass on a persistent worker pool`
      *Criterion:* the commit landing this ticket uses that subject
