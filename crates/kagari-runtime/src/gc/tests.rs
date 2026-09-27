use kagari_abi::types::AbiType;

#[test]
fn interface_roots_trace_data_and_retain_old_dependency_versions() {
    let mut runtime = crate::Runtime::default();
    let array = runtime.alloc_array(vec![Value::I32(7)]).unwrap();
    let interface = crate::layout_fixtures::interface_value_with(
        &mut runtime,
        AbiType::Array(
            Box::new(AbiType::Builtin(kagari_abi::scalar::BuiltinType::I32)),
            CollectionAccess::Mutable,
        ),
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
    assert_eq!(runtime.modules().retention_counts(old).runtime_values, 1);
    let root = runtime.root_value(interface.clone()).unwrap();
    assert!(runtime.gc().interface_snapshot(id).is_some());
    assert!(!crate::Runtime::default().gc().validate_value(&interface));

    let _new = crate::layout_fixtures::interface_value(&mut runtime);
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
            .modules()
            .collect_unreachable_epochs()
            .contains(&old)
    );

    drop(root);
    runtime.collect_garbage().unwrap();
    assert!(!runtime.gc().validate_value(&interface));
    assert_eq!(runtime.modules().retention_counts(old).runtime_values, 0);
    assert!(
        runtime
            .modules()
            .collect_unreachable_epochs()
            .contains(&old)
    );
}
fn layout(name: &str, field: &str, ty: AbiType) -> crate::module::StructLayoutRef {
    crate::layout_fixtures::layout(&mut crate::Runtime::default(), name, &[(field, ty, true)])
}
use super::*;
use {
    crate::host::DynamicPathArguments, crate::host::HostBorrowTable, crate::host::HostObjectId,
    crate::host::HostPathDescriptorRegistration, crate::host::HostPathSegmentRegistration,
    crate::host::HostRootHandle, crate::host::HostSchemaEpoch, crate::metadata::AbiFingerprint,
    crate::metadata::TypeId, kagari_common::host_interface::HostTypeOwnership,
    kagari_common::host_interface::PathAccess,
};

fn host_root_value(object_id: u64) -> Value {
    Value::HostRoot(HostRootHandle::new(
        Default::default(),
        HostObjectId(object_id),
        TypeId::new(0),
        HostSchemaEpoch::new(0),
        AbiFingerprint(1),
    ))
}

fn path_view_value(object_id: u64) -> Value {
    let result_type = TypeId::new(1);
    let mut runtime = crate::Runtime::default();
    let mut declaration = kagari_common::host_interface::HostTypeDeclaration::new("Player");
    declaration.ownership = HostTypeOwnership::HostRoot;
    declaration.path_access = PathAccess::ReadWrite;
    let mut hp = kagari_common::host_interface::HostFieldDeclaration::new(
        &declaration.id,
        "hp",
        kagari_common::host_interface::HostValueType::I32,
    );
    hp.writable = true;
    hp.path_access = PathAccess::ReadWrite;
    declaration.fields.push(hp);
    let root_type = runtime
        .register_host_type(crate::HostTypeRegistration::new(declaration, "Player"))
        .unwrap();
    let root = runtime
        .register_host_root(HostObjectId(object_id), root_type, HostSchemaEpoch::new(0))
        .unwrap();
    let descriptor = runtime
        .register_host_path_descriptor(HostPathDescriptorRegistration {
            root_type,
            result_type,
            segments: vec![HostPathSegmentRegistration::Field {
                declaration: runtime
                    .host()
                    .host_type(root_type)
                    .unwrap()
                    .declaration
                    .fields
                    .iter()
                    .find(|field| field.name == "hp")
                    .unwrap()
                    .id
                    .clone(),
            }],
            access: PathAccess::ReadWrite,
            schema_epoch: HostSchemaEpoch::new(0),
            capability_requirements: crate::CapabilitySet::default(),
        })
        .unwrap();
    Value::HostPathView(
        runtime
            .host()
            .make_path_view(root, descriptor, DynamicPathArguments::empty())
            .unwrap(),
    )
}

fn shared_borrow_value(object_id: u64) -> Value {
    let table = HostBorrowTable::default();
    let guard = table.enter_frame().unwrap();
    Value::host_ref(
        guard
            .borrow_shared(HostObjectId(object_id), TypeId::new(0))
            .unwrap(),
    )
}

fn unique_borrow_value(object_id: u64) -> Value {
    let table = HostBorrowTable::default();
    let guard = table.enter_frame().unwrap();
    Value::host_mut(
        guard
            .borrow_unique(HostObjectId(object_id), TypeId::new(0))
            .unwrap(),
    )
}

