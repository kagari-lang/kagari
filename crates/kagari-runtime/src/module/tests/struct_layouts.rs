use crate::{Runtime, error::RuntimeErrorKind, reflection, value::Value};
use kagari_bytecode::{
    instruction::StructId,
    program::{BytecodeProgram, ModuleRef},
};
use kagari_common::identity::{map::DefinitionContext, metadata::DefinitionMetadata};
use kagari_types::{
    scalar::BuiltinType,
    ty::{NominalTy, Ty},
};

use crate::test_support::layouts;

#[test]
fn slot_access_checks_nominal_owner_schema_permission_and_representation() {
    let mut runtime = Runtime::default();
    let layout = layouts::layout(
        &mut runtime,
        "Point",
        &[
            ("x", Ty::Builtin(BuiltinType::I32), true),
            ("fixed", Ty::Builtin(BuiltinType::Bool), false),
        ],
    );
    let object = runtime
        .alloc_struct(layout.clone(), vec![Value::I32(1), Value::Bool(true)])
        .unwrap();
    let context = DefinitionContext::new().unwrap();
    let mut foreign = layout
        .module()
        .to_unverified(&Default::default())
        .unwrap()
        .import_into(&context, &Default::default())
        .unwrap()
        .into_records();
    let relocate = |id| {
        let mut path = context.snapshot().resolve(id).unwrap().to_path();
        path.module.path = vec!["other.kgr".into()];
        context.intern(&path).unwrap()
    };
    foreign.structures[0].declaration = relocate(foreign.structures[0].declaration);
    for field in &mut foreign.structures[0].fields {
        field.declaration = relocate(field.declaration);
    }
    let foreign = runtime
        .load_program(
            "other",
            DefinitionMetadata::checked(
                context.snapshot(),
                BytecodeProgram {
                    exports: Default::default(),
                    root: ModuleRef::new(0),
                    modules: vec![foreign],
                },
                &Default::default(),
            )
            .unwrap(),
        )
        .unwrap()
        .struct_layout(StructId::new(0))
        .unwrap();
    let heap = runtime.gc();
    assert!(heap.struct_get_slot(object, &foreign, 0).is_none());
    assert!(
        heap.struct_set_slot(object, &foreign, 0, Value::I32(99))
            .is_err()
    );
    assert!(
        heap.struct_set_slot(object, &layout, 1, Value::Bool(false))
            .is_err()
    );
    assert!(
        heap.struct_set_slot(object, &layout, 0, Value::Bool(false))
            .is_err()
    );
    assert!(
        heap.struct_set_slot(object, &layout, usize::MAX, Value::I32(99))
            .is_err()
    );
    assert!(
        reflection::set_field(heap, &Value::Struct(object), "fixed", Value::Bool(false)).is_err()
    );
    assert!(reflection::set_field(heap, &Value::Struct(object), "x", Value::Bool(false)).is_err());
    assert_eq!(
        heap.struct_get_slot(object, &layout, 0),
        Some(Value::I32(1))
    );
    assert_eq!(
        heap.struct_get_slot(object, &layout, 1),
        Some(Value::Bool(true))
    );
    reflection::set_field(heap, &Value::Struct(object), "x", Value::I32(42)).unwrap();
    assert_eq!(
        heap.struct_get_slot(object, &layout, 0),
        Some(Value::I32(42))
    );
}

