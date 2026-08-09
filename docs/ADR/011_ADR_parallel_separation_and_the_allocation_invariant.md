# ADR 011: Parallel Separation + the Allocation Invariant

- Status: Accepted
- Date: 2026-08-09
- Supplements: [ADR 010](010_ADR_separation_amortisation_and_push_priority.md)
- Supplements: [ADR 009](009_ADR_agent_separation_and_collision.md) — the
  separation model is unchanged; this records which threads run it
- Plan: `ai-artifacts/PLAN_2026_08_09_horde-sim-headroom.md`

## Context

`sim::collision::accumulate_separation_phase` reads `x`, `y`, `mass`,
`inv_mass` and the neighbour grid — all immutable — and writes only `sep_x[i]`
and `sep_y[i]`. It is already a pure, index-disjoint, read-only-input pass: the
textbook parallel for-loop, and the most expensive loop in the tick.

The movement loop in `sim::tick::step` is not. It mutates `sim.x[i]` and
`sim.y[i]` in place and, through `recycle_one`, advances the shared
`sim.recycle_cursor` — a sequential dependency that would hand out different
spawn slots under any nondeterministic ordering, and break the cross-process
determinism proof.

Two constraints make this harder than it looks. `crates/mmd-engine/src/alloc_guard.rs`
forbids heap allocation on the frame path and enforces it as a merge gate. And
that module's own doc already flagged the debt: *"Introducing a worker thread on
the frame path means this guard must be revisited before it can still claim
'zero allocations per frame'."*

Every primary source that succeeded at this used one shape — read last frame's
state immutably, write to a disjoint buffer, synchronise at the boundary. AoE
IV's MAW allows *"one task in a task group to have write access."* Naughty Dog's
rotating FrameParams: *"No locks needed as each stage works on a unique
instance."* Destiny's frame packet is *"fully stateless."*

Factorio is the counter-example and the reason to be careful: they parallelised
belts, trains and the electric network and got something *"slower than the
non-parallel solution"* because the threads were *"invalidating each others
cache all the time."* Drepper measured the mechanism directly — sharing one
cache line across threads costs multiples of the serial case.

## Decision

**Parallelise the separation pass only.** The movement loop stays serial. Making
it parallel would need arrivals marked into a bitset by a read-only pass and
spawn slots assigned by a short serial pass in ascending index order — the AoE
IV shape. That is a separate decision and is not taken here.

**A persistent pool, sized by the scenario.** `separation_threads` is scenario
data, capped at 16, identity `1`. A scenario asking for `T > 1` spawns `T - 1`
workers once, in `Simulation::new_custom`; the ticking thread is the `T`-th
participant. Threads are never spawned inside a tick — creating one allocates,
and `std::thread::scope` creates them per call.

**Two barriers, no channel.** The ticking thread publishes a job descriptor,
waits on the gate, runs its own chunk, waits on done. Workers wait on the gate,
run their chunk, wait on done. `Barrier::wait` allocates nothing; a channel send
does.

**Contiguous index ranges.** Participant `w` of `T` owns
`[w * n / T, (w + 1) * n / T)` and visits the current phase's indices within it.
Every output element is written by exactly one participant, from immutable
inputs, in the same intra-agent order it would have had serially. **No partial
sum crosses a boundary, so no floating-point reassociation is possible and the
result cannot depend on thread count.** That is asserted directly by
`threads_do_not_change_the_walk`, not argued.

**No new dependency.** `rayon` was the obvious candidate and is rejected: its
parallel bridge carries no documented allocation-free guarantee, and the
allocation invariant is a merge gate, so "probably fine" is not a standard this
can be held to. The cost of refusing it is one `unsafe impl Send for Job` over a
descriptor of raw pointers, with the safety argument written next to it: every
pointer refers to a buffer owned by the `Simulation` that owns the pool, that
`Simulation` is mutably borrowed for the whole of `tick::step`, and the two
write targets are touched through disjoint index ranges.

**Pay the allocation-guard debt now.** Workers arm themselves with
`alloc_guard::arm_worker()` for the duration of each chunk. The counter behind
the guard is process-wide and only records an allocation when the allocating
thread is armed, so an unarmed worker would be invisible. Arming makes a worker
allocation **counted** — the existing zero-allocation assertions now cover the
pool, and the invariant is stronger than it was before threads existed. A thread
that never arms is still invisible, which is exactly what
`foreign_thread_allocations_do_not_leak_into_a_measure_scope` pins, and that
test is untouched.

