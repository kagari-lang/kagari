//! Local callables through encoded, source-free execution.

use crate::{
    BytecodeArtifact, context::ExecutionContext, engine::KagariEngine, program::PreparedProgram,
    runtime::owned::DriveResult,
};
use kagari_runtime::{module::entry::ProgramEntry, value::Value};
use kagari_source::source::SourceFile;
use std::num::NonZeroUsize;

#[test]
fn local_functions_execute_from_artifacts_without_export_leakage() {
    let engine = KagariEngine::default();
    let artifact = engine.compile_to_artifact(SourceFile::new("local-functions.kgr", r#"
pub fn main() -> i32 {
    val result = inner(41);
    fn inner(x: i32) -> i32 { x + 1 }
    result
}
pub fn mutual() -> i32 {
    fn even(n: i32) -> bool { if n == 0 { true } else { odd(n - 1) } }
    fn odd(n: i32) -> bool { if n == 0 { false } else { even(n - 1) } }
    if even(12) { 42 } else { 0 }
}
pub fn generic() -> i32 {
    fn identity<T>(x: T) -> T { x }
    val selected: fn(i32) -> i32 = identity;
    val explicit = identity::<i32>;
    selected(explicit(identity(42)))
}
pub fn shadowing() -> i32 {
    fn inner() -> i32 { 20 }
    val left = inner();
    val right = { fn inner() -> i32 { 22 } inner };
    left + right()
}
pub fn closure() -> i32 {
    fn make(x: i32) -> fn() -> i32 { || x }
    val callback = make(42);
    callback()
}
trait Helper { fn run(self) -> i32 { fn identity<T>(x: T) -> T { x } val f: fn(i32) -> i32 = identity; f(42) } }
impl Helper for i32 {}
pub fn method() -> i32 { val helper: Helper = 0; helper.run() }
pub async fn asynchronous() -> i32 {
    async fn inner(x: i32) -> i32 { x + 1 }
    val selected: fn(i32) -> Future<i32> = inner;
    selected(41).await
}
"#), Default::default()).unwrap();
    let artifact =
        BytecodeArtifact::from_bytes(&artifact.records().to_bytes(artifact.definitions()).unwrap())
            .unwrap();
    let program =
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    for entry in [
        "main",
        "mutual",
        "generic",
        "shadowing",
        "closure",
        "method",
    ] {
        let result = runtime.execute(&loaded, entry, &[], &context).unwrap();
        assert_eq!(
            result.return_value.value(runtime.runtime().gc()).unwrap(),
            Value::I32(42),
            "{entry}"
        );
    }
    assert!(loaded.exported_function("inner").is_none());
    assert!(loaded.exported_function("main::inner").is_none());
    assert!(loaded.exported_function("identity").is_none());
    assert!(
        runtime
            .execute(&loaded, ProgramEntry::Main, &[], &context)
            .is_ok()
    );
    let cold = runtime
        .execute(&loaded, "asynchronous", &[], &context)
        .unwrap()
        .return_value;
    let execution = runtime
        .start_future(&cold.value(runtime.runtime().gc()).unwrap(), &context)
        .unwrap();
    let result = loop {
        match runtime
            .drive(&execution, NonZeroUsize::new(64).unwrap())
            .unwrap()
        {
            DriveResult::Runnable => continue,
            DriveResult::Waiting => panic!("local async functions have no external wait"),
            DriveResult::Complete(result) => break result.unwrap(),
        }
    };
    assert_eq!(
        result.value(runtime.runtime().gc()).unwrap(),
        Value::I32(42)
    );
}
