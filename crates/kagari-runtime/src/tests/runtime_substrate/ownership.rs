//! Runtime ownership boundaries use genuine roots and the actual execution source.
use crate::{
    Runtime, RuntimeConfig,
    error::RuntimeErrorKind,
    execution::memory::GcHeapConfig,
    host::HostError,
    native::{
        builder::ModuleBuilder, conversion::context::ConversionContext, declarations::FunctionDecl,
        host::callback, types::Type,
    },
    test_support::{
        bytecode::load_bytecode,
        layouts,
        native::{invoke::invoke, program::free_function_program},
        update_programs::load_empty,
    },
    value::{
        EnumTag,
        arrays::ArrayObject,
        carriers::{RootedValue, ScopedValue},
        interfaces::InterfaceObject,
        scalar,
        strings::StringObject,
        view::ValueView,
    },
    value_semantics::{identity_equal, script_equal},
};
use kagari_bytecode::instruction::EnumId;
use kagari_types::{declaration::host::HostCallPolicy, scalar::BuiltinType, ty::Ty};
use std::{slice, thread};

#[test]
fn runtime_and_host_roots_can_transfer_between_threads() {
    fn movable<T: Send>() {}

    fn shareable<T: Send + Sync>() {}
    movable::<Runtime>();
    shareable::<RootedValue>();
    let mut runtime = Runtime::default();
    let program = load_empty(&mut runtime, "root-clones");
    let owner = runtime.select_program(&program).unwrap();
    runtime.collect_garbage().unwrap();
    let baseline = runtime.gc_stats();
    let root = runtime
        .alloc_array(&owner, Ty::Builtin(BuiltinType::I32), &[42i32.into()])
        .unwrap();
    let retained = root.clone();
    drop(root);
    let retained = thread::spawn(move || retained).join().unwrap();
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().objects, baseline.objects + 1);
    assert_eq!(
        runtime.gc_stats().root_registrations,
        baseline.root_registrations + 1
    );
    let mut cx = ConversionContext::new(&mut runtime, &owner).unwrap();
    let value = cx.import(&retained).unwrap();
    assert_eq!(
        cx.array_element(&value, 0).unwrap().unwrap().as_i32(),
        Some(42)
    );
    drop(cx);
    let final_lease = retained.clone();
    drop(retained);
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().objects, baseline.objects + 1);
    thread::spawn(move || drop(final_lease)).join().unwrap();
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().objects, baseline.objects);
    assert_eq!(
        runtime.gc_stats().root_registrations,
        baseline.root_registrations
    );
    let orphan = runtime
        .alloc_array(&owner, Ty::Builtin(BuiltinType::I32), &[])
        .unwrap();
    drop(runtime);
    thread::spawn(move || drop(orphan)).join().unwrap();
}

#[test]
fn foreign_values_and_invalid_payloads_preserve_targets_and_accounting() {
    let mut first = Runtime::default();
    let first_program = load_empty(&mut first, "first");
    let owner = first.select_program(&first_program).unwrap();
    let enumeration = layouts::enum_owner(
        &mut first,
        vec![Ty::Array(Box::new(Ty::Builtin(BuiltinType::I32)))],
    );
    let variant = enumeration.enum_variant(EnumId::new(0), 0).unwrap();
    let own = first
        .alloc_array(&owner, Ty::Builtin(BuiltinType::I32), &[1i32.into()])
        .unwrap();
    let mut second = Runtime::default();
    let second_program = load_empty(&mut second, "second");
    let second_owner = second.select_program(&second_program).unwrap();
    let foreign = second
        .alloc_array(&second_owner, Ty::Builtin(BuiltinType::I32), &[2i32.into()])
        .unwrap();
    first.collect_garbage().unwrap();
    let before = first.gc_stats();
    assert!(identity_equal(&mut first, &own, &own).unwrap());
    for error in [
        identity_equal(&mut first, &own, &foreign).unwrap_err(),
        script_equal(&mut first, &foreign, &foreign).unwrap_err(),
    ] {
        assert_eq!(error.kind(), RuntimeErrorKind::ModuleValidation);
    }
    assert_eq!(
        identity_equal(&mut first, &1i32.into(), &1i32.into())
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ScriptTrap
    );
    assert_eq!(
        first
            .alloc_enum(EnumTag::Declared(variant), slice::from_ref(&foreign))
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ScriptTrap
    );
    {
        let mut cx = ConversionContext::new(&mut first, &owner).unwrap();
        assert_eq!(
            cx.import(&foreign).unwrap_err().kind(),
            RuntimeErrorKind::ModuleValidation
        );
        let receiver = cx.import(&own).unwrap();
        assert_eq!(
            cx.array_len(&ValueView::from(3i32)).unwrap_err().kind(),
            RuntimeErrorKind::ScriptTrap
        );
        assert_eq!(
            cx.array_set(&receiver, 0, &ValueView::from(true))
                .unwrap_err()
                .kind(),
            RuntimeErrorKind::ScriptTrap
        );
        for index in [2u64, 1u64 << 32, u64::MAX] {
            assert_eq!(
                cx.array_set(&receiver, index, &9i32.into())
                    .unwrap_err()
                    .kind(),
                RuntimeErrorKind::IndexOutOfBounds
            );
            assert!(cx.array_element(&receiver, index).unwrap().is_none());
        }
        let unrepresentable = usize::MAX as u128 + 1;
        assert_eq!(
            cx.array_set(&receiver, unrepresentable, &9i32.into())
                .unwrap_err()
                .kind(),
            RuntimeErrorKind::IndexOutOfBounds
        );
        assert!(
            cx.array_element(&receiver, unrepresentable)
                .unwrap()
                .is_none()
        );
        assert_eq!(
            cx.array_set(&receiver, unrepresentable, &true.into())
                .unwrap_err()
                .kind(),
            RuntimeErrorKind::ScriptTrap
        );
        assert_eq!(
            cx.array_element(&receiver, 0).unwrap().unwrap().as_i32(),
            Some(1)
        );
    }
    first.collect_garbage().unwrap();
    let after = first.gc_stats();
    assert_eq!(after.objects, before.objects);
    assert_eq!(after.root_registrations, before.root_registrations);
    assert_eq!(after.indirect_bytes, before.indirect_bytes);
    assert!(!first.is_quarantined());
    assert!(!second.is_quarantined());
}

