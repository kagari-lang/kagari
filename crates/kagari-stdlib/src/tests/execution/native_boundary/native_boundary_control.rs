//! Retained control-boundary behavior using ordinary synchronous application natives.
use crate::tests::{
    execution::{
        common::{compile_native_test_bytecode, logging_module, round_trip_bytecode},
        native_boundary::{publish_unused_version, support::CancelAt},
    },
    support::bytecode::reload_bytecode,
};

#[cfg(test)]
use crate::tests::support::bytecode::load_bytecode;

use kagari_runtime::{
    Runtime, RuntimeConfig,
    debug::{DebugPauseReason, DebugSession, SourceBreakpoint},
    error::RuntimeErrorKind,
    execution::memory::GcHeapConfig,
    execution::reentry::reenter,
    module::entry::ProgramEntry,
    native::{
        binding::NativeResult, builder::ModuleBuilder, callable::CallableHandle,
        context::NativeCall, conversion::context::ConversionContext, declarations::FunctionDecl,
        host, installation::NativeInstallation, module::NativeModule, types::Type,
    },
    resource::RuntimeLimits,
    value::{carriers::RootedValue, view::ValueView},
};

use kagari_bytecode::{
    instruction::{BytecodeInstruction, CallTarget},
    program::BytecodeProgram,
};
use kagari_common::{cancellation::CancellationToken, identity::metadata::DefinitionMetadata};

use kagari_types::declaration::logging;
use std::sync::{Arc, Mutex};

fn module() -> NativeModule {
    let mut module = ModuleBuilder::new("test::boundary", &Default::default()).unwrap();
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
            |cx: &mut NativeCall<'_>,
             run: bool,
             callback: CallableHandle<'_>|
             -> NativeResult<i32> { if run { callback.call(cx, ()) } else { Ok(7) } },
        )
        .unwrap();
    module.finish().unwrap()
}

fn compile_test_bytecode(text: &str) -> DefinitionMetadata<BytecodeProgram> {
    let native = module();
    let logging = logging_module(|_, _| Ok(ValueView::unit()));
    compile_native_test_bytecode(text, &[&native, &logging])
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
    NativeModule::install_all(&crate::modules().unwrap(), &mut runtime).unwrap();
    module().install(&mut runtime).unwrap();
    runtime
}

fn route(
    program: &DefinitionMetadata<BytecodeProgram>,
    encoded: bool,
) -> DefinitionMetadata<BytecodeProgram> {
    if encoded {
        round_trip_bytecode(program.clone())
    } else {
        program.clone()
    }
}

fn assert_clean(vm: &Runtime) {
    assert_eq!(vm.resources().counters().current_call_depth, 0);
    assert!(!vm.is_quarantined());
}

#[test]
fn callback_depth_failure_precedes_effects_and_cleans_native_roots() {
    for (input, fails) in [("true", true), ("false", false)] {
        let program = compile_test_bytecode(&format!(
            "use test::boundary; use host; fn main() -> i32 {{ boundary::choose({input}, || {{ host::log(\"effect\"); 42 }}) }}"
        ));
        for encoded in [false, true] {
            let mut runtime = runtime(RuntimeLimits {
                max_call_depth: Some(2),
            });
            let effects = Arc::new(Mutex::new(0));
            let sink = effects.clone();
            logging_module(move |_, _| {
                *sink.lock().unwrap() += 1;
                Ok(ValueView::unit())
            })
            .install(&mut runtime)
            .unwrap();
            let loaded =
                load_bytecode(&mut runtime, "native-control", route(&program, encoded)).unwrap();
            let _loaded_program = loaded;
            let loaded = runtime.select_program(&_loaded_program).unwrap();
            let mut vm = runtime;
            vm.collect_garbage().unwrap();
            let baseline = vm.gc_stats();
            let result = vm.execute_module(&loaded, ProgramEntry::Main, vm.execution_options());
            if fails {
                assert!(
                    matches!(result, Err(error) if error.kind() == RuntimeErrorKind::ResourceLimitExceeded)
                );
            } else {
                assert_eq!(result.unwrap().return_value.as_i32(), Some(7));
            }
            assert_eq!(*effects.lock().unwrap(), 0);
            assert_clean(&vm);
            vm.collect_garbage().unwrap();
            assert_eq!(
                vm.gc_stats().root_registrations,
                baseline.root_registrations
            );
            assert_eq!(vm.gc_stats().objects, baseline.objects);
        }
    }
}

