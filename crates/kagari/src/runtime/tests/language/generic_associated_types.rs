use crate::{
    BytecodeArtifact,
    context::{ExecutionContext, JitPolicy},
    engine::KagariEngine,
    program::PreparedProgram,
};
use kagari_codegen_cranelift::CraneliftBackend;
use kagari_runtime::{module::entry::ProgramEntry, value::Value};
use kagari_source::source::SourceFile;
use rstest::rstest;
use std::{fs, path::PathBuf};

mod admission;
mod diagnostics;
mod linking;

#[rstest]
fn ordinary_execution(
    #[base_dir = "tests/fixtures/language/generic_associated_types"]
    #[files("*.kgr")]
    path: PathBuf,
) {
    let source = fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "failed to read generic associated types fixture {}: {error}",
            path.display()
        )
    });
    let engine = KagariEngine::default();
    let artifact = engine
        .compile_to_artifact(SourceFile::new("families.kgr", &source), Default::default())
        .unwrap();
    for (encoded, jit) in [(false, false), (true, false), (true, true)] {
        let artifact = if encoded {
            BytecodeArtifact::from_bytes(
                &artifact.records().to_bytes(artifact.definitions()).unwrap(),
            )
            .unwrap()
        } else {
            artifact.clone()
        };
        let context = ExecutionContext {
            jit_policy: if jit {
                JitPolicy::Enabled
            } else {
                JitPolicy::Disabled
            },
            ..Default::default()
        };
        let mut runtime = engine.runtime(context.clone());
        let loaded_program =
            PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap();
        let loaded = runtime
            .load_program(&loaded_program, Default::default())
            .unwrap();
        let result = if jit {
            let mut backend = CraneliftBackend::for_host().unwrap();
            let prepared = runtime
                .prepare_native(
                    &loaded_program,
                    &loaded,
                    ProgramEntry::Main,
                    &mut backend,
                    &context.cancellation,
                )
                .unwrap();
            runtime.execute_prepared(&loaded, ProgramEntry::Main, &[], &context, &prepared)
        } else {
            runtime.execute(&loaded, ProgramEntry::Main, &[], &context)
        }
        .unwrap();
        assert_eq!(
            result
                .return_value
                .inspect(runtime.runtime(), |view| view.as_i32().map(Value::I32))
                .expect("retained execution result"),
            Value::I32(42)
        );
    }
}
