//! Standalone consumer: the producer's ordinary KBC is the only input.
use kagari::{BytecodeArtifact, context::ExecutionContext, engine::KagariEngine, program::Program};
use kagari_runtime::module::entry::ProgramEntry;
use std::{env, fs};

#[test]
fn authored_point_compares_without_providers() {
    let path = env::var_os("KAGARI_POINT_ARTIFACT").expect("run check_features.py --core-only");
    let artifact = BytecodeArtifact::from_bytes(&fs::read(path).unwrap()).unwrap();
    let program =
        Program::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let context = ExecutionContext::default();
    let mut runtime = KagariEngine::default().runtime(context.clone());
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    let result = runtime
        .execute(&loaded, ProgramEntry::Main, &[], &context)
        .unwrap();
    // main requires same(x, different label) and !same(different x, same label).
    assert_eq!(result.return_value.as_bool(), Some(true));
}