#[test]
fn allocation_rejects_foreign_layout_and_invalid_initializers_before_accounting() {
    let mut runtime = Runtime::default();
    let layout = layouts::layout(
        &mut runtime,
        "Point",
        &[("x", Ty::Builtin(BuiltinType::I32), true)],
    );
    let mut other = Runtime::default();
    let foreign = layouts::layout(
        &mut other,
        "Point",
        &[("x", Ty::Builtin(BuiltinType::I32), true)],
    );
    let counters = runtime.resources().counters();
    assert_eq!(
        runtime
            .alloc_struct(foreign.clone(), vec![Value::I32(7)])
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ModuleValidation
    );
    for fields in [
        vec![],
        vec![Value::Bool(false)],
        vec![Value::I32(1), Value::I32(2)],
    ] {
        assert_eq!(
            runtime
                .alloc_struct(layout.clone(), fields)
                .unwrap_err()
                .kind(),
            RuntimeErrorKind::ScriptTrap
        );
    }
    assert_eq!(runtime.resources().counters(), counters);
    assert_eq!(runtime.gc().allocated_objects(), 0);
    let object = runtime.alloc_struct(layout, vec![Value::I32(1)]).unwrap();
    assert!(runtime.gc().struct_get_slot(object, &foreign, 0).is_none());
    assert!(
        runtime
            .gc()
            .struct_set_slot(object, &foreign, 0, Value::I32(2))
            .is_err()
    );
}

#[test]
fn objects_retain_old_layouts_and_require_equal_schemas_across_generations() {
    let mut runtime = Runtime::default();
    let original = layouts::layout(
        &mut runtime,
        "Point",
        &[("x", Ty::Builtin(BuiltinType::I32), true)],
    );
    let object = runtime
        .alloc_struct(original.clone(), vec![Value::I32(1)])
        .unwrap();
    let _root = runtime.root_value(Value::Struct(object)).unwrap();
    let (definitions, bytecode) = original
        .module()
        .to_unverified(&Default::default())
        .unwrap()
        .into_parts();
    let next = runtime
        .stage_reload_program(
            original.module(),
            "Point",
            DefinitionMetadata::checked(
                definitions,
                BytecodeProgram {
                    exports: Default::default(),
                    root: ModuleRef::new(0),
                    modules: vec![bytecode],
                },
                &Default::default(),
            )
            .unwrap(),
        )
        .unwrap();
    let next = runtime.publish_staged_reload(next).unwrap();
    let compatible = next.struct_layout(StructId::new(0)).unwrap();
    assert_ne!(original.module().key(), compatible.module().key());
    runtime
        .gc()
        .struct_set_slot(object, &compatible, 0, Value::I32(42))
        .unwrap();
    let original_key = original.module().key();
    drop(original);
    assert!(
        runtime
            .collect_garbage()
            .unwrap()
            .reclaimed_modules
            .contains(&original_key)
    );
    let retained = runtime.gc().struct_layout(object).unwrap();
    assert_eq!(retained.module().key(), original_key);
    assert_eq!(
        reflection::get_field(runtime.gc(), &Value::Struct(object), "x").unwrap(),
        Value::I32(42)
    );
    // A retained verified generation can still allocate without a module-store lookup.
    assert!(
        runtime
            .alloc_struct(retained.clone(), vec![Value::I32(3)])
            .is_ok()
    );
    let changed = layouts::layout(
        &mut runtime,
        "Point",
        &[("x", Ty::Builtin(BuiltinType::I32), false)],
    );
    assert!(runtime.gc().struct_get_slot(object, &changed, 0).is_none());
    assert!(
        runtime
            .gc()
            .struct_set_slot(object, &changed, 0, Value::I32(99))
            .is_err()
    );
    assert_eq!(
        runtime.gc().struct_get_slot(object, &retained, 0),
        Some(Value::I32(42))
    );
}

