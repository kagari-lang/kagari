use crate::{
    Runtime,
    error::{RuntimeErrorKind, trap::TrapReason},
    module::{EnumVariantRef, ExecutableModule},
    native::storage_type::StorageType,
    test_support::{bytecode::load_bytecode, native::program::ProgramFixture},
    value::{
        aggregates::{enumerations::EnumObject, tuples::TupleObject},
        arrays::{ArrayObject, storage::ArrayValues},
        carriers::{RootedValue, ScopedValue},
        strings::StringObject,
    },
    value_semantics::{builtin_order, format_value, script_equal},
};
use kagari_bytecode::{instruction::EnumId, module::BytecodeModule, program::ModuleRef};
use kagari_common::identity::{
    DefinitionKind, ModuleIdentity, map::DefinitionContext, metadata::DefinitionMetadata,
};
use kagari_contract::layout::{EnumLayout, EnumVariantLayout};
use kagari_gc::{access::Scope, error::GcError};
use kagari_types::{scalar::BuiltinType, ty::Ty};
use std::{cmp::Ordering, sync::Arc};

fn allocate(
    runtime: &mut Runtime,
    make: impl for<'gc> FnOnce(&mut Scope<'gc>) -> Result<ScopedValue<'gc>, GcError>,
) -> RootedValue {
    runtime
        .memory
        .scope(&runtime.resources, |cx, _| {
            let value = make(cx)?;
            value.retain(cx)
        })
        .unwrap()
}

fn enumeration(runtime: &mut Runtime, layout: EnumVariantRef, field: &RootedValue) -> RootedValue {
    allocate(runtime, |cx| {
        let field = field.get(cx)?;
        EnumObject::alloc(cx, layout, &[field], None).map(ScopedValue::managed)
    })
}

fn array(runtime: &mut Runtime, owner: &ExecutableModule) -> RootedValue {
    let ty = Ty::Builtin(BuiltinType::I32);
    let contract = Arc::new(StorageType::prepare(ty.clone(), owner).unwrap());
    allocate(runtime, |cx| {
        let mut values = ArrayValues::empty(&ty);
        values.push(cx, ScopedValue::from(3_i32))?;
        ArrayObject::alloc(cx, contract, values, Default::default()).map(ScopedValue::managed)
    })
}

#[test]
fn char_equality_order_and_formatting_preserve_scalar_semantics() {
    let mut runtime = Runtime::default();
    for ch in ['\0', '\n', '\t', '\'', '\\', 'é', '🙂', '\u{301}'] {
        let value = RootedValue::from(ch);
        assert!(script_equal(&mut runtime, &value, &value).unwrap());
        assert!(
            !script_equal(
                &mut runtime,
                &value,
                &RootedValue::from(i64::from(u32::from(ch)))
            )
            .unwrap()
        );
        assert_eq!(
            format_value(&mut runtime, &value, false).unwrap(),
            ch.to_string()
        );
        assert_eq!(
            format_value(&mut runtime, &value, true).unwrap(),
            format!("{ch:?}")
        );
    }
    assert_eq!(
        format_value(&mut runtime, &RootedValue::from('\0'), true).unwrap(),
        "'\\0'"
    );
    assert_eq!(
        format_value(&mut runtime, &RootedValue::from('\''), true).unwrap(),
        "'\\''"
    );
    assert_eq!(
        builtin_order(
            &mut runtime,
            &RootedValue::from('é'),
            &RootedValue::from('🙂')
        )
        .unwrap(),
        Some(Ordering::Less)
    );
    assert!(
        builtin_order(
            &mut runtime,
            &RootedValue::from('a'),
            &RootedValue::from(97_i64)
        )
        .is_err()
    );
}

