//! Diagnostic-only allocation accounting. Source backing and reporting are outside the window.

use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};

use serde::Serialize;

#[derive(Clone, Copy, Default, Serialize)]
pub struct Counts {
    pub requests: usize,
    pub requested_bytes: usize,
    pub frees: usize,
    pub freed_bytes: usize,
    pub live_bytes: usize,
    pub peak_bytes: usize,
}

thread_local! {
    static COUNTS: Cell<Option<Counts>> = const { Cell::new(None) };
}

pub struct Allocator;

fn update(allocated: usize, freed: usize, request: bool) {
    COUNTS.with(|cell| {
        if let Some(mut counts) = cell.get() {
            counts.requests += usize::from(request);
            counts.requested_bytes += allocated;
            counts.frees += usize::from(freed != 0);
            counts.freed_bytes += freed;
            counts.live_bytes = counts
                .live_bytes
                .checked_sub(freed)
                .expect("owned allocation window")
                + allocated;
            counts.peak_bytes = counts.peak_bytes.max(counts.live_bytes);
            cell.set(Some(counts));
        }
    });
}

// SAFETY: Every operation delegates the original pointer/layout to System; the
// thread-local counters never allocate and do not change allocator semantics.
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            update(layout.size(), 0, true);
        }
        pointer
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc_zeroed(layout) };
        if !pointer.is_null() {
            update(layout.size(), 0, true);
        }
        pointer
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let result = unsafe { System.realloc(pointer, layout, size) };
        if !result.is_null() {
            update(size, layout.size(), true);
        }
        result
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        update(0, layout.size(), false);
        unsafe { System.dealloc(pointer, layout) };
    }
}

pub fn begin() {
    COUNTS.with(|cell| {
        assert!(cell.replace(Some(Counts::default())).is_none());
    });
}

pub fn snapshot() -> Counts {
    COUNTS.with(|cell| cell.get().expect("active measurement"))
}

pub fn finish() -> Counts {
    COUNTS.with(|cell| cell.take().expect("active measurement"))
}
