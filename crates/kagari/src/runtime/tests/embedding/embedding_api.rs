use crate::test_support::standard::standard_library;
use crate::{
    BytecodeArtifact,
    context::ExecutionContext,
    engine::{KagariEngine, source::ArtifactOptions},
    error::EmbeddingError,
    program::Program,
    runtime::{LoadOptions, ReloadOptions},
};
use kagari_runtime::{
    error::RuntimeErrorKind, host::HostError, module::entry::ProgramEntry,
    native::conversion::context::ConversionContext, reload::ReloadValidationError,
    value::carriers::RootedValue,
};
use kagari_source::source::SourceFile;
use kagari_stdlib::io;
use std::sync::{Arc, Mutex};

mod host_paths;

#[test]
fn compiles_loads_executes_and_reloads_through_embedding_api() {
    let engine = KagariEngine::with_native_modules(
        Default::default(),
        standard_library().modules().unwrap(),
    )
    .unwrap();
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
    let engine = KagariEngine::with_native_modules(
        Default::default(),
        standard_library().modules().unwrap(),
    )
    .unwrap();
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
    let engine = KagariEngine::with_native_modules(
        Default::default(),
        standard_library().modules().unwrap(),
    )
    .unwrap();
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
    let engine = KagariEngine::with_native_modules(
        Default::default(),
        standard_library().modules().unwrap(),
    )
    .unwrap();
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
    let engine = KagariEngine::with_native_modules(
        Default::default(),
        standard_library().modules().unwrap(),
    )
    .unwrap();
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
fn output_requires_explicit_io_installation() {
    let bare = KagariEngine::default();
    for (source, missing) in [
        (r#"fn main() { print("first"); }"#, "print"),
        ("use std::io::print; fn main() {}", "std::io::print"),
    ] {
        let error = bare
            .compile_source(SourceFile::new("missing-output.kgr", source))
            .expect_err("the bare engine should not provide output");
        let EmbeddingError::Diagnostics { diagnostics } = error else {
            panic!("expected structured name resolution diagnostics, got {error:?}");
        };
        assert!(
            diagnostics.iter().any(|diagnostic| {
                diagnostic.code == "KG_RESOLVE_UNKNOWN_NAME"
                    && diagnostic.message == format!("unknown name `{missing}`")
            }),
            "{diagnostics:?}"
        );
    }

    let mut builder = KagariEngine::builder().unwrap();
    let reference = io::module(builder.declarations(), |_, _, _| Ok(())).unwrap();
    let events = Arc::new(Mutex::new(Vec::new()));
    let capture = events.clone();
    let sink = io::module(builder.declarations(), move |cx, message, suffix| {
        let message = cx
            .read_string(message, str::to_owned)
            .map_err(|error| HostError::new(error.to_string()))?;
        capture.lock().unwrap().push((message, suffix));
        Ok(())
    })
    .unwrap();
    builder.install(reference).unwrap();
    let engine = builder.build().unwrap();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "explicit-output.kgr",
                include_str!("../../../../tests/fixtures/embedding/host/explicit-output.kgr"),
            ),
            ArtifactOptions::default(),
        )
        .expect("std::io alone should provide both output functions");
    let bytes = artifact.records().to_bytes(artifact.definitions()).unwrap();
    let decoded = BytecodeArtifact::from_bytes(&bytes).unwrap();
    let program = Program::from_artifact(decoded, &Default::default(), &Default::default())
        .expect("portable decoding does not need a sink");

    let context = ExecutionContext::default();
    let mut runtime = bare.runtime(context.clone());
    let error = runtime
        .load_program(&program, LoadOptions::default())
        .expect_err("loading the decoded program requires a sink");
    assert_eq!(error.kind(), RuntimeErrorKind::ModuleValidation);
    assert!(
        error
            .to_string()
            .contains("native binding is not installed"),
        "{error}"
    );
    assert_eq!(runtime.modules().loaded_count(), 0);

    sink.install(&mut runtime).unwrap();
    let loaded = runtime
        .load_program(&program, LoadOptions::default())
        .unwrap();
    assert!(events.lock().unwrap().is_empty(), "loading must not write");
    runtime
        .execute_typed::<_, ()>(&loaded, ProgramEntry::Main, (), &context)
        .unwrap();
    assert_eq!(
        *events.lock().unwrap(),
        [("first".to_owned(), ""), ("second".to_owned(), "\n")]
    );
}

#[test]
fn reflection_helper_is_available() {
    let engine = KagariEngine::with_native_modules(
        Default::default(),
        standard_library().modules().unwrap(),
    )
    .unwrap();

    let type_of_artifact = engine
        .compile_to_artifact(
            SourceFile::new("type_of.kgr", r#"fn main() -> String { type_of(7) }"#),
            ArtifactOptions::default(),
        )
        .expect("reflection source should compile with reflection profile");
    let mut runtime = engine.runtime(ExecutionContext::default());

    let type_of_module = runtime
        .load_program(
            &Program::from_artifact(type_of_artifact, &Default::default(), &Default::default())
                .unwrap(),
            LoadOptions {
                program_name: Some("type_of".to_owned()),
            },
        )
        .expect("type_of module should load");

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
