//! Native ABI safepoints: actual entry, bounded automatic service and intervening mutation.
use super::*;
use crate::{
    gc::{GcHeapConfig, metrics::CollectionObservation},
    native::storage::scan::NORMAL_WORK,
};
use kagari_abi::native_call::JIT_STATUS_ENGINE_FAULT;
use std::cell::RefCell;

#[derive(Debug, Clone, Copy)]
struct PollRecord {
    progress: usize,
    before: CollectionObservation,
    after: CollectionObservation,
    collections: u64,
    allocations: usize,
    retained: Value,
}

#[derive(Default)]
struct NativeProbe {
    entries: usize,
    polls: [Option<PollRecord>; 2],
}

thread_local! {
    static PROBE: RefCell<NativeProbe> = RefCell::new(NativeProbe::default());
}

unsafe extern "C" fn collecting_unit(
    context: *const NativeInvocationContext,
    result: *mut JitValue,
) -> i32 {
    // SAFETY: installation supplies this live runtime and writable result slot for
    // the existing two-point Unit fixture; no pointer escapes this native entry.
    let runtime = unsafe { &*(*context).runtime.cast::<Runtime>() };
    let heap = runtime.gc();
    PROBE.with(|probe| probe.borrow_mut().entries += 1);
    let Some(root) = heap.root_value(Value::I32(0)) else {
        return JIT_STATUS_ENGINE_FAULT;
    };
    for offset in 0..2 {
        let before = heap.collection_observation().unwrap();
        // SAFETY: these are precisely the fixture's Jump/Return program points.
        let status = unsafe { jit_poll_execution(runtime, offset as u64) };
        if status != JIT_STATUS_OK {
            return status;
        }
        let Some(retained) = root.value(heap) else {
            return JIT_STATUS_ENGINE_FAULT;
        };
        let stats = heap.stats();
        PROBE.with(|probe| {
            probe.borrow_mut().polls[offset] = Some(PollRecord {
                progress: offset,
                before,
                after: heap.collection_observation().unwrap(),
                collections: stats.collections,
                allocations: stats.allocated_objects,
                retained,
            });
        });
        if offset == 0 {
            // Real mutator allocation and root publication, not another call to
            // service_collection disguised as progress between native polls.
            let Ok(value) = heap.alloc_string("native mutation between polls".into()) else {
                return JIT_STATUS_ENGINE_FAULT;
            };
            if root.set(heap, value).is_none() {
                return JIT_STATUS_ENGINE_FAULT;
            }
        }
    }
    // SAFETY: the invocation owns this output slot; Unit matches the checked body.
    unsafe { result.write(JitValue::unit()) };
    JIT_STATUS_OK
}

#[test]
fn native_body_polls_interleave_bounded_automatic_gc_and_root_mutation() {
    let mut runtime = Runtime::new(RuntimeConfig {
        gc: GcHeapConfig {
            collection_threshold: Some(NORMAL_WORK),
            ..Default::default()
        },
        ..Default::default()
    });
    runtime.gc().start_collection_observation().unwrap();
    let installed = install(&mut runtime, collecting_unit);
    let heap = runtime.gc();
    // A root extent larger than both service invocations keeps one cycle active;
    // garbage establishes pressure before entry without any explicit collection.
    let retained = heap
        .root_execution_values(vec![Value::Unit; NORMAL_WORK * 4])
        .unwrap();
    for _ in 0..NORMAL_WORK * 2 {
        heap.alloc_string("garbage".into()).unwrap();
    }
    let before = heap.collection_observation().unwrap();
    let collections = heap.stats().collections;
    PROBE.with(|probe| *probe.borrow_mut() = NativeProbe::default());

    // This API invokes installed machine code directly and has no interpreter
    // fallback. The entry-only observations additionally prove that body ran.
    assert_eq!(
        runtime.invoke_native_function(&installed).unwrap(),
        Value::Unit
    );
    let probe = PROBE.with(|probe| probe.take());
    assert_eq!(probe.entries, 1);
    let [Some(first), Some(second)] = probe.polls else {
        panic!("both actual native ABI polls must execute");
    };
    assert_eq!((first.progress, second.progress), (0, 1));
    assert_eq!(first.before.normal.count, before.normal.count);
    assert_eq!(first.after.normal.count, first.before.normal.count + 1);
    assert_eq!(second.before.normal.count, first.after.normal.count);
    assert_eq!(second.after.normal.count, second.before.normal.count + 1);
    assert_eq!(
        heap.collection_observation().unwrap().normal.count,
        second.after.normal.count
    );
    for poll in [first, second] {
        assert!((1..=NORMAL_WORK).contains(&poll.after.normal.last_work));
        assert!(poll.after.normal.max_work <= NORMAL_WORK);
        assert_eq!(
            poll.after.pressure_assist.count,
            before.pressure_assist.count
        );
        assert_eq!(
            poll.collections, collections,
            "native polls must not drain the cycle"
        );
    }
    assert_eq!(first.retained, Value::I32(0));
    assert_eq!(second.allocations, first.allocations + 1);
    let Value::Str(value) = second.retained else {
        panic!("the root mutation must survive the second native GC step");
    };
    assert_eq!(
        &*heap.string(value).unwrap(),
        "native mutation between polls"
    );
    assert_eq!(retained.get(heap, NORMAL_WORK * 4 - 1), Some(Value::Unit));
    eprintln!(
        "native entries={}, ABI polls=2, step work=[{}, {}], automatic steps=2, pressure assists=0, intervening allocations=1",
        probe.entries, first.after.normal.last_work, second.after.normal.last_work,
    );
    drop(retained);
    assert_clean(&runtime);
}
