use crate::{
    DebugPauseReason, DebugSession, SourceBreakpoint, Vm, VmError,
    tests::common::compile_test_bytecode,
};
use kagari_abi::{
    callable::{EngineNativeBinding, NativeCall},
    native_import::EngineNativeOperation,
    standard::RuntimePrimitive,
};
use kagari_bytecode::{BytecodeInstruction, BytecodeProgram, CallTarget, KbcArtifact};
use kagari_common::{cancellation::CancellationToken, host_interface::standard_log};
use kagari_runtime::{
    CapabilitySet, DebugVisibilityPolicy, HostExposurePolicy, LanguageProfile, ResourcePolicy,
    Runtime, RuntimeConfig, RuntimeErrorKind, SecurityContext, gc::GcHeapConfig,
    host::HostFunction, value::Value,
};
use std::{cell::RefCell, rc::Rc};

fn runtime(resources: ResourcePolicy) -> Runtime {
    Runtime::new(RuntimeConfig {
        gc: GcHeapConfig {
            collection_threshold: Some(1),
        },
        security: SecurityContext {
            profile: LanguageProfile {
                allow_host_calls: true,
                allow_debugger: true,
                ..Default::default()
            },
            capabilities: CapabilitySet {
                host_calls: true,
                debug_attach: true,
                debug_breakpoints: true,
                debug_pause: true,
                debug_stack_inspection: true,
                debug_value_inspection: true,
                ..Default::default()
            },
        },
        host_exposure: HostExposurePolicy {
            allowed_host_functions: vec!["host.log".into()],
            ..Default::default()
        },
        debug_visibility: DebugVisibilityPolicy {
            visible_modules: vec!["native-continuation".into()],
            ..Default::default()
        },
        resources,
    })
}

fn route(program: &BytecodeProgram, encoded: bool) -> BytecodeProgram {
    if encoded {
        KbcArtifact::from_bytes(
            &KbcArtifact::from_program(program.clone(), Default::default())
                .unwrap()
                .to_bytes()
                .unwrap(),
        )
        .unwrap()
        .program
    } else {
        program.clone()
    }
}

fn assert_clean(vm: &Vm) {
    assert_eq!(vm.runtime().gc().active_roots(), 0);
    assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
    assert!(!vm.runtime().is_quarantined());
}

#[test]
fn option_fallback_preserves_lazy_effects_and_logical_charges() {
    for (input, expected, steps, effect_steps) in
        [("Some(7)", 7, 11, vec![]), ("None", 42, 14, vec![9])]
    {
        let source = format!(
            "fn main() -> i32 {{ val value: Option<i32> = {input}; value.unwrap_or_else(|| {{ print(\"effect\"); 42 }}) }}"
        );
        let original = compile_test_bytecode(&source);
        let root = &original.modules[original.root.index()];
        let import = root
            .native_imports
            .iter()
            .position(|import| {
                import.resolve()
                    == Some(EngineNativeOperation::Resumable(
                        EngineNativeBinding::Intrinsic(RuntimePrimitive::OptionUnwrapOrElse),
                    ))
            })
            .expect("resumable native binding");
        assert!(root.functions.iter().flat_map(|function| &function.instructions).any(|instruction| matches!(instruction, BytecodeInstruction::Call { callee: CallTarget::Native(id), .. } if id.index() == import)));
        for encoded in [false, true] {
            let program = route(&original, encoded);
            let mut runtime = runtime(Default::default());
            let effects = Rc::new(RefCell::new(Vec::new()));
            let sink = effects.clone();
            runtime
                .register_host_function(HostFunction::new(standard_log(), move |context, _| {
                    sink.borrow_mut()
                        .push(context.runtime().resources().counters().instruction_steps);
                    Ok(Value::Unit)
                }))
                .unwrap();
            let loaded = runtime
                .load_program("native-continuation", program)
                .unwrap();
            let mut vm = Vm::new(runtime);
            assert_eq!(
                vm.execute(&loaded, "main").unwrap().return_value,
                Value::I32(expected)
            );
            assert_eq!(vm.runtime().resources().counters().instruction_steps, steps);
            assert_eq!(*effects.borrow(), effect_steps);
            assert_clean(&vm);
        }
    }
}

