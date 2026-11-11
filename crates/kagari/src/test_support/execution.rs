//! Shared preparation for ordinary source/SDK contracts. Registration and
//! invalidation tests should build their own engine instead of using this helper.
#[cfg(feature = "native")]
use crate::program::native::NativeCompiler;
use crate::{
    BytecodeArtifact,
    context::{ExecutionContext, JitPolicy},
    engine::{EngineConfig, KagariEngine, source::CheckedModule},
    program::Compilation,
};
use kagari_codegen_cranelift::CraneliftBackend;
use kagari_common::identity::metadata::DefinitionMetadata;
use kagari_runtime::module::entry::ProgramEntry;
use kagari_runtime::value::Value;
use kagari_stdlib as stdlib;
use std::sync::{Mutex, OnceLock};

pub fn engine(config: EngineConfig) -> KagariEngine {
    static BASELINE: OnceLock<Mutex<KagariEngine>> = OnceLock::new();
    // AnalysisDatabase is Send, not Sync. The lock only protects cloning the
    // native-only baseline; no source analysis or execution runs under it.
    BASELINE
        .get_or_init(|| {
            Mutex::new(
                KagariEngine::with_native_modules(Default::default(), stdlib::modules().unwrap())
                    .unwrap(),
            )
        })
        .lock()
        .unwrap()
        .new_session(config)
}

#[derive(Clone, Copy, Debug)]
pub enum Route {
    Source,
    Artifact,
    #[allow(dead_code)] // Some consumers intentionally cover only interpreter routes.
    ArtifactJit,
}

pub fn assert_returns_i32(
    engine: &KagariEngine,
    checked: CheckedModule,
    routes: &[Route],
    expected: i32,
) {
    let direct = routes
        .iter()
        .any(|route| matches!(route, Route::Source))
        .then(|| engine.prepare(&checked, Default::default()).unwrap());
    let encoded = routes
        .iter()
        .any(|route| !matches!(route, Route::Source))
        .then(|| {
            let artifact = engine.emit_artifact(&checked, Default::default()).unwrap();
            let decoded = BytecodeArtifact::from_bytes(
                &artifact.records().to_bytes(artifact.definitions()).unwrap(),
            )
            .unwrap();
            Compilation::from_artifact(decoded, &Default::default(), &Default::default()).unwrap()
        });
    assert_prepared_returns_i32(engine, direct.as_ref(), encoded.as_ref(), routes, expected);
}

pub fn assert_artifact_returns_i32(
    engine: &KagariEngine,
    artifact: DefinitionMetadata<BytecodeArtifact>,
    expected: i32,
) {
    let decoded =
        BytecodeArtifact::from_bytes(&artifact.records().to_bytes(artifact.definitions()).unwrap())
            .unwrap();
    let program =
        Compilation::from_artifact(decoded, &Default::default(), &Default::default()).unwrap();
    assert_prepared_returns_i32(
        engine,
        None,
        Some(&program),
        &[Route::Artifact, Route::ArtifactJit],
        expected,
    );
}

fn assert_prepared_returns_i32(
    engine: &KagariEngine,
    direct: Option<&Compilation>,
    encoded: Option<&Compilation>,
    routes: &[Route],
    expected: i32,
) {
    for route in routes {
        let program = match route {
            Route::Source => direct.unwrap(),
            Route::Artifact | Route::ArtifactJit => encoded.unwrap(),
        };
        let jit = matches!(route, Route::ArtifactJit);
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
            .load_program(program.program(), Default::default())
            .unwrap();
        let result = if jit {
            let mut backend = CraneliftBackend::for_host().unwrap();
            let prepared = NativeCompiler::default()
                .prepare(
                    program,
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
        assert_eq!(
            result
                .return_value
                .value(runtime.gc())
                .expect("retained execution result"),
            Value::I32(expected),
            "{route:?}"
        );
        drop(result);
        assert_eq!(runtime.gc().active_roots(), 0, "{route:?}");
    }
}