#[test]
fn host_temporary_sources_retain_values_until_scope_retirement() {
    let mut runtime = Runtime::default();
    let program = load_empty(&mut runtime, "temporary-source");
    let owner = runtime.select_program(&program).unwrap();
    let mut foreign = Runtime::default();
    let foreign_program = load_empty(&mut foreign, "foreign");
    let foreign_owner = foreign.select_program(&foreign_program).unwrap();
    let other = foreign
        .alloc_array(&foreign_owner, Ty::Builtin(BuiltinType::I32), &[])
        .unwrap();
    runtime.collect_garbage().unwrap();
    let baseline = runtime.gc_stats();
    let array = runtime
        .alloc_array(&owner, Ty::Builtin(BuiltinType::I32), &[7i32.into()])
        .unwrap();
    let mut scope = runtime.host_scope(slice::from_ref(&array)).unwrap();
    let value = scope.import(&array).unwrap();
    drop(array);
    assert_eq!(
        scope.import(&other).unwrap_err().kind(),
        RuntimeErrorKind::ModuleValidation
    );
    scope.runtime_mut().collect_garbage().unwrap();
    assert_eq!(scope.runtime().gc_stats().objects, baseline.objects + 1);
    assert_eq!(
        scope.runtime().gc_stats().root_registrations,
        baseline.root_registrations
    );
    let escaped = scope.retain(&value).unwrap();
    drop(scope);
    runtime.collect_garbage().unwrap();
    let mut cx = ConversionContext::new(&mut runtime, &owner).unwrap();
    let value = cx.import(&escaped).unwrap();
    assert_eq!(
        cx.array_element(&value, 0).unwrap().unwrap().as_i32(),
        Some(7)
    );
    drop(cx);
    drop(escaped);
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().objects, baseline.objects);
    assert_eq!(
        runtime.gc_stats().root_registrations,
        baseline.root_registrations
    );
    assert!(runtime.resources().active_session().is_none());
    assert_eq!(runtime.resources().counters().current_call_depth, 0);
}

#[test]
fn actual_host_callbacks_can_retain_checked_root_leases() {
    let mut runtime = Runtime::default();
    let program = load_empty(&mut runtime, "captured-root");
    let owner = runtime.select_program(&program).unwrap();
    let object = runtime
        .alloc_array(&owner, Ty::Builtin(BuiltinType::I32), &[7i32.into()])
        .unwrap();
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
        .bind(
            function,
            callback(move |cx, _| {
                let value = cx
                    .import(&object)
                    .map_err(|error| HostError::new(error.to_string()))?;
                let retained = cx
                    .retain(&value)
                    .map_err(|error| HostError::new(error.to_string()))?;
                let identical = identity_equal(cx.runtime_mut(), &object, &retained)
                    .map_err(|error| HostError::new(error.to_string()))?;
                Ok(ValueView::from(identical))
            }),
        )
        .unwrap();
    let module = builder.finish().unwrap();
    let input = free_function_program(&module.to_declaration());
    module.install(&mut runtime).unwrap();
    let program = load_bytecode(&mut runtime, "retained", input).unwrap();
    let owner = runtime.select_program(&program).unwrap();
    runtime.collect_garbage().unwrap();
    let before = runtime.gc_stats();
    assert_eq!(
        invoke(&mut runtime, &owner, 0, &[]).unwrap().as_bool(),
        Some(true)
    );
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().objects, before.objects);
    assert_eq!(
        runtime.gc_stats().root_registrations,
        before.root_registrations
    );
    assert!(runtime.resources().active_session().is_none());
    assert_eq!(runtime.resources().counters().current_call_depth, 0);
}

