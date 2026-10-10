//! Allocation and runtime-protocol counts, kept outside throughput measurements.
use kagari_embed::{context::ExecutionContext, runtime::KagariRuntime};
use kagari_runtime::{diagnostics, module::LoadedModule, value::Value};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};

#[derive(Default, Clone, Copy)]
struct Counts {
    requests: usize,
    requested_bytes: usize,
    net_bytes: i128,
}

thread_local! {
    static ACTIVE: Cell<Option<Counts>> = const { Cell::new(None) };
}

struct Allocator;

#[global_allocator]
static ALLOCATOR: Allocator = Allocator;

fn record(size: usize, released: usize, request: bool) {
    let _ = ACTIVE.try_with(|active| {
        if let Some(mut counts) = active.get() {
            counts.requests += usize::from(request);
            counts.requested_bytes += size;
            counts.net_bytes += size as i128 - released as i128;
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
    ACTIVE.with(|active| {
        assert!(active.get().is_none());
        active.set(Some(Counts::default()));
    });
    let reset = Reset;
    let (report, execution) =
        diagnostics::measure(|| runtime.execute(module, entry, args, context));
    let allocations = ACTIVE.with(|active| active.get().unwrap());
    drop(reset);
    let report = report.unwrap();
    assert_eq!(
        report.return_value.value(runtime.runtime().gc()),
        Some(Value::I32(expected))
    );
    let after = runtime.runtime().gc().stats();
    println!(
        "DIAGNOSTIC,{entry},phase={phase},args={args:?},requests={},requested_bytes={},net_bytes={},objects={},collections={},environments_delta={},applications_delta={},method_preparations={},shared_preparations={},operation_preparations={},native_preparations={},environment_allocations={},metadata_validations={},slow_boundaries={}",
        allocations.requests,
        allocations.requested_bytes,
        allocations.net_bytes,
        after.allocated_objects as i128 - before.allocated_objects as i128
            + (after.reclaimed_objects - before.reclaimed_objects) as i128,
        after.collections - before.collections,
        after.environments as i128 - before.environments as i128,
        after.method_applications as i128 - before.method_applications as i128,
        execution.method_preparations,
        execution.shared_preparations,
        execution.operation_preparations,
        execution.native_preparations,
        execution.environment_allocations,
        execution.metadata_validations,
        execution.slow_boundaries,
    );
}
