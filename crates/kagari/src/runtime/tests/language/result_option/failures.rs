use crate::{
    context::ExecutionContext,
    engine::{EngineConfig, KagariEngine},
    program::Program,
};
use kagari_runtime::{error::reason::ErrorReason, module::entry::ProgramEntry};
use kagari_source::source::SourceFile;
use kagari_stdlib as stdlib;
use kagari_types::{arithmetic::ArithmeticError, scalar::error::ScalarError};

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
    runtime.collect_garbage().unwrap();
    let roots = runtime.gc_stats().root_registrations;
    let error = runtime
        .execute(&loaded, ProgramEntry::Main, &[], &context)
        .unwrap_err();
    assert_eq!(
        error.reason(),
        &ErrorReason::Scalar(ScalarError::Arithmetic(ArithmeticError::DivisionByZero))
    );
    assert!(
        error
            .trace()
            .unwrap()
            .frames
            .iter()
            .any(|frame| frame.function_name.contains("from"))
    );
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().root_registrations, roots);
    runtime.collect_garbage().unwrap();
    assert_eq!(
        runtime
            .execute(&loaded, "after", &[], &context)
            .unwrap()
            .return_value
            .as_i32()
            .unwrap(),
        42
    );
}

/* Temporarily paused: stdlib restoration (NC01-NC06).
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
    runtime.collect_garbage().unwrap();
    let roots = runtime.gc_stats().root_registrations;
    let error = runtime
        .execute(
            &loaded,
            ProgramEntry::Main,
            &[],
            &ExecutionContext::default(),
        )
        .unwrap_err();
    assert!(format!("{error:?}").contains("division by zero"));
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().root_registrations, roots);
    let limited = ExecutionContext::default();
    let selected = runtime.select_program(&loaded).unwrap();
    let mut cancellation = support::cancel_after(&mut runtime, &selected, 1);

    assert_eq!(
        cancellation
            .runtime_mut()
            .execute(&loaded, ProgramEntry::Main, &[], &limited)
            .unwrap_err()
            .code(),
        "KG_RUNTIME_CANCELLED"
    );
    cancellation.runtime_mut().collect_garbage().unwrap();
    assert_eq!(cancellation.runtime().gc_stats().root_registrations, roots);
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
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().root_registrations, roots);
    assert_eq!(
        runtime
            .execute(&loaded, "after", &[], &ExecutionContext::default())
            .unwrap()
            .return_value
            .as_i32()
            .unwrap(),
        42
    );
    assert!(!runtime.is_quarantined());
}
*/
