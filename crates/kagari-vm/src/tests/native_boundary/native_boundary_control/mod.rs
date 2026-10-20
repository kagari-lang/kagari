//! Retained control-boundary behavior using ordinary synchronous application natives.
use crate::tests::native_boundary::{publish_unused_version, support::round_trip_program};
use crate::{
    debug::{DebugPauseReason, DebugSession, SourceBreakpoint},
    error::VmError,
    reentry::reenter,
    vm::Vm,
};
use kagari_bytecode::{
    instruction::{BytecodeInstruction, CallTarget},
    program::BytecodeProgram,
};
use kagari_common::{cancellation::CancellationToken, identity::metadata::DefinitionMetadata};
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_hir::analysis::AnalysisDatabase;
use kagari_runtime::{
    Runtime, RuntimeConfig,
    error::RuntimeErrorKind,
    gc::GcHeapConfig,
    host::{HostCallContext, HostError},
    module::entry::ProgramEntry,
    native::{
        binding::NativeResult, builder::ModuleBuilder, callable::CallableHandle,
        context::CallContext, declarations::FunctionDecl, module::NativeModule, types::Type,
    },
    resource::RuntimeLimits,
    value::Value,
};
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_stdlib::{catalog as foundation_catalog, declarations::StandardDeclarations};
use kagari_types::declaration::logging;
use std::sync::Arc;
use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};

fn logging_module(
    entry: impl for<'call> Fn(&HostCallContext<'call>, &[Value]) -> Result<Value, HostError>
    + Send
    + Sync
    + 'static,
) -> NativeModule {
    let mut builder = ModuleBuilder::from_declaration(
        logging::declaration().unwrap(),
        &StandardDeclarations::default().catalog().unwrap(),
        Default::default(),
    )
    .unwrap();
    let log = builder.declared_function("log").unwrap();
    builder.bind(log, entry).unwrap();
    builder.finish().unwrap()
}

fn module() -> NativeModule {
    let mut module = ModuleBuilder::new(
        "test::boundary",
        &StandardDeclarations::default()
            .catalog()
            .expect("explicit standard providers"),
    )
    .unwrap();
    let choose = module
        .define_function(
            FunctionDecl::new("choose")
                .parameter("run", Type::bool())
                .parameter("callback", Type::function([], Type::i32()).unwrap())
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

fn compile_test_bytecode(text: &str) -> DefinitionMetadata<BytecodeProgram> {
    let native = module();
    let mut sources = SourceDatabase::default();
    let file = sources
        .set(
            "test.kgr",
            format!("{text}\nuse test::boundary; use host;"),
            SourceLayer::Base,
        )
        .unwrap();
    let mut analysis = AnalysisDatabase::default();
    analysis.set_native_modules(
        foundation_catalog::shared()
            .unwrap()
            .into_iter()
            .chain([
                Arc::new(native.to_declaration()),
                Arc::new(logging::declaration().unwrap()),
            ])
            .collect(),
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
            ..Default::default()
        },

        limits,
        ..Default::default()
    });
    kagari_runtime::native::module::NativeModule::install_all(
        &kagari_stdlib::modules().unwrap(),
        &mut runtime,
    )
    .unwrap();
    module().install(&mut runtime).unwrap();
    runtime
}

fn route(
    program: &DefinitionMetadata<BytecodeProgram>,
    encoded: bool,
) -> DefinitionMetadata<BytecodeProgram> {
    if encoded {
        round_trip_program(program.clone())
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
            let effects = Arc::new(Mutex::new(0));
            let sink = effects.clone();
            logging_module(move |_, _| {
                *sink.lock().unwrap() += 1;
                Ok(Value::Unit)
            })
            .install(&mut runtime)
            .unwrap();
            let loaded = runtime
                .load_program("native-control", route(&program, encoded))
                .unwrap();
            let vm = Vm::new(runtime);
            let result = vm.execute(&loaded, ProgramEntry::Main);
            if fails {
                assert!(
                    matches!(result, Err(VmError::RuntimeError(error)) if error.kind() == RuntimeErrorKind::ResourceLimitExceeded)
                );
            } else {
                assert_eq!(
                    result
                        .unwrap()
                        .return_value
                        .value(vm.runtime().gc())
                        .expect("retained execution result"),
                    Value::I32(7)
                );
            }
            assert_eq!(*effects.lock().unwrap(), 0);
            assert_clean(&vm);
        }
    }
}

