//! Persistent worker pool for the separation pass.
//!
//! Spawned once per [`Simulation`], never per tick: creating a thread
//! allocates, and [`crate::alloc_guard`] forbids that on the frame path.
//! Synchronisation is two [`Barrier`]s, which allocate nothing to wait on — a
//! channel send would.
//!
//! The shape is the one every primary source that succeeded at this converged
//! on: read last frame's state immutably, write to a disjoint buffer,
//! synchronise at the boundary. The ticking thread publishes a job descriptor,
//! opens the gate, runs its own chunk as participant 0, and waits on `done`.
//! Participant `w` of `T` owns the agent range `[w * n / T, (w + 1) * n / T)`,
//! so every output element is written by exactly one participant, from
//! immutable inputs, in the same intra-agent order it would have had serially.
//! No partial sum crosses a boundary, so no floating-point reassociation is
//! possible and the digest cannot depend on the thread count.
//!
//! # Panics
//!
//! A panic inside a chunk must never abandon the rendezvous. If it did, the
//! other participants would block on `done` forever — and if the panicking
//! participant were the *ticking* thread, its unwind would drop the
//! `Simulation`, and [`SeparationPool::drop`] would then wait on `gate` for
//! workers parked on `done`, hanging the process mid-unwind. A failed assertion
//! would surface as a stalled run rather than a red test.
//!
//! So every participant catches its own unwind, flags `poisoned`, and reaches
//! `done` anyway; the ticking thread re-raises afterwards — its own panic
//! verbatim via `resume_unwind`, or a worker's as a fresh panic naming it.
//! Workers therefore never unwind out of their loop and stay available as
//! participants, so `Drop` can still shut the pool down. `catch_unwind`
//! allocates only on the panic path, which is already fatal, so the
//! zero-allocation invariant is untouched.
//!
//! One panic is deliberately raised *before* `gate.wait()`: the `in_use` check
//! in [`SeparationPool::run`], which fires when two threads tick simulations
//! sharing one pool. Raising it before the barrier is what keeps it from
//! wedging the rendezvous it exists to protect.

use std::cell::UnsafeCell;
use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Barrier};
use std::thread::JoinHandle;

use super::agents::Simulation;
use super::spatial::SpatialGrid;

