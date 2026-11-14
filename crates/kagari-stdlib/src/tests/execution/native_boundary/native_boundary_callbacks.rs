//! Function arguments, generic conversion and retained callback lifetimes.
use super::{compile, compile_program};
use crate::{declarations::StandardDeclarations, tests::support::bytecode::reload_bytecode};
use kagari_runtime::{
    error::RuntimeErrorKind,
    module::entry::ProgramEntry,
    native::{
        binding::{Codec, NativeBinding, NativeResult},
        builder::ModuleBuilder,
        callable::{CallableHandle, StoredCallable},
        context::NativeCall,
        conversion::context::ConversionContext,
        declarations::FunctionDecl,
        function_handle::Function,
        storage::NativeStorage,
        typed::NativeContext,
        types::Type,
        views::ValueHandle,
    },
    value::view::ValueView,
};

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

type Callback = Function<(i32,), i32>;

#[test]
fn machine_scalar_conveniences_check_host_range_before_callback_entry() {
    let catalog = StandardDeclarations::default().catalog().unwrap();
    let mut builder = ModuleBuilder::new("example::machine_scalars", &catalog).unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let unsigned = builder
        .define_function(
            FunctionDecl::new("unsigned")
                .parameter("value", Type::u64())
                .returns(Type::u64()),
        )
        .unwrap();
    let observed = calls.clone();
    builder
        .bind(
            unsigned,
            move |_: &mut NativeCall<'_>, value: usize| -> NativeResult<usize> {
                observed.fetch_add(1, Ordering::SeqCst);
                Ok(value)
            },
        )
        .unwrap();
    let signed = builder
        .define_function(
            FunctionDecl::new("signed")
                .parameter("value", Type::i64())
                .returns(Type::i64()),
        )
        .unwrap();
    let observed = calls.clone();
    builder
        .bind(
            signed,
            move |_: &mut NativeContext<'_>, (value,): (isize,)| -> NativeResult<isize> {
                observed.fetch_add(1, Ordering::SeqCst);
                Ok(value)
            },
        )
        .unwrap();
    let (mut vm, owner) = compile(
        include_str!("../../../../tests/fixtures/native/callback-machine-scalars.kgr"),
        Some(&builder.finish().unwrap()),
    );
    let _owner_program = owner;
    let _owner = vm.select_program(&_owner_program).unwrap();
    vm.collect_garbage().unwrap();
    let baseline = vm.gc_stats();
    for value in [0, u32::MAX as u64 + 1, u64::MAX] {
        let before = calls.load(Ordering::SeqCst);
        let result =
            vm.execute_typed::<_, u64>(&_owner_program, "unsigned", (value,), &Default::default());
        if usize::try_from(value).is_ok() {
            assert_eq!(result.unwrap(), value);
            assert_eq!(calls.load(Ordering::SeqCst), before + 1);
        } else {
            assert!(
                matches!(result.unwrap_err(), error if error.kind() == RuntimeErrorKind::HostCallFailure)
            );
            assert_eq!(calls.load(Ordering::SeqCst), before);
        }
    }
    for value in [
        i64::MIN,
        i32::MIN as i64 - 1,
        0,
        i32::MAX as i64 + 1,
        i64::MAX,
    ] {
        let before = calls.load(Ordering::SeqCst);
        let result =
            vm.execute_typed::<_, i64>(&_owner_program, "signed", (value,), &Default::default());
        if isize::try_from(value).is_ok() {
            assert_eq!(result.unwrap(), value);
            assert_eq!(calls.load(Ordering::SeqCst), before + 1);
        } else {
            assert!(
                matches!(result.unwrap_err(), error if error.kind() == RuntimeErrorKind::HostCallFailure)
            );
            assert_eq!(calls.load(Ordering::SeqCst), before);
        }
    }
    vm.collect_garbage().unwrap();
    assert_eq!(
        vm.gc_stats().root_registrations,
        baseline.root_registrations
    );
    assert_eq!(vm.gc_stats().objects, baseline.objects);
    assert_eq!(vm.resources().counters().current_call_depth, 0);
}

fn call_repeatedly(
    cx: &mut NativeCall<'_>,
    callback: CallableHandle<'_>,
    mut value: i32,
    count: usize,
) -> NativeResult<i32> {
    assert!(
        callback
            .call::<u64, _>(cx, (value,))
            .unwrap_err()
            .to_string()
            .contains("conversion")
    );
    for _ in 0..count {
        cx.collect_garbage()?;
        value = callback.call(cx, (value,))?;
    }
    Ok(value)
}