#[test]
fn cancellation_in_callback_keeps_the_effect_and_is_sticky_only_in_its_session() {
    let program = compile_test_bytecode(include_str!(
        "../../../../tests/fixtures/native/native-control-cancellation.kgr"
    ));
    for encoded in [false, true] {
        let token = CancellationToken::default();
        let cancel = token.clone();
        let effects = Arc::new(Mutex::new(0));
        let sink = effects.clone();
        let mut runtime = runtime(Default::default());
        logging_module(move |_, _| {
            *sink.lock().unwrap() += 1;
            cancel.cancel();
            Ok(ValueView::unit())
        })
        .install(&mut runtime)
        .unwrap();
        let loaded =
            load_bytecode(&mut runtime, "native-control", route(&program, encoded)).unwrap();
        let _loaded_program = loaded;
        let loaded = runtime.select_program(&_loaded_program).unwrap();
        runtime.collect_garbage().unwrap();
        let baseline = runtime.gc_stats();
        let mut options = runtime.execution_options();
        options.cancellation = token;
        let mut vm = runtime;
        let mut session = vm.begin_execution(&loaded, options).unwrap();
        let session_options = session.runtime().execution_options();

        assert!(
            matches!(session.runtime_mut().execute_module(&loaded, ProgramEntry::Main, session_options.clone()), Err(error) if error.kind() == RuntimeErrorKind::Cancelled)
        );
        assert_eq!(*effects.lock().unwrap(), 1);
        assert_clean(session.runtime());
        assert!(
            session
                .runtime_mut()
                .execute_module(&loaded, "ready", session_options.clone())
                .is_err()
        );
        drop(session);
        vm.collect_garbage().unwrap();
        assert_eq!(
            vm.execute_module(&loaded, "ready", vm.execution_options())
                .unwrap()
                .return_value
                .as_i32(),
            Some(7)
        );
        // Check a fresh session on the same version before retiring its constants.
        publish_unused_version(&mut vm, &_loaded_program);
        vm.collect_garbage().unwrap();
        assert_eq!(vm.gc_stats().objects, baseline.objects);
        assert_eq!(
            vm.gc_stats().root_registrations,
            baseline.root_registrations
        );
    }
}

#[test]
fn callback_trap_preserves_the_public_caller_origin_and_debug_frames() {
    let source = include_str!("../../../../tests/fixtures/native/native-control-trap.kgr");
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
            Ok(ValueView::unit())
        })
        .install(&mut runtime)
        .unwrap();
        let loaded =
            load_bytecode(&mut runtime, "native-control", route(&program, encoded)).unwrap();
        let _loaded_program = loaded;
        let loaded = runtime.select_program(&_loaded_program).unwrap();
        runtime.collect_garbage().unwrap();
        let baseline = runtime.gc_stats();
        let mut debug = DebugSession::new(&runtime).unwrap();
        let breakpoint = debug
            .add_breakpoint(SourceBreakpoint::at_source_offset(
                "native-control",
                source.find("n + 1").unwrap(),
            ))
            .unwrap();
        let mut vm = runtime;
        vm.attach_debug_session(debug).unwrap();
        let error = vm
            .execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
            .unwrap_err();
        let trace = error.trace().unwrap();
        assert_eq!(trace.frames.len(), 3);
        assert_eq!(trace.frames[1].function_name, "choose");
        assert_eq!(trace.frames[2].function_name, "main");
        assert_eq!(trace.frames[2].instruction_offset, caller_offset);
        assert_eq!(trace.frames[2].epoch, loaded.epoch.0);
        assert!(trace.frames[2].source_span.is_some());
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
            assert_eq!(pause.frames.len(), 3);
            assert_eq!(pause.frames[1].function_name, "choose");
            assert_eq!(pause.frames[0].function_name, "main");
            assert_eq!(pause.frames[0].instruction_offset, caller_offset);
            assert!(
                pause.frames[2]
                    .bindings
                    .iter()
                    .any(|binding| binding.name == "n" && binding.value.as_i32() == Some(i32::MAX))
            );
        }
        assert_eq!(*effects.lock().unwrap(), 1);
        // Debug snapshots intentionally retain values until the session is replaced.
        drop(debug);
        let empty_debug = DebugSession::new(&vm).unwrap();
        vm.attach_debug_session(empty_debug).unwrap();
        assert_clean(&vm);
        // Current publication owns warmed String constants independently of call
        // locals. Replace it before asserting that execution roots are reclaimed.
        publish_unused_version(&mut vm, &_loaded_program);
        vm.collect_garbage().unwrap();
        assert!(vm.validate_executable_module(&loaded).is_err());
        assert_eq!(
            vm.gc_stats().root_registrations,
            baseline.root_registrations
        );
        assert_eq!(vm.gc_stats().objects, baseline.objects);
    }
}

