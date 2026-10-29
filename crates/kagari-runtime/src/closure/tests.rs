use crate::{
    Runtime,
    closure::{ClosureTarget, ObservedCallableTarget},
    frame::{
        cursor::kernel::RegionExit,
        driver::{ExecutionAction, ReturnAction},
    },
    module::dispatch::CallRouteKind,
    native::{
        callable::PreparedClosure, context::ScriptInvoker, conversion::context::ConversionContext,
        function_handle::Function,
    },
    source::{SourceQuery, SourceUnavailableReason},
    test_support::update_programs as programs,
    value::Value,
};
use kagari_bytecode::module::CallableTarget;

#[test]
fn named_values_and_named_locals_select_current_but_zero_capture_bodies_stay_exact() {
    let source = r#"
        pub fn helper() -> i32 { 1 }
        pub fn named() -> fn() -> i32 {
            fn local() -> i32 { 3 }
            local
        }
        pub fn generated() -> fn() -> i32 { || helper() + 10 }
        fn main() {}
    "#;
    let mut runtime = Runtime::default();
    let program = runtime
        .load_verified_program("exports", programs::source_code(source))
        .unwrap();
    let old = runtime.select_program(&program).unwrap();
    let old_key = old.key();
    let old_token = old.publication_token();
    let mut local_seen = false;
    let mut generated_seen = false;
    let mut values = Vec::new();
    for function in &old.bytecode.functions {
        let kind = old
            .bind_call_route(CallableTarget::Script(function.id))
            .unwrap()
            .kind();
        if function.name == "local" {
            local_seen = true;
            assert_eq!(kind, CallRouteKind::Named);
        }
        generated_seen |= kind == CallRouteKind::BodyOwned;
        let value = runtime
            .make_closure(&old, function.id, vec![], None)
            .unwrap();
        let root = runtime.root_value(value).unwrap();
        assert_eq!(
            runtime
                .resolve_closure(&value)
                .unwrap()
                .call_route()
                .is_some(),
            kind != CallRouteKind::BodyOwned
        );
        values.push((root, kind));
    }
    assert!(
        local_seen,
        "named local must be emitted as a named callable"
    );
    assert!(
        generated_seen,
        "fixture must contain a zero-capture generated closure"
    );
    let next = source
        .replace("{ 1 }", "{ 2 }")
        .replace("{ 3 }", "{ 4 }")
        .replace("+ 10", "+ 20");
    for result in programs::source_free_reload_results(&old, &next) {
        result.unwrap();
    }
    let staged = runtime
        .stage_reload_program(&program, None, programs::bytecode(&next, Some(&old)))
        .unwrap();
    let current_token = staged.publication_token();
    runtime.publish_staged_reload(staged).unwrap();
    runtime.collect_garbage().unwrap();
    let mut observed = Vec::new();
    for (root, kind) in &values {
        let value = root.value(runtime.gc()).unwrap();
        let selected = runtime
            .resolve_closure(&value)
            .unwrap()
            .select_call(&runtime)
            .unwrap();
        assert!(matches!(selected.target, ClosureTarget::Script(_)));
        let expected = if *kind == CallRouteKind::BodyOwned {
            old_token
        } else {
            current_token
        };
        assert_eq!(selected.implementation.publication_token(), expected);
        let token = runtime.observe_callable_value(root).unwrap().unwrap();
        assert_eq!(token.publication, expected);
        assert_eq!(
            token.target,
            ObservedCallableTarget::Entry(selected.callable_target())
        );
        observed.push(token);
        assert!(Runtime::default().observe_callable_value(root).is_err());
        let current = runtime.select_program(&program).unwrap();
        let stack = runtime.enter_execution_stack(&current).unwrap();
        stack.push_closure(&runtime, &value, &[], None).unwrap();
        assert_eq!(
            stack.current().unwrap().loaded().publication_token(),
            expected
        );
        if *kind == CallRouteKind::BodyOwned {
            let mut called = false;
            for _ in 0..64 {
                match stack
                    .execute_region(&runtime, &mut None, unsafe {
                        // SAFETY: The callback rejects execution or returns only a scalar; it retains no raw edges.
                        ScriptInvoker::new(|_, _, _, _| panic!("no native calls"))
                    })
                    .unwrap()
                {
                    RegionExit::Call(ExecutionAction::Script) => {
                        assert_eq!(
                            stack.current().unwrap().loaded().publication_token(),
                            current_token
                        );
                        called = true;
                        break;
                    }
                    RegionExit::Boundary | RegionExit::Safepoint => {}
                    _ => panic!("retained body must call the ordinary helper"),
                }
            }
            assert!(called);
        }
    }
    // Observation alone must not keep an old executable or heap value alive.
    drop(values);
    drop(old);
    runtime.collect_garbage().unwrap();
    assert!(runtime.modules().loaded(old_key).is_none());
    assert!(observed.iter().any(|token| token.publication == old_token));
}

