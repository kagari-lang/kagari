//! Source-side consumer of the same application-owned registration fixture.
use super::{artifact, assert_provider_released, engine};
use crate::{
    context::ExecutionContext, engine::EngineConfig, program::Program,
    runtime::tests::embedding::generic_reload::prepare_static_program,
};
use kagari_bytecode::artifact::KbcArtifact;
use kagari_runtime::module::entry::ProgramEntry;
use kagari_runtime::reload::ReloadValidationError;
use rstest::rstest;

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
    let old = runtime
        .load_program(first.program(), Default::default())
        .unwrap();
    let mut call = |module, entry| {
        runtime
            .execute(module, entry, &[], &Default::default())
            .unwrap()
            .return_value
            .as_i32()
            .unwrap()
    };
    assert_eq!(call(&old, "read"), 13);
    runtime
        .program(&old)
        .map_err(ReloadValidationError::Runtime)
        .and_then(|base| base.prepare_replacement(second.program(), &Default::default()))
        .and_then(|candidate| runtime.replace_program(&old, &candidate, Default::default()))
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
                .as_i32()
                .unwrap(),
            expected,
            "{entry}/decoded={decoded}"
        );
    }
}

#[test]
fn an_embedding_owned_provider_can_allocate_and_chain_checked_callbacks() {
    let artifact = artifact(
        "fn main() -> i32 { val values = native::from_fn(2u64, |i| { [21] }); values[0u64][0u64] + values[1u64][0u64] }",
    );
    let bytes = artifact.records().to_bytes(artifact.definitions()).unwrap();
    let program = Program::from_artifact(
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
    runtime.collect_garbage().unwrap();
    let baseline = runtime.gc_stats();
    assert_eq!(
        runtime
            .execute(&loaded, ProgramEntry::Main, &[], &context)
            .unwrap()
            .return_value
            .as_i32()
            .unwrap(),
        42
    );
    assert_eq!(runtime.resources().counters().current_call_depth, 0);
    assert_provider_released(&mut runtime, &program, &loaded, baseline);
}