#[test]
fn host_reentry_returns_to_the_synchronous_native_caller() {
    let program = compile_test_bytecode(include_str!(
        "../../../../tests/fixtures/native/native-control-reentry.kgr"
    ));
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
            let text = context.read_string(&args[0], str::to_owned).unwrap();
            sink.lock().unwrap().push(text.clone());
            let root = context.runtime().execution_root().unwrap();
            if text == "outer" {
                let arguments = context.retain_temporaries(&[]).unwrap();
                assert_eq!(
                    reenter(context, &root, inner, arguments).unwrap().as_i32(),
                    Some(7)
                );
                assert_eq!(
                    reenter(context, &root, fail, arguments).unwrap_err().kind(),
                    RuntimeErrorKind::ScriptTrap
                );
                assert_eq!(
                    context.runtime().resources().counters().current_call_depth,
                    4
                );
            } else {
                assert_eq!(
                    context.runtime().resources().counters().current_call_depth,
                    8
                );
            }
            context.runtime_mut().collect_garbage().unwrap();
            Ok(ValueView::unit())
        })
        .install(&mut runtime)
        .unwrap();
        let loaded =
            load_bytecode(&mut runtime, "native-control", route(&program, encoded)).unwrap();
        let _loaded_program = loaded;
        let loaded = runtime.select_program(&_loaded_program).unwrap();
        let mut vm = runtime;
        vm.collect_garbage().unwrap();
        let baseline = vm.gc_stats();
        assert_eq!(
            vm.execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
                .unwrap()
                .return_value
                .as_i32(),
            Some(42)
        );
        assert_eq!(*effects.lock().unwrap(), ["outer", "inner"]);
        assert_clean(&vm);
        // Current publication owns warmed String constants independently of call
        // locals. Replace it before asserting that execution roots are reclaimed.
        publish_unused_version(&mut vm, &_loaded_program);
        vm.collect_garbage().unwrap();
        assert!(vm.validate_executable_module(&loaded).is_err());
        assert_eq!(
            vm.gc_stats().root_registrations,
            baseline.root_registrations
        );
        assert_eq!(vm.gc_stats().objects, baseline.objects);
    }
}

