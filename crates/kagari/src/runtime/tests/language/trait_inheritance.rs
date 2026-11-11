#[cfg(feature = "native")]
use crate::program::native::NativeCompiler;
use crate::{
    BytecodeArtifact,
    context::{ExecutionContext, JitPolicy},
    engine::{KagariEngine, source::ArtifactOptions},
    error::EmbeddingError,
    program::Compilation,
};
use kagari_codegen_cranelift::CraneliftBackend;
use kagari_common::identity::metadata::DefinitionMetadata;
use kagari_runtime::{module::entry::ProgramEntry, value::Value};
use kagari_source::source::SourceFile;
use kagari_stdlib as stdlib;
use rstest::rstest;
use std::{fs, path::PathBuf};

mod admission;
mod diagnostics;
mod lifecycle;
mod linking;

fn compile(source: &str) -> Result<DefinitionMetadata<BytecodeArtifact>, EmbeddingError> {
    KagariEngine::with_native_modules(Default::default(), stdlib::modules().unwrap())
        .unwrap()
        .compile_to_artifact(
            SourceFile::new("inheritance.kgr", source),
            ArtifactOptions::default(),
        )
}

fn execute(artifact: DefinitionMetadata<BytecodeArtifact>) {
    for (encoded, jit) in [(false, false), (true, false), (true, true)] {
        let artifact = if encoded {
            BytecodeArtifact::from_bytes(
                &artifact.records().to_bytes(artifact.definitions()).unwrap(),
            )
            .unwrap()
        } else {
            artifact.clone()
        };
        let context = ExecutionContext {
            jit_policy: if jit {
                JitPolicy::Enabled
            } else {
                JitPolicy::Disabled
            },
            ..Default::default()
        };
        let mut runtime =
            KagariEngine::with_native_modules(Default::default(), stdlib::modules().unwrap())
                .unwrap()
                .runtime(context.clone());
        let program =
            Compilation::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
        let loaded = runtime
            .load_program(program.program(), Default::default())
            .unwrap();
        let result = if jit {
            let mut backend = CraneliftBackend::for_host().unwrap();
            let prepared = NativeCompiler::default()
                .prepare(
                    &program,
                    ProgramEntry::Main,
                    &mut backend,
                    &context.cancellation,
                )
                .and_then(|native_code| (native_code).install(&mut runtime, &loaded))
                .unwrap();
            runtime.execute_prepared(&loaded, ProgramEntry::Main, &[], &context, &prepared)
        } else {
            runtime.execute(&loaded, ProgramEntry::Main, &[], &context)
        }
        .unwrap();
        assert_eq!(
            result
                .return_value
                .value(runtime.gc())
                .expect("retained execution result"),
            Value::I32(42)
        );
    }
}

#[rstest]
fn ordinary_execution(
    #[base_dir = "tests/fixtures/language/trait_inheritance/all_routes"]
    #[files("*.kgr")]
    path: PathBuf,
) {
    let source = fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "failed to read trait inheritance fixture {}: {error}",
            path.display()
        )
    });
    execute(compile(&source).unwrap());
}

#[rstest]
fn interpreter_execution(
    #[base_dir = "tests/fixtures/language/trait_inheritance/interpreter_routes"]
    #[files("*.kgr")]
    path: PathBuf,
) {
    let source = fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "failed to read trait inheritance fixture {}: {error}",
            path.display()
        )
    });
    let artifact = compile(&source).unwrap();
    let context = ExecutionContext::default();
    for encoded in [false, true] {
        let artifact = if encoded {
            BytecodeArtifact::from_bytes(
                &artifact.records().to_bytes(artifact.definitions()).unwrap(),
            )
            .unwrap()
        } else {
            artifact.clone()
        };
        let mut runtime =
            KagariEngine::with_native_modules(Default::default(), stdlib::modules().unwrap())
                .unwrap()
                .runtime(context.clone());
        let program_program =
            Compilation::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
        let program = runtime
            .load_program(program_program.program(), Default::default())
            .unwrap();
        assert_eq!(
            runtime
                .execute(&program, ProgramEntry::Main, &[], &context)
                .unwrap()
                .return_value
                .value(runtime.gc())
                .expect("retained execution result"),
            Value::I32(42)
        );
    }
}

#[rstest]
fn encoded_execution(
    #[base_dir = "tests/fixtures/language/trait_inheritance/encoded"]
    #[files("*.kgr")]
    path: PathBuf,
) {
    let source = fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "failed to read trait inheritance fixture {}: {error}",
            path.display()
        )
    });
    let artifact = compile(&source).unwrap();
    let artifact =
        BytecodeArtifact::from_bytes(&artifact.records().to_bytes(artifact.definitions()).unwrap())
            .unwrap();
    let context = ExecutionContext::default();
    let mut runtime =
        KagariEngine::with_native_modules(Default::default(), stdlib::modules().unwrap())
            .unwrap()
            .runtime(context.clone());
    let program_program =
        Compilation::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let program = runtime
        .load_program(program_program.program(), Default::default())
        .unwrap();
    assert_eq!(
        runtime
            .execute(&program, ProgramEntry::Main, &[], &context)
            .unwrap()
            .return_value
            .value(runtime.gc())
            .expect("retained execution result"),
        Value::I32(42)
    );
}

#[rstest]
fn direct_execution(
    #[base_dir = "tests/fixtures/language/trait_inheritance/direct"]
    #[files("*.kgr")]
    path: PathBuf,
) {
    let source = fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "failed to read trait inheritance fixture {}: {error}",
            path.display()
        )
    });
    let artifact = compile(&source).unwrap();
    let context = ExecutionContext::default();
    let mut runtime =
        KagariEngine::with_native_modules(Default::default(), stdlib::modules().unwrap())
            .unwrap()
            .runtime(context.clone());
    let program_program =
        Compilation::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let program = runtime
        .load_program(program_program.program(), Default::default())
        .unwrap();
    assert_eq!(
        runtime
            .execute(&program, ProgramEntry::Main, &[], &context)
            .unwrap()
            .return_value
            .value(runtime.gc())
            .expect("retained execution result"),
        Value::I32(42)
    );
}
