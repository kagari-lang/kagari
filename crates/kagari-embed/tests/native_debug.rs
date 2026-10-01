//! Registered debug, native Never and common guarded host invocation without source.
#[path = "fixtures/native_debug_api.rs"]
mod fixture_api;
use fixture_api::diagnostics;
use kagari_abi::{scalar::BuiltinType, types::AbiType};
use kagari_bytecode::{
    artifact::KbcArtifact,
    instruction::{BytecodeInstruction, ConstantOperand},
};
use kagari_common::host_interface::standard_log;
use kagari_embed::{
    context::ExecutionContext,
    engine::{EngineConfig, KagariEngine},
    error::EmbeddingError,
    program::PreparedProgram,
    runtime::KagariRuntime,
};
use kagari_runtime::{
    Runtime,
    host::{HostError, HostFunction},
    native::packages::standard_library,
    value::Value,
};
use kagari_vm::reentry::reenter;
use std::{cell::RefCell, rc::Rc};

const ARTIFACT: &[u8] = include_bytes!("fixtures/native_debug.kbc");
fn message(error: &EmbeddingError) -> &str {
    match error {
        EmbeddingError::Runtime { message, .. } => message,
        _ => panic!("{error:?}"),
    }
}
fn engine() -> KagariEngine {
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    KagariEngine::builder()
        .config(config)
        .install_standard_library(false)
        .install(Ok(standard_library()))
        .install(diagnostics::native_api())
        .build()
        .unwrap()
}
fn context() -> ExecutionContext {
    let mut context = ExecutionContext::default();
    context.language_profile.allow_host_calls = true;
    context.capabilities.host_calls = true;
    context.host_policy.allowed_host_functions = vec!["host.log".into(), "app.echo".into()];
    context.tracing_enabled = true;
    context
}
fn prepared(assertion: bool) -> PreparedProgram {
    let mut program = KbcArtifact::from_bytes(ARTIFACT).unwrap().program;
    if !assertion {
        let root = &mut program.modules[program.root.index()];
        let function = root
            .functions
            .iter_mut()
            .find(|function| function.name == "assertion")
            .unwrap();
        for instruction in &mut function.instructions {
            if let BytecodeInstruction::LoadConst {
                constant: ConstantOperand::Bool(condition),
                ..
            } = instruction
            {
                *condition = false;
            }
        }
        root.constants.push(ConstantOperand::Bool(false));
    }
    let artifact = KbcArtifact::from_program(program, Default::default()).unwrap();
    PreparedProgram::from_artifact(
        KbcArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap(),
        &Default::default(),
        &Default::default(),
    )
    .unwrap()
}
fn clean(runtime: &Runtime) {
    assert_eq!(runtime.gc().active_roots(), 0);
    assert_eq!(runtime.resources().counters().current_call_depth, 0);
    assert!(!runtime.is_quarantined());
}
fn logging(runtime: &mut KagariRuntime, fail: bool) -> Rc<RefCell<Vec<String>>> {
    let events = Rc::new(RefCell::new(vec![]));
    let observed = events.clone();
    let mut declaration = standard_log();
    declaration.resource_cost_hint = Some(17);
    runtime
        .register_host_function(HostFunction::new(declaration, move |_, arguments| {
            let [Value::Str(message)] = arguments else {
                panic!("checked signature");
            };
            observed.borrow_mut().push(message.clone());
            if fail {
                Err(HostError::new("sink failed"))
            } else {
                Ok(Value::Unit)
            }
        }))
        .unwrap();
    events
}

#[test]
fn assertions_and_never_results_preserve_trap_class_message_and_caller_trace() {
    let context = context();
    let mut runtime = engine().runtime(context.clone());
    let loaded = runtime
        .load_program(&prepared(true), Default::default())
        .unwrap();
    assert_eq!(
        runtime
            .execute(&loaded, "assertion", &[], &context)
            .unwrap()
            .return_value,
        Value::Unit
    );
    for entry in ["stop", "coerced", "nested", "app_stop"] {
        let error = runtime.execute(&loaded, entry, &[], &context).unwrap_err();
        assert_eq!(error.code(), "KG_RUNTIME_SCRIPT_TRAP");
        assert!(message(&error).contains(if entry == "app_stop" {
            "application fatal"
        } else {
            "debug.panic:"
        }));
        let trace = error.error_trace().unwrap();
        assert!(
            trace
                .frames
                .iter()
                .any(|frame| frame.source_uri == "memory://native-debug.kgr"
                    && frame.function_name == entry)
        );
        if entry == "nested" {
            assert!(
                trace
                    .frames
                    .iter()
                    .any(|frame| frame.function_name == "coerced")
            );
        }
        clean(runtime.runtime());
    }
    let loaded = runtime
        .load_program(&prepared(false), Default::default())
        .unwrap();
    let error = runtime
        .execute(&loaded, "assertion", &[], &context)
        .unwrap_err();
    assert_eq!(error.code(), "KG_RUNTIME_SCRIPT_TRAP");
    assert!(message(&error).contains("debug.assert failed: message"));
    clean(runtime.runtime());
}