#[test]
fn nested_field_types_reject_wrong_nominals_before_allocation_or_commit() {
    let mut runtime = Runtime::default();
    let leaf = layouts::layout(
        &mut runtime,
        "Leaf",
        &[("x", Ty::Builtin(BuiltinType::I32), true)],
    );
    let wrapper = layouts::layout(
        &mut runtime,
        "Wrapper",
        &[("items", Ty::Builtin(BuiltinType::I32), true)],
    );
    let wrong = layouts::layout(
        &mut runtime,
        "Other",
        &[("x", Ty::Builtin(BuiltinType::I32), true)],
    );
    let wrong_owner = wrong.module().clone();
    let wrong_type = Ty::Struct(NominalTy {
        declaration: wrong.layout().declaration,
        arguments: vec![],
        associated_types: Default::default(),
    });
    let wrong_value = Value::Struct(runtime.alloc_struct(wrong, vec![Value::I32(9)]).unwrap());
    let context = DefinitionContext::new().unwrap();
    let mut bytecode = wrapper
        .module()
        .to_unverified(&Default::default())
        .unwrap()
        .import_into(&context, &Default::default())
        .unwrap()
        .into_records();
    let leaf = leaf
        .module()
        .to_unverified(&Default::default())
        .unwrap()
        .import_into(&context, &Default::default())
        .unwrap()
        .into_records();
    bytecode.structures.push(leaf.structures[0].clone());
    bytecode.structures[0].fields[0].ty = Ty::Array(Box::new(Ty::Tuple(vec![
        Ty::Struct(NominalTy {
            associated_types: Default::default(),
            declaration: bytecode.structures[1].declaration,
            arguments: vec![],
        }),
        Ty::Builtin(BuiltinType::Bool),
    ])));
    let module = runtime
        .load_program(
            "concrete",
            DefinitionMetadata::checked(
                context.snapshot(),
                BytecodeProgram {
                    exports: Default::default(),
                    root: ModuleRef::new(0),
                    modules: vec![bytecode],
                },
                &Default::default(),
            )
            .unwrap(),
        )
        .unwrap();
    let wrapper = module.struct_layout(StructId::new(0)).unwrap();
    let leaf = module.struct_layout(StructId::new(1)).unwrap();
    let valid = Value::Struct(runtime.alloc_struct(leaf, vec![Value::I32(42)]).unwrap());
    let Ty::Array(element) = &wrapper.layout().fields[0].ty else {
        panic!("array field")
    };
    let element = (**element).clone();
    let initial = Value::Array(
        runtime
            .alloc_array(
                &module,
                element.clone(),
                vec![
                    runtime
                        .gc()
                        .alloc_tuple(vec![valid, Value::Bool(true)])
                        .unwrap(),
                ],
            )
            .unwrap(),
    );
    let target = runtime
        .alloc_struct(wrapper.clone(), vec![initial])
        .unwrap();
    let Ty::Tuple(mut wrong_member_type) = element.clone() else {
        panic!("tuple element")
    };
    wrong_member_type[1] = Ty::Builtin(BuiltinType::I32);
    for (owner, ty, values) in [
        (
            &wrong_owner,
            Ty::Tuple(vec![wrong_type, Ty::Builtin(BuiltinType::Bool)]),
            vec![
                runtime
                    .gc()
                    .alloc_tuple(vec![wrong_value, Value::Bool(true)])
                    .unwrap(),
            ],
        ),
        (
            &module,
            Ty::Tuple(wrong_member_type.clone()),
            vec![
                runtime
                    .gc()
                    .alloc_tuple(vec![valid, Value::I32(1)])
                    .unwrap(),
            ],
        ),
        (&module, Ty::Tuple(wrong_member_type), vec![]),
    ] {
        let invalid = Value::Array(runtime.alloc_array(owner, ty, values).unwrap());
        let before = runtime.resources().counters();
        let objects = runtime.gc().allocated_objects();
        assert_eq!(
            runtime
                .alloc_struct(wrapper.clone(), vec![invalid])
                .unwrap_err()
                .kind(),
            RuntimeErrorKind::ScriptTrap
        );
        assert_eq!(
            runtime
                .gc()
                .struct_set_slot(target, &wrapper, 0, invalid)
                .unwrap_err()
                .kind(),
            RuntimeErrorKind::ScriptTrap
        );
        assert_eq!(
            runtime.gc().struct_get_slot(target, &wrapper, 0),
            Some(initial)
        );
        assert_eq!(runtime.resources().counters(), before);
        assert_eq!(runtime.gc().allocated_objects(), objects);
    }
    let replacement = Value::Array(runtime.alloc_array(&module, element, vec![]).unwrap());
    runtime
        .gc()
        .struct_set_slot(target, &wrapper, 0, replacement)
        .unwrap();
    assert_eq!(
        runtime.gc().struct_get_slot(target, &wrapper, 0),
        Some(replacement)
    );
}