#[test]
fn nested_native_callbacks_keep_captures_and_results_alive() {
    let program = compile_test_bytecode(
        r#"
fn main() -> i32 {
    val kept = [40];
    var calls = 0;
    val outer: Option<ArrayList<i32>> = None;
    val result = outer.unwrap_or_else(|| {
        calls = calls + 1;
        val inner: Option<ArrayList<i32>> = None;
        inner.unwrap_or_else(|| { calls = calls + 1; [kept[0].wrapping_add(0), 2] })
    });
    result[0] + result[1] + calls
}

"#,
    );
    for encoded in [false, true] {
        let mut runtime = runtime(Default::default());
        let loaded = runtime
            .load_program("native-continuation", route(&program, encoded))
            .unwrap();
        let mut vm = Vm::new(runtime);
        assert_eq!(
            vm.execute(&loaded, "main").unwrap().return_value,
            Value::I32(44)
        );
        assert_eq!(vm.runtime().resources().counters().peak_call_depth, 3);
        assert!(vm.runtime().gc().stats().collections > 0);
        assert_clean(&vm);
        vm.runtime().collect_garbage().unwrap();
        assert_eq!(vm.runtime().gc().allocated_objects(), 0);
    }
}

#[test]
fn native_storage_constructor_callback_preserves_the_result_root() {
    let program = compile_test_bytecode(
        "fn main() -> i32 { val value: Option<ArrayList<i32>> = None; val result = value.unwrap_or_else(|| { val empty: ArrayList<i32> = ArrayList::new(); empty }); result.push(42); result[0] }",
    );
    for encoded in [false, true] {
        let mut runtime = runtime(Default::default());
        let loaded = runtime
            .load_program("native-continuation", route(&program, encoded))
            .unwrap();
        let mut vm = Vm::new(runtime);
        assert_eq!(
            vm.execute(&loaded, "main").unwrap().return_value,
            Value::I32(42)
        );
        assert_clean(&vm);
    }
}

#[test]
fn every_budget_cut_preserves_the_completed_effect_and_unwinds_continuations() {
    let program = compile_test_bytecode(
        "fn main() -> i32 { val value: Option<i32> = None; value.unwrap_or_else(|| { print(\"effect\"); 42 }) } fn ready() -> i32 { 7 }",
    );
    for encoded in [false, true] {
        for limit in 0..=14 {
            let mut runtime = runtime(Default::default());
            let effects = Rc::new(RefCell::new(0));
            let sink = effects.clone();
            runtime
                .register_host_function(HostFunction::new(standard_log(), move |_, _| {
                    *sink.borrow_mut() += 1;
                    Ok(Value::Unit)
                }))
                .unwrap();
            let loaded = runtime
                .load_program("native-continuation", route(&program, encoded))
                .unwrap();
            let mut options = runtime.execution_options();
            options.resources.max_instruction_steps = Some(limit);
            let session = runtime.begin_execution(&loaded, options).unwrap();
            let mut vm = Vm::new(runtime);
            let result = vm.execute(&loaded, "main");
            if limit < 14 {
                assert!(
                    matches!(result, Err(VmError::RuntimeError(error)) if error.kind() == RuntimeErrorKind::ResourceLimitExceeded),
                    "limit {limit}"
                );
                assert!(vm.execute(&loaded, "ready").is_err());
            } else {
                assert_eq!(result.unwrap().return_value, Value::I32(42));
            }
            assert_eq!(session.counters().instruction_steps, limit);
            assert_eq!(*effects.borrow(), usize::from(limit >= 9));
            assert_clean(&vm);
            drop(session);
            vm.runtime().collect_garbage().unwrap();
            assert_eq!(vm.runtime().gc().allocated_objects(), 0);
            assert_eq!(
                vm.execute(&loaded, "ready").unwrap().return_value,
                Value::I32(7)
            );
        }
    }
}