#[test]
fn function_arguments_call_captured_script_closures_synchronously() {
    let mut builder = ModuleBuilder::new(
        "example::callbacks",
        &StandardDeclarations::default()
            .catalog()
            .expect("explicit standard providers"),
    )
    .unwrap();
    let apply = builder
        .define_function(
            FunctionDecl::new("apply")
                .parameter(
                    "callback",
                    Type::function([Type::i32()], Type::i32()).unwrap(),
                )
                .parameter("value", Type::i32())
                .parameter("count", Type::u64())
                .returns(Type::i32()),
        )
        .unwrap();
    builder.bind(apply, call_repeatedly).unwrap();
    let step = builder
        .define_function(
            FunctionDecl::new("step")
                .parameter("value", Type::i32())
                .parameter("offset", Type::i32())
                .returns(Type::array(Type::i32()).unwrap()),
        )
        .unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    builder
        .bind(
            step,
            NativeBinding::scoped(
                vec![Codec::Exact(Type::i32()), Codec::Exact(Type::i32())],
                Codec::Value,
                move |call| {
                    observed.fetch_add(1, Ordering::SeqCst);
                    let value = call.argument(0)?.as_i32().unwrap();
                    let offset = call.argument(1)?.as_i32().unwrap();
                    let expected = value + offset;
                    let element = call.result_type_parameter(0)?;
                    let result = call.allocate_array(element, &[ValueView::from(expected)])?;
                    call.set_result(result)?;
                    call.collect_garbage()?;
                    assert_eq!(
                        call.array_element(&result, 0)?.unwrap().as_i32(),
                        Some(expected),
                        "nested Native pending result survives collection before return"
                    );
                    Ok(())
                },
            ),
        )
        .unwrap();
    let module = builder.finish().unwrap();
    let (mut vm, loaded) = compile(
        include_str!("../../../../tests/fixtures/native/callback-captured-calls.kgr"),
        Some(&module),
    );
    let _loaded_program = loaded;
    let loaded = vm.select_program(&_loaded_program).unwrap();
    vm.collect_garbage().unwrap();
    let baseline = vm.gc_stats();
    assert_eq!(
        vm.execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
            .unwrap()
            .return_value
            .as_i32(),
        Some(42)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    vm.collect_garbage().unwrap();
    assert_eq!(vm.gc_stats().objects, baseline.objects);
    assert_eq!(
        vm.gc_stats().root_registrations,
        baseline.root_registrations
    );
    let failure = vm
        .execute_module(&loaded, "trapped", vm.execution_options())
        .unwrap_err();
    assert_eq!(failure.kind(), RuntimeErrorKind::ScriptTrap);
    assert!(failure.to_string().contains("division by zero"));
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    vm.collect_garbage().unwrap();
    assert_eq!(vm.gc_stats().objects, baseline.objects);
    assert_eq!(
        vm.gc_stats().root_registrations,
        baseline.root_registrations
    );
    assert_eq!(
        vm.execute_module(&loaded, "healthy", vm.execution_options())
            .unwrap()
            .return_value
            .as_i32(),
        Some(42)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 5);
    vm.collect_garbage().unwrap();
    assert_eq!(vm.gc_stats().objects, baseline.objects);
    assert_eq!(
        vm.gc_stats().root_registrations,
        baseline.root_registrations
    );
    assert_eq!(vm.resources().counters().current_call_depth, 0);
    assert!(vm.execution_root().is_none());
    assert!(!vm.is_quarantined());
}

#[test]
fn retained_host_callbacks_pin_their_capture_program_across_reload() {
    let held: Arc<Mutex<Option<Callback>>> = Arc::new(Mutex::new(None));
    let mut builder = ModuleBuilder::new(
        "example::retained",
        &StandardDeclarations::default()
            .catalog()
            .expect("explicit standard providers"),
    )
    .unwrap();
    let remember = builder
        .define_function(FunctionDecl::new("remember").parameter(
            "callback",
            Type::function([Type::i32()], Type::i32()).unwrap(),
        ))
        .unwrap();
    let stored = held.clone();
    builder
        .bind(
            remember,
            move |_: &mut NativeContext<'_>, (callback,): (Callback,)| -> NativeResult<()> {
                *stored.lock().unwrap() = Some(callback);
                Ok(())
            },
        )
        .unwrap();
    let invoke = builder
        .define_function(
            FunctionDecl::new("invoke")
                .parameter("value", Type::i32())
                .returns(Type::i32()),
        )
        .unwrap();
    let current = held.clone();
    builder
        .bind(
            invoke,
            move |cx: &mut NativeContext<'_>, (value,): (i32,)| -> NativeResult<i32> {
                let callback = current.lock().unwrap().as_ref().unwrap().clone();
                cx.collect_garbage()?;
                callback.call(cx, (value,))
            },
        )
        .unwrap();
    let module = builder.finish().unwrap();
    let old = include_str!("../../../../tests/fixtures/native/callback-retained-reload.kgr");
    let (mut vm, loaded) = compile(old, Some(&module));
    let _loaded_program = loaded;
    let loaded = vm.select_program(&_loaded_program).unwrap();
    vm.collect_garbage().unwrap();
    let baseline = vm.gc_stats();
    let old_key = loaded.key();
    let old_descriptor = loaded.clone();
    assert_eq!(
        vm.execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
            .unwrap()
            .return_value
            .as_i32(),
        Some(42)
    );
    assert_eq!(old.matches("[41]").count(), 1);
    reload_bytecode(
        &mut vm,
        &_loaded_program,
        Some("boundary".into()),
        compile_program(&old.replace("[41]", "[99]"), Some(&module)),
    )
    .unwrap();
    let _replacement_program = _loaded_program.clone();
    let replacement = vm.select_program(&_replacement_program).unwrap();
    drop(loaded);
    vm.collect_garbage().unwrap();
    assert!(vm.validate_executable_module(&old_descriptor).is_ok());
    assert_eq!(
        vm.execute_module(&replacement, "run", vm.execution_options())
            .unwrap()
            .return_value
            .as_i32(),
        Some(42)
    );
    assert_eq!(
        vm.execute_module(&replacement, ProgramEntry::Main, vm.execution_options())
            .unwrap()
            .return_value
            .as_i32(),
        Some(100)
    );
    held.lock().unwrap().take();
    vm.collect_garbage().unwrap();
    assert_eq!(vm.gc_stats().objects, baseline.objects);
    assert_eq!(
        vm.gc_stats().root_registrations,
        baseline.root_registrations
    );
    assert_eq!(vm.modules().retention_counts(old_key).runtime_values, 0);
    assert!(vm.validate_executable_module(&old_descriptor).is_err());
}

