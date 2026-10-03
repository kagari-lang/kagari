//! Per-thread counters exclude allocations made by the test harness on other threads.
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    hint::black_box,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Counts {
    pub(super) allocations: usize,
    reallocations: usize,
    deallocations: usize,
    pub(super) requested_bytes: usize,
}
thread_local! {
    static ACTIVE: Cell<Option<Counts>> = const { Cell::new(None) };
}
struct Allocator;
#[global_allocator]
static ALLOCATOR: Allocator = Allocator;

fn record(update: impl FnOnce(&mut Counts)) {
    let _ = ACTIVE.try_with(|active| {
        if let Some(mut counts) = active.get() {
            update(&mut counts);
            active.set(Some(counts));
        }
    });
}
// SAFETY: this wrapper forwards the exact pointer/layout contracts to System;
// counters use const-initialized thread-local cells and never allocate or reenter it.
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record(|counts| {
            counts.allocations += 1;
            counts.requested_bytes += layout.size();
        });
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        record(|counts| {
            counts.allocations += 1;
            counts.requested_bytes += layout.size();
        });
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        record(|counts| {
            counts.reallocations += 1;
            counts.requested_bytes += size;
        });
        unsafe { System.realloc(ptr, layout, size) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        record(|counts| counts.deallocations += 1);
        unsafe { System.dealloc(ptr, layout) };
    }
}
struct Reset;
impl Drop for Reset {
    fn drop(&mut self) {
        ACTIVE.with(|active| active.set(None));
    }
}
pub(super) fn measured(body: impl FnOnce()) -> (Counts, Duration) {
    let reset = Reset;
    ACTIVE.with(|active| {
        assert!(active.get().is_none());
        active.set(Some(Counts::default()));
    });
    let start = Instant::now();
    body();
    let elapsed = start.elapsed();
    let counts = ACTIVE.with(|active| active.get().unwrap());
    drop(reset);
    (counts, elapsed)
}

pub(super) fn verify_counter() {
    let (counts, _) = measured(|| {
        let mut values = vec![black_box(1u64)];
        black_box(&mut values).push(black_box(2u64));
        black_box(&values);
    });
    assert_eq!(counts.allocations, 1);
    assert_eq!(counts.reallocations, 1);
    assert_eq!(counts.deallocations, 1);
    assert!(counts.requested_bytes >= 24);
}
