#[cfg(test)]
use crate::test_support::bytecode::load_bytecode;
use crate::test_support::native::program::ProgramFixture;
use crate::{
    Runtime,
    error::{RuntimeErrorKind, reason::ErrorReason, trap::TrapReason},
    module::ExecutableModule,
    native::conversion::context::ConversionContext,
    value::{aggregates::structures::StructObject, carriers::RootedValue, view::ValueView},
};
use kagari_abi::representation::ValueType;
use kagari_bytecode::{instruction::StructId, program::ModuleRef};
use kagari_common::identity::map::DefinitionContext;
use kagari_types::{
    scalar::BuiltinType,
    ty::{NominalTy, Ty},
};
use std::slice;

use crate::test_support::layouts;

fn rooted_tuple<const N: usize>(
    runtime: &mut Runtime,
    owner: &ExecutableModule,
    values: &[&RootedValue; N],
) -> RootedValue {
    let mut source = ConversionContext::new(runtime, owner).unwrap();
    let values = (*values).map(|value| source.import(value).unwrap());
    let tuple = source.allocate_tuple(&values).unwrap();
    source.retain(&tuple).unwrap()
}

fn rooted_string(runtime: &mut Runtime, owner: &ExecutableModule, text: String) -> RootedValue {
    let mut source = ConversionContext::new(runtime, owner).unwrap();
    let value = source.allocate_string(text).unwrap();
    source.retain(&value).unwrap()
}

fn assert_field_target(
    runtime: &mut Runtime,
    object: &RootedValue,
    slot: usize,
    expected: &RootedValue,
) {
    runtime
        .memory
        .scope(&runtime.resources, |cx, _| {
            let object = cx.downcast::<StructObject>(object.get(cx)?.object().unwrap())?;
            let value = StructObject::get(cx, object, slot)?;
            assert!(
                value
                    .object()
                    .unwrap()
                    .ptr_eq(expected.get(cx)?.object().unwrap())
            );
            Ok(())
        })
        .unwrap();
}

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
        .alloc_struct(
            layout.clone(),
            &[RootedValue::from(1i32), RootedValue::from(true)],
        )
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
    let foreign_program = load_bytecode(
        &mut runtime,
        "other",
        ProgramFixture {
            exports: Default::default(),
            root: ModuleRef::new(0),
            modules: vec![foreign],
        }
        .metadata(context.snapshot(), &Default::default())
        .unwrap(),
    )
    .unwrap();
    let foreign = runtime
        .select_program(&foreign_program)
        .unwrap()
        .struct_layout(StructId::new(0))
        .unwrap();
    let mut source = ConversionContext::new(&mut runtime, layout.module()).unwrap();
    let value = source.import(&object).unwrap();
    assert_eq!(
        source.struct_field(&value, &foreign, 0).unwrap_err().kind(),
        RuntimeErrorKind::ModuleValidation
    );
    for error in [
        source
            .set_struct_field(&value, &foreign, 0, &ValueView::from(99i32))
            .unwrap_err(),
        source
            .set_struct_field(&value, &layout, 1, &ValueView::from(false))
            .unwrap_err(),
        source
            .set_struct_field(&value, &layout, 0, &ValueView::from(false))
            .unwrap_err(),
        source
            .set_struct_field(&value, &layout, usize::MAX, &ValueView::from(99i32))
            .unwrap_err(),
    ] {
        assert_eq!(error.kind(), RuntimeErrorKind::ModuleValidation);
    }
    assert_eq!(
        source.struct_field(&value, &layout, 0).unwrap().as_i32(),
        Some(1)
    );
    assert_eq!(
        source.struct_field(&value, &layout, 1).unwrap().as_bool(),
        Some(true)
    );
    drop(source);
    for field in ["fixed", "x"] {
        assert_eq!(
            runtime
                .reflect_set_field(&object, field, &RootedValue::from(false))
                .unwrap_err()
                .kind(),
            RuntimeErrorKind::InvalidReflectiveWrite
        );
    }
    let _ = runtime
        .reflect_set_field(&object, "x", &RootedValue::from(42i32))
        .unwrap();
    assert_eq!(
        runtime.reflect_get_field(&object, "x").unwrap().as_i32(),
        Some(42)
    );
    assert!(!runtime.is_quarantined());
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
    let before = runtime.gc_stats();
    assert_eq!(
        runtime
            .alloc_struct(foreign.clone(), &[RootedValue::from(7)])
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ModuleValidation,
    );
    for fields in [
        vec![],
        vec![RootedValue::from(false)],
        vec![RootedValue::from(1), RootedValue::from(2)],
    ] {
        let error = runtime.alloc_struct(layout.clone(), &fields).unwrap_err();
        assert_eq!(error.kind(), RuntimeErrorKind::ScriptTrap);
        assert!(matches!(
            error.reason(),
            ErrorReason::Trap(TrapReason::InvalidHeapTargetIndexOrPayload)
        ));
        assert_eq!(runtime.gc_stats().objects, before.objects);
        assert_eq!(runtime.gc_stats().provider_units, before.provider_units);
        assert_eq!(
            runtime.gc_stats().root_registrations,
            before.root_registrations
        );
    }
    let object = runtime
        .alloc_struct(layout.clone(), &[RootedValue::from(1)])
        .unwrap();
    assert_eq!(runtime.gc_stats().objects, before.objects + 1);
    let mut conversion = ConversionContext::new(&mut runtime, layout.module()).unwrap();
    let value = conversion.import(&object).unwrap();
    assert_eq!(
        conversion
            .struct_field(&value, &layout, 0)
            .unwrap()
            .as_i32(),
        Some(1)
    );
    drop((conversion, object));
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().objects, before.objects);
    assert_eq!(
        runtime.gc_stats().root_registrations,
        before.root_registrations
    );
    assert!(!runtime.is_quarantined());
}

