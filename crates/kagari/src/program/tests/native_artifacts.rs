use crate::{
    context::ExecutionContext,
    engine::{
        KagariEngine,
        source::{ArtifactOptions, NativeInputExport},
    },
    error::EmbeddingError,
    program::PreparedProgram,
};
use kagari_bytecode::{
    artifact::{ArtifactBuildOptions, KbcArtifact},
    native_input::PortableMir,
};
use kagari_common::identity::map::DefinitionContext;
use kagari_compiler::{
    bytecode::lower_program_to_bytecode, native_input::verify_native_input,
    source::program::lower_program_to_mir,
};
use kagari_hir::analysis::AnalysisDatabase;
use kagari_runtime::module::entry::ProgramEntry;
use kagari_runtime::value::Value;
use kagari_source::{
    source::SourceFile,
    source_database::{SourceDatabase, SourceLayer},
};

#[test]
fn source_exports_matching_native_input_or_explicit_bytecode_only_artifacts() {
    let engine = KagariEngine::default();
    let checked = engine
        .compile_source(SourceFile::new("native-export", "fn main() -> i32 { 42 }"))
        .unwrap();
    for native_input in [
        NativeInputExport::PortableMir,
        NativeInputExport::BytecodeOnly,
    ] {
        let artifact = engine
            .emit_artifact(
                &checked,
                ArtifactOptions {
                    native_input,
                    // Source emission must never attach independently supplied compiler input.
                    build: ArtifactBuildOptions {
                        portable_mir: Some(PortableMir { bytes: vec![0xff] }),
                        ..Default::default()
                    },
                    ..Default::default()
                },
            )
            .unwrap();
        let artifact =
            KbcArtifact::from_bytes(&artifact.records().to_bytes(artifact.definitions()).unwrap())
                .unwrap();
        let definitions = DefinitionContext::new().unwrap();
        let verified = artifact
            .import_into(&definitions, &Default::default())
            .unwrap()
            .into_records()
            .into_verified(&Default::default(), &definitions, &Default::default())
            .unwrap();
        match native_input {
            NativeInputExport::PortableMir => {
                verify_native_input(
                    &verified.portable_mir().unwrap().bytes,
                    verified.bytecode(),
                    &Default::default(),
                )
                .unwrap();
            }
            NativeInputExport::BytecodeOnly => assert!(artifact.records().portable_mir.is_none()),
        }
        let context = ExecutionContext::default();
        let mut runtime = engine.runtime(context.clone());
        let program =
            PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap();
        assert_eq!(
            program.has_native_input(),
            native_input == NativeInputExport::PortableMir
        );
        let loaded = runtime.load_program(&program, Default::default()).unwrap();
        assert_eq!(
            runtime
                .execute(&loaded, ProgramEntry::Main, &[], &context)
                .unwrap()
                .return_value
                .value(runtime.runtime().gc())
                .expect("retained execution result"),
            Value::I32(42)
        );
    }
}

#[test]
fn prepared_reload_preserves_abi_validation_and_the_previous_version_on_failure() {
    let engine = KagariEngine::default();
    // Author a provider-free ABI fixture and admit only its verified artifact.
    let prepare = |source: &str| {
        let mut sources = SourceDatabase::default();
        let root = sources
            .set("reload", source.into(), SourceLayer::Base)
            .unwrap();
        let mut analysis = AnalysisDatabase::default();
        let checked = analysis
            .snapshot(sources.snapshot(), &Default::default())
            .unwrap()
            .check_program(root, &Default::default())
            .unwrap();
        let mir = lower_program_to_mir(&checked, &Default::default()).unwrap();
        let bytecode = lower_program_to_bytecode(&mir, &Default::default()).unwrap();
        let artifact =
            KbcArtifact::from_verified(bytecode, Default::default(), &Default::default()).unwrap();
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap()
    };
    let first = prepare("pub fn main() -> i32 { 1 }");
    let second = prepare("pub fn main() -> i32 { 2 }");
    let incompatible = prepare("pub fn main() -> bool { true }");
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let old = runtime.load_program(&first, Default::default()).unwrap();
    let independent = runtime.load_program(&first, Default::default()).unwrap();
    let root = old.root();
    let retained = runtime.function::<(), i32>(&root, "main").unwrap();
    assert_ne!(
        runtime.runtime().select_program(&old).unwrap().id,
        runtime.runtime().select_program(&independent).unwrap().id
    );
    runtime
        .reload_program(&old, &second, Default::default())
        .unwrap();
    let current = old.clone();
    let selected = runtime.runtime().select_program(&current).unwrap();
    assert!(second.bytecode().same_version(selected.verified_program()));
    let error = runtime
        .reload_program(&current, &incompatible, Default::default())
        .unwrap_err();
    assert!(
        matches!(&error, EmbeddingError::ReloadValidation { code, .. }
            if *code == "KG_RELOAD_PUBLIC_EXPORT_MISMATCH"),
        "{error:?}"
    );
    assert_eq!(
        runtime.runtime().select_program(&current).unwrap().key(),
        selected.key()
    );
    runtime
        .reload_program(&old, &second, Default::default())
        .unwrap();
    assert_eq!(runtime.call(&retained, ()).unwrap(), 2);
    let rebound = runtime.function::<(), i32>(&root, "main").unwrap();
    assert_eq!(runtime.call(&rebound, ()).unwrap(), 2);
    let separate = runtime
        .function::<(), i32>(&independent.root(), "main")
        .unwrap();
    assert_eq!(runtime.call(&separate, ()).unwrap(), 1);
    assert_eq!(
        runtime
            .execute(&old, ProgramEntry::Main, &[], &context)
            .unwrap()
            .return_value
            .value(runtime.runtime().gc())
            .expect("retained execution result"),
        Value::I32(2)
    );
    assert_eq!(
        runtime
            .execute(&current, ProgramEntry::Main, &[], &context)
            .unwrap()
            .return_value
            .value(runtime.runtime().gc())
            .expect("retained execution result"),
        Value::I32(2)
    );
}
