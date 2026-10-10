//! Opt-in per-thread execution diagnostics, absent from ordinary builds.
//! Counts include synchronous reentry on the measured thread, including other runtimes.
use std::cell::Cell;

#[derive(Debug, Default, Clone, Copy)]
pub struct ExecutionCounts {
    pub method_preparations: u64,
    pub shared_preparations: u64,
    pub operation_preparations: u64,
    pub environment_allocations: u64,
    pub metadata_validations: u64,
    pub slow_boundaries: u64,
}

pub(crate) enum Event {
    MethodPreparation,
    SharedPreparation,
    OperationPreparation,
    EnvironmentAllocation,
    MetadataValidation,
    SlowBoundary,
}

thread_local! {
    static ACTIVE: Cell<Option<ExecutionCounts>> = const { Cell::new(None) };
}

pub(crate) fn record(event: Event) {
    ACTIVE.with(|active| {
        let Some(mut counts) = active.get() else {
            return;
        };
        let counter = match event {
            Event::MethodPreparation => &mut counts.method_preparations,
            Event::SharedPreparation => &mut counts.shared_preparations,
            Event::OperationPreparation => &mut counts.operation_preparations,
            Event::EnvironmentAllocation => &mut counts.environment_allocations,
            Event::MetadataValidation => &mut counts.metadata_validations,
            Event::SlowBoundary => &mut counts.slow_boundaries,
        };
        *counter += 1;
        active.set(Some(counts));
    });
}

struct Reset;

impl Drop for Reset {
    fn drop(&mut self) {
        ACTIVE.with(|active| active.set(None));
    }
}

/// Count preparation/allocation attempts, graph validation entries and region exits.
/// This instrumentation changes execution cost and must not be used for throughput.
pub fn measure<T>(operation: impl FnOnce() -> T) -> (T, ExecutionCounts) {
    ACTIVE.with(|active| {
        assert!(active.get().is_none(), "nested execution diagnostics");
        active.set(Some(ExecutionCounts::default()));
    });
    let reset = Reset;
    let result = operation();
    let counts = ACTIVE.with(|active| active.get().expect("active execution diagnostics"));
    drop(reset);
    (result, counts)
}
