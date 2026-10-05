//! Source-side consumer of the same application-owned registration fixture.
use super::{artifact, engine, provider};
use kagari_bytecode::artifact::KbcArtifact;
use kagari_embed::{
    context::ExecutionContext,
    engine::{EngineConfig, KagariEngine},
    program::PreparedProgram,
};
use kagari_runtime::value::Value;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[test]
fn retained_host_lease_does_not_keep_native_payload_alive_after_runtime_teardown() {
    let program = PreparedProgram::from_artifact(
        artifact("fn main() -> native::Handler<i32> { native::hold(42, |value| value) }"),
        &Default::default(),
        &Default::default(),
    )
    .unwrap();
    let drops = Arc::new(AtomicUsize::new(0));
    let mut builder = KagariEngine::builder().unwrap();
    builder.install(provider::module(drops.clone())).unwrap();
    let engine = builder.build().unwrap();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    let result = runtime.execute(&loaded, "main", &[], &context).unwrap();
    let root = runtime.runtime().root_value(result.return_value).unwrap();
    runtime.runtime().collect_garbage().unwrap();
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    drop(runtime);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    let other = engine.runtime(context);
    assert!(root.value(other.runtime().gc()).is_none());
    assert!(root.set(other.runtime().gc(), Value::Unit).is_none());
    drop(root);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
}

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