**Ship it switched off.** Every tracked scene keeps `separation_threads: 1`. The
threaded path is proven by inline harness grids only. The merge gate has to
reproduce on a host with any core count, and a scene that spawns threads is a
scene whose behaviour depends on the machine that ran it — even when the output
does not.

## Consequences

- `Simulation` gains `Option<Arc<SeparationPool>>`. It stays `Clone`, and clones
  share one pool. Ticking two clones concurrently is rejected by a real
  `assert!`, not a `debug_assert!` — the pool's `in_use` check. `Simulation` is
  `pub`, `Clone` and `Send`, so two clones ticking concurrently is reachable
  from entirely safe code, and it races on the shared job cell; a check that a
  release build compiled away would leave that race behind a safe API. The
  check fires *before* the rendezvous barrier, which is what keeps a caught
  re-entrancy from wedging the gate it exists to protect.
- **The rendezvous is panic-safe.** A chunk that panics no longer abandons the
  other participants: every participant — the ticking thread included — catches
  its own unwind with `catch_unwind`, sets a shared `poisoned` flag, and still
  reaches the `done` barrier. Only once every participant has arrived does the
  ticking thread re-raise: its own panic verbatim via `resume_unwind`, or a
  worker's as a fresh panic naming it. Before this, an abandoned rendezvous left
  the other participants blocked on `done` forever, and — when the panicking
  thread was the ticking thread — its unwind then dropped the `Simulation`,
  which made `SeparationPool::drop` wait on `gate` behind workers parked on
  `done`, hanging the process mid-unwind instead of failing a test. A test
  injects `phases = 0` on every participant at once to pin this.
- **Job publication does not rest on `Barrier`'s ordering.** `std` documents
  that a `Barrier` is reusable across generations, not that it supplies a
  happens-before edge, so an `unsafe` read must not lean on that alone. Two
  explicit `AtomicU64` counters — `publish`, released by the ticking thread and
  acquired by every worker before it reads the job; `completed`, released by
  every worker after it writes its chunk and acquired by the ticking thread
  after `done`, before it reads `sep_x` / `sep_y` back — carry the ordering
  argument in both directions instead.
- Dropping the last `Simulation` sharing a pool sets the shutdown flag, releases
  the workers through the gate and joins them. No thread outlives the process.
- One `unsafe` block on the frame path, in one file, with its safety argument
  adjacent. The repo already carries `unsafe` in `alloc_guard.rs` and
  `render/renderer.rs`, and has no `forbid(unsafe_code)`.
- The sequencing was deliberate: amortisation ([ADR 010](010_ADR_separation_amortisation_and_push_priority.md))
  landed first. It divides the same pass by a known factor with no new failure
  modes; threading brings false sharing, a shutdown path and a determinism
  surface to defend. Cheap and safe first.
- **Two residual risks, left open rather than fixed here.** (a) The pinned
  toolchain (stable 1.95.0) runs neither `cargo miri` nor `loom`, so every
  soundness claim in `crates/mmd-engine/src/sim/pool.rs` — the `Send` impl on
  `Job`, the `Sync` impl on `Shared`, the release-acquire pairing — is prose
  plus tests, not machine-checked. (b) `SeparationPool::new` spawns workers in
  a loop; if `thread::spawn` fails partway through, the already-spawned workers
  park at the gate forever with no participant left to release them. Fixing it
  needs an `Option<Self>` plus a condvar-based teardown path and is out of this
  plan's scope.
- **No claim of speed is made here about this engine.** Performance measurement
  is retired for phase 0. The acceptance criteria for the pool are behavioural:
  identical output at every thread count, zero allocation on every thread, and a
  clean shutdown. Whether it is worth switching on is a question for the
  optimization phase, with instruments this project does not currently run.

## Sources

- Pritchett, *The MAW: Safely Multithreading the Deterministic Gameplay of Age
  of Empires IV*, GDC 2022
- Gyrling, *Parallelizing the Naughty Dog Engine Using Fibers*, GDC 2015
- Tatarchuk, *Destiny Renderer*, GDC 2015
- Wube Software, *Factorio Friday Facts #215*
- Drepper, *What Every Programmer Should Know About Memory*, §6.4.1
- Intel oneTBB, *Controlling Chunking* (grain-size guidance)
