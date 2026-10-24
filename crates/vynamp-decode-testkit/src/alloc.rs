//! Allocation counter for "zero allocations in the hot path" tests.
//!
//! This is the only module of the workspace that needs `unsafe`
//! (`unsafe impl GlobalAlloc`); see the lint exception in this crate's
//! Cargo.toml and in CONTRIBUTING.md. The allocator is only active in test
//! binaries that declare it:
//!
//! ```ignore
//! #[global_allocator]
//! static A: vynamp_decode_testkit::alloc::CountingAlloc = vynamp_decode_testkit::alloc::CountingAlloc;
//! ```

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

thread_local! {
    static ALLOCATIONS: Cell<u64> = const { Cell::new(0) };
}

fn bump() {
    // `try_with` so allocations during thread teardown never panic.
    let _ = ALLOCATIONS.try_with(|c| c.set(c.get() + 1));
}

/// Global allocator that delegates to [`System`] and counts allocations per thread.
pub struct CountingAlloc;

// SAFETY: every method delegates unchanged to `std::alloc::System`, which
// upholds the `GlobalAlloc` contract; the counter only touches a const-initialised
// thread-local `Cell` and never allocates.
unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        bump();
        // SAFETY: caller upholds the `GlobalAlloc::alloc` contract.
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        bump();
        // SAFETY: caller upholds the `GlobalAlloc::alloc_zeroed` contract.
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr`/`layout` come from a prior call to this allocator, i.e. `System`.
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        bump();
        // SAFETY: caller upholds the `GlobalAlloc::realloc` contract.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

/// Number of allocations (alloc, alloc_zeroed, realloc) made by the current
/// thread since it started. Stays 0 unless [`CountingAlloc`] is the global allocator.
pub fn allocations_on_this_thread() -> u64 {
    ALLOCATIONS.try_with(Cell::get).unwrap_or(0)
}
