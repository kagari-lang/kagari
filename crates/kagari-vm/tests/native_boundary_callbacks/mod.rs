//! Function arguments, generic conversion and retained callback lifetimes.
use super::{compile, compile_program};
use kagari_runtime::{
    native::{
        binding::NativeResult,
        builder::ModuleBuilder,
        callable::{CallableHandle, RootedCallable, StoredCallable},
        context::CallContext,
        declarations::FunctionDecl,
        storage::NativeStorage,
        types::Type,
        views::{SequenceHandle, ValueHandle},
    },
    value::Value,
};
use kagari_stdlib::declarations::StandardDeclarations;
use std::{cell::RefCell, rc::Rc};

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
    );
    let apply = builder
        .define_function(
            FunctionDecl::new("apply")
                .parameter("callback", Type::function([Type::i32()], Type::i32()))
                .parameter("value", Type::i32())
                .parameter("count", Type::usize())
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
        fn trapped() -> i32 { apply(|value| 10 / value, 0, 1) }
        fn healthy() -> i32 { apply(|value| value + 1, 40, 2) }
    "#,
        Some(&module),
    );
    assert_eq!(
        vm.execute(&loaded, "main").unwrap().return_value,
        Value::I32(42)
    );
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
    assert!(vm.execute(&loaded, "trapped").is_err());
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
    assert_eq!(
        vm.execute(&loaded, "healthy").unwrap().return_value,
        Value::I32(42)
    );
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
}

#[test]
fn retained_host_callbacks_pin_their_capture_program_across_reload() {
    let held: Rc<RefCell<Option<RootedCallable>>> = Rc::new(RefCell::new(None));
    let mut builder = ModuleBuilder::new(
        "example::retained",
        &StandardDeclarations::default()
            .catalog()
            .expect("explicit standard providers"),
    );
    let remember = builder
        .define_function(
            FunctionDecl::new("remember")
                .parameter("callback", Type::function([Type::i32()], Type::i32())),
        )
        .unwrap();
    let stored = held.clone();
    builder
        .bind(
            remember,
            move |cx: &mut CallContext<'_>, callback: CallableHandle<'_>| -> NativeResult<()> {
                let rooted = callback.store().root(cx)?;
                *stored.borrow_mut() = Some(rooted);
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
            move |cx: &mut CallContext<'_>, value: i32| -> NativeResult<i32> {
                let callback = current.borrow().as_ref().unwrap().clone();
                cx.collect_garbage()?;
                callback.call(cx, (value,))
            },
        )
        .unwrap();
    let module = builder.finish().unwrap();
    let old = r#"
        use example::retained::{remember, invoke};
        fn main() -> i32 { val offset = [41]; remember(|value| value + offset[0]); invoke(1) }
        fn run() -> i32 { invoke(1) }
    "#;
    let (vm, loaded) = compile(old, Some(&module));
    let old_key = loaded.key();
    assert_eq!(
        vm.execute(&loaded, "main").unwrap().return_value,
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
        vm.execute(&replacement, "run").unwrap().return_value,
        Value::I32(42)
    );
    assert_eq!(
        vm.execute(&replacement, "main").unwrap().return_value,
        Value::I32(100)
    );
    held.borrow_mut().take();
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
    );
    let mut declaration = builder.define_type("Handler");
    declaration
        .native_storage(NativeStorage::payload::<StoredCallable>())
        .unwrap();
    let handler = declaration.finish().unwrap();
    let new = builder
        .define_function(
            FunctionDecl::new("new")
                .parameter("callback", Type::function([Type::i32()], Type::i32()))
                .returns(handler.apply([]).unwrap()),
        )
        .unwrap();
    builder
        .bind(
            new,
            |cx: &mut CallContext<'_>, callback: CallableHandle<'_>| -> NativeResult<Value> {
                cx.allocate_result_payload(callback.store())
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
                let callback = stored.root(cx)?;
                cx.collect_garbage()?;
                callback.call(cx, (1,))
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
        vm.execute(&loaded, "main").unwrap().return_value,
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
    );
    let function = builder
        .define_function(FunctionDecl::new("identity"))
        .unwrap();
    builder
        .function(&function, |signature| {
            let item = signature.type_parameter("T")?;
            signature.parameter("value", item.ty());
            signature.returns(item.ty());
            Ok(())
        })
        .unwrap();
    builder.bind(function, identity).unwrap();
    let probe = builder
        .define_function(
            FunctionDecl::new("probe")
                .parameter("value", Type::usize())
                .returns(Type::usize()),
        )
        .unwrap();
    builder
        .bind(
            probe,
            |_cx: &mut CallContext<'_>, value: ValueHandle<'_>| -> NativeResult<usize> {
                assert!(
                    value
                        .scalar::<u64>()
                        .unwrap_err()
                        .message()
                        .contains("argument type")
                );
                value.scalar::<usize>()
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
        vm.execute(&loaded, "main").unwrap().return_value,
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
    );
    let fold = builder
        .define_function(
            FunctionDecl::new("fold")
                .parameter("values", language.vec(Type::i32()))
                .parameter("map", Type::function([Type::i32()], Type::i32()))
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
        "use example::fold::fold; fn main() -> i32 { fold([9, 10], |value| (value + 1) * 2) }",
        Some(&module),
    );
    assert_eq!(
        vm.execute(&loaded, "main").unwrap().return_value,
        Value::I32(42)
    );
}