#[test]
fn mixed_scalar_fields_preserve_full_domains_and_float_bits_at_value_boundaries() {
    let cases = [
        (BuiltinType::Bool, Value::Bool(true)),
        (BuiltinType::Unit, Value::Unit),
        (BuiltinType::I8, Value::I32(i8::MIN.into())),
        (BuiltinType::I16, Value::I32(i16::MIN.into())),
        (BuiltinType::I32, Value::I32(i32::MIN)),
        (BuiltinType::I64, Value::I64(i64::MIN)),
        (BuiltinType::ISize, Value::I64(i64::MAX)),
        (BuiltinType::U8, Value::I64(u8::MAX.into())),
        (BuiltinType::U16, Value::I64(u16::MAX.into())),
        (BuiltinType::U32, Value::I64(u32::MAX.into())),
        (BuiltinType::U64, Value::U64(u64::MAX)),
        (BuiltinType::USize, Value::U64(u64::MAX)),
        (BuiltinType::F32, Value::F32(f32::from_bits(0x7fc1_2345))),
        (
            BuiltinType::F64,
            Value::F64(f64::from_bits(0x7ff8_1234_5678_9abc)),
        ),
    ];
    let names = (0..cases.len())
        .map(|slot| format!("field{slot}"))
        .collect::<Vec<_>>();
    let fields = cases
        .iter()
        .zip(&names)
        .map(|((ty, _), name)| (name.as_str(), Ty::Builtin(*ty), true))
        .collect::<Vec<_>>();
    let mut runtime = Runtime::default();
    let layout = layouts::layout(&mut runtime, "Scalars", &fields);
    let counters = runtime.resources().counters();
    for (slot, invalid) in [
        (2, Value::I32(-129)),
        (2, Value::I32(128)),
        (3, Value::I32(-32_769)),
        (3, Value::I32(32_768)),
        (7, Value::I64(-1)),
        (7, Value::I64(256)),
        (8, Value::I64(-1)),
        (8, Value::I64(65_536)),
        (9, Value::I64(-1)),
        (9, Value::I64(4_294_967_296)),
    ] {
        let mut initial = cases.iter().map(|(_, value)| *value).collect::<Vec<_>>();
        initial[slot] = invalid;
        assert_eq!(
            runtime
                .alloc_struct(layout.clone(), initial)
                .unwrap_err()
                .kind(),
            RuntimeErrorKind::ScriptTrap
        );
    }
    assert_eq!(runtime.resources().counters(), counters);
    assert_eq!(runtime.gc().allocated_objects(), 0);
    let object = runtime
        .alloc_struct(
            layout.clone(),
            cases.iter().map(|(_, value)| *value).collect(),
        )
        .unwrap();
    let root = runtime.root_value(Value::Struct(object)).unwrap();
    runtime.collect_garbage().unwrap();
    for (slot, (_, expected)) in cases.iter().enumerate() {
        let actual = runtime.gc().struct_get_slot(object, &layout, slot).unwrap();
        match (actual, expected) {
            (Value::F32(actual), Value::F32(expected)) => {
                assert_eq!(actual.to_bits(), expected.to_bits())
            }
            (Value::F64(actual), Value::F64(expected)) => {
                assert_eq!(actual.to_bits(), expected.to_bits())
            }
            (actual, expected) => assert_eq!(actual, *expected),
        }
    }
    runtime
        .gc()
        .struct_set_slot(object, &layout, 12, Value::F32(-0.0))
        .unwrap();
    runtime
        .gc()
        .struct_set_slot(object, &layout, 13, Value::F64(-0.0))
        .unwrap();
    let (_, fields) = runtime.gc().struct_snapshot(object).unwrap();
    let Value::F32(value) = fields[12].value else {
        unreachable!()
    };
    assert_eq!(value.to_bits(), (-0.0_f32).to_bits());
    let Value::F64(value) = fields[13].value else {
        unreachable!()
    };
    assert_eq!(value.to_bits(), (-0.0_f64).to_bits());
    assert!(
        runtime
            .gc()
            .struct_set_slot(object, &layout, 2, Value::I32(128))
            .is_err()
    );
    assert_eq!(
        runtime.gc().struct_get_slot(object, &layout, 2),
        Some(Value::I32(-128))
    );
    drop(root);
    assert_eq!(runtime.collect_garbage().unwrap().live_objects, 0);
}