#[test]
fn cancellation_in_callback_keeps_the_effect_and_is_sticky_only_in_its_session() {
    let program = compile_test_bytecode(
        "fn main() -> i32 { val kept = [42]; boundary::choose(true, || { host::log(\"cancel\"); kept[0] }) } pub fn ready() -> i32 { 7 }",
    );
    for encoded in [false, true] {
        let token = CancellationToken::default();
        let cancel = token.clone();
        let effects = Arc::new(Mutex::new(0));
        let sink = effects.clone();
        let mut runtime = runtime(Default::default());
        logging_module(move |_, _| {
            *sink.lock().unwrap() += 1;
            cancel.cancel();
            Ok(Value::Unit)
        })
        .install(&mut runtime)
        .unwrap();
        let loaded = runtime
            .load_program("native-control", route(&program, encoded))
            .unwrap();
        let mut options = runtime.execution_options();
        options.cancellation = token;
        let vm = Vm::new(runtime);
        let session = vm.runtime().begin_execution(&loaded, options).unwrap();

        assert!(
            matches!(vm.execute(&loaded, ProgramEntry::Main), Err(VmError::RuntimeError(error)) if error.kind() == RuntimeErrorKind::Cancelled)
        );
        assert_eq!(*effects.lock().unwrap(), 1);
        assert_clean(&vm);
        assert!(vm.execute(&loaded, "ready").is_err());
        drop(session);
        vm.runtime().collect_garbage().unwrap();
        assert_eq!(
            vm.execute(&loaded, "ready")
                .unwrap()
                .return_value
                .value(vm.runtime().gc())
                .expect("retained execution result"),
            Value::I32(7)
        );
        // Check a fresh session on the same version before retiring its constants.
        publish_unused_version(&vm, &loaded);
        assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
    }
}

