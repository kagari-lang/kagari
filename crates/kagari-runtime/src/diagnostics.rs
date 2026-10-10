//! Opt-in per-thread execution diagnostics, absent from ordinary builds.
//! Counts include synchronous reentry on the measured thread, including other runtimes.
pub mod allocations;
#[cfg(test)]
mod memory;

use std::cell::Cell;

#[derive(Debug, Default, Clone, Copy)]
pub struct ExecutionCounts {
    pub driver_admissions: u64,
    pub await_polls: u64,
    pub method_preparations: u64,
    pub interface_call_preparations: u64,
    pub shared_preparations: u64,
    pub operation_preparations: u64,
    pub native_preparations: u64,
    pub layout_scope_preparations: u64,
    pub layout_operand_preparations: u64,
    pub layout_comparisons: u64,
    pub environment_allocations: u64,
    pub metadata_validations: u64,
    pub slow_boundaries: u64,
}

pub(crate) enum Event {
    DriverAdmission,
    AwaitPoll,
    MethodPreparation,
    InterfaceCallPreparation,
    SharedPreparation,
    OperationPreparation,
    NativePreparation,
    LayoutScopePreparation,
    LayoutOperandPreparation,
    LayoutComparison,
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
            Event::DriverAdmission => &mut counts.driver_admissions,
            Event::AwaitPoll => &mut counts.await_polls,
            Event::MethodPreparation => &mut counts.method_preparations,
            Event::InterfaceCallPreparation => &mut counts.interface_call_preparations,
            Event::SharedPreparation => &mut counts.shared_preparations,
            Event::OperationPreparation => &mut counts.operation_preparations,
            Event::NativePreparation => &mut counts.native_preparations,
            Event::LayoutScopePreparation => &mut counts.layout_scope_preparations,
            Event::LayoutOperandPreparation => &mut counts.layout_operand_preparations,
            Event::LayoutComparison => &mut counts.layout_comparisons,
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
