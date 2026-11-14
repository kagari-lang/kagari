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

mod diagnostics;
mod evaluation;
mod lifecycle;

fn execute_entries(source: &str, entries: &[(ProgramEntry<'_>, i32)]) {
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    let engine = KagariEngine::with_native_modules(config, stdlib::modules().unwrap()).unwrap();
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
            Compilation::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
        let loaded = runtime
            .load_program(prepared.program(), Default::default())
            .unwrap();
        runtime.collect_garbage().unwrap();
        let roots = runtime.gc_stats().root_registrations;
        for &(entry, expected) in entries {
            let result = if jit {
                let mut backend = CraneliftBackend::for_host().unwrap();
                let native = {
                    let native_code = NativeCompiler::default().prepare(
                        &prepared,
                        entry,
                        &mut backend,
                        &context.cancellation,
                    );
                    native_code.and_then(|native_code| (native_code).install(&mut runtime, &loaded))
                }
                .unwrap();
                runtime.execute_prepared(&loaded, entry, &[], &context, &native)
            } else {
                runtime.execute(&loaded, entry, &[], &context)
            }
            .unwrap();
            assert_eq!(
                result.return_value.as_i32().unwrap(),
                expected,
                "{entry:?}, encoded={encoded}, jit={jit}"
            );
            drop(result);
            assert_eq!(runtime.resources().counters().current_call_depth, 0);
            runtime.collect_garbage().unwrap();
            assert_eq!(runtime.gc_stats().root_registrations, roots);
        }
    }
}

/* Temporarily paused: stdlib restoration (NC01-NC06).
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
*/