#[test]
fn callback_trap_preserves_the_public_caller_origin_and_debug_frames() {
    let source = "fn main() -> i32 { boundary::choose(true, || { host::log(\"effect\"); val n = 2147483647; n + 1 }) }";
    let program = compile_test_bytecode(source);
    let caller_offset = program.records().modules[program.records().root.index()]
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
        let effects = Arc::new(Mutex::new(0));
        let sink = effects.clone();
        logging_module(move |_, _| {
            *sink.lock().unwrap() += 1;
            Ok(Value::Unit)
        })
        .install(&mut runtime)
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
        let error = vm.execute(&loaded, ProgramEntry::Main).unwrap_err();
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
        assert_eq!(*effects.lock().unwrap(), 1);
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
    let root = &program.records().modules[program.records().root.index()];
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
        let effects = Arc::new(Mutex::new(Vec::new()));
        let sink = effects.clone();
        let mut runtime = runtime(Default::default());
        logging_module(move |context, args| {
            let [Value::Str(id)] = args else {
                panic!("log string")
            };
            let text = context.runtime().gc().string(*id).unwrap().to_owned();
            sink.lock().unwrap().push(text.clone());
            let root = context.runtime().execution_root().unwrap();
            if text == "outer" {
                assert_eq!(
                    reenter(context, &root, inner, &[])
                        .unwrap()
                        .value(context.runtime().gc())
                        .unwrap(),
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
        })
        .install(&mut runtime)
        .unwrap();
        let loaded = runtime
            .load_program("native-control", route(&program, encoded))
            .unwrap();
        let vm = Vm::new(runtime);
        assert_eq!(
            vm.execute(&loaded, ProgramEntry::Main)
                .unwrap()
                .return_value
                .value(vm.runtime().gc())
                .expect("retained execution result"),
            Value::I32(42)
        );
        assert_eq!(*effects.lock().unwrap(), ["outer", "inner"]);
        assert_clean(&vm);
    }
}

#[test]
fn native_callbacks_invoke_the_retained_closure_generation_after_replacement() {
    let program = |number| {
        compile_test_bytecode(&format!(
            r#"
pub fn make() -> fn() -> i32 {{ || {{ boundary::choose(true, || {number}) }} }}
fn consume(callback: fn() -> i32) -> i32 {{ boundary::choose(true, callback) }}
fn main() -> i32 {{ host::log("invoke"); 0 }}
"#
        ))
    };
    let old_program = program(41);
    let new_program = program(42);
    let consume = new_program.records().modules[new_program.records().root.index()]
        .functions
        .iter()
        .find(|function| function.name == "consume")
        .unwrap()
        .id;
    for encoded in [false, true] {
        let retained = Arc::new(Mutex::new(None::<Value>));
        let callback = retained.clone();
        let observed = Arc::new(Mutex::new(Vec::new()));
        let sink = observed.clone();
        let mut runtime = runtime(Default::default());
        logging_module(move |context, _| {
            let value = *callback.lock().unwrap().as_ref().unwrap();
            let root = context.runtime().execution_root().unwrap();
            let result = reenter(context, &root, consume, &[value]).unwrap();
            sink.lock()
                .unwrap()
                .push(result.value(context.runtime().gc()).unwrap());
            context.runtime().collect_garbage().unwrap();
            Ok(Value::Unit)
        })
        .install(&mut runtime)
        .unwrap();
        let old = runtime
            .load_program("native-control", route(&old_program, encoded))
            .unwrap();
        let old_key = old.key();
        let vm = Vm::new(runtime);
        let closure = vm
            .execute(&old, "make")
            .unwrap()
            .return_value
            .value(vm.runtime().gc())
            .expect("retained execution result");
        let rooted = vm.runtime().root_value(closure).unwrap();
        *retained.lock().unwrap() = Some(closure);
        let new = vm
            .reload_program(&old, "native-control", route(&new_program, encoded))
            .unwrap();
        drop(old);
        vm.runtime().collect_garbage().unwrap();
        assert_eq!(
            vm.runtime()
                .resolve_closure(&rooted.value(vm.runtime().gc()).unwrap())
                .unwrap()
                .implementation
                .key(),
            old_key
        );
        assert_ne!(new.key(), old_key);
        assert_eq!(
            vm.execute(&new, ProgramEntry::Main)
                .unwrap()
                .return_value
                .value(vm.runtime().gc())
                .expect("retained execution result"),
            Value::I32(0)
        );
        assert_eq!(*observed.lock().unwrap(), [Value::I32(41)]);
        retained.lock().unwrap().take();
        drop(rooted);
        assert_clean(&vm);
        publish_unused_version(&vm, &new);
        vm.runtime().collect_garbage().unwrap();
        assert_eq!(vm.runtime().gc().allocated_objects(), 0);
    }
}

#[test]
fn cancellation_at_observed_boundaries_cleans_callback_scopes() {
    let program = compile_test_bytecode(
        "fn main() -> i32 { boundary::choose(true, || { host::log(\"effect\"); 42 }) } pub fn ready() -> i32 { 7 }",
    );
    for encoded in [false, true] {
        let mut cancellations = 0;
        for at in 0..80 {
            let mut runtime = runtime(Default::default());
            let effects = Arc::new(Mutex::new(0));
            let sink = effects.clone();
            logging_module(move |_, _| {
                *sink.lock().unwrap() += 1;
                Ok(Value::Unit)
            })
            .install(&mut runtime)
            .unwrap();
            let loaded = runtime
                .load_program("native-control", route(&program, encoded))
                .unwrap();
            let options = runtime.execution_options();
            runtime
                .set_execution_observer(crate::tests::native_boundary::support::CancelAt {
                    seen: Default::default(),
                    at,
                    token: options.cancellation.clone(),
                })
                .unwrap();
            let vm = Vm::new(runtime);
            let session = vm.runtime().begin_execution(&loaded, options).unwrap();
            vm.runtime().attach_execution_observer().unwrap();
            match vm.execute(&loaded, ProgramEntry::Main) {
                Ok(report) => {
                    assert_eq!(
                        report
                            .return_value
                            .value(vm.runtime().gc())
                            .expect("retained execution result"),
                        Value::I32(42)
                    );
                    assert_eq!(*effects.lock().unwrap(), 1);
                }
                Err(error) => {
                    cancellations += 1;
                    assert!(
                        matches!(error.cause(), VmError::RuntimeError(error) if error.kind() == RuntimeErrorKind::Cancelled)
                    );
                    assert!(vm.execute(&loaded, "ready").is_err());
                }
            }
            assert!(*effects.lock().unwrap() <= 1);
            assert_clean(&vm);
            drop(session);
            vm.runtime().collect_garbage().unwrap();
            assert_eq!(
                vm.execute(&loaded, "ready")
                    .unwrap()
                    .return_value
                    .value(vm.runtime().gc())
                    .expect("retained execution result"),
                Value::I32(7)
            );
            publish_unused_version(&vm, &loaded);
            assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
            if vm
                .runtime()
                .execution_observer::<crate::tests::native_boundary::support::CancelAt>()
                .unwrap()
                .seen
                .get()
                <= at
            {
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
        let visited = Arc::new(AtomicUsize::new(0));
        let observed = visited.clone();
        let mut native = ModuleBuilder::new(
            "test::polling",
            &StandardDeclarations::default()
                .catalog()
                .expect("explicit standard providers"),
        )
        .unwrap();
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
                    observed.fetch_add(1, Ordering::SeqCst);
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
        let vm = Vm::new(runtime);
        let session = vm.runtime().begin_execution(&loaded, options).unwrap();

        let error = vm.execute(&loaded, ProgramEntry::Main).unwrap_err();
        assert!(
            matches!(error.cause(), VmError::RuntimeError(error) if error.kind() == RuntimeErrorKind::Cancelled)
        );
        assert_eq!(visited.load(Ordering::SeqCst), 3);
        assert_eq!(vm.runtime().gc().active_roots(), 0);
        assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
        drop(session);
        assert!(vm.runtime().resources().poll_execution().is_ok());
    }
}
