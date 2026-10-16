use super::*;
use crate::{
    Runtime,
    closure::ClosureTarget,
    host::{HostBorrowTable, HostObjectId, HostRootHandle, HostSchemaEpoch},
    metadata::{AbiFingerprint, TypeId},
    test_support::layouts::{allocation_owner, path_view_value},
};
use kagari_contract::ids::FunctionRef;

use kagari_types::{scalar::BuiltinType, ty::Ty};

#[test]
fn foreign_executable_edges_abort_collection_before_any_storage_is_detached() {
    let mut runtime = Runtime::default();
    let owner = allocation_owner(&mut runtime);
    let dead = runtime
        .alloc_array(&owner, Ty::Builtin(BuiltinType::I32), vec![])
        .unwrap();
    let mut foreign = Runtime::default();
    let foreign_owner = allocation_owner(&mut foreign);
    assert_eq!(owner.key(), foreign_owner.key());
    // Deliberately bypass Runtime::make_closure to exercise corrupted storage.
    let closure = runtime
        .gc()
        .alloc_closure(ClosureValueSnapshot {
            implementation: foreign_owner,
            target: ClosureTarget::Script(FunctionRef::new(0)),
            captures: vec![],
            environment: None,
        })
        .unwrap();
    let _root = runtime.root_value(Value::Closure(closure)).unwrap();
    let before = runtime.resources().counters();
    let stats = runtime.gc().stats();
    assert_eq!(
        runtime.collect_garbage().unwrap_err().kind(),
        RuntimeErrorKind::EngineFault
    );
    assert_eq!(runtime.resources().counters(), before);
    assert_eq!(runtime.gc().stats(), stats);
    assert_eq!(runtime.gc().object_kind(dead), Some(GcObjectKind::Array));
    assert_eq!(
        runtime.gc().object_kind(closure),
        Some(GcObjectKind::Closure)
    );
}

#[test]
fn interface_roots_trace_data_and_retain_old_dependency_versions() {
    let mut runtime = crate::Runtime::default();
    let owner = allocation_owner(&mut runtime);
    let array = runtime
        .alloc_array(&owner, Ty::Builtin(BuiltinType::I32), vec![Value::I32(7)])
        .unwrap();
    let interface = crate::test_support::layouts::interface_value_with(
        &mut runtime,
        Ty::Array(Box::new(Ty::Builtin(
            kagari_types::scalar::BuiltinType::I32,
        ))),
        Value::Array(array),
    );
    let Value::Interface(id) = interface else {
        panic!("verified interface allocation");
    };
    let old = runtime.modules().latest("interface-fixture").unwrap().key();
    let loaded = runtime.modules().latest("interface-fixture").unwrap();
    assert!(
        runtime
            .make_interface(&loaded, 0, Value::Bool(true))
            .is_err()
    );
    assert!(
        crate::Runtime::default()
            .make_interface(&loaded, 0, Value::I32(7))
            .is_err()
    );
    assert_eq!(runtime.modules().retention_counts(old).runtime_values, 0);
    let root = runtime.root_value(interface).unwrap();
    assert!(runtime.gc().interface_snapshot(id).is_some());
    assert!(!crate::Runtime::default().gc().validate_value(&interface));

    let _new = crate::test_support::layouts::interface_value(&mut runtime);
    assert_ne!(
        runtime.modules().latest("interface-fixture").unwrap().key(),
        old
    );
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc().object_kind(array), Some(GcObjectKind::Array));
    assert_eq!(
        runtime.gc().object_kind(id.0),
        Some(GcObjectKind::Interface)
    );
    assert!(
        !runtime
            .collect_garbage()
            .unwrap()
            .reclaimed_modules
            .contains(&old)
    );

    drop(root);
    let collected = runtime.collect_garbage().unwrap();
    assert!(!runtime.gc().validate_value(&interface));
    assert_eq!(runtime.modules().retention_counts(old).runtime_values, 0);
    assert!(collected.reclaimed_modules.contains(&old));
    assert!(runtime.modules().loaded(old).is_none());
}

