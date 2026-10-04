//! Retained control-boundary behavior using ordinary synchronous application natives.
use kagari_bytecode::{
    artifact::KbcArtifact,
    instruction::{BytecodeInstruction, CallTarget},
    program::BytecodeProgram,
};
use kagari_common::cancellation::CancellationToken;
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_hir::{analysis::AnalysisDatabase, host::HostDeclarations};
use kagari_runtime::{
    Runtime, RuntimeConfig,
    error::RuntimeErrorKind,
    gc::GcHeapConfig,
    host::HostFunction,
    native::{
        binding::NativeResult, builder::ModuleBuilder, callable::CallableHandle,
        context::CallContext, declarations::FunctionDecl, module::NativeModule, types::Type,
    },
    resource::RuntimeLimits,
    value::Value,
};
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_stdlib::{catalog as foundation_catalog, declarations::StandardDeclarations};
use kagari_types::host_interface::{HostInterface, standard_log};
use kagari_vm::{
    debug::{DebugPauseReason, DebugSession, SourceBreakpoint},
    error::VmError,
    reentry::reenter,
    vm::Vm,
};
use std::{cell::RefCell, rc::Rc, sync::Arc};

fn module() -> NativeModule {
    let mut module = ModuleBuilder::new(
        "test::boundary",
        &StandardDeclarations::default()
            .catalog()
            .expect("explicit standard providers"),
    );
    let choose = module
        .define_function(
            FunctionDecl::new("choose")
                .parameter("run", Type::bool())
                .parameter("callback", Type::function([], Type::i32()))
                .returns(Type::i32()),
        )
        .unwrap();
    module
        .bind(
            choose,
            |cx: &mut CallContext<'_>,
             run: bool,
             callback: CallableHandle<'_>|
             -> NativeResult<i32> { if run { callback.call(cx, ()) } else { Ok(7) } },
        )
        .unwrap();
    module.finish().unwrap()
}

fn compile_test_bytecode(text: &str) -> BytecodeProgram {
    let native = module();
    let mut sources = SourceDatabase::default();
    let file = sources
        .set(
            "test.kgr",
            format!("{text}\nuse test::boundary;"),
            SourceLayer::Base,
        )
        .unwrap();
    let mut analysis = AnalysisDatabase::default();
    analysis.set_native_modules(
        foundation_catalog::shared()
            .into_iter()
            .chain([Arc::new(native.to_declaration().unwrap())])
            .collect(),
    );
    analysis.set_host_declarations(
        HostDeclarations::new(HostInterface {
            paths: vec![],
            types: vec![],
            functions: vec![standard_log()],
        })
        .unwrap(),
    );
    let snapshot = analysis
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let checked = snapshot.check_program(file, &Default::default()).unwrap();
    let mir = lower_program_to_mir(&checked, &Default::default()).unwrap();
    lower_program_to_bytecode(&mir).unwrap()
}

