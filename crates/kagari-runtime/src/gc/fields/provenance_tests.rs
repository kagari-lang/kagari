//! Direct references retain provenance through packed fields and graph reclamation.
use crate::{
    Runtime,
    gc::object_header::ObjectHeader,
    test_support::{
        layouts::{allocation_owner, layout},
        native::program::ProgramFixture,
    },
    value::Value,
};
use kagari_bytecode::{instruction::StructId, module::BytecodeModule, program::ModuleRef};
use kagari_common::identity::{DefinitionKind, ModuleIdentity, map::DefinitionContext};
use kagari_contract::layout::{StructFieldLayout, StructLayout};
use kagari_types::{
    scalar::BuiltinType,
    ty::{NominalTy, Ty},
    visibility::Visibility,
};
use std::sync::Arc;

#[test]
fn packed_managed_edges_survive_collection_and_release_replaced_graphs() {
    let mut runtime = Runtime::default();
    let owner = allocation_owner(&mut runtime);
    let string_type = Ty::Builtin(BuiltinType::String);
    let shape = layout(
        &mut runtime,
        "BuiltinEdges",
        &[
            ("prefix", Ty::Builtin(BuiltinType::Bool), false),
            ("label", string_type.clone(), true),
            (
                "pair",
                Ty::Tuple(vec![string_type.clone(), string_type.clone()]),
                true,
            ),
            ("items", Ty::Array(Box::new(string_type.clone())), true),
        ],
    );
    let heap = runtime.gc();
    let old = heap.alloc_string("old".into()).unwrap();
    let sibling = heap.alloc_string("sibling".into()).unwrap();
    let pair = heap.alloc_tuple(vec![old, sibling]).unwrap();
    let array = heap
        .alloc_array(&owner, string_type.clone(), vec![old, sibling])
        .unwrap();
    let record = heap
        .alloc_struct(
            shape.clone(),
            vec![Value::Bool(true), old, pair, Value::Array(array)],
        )
        .unwrap();
    let root = runtime.root_value(Value::Struct(record)).unwrap();
    let label_field = heap.prepare_struct_field(shape.clone(), 1).unwrap();
    let pair_field = heap.prepare_struct_field(shape.clone(), 2).unwrap();
    let array_field = heap.prepare_struct_field(shape.clone(), 3).unwrap();
    assert_eq!(runtime.collect_garbage().unwrap().live_objects, 5);
    assert_eq!(
        unsafe { array_field.read(heap, record) },
        Some(Value::Array(array))
    );
    assert_eq!(heap.array_snapshot(array), Some(vec![old, sibling]));
    // SAFETY: the root retains both unaligned pointer fields and all tuple edges.
    let Value::Str(label) = (unsafe { label_field.read(heap, record) }).unwrap() else {
        panic!("declared string field");
    };
    assert_eq!(&*heap.string(label).unwrap(), "old");
    let Value::Tuple(tuple) = (unsafe { pair_field.read(heap, record) }).unwrap() else {
        panic!("declared tuple field");
    };
    assert_eq!(&*heap.tuple(tuple).unwrap(), &[old, sibling]);
    let replacement = heap.alloc_string("new".into()).unwrap();
    let next_pair = heap.alloc_tuple(vec![replacement, replacement]).unwrap();
    let next_array = heap
        .alloc_array(&owner, string_type, vec![replacement])
        .unwrap();
    // SAFETY: allocation/publication cannot collect between these operations;
    // the destination root remains live through both field commits.
    unsafe { label_field.write(heap, record, replacement) }.unwrap();
    unsafe { pair_field.write(heap, record, next_pair) }.unwrap();
    unsafe { array_field.write(heap, record, Value::Array(next_array)) }.unwrap();
    let collected = runtime.collect_garbage().unwrap();
    assert_eq!(
        (collected.reclaimed_objects, collected.live_objects),
        (4, 4)
    );
    assert_eq!(heap.struct_get_slot(record, &shape, 1), Some(replacement));
    let Value::Tuple(tuple) = heap.struct_get_slot(record, &shape, 2).unwrap() else {
        panic!("replacement tuple field");
    };
    assert_eq!(&*heap.tuple(tuple).unwrap(), &[replacement, replacement]);
    assert_eq!(
        unsafe { array_field.read(heap, record) },
        Some(Value::Array(next_array))
    );
    assert_eq!(heap.array_snapshot(next_array), Some(vec![replacement]));
    drop(root);
    assert_eq!(runtime.collect_garbage().unwrap().reclaimed_objects, 4);
    assert_eq!(heap.stats().current_retained_bytes, 0);
}

