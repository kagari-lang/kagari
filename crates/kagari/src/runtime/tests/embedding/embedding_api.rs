use crate::{
    BytecodeArtifact,
    context::ExecutionContext,
    engine::{KagariEngine, source::ArtifactOptions},
    error::{EmbeddingError, RuntimeFailureKind},
    program::{PreparedProgram, ProgramPreparationError},
    runtime::{LoadOptions, ReloadOptions},
};
use kagari_common::identity::metadata::DefinitionMetadata;
use kagari_runtime::{
    host::{HostCallContext, HostError},
    module::entry::ProgramEntry,
    native::{builder::ModuleBuilder, host::HostCallback, module::NativeModule},
    value::Value,
};
use kagari_source::source::SourceFile;
use kagari_types::declaration::logging;

mod host_paths;

fn compile_artifact(
    engine: &KagariEngine,
    name: &str,
    source: &str,
) -> DefinitionMetadata<BytecodeArtifact> {
    engine
        .compile_to_artifact(SourceFile::new(name, source), ArtifactOptions::default())
        .expect("source should compile")
}

fn logging_module(
    entry: HostCallback<
        impl for<'call> Fn(&HostCallContext<'call>, &[Value]) -> Result<Value, HostError>
        + Send
        + Sync
        + 'static,
    >,
) -> NativeModule {
    let mut builder = ModuleBuilder::from_declaration(
        logging::declaration().unwrap(),
        &kagari_stdlib::declarations::StandardDeclarations::default()
            .catalog()
            .unwrap(),
        Default::default(),
    )
    .unwrap();
    let log = builder.declared_function("log").unwrap();
    builder.bind(log, entry).unwrap();
    builder.finish().unwrap()
}

#[test]
fn compiles_loads_executes_and_reloads_through_embedding_api() {
    let engine = KagariEngine::default();
    let context = ExecutionContext::default();
    let first = engine
        .compile_to_program(
            SourceFile::new("game/main.kgr", "fn main() -> i32 { 1 }"),
            Default::default(),
        )
        .unwrap();
    let second = engine
        .compile_to_program(
            SourceFile::new("game/main.kgr", "fn main() -> i32 { 2 }"),
            Default::default(),
        )
        .unwrap();

    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime
        .load_program(
            &first,
            LoadOptions {
                program_name: Some("game.main".to_owned()),
            },
        )
        .expect("module should load");
    let report = runtime
        .execute(&loaded, ProgramEntry::Main, &[], &context)
        .expect("entry should execute");
    assert_eq!(
        runtime
            .inspect(&report.return_value, |view| view.as_i32())
            .expect("retained execution result"),
        1
    );

    let previous = runtime.runtime().select_program(&loaded).unwrap();
    runtime
        .reload_program(
            &loaded,
            &second,
            ReloadOptions {
                program_name: Some("game.main".to_owned()),
            },
        )
        .expect("compatible module should reload");
    let reloaded = runtime.runtime().select_program(&loaded).unwrap();
    assert_eq!(reloaded.id, previous.id);
    assert_eq!(reloaded.epoch.0, previous.epoch.0 + 1);

    let report = runtime
        .execute(&loaded, ProgramEntry::Main, &[], &context)
        .expect("reloaded entry should execute");
    assert_eq!(
        runtime
            .inspect(&report.return_value, |view| view.as_i32())
            .expect("retained execution result"),
        2
    );
}

#[test]
fn compile_failures_return_structured_diagnostics() {
    let engine = KagariEngine::default();
    let error = engine
        .compile_source(SourceFile::new("bad.kgr", "fn main( -> i32 { 1 }"))
        .expect_err("parse failure should be structured");

    let EmbeddingError::Diagnostics { diagnostics } = error else {
        panic!("expected diagnostic error");
    };
    assert!(!diagnostics.is_empty());
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| !diagnostic.code.is_empty())
    );
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| !diagnostic.message.is_empty())
    );
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code.starts_with("KG_PARSE_"))
    );
}

#[test]
fn analysis_failures_return_structured_diagnostics() {
    let engine = KagariEngine::default();
    let error = engine
        .compile_source(SourceFile::new(
            "bad_type.kgr",
            "fn main() -> Missing { 1 }",
        ))
        .expect_err("analysis failure should be structured");

    let EmbeddingError::Diagnostics { diagnostics } = error else {
        panic!("expected diagnostic error");
    };
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "KG_TYPE_UNKNOWN_TYPE")
    );
}

#[test]
fn each_execute_uses_its_cancellation_without_changing_runtime_defaults() {
    let engine = KagariEngine::default();
    let mut runtime = engine.runtime(ExecutionContext::default());
    let artifact = engine
        .compile_to_program(
            SourceFile::new("scoped.kgr", "fn main() -> i32 { 42 }"),
            Default::default(),
        )
        .unwrap();
    let loaded = runtime
        .load_program(&artifact, LoadOptions::default())
        .unwrap();
    let context = ExecutionContext::default();

    for _ in 0..2 {
        assert_eq!(
            runtime
                .execute(&loaded, ProgramEntry::Main, &[], &context)
                .unwrap()
                .return_value
                .inspect(runtime.runtime(), |view| view.as_i32().map(Value::I32))
                .expect("retained execution result"),
            Value::I32(42)
        );
    }

    context.cancellation.cancel();
    assert_eq!(
        runtime
            .execute(&loaded, ProgramEntry::Main, &[], &context)
            .unwrap_err()
            .code(),
        "KG_RUNTIME_CANCELLED"
    );
    assert_eq!(
        runtime
            .execute(
                &loaded,
                ProgramEntry::Main,
                &[],
                &ExecutionContext::default()
            )
            .unwrap()
            .return_value
            .inspect(runtime.runtime(), |view| view.as_i32().map(Value::I32))
            .expect("retained execution result"),
        Value::I32(42)
    );
    assert!(!runtime.runtime().is_quarantined());
}

