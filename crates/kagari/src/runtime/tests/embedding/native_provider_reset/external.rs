//! Source-side consumer of the same application-owned registration fixture.
use super::{artifact, engine, provider};
use crate::{
    context::ExecutionContext,
    engine::{EngineConfig, KagariEngine},
    program::PreparedProgram,
    runtime::tests::embedding::generic_reload::prepare_static_program,
};
use kagari_bytecode::artifact::KbcArtifact;
use kagari_runtime::module::entry::ProgramEntry;
use kagari_runtime::value::Value;
use rstest::rstest;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[rstest]
#[case::source(false)]
#[case::decoded_artifact(true)]
fn persistent_static_native_payload_retains_its_callback_and_shared_state(#[case] decoded: bool) {
    let source = r#"
use external::fixture as native;
static var STATE: i32 = 1;
static val HANDLE: native::Handler<i32> = native::hold(2, |value| value + version() + STATE);
fn version() -> i32 { 10 }
pub fn read() -> i32 { HANDLE.apply() }
pub fn fresh() -> i32 { native::hold(2, |value| value + version() + STATE).apply() }
pub fn update() { STATE = 5; }
"#;
    let engine = engine(Default::default());
    let first = prepare_static_program(&engine, source, decoded);
    let second = prepare_static_program(
        &engine,
        &source.replace("fn version() -> i32 { 10 }", "fn version() -> i32 { 20 }"),
        decoded,
    );
    let mut runtime = engine.runtime(Default::default());
    let old = runtime.load_program(&first, Default::default()).unwrap();
    let call = |module, entry| {
        runtime
            .execute(module, entry, &[], &Default::default())
            .unwrap()
            .return_value
            .inspect(runtime.runtime(), |view| view.as_i32().map(Value::I32))
            .unwrap()
    };
    assert_eq!(call(&old, "read"), Value::I32(13));
    runtime
        .reload_program(&old, &second, Default::default())
        .unwrap();
    let current = old.clone();
    runtime
        .execute(&current, "update", &[], &Default::default())
        .unwrap();
    for (entry, expected) in [("read", 27), ("fresh", 27)] {
        assert_eq!(
            runtime
                .execute(&current, entry, &[], &Default::default())
                .unwrap()
                .return_value
                .inspect(runtime.runtime(), |view| view.as_i32().map(Value::I32))
                .unwrap(),
            Value::I32(expected),
            "{entry}/decoded={decoded}"
        );
    }
}

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
    let result = runtime
        .execute(&loaded, ProgramEntry::Main, &[], &context)
        .unwrap();
    let root = unsafe {
        runtime.runtime().gc().root_value_transport(
            result
                .return_value
                .value_transport(runtime.runtime().gc())
                .expect("retained execution result"),
        )
    }
    .unwrap();
    runtime.runtime().collect_garbage().unwrap();
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    drop(runtime);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    let other = engine.runtime(context);
    assert!(root.inspect(other.runtime(), |_| Ok(())).is_err());
    assert!(
        root.replace(
            other.runtime().gc(),
            &unsafe { other.runtime().gc().root_value_transport(Value::Unit) }.unwrap()
        )
        .is_none()
    );
    drop(root);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
}

#[test]
fn an_embedding_owned_provider_can_allocate_and_chain_checked_callbacks() {
    let artifact = artifact(
        "fn main() -> i32 { val values = native::from_fn(2u64, |i| { [21] }); values[0u64][0u64] + values[1u64][0u64] }",
    );
    let bytes = artifact.records().to_bytes(artifact.definitions()).unwrap();
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
            .execute(&loaded, ProgramEntry::Main, &[], &context)
            .unwrap()
            .return_value
            .inspect(runtime.runtime(), |view| view.as_i32().map(Value::I32))
            .expect("retained execution result"),
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