#[test]
fn callback_depth_failure_precedes_effects_and_cleans_native_roots() {
    for (input, fails) in [("None", true), ("Some(7)", false)] {
        let program = compile_test_bytecode(&format!(
            "fn main() -> i32 {{ val value: Option<i32> = {input}; value.unwrap_or_else(|| {{ print(\"effect\"); 42 }}) }}"
        ));
        for encoded in [false, true] {
            let mut runtime = runtime(ResourcePolicy {
                max_call_depth: Some(1),
                ..Default::default()
            });
            let effects = Rc::new(RefCell::new(0));
            let sink = effects.clone();
            runtime
                .register_host_function(HostFunction::new(standard_log(), move |_, _| {
                    *sink.borrow_mut() += 1;
                    Ok(Value::Unit)
                }))
                .unwrap();
            let loaded = runtime
                .load_program("native-continuation", route(&program, encoded))
                .unwrap();
            let mut vm = Vm::new(runtime);
            let result = vm.execute(&loaded, "main");
            if fails {
                assert!(
                    matches!(result, Err(VmError::RuntimeError(error)) if error.kind() == RuntimeErrorKind::ResourceLimitExceeded)
                );
                assert_eq!(vm.runtime().resources().counters().instruction_steps, 7);
            } else {
                assert_eq!(result.unwrap().return_value, Value::I32(7));
            }
            assert_eq!(*effects.borrow(), 0);
            assert_clean(&vm);
        }
    }
}

#[test]
fn diverging_callbacks_preserve_the_selected_call_contract_and_trap_origin() {
    for input in ["Some(7)", "None"] {
        let program = compile_test_bytecode(&format!(
            "fn main() -> i32 {{ val value: Option<i32> = {input}; value.unwrap_or_else(|| std::debug::panic(\"fallback panic\")) }}"
        ));
        for encoded in [false, true] {
            let mut runtime = runtime(Default::default());
            let loaded = runtime
                .load_program("native-continuation", route(&program, encoded))
                .unwrap();
            let mut vm = Vm::new(runtime);
            let result = vm.execute(&loaded, "main");
            if input == "Some(7)" {
                assert_eq!(result.unwrap().return_value, Value::I32(7));
            } else {
                let error = result.unwrap_err();
                assert!(format!("{error:?}").contains("fallback panic"), "{error:?}");
                assert_eq!(error.trace().unwrap().frames.len(), 2);
            }
            assert_clean(&vm);
        }
    }
}

#[test]
fn cancellation_in_callback_keeps_the_effect_and_is_sticky_only_in_its_session() {
    let program = compile_test_bytecode(
        "fn main() -> i32 { val kept = [42]; val value: Option<i32> = None; value.unwrap_or_else(|| { print(\"cancel\"); kept[0] }) } fn ready() -> i32 { 7 }",
    );
    for encoded in [false, true] {
        let token = CancellationToken::default();
        let cancel = token.clone();
        let effects = Rc::new(RefCell::new(0));
        let sink = effects.clone();
        let mut runtime = runtime(Default::default());
        runtime
            .register_host_function(HostFunction::new(standard_log(), move |_, _| {
                *sink.borrow_mut() += 1;
                cancel.cancel();
                Ok(Value::Unit)
            }))
            .unwrap();
        let loaded = runtime
            .load_program("native-continuation", route(&program, encoded))
            .unwrap();
        let mut options = runtime.execution_options();
        options.cancellation = token;
        let session = runtime.begin_execution(&loaded, options).unwrap();
        let mut vm = Vm::new(runtime);
        assert!(
            matches!(vm.execute(&loaded, "main"), Err(VmError::RuntimeError(error)) if error.kind() == RuntimeErrorKind::Cancelled)
        );
        assert_eq!(*effects.borrow(), 1);
        assert_clean(&vm);
        assert!(vm.execute(&loaded, "ready").is_err());
        drop(session);
        vm.runtime().collect_garbage().unwrap();
        assert_eq!(vm.runtime().gc().allocated_objects(), 0);
        assert_eq!(
            vm.execute(&loaded, "ready").unwrap().return_value,
            Value::I32(7)
        );
    }
}

