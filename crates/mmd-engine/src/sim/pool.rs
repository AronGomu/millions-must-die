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
    /// Bumped with `Release` once `job` is written, loaded with `Acquire`
    /// before it is read. This is the outbound happens-before edge, stated
    /// explicitly rather than borrowed from `Barrier` — see the `Sync` note.
    publish: AtomicU64,
    /// The inbound mirror of `publish`: each worker bumps it with `Release`
    /// after writing its chunk, and the ticking thread acquires it after
    /// `done`, before reading `sep_x` / `sep_y` back. Without it the argument
    /// would be one-directional — proven on the way out, assumed on the way
    /// back.
    completed: AtomicU64,
    /// Set by any participant whose chunk panicked, before it reaches `done`.
    /// The ticking thread turns it back into a panic on its own side.
    poisoned: AtomicBool,
    /// Set by `Drop` before its single `gate` wait; workers break on it.
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
// So both directions carry their own edge, and neither is left to the barrier:
// `publish` is released by the ticking thread after it writes the cell and
// acquired by every worker before it reads; `completed` is released by every
// worker after it writes its chunk and acquired by the ticking thread after
// `done`, before it reads `sep_x` / `sep_y` back.
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
        // `sep_x` / `sep_y` back. This closes the loop opened by `publish`.
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
        // Release before the gate wait, so the `Acquire` load in `worker` that
        // is ordered after the same barrier generation is guaranteed to see it.
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
    loop {
        shared.gate.wait();
        if shared.shutdown.load(Ordering::Acquire) {
            // Do not touch `done` on the way out — the dropping thread is not
            // waiting on it, and a wait here would never be satisfied.
            break;
        }

        // Acquire the counter the ticking thread released after writing the
        // cell. This is the documented half of the ordering argument; the
        // barrier supplies the timing, this supplies the edge.
        let published = shared.publish.load(Ordering::Acquire);
        debug_assert!(published > 0, "a job is read only after it is published");

        // SAFETY: the ticking thread wrote the cell before the `gate.wait()`
        // above returned, and writes it again only after the `done.wait()`
        // below has released every participant. The `Acquire` load above pairs
        // with its `Release` bump, so the write is ordered before this read.
        // See `unsafe impl Sync for Shared`.
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
        // Release this chunk's writes to the ticking thread's `Acquire` load
        // after `done`. The barrier almost certainly orders them already; this
        // is the half of the argument the memory model actually promises.
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
        // this drop would block on `gate` forever — and the test would hang
        // here rather than fail.
        drop(pool);
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
        drop(pool);
    }
}