#[test]
fn objects_retain_old_layouts_and_require_equal_schemas_across_generations() {
    let mut runtime = Runtime::default();
    let original = layouts::layout(
        &mut runtime,
        "Point",
        &[("x", Ty::Builtin(BuiltinType::I32), true)],
    );
    let program = runtime
        .load_verified_program("Point", original.module().verified_program().clone())
        .unwrap();
    let original = runtime
        .select_program(&program)
        .unwrap()
        .struct_layout(StructId::new(0))
        .unwrap();
    let object = runtime
        .alloc_struct(original.clone(), &[RootedValue::from(1)])
        .unwrap();
    let (definitions, bytecode) = original
        .module()
        .to_unverified(&Default::default())
        .unwrap()
        .into_parts();
    let next = runtime
        .stage_reload_program(
            &program,
            None,
            ProgramFixture {
                exports: Default::default(),
                root: ModuleRef::new(0),
                modules: vec![bytecode],
            }
            .metadata(definitions, &Default::default())
            .unwrap(),
        )
        .unwrap();
    runtime.publish_staged_reload(next).unwrap();
    let next = runtime.select_program(&program).unwrap();
    let compatible = next.struct_layout(StructId::new(0)).unwrap();
    assert_ne!(original.module().key(), compatible.module().key());
    {
        let mut conversion = ConversionContext::new(&mut runtime, &next).unwrap();
        let value = conversion.import(&object).unwrap();
        conversion
            .set_struct_field(&value, &compatible, 0, &ValueView::from(42))
            .unwrap();
    }
    let original_key = original.module().key();
    drop(original);
    runtime.collect_garbage().unwrap();
    assert!(runtime.modules().loaded(original_key).is_none());
    let retained = {
        let mut conversion = ConversionContext::new(&mut runtime, &next).unwrap();
        let value = conversion.import(&object).unwrap();
        let retained = conversion.struct_layout(&value).unwrap();
        assert_eq!(retained.module().key(), original_key);
        assert_eq!(
            conversion
                .struct_field(&value, &retained, 0)
                .unwrap()
                .as_i32(),
            Some(42)
        );
        retained
    };
    // Immutable nominal backing remains sufficient after the mutable revision is
    // gone. Creating plain data must not open an execution or resurrect a body.
    let fresh = runtime
        .alloc_struct(retained.clone(), &[RootedValue::from(3)])
        .unwrap();
    assert!(runtime.modules().loaded(original_key).is_none());
    let changed = layouts::layout(
        &mut runtime,
        "Point",
        &[("x", Ty::Builtin(BuiltinType::I32), false)],
    );
    {
        let mut conversion = ConversionContext::new(&mut runtime, &next).unwrap();
        let value = conversion.import(&object).unwrap();
        assert_eq!(
            conversion
                .struct_field(&value, &changed, 0)
                .unwrap_err()
                .kind(),
            RuntimeErrorKind::ModuleValidation
        );
        assert_eq!(
            conversion
                .set_struct_field(&value, &changed, 0, &ValueView::from(99))
                .unwrap_err()
                .kind(),
            RuntimeErrorKind::ModuleValidation
        );
        assert_eq!(
            conversion
                .struct_field(&value, &retained, 0)
                .unwrap()
                .as_i32(),
            Some(42)
        );
        let value = conversion.import(&fresh).unwrap();
        assert_eq!(
            conversion
                .struct_field(&value, &retained, 0)
                .unwrap()
                .as_i32(),
            Some(3)
        );
    }
    let roots = runtime.gc_stats().root_registrations;
    drop((object, fresh));
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().root_registrations, roots - 2);
    assert!(!runtime.is_quarantined());
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
    let wrong_value = runtime
        .alloc_struct(wrong, &[RootedValue::from(9i32)])
        .unwrap();
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
    let module_program = load_bytecode(
        &mut runtime,
        "concrete",
        ProgramFixture {
            exports: Default::default(),
            root: ModuleRef::new(0),
            modules: vec![bytecode],
        }
        .metadata(context.snapshot(), &Default::default())
        .unwrap(),
    )
    .unwrap();
    let module = runtime.select_program(&module_program).unwrap();
    let wrapper = module.struct_layout(StructId::new(0)).unwrap();
    let leaf = module.struct_layout(StructId::new(1)).unwrap();
    let valid = runtime
        .alloc_struct(leaf, &[RootedValue::from(42i32)])
        .unwrap();
    let Ty::Array(element) = &wrapper.layout().fields[0].ty else {
        panic!("array field")
    };
    let element = (**element).clone();
    let tuple = rooted_tuple(&mut runtime, &module, &[&valid, &RootedValue::from(true)]);
    let initial = runtime
        .alloc_array(&module, element.clone(), &[tuple])
        .unwrap();
    let target = runtime
        .alloc_struct(wrapper.clone(), slice::from_ref(&initial))
        .unwrap();
    let Ty::Tuple(mut wrong_member_type) = element.clone() else {
        panic!("tuple element")
    };
    wrong_member_type[1] = Ty::Builtin(BuiltinType::I32);
    let wrong_tuple = rooted_tuple(
        &mut runtime,
        &wrong_owner,
        &[&wrong_value, &RootedValue::from(true)],
    );
    let wrong_scalar = rooted_tuple(&mut runtime, &module, &[&valid, &RootedValue::from(1i32)]);
    for (owner, ty, values) in [
        (
            &wrong_owner,
            Ty::Tuple(vec![wrong_type, Ty::Builtin(BuiltinType::Bool)]),
            vec![wrong_tuple],
        ),
        (
            &module,
            Ty::Tuple(wrong_member_type.clone()),
            vec![wrong_scalar],
        ),
        (&module, Ty::Tuple(wrong_member_type), vec![]),
    ] {
        let invalid = runtime.alloc_array(owner, ty, &values).unwrap();
        let before = runtime.resources().counters();
        let heap = runtime.gc_stats();
        let error = runtime
            .alloc_struct(wrapper.clone(), slice::from_ref(&invalid))
            .unwrap_err();
        assert_eq!(error.kind(), RuntimeErrorKind::ScriptTrap);
        assert!(matches!(
            error.reason(),
            ErrorReason::Trap(TrapReason::InvalidHeapTargetIndexOrPayload)
        ));
        assert_eq!(
            runtime
                .reflect_set_field(&target, "items", &invalid)
                .unwrap_err()
                .kind(),
            RuntimeErrorKind::InvalidReflectiveWrite
        );
        assert_field_target(&mut runtime, &target, 0, &initial);
        assert_eq!(runtime.resources().counters(), before);
        assert_eq!(runtime.gc_stats(), heap);
    }
    let replacement = runtime.alloc_array(&module, element, &[]).unwrap();
    let _ = runtime
        .reflect_set_field(&target, "items", &replacement)
        .unwrap();
    assert_field_target(&mut runtime, &target, 0, &replacement);
    assert!(!runtime.is_quarantined());
}