/// One tick's separation work, as raw pointers into the owning simulation.
///
/// Raw rather than borrowed because the workers outlive any borrow the ticking
/// thread could hand them: the pool is built once and reused, so there is no
/// lifetime that both a `'static` worker closure and a per-tick `&Simulation`
/// can share.
#[derive(Debug, Clone, Copy)]
pub(super) struct Job {
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

// SAFETY: a `Job` is published by the ticking thread before it opens the gate
// and is read by the workers only between the two barrier waits in `run`; the
// `publish` / `completed` release-acquire pairs on `Shared` order the write
// against every read, in both directions.
// Every pointer refers to a `Vec` (or the `SpatialGrid`) owned by the
// `Simulation` that owns this pool; `job_for` takes `&mut Simulation`, and the
// ticking thread is inside `run` — touching nothing — for the whole window in
// which a job is live, so no buffer can be moved, grown or freed while a worker
// holds a pointer to it. `x`, `y`, `mass`, `inv_mass` and `grid` are only ever
// read. `sep_x` and `sep_y` are only ever written through the disjoint
// sub-slice `[w * n / T, (w + 1) * n / T)` that `run_chunk` carves out for
// participant `w`, so no two threads ever form overlapping references to them,
// let alone write the same element.
//
// `Sync` is deliberately *not* implemented: no `&Job` is ever shared across
// threads (each participant copies the descriptor out of the `UnsafeCell` by
// value), so claiming it would be unsafe with nothing to justify it.
unsafe impl Send for Job {}

impl Job {
    /// This participant's half-open agent range.
    fn range(&self, w: usize) -> (usize, usize) {
        debug_assert!(w < self.participants);
        (
            w * self.n / self.participants,
            (w + 1) * self.n / self.participants,
        )
    }
}

/// Build the descriptor for this tick from the simulation's own buffers.
///
/// Takes `&mut Simulation` because it hands out a `*mut` into `sep_x` / `sep_y`;
/// the exclusive borrow is what makes those pointers the only write path to
/// those buffers for as long as the job is live.
pub(super) fn job_for(sim: &mut Simulation, phases: u32, phase: u32) -> Job {
    let n = sim.x.len();
    Job {
        x: sim.x.as_ptr(),
        y: sim.y.as_ptr(),
        mass: sim.mass.as_ptr(),
        inv_mass: sim.inv_mass.as_ptr(),
        grid: &raw const sim.grid,
        sep_x: sim.sep_x.as_mut_ptr(),
        sep_y: sim.sep_y.as_mut_ptr(),
        n,
        radius_cells: sim.collision.radius_cells,
        phases,
        phase,
        // Overwritten by `SeparationPool::run`, which is the authority on how
        // many participants will actually arrive.
        participants: sim.collision.threads as usize,
    }
}

/// Run participant `w`'s share of `job`.
fn run_chunk(job: &Job, w: usize) {
    let (lo, hi) = job.range(w);
    // SAFETY: see `unsafe impl Send for Job`. The input slices span the whole
    // population and are read-only for every participant, which shared
    // references permit. The two output slices are offset to `lo` and sized
    // `hi - lo`, and `Job::range` partitions `0..n` — participant `w`'s window
    // is disjoint from every other participant's — so these are the only
    // unique references to that memory in the process for the duration of the
    // call.
    unsafe {
        super::collision::accumulate_separation_range(
            std::slice::from_raw_parts(job.x, job.n),
            std::slice::from_raw_parts(job.y, job.n),
            &*job.grid,
            job.radius_cells,
            std::slice::from_raw_parts(job.mass, job.n),
            std::slice::from_raw_parts(job.inv_mass, job.n),
            job.phases,
            job.phase,
            lo,
            hi,
            std::slice::from_raw_parts_mut(job.sep_x.add(lo), hi - lo),
            std::slice::from_raw_parts_mut(job.sep_y.add(lo), hi - lo),
        );
    }
}

/// State every participant touches.
struct Shared {
    /// Opened by the ticking thread once the job is published.
    gate: Barrier,
    /// Closed once every participant has written its chunk.
    done: Barrier,
    /// This tick's work. Written only between `done` and the next `gate`.
    job: UnsafeCell<Job>,
    /// Bumped with `Release` once `job` is written; each worker spins on an
    /// `Acquire` load until it *differs from the epoch that worker last ran*,
    /// and only then reads the cell. Waiting for the value to change is what
    /// makes this a real happens-before edge rather than a load that might have
    /// read a stale value — see the `Sync` note.
    publish: AtomicU64,
    /// The inbound mirror of `publish`: each worker bumps it with `Release`
    /// after writing its chunk, and the ticking thread acquires it after
    /// `done`, before reading `sep_x` / `sep_y` back.
    ///
    /// Weaker than its outbound twin, deliberately and knowingly: `completed`
    /// is monotone and never reset, so the ticking thread cannot tell this
    /// generation's bumps from an earlier one's, and `done.wait()` is what
    /// actually orders the reads. See the `Sync` note for why that is still
    /// sound and why this comment does not claim more.
    completed: AtomicU64,
    /// Set by any participant whose chunk panicked, before it reaches `done`.
    /// The ticking thread turns it back into a panic on its own side.
    poisoned: AtomicBool,
    /// Set with `Release` by `Drop` before its single `gate` wait. A worker
    /// breaks only on an `Acquire` load that observed it `true`; it never falls
    /// through to `job` on a stale `false`, because `publish` has not moved and
    /// the wait loop keeps it out of the cell either way.
    shutdown: AtomicBool,
    /// Re-entrancy check for two threads sharing one pool. A real `assert!`,
    /// not a `debug_assert!`: `Simulation` is `Clone`, public and `Send`, so
    /// ticking two clones concurrently is reachable from entirely safe code,
    /// and it would be a data race on `job`. A check that vanished in release
    /// would leave undefined behaviour behind a safe API.
    in_use: AtomicBool,
}

// SAFETY: the only non-`Sync` member is `job`. Access to it is disjoint in
// time: the ticking thread writes it before `gate.wait()` and every worker
// reads it after its own `gate.wait()` returns. No participant touches it
// again until after `done.wait()` has released everyone, and the next write
// happens on the ticking thread before the next `gate.wait()` — by which point
// every worker has already left the previous read window. `Barrier` is
// documented as reusable, so a worker looping straight back into `gate.wait()`
// joins the next generation instead of falling through this one.
//
// Disjointness in time is necessary but not sufficient: it also has to be
// *ordered*. `Barrier` is a mutex and a condvar underneath and so certainly
// orders these accesses, but std documents reuse, not a happens-before edge,
// and an `unsafe` read must not rest on an undocumented implementation detail.
// So the counters carry edges of their own. Be precise about what each one
// actually proves, because the two directions are **not** equally strong:
//
// * **Outbound (`publish`, the `unsafe` read) — independent of the barrier.**
//   An acquire load only synchronises-with a release store when it *reads that
//   store's value*, which a single unconditional load cannot promise. So the
//   worker does not take one: it spins until `publish` differs from the epoch
//   it last ran, and only that observation lets it reach the cell. Having read
//   the bump, it has read the store, and the job written before that store
//   happens-before the read. `shutdown` is on the same footing — the worker
//   leaves on an `Acquire` load that read `Drop`'s `Release` store, rather than
//   falling through to the cell when a stale `false` is observed, which was the
//   one path here that could have been undefined behaviour rather than a hang.
//
// * **Inbound (`completed`, reading `sep_x` / `sep_y` back) — corroborating,
//   not independent.** Each worker bumps `completed` with `Release` after
//   writing its chunk and the ticking thread acquires it after `done`, but
//   nothing here forces that load to observe *this* generation's bumps:
//   `completed` is monotone and never reset, so after the first tick any stale
//   value still satisfies the assertion below it. What guarantees the ticking
//   thread sees this tick's writes is `done.wait()` itself — the barrier is the
//   synchronising primitive on the way back, and the counter documents the
//   intent rather than discharging it. That is sound (`std::sync::Barrier` is
//   `Mutex` + `Condvar`), but it is an argument about the implementation, and
//   this comment says so instead of claiming an edge the code does not build.
unsafe impl Sync for Shared {}

/// Persistent worker pool, one per [`Simulation`] that asked for `T > 1`.
pub(super) struct SeparationPool {
    shared: Arc<Shared>,
    handles: Vec<JoinHandle<()>>,
}

// `Simulation` derives `Debug`; `Barrier` and `UnsafeCell` do not print
// anything useful, and the worker count is the only interesting fact here.
impl fmt::Debug for SeparationPool {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SeparationPool")
            .field("workers", &self.handles.len())
            .finish_non_exhaustive()
    }
}