#[test]
fn stored_callbacks_trace_captures_through_an_ordinary_native_payload() {
    let mut builder = ModuleBuilder::new(
        "example::stored",
        &StandardDeclarations::default()
            .catalog()
            .expect("explicit standard providers"),
    )
    .unwrap();
    let mut declaration = builder.define_type("Handler").unwrap();
    declaration
        .native_storage(NativeStorage::payload::<StoredCallable>())
        .unwrap();
    let handler = declaration.finish().unwrap();
    let new = builder
        .define_function(
            FunctionDecl::new("new")
                .parameter(
                    "callback",
                    Type::function([Type::i32()], Type::i32()).unwrap(),
                )
                .returns(handler.apply([]).unwrap()),
        )
        .unwrap();
    builder
        .bind(
            new,
            NativeBinding::new(vec![Codec::Callable], Codec::Value, |cx| {
                let callback = cx.callable(0)?;
                cx.allocate_result_with(&[], |init| callback.store(init))
            }),
        )
        .unwrap();
    let invoke = builder
        .define_function(
            FunctionDecl::new("invoke")
                .parameter("handler", handler.apply([]).unwrap())
                .returns(Type::i32()),
        )
        .unwrap();
    builder
        .bind(
            invoke,
            |cx: &mut NativeCall<'_>, handler: ValueHandle<'_>| -> NativeResult<i32> {
                let result = handler
                    .with_operation(cx, |cx| {
                        let value = handler.value(cx)?;
                        let stored = cx
                            .callable_payload::<StoredCallable>(&value, |_, callback| {
                                Some(callback)
                            })?
                            .expect("stored callback provider");
                        cx.collect_garbage()?;
                        stored.call::<i32, _>(cx, (1,))
                    })?
                    .expect("first guarded callback");
                Ok(result)
            },
        )
        .unwrap();
    let module = builder.finish().unwrap();
    let (mut vm, installed) = compile(
        include_str!("../../../../tests/fixtures/native/stored_callback.kgr"),
        Some(&module),
    );
    let loaded = vm.select_program(&installed).unwrap();
    vm.collect_garbage().unwrap();
    let baseline = vm.gc_stats().objects;
    let output = vm
        .execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
        .unwrap();
    let mut conversion = ConversionContext::new(&mut vm, &loaded).unwrap();
    assert_eq!(conversion.decode::<i32>(&output.return_value).unwrap(), 42);
    drop(conversion);
    vm.collect_garbage().unwrap();
    assert_eq!(vm.gc_stats().objects, baseline);
}

