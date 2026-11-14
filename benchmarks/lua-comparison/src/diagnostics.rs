//! Allocation and runtime-protocol counts, kept outside throughput measurements.
use kagari::{context::ExecutionContext, runtime::Runtime};
use kagari_runtime::{
    diagnostics::{self, allocations},
    module::{InstalledProgram, entry::ProgramEntry},
};

#[global_allocator]
static ALLOCATOR: allocations::Allocator = allocations::Allocator;

pub(super) fn measure(
    runtime: &mut Runtime,
    module: &InstalledProgram,
    context: &ExecutionContext,
    entry: ProgramEntry<'_>,
    expected: i32,
) {
    count(runtime, module, context, entry, expected, "cold");
    for _ in 0..2 {
        let report = runtime.execute(module, entry, &[], context).unwrap();
        assert_eq!(report.return_value.as_i32(), Some(expected));
    }
    count(runtime, module, context, entry, expected, "warm");
}

fn count(
    runtime: &mut Runtime,
    module: &InstalledProgram,
    context: &ExecutionContext,
    entry: ProgramEntry<'_>,
    expected: i32,
    phase: &str,
) {
    let before = runtime.gc_stats();
    let ((report, execution), allocations) = allocations::measure(|| {
        diagnostics::measure(|| runtime.execute(module, entry, &[], context))
    });
    let report = report.unwrap();
    assert_eq!(report.return_value.as_i32(), Some(expected));
    let after = runtime.gc_stats();
    let entry = entry.label();
    println!(
        "DIAGNOSTIC,{entry},phase={phase},requests={},requested_bytes={},net_bytes={},live_objects_delta={},completed_cycles={},provider_units_delta={},root_registrations_delta={},method_preparations={},interface_call_preparations={},shared_preparations={},operation_preparations={},native_preparations={},layout_scope_preparations={},layout_operand_preparations={},layout_comparisons={},environment_allocations={},metadata_graph_admissions={},slow_boundaries={},driver_admissions={},await_polls={}",
        allocations.requests,
        allocations.requested_bytes,
        allocations.net_bytes,
        after.objects as i128 - before.objects as i128,
        after.completed_cycles - before.completed_cycles,
        after.provider_units as i128 - before.provider_units as i128,
        after.root_registrations as i128 - before.root_registrations as i128,
        execution.method_preparations,
        execution.interface_call_preparations,
        execution.shared_preparations,
        execution.operation_preparations,
        execution.native_preparations,
        execution.layout_scope_preparations,
        execution.layout_operand_preparations,
        execution.layout_comparisons,
        execution.environment_allocations,
        execution.metadata_graph_admissions,
        execution.slow_boundaries,
        execution.driver_admissions,
        execution.await_polls,
    );
    println!(
        "TRANSITION,{entry},phase={phase},selections={},admissions={},returns={},descriptors={},bank_reservations={},execution_root_slots_examined={},initialized_bytes={},reallocated_capacity_bytes={},arguments={},root_registrations={}",
        execution.call_selections,
        execution.call_admissions,
        execution.call_returns,
        execution.call_descriptors,
        execution.bank_reservations,
        execution.execution_root_slots_examined,
        execution.bank_bytes_initialized,
        execution.bank_bytes_reallocated,
        execution.argument_transfers,
        execution.root_registrations,
    );
}
