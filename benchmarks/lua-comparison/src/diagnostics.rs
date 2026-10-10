//! Allocation and runtime-protocol counts, kept outside throughput measurements.
use kagari_embed::{context::ExecutionContext, runtime::KagariRuntime};
use kagari_runtime::{
    diagnostics::{self, allocations},
    module::LoadedModule,
    value::Value,
};

#[global_allocator]
static ALLOCATOR: allocations::Allocator = allocations::Allocator;

pub(super) fn measure(
    runtime: &KagariRuntime,
    module: &LoadedModule,
    context: &ExecutionContext,
    entry: &str,
    args: &[Value],
    expected: i32,
) {
    count(runtime, module, context, entry, args, expected, "cold");
    for _ in 0..2 {
        let report = runtime.execute(module, entry, args, context).unwrap();
        assert_eq!(
            report.return_value.value(runtime.runtime().gc()),
            Some(Value::I32(expected))
        );
    }
    count(runtime, module, context, entry, args, expected, "warm");
}

fn count(
    runtime: &KagariRuntime,
    module: &LoadedModule,
    context: &ExecutionContext,
    entry: &str,
    args: &[Value],
    expected: i32,
    phase: &str,
) {
    let before = runtime.runtime().gc().stats();
    let ((report, execution), allocations) = allocations::measure(|| {
        diagnostics::measure(|| runtime.execute(module, entry, args, context))
    });
    let report = report.unwrap();
    assert_eq!(
        report.return_value.value(runtime.runtime().gc()),
        Some(Value::I32(expected))
    );
    let after = runtime.runtime().gc().stats();
    println!(
        "DIAGNOSTIC,{entry},phase={phase},args={args:?},requests={},requested_bytes={},net_bytes={},objects={},collections={},environments_delta={},applications_delta={},method_preparations={},interface_call_preparations={},shared_preparations={},operation_preparations={},native_preparations={},layout_scope_preparations={},layout_operand_preparations={},layout_comparisons={},environment_allocations={},metadata_validations={},slow_boundaries={},driver_admissions={},await_polls={}",
        allocations.requests,
        allocations.requested_bytes,
        allocations.net_bytes,
        after.allocated_objects as i128 - before.allocated_objects as i128
            + (after.reclaimed_objects - before.reclaimed_objects) as i128,
        after.collections - before.collections,
        after.environments as i128 - before.environments as i128,
        after.method_applications as i128 - before.method_applications as i128,
        execution.method_preparations,
        execution.interface_call_preparations,
        execution.shared_preparations,
        execution.operation_preparations,
        execution.native_preparations,
        execution.layout_scope_preparations,
        execution.layout_operand_preparations,
        execution.layout_comparisons,
        execution.environment_allocations,
        execution.metadata_validations,
        execution.slow_boundaries,
        execution.driver_admissions,
        execution.await_polls,
    );
}