fn runtime(limits: RuntimeLimits) -> Runtime {
    let mut runtime = Runtime::new(RuntimeConfig {
        gc: GcHeapConfig {
            collection_threshold: Some(1),
        },

        limits,
    });
    kagari_runtime::native::module::NativeModule::install_all(
        &kagari_stdlib::modules().unwrap(),
        &mut runtime,
    )
    .unwrap();
    module().install(&mut runtime).unwrap();
    runtime
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
fn callback_depth_failure_precedes_effects_and_cleans_native_roots() {
    for (input, fails) in [("true", true), ("false", false)] {
        let program = compile_test_bytecode(&format!(
            "fn main() -> i32 {{ boundary::choose({input}, || {{ host::log(\"effect\"); 42 }}) }}"
        ));
        for encoded in [false, true] {
            let mut runtime = runtime(RuntimeLimits {
                max_call_depth: Some(1),
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
                .load_program("native-control", route(&program, encoded))
                .unwrap();
            let mut vm = Vm::new(runtime);
            let result = vm.execute(&loaded, "main");
            if fails {
                assert!(
                    matches!(result, Err(VmError::RuntimeError(error)) if error.kind() == RuntimeErrorKind::ResourceLimitExceeded)
                );
            } else {
                assert_eq!(result.unwrap().return_value, Value::I32(7));
            }
            assert_eq!(*effects.borrow(), 0);
            assert_clean(&vm);
        }
    }
}

#[test]
fn cancellation_in_callback_keeps_the_effect_and_is_sticky_only_in_its_session() {
    let program = compile_test_bytecode(
        "fn main() -> i32 { val kept = [42]; boundary::choose(true, || { host::log(\"cancel\"); kept[0] }) } fn ready() -> i32 { 7 }",
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
            .load_program("native-control", route(&program, encoded))
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
    let source = "fn main() -> i32 { boundary::choose(true, || { host::log(\"effect\"); val n = 2147483647; n + 1 }) }";
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
            .load_program("native-control", route(&program, encoded))
            .unwrap();
        let mut debug = DebugSession::new(&runtime).unwrap();
        let breakpoint = debug
            .add_breakpoint(SourceBreakpoint::at_source_offset(
                "native-control",
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
fn host_reentry_returns_to_the_synchronous_native_caller() {
    let program = compile_test_bytecode(
        r#"
fn main() -> i32 { val kept = [40]; boundary::choose(true, || { host::log("outer"); kept[0] + 2 }) }
fn inner() -> i32 { boundary::choose(true, || { host::log("inner"); 7 }) }
fn fail() -> i32 { boundary::choose(true, || { val n = 2147483647; n + 1 }) }
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
                        reenter(context, &root, inner, &[]).unwrap().value(),
                        Value::I32(7)
                    );
                    assert!(reenter(context, &root, fail, &[]).is_err());
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
            .load_program("native-control", route(&program, encoded))
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
fn make() -> fn() -> i32 {{ || {{ boundary::choose(true, || {number}) }} }}
fn consume(callback: fn() -> i32) -> i32 {{ boundary::choose(true, callback) }}
fn main() -> i32 {{ host::log("invoke"); 0 }}
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
                let result = reenter(context, &root, consume, &[value]).unwrap();
                sink.borrow_mut().push(result.value());
                context.runtime().collect_garbage().unwrap();
                Ok(Value::Unit)
            }))
            .unwrap();
        let old = runtime
            .load_program("native-control", route(&old_program, encoded))
            .unwrap();
        let old_key = old.key();
        let mut vm = Vm::new(runtime);
        let closure = vm.execute(&old, "make").unwrap().return_value;
        let rooted = vm.runtime().root_value(closure.clone()).unwrap();
        *retained.borrow_mut() = Some(closure);
        let new = vm
            .reload_program(&old, "native-control", route(&new_program, encoded))
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

#[test]
fn cancellation_at_observed_boundaries_cleans_callback_scopes() {
    let program = compile_test_bytecode(
        "fn main() -> i32 { boundary::choose(true, || { host::log(\"effect\"); 42 }) } fn ready() -> i32 { 7 }",
    );
    for encoded in [false, true] {
        let mut cancellations = 0;
        for at in 0..80 {
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
                .load_program("native-control", route(&program, encoded))
                .unwrap();
            let options = runtime.execution_options();
            let observer = Rc::new(crate::support::CancelAt {
                seen: Default::default(),
                at,
                token: options.cancellation.clone(),
            });
            let session = runtime.begin_execution(&loaded, options).unwrap();
            runtime.attach_execution_observer(observer.clone()).unwrap();
            let mut vm = Vm::new(runtime);
            match vm.execute(&loaded, "main") {
                Ok(report) => {
                    assert_eq!(report.return_value, Value::I32(42));
                    assert_eq!(*effects.borrow(), 1);
                }
                Err(error) => {
                    cancellations += 1;
                    assert!(
                        matches!(error.cause(), VmError::RuntimeError(error) if error.kind() == RuntimeErrorKind::Cancelled)
                    );
                    assert!(vm.execute(&loaded, "ready").is_err());
                }
            }
            assert!(*effects.borrow() <= 1);
            assert_clean(&vm);
            drop(session);
            assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
            assert_eq!(
                vm.execute(&loaded, "ready").unwrap().return_value,
                Value::I32(7)
            );
            if observer.seen.get() <= at {
                break;
            }
        }
        assert!(cancellations > 5);
    }
}

#[test]
fn native_poll_observes_cancellation_and_return_cannot_swallow_it() {
    for swallow in [false, true] {
        let token = CancellationToken::default();
        let cancel = token.clone();
        let visited = Rc::new(std::cell::Cell::new(0));
        let observed = visited.clone();
        let mut native = ModuleBuilder::new(
            "test::polling",
            &StandardDeclarations::default()
                .catalog()
                .expect("explicit standard providers"),
        );
        let work = native
            .define_function(FunctionDecl::new("work").returns(Type::i32()))
            .unwrap();
        native
            .bind(work, move |cx: &mut CallContext<'_>| -> NativeResult<i32> {
                for index in 0..8 {
                    if index == 3 {
                        cancel.cancel();
                    }
                    if let Err(error) = cx.poll() {
                        return if swallow { Ok(42) } else { Err(error) };
                    }
                    observed.set(observed.get() + 1);
                }
                Ok(42)
            })
            .unwrap();
        let native = native.finish().unwrap();
        let program = super::compile_program(
            "use test::polling; fn main() -> i32 { polling::work() }",
            Some(&native),
        );
        let mut runtime = Runtime::default();
        kagari_runtime::native::module::NativeModule::install_all(
            &kagari_stdlib::modules().unwrap(),
            &mut runtime,
        )
        .unwrap();
        native.install(&mut runtime).unwrap();
        let loaded = runtime.load_program("native-poll", program).unwrap();
        let mut options = runtime.execution_options();
        options.cancellation = token;
        let session = runtime.begin_execution(&loaded, options).unwrap();
        let mut vm = Vm::new(runtime);
        let error = vm.execute(&loaded, "main").unwrap_err();
        assert!(
            matches!(error.cause(), VmError::RuntimeError(error) if error.kind() == RuntimeErrorKind::Cancelled)
        );
        assert_eq!(visited.get(), 3);
        assert_eq!(vm.runtime().gc().active_roots(), 0);
        assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
        drop(session);
        assert!(vm.runtime().resources().poll_execution().is_ok());
    }
}