#[test]
fn callback_trap_preserves_the_public_caller_origin_and_debug_frames() {
    let source = "fn main() -> i32 { val value: Option<i32> = None; value.unwrap_or_else(|| { print(\"effect\"); val n = 2147483647; n + 1 }) }";
    let program = compile_test_bytecode(source);
    let caller_offset = program.modules[program.root.index()]
        .functions
        .iter()
        .find(|function| function.name == "main")
        .unwrap()
        .instructions
        .iter()
        .position(|instruction| {
            matches!(
                instruction,
                BytecodeInstruction::Call {
                    callee: CallTarget::Native(_),
                    ..
                }
            )
        })
        .unwrap();
    for encoded in [false, true] {
        let mut runtime = runtime(Default::default());
        let effects = Rc::new(RefCell::new(0));
        let sink = effects.clone();
        runtime
            .register_host_function(HostFunction::new(standard_log(), move |_, _| {
                *sink.borrow_mut() += 1;
                Ok(Value::Unit)
            }))
            .unwrap();
        let loaded = runtime
            .load_program("native-continuation", route(&program, encoded))
            .unwrap();
        let mut debug = DebugSession::new(&runtime).unwrap();
        let breakpoint = debug
            .add_breakpoint(SourceBreakpoint::at_source_offset(
                "native-continuation",
                source.find("n + 1").unwrap(),
            ))
            .unwrap();
        let mut vm = Vm::new(runtime);
        vm.attach_debug_session(debug).unwrap();
        let error = vm.execute(&loaded, "main").unwrap_err();
        let trace = error.trace().unwrap();
        assert_eq!(trace.frames.len(), 2);
        assert_eq!(trace.frames[1].function_name, "main");
        assert_eq!(trace.frames[1].instruction_offset, caller_offset);
        assert_eq!(trace.frames[1].epoch, loaded.epoch.0);
        assert!(trace.frames[1].source_span.is_some());
        let debug = vm.debug_session().unwrap();
        for reason in [
            DebugPauseReason::Breakpoint(breakpoint),
            DebugPauseReason::Trap,
        ] {
            let pause = debug
                .pauses()
                .iter()
                .find(|pause| {
                    pause.reason == reason
                        && pause
                            .frames
                            .last()
                            .is_some_and(|frame| frame.function_name != "main")
                })
                .expect("callback pause");
            assert_eq!(pause.frames.len(), 2);
            assert_eq!(pause.frames[0].function_name, "main");
            assert_eq!(pause.frames[0].instruction_offset, caller_offset);
            assert!(
                pause.frames[1]
                    .bindings
                    .iter()
                    .any(|binding| binding.name == "n" && binding.value == Value::I32(i32::MAX))
            );
        }
        assert_eq!(*effects.borrow(), 1);
        // Debug snapshots intentionally retain values until the session is replaced.
        drop(debug);
        let empty_debug = DebugSession::new(vm.runtime()).unwrap();
        vm.attach_debug_session(empty_debug).unwrap();
        assert_clean(&vm);
    }
}