#[test]
fn logging_and_eager_traps_preserve_exact_host_effects_and_unreachable_code() {
    let context = context();
    let mut runtime = engine().runtime(context.clone());
    let events = logging(&mut runtime, false);
    let loaded = runtime
        .load_program(&prepared(true), Default::default())
        .unwrap();
    for (entry, expected, success) in [
        ("logging", vec!["é😀"], true),
        ("sequence", vec!["first"], false),
        ("unreachable_log", vec!["before"], false),
        ("eager", vec!["condition", "message"], false),
    ] {
        let result = runtime.execute(&loaded, entry, &[], &context);
        assert_eq!(result.is_ok(), success);
        if let Err(error) = result {
            assert_eq!(error.code(), "KG_RUNTIME_SCRIPT_TRAP");
        }
        assert_eq!(*events.borrow(), expected);
        events.borrow_mut().clear();
        clean(runtime.runtime());
    }
}

#[test]
fn host_lookup_permissions_costs_failures_and_return_validation_remain_guarded() {
    let context = context();
    let mut runtime = engine().runtime(context.clone());
    let loaded = runtime
        .load_program(&prepared(true), Default::default())
        .unwrap();
    let error = runtime
        .execute(&loaded, "logging", &[], &context)
        .unwrap_err();
    assert_eq!(error.code(), "KG_RUNTIME_HOST_CALL_FAILURE");
    clean(runtime.runtime());
    let events = logging(&mut runtime, false);
    for kind in 0..4 {
        let mut denied = context.clone();
        match kind {
            0 => denied.capabilities.host_calls = false,
            1 => denied.language_profile.allow_host_calls = false,
            2 => denied.host_policy.allowed_host_functions.clear(),
            _ => denied.resources.max_host_calls = Some(0),
        }
        let error = runtime
            .execute(&loaded, "logging", &[], &denied)
            .unwrap_err();
        assert_eq!(
            error.code(),
            if kind == 3 {
                "KG_RUNTIME_RESOURCE_LIMIT_EXCEEDED"
            } else {
                "KG_RUNTIME_CAPABILITY_DENIED"
            }
        );
        assert!(events.borrow().is_empty());
        clean(runtime.runtime());
    }
    let mut failed = engine().runtime(context.clone());
    let events = logging(&mut failed, true);
    let loaded = failed
        .load_program(&prepared(true), Default::default())
        .unwrap();
    let error = failed
        .execute(&loaded, "logging", &[], &context)
        .unwrap_err();
    assert_eq!(error.code(), "KG_RUNTIME_HOST_CALL_FAILURE");
    assert!(message(&error).contains("sink failed"));
    assert_eq!(*events.borrow(), vec!["é😀"]);
    clean(failed.runtime());
    let mut invalid = engine().runtime(context.clone());
    invalid
        .register_host_function(HostFunction::new(standard_log(), |_, _| Ok(Value::I32(1))))
        .unwrap();
    let loaded = invalid
        .load_program(&prepared(true), Default::default())
        .unwrap();
    assert_eq!(
        invalid
            .execute(&loaded, "logging", &[], &context)
            .unwrap_err()
            .code(),
        "KG_RUNTIME_HOST_CALL_FAILURE"
    );
    clean(invalid.runtime());
}