fn layout(name: &str, field: &str, ty: Ty) -> crate::module::StructLayoutRef {
    crate::test_support::layouts::layout(&mut crate::Runtime::default(), name, &[(field, ty, true)])
}

fn host_root_value(heap: &GcHeap, object_id: u64) -> Value {
    heap.alloc_host_root(HostRootHandle::new(
        Default::default(),
        HostObjectId(object_id),
        TypeId::new(0),
        HostSchemaEpoch::new(0),
        AbiFingerprint(1),
    ))
    .unwrap()
}

fn shared_borrow_value(heap: &GcHeap, object_id: u64) -> Value {
    let table = HostBorrowTable::default();
    let guard = table.enter_frame().unwrap();
    heap.alloc_host_ref(
        guard
            .borrow_shared(HostObjectId(object_id), TypeId::new(0))
            .unwrap(),
    )
    .unwrap()
}

fn unique_borrow_value(heap: &GcHeap, object_id: u64) -> Value {
    let table = HostBorrowTable::default();
    let guard = table.enter_frame().unwrap();
    heap.alloc_host_mut(
        guard
            .borrow_unique(HostObjectId(object_id), TypeId::new(0))
            .unwrap(),
    )
    .unwrap()
}

#[test]
fn rejects_ephemeral_values_as_heap_payloads() {
    let mut runtime = Runtime::default();
    let owner = allocation_owner(&mut runtime);
    let heap = runtime.gc();

    let rejected = [shared_borrow_value(heap, 1), unique_borrow_value(heap, 2)];
    let before = heap.stats();
    assert!(
        runtime
            .alloc_array(&owner, Ty::Builtin(BuiltinType::I32), vec![rejected[0]])
            .is_err()
    );
    assert!(
        runtime
            .alloc_array(&owner, Ty::Builtin(BuiltinType::I32), vec![rejected[1]])
            .is_err()
    );
    assert_eq!(heap.stats(), before);
}

#[test]
fn rejects_host_handles_and_path_views_as_default_heap_payloads() {
    let mut runtime = Runtime::default();
    let owner = allocation_owner(&mut runtime);
    let heap = runtime.gc();

    let rejected = [host_root_value(heap, 1), path_view_value(heap, 3)];
    let before = heap.stats();
    assert!(
        runtime
            .alloc_array(&owner, Ty::Builtin(BuiltinType::I32), vec![rejected[0]])
            .is_err()
    );
    assert!(
        heap.alloc_struct(
            layout(
                "HostBacked",
                "path",
                Ty::Array(Box::new(Ty::Builtin(
                    kagari_types::scalar::BuiltinType::I32
                )))
            ),
            vec![rejected[1]],
        )
        .is_err()
    );
    assert_eq!(heap.stats(), before);
}

#[test]
fn rejects_non_storable_heap_mutations() {
    let mut runtime = Runtime::default();
    let owner = allocation_owner(&mut runtime);
    let heap = runtime.gc();
    let array = runtime
        .alloc_array(&owner, Ty::Builtin(BuiltinType::I32), vec![Value::I32(1)])
        .unwrap();
    let record = heap
        .alloc_struct(
            layout(
                "Record",
                "value",
                Ty::Builtin(kagari_types::scalar::BuiltinType::I32),
            ),
            vec![Value::I32(1)],
        )
        .unwrap();

    assert!(
        heap.array_set(array, 0, shared_borrow_value(heap, 1))
            .is_err()
    );
    assert!(heap.array_set(array, 0, path_view_value(heap, 4)).is_err());
    assert!(
        heap.struct_set_slot(
            record,
            &heap.struct_layout(record).unwrap(),
            0,
            host_root_value(heap, 5)
        )
        .is_err()
    );

    assert_eq!(heap.array_snapshot(array), Some(vec![Value::I32(1)]));
    assert_eq!(
        heap.struct_get_slot(record, &heap.struct_layout(record).unwrap(), 0),
        Some(Value::I32(1))
    );
}

