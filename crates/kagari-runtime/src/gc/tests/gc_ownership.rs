use crate::test_support as support;

use crate::{
    Runtime, RuntimeConfig,
    error::RuntimeErrorKind,
    host::HostCallContext,
    native::{builder::ModuleBuilder, declarations::FunctionDecl, host::HostCallback, types::Type},
    value::Value,
    value_semantics::{identity_equal, script_equal},
};
use kagari_types::{declaration::host::HostCallPolicy, scalar::BuiltinType, ty::Ty};
use layouts::allocation_owner;
use support::native::{invoke::invoke, program::free_function_program};

use crate::test_support::layouts;

#[test]
fn runtime_is_send() {
    fn assert_send<T: Send>() {}
    assert_send::<Runtime>();
}

#[test]
fn foreign_handles_and_wrong_value_tags_are_rejected_before_mutation_or_accounting() {
    let mut first = Runtime::default();
    let first_owner = allocation_owner(&mut first);
    let enum_owner = layouts::enum_owner(
        &mut first,
        vec![Ty::Array(Box::new(Ty::Builtin(BuiltinType::I32)))],
    );
    let mut second = Runtime::default();
    let second_owner = allocation_owner(&mut second);
    let own = first
        .alloc_array(
            &first_owner,
            Ty::Builtin(BuiltinType::I32),
            vec![Value::I32(1)],
        )
        .unwrap();
    let foreign = second
        .alloc_array(
            &second_owner,
            Ty::Builtin(BuiltinType::I32),
            vec![Value::I32(2)],
        )
        .unwrap();
    assert_eq!(own.index(), foreign.index());
    assert_ne!(own, foreign);
    let own_shape = layouts::layout(
        &mut first,
        "OwnedPoint",
        &[("value", Ty::Builtin(BuiltinType::I32), true)],
    );
    let foreign_shape = layouts::layout(
        &mut second,
        "ForeignPoint",
        &[("value", Ty::Builtin(BuiltinType::I32), true)],
    );
    let own_struct = first
        .gc()
        .alloc_struct(own_shape.clone(), vec![Value::I32(1)])
        .unwrap();
    let foreign_struct = second
        .gc()
        .alloc_struct(foreign_shape, vec![Value::I32(2)])
        .unwrap();
    let _own_root = first.root_value(Value::Struct(own_struct)).unwrap();
    let _foreign_root = second.root_value(Value::Struct(foreign_struct)).unwrap();
    let before = first.gc().stats();
    assert!(first.root_value(Value::Struct(foreign_struct)).is_none());
    assert!(
        first
            .gc()
            .struct_get_slot(foreign_struct, &own_shape, 0)
            .is_none()
    );
    assert!(
        first
            .gc()
            .struct_set_slot(foreign_struct, &own_shape, 0, Value::I32(3))
            .is_err()
    );
    assert_eq!(
        first.gc().struct_get_slot(own_struct, &own_shape, 0),
        Some(Value::I32(1))
    );
    assert!(first.gc().array_get(foreign, 0).is_none());
    assert!(first.gc().array_set(foreign, 0, Value::I32(3)).is_err());
    assert!(first.gc().array_set(own, 0, Value::Array(foreign)).is_err());
    assert!(
        first
            .gc()
            .array_set(
                own,
                0,
                second
                    .gc()
                    .alloc_tuple(vec![Value::Array(foreign)])
                    .unwrap()
            )
            .is_err()
    );
    assert!(
        first
            .alloc_enum(
                crate::value::EnumTag::Declared(
                    enum_owner
                        .enum_variant(kagari_bytecode::instruction::EnumId::new(0), 0)
                        .unwrap()
                ),
                vec![Value::Array(foreign)]
            )
            .is_err()
    );
    assert!(first.root_value(Value::Array(foreign)).is_none());
    assert!(first.root_value(Value::Enum(own)).is_none());

    assert_eq!(
        first
            .alloc_array(
                &first_owner,
                Ty::Array(Box::new(Ty::Builtin(BuiltinType::I32))),
                vec![Value::Array(foreign)]
            )
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ScriptTrap
    );

    assert_eq!(first.gc().stats(), before);
    assert_eq!(first.gc().array_snapshot(own), Some(vec![Value::I32(1)]));
    assert!(script_equal(first.gc(), &Value::Array(foreign), &Value::Array(foreign)).is_err());
}