#[test]
fn mixed_scalar_fields_preserve_full_domains_and_float_bits_at_value_boundaries() {
    // Expected tags/bits are independent constants, including NaN payloads and
    // sign extension; deriving them from the input codec would hide a codec bug.
    let cases = [
        (
            BuiltinType::Bool,
            RootedValue::from(true),
            (ValueType::Bool, 1),
        ),
        (BuiltinType::Unit, RootedValue::unit(), (ValueType::Unit, 0)),
        (
            BuiltinType::I8,
            RootedValue::from(-128i32),
            (ValueType::I32, 0xffff_ffff_ffff_ff80),
        ),
        (
            BuiltinType::I16,
            RootedValue::from(-32_768i32),
            (ValueType::I32, 0xffff_ffff_ffff_8000),
        ),
        (
            BuiltinType::I32,
            RootedValue::from(i32::MIN),
            (ValueType::I32, 0xffff_ffff_8000_0000),
        ),
        (
            BuiltinType::I64,
            RootedValue::from(i64::MIN),
            (ValueType::I64, 0x8000_0000_0000_0000),
        ),
        (
            BuiltinType::I64,
            RootedValue::from(i64::MAX),
            (ValueType::I64, 0x7fff_ffff_ffff_ffff),
        ),
        (
            BuiltinType::U8,
            RootedValue::from(255i64),
            (ValueType::I64, 0xff),
        ),
        (
            BuiltinType::U16,
            RootedValue::from(65_535i64),
            (ValueType::I64, 0xffff),
        ),
        (
            BuiltinType::U32,
            RootedValue::from(4_294_967_295i64),
            (ValueType::I64, 0xffff_ffff),
        ),
        (
            BuiltinType::U64,
            RootedValue::from(u64::MAX),
            (ValueType::U64, u64::MAX),
        ),
        (
            BuiltinType::F32,
            RootedValue::from(f32::from_bits(0x7fc1_2345)),
            (ValueType::F32, 0x7fc1_2345),
        ),
        (
            BuiltinType::F64,
            RootedValue::from(f64::from_bits(0x7ff8_1234_5678_9abc)),
            (ValueType::F64, 0x7ff8_1234_5678_9abc),
        ),
        (
            BuiltinType::Char,
            RootedValue::from('\0'),
            (ValueType::Char, 0),
        ),
        (
            BuiltinType::Char,
            RootedValue::from(char::MAX),
            (ValueType::Char, 0x10ffff),
        ),
    ];
    let names = (0..cases.len())
        .map(|slot| format!("field{slot}"))
        .collect::<Vec<_>>();
    let fields = cases
        .iter()
        .zip(&names)
        .map(|((ty, _, _), name)| (name.as_str(), Ty::Builtin(*ty), true))
        .collect::<Vec<_>>();
    let mut runtime = Runtime::default();
    let layout = layouts::layout(&mut runtime, "Scalars", &fields);
    runtime.collect_garbage().unwrap();
    let baseline = runtime.gc_stats();
    let counters = runtime.resources().counters();
    for (slot, invalid) in [
        (2, RootedValue::from(-129i32)),
        (2, RootedValue::from(128i32)),
        (3, RootedValue::from(-32_769i32)),
        (3, RootedValue::from(32_768i32)),
        (7, RootedValue::from(-1i64)),
        (7, RootedValue::from(256i64)),
        (8, RootedValue::from(-1i64)),
        (8, RootedValue::from(65_536i64)),
        (9, RootedValue::from(-1i64)),
        (9, RootedValue::from(4_294_967_296i64)),
        (9, RootedValue::from('a')),
        (13, RootedValue::from(0i64)),
        (14, RootedValue::from(0x10ffffi64)),
    ] {
        let mut initial = cases
            .iter()
            .map(|(_, value, _)| value.clone())
            .collect::<Vec<_>>();
        initial[slot] = invalid;
        let error = runtime.alloc_struct(layout.clone(), &initial).unwrap_err();
        assert_eq!(error.kind(), RuntimeErrorKind::ScriptTrap);
        assert!(matches!(
            error.reason(),
            ErrorReason::Trap(TrapReason::InvalidHeapTargetIndexOrPayload)
        ));
        assert_eq!(runtime.gc_stats(), baseline);
    }
    assert_eq!(runtime.resources().counters(), counters);
    let initial = cases
        .iter()
        .map(|(_, value, _)| value.clone())
        .collect::<Vec<_>>();
    let object = runtime.alloc_struct(layout.clone(), &initial).unwrap();
    runtime.collect_garbage().unwrap();
    let mut source = ConversionContext::new(&mut runtime, layout.module()).unwrap();
    let value = source.import(&object).unwrap();
    for (slot, (_, _, expected)) in cases.iter().enumerate() {
        assert_eq!(
            source
                .struct_field(&value, &layout, slot)
                .unwrap()
                .scalar_payload(),
            Some(*expected)
        );
    }
    source
        .set_struct_field(&value, &layout, 11, &ValueView::from(-0.0f32))
        .unwrap();
    source
        .set_struct_field(&value, &layout, 12, &ValueView::from(-0.0f64))
        .unwrap();
    let snapshot = source.snapshot_struct(&value, &layout).unwrap();
    assert_eq!(
        snapshot
            .element(&mut source, 11)
            .unwrap()
            .as_f32()
            .unwrap()
            .to_bits(),
        0x8000_0000
    );
    assert_eq!(
        snapshot
            .element(&mut source, 12)
            .unwrap()
            .as_f64()
            .unwrap()
            .to_bits(),
        0x8000_0000_0000_0000
    );
    assert_eq!(
        source
            .set_struct_field(&value, &layout, 2, &ValueView::from(128i32))
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ModuleValidation
    );
    assert_eq!(
        source.struct_field(&value, &layout, 2).unwrap().as_i32(),
        Some(-128)
    );
    assert_eq!(
        source
            .set_struct_field(&value, &layout, 13, &ValueView::from(97i64))
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ModuleValidation
    );
    source
        .set_struct_field(&value, &layout, 13, &ValueView::from('🙂'))
        .unwrap();
    assert_eq!(
        source.struct_field(&value, &layout, 13).unwrap().as_char(),
        Some('🙂')
    );
    drop(source);
    drop(object);
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().objects, baseline.objects);
    assert_eq!(
        runtime.gc_stats().root_registrations,
        baseline.root_registrations
    );
    assert!(!runtime.is_quarantined());
}