#[test]
fn rejects_ephemeral_values_as_heap_payloads() {
    let heap = GcHeap::new(
        GcHeapConfig::default(),
        std::rc::Rc::new(crate::resource::ResourceState::default()),
    );

    assert!(heap.alloc_array(vec![shared_borrow_value(1)]).is_err());
    assert!(heap.alloc_array(vec![unique_borrow_value(2)]).is_err());
    assert_eq!(heap.allocated_objects(), 0);
}

#[test]
fn rejects_host_handles_and_path_views_as_default_heap_payloads() {
    let heap = GcHeap::new(
        GcHeapConfig::default(),
        std::rc::Rc::new(crate::resource::ResourceState::default()),
    );

    assert!(heap.alloc_array(vec![host_root_value(1)]).is_err());
    assert!(
        heap.alloc_map(vec![(Value::Str("host".to_owned()), host_root_value(2))])
            .is_err()
    );
    assert!(
        heap.alloc_map(vec![(path_view_value(3), Value::I32(1))])
            .is_err()
    );
    assert!(heap.alloc_set(vec![host_root_value(4)]).is_err());
    assert!(
        heap.alloc_struct(
            layout(
                "HostBacked",
                "path",
                AbiType::Array(
                    Box::new(AbiType::Builtin(kagari_abi::scalar::BuiltinType::I32)),
                    CollectionAccess::Mutable
                )
            ),
            vec![path_view_value(3)],
        )
        .is_err()
    );
    assert_eq!(heap.allocated_objects(), 0);
}

