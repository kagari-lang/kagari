//! Opt-in thread-local work attribution for the library's sort kernels.
use std::cell::Cell;

#[derive(Debug, Default, Clone, Copy)]
pub struct SortCounts {
    pub scalar_leaf_calls: u64,
    pub scalar_leaf_elements: u64,
    /// Upper bound, not a measurement of stack bytes touched by Rust's sort.
    pub scalar_leaf_stack_bound_bytes: usize,
    /// Rust's internal scalar moves are not observable through the slice API.
    pub scalar_leaf_internal_moves: Option<u64>,
    pub comparisons: u64,
    pub scratch_copies: u64,
    pub output_copies: u64,
    pub recovery_copies: u64,
    pub scratch_peak_bytes: usize,
    /// Returned callbacks, including errors; unwinding calls have no return timestamp.
    pub callback_max_ns: u128,
}

thread_local! {
    static ACTIVE: Cell<Option<SortCounts>> = const { Cell::new(None) };
}

pub(crate) fn update(edit: impl FnOnce(&mut SortCounts)) {
    ACTIVE.with(|active| {
        if let Some(mut counts) = active.get() {
            edit(&mut counts);
            active.set(Some(counts));
        }
    });
}

struct Reset;

impl Drop for Reset {
    fn drop(&mut self) {
        ACTIVE.with(|active| active.set(None));
    }
}

/// Count one synchronous operation. These counters must not be used for timings.
/// Callback delay is synchronous elapsed time, not incremental collector latency.
pub fn measure<T>(operation: impl FnOnce() -> T) -> (T, SortCounts) {
    ACTIVE.with(|active| {
        assert!(active.get().is_none(), "nested sort diagnostics");
        active.set(Some(SortCounts::default()));
    });
    let reset = Reset;
    let result = operation();
    let counts = ACTIVE.with(|active| active.get().expect("active sort diagnostics"));
    drop(reset);
    (result, counts)
}