#[test]
fn reserved_empty_strings_use_byte_pressure_only_when_collection_is_enabled() {
    for enabled in [false, true] {
        let mut runtime = Runtime::new(RuntimeConfig {
            gc: GcHeapConfig {
                collection_threshold: enabled.then_some(usize::MAX),
                retained_byte_threshold: 128,
            },
            ..Default::default()
        });
        let root = runtime
            .memory
            .scope(&runtime.resources, |cx, _| {
                let text = StringObject::alloc(cx, String::with_capacity(256))?;
                ScopedValue::managed(text).retain(cx)
            })
            .unwrap();
        let allocated = runtime.gc_stats();
        assert_eq!(allocated.objects, 1);
        assert_eq!(allocated.provider_units, 0);
        assert!(allocated.indirect_bytes >= 256);
        assert_eq!(runtime.memory.service_requested(false), enabled);
        runtime.gc_safepoint().unwrap();
        let mut steps = 1;
        while runtime.memory.service_requested(false) {
            assert!(steps < 16, "byte-pressure collection stopped progressing");
            runtime.gc_safepoint().unwrap();
            steps += 1;
        }
        assert_eq!(
            runtime.gc_stats().completed_cycles > allocated.completed_cycles,
            enabled
        );
        runtime
            .memory
            .scope(&runtime.resources, |cx, _| {
                let text = cx.downcast::<StringObject>(root.get(cx)?.object().unwrap())?;
                assert!(cx.read(text, |text| text.as_str().is_empty()));
                Ok(())
            })
            .unwrap();
        drop(root);
        runtime.collect_garbage().unwrap();
        assert_eq!(runtime.gc_stats().objects, 0);
        assert_eq!(runtime.gc_stats().root_registrations, 0);
        assert_eq!(runtime.gc_stats().indirect_bytes, 0);
    }
}

#[test]
fn interface_roots_trace_data_and_retain_old_dependency_versions() {
    let mut runtime = Runtime::default();
    let program = load_empty(&mut runtime, "interface-data");
    let owner = runtime.select_program(&program).unwrap();
    let array = runtime
        .alloc_array(&owner, Ty::Builtin(BuiltinType::I32), &[7i32.into()])
        .unwrap();
    let template = layouts::interface_value_with(
        &mut runtime,
        Ty::Array(Box::new(Ty::Builtin(BuiltinType::I32))),
        &array,
    );
    let code = runtime
        .interface_description(&template)
        .unwrap()
        .implementation
        .verified_program()
        .clone();
    drop(template);
    let program = runtime
        .load_verified_program("interface-retention", code.clone())
        .unwrap();
    let loaded = runtime.select_program(&program).unwrap();
    let old = loaded.key();
    let interface = runtime.make_interface(&loaded, 0, &array).unwrap();
    assert_eq!(
        runtime
            .make_interface(&loaded, 0, &true.into())
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ScriptTrap
    );
    assert_eq!(
        Runtime::default()
            .make_interface(&loaded, 0, &array)
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ModuleValidation
    );
    drop(array);
    let staged = runtime
        .stage_reload_verified_program(&program, None, code)
        .unwrap();
    runtime.publish_staged_reload(staged).unwrap();
    assert_ne!(runtime.select_program(&program).unwrap().key(), old);
    runtime.collect_garbage().unwrap();
    assert!(runtime.modules().loaded(old).is_some());
    assert_eq!(runtime.modules().retention_counts(old).runtime_values, 0);
    runtime
        .memory
        .scope(&runtime.resources, |cx, _| {
            let interface = cx.downcast::<InterfaceObject>(interface.get(cx)?.object().unwrap())?;
            let array = cx
                .downcast::<ArrayObject>(InterfaceObject::data(cx, interface)?.object().unwrap())?;
            assert_eq!(
                scalar::read_integer(BuiltinType::I32, ArrayObject::get(cx, array, 0)?).unwrap(),
                7
            );
            Ok(())
        })
        .unwrap();
    drop(interface);
    runtime.collect_garbage().unwrap();
    assert!(runtime.modules().loaded(old).is_none());
    assert_eq!(runtime.modules().retention_counts(old).runtime_values, 0);
}
