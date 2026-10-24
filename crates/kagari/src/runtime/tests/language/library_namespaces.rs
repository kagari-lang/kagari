use crate::{
    BytecodeArtifact, context::ExecutionContext, engine::KagariEngine, program::PreparedProgram,
};
use kagari_runtime::{module::entry::ProgramEntry, value::Value};
use kagari_source::source::SourceFile;

mod prelude;

#[test]
fn reexports_preserve_trait_type_and_constructor_identity_in_artifacts() {
    let engine = KagariEngine::new(Default::default());
    let source = include_str!(
        "../../../../tests/fixtures/language/library_namespaces/reexports_preserve_trait_type_and_constructor_identity_in_artifacts.kgr"
    );
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("namespaces.kgr", source),
            Default::default(),
        )
        .unwrap();
    let artifact =
        BytecodeArtifact::from_bytes(&artifact.records().to_bytes(artifact.definitions()).unwrap())
            .unwrap();
    let program =
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
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