#[test]
fn exhausted_object_generation_retires_storage_without_aliasing_old_values() {
    let runtime = Runtime::default();
    let heap = runtime.gc();
    let first = heap.alloc_cell(ValueType::I32, Value::I32(7)).unwrap();
    // Advance the existing allocation to its final reusable generation without
    // billions of collections. No stale handle is admitted as a root.
    heap.objects.borrow_mut().slots[first.index()].generation = u32::MAX - 1;
    let penultimate = HeapObjectId::new(heap.owner, first.index(), u32::MAX - 1);
    let root = heap.root_value(Value::Cell(penultimate)).unwrap();
    assert!(!heap.validate_value(&Value::Cell(first)));
    assert_eq!(runtime.collect_garbage().unwrap().live_objects, 1);
    drop(root);
    assert_eq!(runtime.collect_garbage().unwrap().reclaimed_objects, 1);

    let last = heap.alloc_cell(ValueType::I32, Value::I32(8)).unwrap();
    assert_eq!(last.index(), first.index());
    assert_eq!(last.generation(), u32::MAX);
    assert!(!heap.validate_value(&Value::Cell(penultimate)));
    let root = heap.root_value(Value::Cell(last)).unwrap();
    assert_eq!(runtime.collect_garbage().unwrap().live_objects, 1);
    drop(root);
    assert_eq!(runtime.collect_garbage().unwrap().reclaimed_objects, 1);

    let next = heap.alloc_cell(ValueType::I32, Value::I32(9)).unwrap();
    assert_ne!(next.index(), last.index());
    assert!(!heap.validate_value(&Value::Cell(first)));
    assert!(!heap.validate_value(&Value::Cell(last)));
    assert!(heap.root_value(Value::Cell(last)).is_none());
    assert_eq!(heap.cell_get(next, ValueType::I32).unwrap(), Value::I32(9));
    assert_eq!(heap.stats().allocated_objects, 1);
}

#[test]
fn sparse_collection_preserves_aliases_and_mutation_across_payload_relocation() {
    let runtime = Runtime::default();
    let heap = runtime.gc();
    let mut retained = Vec::new();
    for index in 0..256_i32 {
        let text = heap.alloc_string(index.to_string()).unwrap();
        let cell = heap.alloc_cell(ValueType::I32, Value::I32(index)).unwrap();
        let tuple = heap.alloc_tuple(vec![text, Value::Cell(cell)]).unwrap();
        if index % 31 == 0 || index == 255 {
            retained.push((heap.root_value(tuple).unwrap(), text, cell, index));
        }
    }
    assert_eq!(
        runtime.collect_garbage().unwrap().live_objects,
        retained.len() * 3
    );
    for (_, text, cell, expected) in &retained {
        let Value::Str(id) = text else { unreachable!() };
        assert_eq!(&*heap.string(*id).unwrap(), expected.to_string());
        assert_eq!(
            heap.cell_get(*cell, ValueType::I32).unwrap(),
            Value::I32(*expected)
        );
        heap.cell_set(*cell, ValueType::I32, Value::I32(*expected + 1))
            .unwrap();
    }
    // Reuse directory slots and grow the now sparse stores again.
    for index in 0..512_i32 {
        heap.alloc_string(index.to_string()).unwrap();
        heap.alloc_cell(ValueType::I32, Value::I32(index)).unwrap();
    }
    runtime.collect_garbage().unwrap();
    for (root, text, cell, expected) in &retained {
        let Value::Tuple(tuple) = root.value(heap).unwrap() else {
            unreachable!()
        };
        assert_eq!(&*heap.tuple(tuple).unwrap(), &[*text, Value::Cell(*cell)]);
        assert_eq!(
            heap.cell_get(*cell, ValueType::I32).unwrap(),
            Value::I32(*expected + 1)
        );
    }
    let stale = retained[0].2;
    drop(retained);
    assert_eq!(runtime.collect_garbage().unwrap().live_objects, 0);
    heap.alloc_cell(ValueType::I32, Value::I32(1)).unwrap();
    assert!(!heap.validate_value(&Value::Cell(stale)));
}