#[test]
fn rooted_clones_keep_values_alive_and_reused_slots_reject_stale_handles() {
    let mut runtime = Runtime::default();
    let runtime_owner = allocation_owner(&mut runtime);
    let object = runtime
        .alloc_array(
            &runtime_owner,
            Ty::Builtin(BuiltinType::I32),
            vec![Value::I32(42)],
        )
        .unwrap();
    let naked_copy = Value::Array(object);
    let root = runtime.root_value(naked_copy).unwrap();
    let retained = root.clone();
    drop(root);
    assert_eq!(runtime.collect_garbage().unwrap().reclaimed_objects, 0);
    assert_eq!(runtime.gc().array_get(object, 0), Some(Value::I32(42)));
    assert_eq!(runtime.gc().active_roots(), 1);
    drop(retained);
    let collected = runtime.collect_garbage().unwrap();
    assert_eq!(
        (collected.reclaimed_objects, collected.reclaimed_units),
        (1, 2)
    );
    assert!(runtime.gc().array_len(object).is_none());
    assert!(runtime.root_value(naked_copy).is_none());
    let next = runtime
        .alloc_array(&runtime_owner, Ty::Builtin(BuiltinType::I32), vec![])
        .unwrap();
    assert_eq!(next.index(), object.index());
    assert!(next.generation() > object.generation());
    let before = runtime.gc().stats();
    assert!(runtime.gc().array_set(object, 0, Value::I32(7)).is_err());
    assert_eq!(runtime.gc().stats(), before);
    assert!(script_equal(runtime.gc(), &Value::Array(object), &Value::Array(object)).is_err());

    runtime.collect_garbage().unwrap();
    for nested in [false, true] {
        let text = runtime.gc().alloc_string("shared".into()).unwrap();
        let Value::Str(text_id) = text else {
            unreachable!()
        };
        let value = if nested {
            runtime
                .gc()
                .alloc_tuple(vec![text, Value::I32(42)])
                .unwrap()
        } else {
            text
        };
        let raw_copy = value;
        let root = runtime.root_value(value).unwrap();
        let retained = root.clone();
        drop(root);
        assert_eq!(runtime.collect_garbage().unwrap().reclaimed_objects, 0);
        assert_eq!(retained.value(runtime.gc()), Some(raw_copy));
        assert_eq!(&*runtime.gc().string(text_id).unwrap(), "shared");
        drop(retained);
        let collected = runtime.collect_garbage().unwrap();
        assert_eq!(collected.reclaimed_objects, if nested { 2 } else { 1 });
        assert_eq!(collected.reclaimed_units, if nested { 10 } else { 7 });
        assert!(runtime.gc().string(text_id).is_none());
        assert!(runtime.root_value(raw_copy).is_none());
        assert!(script_equal(runtime.gc(), &raw_copy, &raw_copy).is_err());
    }
}

