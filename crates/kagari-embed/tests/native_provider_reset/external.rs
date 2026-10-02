//! Source-side consumer of the same application-owned registration fixture.
use super::{artifact, engine};
use kagari_bytecode::artifact::KbcArtifact;
use kagari_embed::{context::ExecutionContext, engine::EngineConfig, program::PreparedProgram};
use kagari_runtime::value::Value;

#[test]
fn an_embedding_owned_provider_can_allocate_and_chain_checked_callbacks() {
    let bytes = artifact(
        "fn main() -> i32 { val values = native::from_fn(2usize, |i| { [21] }); values[0usize][0usize] + values[1usize][0usize] }",
    ).to_bytes().unwrap();
    let program = PreparedProgram::from_artifact(
        KbcArtifact::from_bytes(&bytes).unwrap(),
        &Default::default(),
        &Default::default(),
    )
    .unwrap();
    let context = ExecutionContext::default();
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    // A fresh engine installs the same checked module without source compilation.
    let mut runtime = engine(config).runtime(context.clone());
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    assert_eq!(
        runtime
            .execute(&loaded, "main", &[], &context)
            .unwrap()
            .return_value,
        Value::I32(42)
    );
    assert_eq!(runtime.runtime().gc().active_roots(), 0);
    assert_eq!(
        runtime.runtime().resources().counters().current_call_depth,
        0
    );
    runtime.runtime().collect_garbage().unwrap();
    assert_eq!(runtime.runtime().gc().allocated_objects(), 0);
}
