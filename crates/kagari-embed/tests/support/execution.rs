//! Shared preparation for ordinary source/SDK contracts. Registration and
//! invalidation tests should build their own engine instead of using this helper.
use kagari_codegen_cranelift::CraneliftBackend;
use kagari_embed::{
    BytecodeArtifact,
    context::{ExecutionContext, JitPolicy},
    engine::{EngineConfig, KagariEngine},
    program::PreparedProgram,
};
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
    artifact: BytecodeArtifact,
    routes: &[Route],
    expected: i32,
) {
    let decoded = BytecodeArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
    let direct =
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let decoded =
        PreparedProgram::from_artifact(decoded, &Default::default(), &Default::default()).unwrap();
    for route in routes {
        let program = match route {
            Route::Source => &direct,
            Route::Artifact | Route::ArtifactJit => &decoded,
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
                    "main",
                    &mut backend,
                    &context.cancellation,
                )
                .unwrap();
            runtime.execute_prepared(&loaded, "main", &[], &context, &prepared)
        } else {
            runtime.execute(&loaded, "main", &[], &context)
        }
        .unwrap();
        assert_eq!(
            result
                .return_value
                .value(runtime.runtime().gc())
                .expect("retained execution result"),
            Value::I32(expected),
            "{route:?}"
        );
    }
}
