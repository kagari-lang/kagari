use super::*;
use crate::{Runtime, test_support::layouts::allocation_owner};
use kagari_types::{scalar::BuiltinType, ty::Ty};

#[test]
fn bulk_failure_preserves_slots_and_releases_preparation_resources() {
    let mut runtime = Runtime::default();
    let owner = allocation_owner(&mut runtime);
    let heap = runtime.gc();
    let array = runtime
        .alloc_array(
            &owner,
            Ty::Builtin(BuiltinType::I32),
            vec![Value::I32(1), Value::I32(2)],
        )
        .unwrap();
    let short = runtime
        .alloc_array(&owner, Ty::Builtin(BuiltinType::I32), vec![Value::I32(0)])
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
        .alloc_array(&foreign_owner, Ty::Builtin(BuiltinType::I32), vec![])
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
        .alloc_array(&owner, Ty::Builtin(BuiltinType::I32), original.clone())
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
    heap.array_copy_within(target, Included(0), Excluded(3), 1)
        .unwrap();
    assert_eq!(
        heap.array_snapshot(target).unwrap(),
        vec![Value::I32(1), Value::I32(1), Value::I32(2), Value::I32(3)]
    );
    assert_eq!(heap.stats().current_heap_units, before);
}

#[test]
fn repeat_allocation_rejects_length_and_byte_capacity_without_publication() {
    let mut runtime = Runtime::default();
    let owner = allocation_owner(&mut runtime);
    let heap = runtime.gc();
    let before = heap.stats();
    for (element, value, count) in [
        (BuiltinType::Unit, Value::Unit, usize::MAX),
        (BuiltinType::U64, Value::U64(7), isize::MAX as usize / 8 + 1),
    ] {
        let error = runtime
            .alloc_array_repeat(&owner, Ty::Builtin(element), value, count)
            .unwrap_err();
        assert_eq!(error.kind(), RuntimeErrorKind::ResourceLimitExceeded);
        assert_eq!(heap.stats(), before);
    }
    let id = runtime
        .alloc_array_repeat(
            &owner,
            Ty::Builtin(BuiltinType::U64),
            Value::U64(1_u64 << 32),
            1,
        )
        .unwrap();
    assert_eq!(heap.array_get(id, 0), Some(Value::U64(1_u64 << 32)));
}

#[test]
fn fixed_arrays_reject_sequence_growth_capacity_and_detached_edits() {
    let mut runtime = Runtime::default();
    let owner = allocation_owner(&mut runtime);
    let id = runtime
        .alloc_array(&owner, Ty::Builtin(BuiltinType::I32), vec![Value::I32(1)])
        .unwrap();
    let heap = runtime.gc();
    let before = heap.stats();
    assert!(heap.sequence_push(id, Value::I32(2)).is_err());
    assert!(heap.sequence_pop(id).is_err());
    assert!(heap.sequence_insert(id, 0, Value::I32(2)).is_err());
    assert!(heap.sequence_remove(id, 0).is_err());
    assert!(heap.sequence_clear(id).is_err());
    assert!(heap.sequence_truncate(id, 0).is_err());
    assert!(heap.reserve_collection(&Value::Array(id), 1).is_err());
    assert!(heap.reserve_collection(&Value::GcHandle(id), 1).is_err());
    assert!(heap.with_sequence_mut::<i32, _>(id, |_| Ok(())).is_err());
    assert!(heap.edit_sequence(id, |_| Ok(())).is_err());
    assert_eq!(heap.array_snapshot(id), Some(vec![Value::I32(1)]));
    assert_eq!(heap.stats(), before);
    heap.array_set(id, 0, Value::I32(42)).unwrap();
    assert_eq!(heap.array_snapshot(id), Some(vec![Value::I32(42)]));
}