#[test]
fn every_instruction_depth_and_host_budget_cut_preserves_completed_effects_and_cleanup() {
    let context = context();
    let mut runtime = engine().runtime(context.clone());
    let events = logging(&mut runtime, false);
    let loaded = runtime
        .load_program(&prepared(true), Default::default())
        .unwrap();
    let before = runtime.runtime().resources().counters();
    runtime.execute(&loaded, "logging", &[], &context).unwrap();
    let after = runtime.runtime().resources().counters();
    events.borrow_mut().clear();
    for limit in 0..=after.instruction_steps - before.instruction_steps {
        let mut limited = context.clone();
        limited.resources.max_instruction_steps = Some(limit);
        match runtime.execute(&loaded, "logging", &[], &limited) {
            Ok(report) => {
                assert_eq!(report.return_value, Value::Unit);
                assert_eq!(limit, after.instruction_steps - before.instruction_steps);
            }
            Err(error) => assert_eq!(error.code(), "KG_RUNTIME_RESOURCE_LIMIT_EXCEEDED"),
        }
        assert!(events.borrow().len() <= 1);
        assert!(events.borrow().iter().all(|message| message == "é😀"));
        events.borrow_mut().clear();
        clean(runtime.runtime());
    }
    assert_eq!(after.host_calls - before.host_calls, 1);
    for limit in 0..=after.peak_call_depth {
        let mut limited = context.clone();
        limited.resources.max_call_depth = Some(limit);
        match runtime.execute(&loaded, "logging", &[], &limited) {
            Ok(_) => assert_eq!(limit, after.peak_call_depth),
            Err(error) => {
                assert_eq!(error.code(), "KG_RUNTIME_RESOURCE_LIMIT_EXCEEDED");
                assert!(events.borrow().is_empty());
            }
        }
        events.borrow_mut().clear();
        clean(runtime.runtime());
    }
    let cancelled = ExecutionContext {
        cancellation: Default::default(),
        ..context.clone()
    };
    cancelled.cancellation.cancel();
    assert_eq!(
        runtime
            .execute(&loaded, "logging", &[], &cancelled)
            .unwrap_err()
            .code(),
        "KG_RUNTIME_CANCELLED"
    );
    assert!(events.borrow().is_empty());
    clean(runtime.runtime());
}

#[test]
fn synchronous_host_reentry_retains_scoped_temporaries_until_traps_and_returns() {
    for fail in [false, true] {
        let context = context();
        let mut runtime = engine().runtime(context.clone());
        runtime
            .register_host_function(HostFunction::new(standard_log(), move |call, _| {
                let heap = call.runtime().gc();
                let version = call.runtime().execution_root().unwrap();
                let answer = version
                    .bytecode
                    .functions
                    .iter()
                    .find(|function| function.name == "answer")
                    .unwrap()
                    .id;
                let returned = reenter(call, &version, answer, &[]).unwrap();
                let value = returned.value();
                let Value::Enum(temporary) = value else {
                    panic!("checked Option result");
                };
                call.retain_temporaries(&[value]).unwrap();
                drop(returned);
                call.runtime().collect_garbage().unwrap();
                assert_eq!(
                    heap.enum_snapshot(temporary).unwrap().fields,
                    vec![Value::U64(42)]
                );
                if fail {
                    Err(HostError::new("after reentry"))
                } else {
                    Ok(Value::Unit)
                }
            }))
            .unwrap();
        let loaded = runtime
            .load_program(&prepared(true), Default::default())
            .unwrap();
        let result = runtime.execute(&loaded, "logging", &[], &context);
        assert_eq!(result.is_err(), fail);
        if let Err(error) = result {
            assert_eq!(error.code(), "KG_RUNTIME_HOST_CALL_FAILURE");
        }
        clean(runtime.runtime());
        runtime.runtime().collect_garbage().unwrap();
        assert_eq!(runtime.runtime().gc().allocated_objects(), 0);
    }
}

#[test]
fn host_cancellation_preserves_completed_sink_effect_and_releases_scoped_state() {
    let context = context();
    let mut runtime = engine().runtime(context.clone());
    let token = context.cancellation.clone();
    let events = Rc::new(RefCell::new(vec![]));
    let observed = events.clone();
    runtime
        .register_host_function(HostFunction::new(standard_log(), move |_, arguments| {
            observed.borrow_mut().push(arguments[0].clone());
            token.cancel();
            Ok(Value::Unit)
        }))
        .unwrap();
    let loaded = runtime
        .load_program(&prepared(true), Default::default())
        .unwrap();
    assert_eq!(
        runtime
            .execute(&loaded, "logging", &[], &context)
            .unwrap_err()
            .code(),
        "KG_RUNTIME_CANCELLED"
    );
    assert_eq!(*events.borrow(), vec![Value::Str("é😀".into())]);
    clean(runtime.runtime());
}

#[test]
fn forged_never_import_result_is_rejected_offline() {
    let mut artifact = KbcArtifact::from_bytes(ARTIFACT).unwrap();
    let mut changed = false;
    for module in &mut artifact.program.modules {
        for import in &mut module.native_imports {
            if import.signature.result == AbiType::Builtin(BuiltinType::Never) {
                import.signature.result = AbiType::Builtin(BuiltinType::Unit);
                changed = true;
            }
        }
    }
    assert!(changed);
    assert!(KbcArtifact::from_program(artifact.program, Default::default()).is_err());
}

