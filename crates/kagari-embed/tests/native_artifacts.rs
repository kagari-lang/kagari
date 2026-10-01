use kagari_bytecode::{
    artifact::{ArtifactBuildOptions, KbcArtifact},
    native_input::PortableMir,
};
use kagari_common::source::SourceFile;
use kagari_compiler::native_input::verify_native_input;
use kagari_embed::{
    context::ExecutionContext,
    engine::{
        KagariEngine,
        source::{ArtifactOptions, NativeInputExport},
    },
    error::EmbeddingError,
    program::PreparedProgram,
};
use kagari_runtime::value::Value;

#[test]
fn source_exports_matching_native_input_or_explicit_bytecode_only_artifacts() {
    let engine = KagariEngine::default();
    let checked = engine
        .compile_source(
            SourceFile::new("native-export", "fn main() -> i32 { 42 }"),
            Default::default(),
        )
        .unwrap();
    for native_input in [
        NativeInputExport::PortableMir,
        NativeInputExport::BytecodeOnly,
    ] {
        let artifact = engine
            .emit_bytecode(
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
        let artifact = KbcArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
        artifact.validate_for_loader(&Default::default()).unwrap();
        match native_input {
            NativeInputExport::PortableMir => {
                verify_native_input(
                    &artifact.portable_mir.as_ref().unwrap().bytes,
                    &artifact.program,
                    &Default::default(),
                )
                .unwrap();
            }
            NativeInputExport::BytecodeOnly => assert!(artifact.portable_mir.is_none()),
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
                .execute(&loaded, "main", &[], &context)
                .unwrap()
                .return_value,
            Value::I32(42)
        );
    }
}

#[test]
fn prepared_reload_preserves_abi_validation_and_the_previous_version_on_failure() {
    let engine = KagariEngine::default();
    let prepare = |source| {
        let artifact = engine
            .compile_to_artifact(
                SourceFile::new("reload", source),
                Default::default(),
                Default::default(),
            )
            .unwrap();
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap()
    };
    let first = prepare("pub fn main() -> i32 { 1 }");
    let second = prepare("pub fn main() -> i32 { 2 }");
    let incompatible = prepare("pub fn main() -> bool { true }");
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let old = runtime.load_program(&first, Default::default()).unwrap();
    let current = runtime
        .reload_program(&old, &second, Default::default())
        .unwrap();
    assert!(second.bytecode().same_version(current.verified_program()));
    assert!(matches!(
        runtime.reload_program(&current, &incompatible, Default::default()),
        Err(EmbeddingError::ReloadValidation { code, .. }) if code == "KG_RELOAD_PUBLIC_ABI_FINGERPRINT_MISMATCH"
    ));
    assert_eq!(
        runtime
            .runtime()
            .modules()
            .latest(&current.name)
            .unwrap()
            .key(),
        current.key()
    );
    assert!(matches!(
        runtime.reload_program(&old, &second, Default::default()),
        Err(EmbeddingError::ReloadValidation { code, .. }) if code == "KG_RELOAD_MODULE_NOT_ACTIVE"
    ));
    assert_eq!(
        runtime
            .execute(&old, "main", &[], &context)
            .unwrap()
            .return_value,
        Value::I32(1)
    );
    assert_eq!(
        runtime
            .execute(&current, "main", &[], &context)
            .unwrap()
            .return_value,
        Value::I32(2)
    );
}
