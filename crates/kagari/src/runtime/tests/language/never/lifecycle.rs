/* Temporarily paused: stdlib restoration (NC01-NC06).
use crate::{
    BytecodeArtifact, context::ExecutionContext, engine::KagariEngine, program::Program,
    test_support::{cancellation::cancel_after, standard::standard_library},
};
use kagari_runtime::{error::RuntimeErrorKind, host::HostError, module::entry::ProgramEntry};
use kagari_source::source::SourceFile;
use kagari_stdlib::io;
use std::sync::{Arc, Mutex};

#[test]
fn never_preserves_effects_and_releases_resources_on_traps_and_cancellation() {
    let standard = standard_library();
    let catalog = standard.declarations().catalog();
    let modules = standard.modules().unwrap();
    let engine = KagariEngine::with_native_modules(Default::default(), modules.clone()).unwrap();
    let reference = io::module(catalog, |_, _, _| Ok(())).unwrap();
    let compiler = KagariEngine::with_native_modules(
        Default::default(),
        modules.into_iter().chain([reference]).collect(),
    )
    .unwrap();
    let artifact = compiler
        .compile_to_artifact(
            SourceFile::new(
                "never-effects.kgr",
                include_str!("../../../../../tests/fixtures/language/never/never-effects.kgr"),
            ),
            Default::default(),
        )
        .unwrap();
    let effects = Arc::new(Mutex::new(Vec::new()));
    let captured = effects.clone();
    let sink = io::module(catalog, move |cx, message, suffix| {
        let message = cx
            .read_string(message, str::to_owned)
            .map_err(|error| HostError::new(error.to_string()))?;
        captured.lock().unwrap().push((message, suffix));
        Ok(())
    })
    .unwrap();
    for encoded in [false, true] {
        let artifact = if encoded {
            BytecodeArtifact::from_bytes(
                &artifact.records().to_bytes(artifact.definitions()).unwrap(),
            )
            .unwrap()
        } else {
            artifact.clone()
        };
        let context = ExecutionContext::default();

        let mut runtime = engine.runtime(context.clone());
        let prepared =
            Program::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
        let error = runtime.load_program(&prepared, Default::default()).unwrap_err();
        assert_eq!(error.kind(), RuntimeErrorKind::ModuleValidation);
        assert!(error.to_string().contains("native binding is not installed"));
        effects.lock().unwrap().clear();
        sink.install(&mut runtime).unwrap();
        let loaded = runtime.load_program(&prepared, Default::default()).unwrap();
        runtime.collect_garbage().unwrap();
        let roots = runtime.gc_stats().root_registrations;
        let error = runtime
            .execute(&loaded, ProgramEntry::Main, &[], &context)
            .unwrap_err();
        assert_eq!(error.code(), "KG_RUNTIME_SCRIPT_TRAP");
        assert_eq!(*effects.lock().unwrap(), [("before".to_owned(), "")]);
        assert!(runtime.execution_root().is_none());
        runtime.collect_garbage().unwrap();
        assert_eq!(runtime.gc_stats().root_registrations, roots);
        let limited = context.clone();

        let selected = runtime.select_program(&loaded).unwrap();
        let mut cancellation = cancel_after(&mut runtime, &selected, 20);
        let error = cancellation
            .runtime_mut()
            .execute(&loaded, ProgramEntry::Export("forever"), &[], &limited)
            .unwrap_err();
        assert_eq!(error.code(), "KG_RUNTIME_CANCELLED");
        drop(cancellation);
        assert!(runtime.execution_root().is_none());
        runtime.collect_garbage().unwrap();
        assert_eq!(runtime.gc_stats().root_registrations, roots);

        assert_eq!(
            runtime
                .execute(
                    &loaded,
                    ProgramEntry::Export("healthy"),
                    &[],
                    &ExecutionContext::default(),
                )
                .unwrap()
                .return_value
                .as_i32()
                .unwrap(),
            42
        );
    }
}
*/
