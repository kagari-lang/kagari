//! Count allocations on the test thread, excluding parallel tests and setup.
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};

thread_local! {
    static ALLOCATIONS: Cell<Option<usize>> = const { Cell::new(None) };
}

struct CountingAllocator;

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

fn count() {
    let _ = ALLOCATIONS.try_with(|count| {
        if let Some(value) = count.get() {
            count.set(Some(value + 1));
        }
    });
}

// SAFETY: All allocations and deallocations preserve System's layout/pointer
// contract. The const TLS counter neither allocates nor exposes allocated memory.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count();
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        count();
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        count();
        unsafe { System.realloc(pointer, layout, size) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) }
    }
}

struct Probe;

impl Drop for Probe {
    fn drop(&mut self) {
        ALLOCATIONS.set(None);
    }
}

pub fn assert_no_allocations(action: impl FnOnce()) {
    assert_eq!(ALLOCATIONS.replace(Some(0)), None);
    let probe = Probe;
    action();
    let count = ALLOCATIONS.get().unwrap();
    drop(probe);
    assert_eq!(count, 0, "sealed operation cleanup allocated");
}
