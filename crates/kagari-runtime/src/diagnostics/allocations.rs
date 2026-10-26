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
    /// Maximum signed net requested bytes since the measurement began, including zero.
    pub peak_net_bytes: i128,
    /// Maximum signed net requested bytes since the last checkpoint (or scope start).
    pub interval_peak_net_bytes: i128,
    pub net_bytes: i128,
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
            counts.interval_peak_net_bytes = counts.interval_peak_net_bytes.max(counts.net_bytes);
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

/// Snapshot counters and start a new high-water interval at the current net balance.
///
/// Returns `None` outside a measurement. Cumulative counters and the scope peak are
/// unchanged; only the next interval's peak is reset. This performs no allocation.
/// Values are signed balances relative to scope entry, not process-wide live bytes.
pub fn checkpoint() -> Option<Counts> {
    ACTIVE.with(|active| {
        let previous = active.get()?;
        active.set(Some(Counts {
            interval_peak_net_bytes: previous.net_bytes,
            ..previous
        }));
        Some(previous)
    })
}

/// Count allocator requests, net bytes and high-water balances on the calling thread.
///
/// The scope begins at zero and excludes allocator overhead, other threads and
/// allocations predating entry. A successful realloc replaces the old requested
/// size atomically in this model; transient old/new backing inside the allocator
/// is not observable. Releases of pre-scope allocations can make the balance
/// negative. Scope lifetimes must therefore enclose allocation and release when
/// interpreting the high-water balance as requested live bytes. Checkpoints split
/// intervals without resetting the scope peak. Unwinding ends the measurement.
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
    use super::{checkpoint, current, measure, record};

    #[test]
    fn scoped_high_water_tracks_realloc_release_and_intervals() {
        let (_, counts) = measure(|| {
            record(64, 0, true);
            record(128, 64, true);
            record(0, 128, false);
            let first = checkpoint().unwrap();
            assert_eq!((first.net_bytes, first.peak_net_bytes), (0, 128));
            assert_eq!(first.interval_peak_net_bytes, 128);
            record(32, 0, true);
            record(0, 32, false);
            let second = checkpoint().unwrap();
            assert_eq!(second.interval_peak_net_bytes, 32);
            assert_eq!(second.peak_net_bytes, 128);
            // A pre-scope release is a signed balance, not absolute heap liveness.
            record(0, 16, false);
            checkpoint().unwrap();
            record(8, 0, true);
            assert_eq!(current().unwrap().interval_peak_net_bytes, -8);
        });
        assert_eq!(counts.requests, 4);
        assert_eq!(counts.requested_bytes, 232);
        assert_eq!(counts.net_bytes, -8);
        assert_eq!(counts.peak_net_bytes, 128);
        assert!(current().is_none());
        assert!(checkpoint().is_none());
    }
}