impl SeparationPool {
    /// Spawn `participants - 1` workers. The caller is the remaining one.
    ///
    /// # Panics
    /// If `participants < 2` — a one-participant scenario must run inline and
    /// build no pool at all.
    pub(super) fn new(participants: usize) -> Self {
        assert!(
            participants >= 2,
            "a pool needs at least two participants; {participants} means the \
             pass should have run inline"
        );

        let shared = Arc::new(Shared {
            gate: Barrier::new(participants),
            done: Barrier::new(participants),
            // Never read before the ticking thread's first publish: a worker
            // only reaches the read after a `gate.wait()` that the ticking
            // thread completes, and `Drop`'s wait breaks out above it.
            job: UnsafeCell::new(Job {
                x: std::ptr::null(),
                y: std::ptr::null(),
                mass: std::ptr::null(),
                inv_mass: std::ptr::null(),
                grid: std::ptr::null(),
                sep_x: std::ptr::null_mut(),
                sep_y: std::ptr::null_mut(),
                n: 0,
                radius_cells: 0.0,
                phases: 1,
                phase: 0,
                participants,
            }),
            publish: AtomicU64::new(0),
            completed: AtomicU64::new(0),
            poisoned: AtomicBool::new(false),
            shutdown: AtomicBool::new(false),
            in_use: AtomicBool::new(false),
        });

        let mut handles = Vec::with_capacity(participants - 1);
        for w in 1..participants {
            let shared = Arc::clone(&shared);
            handles.push(std::thread::spawn(move || worker(&shared, w)));
        }

        Self { shared, handles }
    }

