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

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Honest report note: C/SDL/driver heaps stay outside this counter.
pub const ALLOC_VISIBILITY_NOTE: &str =
    "project Rust global allocator only; SDL/driver C malloc and GPU driver heaps not visible";

static COUNTING: AtomicBool = AtomicBool::new(false);
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
        if COUNTING.load(Ordering::Relaxed) {
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

/// True while a measure scope is active.
#[inline]
pub fn is_counting() -> bool {
    COUNTING.load(Ordering::Relaxed)
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

/// Enable counting. Returns previous enabled flag.
#[inline]
pub fn start_measure() -> bool {
    COUNTING.swap(true, Ordering::SeqCst)
}

/// Restore counting enabled flag (pair with [`start_measure`]).
#[inline]
pub fn end_measure(previous_enabled: bool) {
    COUNTING.store(previous_enabled, Ordering::SeqCst);
}

/// RAII measure scope: counts Rust global allocations while held.
///
/// Drop restores previous enable flag (including on unwind).
#[derive(Debug)]
pub struct MeasureGuard {
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
