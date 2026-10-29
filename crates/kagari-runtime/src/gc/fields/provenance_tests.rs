//! Direct references retain provenance through packed fields and graph reclamation.
use crate::{Runtime, test_support::native::program::ProgramFixture, value::Value};
use kagari_bytecode::{instruction::StructId, module::BytecodeModule, program::ModuleRef};
use kagari_common::identity::{DefinitionKind, ModuleIdentity, map::DefinitionContext};
use kagari_contract::layout::{StructFieldLayout, StructLayout};
use kagari_types::{
    scalar::BuiltinType,
    ty::{NominalTy, Ty},
    visibility::Visibility,
};

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
    let old_identity = heap.objects.borrow().struct_location(child).unwrap();
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
    // Empty physical pages may be reused in later epochs. Their mark blocks keep
    // registered work capacity, but must neither lose nor retain a new occupant.
    for value in 0..3 {
        let reference = heap
            .alloc_struct(child_shape.clone(), vec![Value::I32(value)])
            .unwrap();
        let root = runtime.root_value(Value::Struct(reference)).unwrap();
        assert_eq!(runtime.collect_garbage().unwrap().live_objects, 1);
        assert_eq!(
            heap.struct_get_slot(reference, &child_shape, 0),
            Some(Value::I32(value))
        );
        drop(root);
        assert_eq!(runtime.collect_garbage().unwrap().live_objects, 0);
    }
}