    /// Worker threads this pool spawned — one fewer than its participants.
    ///
    /// Its only caller is `Simulation::worker_thread_count`, which is itself
    /// testkit-only; without the same gate the shipping build carries it as
    /// dead code.
    #[cfg(feature = "testkit")]
    pub(super) fn worker_count(&self) -> usize {
        self.handles.len()
    }

    /// Run one tick's separation across every participant.
    ///
    /// Returns once every chunk has been written, so the caller may read
    /// `sep_x` / `sep_y` immediately afterwards.
    pub(super) fn run(&self, mut job: Job) {
        // The pool, not the scenario, is the authority on how many participants
        // will actually arrive. If the two ever disagreed, `Job::range` would
        // stop partitioning `0..n` and two participants could write the same
        // element — so this is an overwrite rather than an assertion.
        job.participants = self.handles.len() + 1;

        // Hoisted out of the assertion deliberately: inside `assert!` the swap
        // would still run, but inside a `debug_assert!` it would be compiled
        // out, and a reader cannot tell those apart at a glance.
        let already_running = self.shared.in_use.swap(true, Ordering::AcqRel);
        assert!(
            !already_running,
            "this separation pool is already in use — either two threads are \
             ticking simulations that share one pool (they are `Clone`, and \
             clones share it), or an earlier tick panicked inside its chunk"
        );

        // SAFETY: no other participant can be reading the cell. The workers
        // only read it between their own `gate.wait()` and `done.wait()`, and
        // the previous tick's `done.wait()` — or, on the first tick, the fact
        // that no `gate` generation has completed yet — guarantees that window
        // is closed. `in_use` above pins the remaining assumption, that only
        // one thread ever calls `run`.
        unsafe {
            *self.shared.job.get() = job;
        }
        // Release the write above to every worker's `Acquire` load below.
        self.shared.publish.fetch_add(1, Ordering::Release);

        self.shared.gate.wait(); // release the workers; the job is published

        // The ticking thread is participant 0. Catching its unwind is what
        // keeps a panicking chunk a *failure* rather than a *hang*: every
        // participant must reach `done`, or the others block on it forever and
        // `Drop`'s `gate.wait()` then blocks behind them during the unwind.
        // The panic is re-raised below, unchanged, once the rendezvous is done.
        let outcome = catch_unwind(AssertUnwindSafe(|| run_chunk(&job, 0)));
        if outcome.is_err() {
            self.shared.poisoned.store(true, Ordering::Release);
        }

        self.shared.done.wait(); // every chunk is written, or was abandoned
        // Acquire every worker's `Release` bump before the caller reads
        // `sep_x` / `sep_y` back. Corroborating, not load-bearing: `completed`
        // is monotone, so this load cannot distinguish this generation's bumps
        // from an earlier tick's, and `done.wait()` above is what actually
        // orders the reads. See the `Sync` note.
        let completed = self.shared.completed.load(Ordering::Acquire);
        debug_assert!(
            completed >= self.handles.len() as u64,
            "returned from `done` without every worker having reported"
        );
        self.shared.in_use.store(false, Ordering::Release);

        // Re-raise after the barrier, never before it.
        match outcome {
            // This thread's own panic: resume it verbatim, so the original
            // message and location survive. Clear `poisoned` on the way out —
            // the workers almost certainly set it too (they share the inputs),
            // and leaving it set would make the *next* tick blame a worker.
            Err(payload) => {
                self.shared.poisoned.store(false, Ordering::Release);
                std::panic::resume_unwind(payload)
            }
            Ok(()) => assert!(
                !self.shared.poisoned.swap(false, Ordering::AcqRel),
                "a separation worker panicked inside its chunk; its own panic \
                 message was already printed above"
            ),
        }
    }
}

impl Drop for SeparationPool {
    fn drop(&mut self) {
        // Release before the gate wait. `Drop` deliberately does **not** bump
        // `publish`: the worker's wait loop distinguishes "a job arrived" from
        // "the pool is closing" by whether the epoch moved, so leaving it still
        // is what makes the shutdown wakeup unambiguous.
        self.shared.shutdown.store(true, Ordering::Release);
        // One wait, not one per worker: the barrier releases all of them
        // together. Workers break *above* `done`, so waiting on `done` here
        // would block forever.
        self.shared.gate.wait();
        for h in self.handles.drain(..) {
            // A worker catches its own unwind and keeps looping, so this join
            // is `Ok` on every path a panic can take. Discarding the result is
            // therefore not discarding a diagnosis — and `Drop` may itself be
            // running during an unwind, where panicking again would abort.
            let _ = h.join();
        }
    }
}