#[test]
fn mixed_layout_relocation_preserves_references_empty_records_and_aliases() {
    let mut runtime = Runtime::default();
    let empty = layouts::layout(&mut runtime, "Empty", &[]);
    let small = layouts::layout(
        &mut runtime,
        "Small",
        &[("flag", Ty::Builtin(BuiltinType::Bool), true)],
    );
    let packed = layouts::layout(
        &mut runtime,
        "Packed",
        &[
            ("number", Ty::Builtin(BuiltinType::I8), true),
            ("text", Ty::Builtin(BuiltinType::String), true),
            ("bits", Ty::Builtin(BuiltinType::F64), true),
        ],
    );
    let mut kept = Vec::new();
    for index in 0..200_i32 {
        let zero = runtime.alloc_struct(empty.clone(), vec![]).unwrap();
        let flag = runtime
            .alloc_struct(small.clone(), vec![Value::Bool(true)])
            .unwrap();
        let text = runtime.gc().alloc_string(index.to_string()).unwrap();
        let bits = 0x7ff8_0000_0000_0000 + index as u64;
        let record = runtime
            .alloc_struct(
                packed.clone(),
                vec![
                    Value::I32(index - 100),
                    text,
                    Value::F64(f64::from_bits(bits)),
                ],
            )
            .unwrap();
        if index % 17 == 0 {
            let tuple = runtime
                .gc()
                .alloc_tuple(vec![
                    Value::Struct(zero),
                    Value::Struct(flag),
                    Value::Struct(record),
                ])
                .unwrap();
            kept.push((
                runtime.root_value(tuple).unwrap(),
                zero,
                flag,
                record,
                index,
                bits,
            ));
        }
    }
    assert_eq!(
        runtime.collect_garbage().unwrap().live_objects,
        kept.len() * 5
    );
    for (_, _, flag, record, _, _) in &kept {
        runtime
            .gc()
            .struct_set_slot(*flag, &small, 0, Value::Bool(false))
            .unwrap();
        runtime
            .gc()
            .struct_set_slot(*record, &packed, 0, Value::I32(-128))
            .unwrap();
    }
    // Grow after compaction, reusing directory IDs while byte offsets remain private.
    for _ in 0..400 {
        runtime
            .alloc_struct(
                packed.clone(),
                vec![
                    Value::I32(127),
                    runtime.gc().alloc_string("dead".to_owned()).unwrap(),
                    Value::F64(-0.0),
                ],
            )
            .unwrap();
    }
    assert_eq!(
        runtime.collect_garbage().unwrap().live_objects,
        kept.len() * 5
    );
    for (_, zero, flag, record, index, bits) in &kept {
        assert!(runtime.gc().struct_snapshot(*zero).unwrap().1.is_empty());
        assert_eq!(
            runtime.gc().struct_get_slot(*flag, &small, 0),
            Some(Value::Bool(false))
        );
        assert_eq!(
            runtime.gc().struct_get_slot(*record, &packed, 0),
            Some(Value::I32(-128))
        );
        let Some(Value::Str(text)) = runtime.gc().struct_get_slot(*record, &packed, 1) else {
            panic!("retained string")
        };
        assert_eq!(&*runtime.gc().string(text).unwrap(), &index.to_string());
        let Some(Value::F64(value)) = runtime.gc().struct_get_slot(*record, &packed, 2) else {
            panic!("retained f64")
        };
        assert_eq!(value.to_bits(), *bits);
    }
    let stale = kept[0].3;
    drop(kept);
    assert_eq!(runtime.collect_garbage().unwrap().live_objects, 0);
    runtime.alloc_struct(empty, vec![]).unwrap();
    assert!(!runtime.gc().validate_value(&Value::Struct(stale)));
}