#[test]
fn rejects_non_storable_heap_mutations() {
    let heap = GcHeap::new(
        GcHeapConfig::default(),
        std::rc::Rc::new(crate::resource::ResourceState::default()),
    );
    let array = heap.alloc_array(vec![Value::I32(1)]).unwrap();
    let record = heap
        .alloc_struct(
            layout(
                "Record",
                "value",
                AbiType::Builtin(kagari_abi::scalar::BuiltinType::I32),
            ),
            vec![Value::I32(1)],
        )
        .unwrap();

    assert!(heap.array_push(array, shared_borrow_value(1)).is_err());
    assert!(heap.array_set(array, 0, path_view_value(4)).is_err());
    assert!(
        heap.struct_set_slot(
            record,
            &heap.struct_layout(record).unwrap(),
            0,
            host_root_value(5)
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
fn assigns_stable_object_identity_and_kind() {
    let heap = GcHeap::new(
        GcHeapConfig::default(),
        std::rc::Rc::new(crate::resource::ResourceState::default()),
    );
    let first = heap.alloc_array(vec![]).unwrap();
    let second = heap.alloc_map(vec![]).unwrap();
    let third = heap.alloc_set(vec![]).unwrap();
    let fourth = heap
        .alloc_struct(
            layout(
                "Empty",
                "value",
                AbiType::Builtin(kagari_abi::scalar::BuiltinType::Unit),
            ),
            vec![Value::Unit],
        )
        .unwrap();

    assert_ne!(first, second);
    assert_ne!(second, third);
    assert_ne!(third, fourth);
    assert_eq!(first.index(), 0);
    assert_eq!(second.index(), 1);
    assert_eq!(third.index(), 2);
    assert_eq!(fourth.index(), 3);
    assert_eq!(heap.object_kind(first), Some(GcObjectKind::Array));
    assert_eq!(heap.object_kind(second), Some(GcObjectKind::Map));
    assert_eq!(heap.object_kind(third), Some(GcObjectKind::Set));
    assert_eq!(heap.object_kind(fourth), Some(GcObjectKind::Struct));
}

#[test]
fn builtin_ordered_maps_preserve_insertion_order_and_account_units() {
    let heap = GcHeap::new(
        GcHeapConfig::default(),
        std::rc::Rc::new(crate::resource::ResourceState::default()),
    );
    let map = heap
        .alloc_map(vec![
            (Value::Str("b".to_owned()), Value::I32(2)),
            (Value::Str("a".to_owned()), Value::I32(1)),
            (Value::Str("b".to_owned()), Value::I32(3)),
        ])
        .unwrap();

    assert_eq!(heap.map_len(map), Some(2));
    assert_eq!(
        heap.map_snapshot(map),
        Some(vec![
            (Value::Str("b".to_owned()), Value::I32(3)),
            (Value::Str("a".to_owned()), Value::I32(1)),
        ])
    );
    assert_eq!(heap.stats().current_heap_units, 3);

    heap.map_insert(map, Value::Str("c".to_owned()), Value::I64(4))
        .unwrap();
    assert_eq!(heap.stats().current_heap_units, 4);
    assert_eq!(
        heap.map_get(map, &Value::Str("c".to_owned())),
        Some(Value::I64(4))
    );

    heap.map_insert(map, Value::Str("a".to_owned()), Value::I32(9))
        .unwrap();
    assert_eq!(heap.stats().current_heap_units, 4);
    assert_eq!(
        heap.map_snapshot(map).unwrap(),
        vec![
            (Value::Str("b".to_owned()), Value::I32(3)),
            (Value::Str("a".to_owned()), Value::I32(9)),
            (Value::Str("c".to_owned()), Value::I64(4)),
        ]
    );

    assert_eq!(
        heap.map_remove(map, &Value::Str("b".to_owned())).unwrap(),
        Some(Value::I32(3))
    );
    assert_eq!(heap.stats().current_heap_units, 3);
    heap.map_clear(map).unwrap();
    assert_eq!(heap.map_snapshot(map), Some(vec![]));
    assert_eq!(heap.stats().current_heap_units, 1);
}

#[test]
fn builtin_ordered_sets_preserve_insertion_order_and_account_units() {
    let heap = GcHeap::new(
        GcHeapConfig::default(),
        std::rc::Rc::new(crate::resource::ResourceState::default()),
    );
    let set = heap
        .alloc_set(vec![
            Value::Str("b".to_owned()),
            Value::Str("a".to_owned()),
            Value::Str("b".to_owned()),
        ])
        .unwrap();

    assert_eq!(heap.set_len(set), Some(2));
    assert_eq!(
        heap.set_snapshot(set),
        Some(vec![Value::Str("b".to_owned()), Value::Str("a".to_owned())])
    );
    assert_eq!(heap.stats().current_heap_units, 3);
    assert_eq!(
        heap.set_contains(set, &Value::Str("a".to_owned())),
        Some(true)
    );

    assert_eq!(heap.set_insert(set, Value::Str("c".to_owned())), Ok(true));
    assert_eq!(heap.stats().current_heap_units, 4);
    assert_eq!(heap.set_insert(set, Value::Str("a".to_owned())), Ok(false));
    assert_eq!(heap.stats().current_heap_units, 4);
    assert_eq!(heap.set_remove(set, &Value::Str("b".to_owned())), Ok(true));
    assert_eq!(heap.stats().current_heap_units, 3);
    heap.set_clear(set).unwrap();
    assert_eq!(heap.set_snapshot(set), Some(vec![]));
    assert_eq!(heap.stats().current_heap_units, 1);
}

#[test]
fn roots_are_explicit_storable_slots() {
    let heap = GcHeap::new(
        GcHeapConfig::default(),
        std::rc::Rc::new(crate::resource::ResourceState::default()),
    );
    let object = heap.alloc_array(vec![Value::I32(1)]).unwrap();
    let root = heap.root_value(Value::Array(object)).unwrap();

    assert_eq!(root.value(), Value::Array(object));
    assert_eq!(heap.active_roots(), 1);
    assert_eq!(heap.trace_roots().unwrap(), vec![object]);

    assert!(heap.root_value(host_root_value(1)).is_none());
    assert!(heap.root_value(path_view_value(1)).is_none());
    assert!(
        heap.root_value(Value::Tuple(vec![shared_borrow_value(2)]))
            .is_none()
    );
    assert_eq!(heap.active_roots(), 1);

    let bare_copy = root.value();
    let retained = root.clone();
    drop(root);
    assert_eq!(heap.collect(&[]).unwrap().live_objects, 1);
    drop(retained);
    assert_eq!(heap.collect(&[]).unwrap().reclaimed_objects, 1);
    assert!(!heap.validate_value(&bare_copy));
}

#[test]
fn root_scanning_traces_only_gc_managed_boundaries() {
    let heap = GcHeap::new(
        GcHeapConfig::default(),
        std::rc::Rc::new(crate::resource::ResourceState::default()),
    );
    let leaf = heap.alloc_array(vec![Value::I32(1)]).unwrap();
    let map = heap
        .alloc_map(vec![(Value::Str("leaf".to_owned()), Value::Array(leaf))])
        .unwrap();
    let set = heap.alloc_set(vec![Value::Str("seen".to_owned())]).unwrap();
    let record = heap
        .alloc_struct(
            layout(
                "Record",
                "map",
                AbiType::Map {
                    key: Box::new(AbiType::Builtin(kagari_abi::scalar::BuiltinType::String)),
                    value: Box::new(AbiType::Array(
                        Box::new(AbiType::Builtin(kagari_abi::scalar::BuiltinType::I32)),
                        CollectionAccess::Mutable,
                    )),
                    access: CollectionAccess::Mutable,
                },
            ),
            vec![Value::Map(map)],
        )
        .unwrap();

    let root = heap
        .root_value(Value::Tuple(vec![
            Value::Struct(record),
            Value::Set(set),
            Value::Unit,
        ]))
        .unwrap();

    assert_eq!(heap.trace_roots().unwrap(), vec![record, map, leaf, set]);

    root.set(&heap, Value::GcHandle(leaf)).unwrap();
    assert_eq!(heap.trace_roots().unwrap(), vec![leaf]);

    assert_eq!(root.value(), Value::GcHandle(leaf));
    drop(root);
    assert_eq!(heap.trace_roots().unwrap(), Vec::<HeapObjectId>::new());
}

#[test]
fn root_scanning_handles_cycles_without_duplicate_identity() {
    let heap = GcHeap::new(
        GcHeapConfig::default(),
        std::rc::Rc::new(crate::resource::ResourceState::default()),
    );
    let array = heap.alloc_array(vec![]).unwrap();
    let record = heap
        .alloc_struct(
            layout(
                "Cycle",
                "array",
                AbiType::Array(
                    Box::new(AbiType::Builtin(kagari_abi::scalar::BuiltinType::I32)),
                    CollectionAccess::Mutable,
                ),
            ),
            vec![Value::Array(array)],
        )
        .unwrap();
    heap.array_push(array, Value::Struct(record)).unwrap();
    let _root = heap.root_value(Value::Array(array)).unwrap();

    assert_eq!(heap.trace_roots().unwrap(), vec![array, record]);
}
#[test]
fn removal_results_distinguish_absence_from_iteration_and_stale_handle_errors() {
    let heap = GcHeap::new(Default::default(), Default::default());
    let array = heap.alloc_array(vec![]).unwrap();
    let map = heap.alloc_map(vec![]).unwrap();
    let set = heap.alloc_set(vec![]).unwrap();
    assert_eq!(heap.array_pop(array).unwrap(), None);
    assert_eq!(heap.array_remove(array, 0).unwrap(), None);
    assert_eq!(heap.map_remove(map, &Value::I32(1)).unwrap(), None);
    assert!(!heap.set_remove(set, &Value::I32(1)).unwrap());
    assert!(heap.map_remove(map, &Value::F64(1.0)).is_err());
    assert!(heap.set_remove(set, &Value::F64(1.0)).is_err());
    let guards = [Value::Array(array), Value::Map(map), Value::Set(set)]
        .map(|value| heap.begin_collection_iteration(&value).unwrap());
    let before = heap.stats().current_heap_units;
    for result in [
        heap.array_pop(array).map(|_| ()),
        heap.array_remove(array, 0).map(|_| ()),
        heap.array_clear(array),
        heap.map_remove(map, &Value::I32(1)).map(|_| ()),
        heap.map_clear(map),
        heap.set_remove(set, &Value::I32(1)).map(|_| ()),
        heap.set_clear(set),
    ] {
        let error = result.unwrap_err();
        assert_eq!(error.kind(), RuntimeErrorKind::ScriptTrap);
        assert_eq!(error.message(), "structural modification during iteration");
    }
    assert_eq!(heap.stats().current_heap_units, before);
    drop(guards);
    heap.array_clear(array).unwrap();
    heap.map_clear(map).unwrap();
    heap.set_clear(set).unwrap();
    heap.collect(&[]).unwrap();
    assert!(heap.array_pop(array).is_err());
    assert!(heap.array_remove(array, 0).is_err());
    assert!(heap.array_clear(array).is_err());
    assert!(heap.map_remove(map, &Value::I32(1)).is_err());
    assert!(heap.map_clear(map).is_err());
    assert!(heap.set_remove(set, &Value::I32(1)).is_err());
    assert!(heap.set_clear(set).is_err());
}
#[test]
fn replacement_errors_preserve_targets_and_internal_fault_categories() {
    let resources = Rc::new(crate::resource::ResourceState::default());
    let heap = GcHeap::new(Default::default(), resources.clone());
    let array = heap.alloc_array(vec![Value::I32(1)]).unwrap();
    let before = heap.stats().current_heap_units;
    let foreign_heap = GcHeap::new(Default::default(), Default::default());
    let foreign = foreign_heap.alloc_array(vec![]).unwrap();
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
        AbiType::Builtin(kagari_abi::scalar::BuiltinType::I32),
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
    let error = crate::reflection::set_field(&heap, &Value::Struct(object), "value", Value::I32(9))
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
        crate::reflection::set_index(&heap, &Value::Array(array), &Value::I32(0), Value::I32(9))
            .unwrap_err();
    assert_eq!(
        error.into_write_error().kind(),
        RuntimeErrorKind::EngineFault
    );
}
