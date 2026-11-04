//! Opt-in per-thread execution diagnostics, absent from ordinary builds.
//! Counts include synchronous reentry on the measured thread, including other runtimes.
pub mod allocations;
#[cfg(test)]
mod memory;
pub mod storage;

use std::cell::Cell;

#[derive(Debug, Default, Clone, Copy)]
pub struct ExecutionCounts {
    pub execution_polls: u64,
    pub execution_guards: u64,
    pub module_validations: u64,
    pub native_entries: u64,
    pub native_argument_reads: u64,
    pub native_parameter_scans: u64,
    pub conversion_contexts: u64,
    pub conversion_type_checks: u64,
    pub conversion_value_checks: u64,
    pub type_argument_validations: u64,
    pub value_type_matches: u64,
    pub root_pack_registrations: u64,
    pub sequence_admissions: u64,
    pub sequence_contract_reads: u64,
    pub sequence_payload_reads: u64,
    pub gc_safepoints: u64,
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
    pub call_selections: u64,
    pub call_admissions: u64,
    pub call_returns: u64,
    pub call_descriptors: u64,
    pub window_allocations: u64,
    pub window_slots_examined: u64,
    pub bank_bytes_initialized: u64,
    pub bank_bytes_moved: u64,
    pub argument_transfers: u64,
}

pub(crate) enum Event {
    ExecutionPoll,
    ExecutionGuard,
    ModuleValidation,
    NativeEntry,
    NativeArgumentRead,
    NativeParameterScan,
    ConversionContext,
    ConversionTypeCheck,
    ConversionValueCheck,
    TypeArgumentValidation,
    ValueTypeMatch,
    RootPackRegistration,
    SequenceAdmission,
    SequenceContractRead,
    SequencePayloadRead,
    GcSafepoint,
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
    CallSelection,
    CallAdmission,
    CallReturn,
    CallDescriptor,
    WindowAllocation,
    WindowSlotsExamined,
    BankBytesInitialized,
    BankBytesMoved,
    ArgumentTransfer,
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
            Event::ExecutionPoll => &mut counts.execution_polls,
            Event::ExecutionGuard => &mut counts.execution_guards,
            Event::ModuleValidation => &mut counts.module_validations,
            Event::NativeEntry => &mut counts.native_entries,
            Event::NativeArgumentRead => &mut counts.native_argument_reads,
            Event::NativeParameterScan => &mut counts.native_parameter_scans,
            Event::ConversionContext => &mut counts.conversion_contexts,
            Event::ConversionTypeCheck => &mut counts.conversion_type_checks,
            Event::ConversionValueCheck => &mut counts.conversion_value_checks,
            Event::TypeArgumentValidation => &mut counts.type_argument_validations,
            Event::ValueTypeMatch => &mut counts.value_type_matches,
            Event::RootPackRegistration => &mut counts.root_pack_registrations,
            Event::SequenceAdmission => &mut counts.sequence_admissions,
            Event::SequenceContractRead => &mut counts.sequence_contract_reads,
            Event::SequencePayloadRead => &mut counts.sequence_payload_reads,
            Event::GcSafepoint => &mut counts.gc_safepoints,
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
            Event::CallSelection => &mut counts.call_selections,
            Event::CallAdmission => &mut counts.call_admissions,
            Event::CallReturn => &mut counts.call_returns,
            Event::CallDescriptor => &mut counts.call_descriptors,
            Event::WindowAllocation => &mut counts.window_allocations,
            Event::WindowSlotsExamined => &mut counts.window_slots_examined,
            Event::BankBytesInitialized => &mut counts.bank_bytes_initialized,
            Event::BankBytesMoved => &mut counts.bank_bytes_moved,
            Event::ArgumentTransfer => &mut counts.argument_transfers,
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
