//! Count requested bytes through runtime teardown; do not use these runs for timing.
use kagari::{context::ExecutionContext, engine::KagariEngine, program::PreparedProgram};
use kagari_runtime::{
    diagnostics::{
        allocations::{self, Counts},
        storage::StorageUsage,
    },
    gc::GcHeapStats,
    native::value_handle::ScriptValue,
};

struct Snapshot {
    allocations: Counts,
    heap: GcHeapStats,
    storage: StorageUsage,
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
                    allocations: allocations::checkpoint().unwrap(),
                    heap: runtime.runtime().gc().stats(),
                    storage: runtime.runtime().gc().storage_usage().unwrap(),
                };
                let setup = snapshot();
                let mut post_setup_peak = setup.allocations.net_bytes;
                let mut capture = || {
                    let next = snapshot();
                    post_setup_peak = post_setup_peak.max(next.allocations.interval_peak_net_bytes);
                    next
                };
                let values: ScriptValue = runtime
                    .execute_typed(&loaded, name, (count,), context)
                    .unwrap();
                let constructed = capture();
                runtime.runtime().collect_garbage().unwrap();
                let live = capture();
                let sparse: Option<ScriptValue> = (name == "points").then(|| {
                    runtime
                        .execute_typed(&loaded, "sparse", (values.clone(), count), context)
                        .unwrap()
                });
                drop(values);
                runtime.runtime().collect_garbage().unwrap();
                let released = capture();
                drop(sparse);
                runtime.runtime().collect_garbage().unwrap();
                let empty = capture();
                // Three equal cycles expose capacity growth after the first reuse.
                let mut reused = None;
                let mut plateau = None;
                for cycle in 0..3 {
                    let again: ScriptValue = runtime
                        .execute_typed(&loaded, name, (count,), context)
                        .unwrap();
                    runtime.runtime().collect_garbage().unwrap();
                    let live = capture();
                    drop(again);
                    runtime.runtime().collect_garbage().unwrap();
                    let empty = capture();
                    let net = (live.allocations.net_bytes, empty.allocations.net_bytes);
                    if let Some(expected) = plateau {
                        assert_eq!(net, expected, "{name} reuse cycle {cycle}");
                    }
                    plateau = Some(net);
                    reused = Some(live);
                }
                assert_eq!(
                    runtime.runtime().gc().stats().allocated_objects,
                    setup.heap.allocated_objects
                );
                (
                    [setup, constructed, live, released, empty, reused.unwrap()],
                    post_setup_peak,
                )
            };
            // Warm shared program owners independently before each fresh Runtime
            // lifecycle; report their allocations separately from Runtime backing.
            let mut retained = None;
            for repetition in 0..3 {
                let (_, first_use) = allocations::measure(lifecycle);
                let ((phases, post_setup_peak), total) = allocations::measure(lifecycle);
                let balances = phases.each_ref().map(|phase| phase.allocations.net_bytes);
                if let Some(expected) = retained {
                    assert_eq!(balances, expected, "{name} lifecycle {repetition}");
                }
                retained = Some(balances);
                let setup = phases[0].allocations;
                let post_setup_peak = post_setup_peak.max(total.interval_peak_net_bytes);
                println!(
                    "shared_first_use,{name},count={count},repetition={repetition},allocations={first_use:?}"
                );
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
                        "memory,{name},count={count},repetition={repetition},phase={phase},allocations={:?},heap={:?}",
                        snapshot.allocations, snapshot.heap
                    );
                    println!(
                        "storage,{name},count={count},repetition={repetition},phase={phase},usage={:?}",
                        snapshot.storage
                    );
                }
                println!(
                    "memory_peak,{name},count={count},repetition={repetition},setup_peak_bytes={},post_setup_peak_bytes={post_setup_peak},post_setup_peak_delta_bytes={},lifecycle_peak_bytes={}",
                    setup.peak_net_bytes,
                    post_setup_peak - setup.net_bytes,
                    total.peak_net_bytes,
                );
                println!(
                    "teardown,{name},count={count},repetition={repetition},allocations={total:?}"
                );
                assert_eq!(total.net_bytes, 0, "runtime teardown for {name}");
            }
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
