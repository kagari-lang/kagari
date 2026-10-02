use super::*;
use crate::{Runtime, layout_fixtures::allocation_owner};
use kagari_abi::{scalar::BuiltinType, types::AbiType};

#[test]
fn bulk_failure_preserves_slots_and_releases_preparation_resources() {
    let mut runtime = Runtime::default();
    let owner = allocation_owner(&mut runtime);
    let heap = runtime.gc();
    let array = runtime
        .alloc_array(
            &owner,
            AbiType::Builtin(BuiltinType::I32),
            vec![Value::I32(1), Value::I32(2)],
        )
        .unwrap();
    let short = runtime
        .alloc_array(
            &owner,
            AbiType::Builtin(BuiltinType::I32),
            vec![Value::I32(0)],
        )
        .unwrap();
    let before = heap.stats().current_heap_units;
    assert!(heap.array_copy_from(array, short).is_err());
    assert_eq!(
        heap.array_snapshot(array).unwrap(),
        vec![Value::I32(1), Value::I32(2)]
    );
    let mut foreign = Runtime::default();
    let foreign_owner = allocation_owner(&mut foreign);
    let foreign_array = foreign
        .alloc_array(&foreign_owner, AbiType::Builtin(BuiltinType::I32), vec![])
        .unwrap();
    assert!(heap.array_fill(array, Value::Array(foreign_array)).is_err());
    assert!(heap.array_copy_from(array, foreign_array).is_err());
    assert_eq!(heap.stats().current_heap_units, before);
    heap.array_fill(array, Value::I32(7)).unwrap();
    assert_eq!(heap.stats().current_heap_units, before);
}
#[test]
fn copy_within_validates_before_commit_and_accounts_temporary_storage() {
    use std::ops::Bound::{Excluded, Included, Unbounded};
    let mut runtime = Runtime::default();
    let owner = allocation_owner(&mut runtime);
    let heap = runtime.gc();
    let original = vec![Value::I32(1), Value::I32(2), Value::I32(3), Value::I32(4)];
    let target = runtime
        .alloc_array(&owner, AbiType::Builtin(BuiltinType::I32), original.clone())
        .unwrap();
    let before = heap.stats().current_heap_units;
    for (start, end, destination) in [
        (Included(3), Excluded(1), 0),
        (Included(0), Excluded(5), 0),
        (Unbounded, Unbounded, 1),
        (Included(4), Excluded(4), 5),
        (Excluded(usize::MAX), Unbounded, 0),
        (Unbounded, Included(usize::MAX), 0),
    ] {
        assert!(
            heap.array_copy_within(target, start, end, destination)
                .is_err()
        );
        assert_eq!(heap.array_snapshot(target).unwrap(), original);
        assert_eq!(heap.stats().current_heap_units, before);
    }
    let guard = heap
        .begin_collection_iteration(&Value::Array(target))
        .unwrap();
    heap.array_copy_within(target, Included(0), Excluded(3), 1)
        .unwrap();
    assert_eq!(
        heap.array_snapshot(target).unwrap(),
        vec![Value::I32(1), Value::I32(1), Value::I32(2), Value::I32(3)]
    );
    assert_eq!(heap.stats().current_heap_units, before);
    drop(guard);
}