#[test]
fn only_named_value_roots_allow_old_code_collection_and_current_admission() {
    let source = "pub fn helper() -> i32 { 1 } pub fn generated() -> fn() -> i32 { || helper() + 10 } fn main() {}";
    let mut runtime = Runtime::default();
    let program = runtime
        .load_verified_program("exports", programs::source_code(source))
        .unwrap();
    let old = runtime.select_program(&program).unwrap();
    let old_key = old.key();
    let (_, helper) = old.exported_function("helper").unwrap();
    let value = runtime.make_closure(&old, helper, vec![], None).unwrap();
    let named = {
        let mut cx = ConversionContext::new(&runtime, &old).unwrap();
        let signature = cx.type_for::<Function<(), i32>>().unwrap();
        /* SAFETY: The new closure has no intervening collection; its concrete Function mapping has no user callback before retention. */
        unsafe { cx.decode_transport::<Function<(), i32>>(&signature, &value) }.unwrap()
    };
    let generated = old
        .bytecode
        .functions
        .iter()
        .find(|function| {
            old.bind_call_route(CallableTarget::Script(function.id))
                .unwrap()
                .kind()
                == CallRouteKind::BodyOwned
        })
        .unwrap()
        .id;
    let generated = runtime.make_closure(&old, generated, vec![], None).unwrap();
    let generated = runtime.root_value(generated).unwrap();
    let staged = runtime
        .stage_reload_program(
            &program,
            None,
            programs::bytecode(&source.replace("{ 1 }", "{ 2 }"), Some(&old)),
        )
        .unwrap();
    let current_token = staged.publication_token();
    runtime.publish_staged_reload(staged).unwrap();
    drop(old);
    runtime.collect_garbage().unwrap();
    assert!(
        runtime.modules().loaded(old_key).is_some(),
        "generated body retains exact code"
    );
    drop(generated);
    let reclaimed = runtime.collect_garbage().unwrap().reclaimed_modules;
    assert!(
        reclaimed.contains(&old_key),
        "a scalar named value is not an executable lease"
    );
    assert!(runtime.modules().loaded(old_key).is_none());

    let root = named.closure_value().unwrap();
    assert_eq!(
        runtime
            .observe_callable_value(root)
            .unwrap()
            .unwrap()
            .publication,
        current_token
    );
    for target in [
        runtime.value_source_target(root).unwrap(),
        named.source_target(),
    ] {
        let SourceQuery::Unavailable(source) = runtime.function_source(&target).unwrap() else {
            panic!("fixture deliberately has no source package");
        };
        assert_eq!(source.reason, SourceUnavailableReason::MissingPackage);
        assert_eq!(source.publication, Some(current_token));
    }
    let prepared = named.prepare_call(&runtime).unwrap();
    assert_eq!(prepared.owner().publication_token(), current_token);
    let current = runtime.select_program(&program).unwrap();
    let stack = runtime.enter_execution_stack(&current).unwrap();
    stack
        .push_closure(&runtime, &root.value(runtime.gc()).unwrap(), &[], None)
        .unwrap();
    let RegionExit::Return(result) = stack
        .execute_region(&runtime, &mut None, unsafe {
            // SAFETY: The callback rejects execution or returns only a scalar; it retains no raw edges.
            ScriptInvoker::new(|_, _, _, _| panic!("no native calls"))
        })
        .unwrap()
    else {
        panic!("scalar helper returns directly");
    };
    assert_eq!(
        stack.finish_return(&runtime, result).unwrap(),
        ReturnAction::Complete(Value::I32(2))
    );
}

#[test]
fn retained_computation_does_not_reclassify_a_named_body_by_capture_count() {
    let source = "pub fn work(value: i32) -> i32 { value + 1 } fn main() {}";
    let mut runtime = Runtime::default();
    let program = runtime
        .load_verified_program("exports", programs::source_code(source))
        .unwrap();
    let old = runtime.select_program(&program).unwrap();
    let (owner, function) = old.exported_function("work").unwrap();
    let ordinary = runtime
        .make_closure(&owner, function, vec![Value::I32(7)], None)
        .unwrap();
    let exact = runtime
        .make_retained_closure(&owner, function, vec![Value::I32(7)], None)
        .unwrap();
    let (_, admitted) = PreparedClosure::from_value(&runtime, ordinary)
        .unwrap()
        .prepare_call(&runtime)
        .unwrap();
    let ordinary = runtime.root_value(ordinary).unwrap();
    let exact = runtime.root_value(exact).unwrap();
    let staged = runtime
        .stage_reload_program(
            &program,
            None,
            programs::bytecode(&source.replace("+ 1", "+ 2"), Some(&old)),
        )
        .unwrap();
    let current = staged.publication_token();
    runtime.publish_staged_reload(staged).unwrap();
    for (root, expected) in [
        (&ordinary, current),
        (&exact, old.publication_token()),
        (&admitted, old.publication_token()),
    ] {
        let selected = runtime
            .resolve_closure(&root.value(runtime.gc()).unwrap())
            .unwrap()
            .select_call(&runtime)
            .unwrap();
        assert_eq!(selected.implementation.publication_token(), expected);
        assert_eq!(selected.captures, [Value::I32(7)]);
    }
}