#[test]
fn roots_are_explicit_storable_slots() {
    let mut runtime = Runtime::default();
    let owner = allocation_owner(&mut runtime);
    let heap = runtime.gc();
    let object = runtime
        .alloc_array(&owner, Ty::Builtin(BuiltinType::I32), vec![Value::I32(1)])
        .unwrap();
    let root = heap.root_value(Value::Array(object)).unwrap();

    assert_eq!(root.value(heap).unwrap(), Value::Array(object));
    assert_eq!(heap.active_roots(), 1);
    assert_eq!(heap.trace_roots().unwrap(), vec![object]);

    assert!(heap.root_value(host_root_value(heap, 1)).is_none());
    assert!(heap.root_value(path_view_value(heap, 1)).is_none());
    assert!(
        heap.root_value(
            heap.alloc_tuple(vec![shared_borrow_value(heap, 2)])
                .unwrap()
        )
        .is_none()
    );
    assert_eq!(heap.active_roots(), 1);

    let bare_copy = root.value(heap).unwrap();
    let retained = root.clone();
    drop(root);
    assert_eq!(runtime.collect_garbage().unwrap().live_objects, 1);
    drop(retained);
    assert_eq!(runtime.collect_garbage().unwrap().reclaimed_objects, 1);
    assert!(!heap.validate_value(&bare_copy));
}

#[test]
fn replacement_errors_preserve_targets_and_internal_fault_categories() {
    let mut runtime = Runtime::default();
    let owner = allocation_owner(&mut runtime);
    let heap = runtime.gc();
    let resources = runtime.resources();
    let array = runtime
        .alloc_array(&owner, Ty::Builtin(BuiltinType::I32), vec![Value::I32(1)])
        .unwrap();
    let before = heap.stats().current_heap_units;
    let mut foreign_runtime = Runtime::default();
    let foreign_owner = allocation_owner(&mut foreign_runtime);
    let foreign = foreign_runtime
        .alloc_array(&foreign_owner, Ty::Builtin(BuiltinType::I32), vec![])
        .unwrap();
    // Payload and receiver rejection must not be relabeled from the index.
    assert_eq!(
        heap.array_set(array, usize::MAX, Value::Array(foreign))
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ScriptTrap
    );
    assert_eq!(
        heap.array_set(foreign, usize::MAX, Value::I32(9))
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ScriptTrap
    );

    assert_eq!(
        heap.array_set(array, 2, Value::I32(9)).unwrap_err().kind(),
        RuntimeErrorKind::IndexOutOfBounds
    );
    assert_eq!(heap.array_get(array, 0), Some(Value::I32(1)));
    assert_eq!(heap.stats().current_heap_units, before);
    let schema = layout(
        "Record",
        "value",
        Ty::Builtin(kagari_types::scalar::BuiltinType::I32),
    );
    let object = heap
        .alloc_struct(schema.clone(), vec![Value::I32(7)])
        .unwrap();
    assert_eq!(
        heap.struct_set_slot(object, &schema, 0, Value::Bool(false))
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ScriptTrap
    );
    assert_eq!(
        heap.struct_get_slot(object, &schema, 0),
        Some(Value::I32(7))
    );
    // Simulate a broken engine invariant rather than a script type error.
    heap.with_struct_mut(object, |_, fields| fields.clear())
        .unwrap();
    let error = crate::reflection::set_field(heap, &Value::Struct(object), "value", Value::I32(9))
        .unwrap_err();
    assert_eq!(error.kind(), RuntimeErrorKind::EngineFault);
    assert_eq!(
        error.into_write_error().kind(),
        RuntimeErrorKind::EngineFault
    );
    assert!(resources.is_quarantined());
    assert_eq!(
        heap.array_set(array, 0, Value::I32(9)).unwrap_err().kind(),
        RuntimeErrorKind::EngineFault
    );
    assert_eq!(heap.array_get(array, 0), Some(Value::I32(1)));
    let error =
        crate::reflection::set_index(heap, &Value::Array(array), &Value::I32(0), Value::I32(9))
            .unwrap_err();
    assert_eq!(
        error.into_write_error().kind(),
        RuntimeErrorKind::EngineFault
    );
}

mod gc_ownership;
