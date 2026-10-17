//! Count requested bytes through runtime teardown; do not use these runs for timing.
use kagari::{context::ExecutionContext, engine::KagariEngine, program::PreparedProgram};
use kagari_runtime::{
    diagnostics::allocations::{self, Counts},
    gc::GcHeapStats,
    native::value_handle::ScriptValue,
};

struct Snapshot {
    allocations: Counts,
    heap: GcHeapStats,
}

pub(super) fn run(
    engine: &KagariEngine,
    program: &PreparedProgram,
    context: &ExecutionContext,
    population: i32,
) {
    for count in [1, population] {
        for name in [
            "points",
            "pairs",
            "interfaces",
            "wide_interfaces",
            "generic_interfaces",
            "copied_interface",
            "nodes",
            "numbers",
        ] {
            let lifecycle = || {
                let mut runtime = engine.runtime(context.clone());
                let loaded = runtime.load_program(program, Default::default()).unwrap();
                runtime.runtime().collect_garbage().unwrap();
                let snapshot = || Snapshot {
                    allocations: allocations::current().unwrap(),
                    heap: runtime.runtime().gc().stats(),
                };
                let setup = snapshot();
                let values: ScriptValue = runtime
                    .execute_typed(&loaded, name, (count,), context)
                    .unwrap();
                let constructed = snapshot();
                runtime.runtime().collect_garbage().unwrap();
                let live = snapshot();
                let sparse: Option<ScriptValue> = (name == "points").then(|| {
                    runtime
                        .execute_typed(&loaded, "sparse", (values.clone(), count), context)
                        .unwrap()
                });
                drop(values);
                runtime.runtime().collect_garbage().unwrap();
                let released = snapshot();
                drop(sparse);
                runtime.runtime().collect_garbage().unwrap();
                let empty = snapshot();
                // Repeat the same population to expose retained-capacity reuse.
                let again: ScriptValue = runtime
                    .execute_typed(&loaded, name, (count,), context)
                    .unwrap();
                runtime.runtime().collect_garbage().unwrap();
                let reused = snapshot();
                drop(again);
                runtime.runtime().collect_garbage().unwrap();
                assert_eq!(
                    runtime.runtime().gc().stats().allocated_objects,
                    setup.heap.allocated_objects
                );
                [setup, constructed, live, released, empty, reused]
            };
            // Immutable prepared-program caches can initialize on first execution
            // and outlive Runtime. Warm that shared owner before measuring its
            // separate runtime-owned lifecycle; teardown must still balance.
            let (_, first_use) = allocations::measure(lifecycle);
            let (phases, total) = allocations::measure(lifecycle);
            println!("shared_first_use,{name},count={count},allocations={first_use:?}");
            for (phase, snapshot) in [
                "setup",
                "constructed",
                "live",
                "released",
                "empty",
                "reused",
            ]
            .into_iter()
            .zip(phases)
            {
                println!(
                    "memory,{name},count={count},phase={phase},allocations={:?},heap={:?}",
                    snapshot.allocations, snapshot.heap
                );
            }
            println!("teardown,{name},count={count},allocations={total:?}");
            assert_eq!(total.net_bytes, 0, "runtime teardown for {name}");
        }
    }
    field_loops(engine, program, context);
}

fn field_loops(engine: &KagariEngine, program: &PreparedProgram, context: &ExecutionContext) {
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime.load_program(program, Default::default()).unwrap();
    let point: ScriptValue = runtime
        .execute_typed(&loaded, "point", (), context)
        .unwrap();
    for name in ["read_fields", "write_fields", "fields"] {
        let execute = || {
            let result: i32 = runtime
                .execute_typed(&loaded, name, (point.clone(), 50_000_i32), context)
                .unwrap();
            assert_eq!(result, 50_000);
        };
        execute();
        let (_, counts) = allocations::measure(execute);
        println!("field_loop,{name},iterations=50000,allocations={counts:?}");
    }
}