#[cfg(feature = "native")]
mod native_backend {
    use super::*;
    use kagari_codegen_cranelift::CraneliftBackend;
    use kagari_vm::vm::{JitExecutionStatus, native::PreparedNativeEntry};
    #[test]
    fn source_free_prepared_fallback_preserves_host_effects_and_never_traps() {
        let mut context = context();
        context.language_profile.allow_jit = true;
        context.capabilities.jit = true;
        let mut runtime = engine().runtime(context.clone());
        let events = logging(&mut runtime, false);
        let program = prepared(true);
        let loaded = runtime.load_program(&program, Default::default()).unwrap();
        let mut backend = CraneliftBackend::for_host().unwrap();
        for (entry, expected, success) in [
            ("assertion", vec![], true),
            ("logging", vec!["é😀"], true),
            ("stop", vec![], false),
            ("unreachable_log", vec!["before"], false),
        ] {
            let native = runtime
                .prepare_native(
                    &program,
                    &loaded,
                    entry,
                    &mut backend,
                    &context.cancellation,
                )
                .unwrap();
            assert!(matches!(native, PreparedNativeEntry::Unsupported { .. }));
            match runtime.execute_prepared(&loaded, entry, &[], &context, &native) {
                Ok(report) => {
                    assert!(success);
                    assert_eq!(
                        report.jit.unwrap().status,
                        JitExecutionStatus::InterpreterFallback
                    );
                }
                Err(error) => {
                    assert!(!success);
                    assert_eq!(error.code(), "KG_RUNTIME_SCRIPT_TRAP");
                }
            }
            assert_eq!(*events.borrow(), expected);
            events.borrow_mut().clear();
            clean(runtime.runtime());
        }
    }
}

#[cfg(feature = "source")]
mod source {
    use super::*;
    use kagari_common::{
        host_interface::{HostFunctionDeclaration, value_type::HostValueType},
        source::SourceFile,
        source_database::SourceLayer,
    };
    #[test]
    fn exact_product_tooling_and_static_never_coercion_follow_real_registrations() {
        let engine = engine();
        let artifact = engine
            .compile_to_artifact(
                SourceFile::new(
                    "memory://native-debug.kgr",
                    include_str!("fixtures/native_debug.kgr"),
                ),
                Default::default(),
                Default::default(),
            )
            .unwrap();
        assert!(artifact.to_bytes().unwrap() == ARTIFACT);
        let text = "use std::debug; fn main() -> usize { debug::panic(\"stop\") }";
        let file = engine
            .set_source(
                "memory://debug-navigation.kgr",
                text.into(),
                SourceLayer::Base,
            )
            .unwrap();
        let snapshot = engine
            .analyze(
                engine.source_snapshot(),
                Default::default(),
                &Default::default(),
            )
            .unwrap();
        let offset = text.find("panic(").unwrap();
        let definition = snapshot.definition_at(file, offset).unwrap();
        assert_eq!(
            snapshot.source(definition.location.file).unwrap().name(),
            "kagari://native/kagari-std/debug.kgr"
        );
        assert!(
            snapshot
                .documentation_at(file, offset)
                .unwrap()
                .written_signature
                .contains("-> !")
        );
        for invalid in [
            "fn main() { debug::assert(1usize, \"x\"); }",
            "fn main() { debug::panic(true); }",
            "fn main() -> usize { debug::print(\"x\") }",
        ] {
            assert!(
                engine
                    .compile_source(
                        SourceFile::new(
                            "memory://invalid-debug.kgr",
                            format!("use std::debug; {invalid}")
                        ),
                        Default::default()
                    )
                    .is_err()
            );
        }
    }
    #[test]
    fn application_never_and_host_calls_compile_and_execute_without_default_library() {
        let engine = KagariEngine::builder()
            .install_standard_library(false)
            .install(diagnostics::native_api())
            .build()
            .unwrap();
        assert_eq!(engine.native_declaration_sources().len(), 1);
        let artifact = engine.compile_to_artifact(SourceFile::new("memory://application-diagnostics.kgr", "use game::diagnostics::{fatal, echo, Text}; fn main() -> usize { fatal() } fn relay() -> Text { echo(\"é😀\") }"), Default::default(), Default::default()).unwrap();
        let prepared = PreparedProgram::from_artifact(
            KbcArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap(),
            &Default::default(),
            &Default::default(),
        )
        .unwrap();
        let context = context();
        let mut runtime = engine.runtime(context.clone());
        let declaration =
            HostFunctionDeclaration::new("app.echo", standard_log().params, HostValueType::String);
        runtime
            .register_host_function(HostFunction::new(declaration, |_, arguments| {
                Ok(arguments[0].clone())
            }))
            .unwrap();
        let loaded = runtime.load_program(&prepared, Default::default()).unwrap();
        assert_eq!(
            runtime
                .execute(&loaded, "main", &[], &context)
                .unwrap_err()
                .code(),
            "KG_RUNTIME_SCRIPT_TRAP"
        );
        assert_eq!(
            runtime
                .execute(&loaded, "relay", &[], &context)
                .unwrap()
                .return_value,
            Value::Str("é😀".into())
        );
        clean(runtime.runtime());
    }
}
