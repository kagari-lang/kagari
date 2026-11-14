use crate::{
    BytecodeArtifact,
    context::ExecutionContext,
    engine::{KagariEngine, source::ArtifactOptions},
    error::EmbeddingError,
    program::Program,
    runtime::{LoadOptions, ReloadOptions},
};
use kagari_common::identity::metadata::DefinitionMetadata;
use kagari_runtime::error::RuntimeErrorKind;
use kagari_runtime::reload::ReloadValidationError;
use kagari_runtime::{
    host::{HostCallContext, HostError},
    module::entry::ProgramEntry,
    native::{
        builder::ModuleBuilder, conversion::context::ConversionContext, host, module::NativeModule,
    },
    value::{carriers::RootedValue, view::ValueView},
};
use kagari_source::source::SourceFile;
use kagari_stdlib as stdlib;
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
    entry: impl for<'call> Fn(
        &mut HostCallContext<'call>,
        &[ValueView<'call>],
    ) -> Result<ValueView<'call>, HostError>
    + Send
    + Sync
    + 'static,
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
    builder.bind(log, host::callback(entry)).unwrap();
    builder.finish().unwrap()
}

#[test]
fn compiles_loads_executes_and_reloads_through_embedding_api() {
    let engine =
        KagariEngine::with_native_modules(Default::default(), stdlib::modules().unwrap()).unwrap();
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
    assert_eq!(report.return_value.as_i32().unwrap(), 1);

    let previous = runtime.select_program(&loaded).unwrap();
    runtime
        .program(&loaded)
        .map_err(ReloadValidationError::Runtime)
        .and_then(|base| base.prepare_replacement(&second, &Default::default()))
        .and_then(|candidate| {
            runtime.replace_program(
                &loaded,
                &candidate,
                ReloadOptions {
                    program_name: Some("game.main".to_owned()),

                    ..Default::default()
                },
            )
        })
        .expect("compatible module should reload");
    let reloaded = runtime.select_program(&loaded).unwrap();
    assert_eq!(reloaded.id, previous.id);
    assert_eq!(reloaded.epoch.0, previous.epoch.0 + 1);

    let report = runtime
        .execute(&loaded, ProgramEntry::Main, &[], &context)
        .expect("reloaded entry should execute");
    assert_eq!(report.return_value.as_i32().unwrap(), 2);
}

#[test]
fn compile_failures_return_structured_diagnostics() {
    let engine =
        KagariEngine::with_native_modules(Default::default(), stdlib::modules().unwrap()).unwrap();
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
    let engine =
        KagariEngine::with_native_modules(Default::default(), stdlib::modules().unwrap()).unwrap();
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
    let engine =
        KagariEngine::with_native_modules(Default::default(), stdlib::modules().unwrap()).unwrap();
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
                .as_i32()
                .unwrap(),
            42
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
            .as_i32()
            .unwrap(),
        42
    );
    assert!(!runtime.is_quarantined());
}

#[test]
fn execute_entry_accepts_args_boundary_and_rejects_unimplemented_arguments() {
    let engine =
        KagariEngine::with_native_modules(Default::default(), stdlib::modules().unwrap()).unwrap();
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

    let error = runtime
        .execute(
            &loaded,
            ProgramEntry::Main,
            &[RootedValue::from(1_i32)],
            &context,
        )
        .expect_err("argument passing is not implemented yet");

    assert_eq!(error.code(), "KG_RUNTIME_UNSUPPORTED_EXECUTION");
    assert!(matches!(
        error,
        ref error if error.kind() == RuntimeErrorKind::UnsupportedExecution
    ));
}

#[test]
fn installed_host_and_reflection_helpers_are_available() {
    let engine =
        KagariEngine::with_native_modules(Default::default(), stdlib::modules().unwrap()).unwrap();
    let print_artifact = compile_artifact(&engine, "print.kgr", r#"fn main() { print("x"); }"#);
    let type_of_artifact = engine
        .compile_to_artifact(
            SourceFile::new("type_of.kgr", r#"fn main() -> String { type_of(7) }"#),
            ArtifactOptions::default(),
        )
        .expect("reflection source should compile with reflection profile");
    let mut runtime = engine.runtime(ExecutionContext::default());
    logging_module(|_, _| Ok(ValueView::unit()))
        .install(&mut runtime)
        .unwrap();
    let print_module = runtime
        .load_program(
            &Program::from_artifact(print_artifact, &Default::default(), &Default::default())
                .unwrap(),
            LoadOptions {
                program_name: Some("print".to_owned()),
            },
        )
        .expect("print module should load");
    let type_of_module = runtime
        .load_program(
            &Program::from_artifact(type_of_artifact, &Default::default(), &Default::default())
                .unwrap(),
            LoadOptions {
                program_name: Some("type_of".to_owned()),
            },
        )
        .expect("type_of module should load");

    runtime
        .execute_typed::<_, ()>(
            &print_module,
            ProgramEntry::Main,
            (),
            &ExecutionContext::default(),
        )
        .unwrap();
    let result = runtime
        .execute(
            &type_of_module,
            ProgramEntry::Main,
            &[],
            &ExecutionContext::default(),
        )
        .unwrap()
        .return_value;
    let owner = runtime.select_program(&type_of_module).unwrap();
    assert_eq!(
        ConversionContext::new(&mut runtime, &owner)
            .unwrap()
            .decode::<String>(&result)
            .unwrap(),
        "i32"
    );
}
