//! Counting global allocator + scoped measure guard (test/bench).
//!
//! Install [`CountingAllocator`] as `#[global_allocator]` in the binary under test
//! (app `main`, `frame_allocations` integration test). Counting is inert until a
//! [`MeasureGuard`] (or [`start_measure`]) enables it.
//!
//! # Visibility limit
//!
//! Counts only allocations that pass through the Rust global allocator.
//! SDL/driver internal C `malloc` / GPU driver heaps are **not** visible here.
//!
//! # Thread scope
//!
//! A measure scope is **per-thread**: [`MeasureGuard::enter`] arms counting on
//! the calling thread only, and [`CountingAllocator`] records an allocation
//! only when the allocating thread is itself inside a scope. Allocations made
//! by any other thread while a scope is open are invisible to it.
//!
//! The *counter* behind it is still one process-wide number, so two threads
//! that both hold a scope at the same time would still share it. Nothing does
//! that today — the only non-test consumer is `bench::runner`, and the tests
//! serialize on a mutex — but per-thread arming is not per-thread counting,
//! and the difference matters the moment a second measuring thread exists.
//!
//! That narrowing is deliberate and load-bearing. The counter is one process-
//! wide number, so with a process-wide enable flag *any* thread's allocation
//! landed inside whichever scope happened to be open — including the test
//! harness's own bookkeeping, which made the zero-allocation assertions fail a
//! few runs in a hundred under parallel load. Per-thread arming removes that
//! cross-talk without weakening a single assertion.
//!
//! The cost of the narrowing, stated plainly: work that a measured frame hands
//! to another thread would not be counted. Nothing in this project does that —
//! the simulation, renderer and bench runner are single-threaded, and
//! `bench::runner` is the only non-test consumer — so no coverage is lost
//! today. Introducing a worker thread on the frame path means this guard must
//! be revisited before it can still claim "zero allocations per frame".

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicU64, Ordering};

/// Honest report note: C/SDL/driver heaps stay outside this counter.
pub const ALLOC_VISIBILITY_NOTE: &str =
    "project Rust global allocator only; SDL/driver C malloc and GPU driver heaps not visible";

thread_local! {
    /// Per-thread measure-scope flag.
    ///
    /// `const`-initialized on purpose: a `const` thread-local needs no lazy
    /// initialization and registers no destructor, so on every target this
    /// project builds for — Linux, Windows and macOS all have native TLS —
    /// reading it from inside the global allocator cannot allocate and cannot
    /// recurse. (On a target without native TLS, std would fall back to a
    /// boxed OS-key storage, and that first access *would* allocate inside
    /// `record`; adding such a target means revisiting this.) Every access
    /// goes through [`counting_here`] / [`set_counting_here`], which use
    /// `try_with` so a thread whose TLS is already being destroyed reads
    /// `false` instead of panicking.
    static COUNTING: Cell<bool> = const { Cell::new(false) };
}

#[inline]
fn counting_here() -> bool {
    COUNTING.try_with(Cell::get).unwrap_or(false)
}

#[inline]
fn set_counting_here(enabled: bool) {
    let _ = COUNTING.try_with(|c| c.set(enabled));
}

static ALLOC_COUNT: AtomicU64 = AtomicU64::new(0);

/// Global allocator wrapper: delegates to [`System`], optionally counts.
///
/// Binaries install with:
/// ```ignore
/// #[global_allocator]
/// static GLOBAL: mmd_engine::alloc_guard::CountingAllocator =
///     mmd_engine::alloc_guard::CountingAllocator;
/// ```
pub struct CountingAllocator;

impl CountingAllocator {
    #[inline]
    fn record() {
        if counting_here() {
            ALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
        }
    }
}

// SAFETY: forwards to System after optional count; System is a valid GlobalAlloc.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        Self::record();
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        Self::record();
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        Self::record();
        unsafe { System.realloc(ptr, layout, new_size) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}

/// True while a measure scope is active **on the calling thread**.
#[inline]
pub fn is_counting() -> bool {
    counting_here()
}

/// Monotonic allocation counter (does not reset on guard exit).
#[inline]
pub fn alloc_count() -> u64 {
    ALLOC_COUNT.load(Ordering::Relaxed)
}

/// Reset counter to zero (tests / trial boundaries).
#[inline]
pub fn reset_count() {
    ALLOC_COUNT.store(0, Ordering::Relaxed);
}

