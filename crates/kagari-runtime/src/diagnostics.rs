//! Opt-in per-thread execution diagnostics, absent from ordinary builds.
//! Counts include synchronous reentry on the measured thread, including other runtimes.
pub mod allocations;
#[cfg(test)]
mod memory;
pub mod storage;

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
    /// Closed metadata graph admission attempts, including readonly queries.
    pub metadata_graph_admissions: u64,
    pub slow_boundaries: u64,
    pub call_selections: u64,
    pub call_admissions: u64,
    pub call_returns: u64,
    /// Committed scalar self-tail replacements, without another physical call.
    pub tail_replacements: u64,
    pub call_descriptors: u64,
    /// Successful capacity changes of managed/scalar/initialization value banks.
    pub bank_reservations: u64,
    /// Execution-source directory/bank/dependency examinations, including retries and holes.
    pub execution_root_slots_examined: u64,
    /// Bytes initialized in appended/reused cells, including self-tail initialization resets.
    pub bank_bytes_initialized: u64,
    /// Old capacity bytes offered to realloc on growth; not measured physical copying.
    pub bank_bytes_reallocated: u64,
    /// Committed frame parameter assignments, including zero-sized Unit.
    pub argument_transfers: u64,
    /// New GC Host registrations in Runtime memory scopes; clones share a lease.
    pub root_registrations: u64,
    pub conversion_type_checks: u64,
    pub native_type_mappings: u64,
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
    MetadataGraphAdmission,
    SlowBoundary,
    CallSelection,
    CallAdmission,
    CallReturn,
    TailReplacement,
    CallDescriptor,
    BankReservation,
    ExecutionRootSlotExamined,
    BankBytesInitialized,
    BankBytesReallocated,
    ArgumentTransfer,
    RootRegistration,
    ConversionTypeCheck,
    NativeTypeMapping,
}

thread_local! {
    static ACTIVE: Cell<Option<ExecutionCounts>> = const { Cell::new(None) };
}

pub(crate) fn record(event: Event) {
    record_amount(event, 1);
}

pub(crate) fn record_amount(event: Event, amount: u64) {
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
            Event::MetadataGraphAdmission => &mut counts.metadata_graph_admissions,
            Event::SlowBoundary => &mut counts.slow_boundaries,
            Event::CallSelection => &mut counts.call_selections,
            Event::CallAdmission => &mut counts.call_admissions,
            Event::CallReturn => &mut counts.call_returns,
            Event::TailReplacement => &mut counts.tail_replacements,
            Event::CallDescriptor => &mut counts.call_descriptors,
            Event::BankReservation => &mut counts.bank_reservations,
            Event::ExecutionRootSlotExamined => &mut counts.execution_root_slots_examined,
            Event::BankBytesInitialized => &mut counts.bank_bytes_initialized,
            Event::BankBytesReallocated => &mut counts.bank_bytes_reallocated,
            Event::ArgumentTransfer => &mut counts.argument_transfers,
            Event::RootRegistration => &mut counts.root_registrations,
            Event::ConversionTypeCheck => &mut counts.conversion_type_checks,
            Event::NativeTypeMapping => &mut counts.native_type_mappings,
        };
        *counter += amount;
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
