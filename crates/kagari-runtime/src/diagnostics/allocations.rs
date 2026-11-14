//! Opt-in allocation accounting shared by runtime probes and the benchmark executable.
//! Register `Allocator` as the executable's global allocator to enable measurements.
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};

#[derive(Debug, Default, Clone, Copy)]
pub struct Counts {
    pub requests: usize,
    pub requested_bytes: usize,
    pub net_bytes: i128,
    /// Maximum net requested bytes since measurement or the last peak reset.
    pub peak_net_bytes: i128,
}

thread_local! {
    static ACTIVE: Cell<Option<Counts>> = const { Cell::new(None) };
}

pub struct Allocator;

#[cfg(test)]
#[global_allocator]
static ALLOCATOR: Allocator = Allocator;

fn record(size: usize, released: usize, request: bool) {
    let _ = ACTIVE.try_with(|active| {
        if let Some(mut counts) = active.get() {
            counts.requests += usize::from(request);
            counts.requested_bytes += size;
            counts.net_bytes += size as i128 - released as i128;
            counts.peak_net_bytes = counts.peak_net_bytes.max(counts.net_bytes);
            active.set(Some(counts));
        }
    });
}

// SAFETY: forward unchanged pointer/layout contracts to System. Const TLS counters
// neither allocate nor invoke callbacks. Record only successful allocation requests.
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            record(layout.size(), 0, true);
        }
        ptr
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc_zeroed(layout) };
        if !ptr.is_null() {
            record(layout.size(), 0, true);
        }
        ptr
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let result = unsafe { System.realloc(ptr, layout, size) };
        if !result.is_null() {
            record(size, layout.size(), true);
        }
        result
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        record(0, layout.size(), false);
        unsafe { System.dealloc(ptr, layout) };
    }
}

struct Reset;

impl Drop for Reset {
    fn drop(&mut self) {
        ACTIVE.with(|active| active.set(None));
    }
}

/// Snapshot the calling thread's active allocation measurement.
pub fn current() -> Option<Counts> {
    ACTIVE.with(Cell::get)
}

/// Start a peak interval at the current net bytes, preserving request/net totals.
/// Capture the prior snapshot first to retain a preceding interval's peak.
/// Panics if this thread has no active measurement.
pub fn reset_peak() {
    ACTIVE.with(|active| {
        let mut counts = active.get().expect("active allocation measurement");
        counts.peak_net_bytes = counts.net_bytes;
        active.set(Some(counts));
    });
}

/// Count allocator requests and net requested bytes, excluding allocator overhead.
/// The caller must keep phase boundaries explicit; deallocations may predate measurement.
pub fn measure<T>(operation: impl FnOnce() -> T) -> (T, Counts) {
    ACTIVE.with(|active| {
        assert!(active.get().is_none(), "nested allocation measurement");
        active.set(Some(Counts::default()));
    });
    let reset = Reset;
    let result = operation();
    let counts = current().expect("active allocation measurement");
    drop(reset);
    (result, counts)
}

#[cfg(test)]
mod tests {
    use crate::diagnostics::allocations::{current, measure, reset_peak};
    use std::hint::black_box;

    #[test]
    fn transient_peak_survives_release_and_reset_preserves_request_totals() {
        let ((first, released), final_counts) = measure(|| {
            let bytes = black_box(Vec::<u8>::with_capacity(8192));
            let first = current().unwrap();
            drop(bytes);
            let released = current().unwrap();
            reset_peak();
            let bytes = black_box(Vec::<u8>::with_capacity(4096));
            drop(bytes);
            (first, released)
        });
        assert_eq!(first.net_bytes, 8192);
        assert_eq!(first.peak_net_bytes, 8192);
        assert_eq!(released.net_bytes, 0);
        assert_eq!(released.peak_net_bytes, 8192);
        assert_eq!(final_counts.net_bytes, 0);
        assert_eq!(final_counts.peak_net_bytes, 4096);
        assert_eq!(final_counts.requests, 2);
        assert_eq!(final_counts.requested_bytes, 8192 + 4096);
    }
}
