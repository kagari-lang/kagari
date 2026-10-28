//! A finite host pressure burst: real retained roots keep the first cycle active.
use crate::automatic::report::{Record, Scope};
use kagari::{context::ExecutionContext, engine::KagariEngine};
use kagari_runtime::value::Value;

pub(super) const NAME: &str = "host_pressure_burst_2048x2048";
const ROOTS: usize = 2048;
const BURST: usize = 2048;
// The selected normal allowance is a contract assertion, not a private driver entry.
const NORMAL_WORK: usize = 1024;
const RETAINED: &str = "retained across pressure assist";

pub(super) fn lifecycle(engine: &KagariEngine) -> Record {
    let mut runtime = engine.runtime(ExecutionContext::default());
    let baseline = runtime.runtime().gc().stats();
    assert_eq!(
        baseline.collections, 0,
        "this control starts the first cycle"
    );
    let threshold = runtime
        .runtime()
        .gc()
        .config()
        .collection_threshold
        .unwrap()
        .max(1);
    assert!(threshold < BURST);
    let mut record = Record::new();
    // These are intentional independent host roots, not roots for library-internal edges.
    let mut roots = Vec::with_capacity(ROOTS);
    record.measure("admit_roots", Scope::Automatic, &mut runtime, |runtime| {
        for _ in 0..ROOTS {
            let value = runtime
                .runtime()
                .gc()
                .alloc_string(RETAINED.into())
                .unwrap();
            roots.push(runtime.runtime().root_value(value).unwrap());
        }
    });
    let admitted = record.last_observation();
    assert_eq!(admitted.normal.count + admitted.pressure_assist.count, 0);
    assert_eq!(runtime.runtime().gc().stats().collections, 0);

    record.measure("start_cycle", Scope::Automatic, &mut runtime, |runtime| {
        runtime.runtime().gc_safepoint().unwrap();
    });
    let first = record.last_observation();
    assert_eq!(first.normal.count, 1);
    assert_eq!(first.pressure_assist.count, 0);
    assert_eq!(first.slices.count, 1);
    assert!(first.normal.last_work > 0 && first.normal.last_work <= NORMAL_WORK);
    assert_eq!(first.slices.total_work, first.normal.total_work);
    assert_eq!(first.cycles.count + first.carried_cycles.count, 0);

    // Allocation has no safepoint. Keep the captured root scan active while committing
    // more positive logical pressure than its unchanged first-cycle trigger threshold.
    let before_burst = runtime.runtime().gc().stats();
    record.measure(
        "pressure_burst",
        Scope::Automatic,
        &mut runtime,
        |runtime| {
            for _ in 0..BURST {
                runtime
                    .runtime()
                    .gc()
                    .alloc_string("unrooted burst".into())
                    .unwrap();
            }
        },
    );
    let burst = record.last_observation();
    let after_burst = runtime.runtime().gc().stats();
    assert!(
        burst.active_cycle_at_start,
        "start observation in the pending cycle"
    );
    assert_eq!(
        burst.normal.count + burst.pressure_assist.count + burst.explicit.count,
        0
    );
    assert_eq!(before_burst.collections, after_burst.collections);
    assert_eq!(
        after_burst.allocated_objects - before_burst.allocated_objects,
        BURST
    );
    assert!(after_burst.current_heap_units - before_burst.current_heap_units > threshold);

    record.measure("assist", Scope::Automatic, &mut runtime, |runtime| {
        runtime.runtime().gc_safepoint().unwrap();
    });
    let assisted = record.last_observation();
    assert!(assisted.active_cycle_at_start);
    assert_eq!(assisted.normal.count, 0);
    assert_eq!(assisted.explicit.count, 0);
    assert_eq!(assisted.pressure_assist.count, 1);
    assert_eq!(assisted.cycles.count, 0);
    assert_eq!(assisted.carried_cycles.count, 1);
    assert!(assisted.pressure_assist.last_work > NORMAL_WORK);
    assert_eq!(
        assisted.carried_cycles.total_work,
        first.normal.total_work + assisted.pressure_assist.total_work,
        "carried work includes the previous window exactly once"
    );
    assert_eq!(
        runtime.runtime().gc().stats().collections,
        before_burst.collections + 1
    );

    record.measure("verify_roots", Scope::Automatic, &mut runtime, |runtime| {
        for root in &roots {
            let Value::Str(id) = root.value(runtime.runtime().gc()).unwrap() else {
                panic!("retained host string changed representation");
            };
            assert_eq!(&*runtime.runtime().gc().string(id).unwrap(), RETAINED);
        }
    });
    record.measure(
        "release_burst_gc",
        Scope::Explicit,
        &mut runtime,
        |runtime| {
            assert_eq!(
                runtime.runtime().collect_garbage().unwrap().live_objects,
                baseline.allocated_objects + ROOTS,
            );
        },
    );
    record.measure("release_roots", Scope::Automatic, &mut runtime, |_| {
        drop(roots)
    });
    record.measure("release_gc", Scope::Explicit, &mut runtime, |runtime| {
        assert_eq!(
            runtime.runtime().collect_garbage().unwrap().live_objects,
            baseline.allocated_objects
        );
    });
    record.finish(&runtime);
    record
}
