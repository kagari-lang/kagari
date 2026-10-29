//! Shared preparation for ordinary source/SDK contracts. Registration and
//! invalidation tests should build their own engine instead of using this helper.
use crate::{
    BytecodeArtifact,
    context::{ExecutionContext, JitPolicy},
    engine::{EngineConfig, KagariEngine, source::CheckedModule},
    program::PreparedProgram,
};
use kagari_codegen_cranelift::CraneliftBackend;
use kagari_common::identity::metadata::DefinitionMetadata;
use kagari_runtime::module::entry::ProgramEntry;
use kagari_runtime::value::Value;
use std::sync::{Mutex, OnceLock};

pub fn engine(config: EngineConfig) -> KagariEngine {
    static BASELINE: OnceLock<Mutex<KagariEngine>> = OnceLock::new();
    // AnalysisDatabase is Send, not Sync. The lock only protects cloning the
    // native-only baseline; no source analysis or execution runs under it.
    BASELINE
        .get_or_init(|| Mutex::new(KagariEngine::default()))
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
            PreparedProgram::from_artifact(decoded, &Default::default(), &Default::default())
                .unwrap()
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
        PreparedProgram::from_artifact(decoded, &Default::default(), &Default::default()).unwrap();
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
    direct: Option<&PreparedProgram>,
    encoded: Option<&PreparedProgram>,
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
        let loaded = runtime.load_program(program, Default::default()).unwrap();
        let result = if jit {
            let mut backend = CraneliftBackend::for_host().unwrap();
            let prepared = runtime
                .prepare_native(
                    program,
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
            Value::I32(expected),
            "{route:?}"
        );
        drop(result);
        assert_eq!(runtime.runtime().gc().active_roots(), 0, "{route:?}");
    }
}
