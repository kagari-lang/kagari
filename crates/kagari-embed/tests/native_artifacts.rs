use kagari_bytecode::native_input::PortableMir;
use kagari_bytecode::{ArtifactBuildOptions, KbcArtifact};
use kagari_common::SourceFile;
use kagari_compiler::native_input::verify_native_input;
use kagari_embed::program::PreparedProgram;
use kagari_embed::{ArtifactOptions, ExecutionContext, KagariEngine, NativeInputExport};
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
