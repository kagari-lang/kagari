//! Function arguments, generic conversion and retained callback lifetimes.
use super::{compile, compile_program};
use kagari_runtime::{
    module::entry::ProgramEntry,
    native::{
        binding::NativeResult,
        builder::ModuleBuilder,
        callable::{CallableHandle, StoredCallable},
        context::CallContext,
        declarations::FunctionDecl,
        function_handle::PinnedFunction,
        storage::NativeStorage,
        typed::NativeContext,
        types::Type,
        views::{SequenceHandle, ValueHandle},
    },
    value::Value,
};
use kagari_stdlib::declarations::StandardDeclarations;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

type Callback = PinnedFunction<(i32,), i32>;

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
            move |_: &mut CallContext<'_>, value: usize| -> NativeResult<usize> {
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
    let (vm, owner) = compile(
        r#"
        use example::machine_scalars;
        pub fn unsigned(value: u64) -> u64 { machine_scalars::unsigned(value) }
        pub fn signed(value: i64) -> i64 { machine_scalars::signed(value) }
        "#,
        Some(&builder.finish().unwrap()),
    );
    for value in [0, u32::MAX as u64 + 1, u64::MAX] {
        let before = calls.load(Ordering::SeqCst);
        let result = vm.execute_typed::<_, u64>(&owner, "unsigned", (value,));
        if usize::try_from(value).is_ok() {
            assert_eq!(result.unwrap(), value);
            assert_eq!(calls.load(Ordering::SeqCst), before + 1);
        } else {
            assert!(result.is_err());
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
        let result = vm.execute_typed::<_, i64>(&owner, "signed", (value,));
        if isize::try_from(value).is_ok() {
            assert_eq!(result.unwrap(), value);
            assert_eq!(calls.load(Ordering::SeqCst), before + 1);
        } else {
            assert!(result.is_err());
            assert_eq!(calls.load(Ordering::SeqCst), before);
        }
    }
    assert_eq!(vm.runtime().gc().active_roots(), 0);
    assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
}

fn call_repeatedly(
    cx: &mut CallContext<'_>,
    callback: CallableHandle<'_>,
    mut value: i32,
    count: usize,
) -> NativeResult<i32> {
    assert!(
        callback
            .call::<u64, _>(cx, (value,))
            .unwrap_err()
            .message()
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
    let module = builder.finish().unwrap();
    let (vm, loaded) = compile(
        r#"
        use example::callbacks::apply;
        fn main() -> i32 {
            val offset = [14];
            var calls = 0;
            val result = apply(|value| { calls += 1; value + offset[0] }, 0, 3);
            if calls == 3 { result } else { -1 }
        }
        pub fn trapped() -> i32 { apply(|value| 10 / value, 0, 1) }
        pub fn healthy() -> i32 { apply(|value| value + 1, 40, 2) }
    "#,
        Some(&module),
    );
    assert_eq!(
        vm.execute(&loaded, ProgramEntry::Main)
            .unwrap()
            .return_value
            .value(vm.runtime().gc())
            .expect("retained execution result"),
        Value::I32(42)
    );
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
    assert!(vm.execute(&loaded, "trapped").is_err());
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
    assert_eq!(
        vm.execute(&loaded, "healthy")
            .unwrap()
            .return_value
            .value(vm.runtime().gc())
            .expect("retained execution result"),
        Value::I32(42)
    );
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
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
    let old = r#"
        use example::retained::{remember, invoke};
        fn main() -> i32 { val offset = [41]; remember(|value| value + offset[0]); invoke(1) }
        pub fn run() -> i32 { invoke(1) }
    "#;
    let (vm, loaded) = compile(old, Some(&module));
    let old_key = loaded.key();
    assert_eq!(
        vm.execute(&loaded, ProgramEntry::Main)
            .unwrap()
            .return_value
            .value(vm.runtime().gc())
            .expect("retained execution result"),
        Value::I32(42)
    );
    let replacement = vm
        .reload_program(
            &loaded,
            "boundary",
            compile_program(&old.replace("[41]", "[99]"), Some(&module)),
        )
        .unwrap();
    drop(loaded);
    vm.runtime().collect_garbage().unwrap();
    assert!(vm.runtime().modules().loaded(old_key).is_some());
    assert_eq!(
        vm.execute(&replacement, "run")
            .unwrap()
            .return_value
            .value(vm.runtime().gc())
            .expect("retained execution result"),
        Value::I32(42)
    );
    assert_eq!(
        vm.execute(&replacement, ProgramEntry::Main)
            .unwrap()
            .return_value
            .value(vm.runtime().gc())
            .expect("retained execution result"),
        Value::I32(100)
    );
    held.lock().unwrap().take();
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
    assert_eq!(
        vm.runtime()
            .modules()
            .retention_counts(old_key)
            .runtime_values,
        0
    );
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
            |cx: &mut CallContext<'_>, callback: CallableHandle<'_>| -> NativeResult<Value> {
                cx.allocate_result_payload(callback.store(), &[])
            },
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
            |cx: &mut CallContext<'_>, handler: ValueHandle<'_>| -> NativeResult<i32> {
                let stored =
                    handler.with_payload::<StoredCallable, _>(|callback| Ok(callback.clone()))?;
                cx.collect_garbage()?;
                let Value::I32(result) = stored.call_values(cx, &[Value::I32(1)])? else {
                    panic!("checked stored callback result");
                };
                Ok(result)
            },
        )
        .unwrap();
    let module = builder.finish().unwrap();
    let (vm, loaded) = compile(
        r#"
        use example::stored::{Handler, new, invoke};
        fn make() -> Handler { val offset = [41]; new(|value| value + offset[0]) }
        fn main() -> i32 { invoke(make()) }
    "#,
        Some(&module),
    );
    assert_eq!(
        vm.execute(&loaded, ProgramEntry::Main)
            .unwrap()
            .return_value
            .value(vm.runtime().gc())
            .expect("retained execution result"),
        Value::I32(42)
    );
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
}

