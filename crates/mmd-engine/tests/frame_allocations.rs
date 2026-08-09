//! Zero-allocation-per-frame contract: counting allocator + measure guard.
//!
//! **Code-health invariant, not a performance gate** (T12, reframed by T28).
//! It catches accidental per-frame heap allocation; it says nothing about
//! throughput and consumes no timing threshold. Runs on every merge under a
//! short deterministic policy — see `docs/05-testing.md`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use mmd_engine::alloc_guard::{
    CountingAllocator, MeasureGuard, alloc_count, is_counting, reset_count,
};
use mmd_engine::runtime::InputAction;
use mmd_engine::scenario::Cell;
use mmd_engine::sim::SpatialGrid;
use mmd_engine::testkit::{
    COLLISION_SPRITE_SCENE, FIXTURE_DENSE_V1, GridSpec, Harness, ScenarioSource, scene_path,
};

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

/// The allocation counter is process-wide — serialize these tests so two of
/// them never share one measure scope.
///
/// The mutex is necessary but **not sufficient**: it orders the tests in this
/// binary, and it cannot stop libtest's own harness thread (or a sibling test
/// thread starting up / tearing down) from allocating while a guard is open.
/// That residual cross-talk is what
/// `foreign_thread_allocations_do_not_leak_into_a_measure_scope` pins down, and
/// why `MeasureGuard` counts the entering thread only.
fn lock_alloc_tests() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// Injected per-frame allocation must still hard-fail the invariant.
#[test]
fn alloc_invariant_still_enforced() {
    let _lock = lock_alloc_tests();
    reset_count();
    let guard = MeasureGuard::enter();
    // Injected Vec growth must be visible to the counting allocator.
    let mut v = Vec::new();
    v.push(1u8);
    v.push(2u8);
    v.reserve(1024);
    std::hint::black_box(&v);
    let n = guard.allocations();
    assert!(n > 0, "injected Vec growth must count, got {n}");
    assert!(guard.check_zero().is_err());
    let err = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        guard.assert_zero();
    }));
    assert!(err.is_err(), "assert_zero must hard-fail on allocs");
}

#[test]
fn warmup_allocation_passes() {
    let _lock = lock_alloc_tests();
    reset_count();
    // Warmup / setup allocs outside the measure guard.
    let mut warm = Vec::with_capacity(4096);
    warm.extend_from_slice(&[1u8, 2, 3, 4]);
    std::hint::black_box(&warm);

    let guard = MeasureGuard::enter();
    // No heap traffic while measuring.
    let x = std::hint::black_box(warm.len() + 7);
    assert_eq!(x, 11);
    assert_eq!(guard.allocations(), 0);
    guard.assert_zero();
}

#[test]
fn guard_resets_between_trials() {
    let _lock = lock_alloc_tests();
    reset_count();

    let n1 = {
        let g = MeasureGuard::enter();
        let forced = vec![0u8; 128];
        std::hint::black_box(forced);
        let n = g.allocations();
        assert!(n >= 1, "trial1 must see alloc");
        n
    };
    assert!(!is_counting());

    let n2 = {
        let g = MeasureGuard::enter();
        // Fresh delta — prior trial must not leak into this count.
        assert_eq!(g.allocations(), 0, "trial2 starts at 0 delta");
        let forced = vec![1u8; 256];
        std::hint::black_box(forced);
        g.allocations()
    };
    assert!(!is_counting());
    assert!(n1 >= 1 && n2 >= 1);
    // Counters are independent deltas (not cumulative across guards).
    let _ = (n1, n2);
}

#[test]
fn panic_restores_guard() {
    let _lock = lock_alloc_tests();
    reset_count();
    assert!(!is_counting());

    let result = std::panic::catch_unwind(|| {
        let _g = MeasureGuard::enter();
        assert!(is_counting());
        panic!("unwind under measure guard");
    });
    assert!(result.is_err());
    assert!(
        !is_counting(),
        "Drop must restore counting flag after panic"
    );

    // Later measure still works and starts clean.
    let before = alloc_count();
    {
        let g = MeasureGuard::enter();
        assert!(is_counting());
        assert_eq!(g.allocations(), 0);
        // no alloc
    }
    assert!(!is_counting());
    assert_eq!(alloc_count(), before);
}