#[test]
fn declared_enum_equality_keeps_nominal_identity_across_private_layout_edits() {
    let context = DefinitionContext::new().unwrap();
    let identity = ModuleIdentity::single_file("enum-equality.kgr");
    let root = context.intern_root(&identity).unwrap();
    let declaration = context
        .intern_child(root, DefinitionKind::Enum, "Event", 0)
        .unwrap();
    let other_declaration = context
        .intern_child(root, DefinitionKind::Enum, "OtherEvent", 0)
        .unwrap();
    let variant = |name: &str, payload| EnumVariantLayout {
        reports_failure: false,
        declaration: context
            .intern_child(declaration, DefinitionKind::Variant, name, 0)
            .unwrap(),
        payload,
    };
    let program = |extra_variant: bool| {
        let program = ProgramFixture {
            exports: Default::default(),
            root: ModuleRef::new(0),
            modules: vec![BytecodeModule {
                identity: identity.clone(),
                enumerations: vec![
                    EnumLayout {
                        declaration,
                        arguments: Vec::new(),
                        variants: if extra_variant {
                            vec![
                                variant("Added", Vec::new()),
                                variant("Data", vec![Ty::Builtin(BuiltinType::I32)]),
                            ]
                        } else {
                            vec![variant("Data", vec![Ty::Builtin(BuiltinType::I32)])]
                        },
                    },
                    EnumLayout {
                        declaration: other_declaration,
                        arguments: Vec::new(),
                        variants: vec![EnumVariantLayout {
                            reports_failure: false,
                            declaration: context
                                .intern_child(other_declaration, DefinitionKind::Variant, "Data", 0)
                                .unwrap(),
                            payload: vec![Ty::Builtin(BuiltinType::I32)],
                        }],
                    },
                ],
                ..Default::default()
            }],
        }
        .build(&context);
        DefinitionMetadata::checked(context.snapshot(), program, &Default::default()).unwrap()
    };

    let mut runtime = Runtime::default();
    let installation = load_bytecode(&mut runtime, "enum-equality", program(false)).unwrap();
    let old = runtime.select_program(&installation).unwrap();
    let old_value = enumeration(
        &mut runtime,
        old.enum_variant(EnumId::new(0), 0).unwrap(),
        &RootedValue::from(7_i32),
    );
    let candidate = runtime
        .stage_reload_program(&installation, None, program(true))
        .unwrap();
    runtime.publish_staged_reload(candidate).unwrap();
    let new = runtime.select_program(&installation).unwrap();
    let new_value = enumeration(
        &mut runtime,
        new.enum_variant(EnumId::new(0), 1).unwrap(),
        &RootedValue::from(7_i32),
    );
    runtime.memory.collect(&runtime.resources).unwrap();
    assert!(script_equal(&mut runtime, &old_value, &new_value).unwrap());
    let other = enumeration(
        &mut runtime,
        new.enum_variant(EnumId::new(1), 0).unwrap(),
        &RootedValue::from(7_i32),
    );
    assert!(!script_equal(&mut runtime, &old_value, &other).unwrap());
    let independent = load_bytecode(&mut runtime, "enum-equality", program(true)).unwrap();
    let independent = runtime.select_program(&independent).unwrap();
    let independent_value = enumeration(
        &mut runtime,
        independent.enum_variant(EnumId::new(0), 1).unwrap(),
        &RootedValue::from(7_i32),
    );
    assert!(!script_equal(&mut runtime, &old_value, &independent_value).unwrap());
    assert!(!script_equal(&mut runtime, &new_value, &independent_value).unwrap());
}

#[test]
fn enum_members_preserve_nan_and_reject_implicit_object_comparison() {
    let mut runtime = Runtime::default();
    let interface = crate::test_support::layouts::interface_value(&mut runtime);
    let owner = crate::test_support::layouts::allocation_owner(&mut runtime);
    let enums = crate::test_support::layouts::enum_owner(
        &mut runtime,
        vec![
            Ty::Builtin(BuiltinType::I32),
            Ty::Array(Box::new(Ty::Builtin(BuiltinType::I32))),
            Ty::Builtin(BuiltinType::F64),
        ],
    );
    let make = |runtime: &mut Runtime, slot, value: &RootedValue| {
        enumeration(
            runtime,
            enums.enum_variant(EnumId::new(slot), 0).unwrap(),
            value,
        )
    };
    let a = make(&mut runtime, 0, &RootedValue::from(3_i32));
    let b = make(&mut runtime, 0, &RootedValue::from(3_i32));
    runtime
        .memory
        .scope(&runtime.resources, |cx, _| {
            assert!(
                !a.get(cx)?
                    .object()
                    .unwrap()
                    .ptr_eq(b.get(cx)?.object().unwrap()),
                "distinct enum allocations must compare by content"
            );
            Ok::<_, GcError>(())
        })
        .unwrap();
    assert!(script_equal(&mut runtime, &a, &b).unwrap());
    let value = array(&mut runtime, &owner);
    let first = make(&mut runtime, 1, &value);
    let second = make(&mut runtime, 1, &value);
    let error = script_equal(&mut runtime, &first, &second).unwrap_err();
    assert_eq!(error.kind(), RuntimeErrorKind::ScriptTrap);
    assert_eq!(
        error.to_string(),
        TrapReason::ValueCategoryDoesNotSupportGeneralEquality.to_string()
    );
    let value = array(&mut runtime, &owner);
    let first = make(&mut runtime, 1, &value);
    let value = array(&mut runtime, &owner);
    let second = make(&mut runtime, 1, &value);
    assert!(script_equal(&mut runtime, &first, &second).is_err());
    let nan = make(&mut runtime, 2, &RootedValue::from(f64::NAN));
    assert!(!script_equal(&mut runtime, &nan, &nan).unwrap());
    // Interface boxing does not introduce builtin equality.
    assert!(script_equal(&mut runtime, &interface, &interface).is_err());
}

