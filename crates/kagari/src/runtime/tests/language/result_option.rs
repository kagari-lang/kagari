#[cfg(feature = "native")]
use crate::program::native::NativeCompiler;
use crate::{
    BytecodeArtifact,
    context::{ExecutionContext, JitPolicy},
    engine::{EngineConfig, KagariEngine},
    program::Compilation,
};
use kagari_codegen_cranelift::CraneliftBackend;
use kagari_runtime::module::entry::ProgramEntry;
use kagari_source::source::SourceFile;
use kagari_stdlib as stdlib;
use rstest::rstest;
use std::{fs, path::PathBuf};

mod admission;
mod failures;
mod rejection;

#[rstest]
fn ordinary_execution(
    #[base_dir = "tests/fixtures/language/result_option"]
    #[files("ordinary/*.kgr")]
    path: PathBuf,
) {
    let source = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("failed to read fixture {}: {error}", path.display()));
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    let engine = KagariEngine::with_native_modules(config, stdlib::modules().unwrap()).unwrap();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("result-option.kgr", &source),
            Default::default(),
        )
        .unwrap();
    let decoded =
        BytecodeArtifact::from_bytes(&artifact.records().to_bytes(artifact.definitions()).unwrap())
            .unwrap();
    let direct =
        Compilation::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let decoded =
        Compilation::from_artifact(decoded, &Default::default(), &Default::default()).unwrap();
    for (loaded_program, jit) in [(&direct, false), (&decoded, false), (&decoded, true)] {
        let context = ExecutionContext {
            jit_policy: if jit {
                JitPolicy::Enabled
            } else {
                JitPolicy::Disabled
            },
            ..Default::default()
        };
        let mut runtime = engine.runtime(context.clone());
        let loaded = runtime
            .load_program(loaded_program.program(), Default::default())
            .unwrap();
        runtime.collect_garbage().unwrap();
        let roots = runtime.gc_stats().root_registrations;
        let result = if jit {
            let mut backend = CraneliftBackend::for_host().unwrap();
            let prepared = NativeCompiler::default()
                .prepare(
                    loaded_program,
                    ProgramEntry::Main,
                    &mut backend,
                    &context.cancellation,
                )
                .and_then(|native_code| (native_code).install(&mut runtime, &loaded))
                .unwrap();
            runtime.execute_prepared(&loaded, ProgramEntry::Main, &[], &context, &prepared)
        } else {
            runtime.execute(&loaded, ProgramEntry::Main, &[], &context)
        }
        .unwrap();
        assert_eq!(result.return_value.as_i32().unwrap(), 42);
        drop(result);
        assert!(runtime.gc_stats().completed_cycles > 0);
        runtime.collect_garbage().unwrap();
        assert_eq!(runtime.gc_stats().root_registrations, roots);
    }
}