/// A thread the measured code did not start must not be able to add to the
/// count. This is the deterministic form of the flake that used to fail
/// `warmup_allocation_passes` and `panic_restores_guard` under parallel load:
/// libtest's harness thread and the sibling test threads allocate on their own
/// schedule, and while the counter was process-wide those allocations landed
/// inside whichever measure scope happened to be open.
///
/// The mutex above cannot fix that — foreign threads never take it. Only
/// scoping the count to the thread that entered the guard can, so this test
/// fails outright against a process-wide counter rather than 3 runs in 100.
#[test]
fn foreign_thread_allocations_do_not_leak_into_a_measure_scope() {
    let _lock = lock_alloc_tests();
    reset_count();

    let stop = Arc::new(AtomicBool::new(false));
    let started = Arc::new(AtomicBool::new(false));
    let noisy = {
        let stop = Arc::clone(&stop);
        let started = Arc::clone(&started);
        std::thread::spawn(move || {
            // Allocate continuously on a thread that never enters a guard.
            while !stop.load(Ordering::Relaxed) {
                let churn: Vec<u8> = Vec::with_capacity(4096);
                std::hint::black_box(&churn);
                started.store(true, Ordering::Relaxed);
            }
        })
    };
    // Do not measure until the foreign thread is demonstrably running, or the
    // test would pass by racing past a thread that had not allocated yet.
    // Bounded by iterations rather than wall clock: this suite asserts on no
    // timing, and an unbounded spin would hang the gate instead of failing it.
    let mut spins = 0u64;
    while !started.load(Ordering::Relaxed) {
        std::hint::spin_loop();
        spins += 1;
        assert!(
            spins < 5_000_000_000,
            "the noisy thread never allocated; this test cannot prove anything \
             about a scope it never contended with"
        );
    }

    let observed = {
        let guard = MeasureGuard::enter();
        // Hold the scope open long enough for the foreign thread to allocate
        // many times over. No allocation happens on *this* thread.
        for _ in 0..200_000 {
            std::hint::black_box(guard.allocations());
        }
        guard.finish()
    };

    stop.store(true, Ordering::Relaxed);
    noisy.join().expect("noisy thread joins");

    assert_eq!(
        observed, 0,
        "a measure scope counted {observed} allocation(s) made by a thread it \
         never entered; the counter must be scoped to the measuring thread"
    );
}

/// The positive control for the test above, and for the whole worker arm.
///
/// `foreign_thread_allocations_do_not_leak_into_a_measure_scope` pins that an
/// *unarmed* thread is invisible. That is exactly why
/// `a_threaded_collision_tick_allocates_nothing` would pass vacuously if the
/// pool's workers never armed themselves — an invisible thread allocates zero
/// by construction. This pins the other half: a thread that *does* arm is
/// counted into the scope the measuring thread is reading. Without it, the
/// claim that threading made the guard stronger is untested.
#[test]
fn an_armed_thread_allocations_do_reach_a_measure_scope() {
    let _lock = lock_alloc_tests();
    reset_count();

    let go = Arc::new(AtomicBool::new(false));
    let done = Arc::new(AtomicBool::new(false));
    let worker = {
        let go = Arc::clone(&go);
        let done = Arc::clone(&done);
        std::thread::spawn(move || {
            while !go.load(Ordering::Acquire) {
                std::hint::spin_loop();
            }
            // Arm exactly the way a separation worker arms around its chunk.
            let _arm = mmd_engine::alloc_guard::arm_worker();
            let churn: Vec<u8> = Vec::with_capacity(4096);
            std::hint::black_box(&churn);
            drop(_arm);
            done.store(true, Ordering::Release);
        })
    };

    let observed = {
        let guard = MeasureGuard::enter();
        go.store(true, Ordering::Release);
        let mut spins = 0u64;
        while !done.load(Ordering::Acquire) {
            std::hint::spin_loop();
            spins += 1;
            assert!(spins < 5_000_000_000, "the armed thread never finished");
        }
        guard.finish()
    };
    worker.join().expect("armed thread joins");

    assert!(
        observed > 0,
        "an armed worker's allocation was invisible to the measuring thread; \
         a zero-allocation assertion would then say nothing about the pool"
    );
}