/// One worker's whole life: park at the gate, run a chunk, report done.
fn worker(shared: &Shared, w: usize) {
    // The publish epoch this worker has already run. `run` bumps `publish`
    // exactly once per tick, before its `gate.wait()`, and `Drop` never bumps
    // it — so "has the epoch moved past `seen`?" distinguishes a real job from
    // a shutdown wakeup by *value*, without asking the barrier anything.
    let mut seen = 0u64;

    loop {
        shared.gate.wait();

        // Wait for one of the two things that can have opened the gate, and do
        // it with loads whose edges stand on their own.
        //
        // The previous form read `shutdown` once, immediately after the gate,
        // and fell through to the job read if it happened to observe `false`.
        // That fall-through was only safe *because* `Barrier` orders the two
        // threads — std documents `Barrier` as reusable, not as establishing a
        // happens-before edge — and its failure mode was not the hang the code
        // anticipated but undefined behaviour: a pool dropped before its first
        // tick would build a slice from a null `job`, and one dropped after a
        // tick would write through a dangling pointer.
        //
        // Spinning on the epoch removes that dependency. Either the ticking
        // thread's `Release` bump becomes visible — and then this `Acquire`
        // load has read that store, which is exactly the condition the memory
        // model requires for the two to synchronise, so the job cell written
        // before it is visible too — or the `Release` store of `shutdown`
        // becomes visible and this worker leaves without touching the cell at
        // all. Atomic stores are guaranteed to become visible in finite time,
        // so neither arm can spin forever; in practice the barrier has already
        // ordered both and the loop runs zero extra iterations.
        let epoch = loop {
            let published = shared.publish.load(Ordering::Acquire);
            if published != seen {
                break Some(published);
            }
            if shared.shutdown.load(Ordering::Acquire) {
                // Do not touch `done` on the way out — the dropping thread is
                // not waiting on it, and a wait here would never be satisfied.
                break None;
            }
            std::hint::spin_loop();
        };
        let Some(published) = epoch else { break };
        debug_assert!(published > 0, "a job is read only after it is published");
        seen = published;

        // SAFETY: the ticking thread wrote the cell before bumping `publish`
        // with `Release`, and the `Acquire` load above returned a value
        // *different from the one this worker last ran*, which on this pool can
        // only be that bump — so this load read that store and the write to the
        // cell happens-before this read. The cell is written again only after
        // the `done.wait()` below has released every participant. See
        // `unsafe impl Sync for Shared`.
        let job = unsafe { *shared.job.get() };

        // Arm the counting allocator for exactly this chunk. Unarmed, an
        // allocation here would be invisible to the measuring thread, and the
        // zero-allocation invariant would silently stop covering the pool.
        let arm = crate::alloc_guard::arm_worker();
        // Catch, flag, and fall through to `done` regardless — an abandoned
        // rendezvous would hang every other participant, and this thread must
        // also stay alive as a participant so the next `gate` (including the
        // one `Drop` uses to shut the pool down) can still complete.
        // `catch_unwind` allocates only on the panic path, which is fatal
        // anyway, so the zero-allocation invariant is untouched.
        let outcome = catch_unwind(AssertUnwindSafe(|| run_chunk(&job, w)));
        drop(arm);
        if outcome.is_err() {
            shared.poisoned.store(true, Ordering::Release);
        }
        // Release this chunk's writes before `done`. Unlike the outbound
        // direction, the acquiring load on the other side cannot prove it read
        // *this* generation's bump — `completed` is monotone and never reset —
        // so `done.wait()` is the primitive that actually orders the ticking
        // thread's read-back. This store makes the intent explicit and costs
        // nothing; it does not, on its own, discharge the edge. See the `Sync`
        // note.
        shared.completed.fetch_add(1, Ordering::Release);

        shared.done.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Owns the buffers a [`Job`] points at, so a test can build one without
    /// hand-rolling ten arguments.
    struct Probe {
        x: Vec<f32>,
        y: Vec<f32>,
        mass: Vec<u8>,
        inv_mass: Vec<f32>,
        grid: SpatialGrid,
        sep_x: Vec<f32>,
        sep_y: Vec<f32>,
    }

    impl Probe {
        /// `n` agents on one point, so every agent has a real push to compute
        /// and a chunk that wrote nothing is visible as a zero.
        fn coincident(n: usize) -> Self {
            let x = vec![2.5f32; n];
            let y = vec![2.5f32; n];
            let mut grid = SpatialGrid::new(8, 8, 1.0, n);
            grid.rebuild(&x, &y);
            Self {
                x,
                y,
                mass: vec![1u8; n],
                inv_mass: vec![1.0f32; n],
                grid,
                sep_x: vec![0.0f32; n],
                sep_y: vec![0.0f32; n],
            }
        }

        /// `phases` is a parameter so a test can violate the pass's own
        /// precondition deliberately.
        fn job(&mut self, participants: usize, phases: u32) -> Job {
            Job {
                x: self.x.as_ptr(),
                y: self.y.as_ptr(),
                mass: self.mass.as_ptr(),
                inv_mass: self.inv_mass.as_ptr(),
                grid: std::ptr::from_ref(&self.grid),
                sep_x: self.sep_x.as_mut_ptr(),
                sep_y: self.sep_y.as_mut_ptr(),
                n: self.x.len(),
                radius_cells: 0.5,
                phases,
                phase: 0,
                participants,
            }
        }
    }

    /// A chunk that panics must **fail** the ticking thread, not wedge the
    /// rendezvous.
    ///
    /// This is the one claim in this module that cannot be checked by reading
    /// it: get it wrong and every other participant blocks on `done` forever,
    /// and `Drop` then blocks on `gate` behind them — so the bug does not
    /// present as a red test, it presents as a merge gate that never returns.
    /// `phases = 0` violates `accumulate_separation_range`'s own `assert!` on
    /// every participant at once, which is the worst case: all four unwind.
    #[test]
    fn a_panicking_chunk_fails_the_caller_instead_of_hanging_the_pool() {
        let mut probe = Probe::coincident(64);
        let pool = SeparationPool::new(4);

        // The panics below are expected; silence the default hook so a passing
        // run does not read as a failing one.
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            let job = probe.job(4, 0); // violates `phases >= 1`
            pool.run(job);
        }));
        std::panic::set_hook(previous);

        assert!(
            outcome.is_err(),
            "a violated precondition must reach the caller as a panic"
        );

        // The pool must still be able to shut down. If a worker had abandoned
        // `done`, or unwound out of its loop and stopped being a participant,
        // this drop would block on `gate` forever. Bounded rather than left to
        // hang: an unbounded wait here does not merely stall this test, it
        // withholds libtest's output for the whole binary, which is what makes
        // a shutdown regression read as a slow machine instead of a red test.
        drop_within_5s(pool, "after a panicking chunk");
    }

    /// The happy path still rendezvouses, the chunks cover every agent, and
    /// the poison flag is not sticky across ticks.
    #[test]
    fn a_clean_chunk_leaves_the_pool_reusable() {
        let mut probe = Probe::coincident(64);
        let pool = SeparationPool::new(3);
        for _ in 0..4 {
            let job = probe.job(3, 1);
            pool.run(job);
        }

        // Every agent is coincident with 63 others, so every agent must have
        // been given a real push. A chunk range that silently covered nothing
        // would leave zeroes behind.
        assert!(
            probe
                .sep_x
                .iter()
                .zip(probe.sep_y.iter())
                .all(|(a, b)| *a != 0.0 || *b != 0.0),
            "some agent was never written; the chunks do not cover 0..n"
        );
        drop_within_5s(pool, "after four clean ticks");
    }

    /// Drop `pool` on a helper thread and fail if it has not finished in five
    /// seconds.
    ///
    /// Load-bearing, not decoration. Every failure mode of the shutdown path is
    /// a **hang**, not a panic: a worker that never leaves its wait loop leaves
    /// `Drop`'s `gate.wait()` blocked forever. libtest has no per-test timeout,
    /// so without this a regression in the wait loop presents as a merge gate
    /// that never returns — indistinguishable in CI from a slow machine —
    /// instead of as a red test. (Confirmed: breaking the epoch comparison to
    /// `published > 0` produces exactly that stall.)
    ///
    /// Five seconds is ~4 orders of magnitude over the real cost; it is a
    /// liveness bound, not a timing assertion, and nothing here measures speed.
    fn drop_within_5s(pool: SeparationPool, what: &str) {
        let (tx, rx) = std::sync::mpsc::channel();
        let h = std::thread::spawn(move || {
            drop(pool);
            let _ = tx.send(());
        });
        assert!(
            rx.recv_timeout(std::time::Duration::from_secs(5)).is_ok(),
            "{what}: the pool did not shut down within 5s — a worker never left \
             its wait loop, and `Drop`'s `gate.wait()` is blocked behind it"
        );
        h.join().expect("the dropping thread panicked");
    }

    /// A pool dropped **before its first tick** shuts down, and does so without
    /// running anything.
    ///
    /// This is the path the shutdown arm exists for. `Job` starts with null
    /// pointers, so a worker that fell through to `unsafe { *shared.job.get() }`
    /// here would build a slice from a null pointer. The worker's wait loop
    /// cannot reach the cell because `publish` has never moved off `0`, so the
    /// only arm left is the `shutdown` one.
    ///
    /// **What this test can and cannot prove.** It proves the shutdown path
    /// *terminates* and does not panic. It does **not** falsify the memory
    /// ordering: the pre-fix form — a single unconditional `shutdown` load that
    /// falls through to the cell — passes this test too, because `Barrier` is
    /// `Mutex` + `Condvar` underneath and does order the two threads on every
    /// machine this can run on. The ordering claim is unobservable to a plain
    /// `cargo test` by construction; falsifying it needs Miri or a weakly
    /// ordered host. That limit is stated here rather than papered over,
    /// because a test whose docstring claims more than it checks is the exact
    /// defect this ticket exists to remove.
    #[test]
    fn a_pool_dropped_before_its_first_tick_shuts_down() {
        for participants in [2usize, 3, 8] {
            let pool = SeparationPool::new(participants);
            assert_eq!(
                pool.shared.publish.load(Ordering::Acquire),
                0,
                "no job may be published before the first tick"
            );
            assert_eq!(
                pool.shared.completed.load(Ordering::Acquire),
                0,
                "a worker ran a chunk before any job was published"
            );
            drop_within_5s(pool, "before first tick");
        }
    }

    /// …and one dropped **after** a tick shuts down without re-running the job
    /// it last ran.
    ///
    /// The stale `Job` still points at `probe`'s buffers, which outlive the
    /// pool here — but in `Simulation`'s real drop order they need not, so a
    /// worker that took the job arm on a shutdown wakeup would write through a
    /// dangling pointer. `publish` stops at the epoch every worker already ran,
    /// so the wait loop takes the shutdown arm instead.
    ///
    /// The observable that makes this more than a liveness check: `completed`
    /// must not advance across the drop, and the output buffers must be
    /// byte-identical afterwards. A shutdown wakeup that re-entered `run_chunk`
    /// would move both. See the sibling test for what this still cannot prove.
    #[test]
    fn a_pool_dropped_after_a_tick_shuts_down() {
        let mut probe = Probe::coincident(32);
        let pool = SeparationPool::new(4);
        pool.run(probe.job(4, 1));
        assert_eq!(
            pool.shared.publish.load(Ordering::Acquire),
            1,
            "one tick must publish exactly one epoch"
        );

        let completed_before = pool.shared.completed.load(Ordering::Acquire);
        let (sep_x_before, sep_y_before) = (probe.sep_x.clone(), probe.sep_y.clone());
        // The premise: the tick actually wrote something, or "unchanged across
        // the drop" would be satisfied by two buffers of zeroes.
        assert!(
            sep_x_before.iter().any(|v| *v != 0.0),
            "the tick wrote nothing, so the comparison below would be vacuous"
        );

        let shared = Arc::clone(&pool.shared);
        drop_within_5s(pool, "after a tick");

        assert_eq!(
            shared.completed.load(Ordering::Acquire),
            completed_before,
            "a worker ran another chunk on the shutdown wakeup"
        );
        assert_eq!(
            (probe.sep_x, probe.sep_y),
            (sep_x_before, sep_y_before),
            "the output buffers moved after the last tick returned; a worker \
             re-entered the stale job on its way out"
        );
    }
}
