use crate::{
    BytecodeArtifact,
    context::{ExecutionContext, JitPolicy},
    engine::{EngineConfig, KagariEngine},
    program::PreparedProgram,
};
use kagari_codegen_cranelift::CraneliftBackend;
use kagari_runtime::{module::entry::ProgramEntry, value::Value};
use kagari_source::source::SourceFile;
use rstest::rstest;
use std::{fs, path::PathBuf};

mod diagnostics;
mod evaluation;
mod lifecycle;

fn execute_entries(source: &str, entries: &[(ProgramEntry<'_>, i32)]) {
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    let engine = KagariEngine::new(config);
    let artifact = engine
        .compile_to_artifact(SourceFile::new("never.kgr", source), Default::default())
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
        let prepared =
            PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap();
        let loaded = runtime.load_program(&prepared, Default::default()).unwrap();
        for &(entry, expected) in entries {
            let result = if jit {
                let mut backend = CraneliftBackend::for_host().unwrap();
                let native = runtime
                    .prepare_native(
                        &prepared,
                        &loaded,
                        entry,
                        &mut backend,
                        &context.cancellation,
                    )
                    .unwrap();
                runtime.execute_prepared(&loaded, entry, &[], &context, &native)
            } else {
                runtime.execute(&loaded, entry, &[], &context)
            }
            .unwrap();
            assert_eq!(
                result
                    .return_value
                    .value(runtime.runtime().gc())
                    .expect("retained execution result"),
                Value::I32(expected),
                "{entry:?}, encoded={encoded}, jit={jit}"
            );
            drop(result);
            assert_eq!(runtime.runtime().gc().active_roots(), 0);
            assert_eq!(
                runtime.runtime().resources().counters().current_call_depth,
                0
            );
        }
    }
}

#[rstest]
fn ordinary_execution(
    #[base_dir = "tests/fixtures/language/never"]
    #[files("*.kgr")]
    path: PathBuf,
) {
    let source = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("failed to read never fixture {}: {error}", path.display()));
    execute_entries(&source, &[(ProgramEntry::Main, 42)]);
}