#[test]
fn failed_reload_validation_does_not_publish_new_epoch() {
    let engine = KagariEngine::default();
    let context = ExecutionContext::default();
    let first = compile_artifact(&engine, "reload.kgr", "fn main() -> i32 { 1 }");
    let candidate = compile_artifact(&engine, "reload.kgr", "fn main() -> i32 { 2 }");
    let (definitions, mut candidate) = candidate.into_parts();
    candidate.header.runtime_abi_version = "wrong-runtime-abi".to_owned();
    let candidate =
        DefinitionMetadata::checked(definitions, candidate, &Default::default()).unwrap();

    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime
        .load_program(
            &PreparedProgram::from_artifact(first, &Default::default(), &Default::default())
                .unwrap(),
            LoadOptions {
                program_name: Some("reload".to_owned()),
            },
        )
        .expect("module should load");
    let _selected_loaded = runtime.runtime().select_program(&loaded).unwrap();
    let before_count = runtime.runtime().modules().loaded_count();
    let before_epoch = runtime.runtime().select_program(&loaded).unwrap().epoch;

    let error =
        PreparedProgram::from_artifact(candidate, &Default::default(), &context.cancellation)
            .expect_err("invalid artifact should not be prepared for reload");
    let ProgramPreparationError::Artifact(error) = error else {
        panic!("expected artifact validation error");
    };
    assert_eq!(error.code(), "KG_ARTIFACT_RUNTIME_ABI_MISMATCH");
    assert_eq!(runtime.runtime().modules().loaded_count(), before_count);
    assert_eq!(
        runtime.runtime().select_program(&loaded).unwrap().epoch,
        before_epoch
    );
    assert_eq!(
        runtime
            .execute(&loaded, ProgramEntry::Main, &[], &context)
            .unwrap()
            .return_value
            .inspect(runtime.runtime(), |view| view.as_i32().map(Value::I32))
            .expect("retained execution result"),
        Value::I32(1)
    );
}

#[test]
fn execute_entry_accepts_args_boundary_and_rejects_unimplemented_arguments() {
    let engine = KagariEngine::default();
    let context = ExecutionContext::default();
    let artifact = engine
        .compile_to_program(
            SourceFile::new("args.kgr", "fn main() -> i32 { 1 }"),
            Default::default(),
        )
        .unwrap();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime
        .load_program(
            &artifact,
            LoadOptions {
                program_name: Some("args".to_owned()),
            },
        )
        .expect("module should load");

    let argument = unsafe { runtime.runtime().gc().root_value_transport(Value::I32(1)) }.unwrap();
    let error = runtime
        .execute(&loaded, ProgramEntry::Main, &[argument], &context)
        .expect_err("argument passing is not implemented yet");

    assert_eq!(error.code(), "KG_RUNTIME_UNSUPPORTED_EXECUTION");
    assert!(matches!(
        error,
        EmbeddingError::Runtime {
            kind: RuntimeFailureKind::UnsupportedExecution,
            ..
        }
    ));
}

#[test]
fn installed_host_and_reflection_helpers_are_available() {
    let engine = KagariEngine::default();
    let print_artifact = compile_artifact(&engine, "print.kgr", r#"fn main() { print("x"); }"#);
    let type_of_artifact = engine
        .compile_to_artifact(
            SourceFile::new("type_of.kgr", r#"fn main() -> String { type_of(7) }"#),
            ArtifactOptions::default(),
        )
        .expect("reflection source should compile with reflection profile");
    let mut runtime = engine.runtime(ExecutionContext::default());
    logging_module(unsafe {
        // SAFETY: The host frame protects inputs; escaped managed values are retained and results transfer immediately.
        HostCallback::new(|_, _| Ok(Value::Unit))
    })
    .install(runtime.runtime_mut())
    .unwrap();
    let print_module = runtime
        .load_program(
            &PreparedProgram::from_artifact(
                print_artifact,
                &Default::default(),
                &Default::default(),
            )
            .unwrap(),
            LoadOptions {
                program_name: Some("print".to_owned()),
            },
        )
        .expect("print module should load");
    let type_of_module = runtime
        .load_program(
            &PreparedProgram::from_artifact(
                type_of_artifact,
                &Default::default(),
                &Default::default(),
            )
            .unwrap(),
            LoadOptions {
                program_name: Some("type_of".to_owned()),
            },
        )
        .expect("type_of module should load");

    assert!(
        runtime
            .execute(
                &print_module,
                ProgramEntry::Main,
                &[],
                &ExecutionContext::default()
            )
            .unwrap()
            .return_value
            .inspect(runtime.runtime(), |view| Ok(view.is_unit()))
            .expect("retained execution result")
    );
    assert_eq!(
        runtime
            .execute(
                &type_of_module,
                ProgramEntry::Main,
                &[],
                &ExecutionContext::default(),
            )
            .unwrap()
            .return_value
            .inspect(runtime.runtime(), |view| Ok(view.as_str()?.to_owned()))
            .unwrap(),
        "i32"
    );
}