#[test]
fn host_reentry_returns_to_its_scope_without_consuming_the_suspended_native_state() {
    let program = compile_test_bytecode(
        r#"
fn main() -> i32 { val kept = [40]; val value: Option<i32> = None; value.unwrap_or_else(|| { print("outer"); kept[0] + 2 }) }
fn inner() -> i32 { val value: Option<i32> = None; value.unwrap_or_else(|| { print("inner"); 7.wrapping_add(0) }) }
fn fail() -> i32 { val value: Option<i32> = None; value.unwrap_or_else(|| { val n = 2147483647; n + 1 }) }
"#,
    );
    let root = &program.modules[program.root.index()];
    let inner = root
        .functions
        .iter()
        .find(|function| function.name == "inner")
        .unwrap()
        .id;
    let fail = root
        .functions
        .iter()
        .find(|function| function.name == "fail")
        .unwrap()
        .id;
    for encoded in [false, true] {
        let effects = Rc::new(RefCell::new(Vec::new()));
        let sink = effects.clone();
        let mut runtime = runtime(Default::default());
        runtime
            .register_host_function(HostFunction::new(standard_log(), move |context, args| {
                sink.borrow_mut().push(args[0].clone());
                let root = context.runtime().execution_root().unwrap();
                if args == [Value::Str("outer".into())] {
                    assert_eq!(
                        crate::reenter(context, &root, inner, &[]).unwrap().value(),
                        Value::I32(7)
                    );
                    assert!(crate::reenter(context, &root, fail, &[]).is_err());
                    assert_eq!(
                        context.runtime().resources().counters().current_call_depth,
                        2
                    );
                } else {
                    assert_eq!(
                        context.runtime().resources().counters().current_call_depth,
                        4
                    );
                }
                context.runtime().collect_garbage().unwrap();
                Ok(Value::Unit)
            }))
            .unwrap();
        let loaded = runtime
            .load_program("native-continuation", route(&program, encoded))
            .unwrap();
        let mut vm = Vm::new(runtime);
        assert_eq!(
            vm.execute(&loaded, "main").unwrap().return_value,
            Value::I32(42)
        );
        assert_eq!(
            *effects.borrow(),
            [Value::Str("outer".into()), Value::Str("inner".into())]
        );
        assert_clean(&vm);
    }
}

#[test]
fn native_callbacks_invoke_the_retained_closure_generation_after_replacement() {
    let program = |number| {
        compile_test_bytecode(&format!(
            r#"
fn make() -> fn() -> i32 {{ || {{ val value: Option<i32> = None; value.unwrap_or_else(|| {number}.wrapping_add(0)) }} }}
fn consume(callback: fn() -> i32) -> i32 {{ val value: Option<i32> = None; value.unwrap_or_else(callback) }}
fn main() -> i32 {{ print("invoke"); 0 }}
"#
        ))
    };
    let old_program = program(41);
    let new_program = program(42);
    let consume = new_program.modules[new_program.root.index()]
        .functions
        .iter()
        .find(|function| function.name == "consume")
        .unwrap()
        .id;
    for encoded in [false, true] {
        let retained = Rc::new(RefCell::new(None::<Value>));
        let callback = retained.clone();
        let observed = Rc::new(RefCell::new(Vec::new()));
        let sink = observed.clone();
        let mut runtime = runtime(Default::default());
        runtime
            .register_host_function(HostFunction::new(standard_log(), move |context, _| {
                let value = callback.borrow().as_ref().unwrap().clone();
                let root = context.runtime().execution_root().unwrap();
                let result = crate::reenter(context, &root, consume, &[value]).unwrap();
                sink.borrow_mut().push(result.value());
                context.runtime().collect_garbage().unwrap();
                Ok(Value::Unit)
            }))
            .unwrap();
        let old = runtime
            .load_program("native-continuation", route(&old_program, encoded))
            .unwrap();
        let old_key = old.key();
        let mut vm = Vm::new(runtime);
        let closure = vm.execute(&old, "make").unwrap().return_value;
        let rooted = vm.runtime().root_value(closure.clone()).unwrap();
        *retained.borrow_mut() = Some(closure);
        let new = vm
            .reload_program(&old, "native-continuation", route(&new_program, encoded))
            .unwrap();
        drop(old);
        vm.runtime().collect_garbage().unwrap();
        assert_eq!(
            vm.runtime()
                .resolve_closure(&rooted.value())
                .unwrap()
                .implementation
                .key(),
            old_key
        );
        assert_ne!(new.key(), old_key);
        assert_eq!(
            vm.execute(&new, "main").unwrap().return_value,
            Value::I32(0)
        );
        assert_eq!(*observed.borrow(), [Value::I32(41)]);
        retained.borrow_mut().take();
        drop(rooted);
        assert_clean(&vm);
        vm.runtime().collect_garbage().unwrap();
        assert_eq!(vm.runtime().gc().allocated_objects(), 0);
    }
}