#[test]
fn native_callbacks_invoke_the_retained_closure_generation_after_replacement() {
    let program = |number| {
        compile_test_bytecode(&format!(
            r#"
use test::boundary;
use host;
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
        let retained = Arc::new(Mutex::new(None::<RootedValue>));
        let callback = retained.clone();
        let observed = Arc::new(Mutex::new(Vec::new()));
        let sink = observed.clone();
        let mut runtime = runtime(Default::default());
        logging_module(move |context, _| {
            let value = callback.lock().unwrap().as_ref().unwrap().clone();
            let value = context.import(&value).unwrap();
            let root = context.runtime().execution_root().unwrap();
            let arguments = context.retain_temporaries(&[value]).unwrap();
            let result = reenter(context, &root, consume, arguments).unwrap();
            sink.lock().unwrap().push(result.as_i32().unwrap());
            context.runtime_mut().collect_garbage().unwrap();
            Ok(ValueView::unit())
        })
        .install(&mut runtime)
        .unwrap();
        let old =
            load_bytecode(&mut runtime, "native-control", route(&old_program, encoded)).unwrap();
        let _old_program = old;
        let old = runtime.select_program(&_old_program).unwrap();
        let old_key = old.key();
        let mut vm = runtime;
        vm.collect_garbage().unwrap();
        let baseline = vm.gc_stats();
        let rooted = vm
            .execute_module(&old, "make", vm.execution_options())
            .unwrap()
            .return_value;
        *retained.lock().unwrap() = Some(rooted.clone());
        reload_bytecode(
            &mut vm,
            &_old_program,
            Some("native-control".into()),
            route(&new_program, encoded),
        )
        .unwrap();
        let _new_program = _old_program.clone();
        let new = vm.select_program(&_new_program).unwrap();
        drop(old);
        vm.collect_garbage().unwrap();
        {
            let mut cx = ConversionContext::new(&mut vm, &new).unwrap();
            let value = cx.import(&rooted).unwrap();
            assert_eq!(
                cx.runtime_mut()
                    .resolve_closure(&value)
                    .unwrap()
                    .implementation
                    .key(),
                old_key
            );
        }
        assert_ne!(new.key(), old_key);
        assert_eq!(
            vm.execute_module(&new, ProgramEntry::Main, vm.execution_options())
                .unwrap()
                .return_value
                .as_i32(),
            Some(0)
        );
        assert_eq!(*observed.lock().unwrap(), [41]);
        retained.lock().unwrap().take();
        drop(rooted);
        assert_clean(&vm);
        publish_unused_version(&mut vm, &_new_program);
        vm.collect_garbage().unwrap();
        assert_eq!(vm.gc_stats().objects, baseline.objects);
        assert_eq!(
            vm.gc_stats().root_registrations,
            baseline.root_registrations
        );
    }
}

#[test]
fn cancellation_at_observed_boundaries_cleans_callback_scopes() {
    let program = compile_test_bytecode(include_str!(
        "../../../../tests/fixtures/native/native-control-observed-cancellation.kgr"
    ));
    let decoded = round_trip_bytecode(program.clone());
    let mut modules = crate::modules().unwrap();
    modules.push(module());
    let installation = NativeInstallation::prepare(&modules).unwrap();
    let logging_template = ModuleBuilder::from_declaration(
        logging::declaration().unwrap(),
        &crate::declarations::StandardDeclarations::default()
            .catalog()
            .unwrap(),
        Default::default(),
    )
    .unwrap();
    let log = logging_template.declared_function("log").unwrap();
    for program in [&program, &decoded] {
        let mut cancellations = 0;
        for at in 0..80 {
            let mut runtime = installation.runtime(RuntimeConfig {
                gc: GcHeapConfig {
                    collection_threshold: Some(1),
                    ..Default::default()
                },
                ..Default::default()
            });
            let effects = Arc::new(Mutex::new(0));
            let sink = effects.clone();
            let mut logging = logging_template.clone();
            logging
                .bind(
                    log.clone(),
                    host::callback(move |_, _| {
                        *sink.lock().unwrap() += 1;
                        Ok(ValueView::unit())
                    }),
                )
                .unwrap();
            logging.finish().unwrap().install(&mut runtime).unwrap();
            let loaded = load_bytecode(&mut runtime, "native-control", program.clone()).unwrap();
            let _loaded_program = loaded;
            let loaded = runtime.select_program(&_loaded_program).unwrap();
            runtime.collect_garbage().unwrap();
            let baseline = runtime.gc_stats();
            let options = runtime.execution_options();
            runtime
                .set_execution_observer(CancelAt {
                    seen: Default::default(),
                    at,
                    token: options.cancellation.clone(),
                })
                .unwrap();
            let mut vm = runtime;
            let mut session = vm.begin_execution(&loaded, options).unwrap();
            let session_options = session.runtime().execution_options();
            session.runtime().attach_execution_observer().unwrap();
            match session.runtime_mut().execute_module(
                &loaded,
                ProgramEntry::Main,
                session_options.clone(),
            ) {
                Ok(report) => {
                    assert_eq!(report.return_value.as_i32(), Some(42));
                    assert_eq!(*effects.lock().unwrap(), 1);
                }
                Err(error) => {
                    cancellations += 1;
                    assert!(matches!(&error, error if error.kind() == RuntimeErrorKind::Cancelled));
                    assert!(
                        session
                            .runtime_mut()
                            .execute_module(&loaded, "ready", session_options.clone())
                            .is_err()
                    );
                }
            }
            assert!(*effects.lock().unwrap() <= 1);
            assert_clean(session.runtime());
            drop(session);
            vm.collect_garbage().unwrap();
            assert_eq!(
                vm.execute_module(&loaded, "ready", vm.execution_options())
                    .unwrap()
                    .return_value
                    .as_i32(),
                Some(7)
            );
            publish_unused_version(&mut vm, &_loaded_program);
            vm.collect_garbage().unwrap();
            assert_eq!(vm.gc_stats().objects, baseline.objects);
            assert_eq!(
                vm.gc_stats().root_registrations,
                baseline.root_registrations
            );
            if vm.execution_observer::<CancelAt>().unwrap().seen.get() <= at {
                break;
            }
        }
        assert!(cancellations > 5);
    }
}