#[test]
fn roots_reject_foreign_replacement_and_execution_root_sets_release_on_drop() {
    let mut runtime = Runtime::default();
    let runtime_owner = allocation_owner(&mut runtime);
    let mut foreign = Runtime::default();
    let foreign_owner = allocation_owner(&mut foreign);
    let array = runtime
        .alloc_array(&runtime_owner, Ty::Builtin(BuiltinType::I32), vec![])
        .unwrap();
    let root = runtime.root_value(Value::Array(array)).unwrap();
    let foreign_unit = foreign.root_value(Value::Unit).unwrap();
    assert!(root.replace(foreign.gc(), &foreign_unit).is_none());
    let other = foreign
        .alloc_array(&foreign_owner, Ty::Builtin(BuiltinType::I32), vec![])
        .unwrap();
    let foreign_root = foreign.root_value(Value::Array(other)).unwrap();
    assert!(root.replace(runtime.gc(), &foreign_root).is_none());
    let slots = runtime
        .gc()
        .root_execution_values(vec![root.value(runtime.gc()).unwrap()])
        .unwrap();
    drop(root);
    assert_eq!(runtime.collect_garbage().unwrap().live_objects, 1);
    assert!(slots.replace(foreign.gc(), 0, &foreign_unit).is_none());
    assert!(slots.replace(runtime.gc(), 0, &foreign_root).is_none());
    let unit = runtime.root_value(Value::Unit).unwrap();
    assert!(slots.replace(runtime.gc(), 1, &unit).is_none());
    slots.replace(runtime.gc(), 0, &unit).unwrap();
    drop(unit);
    assert_eq!(runtime.collect_garbage().unwrap().reclaimed_objects, 1);
    drop(slots);
    assert_eq!(runtime.gc().active_roots(), 0);
}

#[test]
fn shared_host_callbacks_can_retain_checked_root_leases() {
    let mut runtime = Runtime::new(RuntimeConfig {
        ..Default::default()
    });
    let runtime_owner = allocation_owner(&mut runtime);
    let object = runtime
        .alloc_array(
            &runtime_owner,
            Ty::Builtin(BuiltinType::I32),
            vec![Value::I32(7)],
        )
        .unwrap();
    let retained = runtime.root_value(Value::Array(object)).unwrap();
    let mut builder = ModuleBuilder::new("host", &Default::default()).unwrap();
    let function = builder
        .define_function(
            FunctionDecl::new("retained")
                .returns(Type::bool())
                .host_policy(HostCallPolicy {
                    passing: vec![],
                    effects: Default::default(),
                }),
        )
        .unwrap();
    builder
        .bind(function, unsafe {
            // SAFETY: The host frame protects inputs; escaped managed values are retained and results transfer immediately.
            HostCallback::new(move |cx: &HostCallContext<'_>, _: &[Value]| {
                Ok(Value::Bool(
                    retained.value(cx.runtime().gc()).unwrap() == Value::Array(object),
                ))
            })
        })
        .unwrap();
    let module = builder.finish().unwrap();
    let program = free_function_program(&module.to_declaration());
    module.install(&mut runtime).unwrap();
    let loaded_program = runtime.load_program("retained", program).unwrap();
    let loaded = runtime.select_program(&loaded_program).unwrap();
    assert_eq!(runtime.collect_garbage().unwrap().live_objects, 1);
    assert_eq!(
        invoke(&runtime, &loaded, 0, &[]).unwrap(),
        Value::Bool(true)
    );
    assert_eq!(runtime.gc().active_roots(), 1);
    assert_eq!(runtime.gc().array_get(object, 0), Some(Value::I32(7)));
}

#[test]
fn identity_comparison_rejects_foreign_stale_and_disguised_handles() {
    let mut first = Runtime::default();
    let first_owner = allocation_owner(&mut first);
    let mut second = Runtime::default();
    let second_owner = allocation_owner(&mut second);
    let a = first
        .alloc_array(&first_owner, Ty::Builtin(BuiltinType::I32), vec![])
        .unwrap();
    let b = second
        .alloc_array(&second_owner, Ty::Builtin(BuiltinType::I32), vec![])
        .unwrap();
    assert!(identity_equal(first.gc(), &Value::Array(a), &Value::Array(a)).unwrap());
    assert!(identity_equal(first.gc(), &Value::Array(a), &Value::Array(b)).is_err());
    assert!(identity_equal(first.gc(), &Value::Array(a), &Value::GcHandle(a)).is_err());
    assert!(identity_equal(first.gc(), &Value::I32(1), &Value::I32(1)).is_err());
    first.collect_garbage().unwrap();
    assert!(identity_equal(first.gc(), &Value::Array(a), &Value::Array(a)).is_err());
}
