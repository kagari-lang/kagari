use super::*;
use crate::{
    Runtime,
    host::{
        DynamicPathArguments, HostBorrowTable, HostObjectId, HostPathDescriptorRegistration,
        HostPathSegmentRegistration, HostRootHandle, HostSchemaEpoch, HostTypeRegistration,
    },
    layout_fixtures::allocation_owner,
    metadata::{AbiFingerprint, TypeId},
};
use kagari_types::{
    host_interface::{
        type_declaration::{
            HostFieldDeclaration, HostTypeDeclaration, HostTypeOwnership, PathAccess,
        },
        value_type::HostValueType,
    },
    scalar::BuiltinType,
    ty::Ty,
};

#[test]
fn interface_roots_trace_data_and_retain_old_dependency_versions() {
    let mut runtime = crate::Runtime::default();
    let owner = allocation_owner(&mut runtime);
    let array = runtime
        .alloc_array(&owner, Ty::Builtin(BuiltinType::I32), vec![Value::I32(7)])
        .unwrap();
    let interface = crate::layout_fixtures::interface_value_with(
        &mut runtime,
        Ty::Array(
            Box::new(Ty::Builtin(kagari_types::scalar::BuiltinType::I32)),
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

fn layout(name: &str, field: &str, ty: Ty) -> crate::module::StructLayoutRef {
    crate::layout_fixtures::layout(&mut crate::Runtime::default(), name, &[(field, ty, true)])
}

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
    let mut declaration = HostTypeDeclaration::new("Player");
    declaration.ownership = HostTypeOwnership::HostRoot;
    declaration.path_access = PathAccess::ReadWrite;
    let mut hp = HostFieldDeclaration::new(&declaration.id, "hp", HostValueType::I32);
    hp.writable = true;
    hp.path_access = PathAccess::ReadWrite;
    declaration.fields.push(hp);
    let root_type = runtime
        .register_host_type(HostTypeRegistration::new(declaration, "Player"))
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
    let mut runtime = Runtime::default();
    let owner = allocation_owner(&mut runtime);
    let heap = runtime.gc();

    assert!(
        runtime
            .alloc_array(
                &owner,
                Ty::Builtin(BuiltinType::I32),
                vec![shared_borrow_value(1)]
            )
            .is_err()
    );
    assert!(
        runtime
            .alloc_array(
                &owner,
                Ty::Builtin(BuiltinType::I32),
                vec![unique_borrow_value(2)]
            )
            .is_err()
    );
    assert_eq!(heap.allocated_objects(), 0);
}

#[test]
fn rejects_host_handles_and_path_views_as_default_heap_payloads() {
    let mut runtime = Runtime::default();
    let owner = allocation_owner(&mut runtime);
    let heap = runtime.gc();

    assert!(
        runtime
            .alloc_array(
                &owner,
                Ty::Builtin(BuiltinType::I32),
                vec![host_root_value(1)]
            )
            .is_err()
    );
    assert!(
        heap.alloc_struct(
            layout(
                "HostBacked",
                "path",
                Ty::Array(
                    Box::new(Ty::Builtin(kagari_types::scalar::BuiltinType::I32)),
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
fn roots_are_explicit_storable_slots() {
    let mut runtime = Runtime::default();
    let owner = allocation_owner(&mut runtime);
    let heap = runtime.gc();
    let object = runtime
        .alloc_array(&owner, Ty::Builtin(BuiltinType::I32), vec![Value::I32(1)])
        .unwrap();
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
