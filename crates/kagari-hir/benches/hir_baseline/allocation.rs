//! Process-cumulative requested-byte accounting, used only by hir_memory.

use std::{
    alloc::{GlobalAlloc, Layout, System},
    sync::atomic::{AtomicUsize, Ordering},
};

use serde::Serialize;

static REQUESTS: AtomicUsize = AtomicUsize::new(0);
static REQUESTED: AtomicUsize = AtomicUsize::new(0);
static FREES: AtomicUsize = AtomicUsize::new(0);
static FREED: AtomicUsize = AtomicUsize::new(0);
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone, Copy, Default, Serialize)]
pub struct Counts {
    pub requests: usize,
    pub requested_bytes: usize,
    pub frees: usize,
    pub freed_bytes: usize,
    pub live_bytes: usize,
    pub peak_bytes: usize,
}

pub struct Allocator;

fn allocated(size: usize) {
    REQUESTS.fetch_add(1, Ordering::Relaxed);
    REQUESTED.fetch_add(size, Ordering::Relaxed);
    let live = LIVE.fetch_add(size, Ordering::Relaxed) + size;
    PEAK.fetch_max(live, Ordering::Relaxed);
}

fn freed(size: usize) {
    FREES.fetch_add(1, Ordering::Relaxed);
    FREED.fetch_add(size, Ordering::Relaxed);
    LIVE.fetch_sub(size, Ordering::Relaxed);
}

// SAFETY: Original pointers/layouts are forwarded unchanged to System. The
// counters use allocation-free atomics, including during process startup/shutdown.
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            allocated(layout.size());
        }
        pointer
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc_zeroed(layout) };
        if !pointer.is_null() {
            allocated(layout.size());
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) };
        freed(layout.size());
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let result = unsafe { System.realloc(pointer, layout, size) };
        if !result.is_null() {
            // One successful request and one freed old layout, even for in-place
            // realloc. Requested bytes are not allocator calls, capacity or RSS.
            freed(layout.size());
            allocated(size);
        }
        result
    }
}

pub fn begin_window() {
    // Only peak is windowed. Resetting live/cumulative counts would lose ownership
    // of source/catalog allocations later freed during the lifecycle.
    PEAK.store(LIVE.load(Ordering::Relaxed), Ordering::Relaxed);
}

pub fn snapshot() -> Counts {
    // The benchmark executes synchronously on one thread. Atomic accounting also
    // covers other threads, but sampling is not a global allocator stop-the-world.
    Counts {
        requests: REQUESTS.load(Ordering::Relaxed),
        requested_bytes: REQUESTED.load(Ordering::Relaxed),
        frees: FREES.load(Ordering::Relaxed),
        freed_bytes: FREED.load(Ordering::Relaxed),
        live_bytes: LIVE.load(Ordering::Relaxed),
        peak_bytes: PEAK.load(Ordering::Relaxed),
    }
}