/// The hitbox overlay does not get to spend the frame budget it was added
/// under: packing a ring per agent reuses a buffer reserved at load.
///
/// Both states are measured. Rings *off* is the cheap half and would pass on
/// its own even if the visible path reallocated every frame, so measuring only
/// one of them would leave the interesting case uncovered.
///
/// This lives here, not in `render_correctness.rs`, because the counting
/// allocator is installed in *this* test binary — a `MeasureGuard` anywhere
/// else records nothing and the assertion would pass vacuously.
#[test]
fn ring_packing_allocates_nothing() {
    let _lock = lock_alloc_tests();
    reset_count();

    let mut h = Harness::builder(ScenarioSource::path(scene_path(COLLISION_SPRITE_SCENE)))
        .agents(512)
        .build()
        .expect("collision scene");
    assert!(
        h.sim().collision().enabled(),
        "the scene must have a body, or there are no rings to measure"
    );
    assert!(
        h.runtime().hitboxes_visible(),
        "rings are on by default; this test must measure the visible path"
    );

    // Warm-up outside the scope: whatever the first pack grows, it grows now.
    h.step_exact(2);
    h.runtime_mut().pack_groups();
    let packed = h.runtime().ring_instances().len();
    assert_eq!(
        packed,
        h.alive_count(),
        "warm-up must pack a full set of rings"
    );

    let guard = MeasureGuard::enter();
    for _ in 0..8 {
        h.runtime_mut().pack_groups();
        std::hint::black_box(h.runtime().ring_instances().len());
    }
    assert_eq!(guard.allocations(), 0, "packing rings allocated");
    guard.assert_zero();
    drop(guard);

    // …and with the overlay hidden. `clear()` must keep the capacity, so
    // toggling back on does not re-grow the buffer inside a later frame.
    h.runtime_mut().apply_action(InputAction::ToggleHitboxes);
    assert!(!h.runtime().hitboxes_visible());
    h.runtime_mut().pack_groups();

    let guard = MeasureGuard::enter();
    for _ in 0..8 {
        h.runtime_mut().pack_groups();
        std::hint::black_box(h.runtime().ring_instances().len());
    }
    assert_eq!(
        guard.allocations(),
        0,
        "packing with rings hidden allocated"
    );
    guard.assert_zero();
    drop(guard);

    // Toggling back on inside a measured scope must not allocate either.
    h.runtime_mut().apply_action(InputAction::ToggleHitboxes);
    let guard = MeasureGuard::enter();
    h.runtime_mut().pack_groups();
    std::hint::black_box(h.runtime().ring_instances().len());
    assert_eq!(guard.allocations(), 0, "re-showing the rings allocated");
    guard.assert_zero();
    drop(guard);
    assert_eq!(h.runtime().ring_instances().len(), packed);
}

#[test]
fn spatial_rebuild_allocates_nothing() {
    let _lock = lock_alloc_tests();
    reset_count();

    let n = 512;
    let mut grid = SpatialGrid::new(64, 64, 1.0, n);
    let xs: Vec<f32> = (0..n).map(|i| (i % 64) as f32 + 0.5).collect();
    let ys: Vec<f32> = (0..n).map(|i| (i / 64) as f32 + 0.5).collect();
    grid.rebuild(&xs, &ys); // warm-up: any lazy growth happens here

    let guard = MeasureGuard::enter();
    for _ in 0..8 {
        grid.rebuild(&xs, &ys);
        std::hint::black_box(grid.len());
    }
    assert_eq!(guard.allocations(), 0);
    guard.assert_zero();
}

/// The whole tick, not just the index: rebuilding the grid and accumulating the
/// repulsion sums must reuse the buffers the sim reserved at construction.
#[test]
fn a_collision_tick_allocates_nothing() {
    let _lock = lock_alloc_tests();
    reset_count();

    let mut h = Harness::fixture(FIXTURE_DENSE_V1).build().expect("dense");
    assert!(h.sim().collision().enabled(), "fixture must have a body");
    h.step_exact(2); // warm-up outside the measured scope

    let guard = MeasureGuard::enter();
    h.step_exact(10);
    std::hint::black_box(h.tick_index());
    assert_eq!(guard.allocations(), 0);
    guard.assert_zero();
}

/// The pool does not get to weaken the invariant it was allowed to exist under.
///
/// A measure scope is armed per-thread, so an *unarmed* worker's allocations
/// would simply be invisible here and this test would pass without proving
/// anything. Each worker therefore arms itself with `alloc_guard::arm_worker`
/// around its chunk: its allocations land in the same process-wide counter this
/// scope reads, and the zero below covers the workers as well as the ticking
/// thread. That is why introducing threads makes the guard stronger rather than
/// weaker.
#[test]
fn a_threaded_collision_tick_allocates_nothing() {
    let _lock = lock_alloc_tests();
    reset_count();

    let spec = GridSpec::new(32, 32, Cell { x: 31, y: 16 })
        .with_spawns(vec![Cell { x: 1, y: 16 }])
        .with_collision(128, 256)
        .with_agents(256)
        .with_separation_threads(4);
    let mut h = Harness::grid(spec)
        .build()
        .expect("threaded collision grid");
    assert!(h.sim().collision().enabled(), "the grid must have a body");
    assert_eq!(
        h.sim().worker_thread_count(),
        3,
        "the pass must actually be running on workers, or this test measures \
         the inline path and proves nothing about them"
    );
    h.step_exact(2); // warm-up outside the measured scope

    let guard = MeasureGuard::enter();
    h.step_exact(10);
    std::hint::black_box(h.tick_index());
    assert_eq!(guard.allocations(), 0);
    guard.assert_zero();
}