#[test]
fn deep_comparison_keeps_short_circuit_order_without_native_recursion() {
    let mut runtime = Runtime::default();
    let owner = crate::test_support::layouts::allocation_owner(&mut runtime);
    let make = |runtime: &mut Runtime, leaf| {
        allocate(runtime, |cx| {
            let mut value = ScopedValue::from(leaf);
            for _ in 0..4096 {
                value = ScopedValue::managed(TupleObject::alloc(
                    cx,
                    &[value, ScopedValue::from(0_i32)],
                )?);
            }
            Ok(value)
        })
    };
    let a = make(&mut runtime, 1_i32);
    let b = make(&mut runtime, 1_i32);
    let different = make(&mut runtime, 2_i32);
    runtime.memory.collect(&runtime.resources).unwrap();
    assert!(script_equal(&mut runtime, &a, &b).unwrap());
    let object = array(&mut runtime, &owner);
    let a = allocate(&mut runtime, |cx| {
        let fields = [a.get(cx)?, object.get(cx)?];
        TupleObject::alloc(cx, &fields).map(ScopedValue::managed)
    });
    let b = allocate(&mut runtime, |cx| {
        let fields = [different.get(cx)?, object.get(cx)?];
        TupleObject::alloc(cx, &fields).map(ScopedValue::managed)
    });
    assert!(
        !script_equal(&mut runtime, &a, &b).unwrap(),
        "a preceding unequal member must prevent implicit comparison of a later object"
    );
    let error = format_value(&mut runtime, &a, true).unwrap_err();
    assert_eq!(
        error.to_string(),
        TrapReason::UnsupportedFormatValueInvalidHandleOrFormattingLimitExceeded.to_string()
    );
}

#[test]
fn string_formatting_escapes_debug_text_and_bounds_output_bytes() {
    let mut runtime = Runtime::default();
    let text = allocate(&mut runtime, |cx| {
        StringObject::alloc(cx, "a\nb".into()).map(ScopedValue::managed)
    });
    assert_eq!(
        format_value(&mut runtime, &text, true).unwrap(),
        "\"a\\nb\""
    );
    let full = "x".repeat(1_048_576);
    let text = allocate(&mut runtime, |cx| {
        StringObject::alloc(cx, full.clone()).map(ScopedValue::managed)
    });
    assert_eq!(format_value(&mut runtime, &text, false).unwrap(), full);
    // Debug quotes also count toward the same output limit.
    let debug = format_value(&mut runtime, &text, true).unwrap_err();
    assert_eq!(
        debug.to_string(),
        TrapReason::UnsupportedFormatValueInvalidHandleOrFormattingLimitExceeded.to_string()
    );
    let oversized = allocate(&mut runtime, |cx| {
        StringObject::alloc(cx, "x".repeat(1_048_577)).map(ScopedValue::managed)
    });
    let error = format_value(&mut runtime, &oversized, false).unwrap_err();
    assert_eq!(
        error.to_string(),
        TrapReason::UnsupportedFormatValueInvalidHandleOrFormattingLimitExceeded.to_string()
    );
}
