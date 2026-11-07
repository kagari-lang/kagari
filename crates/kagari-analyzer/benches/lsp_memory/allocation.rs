//! Successful System allocation requests and live layouts; these are not RSS.

use serde::Serialize;
use std::{
    alloc::{GlobalAlloc, Layout, System},
    sync::atomic::{AtomicUsize, Ordering},
};

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

// SAFETY: Pointers and layouts are forwarded unchanged. Atomics allocate no memory,
// including callbacks during process startup and shutdown or acquisition work.
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
            freed(layout.size());
            allocated(size);
        }
        result
    }
}

pub fn begin_window() {
    PEAK.store(LIVE.load(Ordering::Relaxed), Ordering::Relaxed);
}

pub fn snapshot() -> Counts {
    // Sampling occurs while the acquisition owner is idle. Atomic loads still do
    // not constitute a general allocator-wide stop-the-world snapshot.
    Counts {
        requests: REQUESTS.load(Ordering::Relaxed),
        requested_bytes: REQUESTED.load(Ordering::Relaxed),
        frees: FREES.load(Ordering::Relaxed),
        freed_bytes: FREED.load(Ordering::Relaxed),
        live_bytes: LIVE.load(Ordering::Relaxed),
        peak_bytes: PEAK.load(Ordering::Relaxed),
    }
}
