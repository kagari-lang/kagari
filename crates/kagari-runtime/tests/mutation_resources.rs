use kagari_ir::builtin::surface::StandardIntrinsic;
use kagari_runtime::{ResourcePolicy, Runtime, RuntimeConfig, RuntimeErrorKind, value::Value};

fn limited(heap: Option<usize>, allocation: Option<usize>) -> Runtime {
    Runtime::new(RuntimeConfig {
        resources: ResourcePolicy {
            max_heap_units: heap,
            max_allocation_units: allocation,
            ..Default::default()
        },
        ..Default::default()
    })
}

#[test]
fn rejected_allocations_leave_all_counters_and_free_slots_unchanged() {
    for (heap, allocation, reason) in [
        (Some(2), None, "heap units"),
        (None, Some(2), "allocation units"),
    ] {
        let runtime = limited(heap, allocation);
        let first = runtime.alloc_array(vec![Value::I32(1)]).unwrap();
        let before = runtime.resources().counters();
        let heap_before = runtime.gc().stats();
        let error = runtime.alloc_array(vec![]).unwrap_err();
        assert_eq!(error.kind(), RuntimeErrorKind::ResourceLimitExceeded);
        assert!(error.message().contains(reason));
        assert_eq!(runtime.resources().counters(), before);
        assert_eq!(runtime.gc().stats(), heap_before);
        assert_eq!(runtime.gc().array_get(first, 0), Some(Value::I32(1)));
    }
    let runtime = limited(Some(2), None);
    let old = runtime.alloc_array(vec![]).unwrap();
    runtime.collect_garbage().unwrap();
    let before = runtime.gc().stats();
    assert!(
        runtime
            .alloc_array(vec![Value::I32(1), Value::I32(2)])
            .is_err()
    );
    assert_eq!(runtime.gc().stats(), before);
    let next = runtime.alloc_array(vec![]).unwrap();
    assert_eq!(next.index(), old.index());
    assert_ne!(next, old);
}

#[test]
fn standard_growth_obeys_shared_limits_without_charging_failed_writes() {
    for (heap, allocation) in [(Some(6), None), (None, Some(6))] {
        let runtime = limited(heap, allocation);
        let array = runtime.alloc_array(vec![Value::I32(1)]).unwrap();
        let map = runtime
            .alloc_map(vec![(Value::I32(1), Value::I32(2))])
            .unwrap();
        let set = runtime.alloc_set(vec![Value::I32(1)]).unwrap();
        let operations = [
            (
                StandardIntrinsic::ArrayPush,
                vec![Value::Array(array), Value::I32(3)],
            ),
            (
                StandardIntrinsic::ArrayInsert,
                vec![Value::Array(array), Value::I32(0), Value::I32(3)],
            ),
            (
                StandardIntrinsic::MapInsert,
                vec![Value::Map(map), Value::I32(2), Value::I32(3)],
            ),
            (
                StandardIntrinsic::SetInsert,
                vec![Value::Set(set), Value::I32(2)],
            ),
        ];
        let before = runtime.gc().stats();
        for (intrinsic, args) in operations {
            let error = runtime
                .invoke_standard_builtin(intrinsic, &args)
                .unwrap_err();
            assert_eq!(error.kind(), RuntimeErrorKind::ResourceLimitExceeded);
            assert_eq!(runtime.gc().stats(), before);
            assert_eq!(runtime.resources().counters().allocation_units, 6);
        }
        assert_eq!(
            runtime.gc().array_snapshot(array),
            Some(vec![Value::I32(1)])
        );
        assert_eq!(
            runtime.gc().map_snapshot(map),
            Some(vec![(Value::I32(1), Value::I32(2))])
        );
        assert_eq!(runtime.gc().set_snapshot(set), Some(vec![Value::I32(1)]));
        // Replacing a map value or inserting a duplicate set key does not grow storage.
        runtime
            .gc()
            .map_insert(map, Value::I32(1), Value::I32(9))
            .unwrap();
        assert!(!runtime.gc().set_insert(set, Value::I32(1)).unwrap());
        assert_eq!(runtime.gc().stats(), before);
        assert_eq!(
            runtime.gc().map_get(map, &Value::I32(1)),
            Some(Value::I32(9))
        );
    }
}

#[test]
fn option_allocation_failure_does_not_remove_an_array_or_map_entry() {
    for (heap, allocation) in [(Some(4), None), (None, Some(4))] {
        let runtime = limited(heap, allocation);
        let array = runtime.alloc_array(vec![Value::I32(42)]).unwrap();
        let map = runtime
            .alloc_map(vec![(Value::I32(1), Value::I32(42))])
            .unwrap();
        for (intrinsic, args) in [
            (StandardIntrinsic::ArrayPop, vec![Value::Array(array)]),
            (
                StandardIntrinsic::ArrayRemove,
                vec![Value::Array(array), Value::I32(0)],
            ),
            (
                StandardIntrinsic::MapRemove,
                vec![Value::Map(map), Value::I32(1)],
            ),
        ] {
            let before = runtime.gc().stats();
            let error = runtime
                .invoke_standard_builtin(intrinsic, &args)
                .unwrap_err();
            assert_eq!(error.kind(), RuntimeErrorKind::ResourceLimitExceeded);
            assert_eq!(runtime.gc().stats(), before);
            assert_eq!(
                runtime.gc().array_snapshot(array),
                Some(vec![Value::I32(42)])
            );
            assert_eq!(
                runtime.gc().map_snapshot(map),
                Some(vec![(Value::I32(1), Value::I32(42))])
            );
        }
    }
}

#[test]
fn successful_removal_accounts_prepared_result_and_never_refunds_allocation_budget() {
    let runtime = limited(None, Some(4));
    let array = runtime.alloc_array(vec![Value::I32(42)]).unwrap();
    let result = runtime
        .invoke_standard_builtin(StandardIntrinsic::ArrayPop, &[Value::Array(array)])
        .unwrap();
    let Value::Enum(result) = result else {
        panic!("Option result")
    };
    assert_eq!(
        runtime.gc().enum_snapshot(result).unwrap().fields,
        vec![Value::I32(42)]
    );
    assert_eq!(runtime.gc().array_len(array), Some(0));
    let counters = runtime.resources().counters();
    assert_eq!(counters.current_heap_units, 3);
    assert_eq!(counters.peak_heap_units, 4);
    assert_eq!(counters.allocation_units, 4);
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.resources().counters().current_heap_units, 0);
    assert_eq!(runtime.resources().counters().allocation_units, 4);
    assert!(
        runtime
            .alloc_array(vec![])
            .unwrap_err()
            .message()
            .contains("allocation units")
    );
}

#[test]
fn duplicate_input_keys_only_charge_final_container_size() {
    let runtime = limited(Some(4), Some(4));
    let map = runtime
        .alloc_map(vec![
            (Value::I32(1), Value::I32(2)),
            (Value::I32(1), Value::I32(3)),
        ])
        .unwrap();
    runtime
        .alloc_set(vec![Value::I32(1), Value::I32(1)])
        .unwrap();
    assert_eq!(runtime.resources().counters().allocation_units, 4);
    assert_eq!(
        runtime.gc().map_get(map, &Value::I32(1)),
        Some(Value::I32(3))
    );
}
