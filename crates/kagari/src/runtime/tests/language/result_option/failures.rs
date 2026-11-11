use crate::{
    context::ExecutionContext,
    engine::{EngineConfig, KagariEngine},
    program::Program,
    test_support::cancellation as support,
};
use kagari_runtime::{module::entry::ProgramEntry, value::Value};
use kagari_source::source::SourceFile;
use kagari_stdlib as stdlib;

#[test]
fn traps_inside_error_conversion_release_resources() {
    let source = r#"
struct Source { var calls: i32 }
struct Target {}
impl From<Source> for Target {
    fn from(error: Source) -> Self {
        error.calls += 1;
        val zero=0; 1/zero;
        Target {}
    }
}
fn fail(error: Source) -> Result<i32, Target> {
    val value: Result<i32, Source> = Err(error);
    Ok(value?)
}
fn main() -> Result<i32, Target> { fail(Source { calls: 0 }) }
pub fn after() -> i32 { 42 }
"#;
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    let engine = KagariEngine::with_native_modules(config, stdlib::modules().unwrap()).unwrap();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("conversion-trap.kgr", source),
            Default::default(),
        )
        .unwrap();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded_program =
        Program::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let loaded = runtime
        .load_program(&loaded_program, Default::default())
        .unwrap();
    let error = runtime
        .execute(&loaded, ProgramEntry::Main, &[], &context)
        .unwrap_err();
    assert!(
        format!("{error:?}").contains("division by zero"),
        "{error:?}"
    );
    assert!(
        error
            .trace()
            .unwrap()
            .frames
            .iter()
            .any(|frame| frame.function_name.contains("from"))
    );
    assert_eq!(runtime.gc().active_roots(), 0);
    runtime.collect_garbage().unwrap();
    assert_eq!(
        runtime
            .execute(&loaded, "after", &[], &context)
            .unwrap()
            .return_value
            .value(runtime.gc())
            .expect("retained execution result"),
        Value::I32(42)
    );
}

#[test]
fn propagation_does_not_catch_traps_or_termination_and_releases_frames() {
    let engine =
        KagariEngine::with_native_modules(Default::default(), stdlib::modules().unwrap()).unwrap();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "cleanup.kgr",
                r#"
fn fail()->Result<i32, String> { val zero = 0; Ok(42 / zero) }
fn main()->Result<i32, String> { for x in [1, 2] { fail()?; } Ok(42) }
pub fn after()->i32 { 42 }
"#,
            ),
            Default::default(),
        )
        .unwrap();
    let mut runtime = engine.runtime(ExecutionContext::default());
    let loaded_program =
        Program::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let loaded = runtime
        .load_program(&loaded_program, Default::default())
        .unwrap();
    let error = runtime
        .execute(
            &loaded,
            ProgramEntry::Main,
            &[],
            &ExecutionContext::default(),
        )
        .unwrap_err();
    assert!(format!("{error:?}").contains("division by zero"));
    assert_eq!(runtime.gc().active_roots(), 0);
    let limited = ExecutionContext::default();
    let cancellation =
        support::cancel_after(&runtime, &runtime.select_program(&loaded).unwrap(), 1);

    assert_eq!(
        runtime
            .execute(&loaded, ProgramEntry::Main, &[], &limited)
            .unwrap_err()
            .code(),
        "KG_RUNTIME_CANCELLED"
    );
    assert_eq!(runtime.gc().active_roots(), 0);
    drop(cancellation);
    let cancelled = ExecutionContext::default();
    cancelled.cancellation.cancel();
    assert_eq!(
        runtime
            .execute(&loaded, ProgramEntry::Main, &[], &cancelled)
            .unwrap_err()
            .code(),
        "KG_RUNTIME_CANCELLED"
    );
    assert_eq!(runtime.gc().active_roots(), 0);
    assert_eq!(
        runtime
            .execute(&loaded, "after", &[], &ExecutionContext::default())
            .unwrap()
            .return_value
            .value(runtime.gc())
            .expect("retained execution result"),
        Value::I32(42)
    );
    assert!(!runtime.is_quarantined());
}