#[test]
fn packed_direct_struct_edges_preserve_provenance_and_collection_lifetimes() {
    let mut runtime = Runtime::default();
    let context = DefinitionContext::new().unwrap();
    let identity = ModuleIdentity::single_file("direct-edges.kgr");
    let module = context.intern_root(&identity).unwrap();
    let child = context
        .intern_child(module, DefinitionKind::Struct, "Child", 0)
        .unwrap();
    let parent = context
        .intern_child(module, DefinitionKind::Struct, "Parent", 0)
        .unwrap();
    let field = |owner, name: &str, ty, mutable| StructFieldLayout {
        declaration: context
            .intern_child(owner, DefinitionKind::Field, name, 0)
            .unwrap(),
        name: name.to_owned(),
        visibility: Visibility::Public,
        ty,
        mutable,
    };
    // The source-free fixture is Child { value: i32 } and
    // Parent { prefix: bool, child: Child }. The leading byte intentionally leaves
    // the packed pointer unaligned. Normal bytecode validation admits both layouts.
    let program = runtime
        .load_program(
            "direct-edges",
            ProgramFixture {
                exports: Default::default(),
                root: ModuleRef::new(0),
                modules: vec![BytecodeModule {
                    identity,
                    structures: vec![
                        StructLayout {
                            declaration: child,
                            arguments: vec![],
                            fields: vec![field(
                                child,
                                "value",
                                Ty::Builtin(BuiltinType::I32),
                                true,
                            )],
                        },
                        StructLayout {
                            declaration: parent,
                            arguments: vec![],
                            fields: vec![
                                field(parent, "prefix", Ty::Builtin(BuiltinType::Bool), false),
                                field(
                                    parent,
                                    "child",
                                    Ty::Struct(NominalTy {
                                        declaration: child,
                                        arguments: vec![],
                                        associated_types: Default::default(),
                                    }),
                                    true,
                                ),
                            ],
                        },
                    ],
                    ..Default::default()
                }],
            }
            .metadata(context.snapshot(), &Default::default())
            .unwrap(),
        )
        .unwrap();
    let owner = runtime.select_program(&program).unwrap();
    let child_shape = owner.struct_layout(StructId::new(0)).unwrap();
    let parent_shape = owner.struct_layout(StructId::new(1)).unwrap();
    let heap = runtime.gc();
    let child = heap
        .alloc_struct(child_shape.clone(), vec![Value::I32(7)])
        .unwrap();
    let child_field = heap.prepare_struct_field(child_shape.clone(), 0).unwrap();
    let parent_field = heap.prepare_struct_field(parent_shape.clone(), 1).unwrap();
    let physical_description = heap
        .struct_storage_description(child_shape.clone())
        .unwrap();
    let descriptor_id = physical_description.struct_identity().registered_id();
    let weak_description = Arc::downgrade(&physical_description);
    let weak_scan = Arc::downgrade(physical_description.structure.as_ref().unwrap());
    drop(physical_description);
    let old_identity = heap.objects.borrow().direct_location(child).unwrap();
    let parent = heap
        .alloc_struct(
            parent_shape.clone(),
            vec![Value::Bool(true), Value::Struct(child)],
        )
        .unwrap();
    let root = runtime.root_value(Value::Struct(parent)).unwrap();
    assert_eq!(runtime.collect_garbage().unwrap().live_objects, 2);
    // SAFETY: this runtime's root retains both references; no collection or
    // callback occurs until their copied results have been consumed.
    let copied = unsafe { parent_field.read(heap, parent) }.unwrap();
    assert_eq!(
        unsafe { child_field.read_scalar(heap, child, BuiltinType::I32) },
        Some(7)
    );
    // A different descriptor must reject before forming an oversized payload loan.
    assert_eq!(unsafe { parent_field.read(heap, child) }, None);
    assert_eq!(
        unsafe { child_field.read_scalar(heap, parent, BuiltinType::I32) },
        None
    );
    assert_eq!(copied, Value::Struct(child));
    assert_eq!(
        heap.struct_get_slot(child, &child_shape, 0),
        Some(Value::I32(7))
    );

    let next = heap
        .alloc_struct(child_shape.clone(), vec![Value::I32(9)])
        .unwrap();
    // SAFETY: both references belong to this heap and remain live until commit;
    // the new allocation cannot collect between construction and publication.
    unsafe { parent_field.write(heap, parent, Value::Struct(next)) }.unwrap();
    unsafe { child_field.write_scalar(heap, next, BuiltinType::I32, 11) }.unwrap();
    let prefix = heap.prepare_struct_field(parent_shape.clone(), 0).unwrap();
    assert!(unsafe { prefix.write_scalar(heap, parent, BuiltinType::Bool, 0) }.is_err());
    assert_eq!(
        unsafe { prefix.read(heap, parent) },
        Some(Value::Bool(true))
    );
    assert_eq!(runtime.collect_garbage().unwrap().live_objects, 2);
    assert!(heap.objects.borrow().structure(old_identity).is_none());
    assert_eq!(
        // SAFETY: the parent root retains the newly published child edge.
        unsafe { parent_field.read(heap, parent) },
        Some(Value::Struct(next))
    );
    assert_eq!(
        // SAFETY: the parent root retains the newly published child edge.
        unsafe { child_field.read_scalar(heap, next, BuiltinType::I32) },
        Some(11)
    );
    drop(root);
    assert_eq!(runtime.collect_garbage().unwrap().live_objects, 0);
    assert!(weak_description.upgrade().is_none());
    assert!(
        weak_scan.upgrade().is_none(),
        "field preparation must not retain scan tables"
    );
    drop(weak_description);
    drop(weak_scan);
    // Empty physical pages may be reused in later epochs. Mark backing and work
    // registration may retire, so admission must reserve them again on reuse.
    for value in 0..3 {
        let reference = heap
            .alloc_struct(child_shape.clone(), vec![Value::I32(value)])
            .unwrap();
        let root = runtime.root_value(Value::Struct(reference)).unwrap();
        assert_eq!(runtime.collect_garbage().unwrap().live_objects, 1);
        // SAFETY: the root retains this runtime's allocation and no collection
        // intervenes during the header/read checks.
        assert_eq!(
            unsafe { ObjectHeader::read(reference.pointer()) }.descriptor_id,
            descriptor_id
        );
        assert_eq!(
            unsafe { child_field.read_scalar(heap, reference, BuiltinType::I32) },
            Some(value as u64)
        );
        let descriptor = heap
            .struct_storage_description(child_shape.clone())
            .unwrap();
        let weak_scan = Arc::downgrade(descriptor.structure.as_ref().unwrap());
        drop(descriptor);
        assert_eq!(
            heap.struct_get_slot(reference, &child_shape, 0),
            Some(Value::I32(value))
        );
        drop(root);
        assert_eq!(runtime.collect_garbage().unwrap().live_objects, 0);
        assert!(weak_scan.upgrade().is_none());
    }
}
