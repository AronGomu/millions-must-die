//! Zero-allocation-per-frame contract: counting allocator + measure guard.
//!
//! **Code-health invariant, not a performance gate** (T12, reframed by T28).
//! It catches accidental per-frame heap allocation; it says nothing about
//! throughput and consumes no timing threshold. Runs on every merge under a
//! short deterministic policy — see `docs/05-testing.md`.

use std::sync::{Mutex, OnceLock};

use mmd_engine::alloc_guard::{
    CountingAllocator, MeasureGuard, alloc_count, is_counting, reset_count,
};

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

/// Global counting flag is process-wide — serialize these tests.
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