/// Enable counting on the calling thread. Returns its previous enabled flag.
#[inline]
pub fn start_measure() -> bool {
    let previous = counting_here();
    set_counting_here(true);
    previous
}

/// Restore the calling thread's counting flag (pair with [`start_measure`]).
#[inline]
pub fn end_measure(previous_enabled: bool) {
    set_counting_here(previous_enabled);
}

/// RAII measure scope: counts Rust global allocations while held.
///
/// Drop restores previous enable flag (including on unwind).
///
/// Deliberately **not `Send`**: the enable flag lives in the entering thread's
/// TLS, so a guard dropped on another thread would disarm the wrong thread and
/// leave the entering one counting forever. The `PhantomData` below makes that
/// a compile error instead of a silent corruption.
#[derive(Debug)]
pub struct MeasureGuard {
    _not_send: PhantomData<*const ()>,
    prev_enabled: bool,
    start_count: u64,
    armed: bool,
}

impl MeasureGuard {
    /// Begin measuring. Nested guards stack via previous-flag restore.
    pub fn enter() -> Self {
        let prev_enabled = start_measure();
        let start_count = alloc_count();
        Self {
            _not_send: PhantomData,
            prev_enabled,
            start_count,
            armed: true,
        }
    }

    /// Allocations observed since enter (delta).
    #[inline]
    pub fn allocations(&self) -> u64 {
        alloc_count().saturating_sub(self.start_count)
    }

    /// Hard-fail when any project Rust allocation occurred in scope.
    pub fn assert_zero(&self) {
        let n = self.allocations();
        assert!(
            n == 0,
            "expected 0 project Rust frame allocations, got {n} ({ALLOC_VISIBILITY_NOTE})"
        );
    }

    /// Result form of zero check.
    pub fn check_zero(&self) -> Result<(), u64> {
        let n = self.allocations();
        if n == 0 { Ok(()) } else { Err(n) }
    }

    /// End scope early; returns allocation delta. Idempotent with Drop.
    pub fn finish(mut self) -> u64 {
        let n = self.allocations();
        self.disarm();
        n
    }

    fn disarm(&mut self) {
        if self.armed {
            end_measure(self.prev_enabled);
            self.armed = false;
        }
    }
}

impl Drop for MeasureGuard {
    fn drop(&mut self) {
        self.disarm();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Probe that reports whether `T: Send`, resolved at compile time.
    ///
    /// Method resolution prefers an inherent method over a trait method, and
    /// the inherent one below exists only while `T: Send` — so `is_send()`
    /// answers truthfully for any `T` without a `static_assertions`
    /// dependency, and the test flips the moment the bound changes.
    struct SendProbe<T>(PhantomData<T>);

    trait MaybeSend {
        fn is_send(&self) -> bool {
            false
        }
    }
    impl<T> MaybeSend for SendProbe<T> {}
    impl<T: Send> SendProbe<T> {
        fn is_send(&self) -> bool {
            true
        }
    }

    /// The scope flag lives in the entering thread's TLS, so a guard that
    /// moved to another thread would disarm the wrong thread and leave the
    /// entering one counting forever. That must stay a compile error.
    #[test]
    fn guard_is_not_send_so_a_scope_cannot_change_threads() {
        assert!(
            !SendProbe::<MeasureGuard>(PhantomData).is_send(),
            "MeasureGuard became Send; a scope could then be dropped on a \
             thread that never entered it, disarming the wrong TLS flag"
        );
        // Control: the probe does report Send for a type that is Send, so a
        // probe stuck at `false` cannot pass this test vacuously.
        assert!(SendProbe::<u64>(PhantomData).is_send());
    }

    #[test]
    fn counting_is_scoped_to_the_entering_thread() {
        let guard = MeasureGuard::enter();
        assert!(is_counting(), "the entering thread is armed");
        let elsewhere = std::thread::spawn(|| {
            let seen = is_counting();
            // Allocate on a thread with no scope of its own.
            let v = vec![0u8; 512];
            std::hint::black_box(&v);
            seen
        })
        .join()
        .expect("probe thread joins");
        assert!(!elsewhere, "a foreign thread must never observe the scope");
        assert_eq!(
            guard.allocations(),
            0,
            "a foreign thread's allocations must stay outside this scope"
        );
    }

    #[test]
    fn guard_restores_flag_on_drop() {
        assert!(!is_counting());
        {
            let g = MeasureGuard::enter();
            assert!(is_counting());
            assert_eq!(g.allocations(), 0);
        }
        assert!(!is_counting());
    }
}
