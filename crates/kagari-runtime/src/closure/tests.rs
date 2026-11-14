use crate::{
    Runtime,
    closure::{ClosureMode, ObservedCallableTarget},
    frame::{
        cursor::kernel::RegionExit,
        driver::{ExecutionAction, ReturnAction},
    },
    module::{ExecutableModule, dispatch::CallRouteKind},
    native::{
        conversion::{FromKagari, arguments::ArgumentValues, context::ConversionContext},
        function_handle::Function,
    },
    source::{SourceQuery, SourceUnavailableReason},
    test_support::update_programs as programs,
    value::{carriers::RootedValue, view::ValueView},
};
use kagari_bytecode::module::CallableTarget;
use kagari_contract::ids::FunctionRef;

fn rooted_closure(
    runtime: &mut Runtime,
    owner: &ExecutableModule,
    function: FunctionRef,
    captures: &[ValueView<'_>],
    mode: ClosureMode,
) -> RootedValue {
    let mut source = ConversionContext::new(runtime, owner).unwrap();
    let value = source
        .allocate_script_closure(function, captures, None, mode)
        .unwrap();
    source.retain(&value).unwrap()
}

#[test]
fn named_values_and_named_locals_select_current_but_zero_capture_bodies_stay_exact() {
    let source = include_str!("../../tests/fixtures/closure/named-routes.kgr");
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
        let root = rooted_closure(&mut runtime, &old, function.id, &[], ClosureMode::Ordinary);
        let mut access = runtime.host_scope(&[]).unwrap();
        let value = access.import(&root).unwrap();
        let record = access.runtime_mut().resolve_closure(&value).unwrap();
        assert_eq!(
            record.call_route().is_some(),
            kind != CallRouteKind::BodyOwned
        );
        drop(access);
        values.push((root, kind));
    }
    assert!(
        local_seen,
        "named local must be emitted as a named callable"
    );
    assert!(
        generated_seen,
        "fixture contains a zero-capture generated closure"
    );
    let next = source
        .replace("    1\n", "    2\n")
        .replace("        3\n", "        4\n")
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
        let expected = if *kind == CallRouteKind::BodyOwned {
            old_token
        } else {
            current_token
        };
        let observation = runtime.observe_callable_value(root).unwrap().unwrap();
        assert_eq!(observation.publication, expected);
        observed.push(observation);
        assert!(Runtime::default().observe_callable_value(root).is_err());
        let current = runtime.select_program(&program).unwrap();
        // A Host source outlives the nested execution activation. Its locals do
        // not cross a live callee mark in that callee's own execution storage.
        let mut access = runtime.host_scope(&[]).unwrap();
        let value = access.import(root).unwrap();
        let mut activation = access
            .runtime_mut()
            .enter_execution_stack(&current)
            .unwrap();
        activation
            .with(|runtime, stack| {
                stack
                    .push_closure(runtime, &value, ArgumentValues::scalars(&[]), None)
                    .unwrap();
                let frame = stack.current(runtime).unwrap();
                assert_eq!(frame.loaded().publication_token(), expected);
                assert!(matches!(frame.target(), CallableTarget::Script(_)));
                assert_eq!(
                    observation.target,
                    ObservedCallableTarget::Entry(frame.target())
                );
                drop(frame);
                if *kind == CallRouteKind::BodyOwned {
                    let mut called = false;
                    for _ in 0..64 {
                        match stack
                            .execute_region(runtime, &mut None, |_, _, _, _, _| {
                                panic!("no native calls")
                            })
                            .unwrap()
                        {
                            RegionExit::Call(ExecutionAction::Script) => {
                                assert_eq!(
                                    stack.current(runtime).unwrap().loaded().publication_token(),
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
                Ok(())
            })
            .unwrap();
    }
    // Detached observation tokens do not keep either the value or old body alive.
    drop(values);
    drop(old);
    runtime.collect_garbage().unwrap();
    assert!(runtime.modules().loaded(old_key).is_none());
    assert!(observed.iter().any(|token| token.publication == old_token));
}

#[test]
fn only_named_value_roots_allow_old_code_collection_and_current_admission() {
    let source = include_str!("../../tests/fixtures/closure/named-roots.kgr");
    let mut runtime = Runtime::default();
    let program = runtime
        .load_verified_program("exports", programs::source_code(source))
        .unwrap();
    let old = runtime.select_program(&program).unwrap();
    let old_key = old.key();
    let (_, helper) = old.exported_function("helper").unwrap();
    let value = rooted_closure(&mut runtime, &old, helper, &[], ClosureMode::Ordinary);
    let named = {
        let mut cx = ConversionContext::new(&mut runtime, &old).unwrap();
        let signature = cx.type_for::<Function<(), i32>>().unwrap();
        let value = cx.import(&value).unwrap();
        Function::<(), i32>::from_kagari(&mut cx, &signature, &value).unwrap()
    };
    drop(value);
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
    let generated = rooted_closure(&mut runtime, &old, generated, &[], ClosureMode::Ordinary);
    let staged = runtime
        .stage_reload_program(
            &program,
            None,
            programs::bytecode(&source.replace("    1\n", "    2\n"), Some(&old)),
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
    runtime.collect_garbage().unwrap();
    assert!(
        runtime.modules().loaded(old_key).is_none(),
        "named values do not own an old body lease"
    );
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
            panic!("fixture has no source package");
        };
        assert_eq!(source.reason, SourceUnavailableReason::MissingPackage);
        assert_eq!(source.publication, Some(current_token));
    }
    let prepared = named.prepare_call(&mut runtime).unwrap();
    assert_eq!(prepared.owner().publication_token(), current_token);
    let current = runtime.select_program(&program).unwrap();
    let mut access = runtime.host_scope(&[]).unwrap();
    let value = access.import(root).unwrap();
    let mut activation = access
        .runtime_mut()
        .enter_execution_stack(&current)
        .unwrap();
    activation
        .with(|runtime, stack| {
            stack
                .push_closure(runtime, &value, ArgumentValues::scalars(&[]), None)
                .unwrap();
            let RegionExit::Return(result) = stack
                .execute_region(runtime, &mut None, |_, _, _, _, _| {
                    panic!("no native calls")
                })
                .unwrap()
            else {
                panic!("scalar helper returns directly");
            };
            let ReturnAction::Complete(result) = stack.finish_return(runtime, result).unwrap()
            else {
                panic!("root helper completes");
            };
            assert_eq!(
                stack.retain_result(runtime, result).unwrap().as_i32(),
                Some(2)
            );
            Ok(())
        })
        .unwrap();
}

#[test]
fn retained_computation_does_not_reclassify_a_named_body_by_capture_count() {
    let source = include_str!("../../tests/fixtures/closure/capture-classification.kgr");
    let mut runtime = Runtime::default();
    let program = runtime
        .load_verified_program("exports", programs::source_code(source))
        .unwrap();
    let old = runtime.select_program(&program).unwrap();
    let (owner, function) = old.exported_function("work").unwrap();
    let capture = [ValueView::from(7i32)];
    let ordinary = rooted_closure(
        &mut runtime,
        &owner,
        function,
        &capture,
        ClosureMode::Ordinary,
    );
    let exact = rooted_closure(
        &mut runtime,
        &owner,
        function,
        &capture,
        ClosureMode::Retained,
    );
    let admitted = {
        let mut cx = ConversionContext::new(&mut runtime, &owner).unwrap();
        let value = cx.import(&ordinary).unwrap();
        let execution = value.location().unwrap().execution;
        let selected = cx
            .runtime_mut()
            .select_closure_call(execution, &value)
            .unwrap();
        let frozen = cx.freeze_closure(&selected).unwrap();
        cx.retain(&frozen).unwrap()
    };
    let staged = runtime
        .stage_reload_program(
            &program,
            None,
            programs::bytecode(&source.replace("+ 1", "+ 2"), Some(&old)),
        )
        .unwrap();
    let current_token = staged.publication_token();
    runtime.publish_staged_reload(staged).unwrap();
    runtime.collect_garbage().unwrap();
    let current = runtime.select_program(&program).unwrap();
    for (root, expected, result) in [
        (&ordinary, current_token, 9),
        (&exact, old.publication_token(), 8),
        (&admitted, old.publication_token(), 8),
    ] {
        assert_eq!(
            runtime
                .observe_callable_value(root)
                .unwrap()
                .unwrap()
                .publication,
            expected
        );
        let mut access = runtime.host_scope(&[]).unwrap();
        let value = access.import(root).unwrap();
        let record = access.runtime_mut().resolve_closure(&value).unwrap();
        assert_eq!(record.capture_types.len(), 1);
        let mut activation = access
            .runtime_mut()
            .enter_execution_stack(&current)
            .unwrap();
        activation
            .with(|runtime, stack| {
                stack
                    .push_closure(runtime, &value, ArgumentValues::scalars(&[]), None)
                    .unwrap();
                let frame = stack.current(runtime).unwrap();
                assert_eq!(frame.loaded().publication_token(), expected);
                let parameter = frame.function().unwrap().metadata.parameter_slots[0];
                assert_eq!(
                    frame.read_slot(runtime, parameter).unwrap().as_i32(),
                    Some(7)
                );
                drop(frame);
                let RegionExit::Return(value) = stack
                    .execute_region(runtime, &mut None, |_, _, _, _, _| {
                        panic!("no native calls")
                    })
                    .unwrap()
                else {
                    panic!("captured scalar helper returns");
                };
                let ReturnAction::Complete(value) = stack.finish_return(runtime, value).unwrap()
                else {
                    panic!("root closure completes");
                };
                assert_eq!(
                    stack.retain_result(runtime, value).unwrap().as_i32(),
                    Some(result)
                );
                Ok(())
            })
            .unwrap();
    }
}