fn identity<'call>(
    cx: &mut NativeCall<'call>,
    value: ValueHandle<'call>,
) -> NativeResult<ValueView<'call>> {
    value.value(cx)
}

#[test]
fn ordinary_generic_value_binding_keeps_closed_types_and_shared_values() {
    let mut builder = ModuleBuilder::new(
        "example::generic",
        &StandardDeclarations::default()
            .catalog()
            .expect("explicit standard providers"),
    )
    .unwrap();
    let function = builder
        .define_function(FunctionDecl::new("identity"))
        .unwrap();
    builder
        .function(&function, |signature| {
            let item = signature.type_parameter("T")?;
            signature.parameter("value", item.ty())?;
            signature.returns(item.ty())?;
            Ok(())
        })
        .unwrap();
    builder.bind(function, identity).unwrap();
    let probe = builder
        .define_function(
            FunctionDecl::new("probe")
                .parameter("value", Type::u64())
                .returns(Type::u64()),
        )
        .unwrap();
    builder
        .bind(
            probe,
            |cx: &mut NativeCall<'_>, value: ValueHandle<'_>| -> NativeResult<usize> {
                assert!(
                    value
                        .scalar::<i64>(cx)
                        .unwrap_err()
                        .to_string()
                        .contains("argument type")
                );
                let host = value.scalar::<usize>(cx)?;
                assert_eq!(value.scalar::<u64>(cx)?, host as u64);
                Ok(host)
            },
        )
        .unwrap();
    let module = builder.finish().unwrap();
    let (mut vm, loaded) = compile(
        include_str!("../../../../tests/fixtures/native/callback-generic-identity.kgr"),
        Some(&module),
    );
    let _loaded_program = loaded;
    let loaded = vm.select_program(&_loaded_program).unwrap();
    vm.collect_garbage().unwrap();
    let baseline = vm.gc_stats();
    assert_eq!(
        vm.execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
            .unwrap()
            .return_value
            .as_i32(),
        Some(42)
    );
    vm.collect_garbage().unwrap();
    assert_eq!(vm.gc_stats().objects, baseline.objects);
    assert_eq!(
        vm.gc_stats().root_registrations,
        baseline.root_registrations
    );
}

#[test]
fn array_and_callable_arguments_share_an_ordinary_rust_binding() {
    let language = StandardDeclarations::default();
    let mut builder = ModuleBuilder::new(
        "example::fold",
        &language.catalog().expect("explicit standard providers"),
    )
    .unwrap();
    let fold = builder
        .define_function(
            FunctionDecl::new("fold")
                .parameter("values", Type::array(Type::i32()).unwrap())
                .parameter("map", Type::function([Type::i32()], Type::i32()).unwrap())
                .returns(Type::i32()),
        )
        .unwrap();
    builder
        .bind(
            fold,
            |cx: &mut NativeCall<'_>,
             values: ValueHandle<'_>,
             map: CallableHandle<'_>|
             -> NativeResult<i32> {
                let values = values.value(cx)?;
                let length = cx.array_len(&values)?;
                let mut sum = 0;
                for index in 0..length {
                    let value = cx
                        .array_element(&values, index)?
                        .expect("checked index")
                        .as_i32()
                        .expect("checked element");
                    sum += map.call::<i32, _>(cx, (value,))?;
                }
                Ok(sum)
            },
        )
        .unwrap();
    let module = builder.finish().unwrap();
    let (mut vm, loaded) = compile(
        include_str!("../../../../tests/fixtures/native/callback-array-fold.kgr"),
        Some(&module),
    );
    let _loaded_program = loaded;
    let loaded = vm.select_program(&_loaded_program).unwrap();
    vm.collect_garbage().unwrap();
    let baseline = vm.gc_stats();
    assert_eq!(
        vm.execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
            .unwrap()
            .return_value
            .as_i32(),
        Some(42)
    );
    vm.collect_garbage().unwrap();
    assert_eq!(vm.gc_stats().objects, baseline.objects);
    assert_eq!(
        vm.gc_stats().root_registrations,
        baseline.root_registrations
    );
}