#[test]
fn mixed_layouts_keep_aliased_fields_across_collection_and_growth() {
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
    let owner = packed.module().clone();
    runtime.collect_garbage().unwrap();
    let baseline = runtime.gc_stats();
    let mut kept = Vec::new();
    for index in 0..200_i32 {
        let zero = runtime.alloc_struct(empty.clone(), &[]).unwrap();
        let flag = runtime
            .alloc_struct(small.clone(), &[RootedValue::from(true)])
            .unwrap();
        let text = rooted_string(&mut runtime, &owner, index.to_string());
        let bits = 0x7ff8_0000_0000_0000 + index as u64;
        let record = runtime
            .alloc_struct(
                packed.clone(),
                &[
                    RootedValue::from(index - 100),
                    text,
                    RootedValue::from(f64::from_bits(bits)),
                ],
            )
            .unwrap();
        if index % 17 == 0 {
            let tuple = rooted_tuple(&mut runtime, &owner, &[&zero, &flag, &record]);
            kept.push((tuple, index, bits));
        }
        // Only each retained tuple protects its three records and String child.
        // No detached IDs or extra child roots survive this iteration.
    }
    runtime.collect_garbage().unwrap();
    assert_eq!(
        runtime.gc_stats().objects,
        baseline.objects + kept.len() * 5
    );
    assert_eq!(
        runtime.gc_stats().root_registrations,
        baseline.root_registrations + kept.len()
    );
    for (tuple, _, _) in &kept {
        let mut source = ConversionContext::new(&mut runtime, &owner).unwrap();
        let tuple = source.import(tuple).unwrap();
        let flag = source.tuple_element(&tuple, 1).unwrap();
        let record = source.tuple_element(&tuple, 2).unwrap();
        source
            .set_struct_field(&flag, &small, 0, &ValueView::from(false))
            .unwrap();
        source
            .set_struct_field(&record, &packed, 0, &ValueView::from(-128i32))
            .unwrap();
    }
    // New dead allocations exercise growth/reuse while the tuple-owned graph
    // remains live. The nonmoving collector exposes no directory/offset identity.
    for _ in 0..400 {
        let text = rooted_string(&mut runtime, &owner, "dead".to_owned());
        let _ = runtime
            .alloc_struct(
                packed.clone(),
                &[RootedValue::from(127i32), text, RootedValue::from(-0.0f64)],
            )
            .unwrap();
    }
    runtime.collect_garbage().unwrap();
    assert_eq!(
        runtime.gc_stats().objects,
        baseline.objects + kept.len() * 5
    );
    for (tuple, index, bits) in &kept {
        let mut source = ConversionContext::new(&mut runtime, &owner).unwrap();
        let tuple = source.import(tuple).unwrap();
        assert_eq!(source.tuple_len(&tuple).unwrap(), 3);
        let zero = source.tuple_element(&tuple, 0).unwrap();
        assert!(
            source
                .struct_layout(&zero)
                .unwrap()
                .layout()
                .fields
                .is_empty()
        );
        let flag = source.tuple_element(&tuple, 1).unwrap();
        let record = source.tuple_element(&tuple, 2).unwrap();
        assert_eq!(
            source.struct_field(&flag, &small, 0).unwrap().as_bool(),
            Some(false)
        );
        assert_eq!(
            source.struct_field(&record, &packed, 0).unwrap().as_i32(),
            Some(-128)
        );
        let text = source.struct_field(&record, &packed, 1).unwrap();
        assert_eq!(
            source.read_string(&text, str::to_owned).unwrap(),
            index.to_string()
        );
        assert_eq!(
            source
                .struct_field(&record, &packed, 2)
                .unwrap()
                .as_f64()
                .unwrap()
                .to_bits(),
            *bits
        );
    }
    drop(kept);
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().objects, baseline.objects);
    assert_eq!(
        runtime.gc_stats().root_registrations,
        baseline.root_registrations
    );
    let fresh = runtime.alloc_struct(empty, &[]).unwrap();
    assert_eq!(runtime.gc_stats().objects, baseline.objects + 1);
    drop(fresh);
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().objects, baseline.objects);
    assert!(!runtime.is_quarantined());
}