fn identity(_cx: &mut CallContext<'_>, value: ValueHandle<'_>) -> NativeResult<Value> {
    Ok(value.value())
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
            |_cx: &mut CallContext<'_>, value: ValueHandle<'_>| -> NativeResult<usize> {
                assert!(
                    value
                        .scalar::<i64>()
                        .unwrap_err()
                        .message()
                        .contains("argument type")
                );
                let host = value.scalar::<usize>()?;
                assert_eq!(value.scalar::<u64>()?, host as u64);
                Ok(host)
            },
        )
        .unwrap();
    let module = builder.finish().unwrap();
    let (vm, loaded) = compile(
        r#"
        use example::generic::{identity, probe};
        struct Item { val value: i32 }
        fn main() -> i32 {
            val original = [20, 22];
            val same = identity(original);
            same[0] = 21;
            val item = Item { value: 21 };
            if original[0] != 21 || probe(42) != 42 { return -1; }
            identity(item).value + identity(21)
        }
    "#,
        Some(&module),
    );
    assert_eq!(
        vm.execute(&loaded, ProgramEntry::Main)
            .unwrap()
            .return_value
            .value(vm.runtime().gc())
            .expect("retained execution result"),
        Value::I32(42)
    );
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
}

#[test]
fn sequence_and_callable_arguments_share_an_ordinary_rust_binding() {
    let language = StandardDeclarations::default();
    let mut builder = ModuleBuilder::new(
        "example::fold",
        &language.catalog().expect("explicit standard providers"),
    )
    .unwrap();
    let fold = builder
        .define_function(
            FunctionDecl::new("fold")
                .parameter("values", language.vec(Type::i32()).unwrap())
                .parameter("map", Type::function([Type::i32()], Type::i32()).unwrap())
                .returns(Type::i32()),
        )
        .unwrap();
    builder
        .bind(
            fold,
            |cx: &mut CallContext<'_>,
             values: SequenceHandle<'_>,
             map: CallableHandle<'_>|
             -> NativeResult<i32> {
                let length = values.len()?;
                let mut sum = 0;
                for index in 0..length {
                    let value = values.with_slice::<i32, _>(|values| Ok(values[index]))?;
                    sum += map.call::<i32, _>(cx, (value,))?;
                }
                Ok(sum)
            },
        )
        .unwrap();
    let module = builder.finish().unwrap();
    let (vm, loaded) = compile(
        "use example::fold::fold; fn main() -> i32 { fold(Vec::from([9, 10]), |value| (value + 1) * 2) }",
        Some(&module),
    );
    assert_eq!(
        vm.execute(&loaded, ProgramEntry::Main)
            .unwrap()
            .return_value
            .value(vm.runtime().gc())
            .expect("retained execution result"),
        Value::I32(42)
    );
}
