use crate::{context::ExecutionContext, engine::KagariEngine, program::PreparedProgram};
use kagari_runtime::{module::entry::ProgramEntry, value::Value};
use kagari_source::source::SourceFile;

#[test]
fn callable_traps_release_roots_and_allow_subsequent_execution() {
    let engine = KagariEngine::default();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "callable-trap.kgr",
                r#"
use std::collections;
struct Failure {}
impl Fn<(i32,)> for Failure { type Output=i32;
 fn call(self, args:(i32,))->i32 { val zero = 0; args[0] / zero }
}
fn main(){ val mapped = Vec::from([1]).map(Failure{}); mapped.next(); }
pub fn healthy()->i32{42}
"#,
            ),
            Default::default(),
        )
        .unwrap();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded_program =
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let loaded = runtime
        .load_program(&loaded_program, Default::default())
        .unwrap();
    assert!(
        runtime
            .execute(&loaded, ProgramEntry::Main, &[], &context)
            .is_err()
    );
    assert_eq!(runtime.runtime().gc().active_roots(), 0);
    assert_eq!(
        runtime
            .execute(&loaded, "healthy", &[], &context)
            .unwrap()
            .return_value
            .inspect(runtime.runtime(), |view| view.as_i32().map(Value::I32))
            .expect("retained execution result"),
        Value::I32(42)
    );
}
