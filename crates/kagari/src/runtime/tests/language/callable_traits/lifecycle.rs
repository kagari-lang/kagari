use crate::{context::ExecutionContext, engine::KagariEngine, program::Program};
use kagari_runtime::module::entry::ProgramEntry;
use kagari_source::source::SourceFile;
use kagari_stdlib as stdlib;

#[test]
fn callable_traps_release_roots_and_allow_subsequent_execution() {
    let engine =
        KagariEngine::with_native_modules(Default::default(), stdlib::modules().unwrap()).unwrap();
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
        Program::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let loaded = runtime
        .load_program(&loaded_program, Default::default())
        .unwrap();
    runtime.collect_garbage().unwrap();
    let roots = runtime.gc_stats().root_registrations;
    assert!(
        runtime
            .execute(&loaded, ProgramEntry::Main, &[], &context)
            .is_err()
    );
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().root_registrations, roots);
    assert_eq!(
        runtime
            .execute(&loaded, "healthy", &[], &context)
            .unwrap()
            .return_value
            .as_i32()
            .unwrap(),
        42
    );
}
